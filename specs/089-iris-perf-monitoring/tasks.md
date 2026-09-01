# Tasks: IRIS Mirror Status and Database Free Space (089)

**Input**: Design documents from `/specs/089-iris-perf-monitoring/`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, contracts/ ✅

---

## Phase 1: Setup

**Purpose**: No new project structure needed — both features land in existing files.
Verify test infrastructure is in place.

- [ ] T001 Verify iris-dev-iris container running: `docker ps --filter name=iris-dev-iris`
- [ ] T002 Verify `cargo test` passes clean before any changes: `cargo test 2>&1 | tail -5`

---

## Phase 2: User Story 1 — iris_mirror_status (Priority: P1)

**Goal**: New read-only tool returns mirror topology for any IRIS instance.

**Independent Test**: Call `iris_mirror_status` on iris-dev-iris, assert `is_member=false`.

### Tests (write first — Layer 1 unit)

- [ ] T003 [US1] Write unit tests for `iris_mirror_status` JSON shape and normalization logic in `crates/iris-agentic-dev-core/tests/unit/test_perf_monitoring.rs` — cover: `is_member=false` shape, null normalization for "Not Member" and "", error shape `{error, is_member: null}`, all four fields present in member case
- [ ] T004 [US1] Run unit tests — confirm they FAIL (no implementation yet): `cargo test test_perf_monitoring 2>&1 | tail -20`

### Tests (Layer 2 — binary invocation, #[ignore])

- [ ] T005 [US1] Add binary invocation test to `crates/iris-agentic-dev-bin/tests/integration/test_exec_live.rs`: spawn IAD_BINARY, send `tools/list`, assert `iris_mirror_status` present

### Tests (Layer 3 — live IRIS, #[ignore])

- [ ] T006 [US1] Write live IRIS integration test in `crates/iris-agentic-dev-core/tests/integration/test_mirror_and_freespace.rs`: call `iris_mirror_status` on iris-dev-iris, assert `is_member=false`, `mirror_name=null`, `is_primary=false`

### Implementation

- [ ] T007 [US1] Add `iris_mirror_status` to tool list in `crates/iris-agentic-dev-core/src/tools/mod.rs` — tool registration, schema, no parameters
- [ ] T008 [US1] Implement `iris_mirror_status` handler in `crates/iris-agentic-dev-core/src/tools/mod.rs` — build ObjectScript for four `%SYSTEM.Mirror` classmethod calls in %SYS, post-process: null-normalize "Not Member" and "", return structured JSON
- [ ] T009 [US1] Run unit tests — confirm they now PASS: `cargo test test_perf_monitoring 2>&1 | tail -20`
- [ ] T010 [US1] Run binary invocation test (requires IAD_BINARY): `IAD_BINARY=./target/debug/iris-agentic-dev cargo test test_mirror_binary -- --include-ignored 2>&1 | tail -20`
- [ ] T011 [US1] Run live IRIS integration test — phase gate: `IRIS_HOST=localhost IRIS_PORT=52780 IRIS_USER=_SYSTEM IRIS_PASS=SYS cargo test test_mirror_live -- --include-ignored --test-threads=1 2>&1 | tail -30`

**Phase 2 Gate**: T011 must pass before Phase 3 begins.

---

## Phase 3: User Story 2 — iris_database_list Free Space (Priority: P2)

**Goal**: Extend existing `iris_database_list` with per-entry free space fields.

**Independent Test**: Call `iris_database_list`, assert `size_mb` present and numeric on iris-dev-iris.

### Tests (write first — Layer 1 unit)

- [ ] T012 [US2] Add unit tests to `crates/iris-agentic-dev-core/tests/unit/test_perf_monitoring.rs`: MaxSize string parsing ("Unlimited"→null, "500MB"→500, "2GB"→2048), graceful degradation shape (root `free_space_note` present), extended entry shape with all four new fields
- [ ] T013 [US2] Run unit tests — confirm new tests FAIL: `cargo test test_perf_monitoring 2>&1 | tail -20`

