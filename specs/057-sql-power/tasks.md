# Tasks: SQL Power Extensions

**Input**: Design documents from `/specs/057-sql-power/`
**Prerequisites**: plan.md ✓, spec.md ✓ (clarified 2026-06-29)

**Organization**: Tasks grouped by user story. US1 = explain (P1), US2 = count (P1),
US3 = write (P2). Phase 2 foundational wiring blocks all US phases.

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Add `mode` param to `QueryParams`, update tool description, confirm clean compile.

**CRITICAL**: All user story phases depend on these — complete before any US work.

- [X] T001 Add `mode: Option<String>` field to `QueryParams` struct in
      `crates/iris-agentic-dev-core/src/tools/mod.rs` (default `"read"` when absent);
      add `max_rows_affected: Option<u32>` field (default 1000 when absent); ensure both
      fields are `serde(default)` compatible
- [X] T002 Update the `iris_query` tool `#[tool(description = ...)]` attribute to mention
      all four modes (`read`, `explain`, `count`, `write`) with a one-line description each
- [X] T003 Add a `match p.mode.as_deref().unwrap_or("read") { ... }` dispatch skeleton
      inside `iris_query` after the existing role-gate block; default arm falls through to
      existing read-mode logic; other arms return `not_implemented` JSON stubs
- [X] T004 Run `cargo build -p iris-agentic-dev-core` — confirm clean compile with all
      new param fields and stub dispatch

**Checkpoint**: `iris_query` accepts `mode` param, dispatches to stubs, compiles clean.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Gate classification for write mode; `validate_dml_sql` function.

**CRITICAL**: Gate wiring must be correct before write mode can be tested.

- [X] T005 Update `check_env_gate()` in
      `crates/iris-agentic-dev-core/src/policy/env_gate.rs` to handle `iris_query` write
      mode: after `tool_to_category_pub()` returns `Query` for `iris_query`, add check:
      if `tool_name == "iris_query"` AND `params["mode"].as_str() == Some("write")`,
      override category to `ToolCategory::Execute`. Follow same pattern as `iris_global`
      action-aware classification (already in place from spec 052)
- [X] T006 Update `dispatch_gate()` call site in `iris_query` to pass `mode` in the
      `params_json`: `serde_json::json!({ "namespace": p.namespace, "mode": p.mode })`
      so the gate receives the mode value for Execute classification
- [X] T007 Implement `pub fn validate_dml_sql(sql: &str) -> Result<(), String>` in
      `crates/iris-agentic-dev-core/src/tools/mod.rs` — mirrors the processing pipeline
      of `validate_read_only_sql`:
      - Strip block and line comments; check empty → `Err("EMPTY")`
      - Walk chars skipping quoted content; extract first unquoted word token
      - If token is DDL (CREATE/DROP/ALTER/GRANT/REVOKE) → `Err(token)`
      - If token is SELECT → `Err("SELECT_IN_WRITE")`
      - If token is DML (INSERT/UPDATE/DELETE/CALL/TRUNCATE) → `Ok(())`
      - Otherwise → `Err("UNKNOWN_STATEMENT")`
- [X] T008 Run `cargo test -p iris-agentic-dev-core` — confirm all pre-existing tests still
      pass after `check_env_gate` and `dispatch_gate` param changes

**Checkpoint**: Gate classifies write mode as Execute; `validate_dml_sql` exists and compiles.

---

## Phase 3: User Story 1 — explain (Priority: P1)

**Goal**: Return the IRIS query execution plan for a SELECT statement.

**Independent Test**: Call `iris_query` with `mode="explain"` and
`query="SELECT * FROM Sample.Person"` on a live IRIS. Verify non-empty `plan_text` returned.

### Tests for US1

> Write FIRST. Must FAIL before T017.

- [X] T009 [US1] Create `crates/iris-agentic-dev-core/tests/unit/test_sql_power_unit.rs` —
      test `validate_dml_sql` with allowed DML: `INSERT INTO t VALUES (1)` → `Ok(())`;
      `UPDATE t SET x=1` → `Ok(())`; `DELETE FROM t` → `Ok(())`; `CALL myproc()` → `Ok(())`;
      `TRUNCATE TABLE t` → `Ok(())`
- [X] T010 [P] [US1] Add unit test to `test_sql_power_unit.rs` — `validate_dml_sql` blocks
      DDL: `CREATE TABLE t (...)` → `Err("CREATE")`; `DROP TABLE t` → `Err("DROP")`;
      `ALTER TABLE t ADD col INT` → `Err("ALTER")`; `GRANT SELECT ON t TO u` → `Err("GRANT")`;
      `REVOKE SELECT ON t FROM u` → `Err("REVOKE")`
