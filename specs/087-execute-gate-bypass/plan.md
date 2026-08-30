# 087 — iris_execute gate bypass: implementation plan

## Status

Draft.

## Current state (verified against codebase)

`iris_execute` is already `wr("iris_execute")` in `write_gate::CLASSIFICATION` —
write-gated. `gate_check` runs before the handler body in `call_tool` dispatch and
blocks `iris_execute` when `write_tools_enabled = false`. That half of the bug is
already fixed.

The open gap: `iris_execute` is classified as `Write`, not `Destructive`. A caller
with `write_tools_enabled = true` but `destructive_tools_enabled = false` can run
`Kill ^SomeGlobal` through `iris_execute` with no refusal. `iris_global` kill is
classified `Destructive` (the `mixed` entry in CLASSIFICATION), so the same operation
through `iris_global` is correctly blocked.

## Design decision

**Option 3 from spec:** content-sensitive destructive check inside the `iris_execute`
handler body, after the `gate_check` write-tier pass, before the IRIS HTTP call.

- `gate_check` continues to enforce the write tier via CLASSIFICATION (no change to
  that path).
- A new helper `contains_global_kill(code: &str) -> bool` checks for the patterns
  that literally appear in the code string and indicate a direct kill of a global:
  `Kill ^`, `KILL ^`, `k ^`, `K ^` (case variants ObjectScript accepts). This is a
  simple substring/regex check.
- If `contains_global_kill` returns true and `gates.destructive_enabled` is false,
  `iris_execute` returns the standard `DESTRUCTIVE_TOOLS_DISABLED` refusal before any
  IRIS call.
- The check lives in a standalone function in `write_gate.rs` (or a small submodule)
  so it is unit-testable without the full handler.

This does NOT claim to catch indirect vectors (`Kill @var`, `Xecute`, `##class`
dispatch, `&sql`). The spec says so plainly. The check is defense against inadvertent
sloppiness, not a security boundary.

## Files changed

| File                                                              | Change                                                                                                            |
| ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `crates/iris-agentic-dev-core/src/tools/write_gate.rs`            | Add `pub fn contains_global_kill(code: &str) -> bool`                                                             |
| `crates/iris-agentic-dev-core/src/tools/mod.rs`                   | Call the helper in `iris_execute` body; return destructive refusal if triggered                                   |
| `crates/iris-agentic-dev-core/tests/unit/test_write_gate.rs`      | Unit tests for `contains_global_kill`                                                                             |
| `crates/iris-agentic-dev-bin/tests/integration/test_exec_live.rs` | Live IRIS integration test: `iris_execute("Kill ^TestGlobal")` blocked when destructive gate off; allowed when on |
| `docs/tools.md`                                                   | Note on `iris_execute` destructive-tier behaviour                                                                 |

## `contains_global_kill` spec

Matches any line that contains a Kill/KILL/k/K token immediately followed by optional
whitespace and then `^`. Case-insensitive on the keyword. Must not match:

- `Kill localvar` (no `^`)
- A comment line containing `// Kill ^foo` — decision: do NOT attempt comment
  stripping; false positives here are acceptable (blocking a comment that looks like a
  kill is better than missing a kill that looks like a comment). Regex: `(?i)\bkill\s*\^`

Must match:

- `Kill ^Foo`
- `KILL ^Foo`
- `Kill  ^Foo("bar")` (extra whitespace)
- Multiline code where one line contains `Kill ^Foo`

## Error message

```
iris_execute contains a Kill ^<global> expression and the destructive tier is disabled
(source: <source>). Set destructive_tools_enabled = true in .iris-agentic-dev.toml
to allow destructive operations. Note: this check applies to literal Kill ^ patterns
in the code string only. Indirect kill operations (via variables, Xecute, or class
methods) are not detected here — IRIS-side credentials and the mcpTemplate env gate
are the appropriate controls for those.
```

The last two sentences prevent anyone from misreading the error as a comprehensive
block.

## Test requirements (three layers, non-negotiable)

### Layer 1 — unit tests for `contains_global_kill`

File: `crates/iris-agentic-dev-core/tests/unit/test_write_gate.rs`

- `Kill ^Foo` → true
- `KILL ^Foo` → true
- `Kill  ^Foo("sub")` (extra space) → true
- `Kill ^` (bare caret) → true
- Multiline with kill on line 3 → true
- `Kill localvar` (no caret) → false
- `Set ^Foo=1` (set, not kill) → false
- Empty string → false
- `// Kill ^Foo` — acceptable either way; document which (recommend: true, false
  positive is the safer side)

### Layer 2 — binary invocation (no live IRIS)

File: `crates/iris-agentic-dev-bin/tests/integration/test_exec_live.rs`
(or a new `test_exec_gate.rs` if cleaner)

Spawn `iris-agentic-dev` as subprocess. Send `tools/call` with
`iris_execute(code="Kill ^IadGateTest")` with env
`IRIS_WRITE_TOOLS_ENABLED=1 IRIS_DESTRUCTIVE_TOOLS_ENABLED=0`. Assert JSON-RPC
response contains `error_code: "DESTRUCTIVE_TOOLS_DISABLED"` without connecting to
IRIS. `#[ignore]` tag; CI wires `IAD_BINARY`.

### Layer 3 — live IRIS integration

File: `crates/iris-agentic-dev-bin/tests/integration/test_exec_live.rs`

`#[ignore]` test against `iris-dev-iris` (localhost:52780), `--test-threads=1`.

Two cases:

1. Destructive gate off + `Kill ^IadGateTest`: expect `DESTRUCTIVE_TOOLS_DISABLED`
2. Destructive gate on + `Kill ^IadGateTest`: expect the kill to succeed (set the
   global first in the same test, clean up after)

The test sets/kills a dedicated `^IadGateTest` global so it never touches anything
meaningful.

## Out of scope

- `iris_ws_exec` — same class of problem, separate spec.
- Detecting indirect kill vectors — stated as out of scope in spec.
- Modifying `gate_check` / CLASSIFICATION to handle code-content dispatch — the
  `classify` function dispatches on `action`/`mode` args; extending it to handle
  arbitrary code-string content is more invasive than needed and the handler-body check
  achieves the same result.
