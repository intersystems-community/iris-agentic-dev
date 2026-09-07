# Phase 0 Research: Typed input schemas for every MCP tool

Every decision below is backed by either a probe of the shipped 1.3.2 binary against
`iris-dev-iris`, or by reading the pinned dependency source in
`~/.cargo/registry/src/…`. Nothing here rests on recall.

## Baseline probe

Command: spawn `/opt/homebrew/bin/iris-agentic-dev mcp`, `initialize`, then `tools/call`,
with `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_NAMESPACE=USER
IRIS_CONTAINER=iris-dev-iris IRIS_WRITE_TOOLS_ENABLED=1 IRIS_ADMIN_TOOLS=1`.

| Call                                                                         | Result                                                                                                |
| ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `iris_query {"query":"SELECT 1","bogus_unknown_key":123,"namesapce":"USER"}` | `success: true`, 1 row, `"namespace":"USER"` from the **connection default** — both bad keys dropped  |
| `iris_query {"query":42}`                                                    | `isError: true`, text `failed to deserialize parameters: invalid type: integer 42, expected a string` |
| `iris_system_performance {"mode":"last_runid","bogus_unknown_key":1}`        | `success: true` — extra key dropped                                                                   |
| `iris_system_performance {"moed":"last_runid"}`                              | `success: false`, `"unknown mode ''; valid values: start, status, last_runid"`                        |

Schema census from the same `tools/list`: 81 tools, 37 with no `properties`, of which 31 are
`Parameters<AnyParams>` handlers (matched against the `async fn … Parameters<AnyParams>`
signatures in `src/tools/mod.rs`) and 6 legitimately take none — `agent_stats`,
`check_config`, `iris_import_servers`, `iris_reload_pool`, `skill_community_list`,
`skill_list`.

**Parameter census corrected.** The feature description put the unadvertised parameter count at 143. Extracting `p.get("…")` from each of the 31 handler bodies gives **132 parameter slots
across 71 distinct names** — the full inventory is in `data-model.md`. `iris_admin` reads **26**
keys, not the 14 the description listed; 089 added the mirror and journal keys
(`mirror_name`, `primary_host`, `primary_port`, `async_member_type`, `global_pattern`,
`time_range`, `max_records`) after that count was taken. `stream_inspect` reads three keys and
`max_chars` is not among them, confirming the documented-but-unread bug is still live.
`spec.md` SC-002 and its Context table were updated to the verified numbers, because a test
asserting 143 would fail on a correct implementation.

---

## Decision 1 — Per-tool struct with `deny_unknown_fields`, following `QueryParams`

**Decision**: each converted tool gets
`#[derive(Debug, Deserialize, JsonSchema)] #[serde(deny_unknown_fields)] pub struct XParams`
with `#[serde(default)]` on every currently-optional key and a doc comment per field
(schemars emits doc comments as `description`).

**Rationale**: `QueryParams` (`src/tools/mod.rs:1055`) is the established shape — 50 tools
already use it, so this is the pattern the codebase reads as normal. `deny_unknown_fields` is
what makes schemars emit `additionalProperties: false`; a comment already in the tree records
that fact from the output-schema work: "schemars only emits `additionalProperties: false`
under `#[serde(deny_unknown_fields)]`, which nothing here uses"
(`src/tools/output_schemas.rs:1764`). Confirmed by grep: zero occurrences in `crates/`.

**Alternatives considered**: keeping `serde_json::Value` and hand-writing a `JsonSchema` impl
per tool — that is `AnyParams` with extra steps, and 31 hand-rolled schemas would drift from
the handlers exactly as the prose did.

## Decision 2 — Enums declared with `#[schemars(extend("enum" = [...]))]` on a `String` field, handler validation untouched

**Decision**: leave each closed-set parameter as `Option<String>` and attach
`#[schemars(extend("enum" = ["start", "status", "last_runid"]))]`. Do **not** convert to a
Rust enum.

**Rationale**: the handlers already produce the error FR-006 asks for.
`iris_system_performance` with a bad mode returns
`unknown mode ''; valid values: start, status, last_runid` — it names the parameter and lists
the values, and it is `{success: false}` in our own response shape. A Rust enum moves that
rejection into serde, whose message (`unknown variant …, expected one of …`) does not reliably
name the field and arrives through rmcp's bare-text path with no `error_code`. Converting
would trade a good error for a worse one in service of a schema field we can declare directly.

**Verified**: `#[schemars(extend(...))]` is supported by the pinned derive —
`schemars_derive-1.2.2/src/attr/mod.rs:124` handles the `"extend"` key. Workspace pins
`schemars = "1"`, resolved to 1.2.2.

**Alternatives considered**: (a) Rust enums — rejected above; (b) a newtype per enum with a
manual `JsonSchema` impl, the technique `AnyParams` itself uses — works, but 8+ newtypes for
what one attribute expresses.

