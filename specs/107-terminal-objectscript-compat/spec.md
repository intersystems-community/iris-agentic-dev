# Feature Specification: Terminal-Mode ObjectScript Compatibility

**Feature Branch**: `096-terminal-objectscript-compat`
**Created**: 2026-09-02
**Status**: Draft

## Overview

`iris_execute` has two execution paths:

1. **HTTP (primary)**: `execute_via_generator` — wraps submitted code in a temporary
   class method body, compiles it via Atelier REST, runs it, deletes the class. Block
   syntax (`{}`) works fine here because class method bodies support it.

2. **Docker exec (fallback)**: `execute` — pipes code into `iris session` stdin when
   `IRIS_CONTAINER` is set and HTTP fails. The IRIS terminal interpreter is line-by-line
   and does **not** support `{}` block syntax. `If (x=1) { Write "yes" }` causes
   `<SYNTAX>` with no explanation.

The problem surfaces when agents are on NoPWS containers or when HTTP fails and the
docker exec fallback is the only path. Block syntax that works perfectly in VS Code (or
would work via HTTP) fails silently on the terminal path with a raw `<SYNTAX>` error.

Additional constraint documented in the existing code: the docker exec path also wraps
at ~80 columns when code is sent as a single long line (see connection.rs:677 comment).

This spec adds:

1. **Detection before docker exec submission** — when routing to the docker exec path,
   scan for `{}` block syntax and return a structured, actionable error before the
   round-trip to IRIS terminal.
2. **Tool description update** — document both execution paths and the terminal
   constraint so agents understand the difference and can plan accordingly.
3. **P2 — .mac workflow documentation** — document the compile-and-run pattern as the
   escape hatch when terminal-compatible syntax is insufficient.

---

## User Scenarios & Testing

### User Story 1 — Docker exec path: block syntax returns actionable error (Priority: P1)

An agent is on a NoPWS container (HTTP unavailable, docker exec is the only path). It
submits `If ($Get(^Setting("debug"))=1) { Write "debug on" }`. Gets `<SYNTAX>`. No hint.

After this fix: the error fires before the IRIS round-trip, names the pattern, and
suggests the terminal-compatible rewrite or the compile-and-run escape hatch.

**Independent Test**: In a test that forces the docker exec path (mock HTTP failure or
`docker_only=true`), submit block-syntax code — assert `TERMINAL_SYNTAX_UNSUPPORTED`
with actionable message before any docker exec is invoked.

**Acceptance Scenarios**:

1. Given the docker exec path is active and code contains `If condition {`, When
   `iris_execute` is called, Then the response is `{success: false,
error_code: "TERMINAL_SYNTAX_UNSUPPORTED", error: "..."}` — no IRIS call made.
2. The error names the specific pattern detected and gives a terminal-compatible example.
3. Given classic single-line form (`If cond Write "yes"`), Then it executes normally.
4. Given dotted-DO syntax (`. Write "yes"`), Then it executes normally.
5. Given `{` inside a string (`Set x = "{"`) or global subscript (`^G("{")`), Then no
   false positive — detection does not fire.

### User Story 2 — Tool description guides agents before they write code (Priority: P1)

An agent planning to run multi-line ObjectScript reads the `iris_execute` tool
description and learns upfront about the two paths and when `{}` is unsafe.

**Independent Test**: Binary invocation — `initialize` + `tools/list` — assert the
`iris_execute` description mentions "terminal mode", "docker exec", and `{}` limitation.
No live IRIS needed.

**Acceptance Scenarios**:

1. The `iris_execute` description explains the two paths: HTTP (class method, `{}`
   works) and docker exec fallback (terminal, `{}` not supported).
2. The description gives one terminal-compatible alternative example.
3. The description mentions `iris_compile` + `.mac` as the escape hatch for complex
   multi-line scripts on the docker exec path.

### User Story 3 — Compile-and-run escape hatch (Priority: P2)

An agent needs to run a script with complex conditionals on a docker exec path. It
writes the code to a `.mac` file, compiles it, and calls the entry point.

**Independent Test**: Live IRIS — write a `.mac` with `{}` block syntax, `iris_compile`
it, `iris_execute Do entry^Routine`, assert correct output.

**Acceptance Scenarios**:

1. A `.mac` file containing `{}` block syntax compiles successfully via `iris_compile`.
2. `iris_execute` calling `Do entry^RoutineName` returns the expected output.

