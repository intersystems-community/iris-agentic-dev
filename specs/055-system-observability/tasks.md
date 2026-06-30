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

- [ ] T001 Create `crates/iris-agentic-dev-core/src/tools/observability.rs` — empty module
      with five pub async fn stubs: `view_locks_impl`, `view_processes_impl`,
      `journal_search_impl`, `namespace_mappings_impl`, `database_status_impl`, each
      returning a `not_implemented` JSON response
- [ ] T002 Add `pub mod observability;` to
      `crates/iris-agentic-dev-core/src/tools/mod.rs` and route the five new action strings
      (`"view_locks"`, `"view_processes"`, `"journal_search"`, `"namespace_mappings"`,
      `"database_status"`) in the `iris_admin` match dispatcher to the corresponding stubs
      in `observability.rs`
- [ ] T003 Update the `iris_admin` tool schema description in
      `crates/iris-agentic-dev-core/src/tools/mod.rs` to include the five new action names
      in the allowed-actions list string (so `check_config` and tool introspection show them)
- [ ] T004 Add `MISSING_PARAMS`, `NAMESPACE_NOT_FOUND`, and `DATABASE_NOT_FOUND` to the
      error code registry comment in `crates/iris-agentic-dev-core/src/policy/gate.rs`
- [ ] T005 Run `cargo build -p iris-agentic-dev-core` — confirm clean compile with stubs

**Checkpoint**: Five stubs registered in `iris_admin` dispatcher, build is clean.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared helpers — `dataPolicy` guard for `view_processes`, journal bulk-PHI
hard-block, MISSING_PARAMS validation, namespace default resolution.

**CRITICAL**: Gate wiring and shared helpers must exist before any US implementation.

- [ ] T006 Implement `fn require_data_policy_allow(data_policy: &str, action: &str) ->
      Option<Result<CallToolResult, McpError>>` in `observability.rs` — returns
      `Some(DATA_POLICY_BLOCKED)` when `data_policy != "allow"`; returns `None` to continue.
      Used by `view_processes` and `journal_search`.
- [ ] T007 Implement `fn redact_process_entry(entry: &mut serde_json::Value)` in
      `observability.rs` — replaces `username`, `client_name`, `client_ip` fields with
      `"[REDACTED]"` in a single process JSON object. Used by `view_processes` in redact
      mode.
- [ ] T008 Implement `fn glob_to_sql_like(pattern: &str) -> String` in `observability.rs`
      — translates glob `*` to SQL `%` and `?` to `_`; escapes existing `%` and `_`
      literals. Used by `journal_search`.
- [ ] T009 Implement `fn resolve_namespace(param: Option<&str>, connection_ns: &str) ->
      String` in `observability.rs` — returns `param` if Some and non-empty, else
      `connection_ns`. Used by `namespace_mappings`.
- [ ] T010 Run `cargo test -p iris-agentic-dev-core` — confirm all pre-existing tests
      still pass after Phase 1 + 2 additions

**Checkpoint**: Shared helpers exist and compile; all pre-existing tests pass.

---

## Phase 3: User Story 1 — view_locks (Priority: P1)

**Goal**: Read the active IRIS lock table — resource, owner PID, type, mode, username.

**Independent Test**: Call `iris_admin action=view_locks` on a live IRIS. Expect
`{success: true, locks: [...], count: N}`. On a quiet instance, expect `count: 0`.

### Tests for US1

> Write FIRST. Must FAIL before T017.

- [ ] T011 [US1] Create
      `crates/iris-agentic-dev-core/tests/unit/test_iris_admin_observability_unit.rs` —
      test `glob_to_sql_like`: `"IrisDevTest.*"` → `"IrisDevTest.%"`;
      `"^PAPMI?"` → `"^PAPMI_"`; `"100%off"` → `"100\%off"` (escaped literal %)
- [ ] T012 [P] [US1] Add unit test — `view_locks` with no IRIS connection returns
      `IRIS_UNREACHABLE` (not a panic)
- [ ] T013 [P] [US1] Add unit test — `view_locks` response with an empty lock list
      returns `{success: true, locks: [], count: 0}` (not an error)
- [ ] T014 [P] [US1] Add unit test — `view_locks` passes through `dispatch_gate()`
      with `mcpTemplate=live` and succeeds (Query category permitted on live)
- [ ] T015 [P] [US1] Add unit test — `view_locks` with `dataPolicy=block` does NOT
      return `DATA_POLICY_BLOCKED` (view_locks is not PHI-gated)
- [ ] T016 [US1] Create
      `crates/iris-agentic-dev-core/tests/integration/test_iris_admin_observability_live.rs`
      — `#[ignore]`; call `iris_admin action=view_locks`; assert `success: true`;
      assert `locks` array is present; assert each entry has `resource`, `owner_pid`,
      `lock_type`, `lock_mode` keys

### Implementation for US1