**Consequence for FR-005's test**: the advertised enum and the handler's match arms are two
places now, so the test compares them directly (schema `enum` array vs the literals in the
handler body) rather than comparing schema against prose.

## Decision 3 — Integers declared as integers, except the two params that already parse strings

**Decision**: `count`, `limit`, `max_bytes`, `max_entries`, `depth`, `max_records`,
`primary_port`, `async_member_type` become `Option<u32>` (narrowed where the handler casts to
`u16`/`u8`). `iris_interop_query`'s `session_id` and `since_id` get a `StringOrI64` helper with
`#[serde(untagged)]` and a `type: ["integer", "string"]` schema, because those two really do
accept both.

**Verified**: every numeric read in the 31 handlers was extracted from `mod.rs` and its
accessor inspected. All but two are `p.get(k).and_then(|v| v.as_u64()).unwrap_or(default)` —
`count` at 8542, `limit` at 6877/6911/8811, `max_bytes` at 7056's body, `max_entries` at 8770,
`depth` at 8986, `primary_port`/`async_member_type`/`max_records` in `iris_admin`. A string
`"20"` fails `as_u64()` in all of those, so the default silently applies and no caller can be
depending on string-encoded numbers there. Declaring `u32` converts a silent drop into a
rejection, consistent with the FR-015 decision.

The exceptions are real and would have shipped a regression:

```rust
since_id: p.get("since_id").and_then(|v| {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
}),
```

`mod.rs:6907` (`since_id`) and the identical block above it for `session_id`. Both accept
`12345` and `"12345"` today. Declaring them `Option<i64>` would reject the string form, which
is precisely the FR-008 violation the spec's "numbers may arrive as text" assumption is
guarding against. They are the only two.

**Alternatives considered**: a blanket `StringOrNumber` on every numeric field for
consistency — it would advertise a union that misdescribes nine parameters that do not accept
strings, and would newly _accept_ input those handlers currently discard, which is a behavior
change in the opposite direction.

## Decision 4 — `iris_admin`: 26 flat optional fields, per-action requirements stay in the docs

**Decision**: one struct, every field `Option<_>` except `action`, no untagged enum, no
`oneOf`. The per-action requirement tables in `docs/tools.md` remain the normative statement
of which fields each action needs.

**Rationale**: JSON Schema can express this with `if`/`then`, but schemars will not derive it
from a flat struct and hand-writing it for 26 fields across ~10 actions produces a schema no
one will maintain. Marking any of them unconditionally required breaks every action that does
not use it (FR-007). Precedent: those doc tables already exist and already caught a real
drift — they documented `type_filter`, `namespace_filter`, `name_filter` for three actions
that read `type`, `namespace`, `name` (recorded at
`tests/unit/test_docs_contract.rs:1290`).

**Alternatives considered**: splitting `iris_admin` into per-action tools — that is new tool
surface, explicitly out of scope, and would break every existing caller.

## Decision 5 — One validation site in `call_tool`, reading properties from the router

**Decision**: after `gate_check` and before `tool_router.call(tcc)` in the `call_tool`
override (`src/tools/mod.rs:9077`), diff the incoming argument keys against the advertised
property names for that tool. Any key not advertised → return the FR-015 rejection.

**Verified**: `ToolRouter::list_all() -> Vec<crate::model::Tool>` is public
(`rmcp-3.1.3/src/handler/server/router/tool.rs:581`), and `Tool` carries `input_schema`
(same file, 170/307/316). So the property names come from the router's own registry — no
literal list, satisfying Principle XIII's rule about gate name lists. Build the
`name → BTreeSet<String>` map once and cache it; the router's tool set is fixed after
construction.

**Rationale**: `deny_unknown_fields` alone does not deliver FR-015 in a usable form. The probe
shows what a serde failure looks like through rmcp: `isError: true` plus the bare string
`failed to deserialize parameters: …`, with no `success` field and no `error_code`. That
breaks Principle V for a response an agent has to act on. One site produces our shape for all
81 tools, including the 50 already-typed ones that swallow unknown keys today — which is the
`namesapce` bug, and it is not in scope of the 31 conversions at all.

**Alternatives considered**: (a) a JSON-Schema validator crate (`jsonschema`, `valico`) — a
set difference over already-parsed names is ~30 lines, so Principle VII forbids the dep;
(b) per-handler unknown-key checks — 31 copies of one rule, the exact shape of the
`error-sentinels` registry entry.

**Ordering constraint**: conversion must precede the flip. A tool advertising no properties
would have _every_ argument rejected, so enabling validation before batch 8 completes would
break the 31 tools it is meant to protect.

## Decision 6 — Gate refusal keeps winning over parameter rejection

