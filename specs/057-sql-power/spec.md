# Feature Specification: SQL Power Extensions

**Feature Branch**: `057-sql-power`
**Created**: 2026-06-29
**Status**: Implemented
**Depends on**: 051 (PHI policy and env gates — merged to master)

## Overview

The existing `iris_query` tool supports read-only SQL SELECT queries with a hard validation
gate (`validate_read_only_sql`) that blocks all DML/DDL before reaching IRIS. This covers
the most common agent use case but leaves three important capabilities missing:

1. **explain** — Agents cannot inspect query execution plans. Understanding whether IRIS
   will use an index or do a full-table scan is essential for diagnosing slow queries.
2. **count** — Agents counting rows must transfer all rows and count them in the response,
   wasting bandwidth on large tables.
3. **write** — Agents cannot execute DML (INSERT/UPDATE/DELETE/CALL/TRUNCATE) against IRIS
   at all, forcing developers to hand-construct workarounds via `iris_execute`.

This feature extends `iris_query` with three new `mode` parameter values: `explain`, `count`,
and `write`. All three modes are additive — the existing default read-only mode is unchanged.
The spec 051 environment template gate applies to write mode (Execute category). Explain and
count modes are Query category (permitted by all templates).

**Primary drivers:**

- **Performance diagnosis**: Agents need query plan access to diagnose slow queries without
  expert IRIS SQL knowledge.
- **Efficiency**: COUNT queries on large tables should not transfer result rows.
- **DML capability**: Controlled data writes are a common agent task; `iris_execute` is too
  low-level and has no SQL-aware safety layer.

---

## Clarifications

### Session 2026-06-29

- Q: Should TRUNCATE be allowed in write mode, or only INSERT/UPDATE/DELETE/CALL? → A: TRUNCATE is allowed in write mode — it is a DML operation in IRIS SQL semantics. CREATE TABLE, DROP TABLE, ALTER TABLE, CREATE INDEX, and GRANT/REVOKE are blocked as DDL. CALL (stored procedure) is allowed. The distinction: DML modifies data, DDL modifies schema. Write mode is for data operations only.
- Q: IRIS EXPLAIN returns a text plan. Should explain mode return raw text or parse it into structured fields? → A: Raw text in a `plan_text` field, no parsing. Parsing IRIS query plans is fragile and version-dependent. Agents can read and interpret the raw plan text. Include `plan_text` and `query_hash` fields. Consistent with how `iris_get_log` returns raw log content.
- Q: Should write mode enforce a max-rows-affected limit to prevent accidental mass updates? → A: Yes — default `max_rows_affected=1000`, configurable up to 10000. If the statement would affect more rows than the limit (detected via COUNT before execution), return `ROWS_LIMIT_EXCEEDED` error with the actual count. User can override by passing `max_rows_affected` explicitly. This is a safety guard, not a hard system limit.

---

## User Scenarios & Testing

### User Story 1 — Query Plan Inspection via explain (Priority: P1)

A developer is diagnosing a slow IRIS query. They want to see the execution plan IRIS will
use — index usage, join strategy, estimated costs — without executing the query and waiting
for results.

**Why this priority**: Query plan inspection is the most common SQL performance diagnosis
step. Agents frequently need to iterate on index/query design without executing against live
data. explain mode is read-only and has no side effects, making it safe for all environments.

**Independent Test**: Call `iris_query` with `mode="explain"` and a SELECT statement.
Verify the response contains `plan_text` (non-empty string). Verify the query is NOT
executed (no rows returned). Verify the call succeeds on a `mcpTemplate=live` connection.

**Acceptance Scenarios**:

1. **Given** `mode="explain"` and a valid SELECT statement, **When** called, **Then** the
   response includes `plan_text` (raw IRIS query plan as a string) and `query_hash`
   (a deterministic identifier for the query shape), with no `rows` field.
2. **Given** `mode="explain"` and a non-SELECT statement (e.g. INSERT), **When** called,
   **Then** `EXPLAIN_REQUIRES_SELECT` error is returned without touching IRIS.
3. **Given** `mode="explain"` on a `mcpTemplate=live` connection, **When** called, **Then**
   the call succeeds (explain is Query category, not blocked by live template).
4. **Given** `mode="explain"` and a syntactically invalid SELECT, **When** IRIS returns an
   error, **Then** the response includes `SQL_ERROR` with the IRIS error message.
5. **Given** `mode="explain"` and an empty query string, **When** called, **Then**
   `EMPTY_QUERY` error is returned before any IRIS call.

---

### User Story 2 — Efficient Row Count via count (Priority: P1)

A developer wants to know how many rows match a query or exist in a table. They cannot
afford to transfer all rows for large tables.