- [X] T011 [P] [US1] Add unit test to `test_sql_power_unit.rs` — `validate_dml_sql` blocks
      SELECT: `SELECT * FROM t` → `Err("SELECT_IN_WRITE")`; empty input → `Err("EMPTY")`;
      comment-only → `Err("EMPTY")`; DML with inner SELECT subquery → `Ok(())`
      (e.g. `INSERT INTO t SELECT * FROM src` — outer statement is INSERT)
- [X] T012 [P] [US1] Add unit test to `test_sql_power_unit.rs` — `mode="explain"` with
      non-SELECT query `"INSERT INTO t VALUES (1)"` → `EXPLAIN_REQUIRES_SELECT` error code
      returned before gate fires (no IRIS call)
- [X] T013 [P] [US1] Add unit test to `test_sql_power_unit.rs` — `mode="explain"` with
      empty query → `EMPTY_QUERY` error code
- [X] T014 [P] [US1] Add unit test to `test_sql_power_unit.rs` — `mode="explain"` on
      `mcpTemplate=live` policy does NOT return `ENV_GATE_BLOCKED` (explain is Query,
      permitted by live); mock gate returns `Ok(())` for live + Query
- [X] T015 [P] [US1] Add unit test to `test_sql_power_unit.rs` — `query_hash` helper:
      same query string → same hash; same query with different whitespace → same hash;
      different queries → different hashes (collision would be acceptable here)
- [X] T016 [US1] Create `crates/iris-agentic-dev-core/tests/integration/test_sql_power_live.rs`
      — `#[ignore]`; call `iris_query` with `mode="explain"`,
      `query="SELECT * FROM Sample.Person"` on live IRIS; assert `plan_text` is non-empty
      string; assert `query_hash` is a 16-char hex string

### Implementation for US1

- [X] T017 [US1] Implement `explain` arm in the mode dispatch:
      - Validate query is non-empty → `EMPTY_QUERY`
      - Validate first keyword is SELECT or WITH → else `EXPLAIN_REQUIRES_SELECT`
      - Build explain query: `EXPLAIN <query>`
      - POST to IRIS `/action/query`, collect rows; concatenate row values with newlines
        to produce `plan_text`
      - If IRIS returns SQL error with explain, retry with `SHOW PLAN <query>`; if both
        fail, return `EXPLAIN_NOT_SUPPORTED` with IRIS version in message
      - Compute `query_hash` from normalized query (uppercase, collapse whitespace)
      - Return `{ "success": true, "plan_text": ..., "query_hash": ... }`
- [X] T018 [US1] Run `cargo test -p iris-agentic-dev-core test_sql_power` — all US1 unit
      tests must pass

**Checkpoint**: US1 complete. `iris_query explain` returns plan_text + query_hash.

---

## Phase 4: User Story 2 — count (Priority: P1)

**Goal**: Return a row count without transferring result rows.

**Independent Test**: Call `iris_query` with `mode="count"` and `table="Sample.Person"`.
Verify `count` integer returned with no `rows` field.

### Tests for US2

> Write FIRST. Must FAIL before T024.

- [X] T019 [P] [US2] Add unit test to `test_sql_power_unit.rs` — `mode="count"` with
      neither `table` nor `query` param → `MISSING_TARGET` error code before IRIS call
- [X] T020 [P] [US2] Add unit test to `test_sql_power_unit.rs` — `mode="count"` with
      `table` param builds query `SELECT COUNT(*) FROM <table>` correctly; verify by
      checking the generated query string (extract as a testable helper)
- [X] T021 [P] [US2] Add unit test to `test_sql_power_unit.rs` — `mode="count"` with
      `query` param builds `SELECT COUNT(*) FROM (<query>) t` correctly
- [X] T022 [P] [US2] Add unit test to `test_sql_power_unit.rs` — `mode="count"` on
      `mcpTemplate=live` does NOT return `ENV_GATE_BLOCKED` (count is Query category)
- [X] T023 [US2] Add integration test to `test_sql_power_live.rs` — `#[ignore]`;
      call `iris_query` with `mode="count"`, `table="Sample.Person"`;
      also call `mode="read"` with `query="SELECT COUNT(*) FROM Sample.Person"`;
      assert count values match; assert count mode response has no `rows` field

### Implementation for US2

- [X] T024 [US2] Implement `count` arm in mode dispatch:
      - Check `p.table` and `p.query` — `MISSING_TARGET` if both absent
      - `p.query` takes precedence: build `SELECT COUNT(*) FROM (<query>) t`
      - `p.table` only: build `SELECT COUNT(*) FROM <table>`
      - POST to IRIS `/action/query`
      - Extract count from `result.content[0]["Aggregate_1"]` or first column of first row
      - Return `{ "success": true, "count": <integer> }`
      - On SQL error: return `SQL_ERROR` with IRIS message
