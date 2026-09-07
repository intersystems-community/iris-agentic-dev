# Contract: the parameter-rejection response

FR-015 through FR-017 introduce one new response. This is its shape, and its position relative
to the responses that already exist.

## The response

A call naming a parameter the tool does not accept returns, as the text content of an error
result:

```json
{
  "success": false,
  "error_code": "UNKNOWN_PARAMETER",
  "error": "iris_query does not accept \"namesapce\". Accepted parameters: confirm, force, max_rows_affected, mode, namespace, parameters, query, server, table. Did you mean \"namespace\"?"
}
```

Requirements on the message:

- **Names the offending parameter** (FR-015), quoted, exactly as the caller spelled it.
- **Lists the accepted parameters** (FR-015), sorted, read from the tool's own advertised
  properties rather than a hand-maintained list.
- **Suggests the near match when there is one.** Not required by any FR, but it is the
  difference between one retry and three, and SC-008 asks that the error alone be enough to
  correct the call without consulting the docs. A single-token edit distance against the
  accepted names is enough; when nothing is close, omit the sentence rather than guess.
- **Reports all unknown keys**, not just the first, when a call carries several. A caller fixing
  one typo per round-trip is the failure mode this feature exists to remove.

Built with the same helper shape as the gate refusals — `success` / `error_code` / `error` via
`err_result` (`write_gate.rs:955`) — so Principle V holds and a client parses one shape for
every refusal.

## Position in the call sequence

```text
gate check ──refused──→ WRITE_TOOLS_DISABLED / DESTRUCTIVE_TOOLS_DISABLED / DESTRUCTIVE_REQUIRES_WRITES
   │ passed
   ▼
key validation ──unknown key──→ UNKNOWN_PARAMETER
   │ all advertised
   ▼
router deserialization ──wrong type──→ type error
   │ ok
   ▼
handler ──bad value──→ INVALID_PARAMS
handler ──bad `action`──→ INVALID_ACTION
```

**Gate first (FR-016 + Principle VI).** `gate_check` already runs on raw arguments before the
router (`mod.rs:9098`), and stays there. A caller with writes disabled who also misspells a key
on `global_kill` is told the gate refused — the security answer outranks the usability one.
Asserted by test, not assumed.

**Nothing executes before the rejection (FR-016).** Both the gate check and key validation sit in
the `call_tool` override, ahead of `tool_router.call`. The handler is never entered, so a
destructive tool cannot half-apply a mis-specified call.

**Distinguishable from a value rejection (FR-017).** `UNKNOWN_PARAMETER` means "I do not know
that name"; `INVALID_PARAMS` means "I know the name, that value is not permitted". A client
branches on `error_code` without parsing prose.

Three codes, not two. `INVALID_ACTION` means "`action` is a name I know, that verb is not one I
have" — the constitution's registry already assigns it exactly this meaning, and
`iris_admin.action` is the largest enum on the surface, so routing it through a generic message
would leave FR-017's distinguishability story unfinished where it matters most.
`INVALID_PARAMS` covers every other bad value, including a required parameter that is missing.
`MISSING_PARAMS` is not used: it appears nowhere in the constitution's code registry, and a
missing required parameter is already `INVALID_PARAMS` there.

## The type-error case

A parameter of the wrong type is already rejected before the handler runs, but not in our shape.
Verified against the shipped 1.3.2 binary — `iris_query {"query": 42}` returns:

```json
{
  "isError": true,
  "content": [
    {
      "type": "text",
      "text": "failed to deserialize parameters: invalid type: integer 42, expected a string"
    }
  ]
}
```

No `success`, no `error_code`. FR-016 is therefore already satisfied structurally by rmcp; what
is missing is the shape. Normalizing it means catching the router's deserialization error in
`call_tool` and re-emitting it as:

```json
{
  "success": false,
  "error_code": "INVALID_PARAMS",
  "error": "iris_query: parameter \"query\" expects a string, received a number. failed to deserialize parameters: invalid type: integer 42, expected a string"
}
```

The original serde message is kept verbatim at the end of the `error` text — it names the
offending value and the expected Rust type, and rewriting it into something prettier is how
detail gets lost.

What serde does **not** name is the parameter. `failed to deserialize parameters: invalid type:
string "100", expected u32` leaves a caller with eight parameters to guess between, so the
leading sentence is built here: each argument's JSON type is compared against the type its
advertised property declares, and the mismatches are named. Nothing is claimed the schema cannot
back up — an untyped property or an unrecognized name falls through to serde's text alone.

serde stays the authority on what deserializes. The comparison only decides how the refusal is
worded; it never refuses a call of its own accord, so a schema it reads differently from the
struct behind it can make a message less specific but cannot reject a call that used to work
(FR-004).

## What does not change

- A call using only advertised parameters with permitted values behaves exactly as before —
  same result, same defaults for omitted parameters (FR-004).
- A parameter that is advertised but omitted is not an error unless it is in `required`.
- Gate error codes and messages are untouched.
