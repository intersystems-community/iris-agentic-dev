# Implementation Plan: SQL Power Extensions

**Branch**: `057-sql-power`
**Spec**: spec.md
**Depends on**: 051 (dispatch_gate, McpTemplate, DataPolicy — merged to master)

## Tech Stack

- Rust 1.92 (workspace edition 2021)
- `iris-agentic-dev-core` crate — all changes here
- `serde_json` — param parsing, response building
- `reqwest` — HTTP calls to IRIS `/api/atelier/v1/{ns}/action/query` endpoint
- `sha2` or `md5` crate (already in workspace, or use stdlib) — `query_hash` for explain
- Policy gate: `dispatch_gate()` from `crates/iris-agentic-dev-core/src/policy/gate.rs`
- Existing: `validate_read_only_sql` in `tools/mod.rs` (unchanged); new `validate_dml_sql`
  added alongside it

## File Structure

```text
crates/iris-agentic-dev-core/src/tools/mod.rs          # MODIFY — add mode param, new handlers, validate_dml_sql
crates/iris-agentic-dev-core/tests/unit/test_sql_power_unit.rs    # NEW — unit tests for new modes
crates/iris-agentic-dev-core/tests/integration/test_sql_power_live.rs  # NEW — integration tests (#[ignore])
```

No new handler files needed — all three modes extend the existing `iris_query` function body
with a `match p.mode { ... }` dispatch after the existing policy gate block. New helper
functions are module-level `pub fn` items in `mod.rs`.

## Mode Dispatch Architecture

The existing `iris_query` function body currently:

1. Runs `dispatch_gate()` (policy + env gate)
2. Runs role gate
3. Runs `validate_read_only_sql` (SQL safety gate)
4. Calls IRIS `/action/query` endpoint
5. Returns rows

With this feature, the flow becomes:

1. Runs `dispatch_gate()` — gate args updated to pass `mode` for Execute classification
2. Runs role gate — `mode="write"` uses `"iris_query:INSERT"` token (write) vs
   `"iris_query:SELECT"` (read/explain/count)
3. Mode dispatch:
   - `read` → existing path (validate_read_only_sql → query IRIS → return rows)
   - `explain` → validate SELECT → fetch plan from IRIS → return plan_text + query_hash
   - `count` → validate table/query → build COUNT query → query IRIS → return count
   - `write` → validate_dml_sql → rows pre-check (UPDATE/DELETE only) → execute DML → return rows_affected

## Gate Classification for Write Mode

`dispatch_gate()` currently receives `"iris_query"` as the tool name. `iris_query` maps to
`ToolCategory::Query` in `tool_to_category()`. For write mode, we need `Execute` category.

**Approach** (consistent with how iris_global handles action-aware categories in spec 052):
Pass `mode` in the `params_json` to `dispatch_gate()`. In `check_env_gate()`, add:
if `tool_name == "iris_query"` AND `params["mode"].as_str() == Some("write")`, override
category to `ToolCategory::Execute`. This follows the same pattern already established for
`iris_global` action classification.

## validate_dml_sql Design

New `pub fn validate_dml_sql(sql: &str) -> Result<(), String>` in `tools/mod.rs`:

```text
BLOCKED DDL: CREATE, DROP, ALTER, GRANT, REVOKE
BLOCKED read: SELECT (return "SELECT_NOT_ALLOWED_IN_WRITE")
ALLOWED DML: INSERT, UPDATE, DELETE, CALL, TRUNCATE
```

Processing pipeline mirrors `validate_read_only_sql`:

1. Strip block and line comments
2. Check for empty → `Err("EMPTY")`
3. Walk chars, skip quoted content
4. Check first unquoted word token against blocked sets
5. Return `Ok(())` if DML keyword found; `Err(keyword)` for DDL/SELECT; `Err("UNKNOWN")` otherwise

The first-word approach is intentional — DML vs DDL classification is based on the
statement type keyword, not inner SELECT subqueries.

## Row-Count Pre-Check Design

For `mode="write"` with UPDATE or DELETE:

1. Parse the `FROM <table>` and `WHERE <clause>` from the DML
2. Build `SELECT COUNT(*) FROM <table> WHERE <clause>`
3. Execute on IRIS `/action/query`
4. Compare result against `max_rows_affected`
5. If count > limit: return `ROWS_LIMIT_EXCEEDED` with `actual_count`
6. If COUNT query parse fails (complex multi-table update): skip pre-check, add
   `rows_check_skipped: true` field to the success response