- [X] T025 [US2] Run `cargo test -p iris-agentic-dev-core test_sql_power` — all US1+US2
      unit tests pass

**Checkpoint**: US2 complete. `iris_query count` returns integer count; no row data.

---

## Phase 5: User Story 3 — write (Priority: P2)

**Goal**: Execute DML statements (INSERT/UPDATE/DELETE/CALL/TRUNCATE) against IRIS.

**Independent Test**: Call `iris_query` with `mode="write"` and
`query="INSERT INTO IrisDevTest.SqlPower (Name) VALUES ('test')"`. Verify `rows_affected=1`.
Then test UPDATE with 1500 matching rows → `ROWS_LIMIT_EXCEEDED`.

### Tests for US3

> Write FIRST. Must FAIL before T034.

- [X] T026 [P] [US3] Add unit test to `test_sql_power_unit.rs` — `mode="write"` on
      `mcpTemplate=live` → `ENV_GATE_BLOCKED`
- [X] T027 [P] [US3] Add unit test to `test_sql_power_unit.rs` — `mode="write"` on
      `mcpTemplate=test` → `ENV_GATE_BLOCKED` (Execute blocked on test too)
- [X] T028 [P] [US3] Add unit test to `test_sql_power_unit.rs` — `mode="write"` with
      DDL statement `"CREATE TABLE t (id INT)"` → `DDL_NOT_ALLOWED` with
      `blocked_keyword: "CREATE"`
- [X] T029 [P] [US3] Add unit test to `test_sql_power_unit.rs` — `mode="write"` with
      SELECT statement → `SELECT_NOT_ALLOWED_IN_WRITE` error
- [X] T030 [P] [US3] Add unit test to `test_sql_power_unit.rs` — `max_rows_affected`
      clamping: value 0 → treated as 1000; value 99999 → clamped to 10000
- [X] T031 [P] [US3] Add unit test to `test_sql_power_unit.rs` — `mode="write"` with
      empty query → `EMPTY_QUERY`
- [X] T032 [P] [US3] Add unit test to `test_sql_power_unit.rs` — `mode="write"` with
      `force=true` includes `force_ignored: true` in any non-error response
      (force has no effect in write mode)
- [X] T033 [US3] Add integration tests to `test_sql_power_live.rs` — `#[ignore]`:
      - INSERT into `IrisDevTest.SqlPower` → `rows_affected=1`; verify via count; cleanup
      - UPDATE 1500 rows (seeded by setup) → `ROWS_LIMIT_EXCEEDED` with correct `actual_count`
      - UPDATE same 1500 rows with `max_rows_affected=2000` → succeeds with `rows_affected=1500`
      - TRUNCATE `IrisDevTest.SqlPower` → `success: true`; verify count=0
      - `mode="write"` on `mcpTemplate=live` connection config → `ENV_GATE_BLOCKED`; no DB change

### Implementation for US3

- [X] T034 [US3] Implement `write` arm in mode dispatch:
      - Apply `validate_dml_sql` → return `DDL_NOT_ALLOWED`, `SELECT_NOT_ALLOWED_IN_WRITE`,
        or `EMPTY_QUERY` as appropriate
      - Clamp `max_rows_affected`: 0 → 1000, > 10000 → 10000
      - For UPDATE and DELETE: run rows pre-check (derive COUNT query, execute, compare);
        return `ROWS_LIMIT_EXCEEDED` with `actual_count` if exceeded
      - If COUNT parse fails (complex WHERE): skip pre-check, set `rows_check_skipped=true`
      - Execute DML via IRIS `/action/query`
      - Extract `rows_affected` from response; if missing, default to 0
      - Return `{ "success": true, "rows_affected": <int> }` (plus `force_ignored: true`
        if `p.force` was set)
- [X] T035 [US3] Run `cargo test -p iris-agentic-dev-core test_sql_power` — all US1–US3
      unit tests pass

**Checkpoint**: US3 complete. DML executes; rows pre-check prevents mass updates.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Full test suite, error code registry, AGENTS.md update, fmt/clippy.

- [X] T036 Add new error codes to the error code registry comment block in
      `crates/iris-agentic-dev-core/src/policy/gate.rs` (or wherever the existing registry
      lives): `EXPLAIN_REQUIRES_SELECT`, `EXPLAIN_NOT_SUPPORTED`, `MISSING_TARGET`,
      `DDL_NOT_ALLOWED`, `SELECT_NOT_ALLOWED_IN_WRITE`, `ROWS_LIMIT_EXCEEDED`,
      `ROWS_CHECK_FAILED`, `EMPTY_QUERY` (already exists — confirm not duplicated)
