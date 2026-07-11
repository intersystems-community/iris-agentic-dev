# Tasks: ObjectScript Line Coverage Tool (064)

**Input**: `specs/064-objectscript-coverage/spec.md` + `plan.md`
**Prerequisites**: spec.md ✅ plan.md ✅

---

## Phase 1: Core implementation

**Goal**: `iris_coverage` tool compiles, registers, dispatches.
**Phase gate**: `cargo build -p iris-agentic-dev-core` passes; tool appears in `registered_tool_names`.

- [ ] T001 Write unit tests for `build_routine_name`, `parse_coverage_output`, `IrisCoverageParams` deserialization, `MISSING_PARAM` validation, `parse_check_output("<FUNCTION>...")` → `BBSIZ_NOT_CONFIGURED`, and `expand_package_to_classes` SQL output parsing in `crates/iris-agentic-dev-core/tests/unit/test_coverage_unit.rs`
- [ ] T002 Implement `crates/iris-agentic-dev-core/src/tools/coverage.rs`: `IrisCoverageParams` struct, `build_routine_name()`, `build_coverage_code()`, `parse_coverage_output()`, `expand_package_to_classes()` (queries `%Dictionary.ClassDefinition` WHERE `Name %STARTSWITH pkg AND Abstract=0`), `handle_iris_coverage()` async fn
- [ ] T003 Wire `coverage` module into `crates/iris-agentic-dev-core/src/tools/mod.rs`: add `mod coverage; use coverage::*;`, add `iris_coverage` to `registered_tool_names`, add `iris_coverage` tool definition with `#[tool]` macro, add `iris_coverage` to `call_for_test` dispatch table
- [ ] T004 Add `[[test]]` entry for `test_coverage_unit` in `crates/iris-agentic-dev-core/Cargo.toml`
- [ ] T005 Run unit tests: `cargo test -p iris-agentic-dev-core --test test_coverage_unit`

**Phase gate checkpoint**: T005 passes → proceed to Phase 2.

---

## Phase 2: Integration tests (live IRIS)

**Goal**: Tool works against real IRIS. Covers both the happy path and pre-flight error path.
**Phase gate**: `live_coverage_check` passes (or returns expected `BBSIZ_NOT_CONFIGURED`).

- [ ] T006 Write live integration test `live_coverage_check_returns_ok_or_bbsiz_error` — calls `iris_coverage(mode="check")` and asserts result is either `{ok: true}` or `{error_code: "BBSIZ_NOT_CONFIGURED"}` in `crates/iris-agentic-dev-core/tests/integration/test_coverage_live.rs`
- [ ] T007 [P] Write live integration test `live_coverage_run_returns_structured_result` — calls `iris_coverage(mode="run", classes=["IrisDevTest.SqlPower"], namespace="USER", test_path="IrisDevTest.SqlPowerTest")`; asserts JSON has `total_pct`, `classes`, `meets_target` fields in same file
- [ ] T008 Add `[[test]]` entry for `test_coverage_live` in `crates/iris-agentic-dev-core/Cargo.toml` with `required-features = ["testing"]`
- [ ] T009 Run integration tests: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_ALLOW_PROD=1 cargo test -p iris-agentic-dev-core --features testing --test test_coverage_live -- --include-ignored`

**Phase gate checkpoint**: T009 passes (or BBSIZ_NOT_CONFIGURED is the only failure) → proceed to Phase 3.

---

## Phase 3: Lift evidence (required before release)

**Goal**: Demonstrate positive lift from `iris_coverage` vs baseline. Gate on spec §Lift Evidence Requirement.

- [ ] T010 Add benchmark task `coverage-001` to `crates/iris-agentic-dev-core/src/benchmark/tasks/coverage-001.json` — task: measure line coverage for `IrisDevTest.SqlPower` by running its compiled test suite; success criteria: JSON with `total_pct`, per-class breakdown, no `MonitorEnabled` hallucination, correct `/noload` flag
- [ ] T011 Run A/B lift measurement: baseline (agent uses raw ObjectScript, no `iris_coverage` tool) vs tool-assisted; record results in `specs/064-objectscript-coverage/lift-results.md`
- [ ] T012 Verify lift ≥ +0.20 on task success rate; document in `lift-results.md`; if below threshold investigate why and iterate on tool description or benchmark task design

## Phase 4: Polish

- [ ] T013 Update `README.md` Tools table — add `iris_coverage` row with description and modes
- [ ] T014 [P] `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` — fix any issues in `coverage.rs`
- [ ] T015 [P] Verify `iris-agentic-dev tool iris_coverage --args '{"mode":"check"}'` works end-to-end via the CLI bin
- [ ] T016 Run coverage gate: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo llvm-cov --summary-only -p iris-agentic-dev-core --features testing -- --include-ignored`; assert TOTAL line coverage ≥ 90% (constitution §VIII)

---

## Dependency graph

```text
Phase 1 (T001–T005): core build
  ↓
Phase 2 (T006–T009): live IRIS validation
  ↓
Phase 3 (T010–T012): lift evidence  ← required before release
  ↓
Phase 4 (T013–T016): polish + coverage gate
```

## Key implementation notes

- `build_routine_name(class: &str) -> String`: appends `.1` → `"MyApp.MyClass"` → `"MyApp.MyClass.1"`
- The generated ObjectScript must do start→run→stop→collect in ONE call to `execute_via_generator`
  (same-process requirement for `%ResultSet` to find the coverage data)
- `mode=check` calls `$zu(84,0,1,1,1,1,1,1)` — if it throws `<FUNCTION>` in IRIS output, return `BBSIZ_NOT_CONFIGURED` with fix instructions
- `mode=run` always calls `Stop()` first (idempotent, clears any stuck session)
- JSON output from ObjectScript: write single-line JSON to stdout, Rust parses with `serde_json::from_str`
- `execCount = -1` → non-executable line → skip (not in denominator)
- See `plan.md` for the full ObjectScript template
