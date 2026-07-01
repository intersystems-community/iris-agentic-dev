# Requirements Checklist: 057-sql-power

## Functional Requirements

- [x] FR-001: `iris_query` accepts `mode` param (`read`, `explain`, `count`, `write`); default is `read`
- [x] FR-002: `mode="read"` behavior unchanged; `validate_read_only_sql` and `force` still apply; no regressions
- [x] FR-003: `mode="explain"` accepts only SELECT/WITH; non-SELECT rejected with `EXPLAIN_REQUIRES_SELECT` before IRIS call
- [x] FR-004: `mode="explain"` returns `{ success, plan_text, query_hash }` with raw IRIS plan text; no parsing
- [x] FR-005: `mode="explain"` classified as `ToolCategory::Query`; permitted on all `mcpTemplate` values including `live`
- [x] FR-006: `mode="count"` accepts `table` or `query` param; `query` takes precedence; `MISSING_TARGET` if neither
- [x] FR-007: `mode="count"` with `table` executes `SELECT COUNT(*) FROM <table>` and returns `{ success, count }`
- [x] FR-008: `mode="count"` with `query` executes `SELECT COUNT(*) FROM (<query>) t` and returns `{ success, count }`
- [x] FR-009: `mode="count"` classified as `ToolCategory::Query`; permitted on all `mcpTemplate` values
- [x] FR-010: `mode="write"` classified as `ToolCategory::Execute`; blocked on `live` and `test` with `ENV_GATE_BLOCKED`
- [x] FR-011: `mode="write"` applies `validate_dml_sql`; DDL rejected with `DDL_NOT_ALLOWED`; `validate_read_only_sql` NOT applied
- [x] FR-012: `mode="write"` accepts INSERT, UPDATE, DELETE, CALL, TRUNCATE; SELECT rejected with `SELECT_NOT_ALLOWED_IN_WRITE`
- [x] FR-013: `mode="write"` with UPDATE or DELETE runs row-count pre-check; `ROWS_LIMIT_EXCEEDED` with `actual_count` if exceeded
- [x] FR-014: `max_rows_affected` defaults to 1000; range 1–10000; values > 10000 clamped; 0 treated as 1000
- [x] FR-015: `mode="write"` returns `{ success: true, rows_affected: <integer> }` on success
- [x] FR-016: All three new modes pass through `dispatch_gate()` before any validation or IRIS call
- [x] FR-017: `mode="explain"` and `mode="count"` do NOT call `validate_read_only_sql`
- [x] FR-018: All new mode values documented in `iris_query` tool description string
- [x] FR-019: `check_config` continues to list `iris_query`; no new tool registration needed
- [x] FR-020: New error codes (`EXPLAIN_REQUIRES_SELECT`, `EXPLAIN_NOT_SUPPORTED`, `MISSING_TARGET`, `DDL_NOT_ALLOWED`, `SELECT_NOT_ALLOWED_IN_WRITE`, `ROWS_LIMIT_EXCEEDED`, `ROWS_CHECK_FAILED`) documented in error code registry

## Success Criteria

- [x] SC-001: `mode="explain"` returns non-empty `plan_text` in under 2 seconds on local IRIS
- [x] SC-002: `mode="count"` on large table returns count in under 500ms; no row data transferred
- [x] SC-003: `mode="write"` INSERT executes; `rows_affected` matches inserted row count; verified by subsequent SELECT
- [x] SC-004: `mode="write"` on `mcpTemplate=live` returns `ENV_GATE_BLOCKED` in under 5ms; no IRIS call made
- [x] SC-005: `mode="write"` UPDATE over row limit returns `ROWS_LIMIT_EXCEEDED` with correct count; zero rows modified
- [x] SC-006: DDL in `mode="write"` returns `DDL_NOT_ALLOWED` before IRIS call; verified by unit test
- [x] SC-007: All existing `iris_query` unit and integration tests pass without modification
- [x] SC-008: `validate_dml_sql` has 100% line coverage in unit tests
