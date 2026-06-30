# Tasks: System Observability Depth

**Input**: Design documents from `/specs/055-system-observability/`
**Prerequisites**: plan.md, spec.md (clarified 2026-06-29)

**Organization**: Tasks grouped by user story. US1=view_locks (P1), US2=view_processes
(P1), US3=journal_search (P2), US4=namespace_mappings (P2), US5=database_status (P2).
Phase 2 foundational wiring blocks all US phases.

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: New handler module skeleton, action arm stubs in `iris_admin` dispatcher,
error code registry additions.

**CRITICAL**: All user story phases depend on these — complete before any US work.

- [X] T001 Create `crates/iris-agentic-dev-core/src/tools/observability.rs` — empty module
      with five pub async fn stubs: `view_locks_impl`, `view_processes_impl`,
      `journal_search_impl`, `namespace_mappings_impl`, `database_status_impl`, each
      returning a `not_implemented` JSON response
- [X] T002 Add `pub mod observability;` to
      `crates/iris-agentic-dev-core/src/tools/mod.rs` and route the five new action strings
      (`"view_locks"`, `"view_processes"`, `"journal_search"`, `"namespace_mappings"`,
      `"database_status"`) in the `iris_admin` match dispatcher to the corresponding stubs
      in `observability.rs`
- [X] T003 Update the `iris_admin` tool schema description in
      `crates/iris-agentic-dev-core/src/tools/mod.rs` to include the five new action names
      in the allowed-actions list string. This covers FR-019: `check_config` reads the tool
      schema description directly, so updating it here propagates to `check_config` output
      automatically — no separate `check_config` change needed.
- [X] T004 Add `MISSING_PARAMS`, `NAMESPACE_NOT_FOUND`, and `DATABASE_NOT_FOUND` to the
      error code registry comment in `crates/iris-agentic-dev-core/src/policy/gate.rs`
- [X] T005 Run `cargo build -p iris-agentic-dev-core` — confirm clean compile with stubs

**Checkpoint**: Five stubs registered in `iris_admin` dispatcher, build is clean.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared helpers — `dataPolicy` guard for `view_processes`, journal bulk-PHI
hard-block, MISSING_PARAMS validation, namespace default resolution.

**CRITICAL**: Gate wiring and shared helpers must exist before any US implementation.

- [X] T006 Implement `fn require_data_policy_allow(data_policy: &str, action: &str) ->
      Option<Result<CallToolResult, McpError>>` in `observability.rs` — returns
      `Some(DATA_POLICY_BLOCKED)` when `data_policy != "allow"`; returns `None` to continue.
      Used by `view_processes` and `journal_search`.
- [X] T007 Implement `fn redact_process_entry(entry: &mut serde_json::Value)` in
      `observability.rs` — replaces `username`, `client_node_name`, `client_ip` fields
      with `"[REDACTED]"` in a single process JSON object (maps to SQL columns `UserName`,
      `ClientNodeName`, `ClientIPAddress`). Used by `view_processes` in redact mode.
- [X] T008 Implement `fn glob_to_sql_like(pattern: &str) -> String` in `observability.rs`
      — translates glob `*` to SQL `%` and `?` to `_`; escapes existing `%` and `_`
      literals. Used by `journal_search`.
- [X] T009 Implement `fn resolve_namespace(param: Option<&str>, connection_ns: &str) ->
      String` in `observability.rs` — returns `param` if Some and non-empty, else
      `connection_ns`. Used by `namespace_mappings`.
- [X] T010 Run `cargo test -p iris-agentic-dev-core` — confirm all pre-existing tests
      still pass after Phase 1 + 2 additions

**Checkpoint**: Shared helpers exist and compile; all pre-existing tests pass.

---

## Phase 3: User Story 1 — view_locks (Priority: P1)

**Goal**: Read the active IRIS lock table — resource, owner PID, type, mode, username.

**Independent Test**: Call `iris_admin action=view_locks` on a live IRIS. Expect
`{success: true, locks: [...], count: N}`. On a quiet instance, expect `count: 0`.

### Tests for US1

> Write FIRST. Must FAIL before T017.

- [X] T011 [US1] Create
      `crates/iris-agentic-dev-core/tests/unit/test_iris_admin_observability_unit.rs` —
      test `glob_to_sql_like`: `"IrisDevTest.*"` → `"IrisDevTest.%"`;
      `"^PAPMI?"` → `"^PAPMI_"`; `"100%off"` → `"100\%off"` (escaped literal %)
- [X] T012 [P] [US1] Add unit test — `view_locks` with no IRIS connection returns
      `IRIS_UNREACHABLE` (not a panic)