---

## Functional Requirements

- **FR-001**: The detection function `contains_terminal_block_syntax(code: &str) -> bool`
  is added to the codebase (alongside `contains_global_kill` in `write_gate.rs` or a new
  `syntax_guard.rs`). It returns `true` when the code contains `{` where the preceding
  non-whitespace token is a block-introducing keyword: `If` (abbrev: `I`), `Else`
  (abbrev: `E`), `For` (abbrev: `F`), `While` (abbrevs: `Wh`, `Whi`, `Whil`), `Do`,
  `Try`, `Catch`. Note: `W` is unambiguously `Write` (output command), not `While` —
  including `W` in the detection list causes false positives on any code that calls
  `Write` before a `{`. ObjectScript allows any unambiguous prefix abbreviation, and `W`
  is unambiguously `Write`, not `While`. `Do`, `Try`, and `Catch` have no single-letter
  abbreviations in terminal mode.
  `ElseIf` is NOT a terminal-mode keyword — in terminal mode, `ElseIf` is written as
  `Else` followed by `If` on separate lines; `ElseIf` as a single keyword exists only
  inside class method bodies. Do not include `ElseIf` in the detection keyword list.
- **FR-002**: Detection must NOT fire on:
  - `{` inside a double-quoted string literal
  - `{` as a global subscript (preceded by `(` or `,`)
  - `$LB(...)`, `$ListBuild(...)`, or other `$` function calls
    The goal is zero false positives on valid terminal-mode code.
- **FR-003**: Detection fires only when the **docker exec path** is about to be used.
  The HTTP path (execute_via_generator) must never apply this check — `{}` is valid in
  class method bodies.
- **FR-004**: On detection, `iris_execute` returns immediately (no docker exec invoked).
  The error message is STATIC — it does not embed a snippet of the caller's code:
  ```json
  {
    "success": false,
    "error_code": "TERMINAL_SYNTAX_UNSUPPORTED",
    "error": "Code contains block-syntax (`{...}`) that is not supported in terminal (docker exec) mode. Rewrite in classic form (If cond Write x) or use iris_doc + iris_compile to write a .mac routine."
  }
  ```
- **FR-005**: Update the `iris_execute` `#[tool]` description string in `mod.rs` to
  document: primary path is HTTP (class method body, `{}` works); docker exec fallback
  is terminal mode (line-by-line, `{}` not supported); the `.mac` + `iris_compile`
  pattern is the escape hatch for complex scripts on the docker exec path.
- **FR-006**: P2 — Add a section to `docs/tools.md` or an existing skill documenting the
  compile-and-run pattern step by step (write `.mac` → `iris_compile` → `iris_execute
Do entry^Routine`). Do not auto-rewrite submitted code.

---

## Key Entities

- **`contains_terminal_block_syntax(code)`**: detection function, pure string analysis,
  no IRIS call.
- **`TERMINAL_SYNTAX_UNSUPPORTED`**: new error code, returned only on docker exec path.
- **Two execution paths**: HTTP (`execute_via_generator`, class method body) vs. docker
  exec (`execute`, iris session terminal).

---

## Success Criteria

- An agent on the docker exec path that submits `{}` block syntax receives
  `TERMINAL_SYNTAX_UNSUPPORTED` with an actionable message in under 10ms (no IRIS
  round-trip).
- Zero false positives: all existing integration tests that use classic terminal-mode
  syntax pass unchanged.
- The `iris_execute` tool description change is visible in `tools/list` output (binary
  invocation test).
- The compile-and-run workflow succeeds end-to-end against `iris-dev-iris`.

---

## Out of Scope

- Detecting `{}` blocks on the HTTP path — they are valid there.
- Auto-rewriting block syntax to terminal-compatible form.
- Detecting all possible ObjectScript syntax errors before submission.
- Supporting `{}` in terminal mode (IRIS limitation).
- Changes to `iris_compile`, `iris_doc` — only `iris_execute` and its description.

---

## Assumptions

- The docker exec path (`execute` in `connection.rs:685`) is the only place where
  `iris session` terminal semantics apply.
- The existing line-length (~80 col) limitation of the docker exec path is documented
  in code (connection.rs:677) and does not need a separate spec — it is a known
  constraint callers should route around via the HTTP path.
- `contains_global_kill` in `write_gate.rs` is the closest existing pattern for a
  pre-submission code scanner; the new detection function should follow that pattern.