**Decision**: leave `gate_check` where it is — first, on raw `request.arguments`. Parameter
validation goes after it.

**Rationale**: `call_tool` resolves gates at `mod.rs:9097-9103`, before the router is reached,
and 085 deliberately put it there. A caller with writes disabled sending a mis-spelled key to
`global_kill` must be told the gate refused, not that a key was unknown — the gate is the
security answer and the parameter error is a usability one. Also: `gate_check` reads raw
arguments by name, so typed structs cannot affect it; the wire format is unchanged.

**Test, not assumption**: a live test calls a destructive tool with the gate closed _and_ a
bogus key, and asserts the gate's refusal code comes back.

## Decision 7 — `NoParams` closes too, and the two no-argument tools adopt it

**Decision**: add `#[serde(deny_unknown_fields)]` to `NoParams` (`mod.rs:937`), and give
`iris_import_servers` (`mod.rs:8271`) and `iris_reload_pool` (`mod.rs:8044`) a
`Parameters<NoParams>` argument — they currently declare no parameter at all.

**Rationale**: FR-002 requires "no parameters" to be advertised explicitly rather than being
indistinguishable from "nobody declared them". Four of the six already use `NoParams`
(`agent_stats`, `check_config`, `skill_community_list`, `skill_list`); making it six and
closing the struct means all 81 tools state their surface. Without the attribute, `NoParams`
advertises an open object just as `AnyParams` does.

## Decision 8 — Deleting `AnyParams` makes the compiler the detector; the scanner covers the rest

**Decision**: delete `AnyParams` (`mod.rs:73-91`) and its `dispatch_any!` macro in
`call_for_test` (`mod.rs:10990`) once batch 8 lands. Add an `undeclared-params` detector to
`scripts/gates/antipatterns.py` that flags a params struct without `deny_unknown_fields`, any
`Parameters<serde_json::Value>`, and any `#[serde(flatten)]` into a `Value` or `Map` — that
third shape satisfies the first two rules while reopening the surface.

**Rationale**: after deletion, `Parameters<AnyParams>` does not compile — the strongest
possible detector, and it satisfies the registry rule that a rule cargo can express belongs in
cargo, not a second scanner. What the compiler _cannot_ see is a future struct that reopens
the surface by omitting `deny_unknown_fields`, or one that takes a raw `Value` under a new
name. That is the scanner's job, and per the registry this class is never baselined.

**Migration risk found**: `call_for_test` (`mod.rs:10952`, `cfg(any(test, feature =
"testing"))`) dispatches typed tools through `serde_json::from_value`. Once the 31 tools have
closed structs, any existing test that passes an extra or misspelled key through
`call_for_test` starts failing. That is the detector working, but it means the conversion
batches must expect test churn, not just new tests.

## Decision 9 — Three test sites in `test_docs_contract.rs` change; one does not

**Decision**:

- `every_documented_tool_parameter_is_in_the_input_schema` (line 894): remove the
  `None => match handler_body(tool)` fallback and the `by_handler > 0` tail assertion. That
  assertion currently _requires_ the fallback to fire, so it fails the moment the last
  AnyParams tool is converted — it has to be retired in the same commit, not before.
- `handler_body` (line 990): delete once nothing calls it.
- The `iris_admin` per-action table extractor (line 1290): **keep**. It reads `docs/tools.md`
  against the handler, which is still the only check on the per-action requirements Decision 4
  leaves in prose.

**Rationale**: FR-012 asks for the escape hatch to be gone and its absence asserted. The
zero-empty-properties test is what replaces it, and it is a stronger statement: the fallback
answered "is this documented parameter readable?", the new test answers "does every tool state
its parameters?"

## Decision 10 — Eval-harness regression run before merge

**Decision**: Principle IX is N/A (no new tool), but the GEPA eval harness runs before merge
and the result goes in `specs/113-typed-tool-schemas/lift-results.md` as a regression check,
not a lift claim.

**Rationale**: this change alters what every agent sees in `tools/list` for 38% of the surface
— adding properties, adding `additionalProperties: false`, and starting to reject calls that
previously "succeeded". That is exactly the input the harness measures. A schema that is
correct but degrades task success is a regression worth catching before a release, and the
harness is the only instrument that sees it.

## Decision 11 — Tests assert on the normalized listing, and resolve enums through `anyOf`

**Decision**: every schema assertion runs against what `list_tools` emits, and reads an
optional parameter's `enum` from `anyOf[0]`, not from the property's top level.

**Verified**: `list_tools` is overridden at `mod.rs:9190` and calls `normalize_schema_openapi3`
on each input schema. That function turns a nullable type array into
`anyOf: [{…}, {"type": "null"}]`, relocating `enum`, `format`, `minimum`, `items`, `properties`,
`required`, and `additionalProperties` into the non-null branch (`mod.rs:9266-9345`). Confirmed
on the wire: `iris_query.namespace` arrives as
`{"anyOf": [{"type": "string"}, {"type": "null"}], "default": null, "description": …}`, while
the defaulted `iris_query.force` stays flat as `{"type": "boolean", "default": false}`.