- [X] T013 [P] [US1] Add unit test — `view_locks` response with an empty lock list
      returns `{success: true, locks: [], count: 0}` (not an error)
- [X] T014 [P] [US1] Add unit test — `view_locks` passes through `dispatch_gate()`
      with `mcpTemplate=live` and succeeds (Query category permitted on live)
- [X] T015 [P] [US1] Add unit test — `view_locks` with `dataPolicy=block` does NOT
      return `DATA_POLICY_BLOCKED` (view_locks is not PHI-gated)
- [X] T016 [US1] Create
      `crates/iris-agentic-dev-core/tests/integration/test_iris_admin_observability_live.rs`
      — `#[ignore]`; call `iris_admin action=view_locks`; assert `success: true`;
      assert `locks` array is present; assert each entry has `resource`, `owner_pid`,
      `lock_type`, `lock_mode` keys

### Implementation for US1

- [X] T017 [US1] Implement `view_locks_impl` in `observability.rs`:
  - Call `dispatch_gate()` for `iris_admin`
  - Execute via `%SYS.LockQuery:Detail` class query (NOT SQL — no SQL table exists;
    verified on IRIS 2026.2). ObjectScript: `Set rs=##class(%ResultSet).%New("%SYS.LockQuery:Detail") Do rs.Execute()`
  - Verify column names at impl time via `rs.GetColumnName(i)` loop; expected: `Name`,
    `Owner`, `Type`, `Mode`, `OwnerName`
  - Map rows to `LockEntry` shape: `resource`(Name), `owner_pid`(Owner),
    `lock_type`(Type), `lock_mode`(Mode), `owner_username`(OwnerName)
  - Return `{success: true, locks: [...], count: N}`
- [X] T018 [US1] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
      all US1 unit tests must pass

**Checkpoint**: US1 complete. `view_locks` returns lock entries or empty array.

---

## Phase 4: User Story 2 — view_processes (Priority: P1)

**Goal**: List all active IRIS processes; apply `dataPolicy` block/redact/allow.

**Independent Test**: Call `iris_admin action=view_processes` with `dataPolicy=allow`. Expect
`processes` array with `pid`, `username`, `namespace`, `state`. Call with `dataPolicy=block`
— expect `DATA_POLICY_BLOCKED`.

### Tests for US2

> Write FIRST. Must FAIL before T025.

- [X] T019 [P] [US2] Add unit test — `redact_process_entry` replaces `username`,
      `client_name`, `client_ip` with `"[REDACTED]"` and leaves `pid`, `namespace`,
      `state`, `routine` unchanged
- [X] T020 [P] [US2] Add unit test — `view_processes` with `dataPolicy=block` returns
      `DATA_POLICY_BLOCKED` before any IRIS call (mock gate returns allow, check the
      dataPolicy guard fires)
- [X] T021 [P] [US2] Add unit test — `view_processes` with `dataPolicy=redact` returns
      processes with `username="[REDACTED]"` and `pid` intact (use mock IRIS response)
- [X] T022 [P] [US2] Add unit test — `view_processes` with `dataPolicy=allow` returns
      full process entries with all fields (use mock IRIS response)
- [X] T023 [P] [US2] Add unit test — `view_processes` with optional `namespace="%SYS"`
      filter produces ObjectScript/SQL that includes the namespace condition (check the
      generated query string or mock output)
- [X] T024 [US2] Add integration test to `test_iris_admin_observability_live.rs` —
      `#[ignore]`; call `view_processes` with `dataPolicy=allow`; assert `success: true`;
      assert at least one process entry; assert `pid` is numeric

### Implementation for US2

- [X] T025 [US2] Implement `view_processes_impl` in `observability.rs`:
  - Call `dispatch_gate()`
  - Check `dataPolicy` via `require_data_policy_allow`; if `block` → return
    `DATA_POLICY_BLOCKED`
  - Build SQL (verified column names on IRIS 2026.2):
    `SELECT Pid, UserName, NameSpace, State, ClientNodeName, ClientIPAddress, Routine
    FROM %SYS.ProcessQuery ORDER BY Pid`
    with optional `WHERE NameSpace = :ns` (note: `NameSpace` not `Namespace`)
  - Execute in `%SYS` namespace
  - If `dataPolicy == redact`: call `redact_process_entry` on each entry
    (redacts `UserName`, `ClientNodeName`, `ClientIPAddress`)
  - Return `{success: true, processes: [...], count: N}`
- [X] T026 [US2] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
      all US1+US2 unit tests pass

**Checkpoint**: US2 complete. `view_processes` block/redact/allow all correct.

