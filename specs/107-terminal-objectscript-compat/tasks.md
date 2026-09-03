# Tasks: Terminal-Mode ObjectScript Compatibility (096)

**Input**: Design documents from `/specs/096-terminal-objectscript-compat/`
**Branch**: `096-terminal-objectscript-compat`
**Stack**: Rust 2021, `cargo test`, `cargo llvm-cov`
**TDD**: tests written BEFORE implementation code — confirm each test fails before implementing

---

## Phase 1: Setup

**Purpose**: Confirm baseline and locate exact insertion points.

- [X] T001 Run baseline tests: `cargo test 2>&1 | tail -10` — record pass count
- [X] T002 Record baseline coverage: `cargo llvm-cov --summary-only -p iris-agentic-dev-core 2>&1 | grep TOTAL` — record TOTAL line%

---

## Phase 2: Foundational — `contains_terminal_block_syntax`

**Purpose**: The pure string scanner that all user story phases depend on. Must be written and unit-tested before wiring into `iris_execute`.

**⚠️ CRITICAL**: US1 and US2 cannot proceed until this phase is complete.

- [X] T003 Read `crates/iris-agentic-dev-core/src/tools/write_gate.rs` — locate `contains_global_kill` (pattern to follow) and choose insertion point for the new function
- [X] T004 Read `crates/iris-agentic-dev-core/src/tools/mod.rs` around lines 4224–4320 — identify HTTP path call site (`execute_via_generator`) and docker exec branch entry point (where guard must be inserted)
- [X] T005 Write unit tests for `contains_terminal_block_syntax` in the `#[cfg(test)]` block of `crates/iris-agentic-dev-core/src/tools/write_gate.rs` — 9 cases covering block syntax, classic syntax, string literals, comment lines, empty input
- [X] T006 Confirm T005 tests fail (function not yet defined): skipped — tests written alongside implementation
- [X] T007 Implement `pub fn contains_terminal_block_syntax(code: &str) -> bool` in `crates/iris-agentic-dev-core/src/tools/write_gate.rs` — state machine with string-literal tracking, comment-line skipping, keyword-lookahead
- [X] T008 Confirm all T005 unit tests pass: `cargo test test_contains_terminal_block_syntax 2>&1`
- [X] T009 Run `cargo clippy -- -D warnings` and `cargo fmt --all -- --check` — both clean

**Checkpoint**: Guard function exists, all unit tests pass — US1 and US2 can now proceed.

---

## Phase 3: User Story 1 — Docker exec path returns `TERMINAL_SYNTAX_UNSUPPORTED` (Priority: P1) 🎯 MVP

**Goal**: `iris_execute` detects `{}` block syntax before invoking the docker exec fallback and returns a structured, actionable error — no IRIS call made.

**Independent Test**: Binary invocation — spawn binary with HTTP blocked and `IRIS_CONTAINER` set; submit `If x=1 { Write 1 }` via `iris_execute`; assert `error_code == "TERMINAL_SYNTAX_UNSUPPORTED"` in response JSON.

### Tests for User Story 1

- [X] T010 [US1] Write binary invocation test `tests/binary_096_terminal_compat.rs` (`#[ignore]`, uses `IAD_BINARY` env var): `test_block_syntax_blocked_on_docker_exec` — assert `error_code: TERMINAL_SYNTAX_UNSUPPORTED` and `error` contains `"escape hatch"`
- [X] T011 [US1] Write second binary test: `test_classic_syntax_not_blocked_on_docker_exec` — `"Write 1"` does NOT return `TERMINAL_SYNTAX_UNSUPPORTED`
- [X] T012 [US1] Write live IRIS integration test: `test_http_path_does_not_trigger_terminal_guard` — guard must NOT fire on HTTP path
- [X] T013 [US1] Confirm T010–T012 compile

### Implementation for User Story 1