Parser approach: simple regex/string split — extract table name after `UPDATE`/`DELETE FROM`
and the remainder after `WHERE`. Full SQL parsing is out of scope.

## Explain Query via IRIS

IRIS EXPLAIN syntax: `EXPLAIN SELECT ...` executed via `/action/query`. The IRIS response
for EXPLAIN returns the plan in the query result rows (one row per plan line). Concatenate
rows with newlines to produce `plan_text`.

`query_hash`: SHA-256 (truncated to 16 hex chars) of the query string normalized by:
uppercasing, collapsing whitespace, stripping string literals. Allows agents to correlate
plans across runs.

Fallback: If IRIS returns an error for `EXPLAIN <query>`, try `SHOW PLAN <query>`. If both
fail, return `EXPLAIN_NOT_SUPPORTED`.

## Response Shapes

```json
// explain
{"success": true, "plan_text": "Read table MyTable, using index...", "query_hash": "a3f9b2c1d4e5f6a7"}

// count
{"success": true, "count": 42837}

// write (success)
{"success": true, "rows_affected": 3}

// write (rows limit exceeded)
{"success": false, "error_code": "ROWS_LIMIT_EXCEEDED", "actual_count": 1503, "limit": 1000}

// write (DDL blocked)
{"success": false, "error_code": "DDL_NOT_ALLOWED", "blocked_keyword": "CREATE"}
```

Error responses follow existing pattern:

```json
{"success": false, "error_code": "EXPLAIN_REQUIRES_SELECT", "error": "explain mode only accepts SELECT statements"}
```

## Test Strategy

### Unit tests (no IRIS needed)

- `validate_dml_sql`: all allowed DML, all blocked DDL, SELECT rejection, empty input, comment stripping
- `mode="explain"` with non-SELECT → `EXPLAIN_REQUIRES_SELECT` before gate
- `mode="count"` with neither table nor query → `MISSING_TARGET`
- `mode="write"` on `mcpTemplate=live` → `ENV_GATE_BLOCKED`
- `mode="write"` on `mcpTemplate=test` → `ENV_GATE_BLOCKED`
- `mode="write"` with DDL → `DDL_NOT_ALLOWED`
- `mode="write"` with SELECT → `SELECT_NOT_ALLOWED_IN_WRITE`
- `max_rows_affected` clamping: 0 → 1000, 99999 → 10000
- `mode="read"` with INSERT → `SQL_WRITE_BLOCKED` (regression — existing behavior intact)
- `query_hash` computation: same input → same hash; different whitespace → same hash

### Integration tests (#[ignore])

- `mode="explain"` on `SELECT * FROM Sample.Person` → non-empty `plan_text`
- `mode="count"` with `table="Sample.Person"` → count matches direct `SELECT COUNT(*)`
- `mode="write"` INSERT into test table → `rows_affected=1`; verify via count
- `mode="write"` UPDATE 1500 rows → `ROWS_LIMIT_EXCEEDED` with `actual_count`
- `mode="write"` on `mcpTemplate=live` config → `ENV_GATE_BLOCKED` (no IRIS call)

## Toolset Registration

`iris_query` is already registered in `Toolset::Merged`. No registration changes needed —
`mode` is a new parameter variant, not a new tool. Only the tool description string and
`QueryParams` struct change.

---

## Constitution Check

| Principle | Status | Notes |
| --- | --- | --- |
| I. Zero-Install Binary | ✅ Pass | Uses existing `/action/query` HTTP endpoint; no new install step |
| II. ObjectScript Sanity | ✅ Pass | EXPLAIN is standard IRIS SQL; no ObjectScript execution used |
| III. HTTP-First | ✅ Pass | All modes use `/action/query` REST; no container required |
| IV. Test-First, Fixture-Driven | ✅ Pass | Unit tests precede implementation in all phases |
| V. Output Shape Parity | ✅ Pass | All response shapes documented above; new error codes registered |
| VI. Environment Guard | ✅ Pass | write mode = Execute (blocked on live/test); explain/count = Query |
| VII. Dependency Minimalism | ✅ Pass | No new crates required; sha2 already available or use simple hash |

---

## Phase Structure

1. **Setup**: Add `mode` param to `QueryParams` struct; update tool description; compile stub
2. **Foundational**: Gate classification for write mode (action-aware); `validate_dml_sql`
3. **US1 (explain)**: Unit tests → implementation → integration tests
4. **US2 (count)**: Unit tests → implementation → integration tests
5. **US3 (write)**: Unit tests → implementation (DML validation + rows pre-check + execution)
6. **Polish**: Full test suite, fmt/clippy, AGENTS.md update, error code registry