- [ ] T017 [US1] Implement `view_locks_impl` in `observability.rs`:
  - Call `dispatch_gate()` for `iris_admin`
  - Execute SQL against `%SYS` namespace: `SELECT Resource, Owner, LockType, LockMode,
    OwnerName FROM %SYS.LockQuery ORDER BY Resource` (or equivalent system table)
  - Map rows to `LockEntry` shape
  - Return `{success: true, locks: [...], count: N}`
- [ ] T018 [US1] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
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

- [ ] T019 [P] [US2] Add unit test — `redact_process_entry` replaces `username`,
      `client_name`, `client_ip` with `"[REDACTED]"` and leaves `pid`, `namespace`,
      `state`, `routine` unchanged
- [ ] T020 [P] [US2] Add unit test — `view_processes` with `dataPolicy=block` returns
      `DATA_POLICY_BLOCKED` before any IRIS call (mock gate returns allow, check the
      dataPolicy guard fires)
- [ ] T021 [P] [US2] Add unit test — `view_processes` with `dataPolicy=redact` returns
      processes with `username="[REDACTED]"` and `pid` intact (use mock IRIS response)
- [ ] T022 [P] [US2] Add unit test — `view_processes` with `dataPolicy=allow` returns
      full process entries with all fields (use mock IRIS response)
- [ ] T023 [P] [US2] Add unit test — `view_processes` with optional `namespace="%SYS"`
      filter produces ObjectScript/SQL that includes the namespace condition (check the
      generated query string or mock output)
- [ ] T024 [US2] Add integration test to `test_iris_admin_observability_live.rs` —
      `#[ignore]`; call `view_processes` with `dataPolicy=allow`; assert `success: true`;
      assert at least one process entry; assert `pid` is numeric

### Implementation for US2

- [ ] T025 [US2] Implement `view_processes_impl` in `observability.rs`:
  - Call `dispatch_gate()`
  - Check `dataPolicy` via `require_data_policy_allow`; if `block` → return
    `DATA_POLICY_BLOCKED`
  - Build SQL: `SELECT Pid, Username, Namespace, State, ClientName, ClientIPAddress,
    Routine FROM %SYS.ProcessQuery` with optional `WHERE Namespace = :ns`
  - Execute in `%SYS` namespace
  - If `dataPolicy == redact`: call `redact_process_entry` on each entry
  - Return `{success: true, processes: [...], count: N}`
- [ ] T026 [US2] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
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

- [ ] T027 [P] [US3] Add unit test — `journal_search` with no `global_pattern` and no
      `time_range` returns `MISSING_PARAMS`
- [ ] T028 [P] [US3] Add unit test — `journal_search` with `dataPolicy=block` returns
      `DATA_POLICY_BLOCKED` even when `acknowledgePhi=true` is in params
- [ ] T029 [P] [US3] Add unit test — `journal_search` with `dataPolicy=redact` returns
      `DATA_POLICY_BLOCKED` (not `allow` → blocked)
- [ ] T030 [P] [US3] Add unit test — `journal_search` with `max_records=5000` is treated
      as 1000 in the generated query; response sets `truncated: true` when result equals cap
- [ ] T031 [P] [US3] Add unit test — `journal_search` with `global_pattern="IrisDevTest.*"`
      only (no `time_range`) is valid; query does not include time filter
- [ ] T032 [P] [US3] Add unit test — `journal_search` with `time_range` only (no
      `global_pattern`) is valid; query does not include LIKE filter
- [ ] T033 [US3] Add integration test to `test_iris_admin_observability_live.rs` —
      `#[ignore]`; call `journal_search` with `dataPolicy=allow` and
      `global_pattern="IrisDevTest.%"`; assert `success: true`; assert each record has
      `global_ref`, `timestamp`, `operation` keys

### Implementation for US3

- [ ] T034 [US3] Implement `journal_search_impl` in `observability.rs`:
  - Validate: require at least one of `global_pattern` or `time_range`; else
    `MISSING_PARAMS`
  - Check `dataPolicy == allow` via `require_data_policy_allow`; `acknowledgePhi` does
    NOT bypass
  - Parse `max_records` (default 100, clamp to 1000)
  - Translate `global_pattern` via `glob_to_sql_like`
  - Build SQL against `%SYS.Journal.Record` with `global_pattern` LIKE and/or timestamp
    BETWEEN filters; always override namespace to `%SYS`
  - Execute, map rows to `JournalRecord` shape
  - If result count equals cap, set `truncated: true`
  - Return `{success: true, records: [...], count: N, truncated: bool}`
- [ ] T035 [US3] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
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

- [ ] T036 [P] [US4] Add unit test — `resolve_namespace` returns provided param when
      non-empty; returns `connection_ns` when param is absent
- [ ] T037 [P] [US4] Add unit test — `namespace_mappings` with omitted `namespace` param
      uses the connection's active namespace as default
- [ ] T038 [P] [US4] Add unit test — `namespace_mappings` for a non-existent namespace
      returns `NAMESPACE_NOT_FOUND` (not a raw IRIS error or panic)
- [ ] T039 [P] [US4] Add unit test — `namespace_mappings` response shape contains
      `mappings.globals`, `mappings.packages`, `mappings.routines` sub-arrays (use mock
      IRIS response with one entry each)
