# Data Model: Terminal-Mode ObjectScript Compatibility (096)

**Date**: 2026-09-02
**Branch**: `096-terminal-objectscript-compat`

---

## New Error Code

### `TERMINAL_SYNTAX_UNSUPPORTED`

Returned by `iris_execute` when the docker exec fallback path is about to be used and the
submitted code contains `{}` block syntax that the IRIS terminal interpreter does not
support.

**Error code registry**: `SCREAMING_SNAKE_CASE`. Follows the standard `err_json` shape
(Principle V — output shape parity):

```json
{
  "success": false,
  "error_code": "TERMINAL_SYNTAX_UNSUPPORTED",
  "error": "Code contains block-syntax (`{...}`) that is not supported in terminal (docker exec) mode. Rewrite in classic form (If cond Write x) or use iris_doc + iris_compile to write a .mac routine."
}
```

Note: The error message is STATIC — it does not embed a snippet of the caller's submitted
code. The function `contains_terminal_block_syntax(code: &str) -> bool` returns `bool`;
the caller is responsible for generating the error message using the fixed template above.

**When fired**: Only in the docker exec branch of `iris_execute` (`mod.rs` around line
4307), after HTTP path failure and before docker exec is invoked. Never on the HTTP
(`execute_via_generator`) path.

**No IRIS call made**: The error is returned synchronously by the Rust guard without
touching the IRIS container.

---

## New Function

### `contains_terminal_block_syntax(code: &str) -> bool`

Location: `crates/iris-agentic-dev-core/src/tools/write_gate.rs`

Companion to `contains_global_kill`. Pure function — no IRIS call, no async, no side
effects.

**Signature**: `pub fn contains_terminal_block_syntax(code: &str) -> bool`

**Returns `true` when**: the code contains `{` where the preceding non-whitespace
context includes a block-introducing keyword — `If` (abbrev: `I`), `Else` (abbrev: `E`),
`For` (abbrev: `F`), `While` (abbrev: `W`), `Do`, `Try`, `Catch` — separated from `{`
by an optional parenthesized condition expression. Note: `Do`, `Try`, and `Catch` have
no single-letter abbreviations in terminal mode. `ElseIf` is NOT in the detection list —
it is not a terminal-mode keyword (see research.md for rationale).

**Returns `false` when**:

- `{` appears only inside double-quoted string literals.
- `{` appears inside a global subscript (`^Foo("{")`, `^G("a","{")`) — detected as
  being inside a string literal at that point.
- `{` appears in `$` function call arguments (inside string).
- `{` appears in a comment line (`;` or `//` prefix).
- Code contains no `{` at all.
- Code uses only classic terminal-mode ObjectScript (single-line `If`, dotted-DO).

**False positive policy**: Zero false positives on valid terminal-mode code. A missed
detection (block syntax not caught) is acceptable if the pattern is unusual; a false
positive on correct terminal-mode code is not acceptable.

---

## Existing Error Codes (unchanged)

Standard codes already established (from constitution Error Code Registry):

| Code                    | Description                                   |
| ----------------------- | --------------------------------------------- |
| `IRIS_UNREACHABLE`      | Connection failed or no connection configured |
| `DOCKER_REQUIRED`       | Tool needs `IRIS_CONTAINER` but it is not set |
| `HTTP_EXECUTION_FAILED` | HTTP path failed, no docker fallback          |
| `IRIS_RUNTIME_ERROR`    | IRIS returned an ERROR: prefix in output      |
| `TIMEOUT`               | Execution timed out                           |
| `EXECUTION_FAILED`      | Docker exec returned an error                 |

The new `TERMINAL_SYNTAX_UNSUPPORTED` code slots between `DOCKER_REQUIRED` and
`EXECUTION_FAILED` in the docker exec error path — it fires before docker exec is
attempted, while the others fire after it fails.

---

## Response Shape Contracts

No new tool is added. `iris_execute` response shape is unchanged for the happy path.
Error responses follow the existing `err_json` shape:

```json
{
  "success": false,
  "error_code": "<CODE>",
  "error": "<human-readable message with actionable suggestion>"
}
```

Principle V (output shape parity) is satisfied: the new error response uses the same
keys as all other `iris_execute` error responses.

---

## Tool Description Contract

The `iris_execute` tool description (string at `mod.rs:4073`) gains three new facts:

1. The primary path is HTTP (`CodeMode=objectgenerator`); the submitted code runs as a
   class method body, so `{}` block syntax is fully supported there.
2. The docker exec fallback uses IRIS terminal mode (line-by-line); `{}` block syntax
   is not supported and triggers `TERMINAL_SYNTAX_UNSUPPORTED`.
3. The escape hatch for complex scripts on the docker exec path: write to a `.mac` with
   `iris_doc`, compile with `iris_compile`, then `iris_execute Do entry^RoutineName`.

These additions are visible in `tools/list` output and testable via binary invocation.