---

## Phase 5: User Story 3 — journal_search (Priority: P2)

**Goal**: Search IRIS journal by global pattern and/or time range; hard-blocked unless
`dataPolicy=allow`; executes in `%SYS`.

**Independent Test**: Call `iris_admin action=journal_search global_pattern="IrisDevTest.*"
time_range={"from":"2026-06-29T00:00:00Z","to":"2026-06-30T00:00:00Z"}` with
`dataPolicy=allow`. Expect `records` array (may be empty). Call with no filters — expect
`MISSING_PARAMS`.

### Tests for US3

> Write FIRST. Must FAIL before T034.

- [X] T027 [P] [US3] Add unit test — `journal_search` with no `global_pattern` and no
      `time_range` returns `MISSING_PARAMS`
- [X] T028 [P] [US3] Add unit test — `journal_search` with `dataPolicy=block` returns
      `DATA_POLICY_BLOCKED` even when `acknowledgePhi=true` is in params
- [X] T029 [P] [US3] Add unit test — `journal_search` with `dataPolicy=redact` returns
      `DATA_POLICY_BLOCKED` (not `allow` → blocked)
- [X] T030 [P] [US3] Add unit test — `journal_search` with `max_records=5000` is treated
      as 1000 in the generated query; response sets `truncated: true` when result equals cap
- [X] T031 [P] [US3] Add unit test — `journal_search` with `global_pattern="IrisDevTest.*"`
      only (no `time_range`) is valid; query does not include time filter
- [X] T032 [P] [US3] Add unit test — `journal_search` with `time_range` only (no
      `global_pattern`) is valid; query does not include LIKE filter
- [X] T033 [US3] Add integration test to `test_iris_admin_observability_live.rs` —
      `#[ignore]`; call `journal_search` with `dataPolicy=allow` and
      `global_pattern="IrisDevTest.%"`; assert `success: true`; assert each record has
      `global_ref`, `timestamp`, `operation` keys

### Implementation for US3

- [X] T034 [US3] Implement `journal_search_impl` in `observability.rs`:
  - Validate: require at least one of `global_pattern` or `time_range`; else
    `MISSING_PARAMS`
  - Check `dataPolicy == allow` via `require_data_policy_allow`; `acknowledgePhi` does
    NOT bypass
  - Parse `max_records` (default 100, clamp to 1000)
  - **NOTE**: `%SYS.Journal.Record` has NO SQL table (verified IRIS 2026.2). Use the
    `%SYS.Journal.File:Search` named class query. Verify its column names at impl time
    with `rs.GetColumnName(i)` — `GlobalReference` is NOT a property of the Record class.
    See research.md for details.
  - `glob_to_sql_like` helper still used to translate `global_pattern` for string
    matching within the Search query filter
  - Always execute in `%SYS` namespace
  - Map results to `JournalRecord` shape: `global_ref`, `timestamp`, `operation`,
    `transaction_id` (column names TBD at impl from Search query metadata)
  - If result count equals cap, set `truncated: true`
  - Return `{success: true, records: [...], count: N, truncated: bool}`
- [X] T035 [US3] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
      all US1–US3 unit tests pass

**Checkpoint**: US3 complete. `journal_search` filters, bulk-PHI guard, and clamping correct.

---

## Phase 6: User Story 4 — namespace_mappings (Priority: P2)

**Goal**: Return global, package, and routine mappings for a namespace; `NAMESPACE_NOT_FOUND`
for non-existent namespace.

**Independent Test**: Call `iris_admin action=namespace_mappings namespace="USER"`. Expect
`mappings` with `globals`, `packages`, `routines` sub-arrays. Call with a non-existent
namespace — expect `NAMESPACE_NOT_FOUND`.

### Tests for US4

> Write FIRST. Must FAIL before T041.

- [X] T036 [P] [US4] Add unit test — `resolve_namespace` returns provided param when
      non-empty; returns `connection_ns` when param is absent
- [X] T037 [P] [US4] Add unit test — `namespace_mappings` with omitted `namespace` param
      uses the connection's active namespace as default
- [X] T038 [P] [US4] Add unit test — `namespace_mappings` for a non-existent namespace
      returns `NAMESPACE_NOT_FOUND` (not a raw IRIS error or panic)
- [X] T039 [P] [US4] Add unit test — `namespace_mappings` response shape contains
      `mappings.globals`, `mappings.packages`, `mappings.routines` sub-arrays (use mock
      IRIS response with one entry each)
- [X] T040 [US4] Add integration test to `test_iris_admin_observability_live.rs` —
      `#[ignore]`; call `namespace_mappings namespace="USER"`; assert `success: true`;
      assert `mappings` has `globals`, `packages`, `routines` keys