### Tests (Layer 3 — live IRIS, #[ignore])

- [ ] T014 [US2] Add live IRIS test to `crates/iris-agentic-dev-core/tests/integration/test_mirror_and_freespace.rs`: call `iris_database_list` on iris-dev-iris, assert ≥1 entry, assert `size_mb` is positive integer, `free_space_mb` is non-negative float, no `free_space_note` at root (query succeeds on this instance)

### Implementation

- [ ] T015 [US2] Add `FreeSpaceData` struct and `parse_max_size` helper in `crates/iris-agentic-dev-core/src/tools/mod.rs`
- [ ] T016 [US2] Implement `%SYS.DatabaseQuery:FreeSpace` query in `iris_database_list` handler in `crates/iris-agentic-dev-core/src/tools/mod.rs`: execute in %SYS, build `HashMap<String, FreeSpaceData>` keyed by `DatabaseName`, merge into existing DB entries, graceful degradation on error
- [ ] T017 [US2] Run unit tests — confirm all pass: `cargo test test_perf_monitoring 2>&1 | tail -20`
- [ ] T018 [US2] Run live IRIS integration test — phase gate: `IRIS_HOST=localhost IRIS_PORT=52780 IRIS_USER=_SYSTEM IRIS_PASS=SYS cargo test test_freespace_live -- --include-ignored --test-threads=1 2>&1 | tail -30`

**Phase 3 Gate**: T018 must pass before Phase 4 begins.

---

## Phase 4: Polish

- [ ] T019 [P] Run `cargo fmt --all` — no formatting diff
- [ ] T020 [P] Run `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings
- [ ] T021 Run `cargo clippy -p iris-agentic-dev-bin -- -D warnings` — zero warnings
- [ ] T022 Run full test suite: `cargo test 2>&1 | tail -10`
- [ ] T023 Update `docs/tools.md` — add `iris_mirror_status` entry; add note to `iris_database_list` about new free space fields
- [ ] T024 **Coverage gate (Constitution VIII — NON-NEGOTIABLE)**: `IRIS_HOST=localhost IRIS_PORT=52780 IRIS_USER=_SYSTEM IRIS_PASS=SYS cargo llvm-cov --summary-only -p iris-agentic-dev-core --features testing -- --include-ignored --test-threads=1` — confirm TOTAL line coverage ≥ 90%; if below, add tests for uncovered branches before marking complete

---

## Dependencies & Execution Order

- Phase 1 (setup): immediate
- Phase 2 (US1): after Phase 1; gate = live IRIS test T011
- Phase 3 (US2): after Phase 2 gate passes; gate = live IRIS test T018
- Phase 4 (polish): after Phase 3 gate passes

### Parallel Opportunities

- T003–T006 (US1 tests) can be written in parallel before implementation
- T012–T014 (US2 tests) can be written in parallel after Phase 2 gate passes
- T019–T021 (format/lint) can run in parallel in Phase 4

---

## Implementation Strategy

### MVP: Phase 2 Only (iris_mirror_status)

1. Phase 1 setup verification
2. Phase 2 complete — ship `iris_mirror_status`
3. Phase 3 adds free space to existing tool
4. Phase 4 polish + coverage gate

---

## Notes

- `--test-threads=1` required for all IRIS integration tests (race conditions)
- `--include-ignored` required to run `#[ignore]` live IRIS tests
- Binary invocation tests require `IAD_BINARY=./target/debug/iris-agentic-dev` env var and `cargo build` first
- Live IRIS credentials: `_SYSTEM`/`SYS` against localhost:52780
- `%SYSTEM.Mirror` verified: `IsMember()=0`, `GetMemberType()="Not Member"`, `MirrorName()=""`, `IsPrimary()=0` on iris-dev-iris
- `%SYS.DatabaseQuery:FreeSpace` verified: columns `DatabaseName`, `SizeInt`, `AvailableNum`, `Free`, `MaxSize` on iris-dev-iris
