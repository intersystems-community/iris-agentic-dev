# Research: Terminal-Mode ObjectScript Compatibility (096)

**Date**: 2026-09-02
**Branch**: `096-terminal-objectscript-compat`

---

## Execution Path Analysis

`iris_execute` in `mod.rs:4076` has two paths:

### Path 1: HTTP (primary) — `execute_via_generator`

- Called at `mod.rs:4224–4228` (with timeout wrapper).
- Implemented at `connection.rs:434`.
- Wraps submitted code in a temp class method body, compiles via Atelier REST
  (`CodeMode=objectgenerator`), queries result, deletes the class.
- Class method bodies support full ObjectScript including `{}` block syntax.
- Works against any IRIS 2016.1+ instance without `IRIS_CONTAINER`.
- This path never applies the terminal constraint.

### Path 2: Docker exec fallback — `execute`

- Called at `mod.rs:4308–4309` when HTTP path fails.
- Implemented at `connection.rs:685`.
- Uses `docker exec -i <container> iris session IRIS -U <namespace>`.
- Pipes code as stdin to the IRIS terminal interpreter.
- The IRIS terminal processes input line by line; `{}` block syntax is NOT supported.
- `If (x=1) { Write "yes" }` causes `<SYNTAX>` with no explanation.
- Additional constraint: wraps at ~80 columns for single-line code (documented at
  `connection.rs:677–681`).
- Requires `IRIS_CONTAINER` env var; returns `DOCKER_REQUIRED` error if not set.

### When docker exec is the only path

Docker exec is the fallback after HTTP fails. HTTP fails (or is unavailable) in two
documented cases:

1. `docker_only=true` — connection configured without an HTTP endpoint.
2. `no_pws` — detected from `iris_version` containing `"2026.2.0AI"` (NoPWS builds;
   see DPP-1192 reference in CLAUDE.md); Atelier REST not available.

In these cases HTTP fails immediately, and the fallback at `mod.rs:4308` is the only
execution path. The check must therefore fire at the docker exec entry point.

---

## Pattern Reference: `contains_global_kill`

Location: `write_gate.rs:758–786`.

This is the exact pattern to follow for `contains_terminal_block_syntax`:

- Pure string scanner, no IRIS call, no async.
- Returns `bool`.
- Called at `mod.rs:4160` before any IRIS interaction.
- The new function follows the same contract.

Key difference: `contains_global_kill` is intentionally permissive on false positives
(kills in comments are caught). For terminal block syntax, the spec requires zero false
positives on valid terminal-mode code — the detection must skip `{` inside string
literals and global subscripts.

---

## Detection Rules (FR-001, FR-002)

### What to detect (FR-001)

`{` where the preceding non-whitespace token is one of: `If` (abbrev: `I`),
`Else` (abbrev: `E`), `For` (abbrev: `F`), `While` (abbrev: `W`), `Do`, `Try`, `Catch`
(case-insensitive).

Note on abbreviations: Standard ObjectScript single-letter terminal abbreviations are
`If`→`I`, `Else`→`E`, `For`→`F`, `While`→`W`. `Do`, `Try`, and `Catch` have NO
single-letter abbreviations in terminal mode.

Note on `ElseIf`: `ElseIf` is NOT a terminal-mode keyword. In terminal mode, the
equivalent construct is `Else` followed by `If` on separate lines. `ElseIf` as a single
keyword exists only inside class method bodies (not terminal mode). Do NOT include
`ElseIf` in the detection keyword list.

In practice, pattern: scan each line for `<keyword>` followed by optional condition
followed by `{`. The simplest correct approach:

1. For each line, skip leading whitespace.
2. Check if the line (or the non-whitespace tail after any condition expression) ends
   with `{` after one of the block-introducing keywords.

Simpler practical implementation: scan for `{` in each line; for each `{` found,
look backward at the non-whitespace context to determine if it's preceded by a
block-introducing keyword (with optional parenthesized condition between).

### What NOT to detect (FR-002, zero false positives)

- `{` inside a double-quoted string literal (e.g., `Set x = "{"` or `Write "{"`)
- `{` as a global subscript (e.g., `^G("{")`, `^G("a","{")`)
- `{` in function call arguments (e.g., `$LB("{")`)
- `{` in comments (lines starting with `;` or `//`)

A practical state machine approach: track whether the scanner is inside a
double-quoted string. When `{` is encountered outside a string, check if it is
preceded (ignoring whitespace and any parenthesized condition) by a block keyword.

---

## Integration Point (FR-003, FR-004)

The guard fires at `mod.rs` in the docker exec fallback block, immediately before
`iris.execute(code_to_run, &namespace)` at line 4309:

```rust
// Line ~4307 (before existing docker exec timeout call)
if crate::tools::write_gate::contains_terminal_block_syntax(code_to_run) {
    self.record_call("iris_execute", false);
    return err_result(serde_json::json!({
        "success": false,
        "error_code": "TERMINAL_SYNTAX_UNSUPPORTED",
        "error": "...",
    }));
}
```

This placement ensures:

- HTTP path never applies the check (the guard is only in the docker exec branch).
- No IRIS call is made when the guard fires (no round-trip latency).
- Fires before `tokio::time::timeout(...)` wrapper, so error is immediate.

---

## Tool Description Update (FR-005)

Current description at `mod.rs:4073` (single long string starting with "Execute
arbitrary ObjectScript code..."). The description mentions neither the two paths nor
the terminal constraint. Three facts must be added:

1. Primary path is HTTP (`CodeMode=objectgenerator`, class method body, `{}` works).
2. Docker exec fallback is terminal mode (line-by-line, `{}` not supported).
3. The `.mac` + `iris_compile` pattern is the escape hatch for complex scripts when on
   the docker exec path.

Update must be verifiable via binary invocation (`initialize` + `tools/list`).

---

## Compile-and-Run Escape Hatch (FR-006, P2)

When an agent needs `{}` block syntax on a docker exec path:

1. `iris_doc(mode="put", name="MyRoutine.mac", content="...")` — write the `.mac` with
   full block syntax.
2. `iris_compile(target="MyRoutine.mac")` — compiles via Atelier REST (HTTP), which
   supports `{}`.
3. `iris_execute(code="Do entry^MyRoutine")` — calls the entry point.

`iris_compile` and `iris_doc` are not changed by this spec. Only the `iris_execute`
description and tool guard are modified.

---

## No New Dependencies

This feature is pure Rust string scanning. No new crates required. The state machine for
string literal tracking is ~40 lines and uses only `std`. Follows Principle VII.

---

## API Verification

No new ObjectScript APIs are introduced. The feature adds a Rust-side pre-submission
guard only. No ObjectScript class or method needs to be verified. Principle II is N/A.

---

## Summary of Changes

| File                                                    | Change                                                                                      |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `crates/iris-agentic-dev-core/src/tools/write_gate.rs`  | Add `contains_terminal_block_syntax(code: &str) -> bool`                                    |
| `crates/iris-agentic-dev-core/src/tools/mod.rs`         | Add guard before docker exec fallback at ~line 4307; update `iris_execute` tool description |
| `docs/tools.md` (P2)                                    | Add compile-and-run escape hatch section                                                    |
| Test files in `crates/iris-agentic-dev-core/src/tools/` | Unit tests + binary invocation test + live IRIS integration test                            |
