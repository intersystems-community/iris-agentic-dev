# Tasks: HTTP-Native Unit Test Runner

**Input**: Design documents from `/specs/032-iris-test-http/`
**Repo**: `~/ws/iris-dev` (Rust — `crates/iris-dev-core`)
**Constitution**: Principle IV — unit tests (no IRIS) before implementation; E2E tests `#[ignore]` with `iris-dev-iris` container

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Add `TestParams` extension, new error codes, and test file stubs — no behavior change yet.

- [x] T001 Add `timeout: u64` field (default 60) to `TestParams` struct in `crates/iris-dev-core/src/tools/mod.rs` — `#[serde(default = "default_test_timeout")]` with helper returning 60u64
- [x] T002 Add new error codes `NO_TESTS_FOUND`, `NAMESPACE_NOT_FOUND`, `TEST_EXECUTION_ERROR` to `crates/iris-dev-core/src/tools/mod.rs` as string constants (or inline in err_json calls — document in data-model.md that they are registered)
- [x] T003 Create empty test file stubs: `crates/iris-dev-core/tests/unit/test_iris_test_http.rs` and `crates/iris-dev-core/tests/integration/test_iris_test_e2e.rs`
- [x] T004 Add `[[test]]` entries for both new test files to `crates/iris-dev-core/Cargo.toml`
- [x] T005 Verify `cargo check -p iris-dev-core` passes with new fields and stubs

**Checkpoint**: `cargo check` passes. New TestParams field compiles. Test stubs present.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Implement the SQL result-parsing logic and TestRun/TestSuite/TestCase struct builders — shared by all user stories. Test-first.

**Constitution IV note**: Phase 2 functions (`build_test_run_from_sql`, `map_status_int`, `build_test_detail`) are pure Rust data-transformation functions — they take structured row data as input and produce JSON. They make **no IRIS calls**. Unit tests (T006–T009) are the correct gate for Phase 2; the Phase 3 E2E test (T017) validates the full HTTP→SQL→JSON pipeline against live IRIS and serves as the combined Phase 2+3 E2E gate.

### Tests for Phase 2 (write first — must FAIL before implementation)

- [x] T006 [P] Write unit test: `build_test_run_from_sql()` with mock SQL rows for a passing suite → correct counts in `crates/iris-dev-core/tests/unit/test_iris_test_http.rs` (WRITE FIRST, must FAIL)
- [x] T007 [P] Write unit test: `build_test_run_from_sql()` with one failed test method → `success: false`, `failed: 1`, `failure_message` populated (WRITE FIRST, must FAIL)
- [x] T008 [P] Write unit test: `build_test_run_from_sql()` with empty rows → `total: 0`, `error_code: "NO_TESTS_FOUND"` (WRITE FIRST, must FAIL)
- [x] T009 [P] Write unit test: `map_status_int()` — maps `1` → `"passed"`, `0` → `"failed"`, other → `"error"` in `crates/iris-dev-core/tests/unit/test_iris_test_http.rs` (WRITE FIRST, must FAIL)

### TDD Gate