**Why it matters**: a test reading `properties.mode.enum` finds nothing on an optional
parameter. Written in the natural defensive style — compare the enum _if one is present_ — it
passes vacuously forever. Principle XI names this exact hazard, and normalization makes it the
default outcome rather than an edge case.

**Also verified**: `tools/list` paginates (`IRIS_LIST_TOOLS_PAGE_SIZE`, default 200 at
`mod.rs:9224`). At 81 tools one page holds everything and `nextCursor` is null. A census test
must assert that rather than assume it.

**Alternatives considered**: asserting on `schema_for!(XParams)` in unit tests only — cheaper,
but it tests schemars rather than the server, and would have missed the `anyOf` rewrite
entirely. This is why FR-013 says "the listing a running server actually emits."

## Decision 12 — `additionalProperties` and enums are cross-cutting, so batch 8 covers the already-typed 50

**Decision**: add an eighth batch that applies `deny_unknown_fields` to the existing params
structs and declares the enums they are missing. Without it, two success criteria are
unreachable.

**Verified by probe against 1.3.2**:

- **`additionalProperties: false` appears on 0 of 81 tools.** Not 50 good and 31 bad — none.
  FR-002 and the FR-015 rejection therefore require touching every params struct in the tree,
  not just the 31 new ones.
- **Eight closed-set parameters on already-typed tools carry their value set only in prose**:
  `iris_query.mode` (read/explain/count/write), `iris_coverage.mode`
  (check/report/run/start/stop), `iris_doc.category` (ALL/CLS/INC/INT/MAC),
  `iris_doc.compiled_type` (INT/OBJ), `iris_doc.elicitation_answer` (yes/no),
  `iris_test.test_type` (auto/testcase/testproduction), `iris_generate.gen_type` (class/test),
  `iris_add_server.scheme` (http/https). Found by scanning emitted descriptions for two or more
  quoted literals on a property with no `enum` anywhere in it; four further hits were false
  positives where the prose quoted example method names, not a closed set.

SC-004 says the count of value sets living only in prose reaches zero, and US2 never restricted
itself to the 31. Leaving these out would mean shipping a feature whose own success criterion
fails on tools it never looked at.

**Alternatives considered**: narrowing SC-004 to the 31 converted tools — that is moving the
goalposts to fit the batch plan, and it leaves `iris_query.mode` (one of the most-called
parameters on the server) guessable from prose only.

## Open items

None blocking. Two things are deliberately deferred rather than unresolved:

- **Per-property description length.** Schema size is context every session pays for. The
  batches should not restate the tool description per field; measured `tools/list` byte growth
  is worth recording in the polish phase, but it does not gate any task.

  Measured (T038), same 81 tools both times, installed 1.3.2 against `target/debug` at the end
  of Phase 7, comparing the minified `{"tools": […]}` payload of one full `tools/list`:

  | Metric                                   | 1.3.2  | 113     | Change |
  | ---------------------------------------- | ------ | ------- | ------ |
  | Whole payload                            | 77,170 | 104,374 | +35%   |
  | Input schemas                            | 35,195 | 62,399  | +77%   |
  | Tool descriptions                        | 35,062 | 35,062  | none   |
  | Tools with no declared properties        | 37     | 6       | −31    |
  | Tools with `additionalProperties: false` | 0      | 81      | +81    |

  27 KB for 143 parameter slots is about 190 bytes per parameter, and it buys the whole feature.
  Two tools carry a third of the growth on their own — `iris_admin` +4,900 B (26 parameters, a
  25-value enum) and `iris_interop_query` +4,550 B (15 parameters, one nested object). No other
  tool exceeds +1 KB. Tool descriptions did not move, which is the check that the growth is
  schema and not restated prose.

  The one repeated description is `server`, identical on 24 of the new structs: "Route this call
  to a named registered IRIS instance. If omitted, uses the default connection." (93 bytes).
  Cutting it to a clause saves under 1 KB of the 27 KB. Left as it is — a client reading only the
  schema has to be told what `server` selects, and 700 bytes does not pay for a caller
  misreading it. Nothing else repeats; `namespace`, `action`, and the per-action notes are
  specific to their tool.

  Clients that do not want the catalog cost have two ways out, both already in
  `docs/tools.md` — Anthropic's Tool Search Tool, or `IRIS_LIST_TOOLS_PAGE_SIZE` pagination.

- **SystemPerformance profile management** (`list_profiles`, `add_profile`, report retrieval)
  is a separate spec, per spec.md's Out of Scope.