**Why this priority**: COUNT is the most frequent "how big is this?" query pattern. Without
it, agents must issue full SELECT queries and count the response array — unacceptable for
tables with millions of rows. count mode is also pure-read and safe for all environments.

**Independent Test**: Call `iris_query` with `mode="count"` and a table name or WHERE
clause. Verify the response contains a single integer `count` field. Verify no row data
is returned. Compare the count against a direct `SELECT COUNT(*) FROM ...` query result.

**Acceptance Scenarios**:

1. **Given** `mode="count"` and a `table` param, **When** called, **Then** the response
   includes `count` (integer, ≥ 0) and no `rows` field; internally executes
   `SELECT COUNT(*) FROM <table>`.
2. **Given** `mode="count"` and a `query` param (a SELECT statement), **When** called,
   **Then** wraps it as `SELECT COUNT(*) FROM (<query>) t` and returns the count.
3. **Given** `mode="count"` on a `mcpTemplate=live` connection, **When** called, **Then**
   the call succeeds (count is Query category).
4. **Given** `mode="count"` with neither `table` nor `query`, **When** called, **Then**
   `MISSING_TARGET` error is returned before any IRIS call.
5. **Given** `mode="count"` and the table does not exist, **When** IRIS returns an error,
   **Then** `SQL_ERROR` is returned with the IRIS error message.

---

### User Story 3 — DML Execution via write (Priority: P2)

A developer wants to INSERT, UPDATE, DELETE, CALL a stored procedure, or TRUNCATE a table
via the agent. The existing `iris_execute` tool works but is ObjectScript-only and has no
SQL-aware safety layer.

**Why this priority**: DML is essential for agent-driven data operations (test setup,
data migration, corrections). However, it is Execute category and must be blocked on live
and test environments per spec 051. The row-limit safety guard prevents accidental mass
updates. This is P2 because the existing workaround (`iris_execute`) exists.

**Independent Test**: Call `iris_query` with `mode="write"` and an INSERT statement on
a `mcpTemplate=dev` connection. Verify a row is inserted (verify with a count or select).
Verify the same call on `mcpTemplate=live` returns `ENV_GATE_BLOCKED`. Verify an UPDATE
that would affect 1500 rows (exceeding the default `max_rows_affected=1000`) returns
`ROWS_LIMIT_EXCEEDED` with the actual count.

**Acceptance Scenarios**:

1. **Given** `mode="write"`, a valid DML statement, and `mcpTemplate=dev` (or no template),
   **When** called, **Then** the statement executes and the response includes `success: true`
   and `rows_affected` (integer).
2. **Given** `mode="write"` on `mcpTemplate=live`, **When** called, **Then**
   `ENV_GATE_BLOCKED` is returned before any IRIS call.
3. **Given** `mode="write"` on `mcpTemplate=test`, **When** called, **Then**
   `ENV_GATE_BLOCKED` is returned (write mode is Execute category, blocked on test too).
4. **Given** `mode="write"` and a DDL statement (CREATE TABLE, DROP TABLE, ALTER TABLE,
   CREATE INDEX, GRANT, REVOKE), **When** called, **Then** `DDL_NOT_ALLOWED` error is
   returned before any IRIS call.
5. **Given** `mode="write"`, an UPDATE/DELETE affecting more rows than `max_rows_affected`
   (default 1000), **When** the pre-execution count check fires, **Then**
   `ROWS_LIMIT_EXCEEDED` is returned with `actual_count` and no rows are modified.
6. **Given** `mode="write"` and explicit `max_rows_affected=5000` (≤ 10000), **When** the
   count check passes, **Then** the statement executes normally.
7. **Given** `mode="write"` and a CALL statement, **When** called, **Then** the stored
   procedure executes and the response includes `success: true`.
8. **Given** `mode="write"` and a TRUNCATE statement, **When** called, **Then** the table
   is truncated and `success: true` is returned (TRUNCATE is DML, not DDL).
9. **Given** `mode="write"` and an empty query, **When** called, **Then** `EMPTY_QUERY`
   error is returned before any IRIS call.

---

### Edge Cases

- What if `mode` is omitted? → Default to `read` (current behavior). Existing callers are
  unaffected. `validate_read_only_sql` still fires for `mode="read"`.
- What if `mode="explain"` is called with a WITH (CTE) query? → Accepted — CTE SELECT is
  valid for explain.
- What if `mode="count"` is given both `table` and `query`? → `query` takes precedence;
  wrap it as a subquery. Log a warning that `table` was ignored.
- What if `mode="write"` is given a SELECT statement? → `SELECT_NOT_ALLOWED_IN_WRITE` error.
  Write mode is DML-only. Use `mode="read"` for SELECT.