### Implementation for US4

- [X] T041 [US4] Implement `namespace_mappings_impl` in `observability.rs`:
  - Call `dispatch_gate()`
  - Resolve namespace via `resolve_namespace`
  - Query with verified column names (all three tables use `Database`, not
    `GlobalDatabase`/`PackageDatabase`/`RoutineDatabase`):
    `SELECT Name, Database FROM Config.MapGlobals WHERE Namespace = :ns`
    `SELECT Name, Database FROM Config.MapPackages WHERE Namespace = :ns`
    `SELECT Name, Database FROM Config.MapRoutines WHERE Namespace = :ns`
  - Existence check: `SELECT Name FROM Config.Namespaces WHERE Name = :ns`
    — SQLCODE 100 → `NAMESPACE_NOT_FOUND`
  - If all three mapping queries return empty AND namespace not in `Config.Namespaces`,
    return `NAMESPACE_NOT_FOUND`
  - Return `{success: true, namespace: ..., mappings: {globals: [...], packages: [...],
    routines: [...]}}`
- [X] T042 [US4] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
      all US1–US4 unit tests pass

**Checkpoint**: US4 complete. `namespace_mappings` returns mappings; `NAMESPACE_NOT_FOUND`
on missing namespace.

---

## Phase 7: User Story 5 — database_status (Priority: P2)

**Goal**: Per-database mount state, free space, journal, mirror info; optional name filter;
`DATABASE_NOT_FOUND` when name filter matches nothing.

**Independent Test**: Call `iris_admin action=database_status`. Expect `databases` array
with at least one entry containing `name`, `mounted`, `free_space_mb`, `journal_state`,
`mirror_state`.

### Tests for US5

> Write FIRST. Must FAIL before T048.

- [X] T043 [P] [US5] Add unit test — `database_status` response shape has `databases`
      array; each entry has `name`, `directory`, `mounted`, `free_space_mb`,
      `journal_state`, `mirror_state` (use mock response)
- [X] T044 [P] [US5] Add unit test — `database_status` with name filter returns
      `DATABASE_NOT_FOUND` when no match (use mock empty response)
- [X] T045 [P] [US5] Add unit test — `database_status` `mirror_state` is `"none"` (not
      null) when the IRIS response has no mirror status column value
- [X] T046 [P] [US5] Add unit test — `database_status` with `mounted: false` entry does
      not include `free_space_mb` (or sets it to `null`), not a crash
- [X] T047 [US5] Add integration test to `test_iris_admin_observability_live.rs` —
      `#[ignore]`; call `database_status`; assert `success: true`; assert at least one
      database entry; assert `name`, `mounted`, `mirror_state` keys present

### Implementation for US5

- [X] T048 [US5] Implement `database_status_impl` in `observability.rs`:
  - Call `dispatch_gate()`
  - **NOTE**: `SYS.Database` has NO SQL table (verified IRIS 2026.2). Use class queries:
    `SYS.Database:FreeSpace` (runtime status, free space) and `SYS.Database:List`
    (mirror/encryption state). Execute `rs.Execute("*")` for all or `rs.Execute(name)`
    for single database filter.
  - `FreeSpace` columns (verified): `DatabaseName`, `Directory`, `Status`,
    `AvailableNum` (free MB as float), `DiskFreeSpaceNum`, `ReadOnly`
  - `List` columns (verified): `Directory`, `Status`, `Mirrored`, `SFN`
  - Map: `DatabaseName`→`name`, `Status` contains `"Mounted"`→`mounted: bool`,
    `AvailableNum`→`free_space_mb`, `Mirrored=="0"`→`mirror_state: "none"`
  - If name filter provided and FreeSpace result is empty, return `DATABASE_NOT_FOUND`
  - Return `{success: true, databases: [...], count: N}`
- [X] T049 [US5] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
      all US1–US5 unit tests pass

**Checkpoint**: US5 complete. `database_status` returns correct shape; name filter works;
`mirror_state` always a string.

---

## Phase 8: Polish and Cross-Cutting Concerns

**Purpose**: Integration tests, AGENTS.md update, check_config schema, final fmt/clippy.

- [X] T050 Add integration test to `test_iris_admin_observability_live.rs` — `#[ignore]`;
      call `view_processes` with `dataPolicy=allow` on `mcpTemplate=live` config; assert
      `success: true` (confirms live-template + Query category = permitted)
- [X] T051 [P] Add integration test — `journal_search` with `dataPolicy=block` and
      `acknowledgePhi=true` returns `DATA_POLICY_BLOCKED` (confirms hard-block, no bypass)
