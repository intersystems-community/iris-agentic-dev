# 087 — iris_execute gate bypass: tasks

## Phase 1: Unit tests (write first)

- [x] T001 Add `contains_global_kill` unit tests to `crates/iris-agentic-dev-core/tests/unit/test_gate_check.rs` — 11 tests cover true/false cases; comment-line treated as true (false positive safer than false negative)

## Phase 2: Implementation

- [x] T002 Add `pub fn contains_global_kill(code: &str) -> bool` to `crates/iris-agentic-dev-core/src/tools/write_gate.rs` — 22/22 unit tests pass
- [x] T003 In `crates/iris-agentic-dev-core/src/tools/mod.rs` `iris_execute` handler body, after role-gate check, before IRIS HTTP call: call `contains_global_kill`; if true and destructive gate off, return refusal with indirection disclaimer

## Phase 3: Binary invocation test (no live IRIS)

- [x] T004 Added `#[ignore]` binary tests in `crates/iris-agentic-dev-bin/tests/integration/test_exec_live.rs`: gate fires before IRIS call (bogus host); blocked and not-blocked cases both pass

## Phase 4: Live IRIS integration tests

- [x] T005 Added `#[ignore]` live-IRIS tests against `iris-dev-iris`: blocked case and allowed case both pass (4/4 tests green)

## Phase 5: Docs

- [x] T006 Updated `docs/tools.md` `iris_execute` entry with destructive-gate note and indirection disclaimer
- [x] T007 `cargo clippy -- -D warnings` clean; `cargo fmt --all -- --check` clean; 22/22 unit tests pass; 4/4 live tests pass

## Done criteria

- `cargo test` (no `--include-ignored`) green: unit tests for `contains_global_kill` pass ✓
- Binary test (T004) passes ✓
- Live IRIS tests (T005) pass against `iris-dev-iris` ✓
- `docs/tools.md` updated ✓
- clippy + fmt clean ✓