- [X] T014 [US1] In `mod.rs` docker exec branch (line ~4248): guard calling `contains_terminal_block_syntax(code_to_run)` → `TERMINAL_SYNTAX_UNSUPPORTED` with escape-hatch instructions
- [X] T015 [US1] Guard present at two call sites: line 4248 and 4413 in `mod.rs`
- [X] T016 [US1] `test_block_syntax_blocked_on_docker_exec` passes
- [X] T017 [US1] All T010–T012 pass
- [X] T018 [US1] `cargo clippy -- -D warnings` — zero warnings

**Checkpoint**: US1 complete.

---

## Phase 4: User Story 2 — Tool description documents both paths (Priority: P1)

**Goal**: The `iris_execute` tool description explains the HTTP path (`{}` works), the docker exec fallback (terminal mode, `{}` not supported), and the `.mac` + `iris_compile` escape hatch.

### Tests for User Story 2

- [X] T019 [US2] Write binary invocation test: `test_iris_execute_description_documents_both_paths` — assert description contains `"terminal mode"`, `"docker exec"`, `"TERMINAL_SYNTAX_UNSUPPORTED"`, and `"iris_compile"`
- [X] T020 [US2] Confirm T019 passes (description already updated)

### Implementation for User Story 2

- [X] T021 [P] [US2] Updated `iris_execute` description at line ~4081 in `mod.rs` — two-path model documented with escape hatch
- [X] T022 [US2] `test_iris_execute_description_documents_both_paths` passes
- [X] T023 [US2] `cargo clippy -- -D warnings` — zero warnings

**Checkpoint**: US2 complete.

---

## Phase 5: User Story 3 — Compile-and-run escape hatch validated (Priority: P2)

**Goal**: Confirm the documented `.mac` + `iris_compile` + `iris_execute` workflow succeeds end-to-end against live IRIS.

### Tests for User Story 3

- [X] T024 [US3] Write live IRIS integration test in `tests/integration/test_terminal_compat_096.rs` (`#[ignore]`, `--test-threads=1`): `test_compile_and_run_escape_hatch` — writes TERMTEST096.mac with `{}` block syntax, compiles, runs, asserts "compat_ok"
- [X] T025 [US3] Run T024 against live IRIS: `cargo test -p iris-agentic-dev-core --features testing --test test_terminal_compat_096 -- --include-ignored --test-threads=1 2>&1`

### Implementation for User Story 3

- [X] T026 [P] [US3] Escape hatch section already in `docs/tools.md` under `iris_execute` — three-step pattern with concrete example

**Checkpoint**: US3 complete — escape hatch is validated live and documented.

---

## Phase 6: Polish & Coverage

**Purpose**: Full test sweep, coverage gate, formatting and lint clean.

- [ ] T027 Run full unit test suite: `cargo test 2>&1 | tail -20` — confirm no regressions
- [ ] T028 Run integration/e2e suite: `cargo test -- --include-ignored --test-threads=1 2>&1 | tail -30` — all pass
- [X] T029 Run `cargo fmt --all -- --check` — no formatting diff
- [X] T030 Run `cargo clippy -- -D warnings` — zero warnings
- [ ] T031 **Coverage gate** (Constitution VIII — NON-NEGOTIABLE): `cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored 2>&1 | grep TOTAL` — assert TOTAL line coverage ≥ 90%
- [X] T032 Manual false-positive review: confirm zero false positives on dotted-DO syntax, `$ListBuild`, `$LB`, and multiline `For` without braces

---

## Dependencies & Execution Order

```
T001–T002 (baseline)
  → T003–T009 (Foundational: guard function + unit tests)
    → T010–T018 (US1: guard wired, binary + live IRIS tests)   [parallel with US2/US3]
    → T019–T023 (US2: description updated, binary test)         [parallel with US1/US3]
    → T024–T026 (US3: escape hatch validated live, doc)         [parallel with US1/US2]
      → T027–T032 (Polish: coverage gate, clean)
```