- [X] T037 [P] Add unit test to `test_sql_power_unit.rs` — `mode="read"` with INSERT
      still returns `SQL_WRITE_BLOCKED` (regression guard for existing behavior)
- [X] T038 [P] Add unit test to `test_sql_power_unit.rs` — `mode="read"` omitted
      (no `mode` in params) behaves identically to `mode="read"` explicit
- [X] T039 [P] Add unit test to `test_sql_power_unit.rs` — `mode="count"` with both
      `table` and `query` set: `query` takes precedence; verify generated COUNT query
      uses the subquery form, not the table form
- [X] T040 [P] Update `light-skills/AGENTS.md` — add documentation for new `iris_query`
      modes: explain (usage example with SELECT), count (usage example with table param),
      write (usage example with INSERT; note about env-gate blocking on live/test)
- [X] T041 Run full test suite: `cargo test -p iris-agentic-dev-core` — all non-ignored
      tests pass, zero regressions
- [X] T042 Run `cargo fmt --all -- --check` — no formatting diff
- [X] T043 Run `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings
- [X] T044 [P] Update spec status to `Status: Implemented` in
      `specs/057-sql-power/spec.md`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 (needs `mode` field in `QueryParams`)
- **Phase 3 (US1 explain)**: Depends on Phase 2 (needs gate + `validate_dml_sql`)
- **Phase 4 (US2 count)**: Depends on Phase 2; can run in parallel with Phase 3
- **Phase 5 (US3 write)**: Depends on Phase 2; can run after Phase 3 (reuses unit test file)
- **Phase 6 (Polish)**: Depends on all US phases complete

### User Story Dependencies

- **US1 (explain)**: Foundational complete
- **US2 (count)**: Foundational complete; independent of US1
- **US3 (write)**: Foundational complete; independent of US1/US2

### Within Each Phase

- Tests written FIRST, must FAIL before implementation task
- `validate_dml_sql` (Phase 2) must exist before write mode tests or implementation
- Gate classification (T005–T006) must compile before any gate-dependent tests

### Parallel Opportunities

- T009–T015 (US1 unit tests) — T010–T015 parallel after T009 creates the file
- T019–T022 (US2 unit tests) — all parallel
- T026–T032 (US3 unit tests) — all parallel
- T036–T040 (Polish) — all parallel (different files)

---

## Parallel Example: Phase 3 (US1)

```text
# Write tests first (T009 creates file, T010–T015 parallel):
T009 → [T010, T011, T012, T013, T014, T015 in parallel] → T016

# Then implement (T017 sequential, T018 validates):
T017 → T018
```

---

## Implementation Strategy

### MVP First (US1 + US2 only — the two P1 stories)

1. Complete Phase 1: Setup (T001–T004)
2. Complete Phase 2: Foundational (T005–T008)
3. Complete Phase 3: US1 explain (T009–T018)
4. Complete Phase 4: US2 count (T019–T025)
5. **STOP and VALIDATE**: `cargo test test_sql_power` green; explain returns plan live
6. Ship P1 — read-only power features are safe on any environment

### Incremental Delivery

1. Setup + Foundational → mode param registered, gate wired, DML validator ready
2. US1 explain → query plan inspection
3. US2 count → efficient row counting
4. US3 write → DML execution with safety guards
5. Polish → error code registry, AGENTS.md, fmt/clippy

---

## Notes

- `validate_dml_sql` is a new `pub fn` alongside `validate_read_only_sql` in `mod.rs`.
  Both functions are independently testable and do not share state.
- The `mode` field is `Option<String>` for backward compatibility; callers that omit `mode`
  continue to work as before.
- `query_hash` is intentionally not a cryptographic guarantee — it is a debugging aid for
  correlating plan observations. SHA-256 truncated to 16 hex chars is sufficient.
- IRIS COUNT query for the rows pre-check may fail for very complex UPDATE WHERE clauses
  (e.g., subqueries in WHERE). The skip-with-warning behavior (T034) prevents false blocking
  while being transparent about the limitation.
- All integration tests use `IrisDevTest.SqlPower` table to avoid polluting production data.
  Integration test setup must CREATE this table if absent (or use an existing Sample namespace
  table that won't be modified by explain/count tests).
- The existing `force` param behavior for `mode="read"` is unchanged. In write mode, `force`
  is accepted but has no effect; `force_ignored: true` is added to the response.