- [ ] T040 [US4] Add integration test to `test_iris_admin_observability_live.rs` —
      `#[ignore]`; call `namespace_mappings namespace="USER"`; assert `success: true`;
      assert `mappings` has `globals`, `packages`, `routines` keys

### Implementation for US4

- [ ] T041 [US4] Implement `namespace_mappings_impl` in `observability.rs`:
  - Call `dispatch_gate()`
  - Resolve namespace via `resolve_namespace`
  - Query `Config.MapGlobals`, `Config.MapPackages`, `Config.MapRoutines` in `%SYS`
  - If all three return empty AND namespace not in `Config.Namespaces`, return
    `NAMESPACE_NOT_FOUND`
  - Return `{success: true, namespace: ..., mappings: {globals: [...], packages: [...],
    routines: [...]}}`
- [ ] T042 [US4] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
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

- [ ] T043 [P] [US5] Add unit test — `database_status` response shape has `databases`
      array; each entry has `name`, `directory`, `mounted`, `free_space_mb`,
      `journal_state`, `mirror_state` (use mock response)
- [ ] T044 [P] [US5] Add unit test — `database_status` with name filter returns
      `DATABASE_NOT_FOUND` when no match (use mock empty response)
- [ ] T045 [P] [US5] Add unit test — `database_status` `mirror_state` is `"none"` (not
      null) when the IRIS response has no mirror status column value
- [ ] T046 [P] [US5] Add unit test — `database_status` with `mounted: false` entry does
      not include `free_space_mb` (or sets it to `null`), not a crash
- [ ] T047 [US5] Add integration test to `test_iris_admin_observability_live.rs` —
      `#[ignore]`; call `database_status`; assert `success: true`; assert at least one
      database entry; assert `name`, `mounted`, `mirror_state` keys present

### Implementation for US5

- [ ] T048 [US5] Implement `database_status_impl` in `observability.rs`:
  - Call `dispatch_gate()`
  - Build SQL: `SELECT Name, Directory, Mounted, FreeBD, Journal, MirrorStatus FROM
    SYS.Database ORDER BY Name` with optional `WHERE Name = :name`
  - Map `FreeBD` (blocks) × block-size → `free_space_mb`; map NULL/absent `MirrorStatus`
    → `"none"`
  - If name filter provided and result is empty, return `DATABASE_NOT_FOUND`
  - Return `{success: true, databases: [...], count: N}`
- [ ] T049 [US5] Run `cargo test -p iris-agentic-dev-core test_iris_admin_observability` —
      all US1–US5 unit tests pass

**Checkpoint**: US5 complete. `database_status` returns correct shape; name filter works;
`mirror_state` always a string.

---

## Phase 8: Polish and Cross-Cutting Concerns

**Purpose**: Integration tests, AGENTS.md update, check_config schema, final fmt/clippy.

- [ ] T050 Add integration test to `test_iris_admin_observability_live.rs` — `#[ignore]`;
      call `view_processes` with `dataPolicy=allow` on `mcpTemplate=live` config; assert
      `success: true` (confirms live-template + Query category = permitted)
- [ ] T051 [P] Add integration test — `journal_search` with `dataPolicy=block` and
      `acknowledgePhi=true` returns `DATA_POLICY_BLOCKED` (confirms hard-block, no bypass)
- [ ] T052 [P] Add integration test — `namespace_mappings namespace="NonExistentNS9999"`
      returns `NAMESPACE_NOT_FOUND`
- [ ] T053 [P] Verify `MISSING_PARAMS`, `NAMESPACE_NOT_FOUND`, `DATABASE_NOT_FOUND` appear
      in error code registry comment in `gate.rs`
- [ ] T054 [P] Update `light-skills/AGENTS.md` — add five new `iris_admin` actions to the
      MCP tool reference section with usage examples; add three new error codes to Section 6
- [ ] T055 Run full test suite: `cargo test -p iris-agentic-dev-core` — all non-ignored
      tests pass, zero regressions
- [ ] T056 Run `cargo fmt --all -- --check` — no formatting diff
- [ ] T057 Run `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings
- [ ] T058 [P] Update spec status to `Status: Implemented` in
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

- The exact SQL table names for IRIS %SYS queries (`%SYS.LockQuery`, `%SYS.ProcessQuery`,
  `%SYS.Journal.Record`, `Config.MapGlobals`, `SYS.Database`) must be verified against a
  live IRIS 2022+ instance during Phase 3–7 implementation. If a table name differs, update
  the ObjectScript accordingly.
- `view_locks` may return an empty result on a quiet development IRIS instance — the
  integration test asserts only that the response shape is correct, not that locks > 0.
- `journal_search` requires `%SYS` access; if the connection user does not have this, the
  IRIS HTTP endpoint returns a `<PROTECT>` error surfaced as `IRIS_EXECUTE_ERROR`. Document
  this in the tool schema description.
- `database_status` `FreeBD` column is in database blocks; multiply by block size (typically
  8192 bytes) to get MB. If block size is not readily available, return raw block count as
  `free_space_blocks` and document the conversion.