- What if the COUNT pre-check for write mode itself fails (SQL error counting)? → Return
  `ROWS_CHECK_FAILED` with the IRIS error. Do not proceed with execution.
- What if `max_rows_affected` is set above 10000? → Clamp to 10000 and proceed; do not
  error. Log a warning.
- What if `max_rows_affected=0` is passed? → Treat as default (1000). Zero is meaningless.
- What if IRIS does not support SHOW PLAN or EXPLAIN for the current version? → Fall back to
  `EXPLAIN SELECT ...`; if neither works, return `EXPLAIN_NOT_SUPPORTED` with the IRIS
  version in the message.
- What if `mode="write"` is called and `force=true`? → `force` has no effect in write mode
  (write mode already performs DML; it has its own validation that is not bypassable by
  `force`). Return a `force_ignored: true` field in the response for transparency.
- What if the write DML includes a subquery that starts with SELECT? → The outer statement
  type determines DML vs. DDL classification, not any inner SELECT.

---

## Requirements

### Functional Requirements

- **FR-001**: `iris_query` MUST accept a `mode` parameter with values `read` (default),
  `explain`, `count`, and `write`. Omitting `mode` is equivalent to `mode="read"`.
- **FR-002**: `mode="read"` behavior MUST be unchanged from the current implementation.
  `validate_read_only_sql` continues to apply; `force` param continues to bypass it on
  non-live connections. No regressions.
- **FR-003**: `mode="explain"` MUST accept only SELECT statements (including CTEs starting
  with WITH). Non-SELECT queries MUST be rejected with `EXPLAIN_REQUIRES_SELECT` before any
  IRIS call.
- **FR-004**: `mode="explain"` MUST return `{ "success": true, "plan_text": "<raw IRIS plan>",
  "query_hash": "<sha256 of normalized query>" }`. The `plan_text` field contains the
  verbatim IRIS query plan text. No parsing or structured extraction is performed.
- **FR-005**: `mode="explain"` MUST be classified as `ToolCategory::Query` in the env-gate
  system. It MUST be permitted on all `mcpTemplate` values including `live`.
- **FR-006**: `mode="count"` MUST accept either a `table` param (table name, no SQL) or a
  `query` param (a SELECT statement). When both are provided, `query` takes precedence.
  When neither is provided, `MISSING_TARGET` error is returned before any IRIS call.
- **FR-007**: `mode="count"` with `table` MUST execute `SELECT COUNT(*) FROM <table>` on
  IRIS and return `{ "success": true, "count": <integer> }`.
- **FR-008**: `mode="count"` with `query` MUST execute `SELECT COUNT(*) FROM (<query>) t`
  on IRIS and return `{ "success": true, "count": <integer> }`.
- **FR-009**: `mode="count"` MUST be classified as `ToolCategory::Query`. Permitted on all
  `mcpTemplate` values.
- **FR-010**: `mode="write"` MUST pass through `dispatch_gate()` as `ToolCategory::Execute`,
  which blocks it on `mcpTemplate=live` and `mcpTemplate=test` with `ENV_GATE_BLOCKED`.
- **FR-011**: `mode="write"` MUST apply its own DML validation (`validate_dml_sql`) before
  any IRIS call. DDL statements (CREATE, DROP, ALTER for tables/indexes/views, GRANT,
  REVOKE) MUST be rejected with `DDL_NOT_ALLOWED`. The existing `validate_read_only_sql` is
  NOT applied to write mode.
- **FR-012**: `mode="write"` MUST accept: INSERT, UPDATE, DELETE, CALL, TRUNCATE. SELECT
  in write mode MUST be rejected with `SELECT_NOT_ALLOWED_IN_WRITE`.
- **FR-013**: `mode="write"` with UPDATE or DELETE MUST perform a row-count pre-check
  before executing: derive a COUNT query from the DML's WHERE clause (or count all rows for
  no-WHERE case), execute it, and compare against `max_rows_affected`. If count exceeds
  limit, return `ROWS_LIMIT_EXCEEDED` with `actual_count` field. INSERT and CALL and
  TRUNCATE skip the pre-check (INSERT inserts what was provided; TRUNCATE is intentionally
  destructive; CALL returns procedure results).
- **FR-014**: `max_rows_affected` MUST default to 1000, accept values 1–10000. Values above
  10000 MUST be clamped to 10000. Values of 0 MUST be treated as 1000 (default).
- **FR-015**: `mode="write"` MUST return `{ "success": true, "rows_affected": <integer> }`
  on success. The `rows_affected` value is taken from the IRIS SQL execution response.