- [x] T010 **GATE**: Confirm T006–T009 all FAIL to compile (functions don't exist yet). Do not proceed until confirmed.

### Implementation for Phase 2

- [x] T011 Implement `map_status_int(status: i64) -> &'static str` helper in `crates/iris-dev-core/src/tools/mod.rs` — maps `1` → `"passed"`, `0` → `"failed"`, other → `"error"`
- [x] T012 Implement `build_test_run_from_sql(suites: Vec<SuiteRow>, methods: Vec<MethodRow>) -> serde_json::Value` in `crates/iris-dev-core/src/tools/mod.rs` — constructs TestRun JSON per data-model.md, applies `success` = all methods passed, sums counts, builds `test_suites` array (suite-level only, no per-case detail inline)
- [x] T013 Implement `build_test_detail(suites: Vec<SuiteRow>, methods: Vec<MethodRow>) -> serde_json::Value` — constructs full TestSuiteDetail with `test_cases` array for log store storage
- [x] T014 Verify T006–T009 now pass (`cargo test --test test_iris_test_http unit`)

**Checkpoint**: All foundational unit tests green. SQL parsing logic verified.

---

## Phase 3: User Story 1 — Run tests without docker, get structured results (Priority: P1) 🎯 MVP

**Goal**: `iris_test` works over HTTP when `IRIS_CONTAINER` is not set. Single call returns structured JSON with suite summaries + `log_id` for detail.

**Independent Test**: Unset `IRIS_CONTAINER`, run `iris_test(pattern="...", namespace="USER")` against `iris-dev-iris` with compiled test classes. Verify structured JSON in one call.

### Tests for US1 (write first — must FAIL before implementation)

- [x] T015 [P] [US1] Write unit test: `iris_test` handler with `IRIS_CONTAINER` unset and `iris=None` → returns `IRIS_UNREACHABLE` (not `DOCKER_REQUIRED`) in `crates/iris-dev-core/tests/unit/test_iris_test_http.rs` (WRITE FIRST, must FAIL)
- [x] T016 [P] [US1] Write unit test: HTTP path with mocked SQL response → returns `{success, total, passed, failed, path: "http", log_id, test_suites}` shape in `test_iris_test_http.rs` (WRITE FIRST, must FAIL)
- [x] T017 [US1] Write E2E test (`#[ignore]`): `IRIS_CONTAINER` unset, compile a simple `%UnitTest.TestCase` subclass into `iris-dev-iris`, run `iris_test` → verify `success: true`, `total > 0`, `log_id` present, `path: "http"` in `crates/iris-dev-core/tests/integration/test_iris_test_e2e.rs` (WRITE FIRST, must FAIL)

### TDD Gate

- [x] T018 [US1] **GATE**: Confirm T015–T017 all FAIL before writing any implementation below

### Implementation for US1

- [x] T019 [US1] Add HTTP execution path to `iris_test` handler in `crates/iris-dev-core/src/tools/mod.rs`: when `IRIS_CONTAINER` is not set, call `iris.execute_via_generator("do ##class(%UnitTest.Manager).RunTest(...)", namespace, client)` instead of `iris.execute()`
- [x] T020 [US1] Before calling `RunTest()`, generate a UUID correlation token. Pass it as the `userparam` argument to `RunTest()`: `do ##class(%UnitTest.Manager).RunTest(pattern, "/noload/run", correlationToken)`. After run completes, query TestInstance by UserParam: `SELECT ID FROM %UnitTest_Result.TestInstance WHERE UserParam = ?` via `iris.query()` in `mod.rs`. This avoids the race condition of "latest by ID" in concurrent environments.
- [x] T021 [US1] Query `%UnitTest.Result.TestSuite` and `%UnitTest.Result.TestMethod` for that TestInstance ID via `iris.query()` — two SQL calls in the iris_test handler in `mod.rs`
- [x] T022 [US1] Call `build_test_run_from_sql()` with the query results and build the inline response JSON; call `build_test_detail()` and store in log store via `self.log_store`; add `log_id` to response in `mod.rs`
- [x] T023 [US1] Handle `NO_TESTS_FOUND`: if TestInstance query returns 0 rows (pattern matched nothing), return `err_json("NO_TESTS_FOUND", ...)` in `mod.rs`
- [x] T024 [US1] **GATE-GREEN**: Run `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --test test_iris_test_e2e -- --ignored us1` — T017 must pass

**Phase gate**: T017 E2E passes. HTTP path returns structured JSON from `iris-dev-iris`.

---

## Phase 4: User Story 2 — Same tool works regardless of docker (Priority: P1)

**Goal**: `iris_test` auto-selects HTTP vs docker transparently. Same JSON shape from both paths. Docker failure triggers HTTP fallback.

**Independent Test**: Run with and without `IRIS_CONTAINER` set; verify identical response shape with correct `path` field.

### Tests for US2 (write first — must FAIL before implementation)

- [x] T025 [P] [US2] Write unit test: with `IRIS_CONTAINER` set, verify docker path is attempted first (mock docker success → `path: "docker"`) in `test_iris_test_http.rs` (WRITE FIRST, must FAIL)
- [x] T026 [P] [US2] Write unit test: docker exec fails → HTTP fallback triggered → `path: "http_fallback"` in `test_iris_test_http.rs` (WRITE FIRST, must FAIL)
- [x] T027 [US2] Write E2E test (`#[ignore]`): with `IRIS_CONTAINER=iris-dev-iris` set and container running, verify `path: "docker"` and same JSON shape as HTTP path in `test_iris_test_e2e.rs` (WRITE FIRST, must FAIL)

### TDD Gate

- [x] T028 [US2] **GATE**: Confirm T025–T027 all FAIL

### Implementation for US2

- [x] T029 [US2] Restructure `iris_test` handler in `mod.rs`: primary branch on `IRIS_CONTAINER` env var — if set, try docker path first; if docker path returns `DOCKER_REQUIRED` or other error, fall through to HTTP path with `path: "http_fallback"`
- [x] T030 [US2] Add new fields to docker path response in `mod.rs` **additively** (Constitution Principle V — no existing fields removed or renamed): add `path: "docker"`, `errors: 0`, `skipped: 0`, `duration_ms: null`, `log_id: null`, `test_suites: []` alongside existing `{passed, failed, total, output}` fields. Store docker path result in log store and populate `log_id`. Do NOT remove `output` field.
- [x] T031 [US2] **GATE-GREEN**: Run E2E T027 against `iris-dev-iris` with `IRIS_CONTAINER` set — must pass

**Phase gate**: T027 E2E passes. Both paths return identical JSON shape.

---

## Phase 5: User Story 3 — Agent can distinguish test failures from execution errors (Priority: P2)

**Goal**: `status: "error"` (unexpected exception) is distinct from `status: "failed"` (assertion). `NAMESPACE_NOT_FOUND` returned immediately for bad namespace.

**Independent Test**: Run `iris_test` with a bad namespace → `NAMESPACE_NOT_FOUND`. Run with a class that errors in `%OnBeforeOneTest` → `status: "error"` in test case.

### Tests for US3 (write first — must FAIL before implementation)

- [x] T032 [P] [US3] Write unit test: `build_test_run_from_sql()` with a method row where `ErrorAction` is populated and `Status != 1 and != 0` → `status: "error"` distinct from `"failed"` in `test_iris_test_http.rs` (WRITE FIRST, must FAIL)
- [x] T033 [P] [US3] Write unit test: namespace check returns false → handler returns `{error_code: "NAMESPACE_NOT_FOUND"}` immediately without calling RunTest in `test_iris_test_http.rs` (WRITE FIRST, must FAIL)
- [x] T034 [US3] Write E2E test (`#[ignore]`): call `iris_test` with `namespace="NONEXISTENT_NS_XYZ"` → verify `error_code: "NAMESPACE_NOT_FOUND"` in `test_iris_test_e2e.rs` (WRITE FIRST, must FAIL)

### TDD Gate

- [x] T035 [US3] **GATE**: Confirm T032–T034 all FAIL

### Implementation for US3

- [x] T036 [US3] Add namespace existence check before `RunTest()` in the HTTP path of `iris_test` handler in `mod.rs`: use `execute_via_generator` to run `Write ##class(%SYS.Namespace).Exists(namespace)` (returns `1` if exists, `0` if not); if result is `"0"`, return `err_json("NAMESPACE_NOT_FOUND", ...)`. Note: `%SYS.Namespace` table is only accessible from %SYS namespace — use the class method approach instead.
- [x] T037 [US3] Update `map_status_int()` in `mod.rs`: `Status=1` → `"passed"`, `Status=0` → `"failed"`, other + non-empty `ErrorAction` → `"error"`, other + empty `ErrorAction` → `"failed"` (edge case)
- [x] T038 [US3] **GATE-GREEN**: Run E2E T034 → must pass

**Phase gate**: T034 E2E passes. Error vs failure distinction confirmed.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [x] T039 [P] Update Constitution Principle III note in `crates/iris-dev-core/src/iris/constitution.md` (or wherever the iris-dev constitution exception was documented) — remove `iris_test` from the docker-required exceptions list since HTTP path is now default
- [x] T040 [P] Update `iris_test` tool description string in `mod.rs` — remove "Set IRIS_CONTAINER=<container_name> to enable" language; describe HTTP-first behavior and `IRIS_CONTAINER` as optional docker acceleration
- [x] T041 [P] Update README.md tool table — `iris_test` no longer marked with `✓ Needs Docker`; update description
- [x] T042 [P] Run full test suite: `cargo test -p iris-dev-core` — all unit tests pass (no regressions in test_toolset, interop_unit_tests, etc.)
- [x] T043 [P] Run `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all -- --check` — both clean
- [x] T044 [P] Run E2E full pass: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --test test_iris_test_e2e -- --ignored` — all 3 E2E tests pass
- [x] T045 Update issue #31 on GitHub with comment linking to PR and summarizing the fix

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — blocks all user story phases
- **Phase 3 (US1 — HTTP path)**: Depends on Phase 2
- **Phase 4 (US2 — path routing)**: Depends on Phase 3 (HTTP path must exist before routing can be added)
- **Phase 5 (US3 — error distinction)**: Depends on Phase 2; can start after T014 (SQL parsing logic done)
- **Phase 6 (Polish)**: Depends on all phases complete

### Critical Path

```
T001-T005 (setup) → T006-T014 (foundational SQL parsing) → T015-T024 (US1 HTTP path)
                                                          → T032-T038 (US3 — parallel with US1 after Phase 2)
T024 (US1 gate) → T025-T031 (US2 path routing)
T031 (US2 gate) → T039-T045 (polish)
```

### Parallel Opportunities

**Phase 2 tests** (all touch same file but are independent functions):
```
T006: build_test_run passing suite
T007: build_test_run failing test  
T008: build_test_run empty → NO_TESTS_FOUND
T009: map_status_int
```

**Phase 3+5 can overlap** after Phase 2 gate (T014 passes):
```
Developer A: Phase 3 (US1 — HTTP execution path)
Developer B: Phase 5 (US3 — error distinction, namespace check)
```

**Phase 6** all tasks are [P] — can all run in parallel.

---

## Implementation Strategy

### MVP: Phase 1 + 2 + 3 (HTTP path working)

1. Setup (Phase 1)
2. SQL parsing logic with unit tests (Phase 2)
3. HTTP execution path (Phase 3)
4. **STOP AND VALIDATE**: `iris_test` works against `iris-dev-iris` without docker. This is the fix for issue #31.

### Full Feature

5. Phase 4: Uniform path routing (docker + HTTP, same shape)
6. Phase 5: Error distinction + namespace check
7. Phase 6: Polish, README, constitution update
