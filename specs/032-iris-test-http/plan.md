# Implementation Plan: HTTP-Native Unit Test Runner

**Branch**: `032-iris-test-http` | **Date**: 2026-05-07 | **Spec**: `specs/032-iris-test-http/spec.md`  
**Input**: Feature specification from `/specs/032-iris-test-http/spec.md`

## Summary

Enhance `iris_test` to work over pure Atelier REST (no docker required) by:
1. Running `%UnitTest.Manager.RunTest()` via `execute_via_generator`
2. Querying `%UnitTest.Result.*` SQL tables for structured results
3. Applying progressive disclosure (log store) for full per-method detail
4. Preserving docker exec path unchanged; HTTP becomes default when `IRIS_CONTAINER` unset

**Key research finding**: `/junit` qualifier is NOT reliably available — direct SQL query of `%UnitTest.Result.*` tables is the correct approach. `^UnitTestRoot` is a path string, not a results global.

## Technical Context

**Language/Version**: Rust 1.92 (`crates/iris-dev-core`)  
**Primary Dependencies**: No new crates. Uses existing: `serde_json`, `reqwest`, `uuid`, `log_store` module (027)  
**Storage**: In-process log store (027) for full test detail  
**Testing**: `cargo test` — unit tests (no IRIS), integration tests (`#[ignore]`)  
**Target Platform**: All (macOS arm64/x86_64, Linux x86_64, Windows x86_64)  
**Performance Goals**: Test runs up to 50 suites return results within configured timeout (default 60s)  
**Constraints**: No new crate dependencies; must not break existing docker exec path

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Zero-Install Binary | ✅ PASS | No new crates; pure HTTP + existing execute_via_generator |
| II. ObjectScript Sanity | ✅ PASS | All APIs verified against live IRIS: `%UnitTest.Manager.RunTest()`, `%UnitTest.Result.*` tables, status integer mapping |
| III. HTTP-First Execution | ✅ PASS | HTTP is now the default path; docker exec is fallback |
| IV. Test-First, Fixture-Driven | ✅ PASS — gated | Unit tests written first; E2E tests `#[ignore]` with `iris-dev-iris` container |
| V. Output Shape Parity | ✅ PASS | Docker and HTTP paths return identical JSON shape; `path` field distinguishes |
| VI. Environment Guard | ✅ N/A | iris_test is read-only (runs tests, queries results) |
| VII. Dependency Minimalism | ✅ PASS | Zero new crate dependencies |

**Constitution exception update**: Principle III previously noted `iris_test` as the sole docker-required exception. This feature removes that exception — `iris_test` now works over HTTP-first. Constitution Principle III note should be updated to reflect this.

## Project Structure

### Documentation

```text
specs/032-iris-test-http/
├── plan.md              # This file
├── research.md          # API verification, design decisions
├── data-model.md        # TestRun/TestSuite/TestCase entities, error codes
├── quickstart.md        # Usage examples
├── contracts/
│   └── iris_test.md     # Full request/response contract
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code

```text
crates/iris-dev-core/src/tools/
└── mod.rs               # iris_test handler — add HTTP path, SQL query, log store integration

crates/iris-dev-core/tests/
├── unit/
│   └── test_iris_test_http.rs   # NEW — unit tests (no IRIS): SQL parsing, result shape
└── integration/
    └── test_iris_test_e2e.rs    # NEW — #[ignore] E2E tests against iris-dev-iris
```

**Structure Decision**: Single-file change in `mod.rs` (iris_test handler). No new modules needed — reuses `iris.query()` for SQL, `execute_via_generator()` for running tests, existing log store integration pattern from iris_compile.

## Complexity Tracking

| Decision | Why Needed | Simpler Alternative Rejected Because |
|----------|------------|-------------------------------------|
| SQL query of `%UnitTest.Result.*` instead of `/junit` | `/junit` qualifier not reliably available across IRIS versions | jUnit file write + parse: version-dependent, requires file I/O round-trip |
| Progressive disclosure always stores (no threshold) | Even small test runs benefit from per-method drill-down | Threshold-based: inconsistent — agents need detail when tests fail regardless of suite size |
| `TestInstance` ID via `ORDER BY %ID DESC TOP 1` | Need to identify which result set belongs to this run | Passing userparam through RunTest: fragile across IRIS versions |