- **FR-016**: All three new modes MUST pass through the existing policy gate
  (`dispatch_gate()`) before any validation or IRIS call. Gate errors take precedence over
  validation errors.
- **FR-017**: `mode="explain"` and `mode="count"` MUST NOT call `validate_read_only_sql`.
  They have their own input validation (FR-003, FR-006).
- **FR-018**: All new mode values MUST be documented in the `iris_query` tool description
  string alongside the existing read mode description.
- **FR-019**: `check_config` tool response MUST continue to list `iris_query` in the tool
  inventory (no registration change needed; mode is a param variant, not a new tool).
- **FR-020**: Error codes introduced by this feature: `EXPLAIN_REQUIRES_SELECT`,
  `EXPLAIN_NOT_SUPPORTED`, `MISSING_TARGET`, `DDL_NOT_ALLOWED`, `SELECT_NOT_ALLOWED_IN_WRITE`,
  `ROWS_LIMIT_EXCEEDED`, `ROWS_CHECK_FAILED`. All MUST be documented in the error code
  registry comment in `gate.rs` or equivalent.

### Key Entities

- **QueryMode**: `read | explain | count | write` — controls which execution path and
  validation applies inside `iris_query`.
- **DmlValidator** (`validate_dml_sql`): New function parallel to `validate_read_only_sql`.
  Accepts DML (INSERT/UPDATE/DELETE/CALL/TRUNCATE), rejects DDL and SELECT in write context.
- **RowsLimitGuard**: Pre-execution COUNT check for UPDATE/DELETE write-mode statements.
  Derives a COUNT query from the DML, executes it, compares against `max_rows_affected`.
- **ExplainResult**: `{ plan_text: String, query_hash: String }` — raw plan output from IRIS.
- **WriteResult**: `{ rows_affected: i64 }` — row count from IRIS DML execution response.
- **CountResult**: `{ count: i64 }` — single integer from COUNT query.

---

## Success Criteria

### Measurable Outcomes

- **SC-001**: `mode="explain"` on any SELECT returns non-empty `plan_text` in under 2 seconds
  on a local IRIS instance.
- **SC-002**: `mode="count"` on a table with 1 million rows returns the count in under 500ms
  and transfers zero row data (no `rows` array in response).
- **SC-003**: `mode="write"` with an INSERT executes successfully and `rows_affected` equals
  the number of rows inserted, verified by a subsequent `mode="read"` SELECT.
- **SC-004**: `mode="write"` on `mcpTemplate=live` returns `ENV_GATE_BLOCKED` in under 5ms
  with no IRIS call made (verified by absence of IRIS-side changes).
- **SC-005**: `mode="write"` with an UPDATE affecting 1500 rows (above default limit) returns
  `ROWS_LIMIT_EXCEEDED` with `actual_count=1500` and zero rows modified.
- **SC-006**: DDL statement in `mode="write"` returns `DDL_NOT_ALLOWED` before any IRIS call,
  verified by automated unit test.
- **SC-007**: All existing `iris_query` unit and integration tests pass without modification
  (zero regressions to `mode="read"` behavior).
- **SC-008**: `validate_dml_sql` function has 100% line coverage in unit tests.

---

## Assumptions

- IRIS's SHOW PLAN / EXPLAIN syntax is available in the target IRIS version (2024+). If not,
  `EXPLAIN_NOT_SUPPORTED` is returned; this is a graceful degradation, not a blocking failure.
- The IRIS `/api/atelier/v1/{ns}/action/query` endpoint is used for all four modes — same
  transport as existing `iris_query`. Explain uses a plan-fetch query; count wraps the input.
- `rows_affected` for DML is returned in the IRIS response body under `result.content` or
  a similar field — actual path confirmed during implementation against the live endpoint.
- The row-limit pre-check for UPDATE/DELETE derives a COUNT query by prepending
  `SELECT COUNT(*) FROM <table>` with the same WHERE clause. Complex multi-table UPDATE
  WHERE extraction may not be possible for all statement shapes; in that case, the pre-check
  is skipped and a warning is included in the response.
- `TRUNCATE` in IRIS SQL DML context truncates the named table and is not reversible. The
  row-limit guard does not apply to TRUNCATE (the operation is intentionally bulk).
- CALL statements invoke stored procedures; their side effects are not row-countable in
  advance. The row-limit guard does not apply to CALL.
- The `mode` parameter is additive to the existing `iris_query` schema. All existing params
  (`query`, `namespace`, `parameters`, `force`, `confirm`) remain unchanged.
- Spec 051 gate rules apply: write mode is Execute category; explain and count are Query
  category. No new `ToolCategory` variant is needed.