- [X] T052 [P] Add integration test — `namespace_mappings namespace="NonExistentNS9999"`
      returns `NAMESPACE_NOT_FOUND`
- [X] T053 [P] Verify `MISSING_PARAMS`, `NAMESPACE_NOT_FOUND`, `DATABASE_NOT_FOUND` appear
      in error code registry comment in `gate.rs`
- [X] T054 [P] Update `light-skills/AGENTS.md` — add five new `iris_admin` actions to the
      MCP tool reference section with usage examples; add three new error codes to Section 6
- [X] T055 Run full test suite: `cargo test -p iris-agentic-dev-core` — all non-ignored
      tests pass, zero regressions
- [X] T056 Run `cargo fmt --all -- --check` — no formatting diff
- [X] T057 Run `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings
- [X] T058 [P] Update spec status to `Status: Implemented` in
      `specs/055-system-observability/spec.md`

---

## Dependencies and Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 (needs `observability.rs` to exist)
- **Phase 3 (US1 view_locks)**: Depends on Phase 2; lowest-risk, start here
- **Phase 4 (US2 view_processes)**: Depends on Phase 2; can run parallel with Phase 3
- **Phase 5 (US3 journal_search)**: Depends on Phase 2 (needs `glob_to_sql_like`); can
  run parallel with Phases 3–4
- **Phase 6 (US4 namespace_mappings)**: Depends on Phase 2 (needs `resolve_namespace`);
  can run parallel with Phases 3–5
- **Phase 7 (US5 database_status)**: Depends on Phase 2; can run parallel with Phases 3–6
- **Phase 8 (Polish)**: Depends on all US phases complete

### User Story Dependencies

All five user stories depend only on Phase 2 and are mutually independent.

### Within Each Phase

- Tests written FIRST, must FAIL before implementation task runs
- Shared helpers (Phase 2) must exist before any action implementation
- `dispatch_gate()` must be called in every action before any IRIS call

### Parallel Opportunities

- T011–T015 (US1 unit tests) — T012–T015 parallel after T011 creates the file
- T019–T023 (US2 unit tests) — all parallel (appending to existing file)
- T027–T032 (US3 unit tests) — all parallel
- T036–T039 (US4 unit tests) — all parallel
- T043–T046 (US5 unit tests) — all parallel
- T050–T053 (Polish) — all parallel after integration test file exists

---

## Implementation Strategy

### MVP First (US1 + US2 only — the two P1 stories)

1. Complete Phase 1: Setup (T001–T005)
2. Complete Phase 2: Foundational (T006–T010)
3. Complete Phase 3: US1 view_locks (T011–T018)
4. Complete Phase 4: US2 view_processes (T019–T026)
5. **STOP and VALIDATE**: `cargo test test_iris_admin_observability` green; live IRIS shows
   locks and processes
6. Ship MVP — operational triage is the highest-value scenario

### Incremental Delivery

1. Setup + Foundational → registered stubs, helpers ready
2. US1 view_locks → lock table reads
3. US2 view_processes → process list with dataPolicy gating
4. US3 journal_search → journal search with bulk-PHI guard
5. US4 namespace_mappings → namespace config inspection
6. US5 database_status → per-database health info
7. Polish → integration coverage, docs, fmt/clippy

---

## Notes

- All five APIs verified on IRIS 2026.2 (see research.md). Key findings:
  `%SYS.LockQuery` and `SYS.Database` are NOT SQL tables — use class queries.
  `%SYS.Journal.Record` has no SQL table and `GlobalReference` is not a property —
  use `%SYS.Journal.File:Search` named query, verify columns at impl time.
  `%SYS.ProcessQuery` SQL confirmed; column corrections: `UserName`, `NameSpace`,
  `ClientNodeName` (not `Username`, `Namespace`, `ClientName`).
  `Config.MapGlobals/Packages/Routines` SQL confirmed; all use `Database` column
  (not `GlobalDatabase`/`PackageDatabase`/`RoutineDatabase`).
- `view_locks` may return an empty result on a quiet development IRIS instance — the
  integration test asserts only that the response shape is correct, not that locks > 0.
- `journal_search` requires `%SYS` access; if the connection user does not have this, the
  IRIS HTTP endpoint returns a `<PROTECT>` error surfaced as `IRIS_EXECUTE_ERROR`. Document
  this in the tool schema description.
- `database_status` `FreeBD` column is in database blocks; multiply by block size (typically
  8192 bytes) to get MB. If block size is not readily available, return raw block count as
  `free_space_blocks` and document the conversion.
