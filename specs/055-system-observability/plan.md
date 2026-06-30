# Implementation Plan: System Observability Depth

**Branch**: `055-system-observability`
**Date**: 2026-06-29
**Spec**: spec.md
**Depends on**: 051 (dispatch_gate, McpTemplate, DataPolicy — merged), 052 (iris_global
— merged)

## Summary

Add five read-only observability actions to the existing `iris_admin` dispatcher:
`view_locks`, `view_processes`, `journal_search`, `namespace_mappings`, and
`database_status`. All five are `ToolCategory::Query` and permitted on every
`mcpTemplate` value. `view_processes` passes through `dataPolicy` (block/redact/allow).
`journal_search` is a bulk-PHI hard-block — `dataPolicy=allow` required. All implemented
in `admin.rs` or a companion `observability.rs` module via the same HTTP ObjectScript
execution path as existing `iris_admin` actions.

## Technical Context

**Language/Version**: Rust 1.92 (workspace edition 2021)
**Primary Dependencies**: `iris-agentic-dev-core` crate; `serde_json`; `reqwest`;
`dispatch_gate()` from `crates/iris-agentic-dev-core/src/policy/gate.rs`
**Storage**: N/A — all operations are read-only queries to IRIS %SYS tables
**Testing**: `cargo test -p iris-agentic-dev-core`; unit tests (no IRIS); integration
tests (`#[ignore]`, require live IRIS)
**Target Platform**: Linux/macOS; IRIS via HTTP execute endpoint
**Performance Goals**: Each action < 500ms on a healthy IRIS instance
**Constraints**: All five actions must pass `dispatch_gate()` before any IRIS call; no
new MCP tool names (extend `iris_admin`); no new crates

## File Structure

```text
crates/iris-agentic-dev-core/src/tools/admin.rs
    MODIFY — add five new action arms to the iris_admin match dispatcher

crates/iris-agentic-dev-core/src/tools/observability.rs
    NEW (if complexity warrants split) — view_locks_impl, view_processes_impl,
    journal_search_impl, namespace_mappings_impl, database_status_impl

crates/iris-agentic-dev-core/src/tools/mod.rs
    MODIFY — update iris_admin action list in tool schema description; add new action
    names to the error message listing allowed actions

crates/iris-agentic-dev-core/src/policy/gate.rs
    MODIFY — add MISSING_PARAMS, NAMESPACE_NOT_FOUND, DATABASE_NOT_FOUND to error
    code registry comment

tests/unit/test_iris_admin_observability_unit.rs
    NEW — unit tests for all five actions (no IRIS required)

tests/integration/test_iris_admin_observability_live.rs
    NEW — integration tests (#[ignore]); require live IRIS in %SYS-accessible config

light-skills/AGENTS.md
    MODIFY — add five new iris_admin actions to MCP tool reference; add new error
    codes to Section 6
```

## ObjectScript Execution Strategy

All five actions use a single ObjectScript code block submitted to `execute_via_generator`
(HTTP-only, same transport as existing `iris_admin` actions). Each action executes in
`%SYS` namespace; the Rust handler overrides the namespace param to `%SYS` for
`journal_search`.

### view_locks

`%SYS.LockQuery` is **not a SQL table** — it is an abstract class with named queries.
Use the `%SYS.LockQuery:Detail` class query via `%ResultSet`:

```objectscript
Set rs = ##class(%ResultSet).%New("%SYS.LockQuery:Detail")
Do rs.Execute()
While rs.Next() {
  // columns: Name, Owner, Type, Mode, OwnerName (verify at impl time)
}
```

Map `Name`→`resource`, `Owner`→`owner_pid`, `Type`→`lock_type`,
`Mode`→`lock_mode`, `OwnerName`→`owner_username`.

### view_processes

SQL table confirmed (`%SYS.ProcessQuery`, SQLCODE 0). **Corrected column names**
(verified on IRIS 2026.2):

```sql
SELECT Pid, UserName, NameSpace, State, ClientNodeName, ClientIPAddress, Routine
FROM %SYS.ProcessQuery
ORDER BY Pid
```

Optional namespace filter: `WHERE NameSpace = :namespace`. Note: column is `NameSpace`
(capital S), not `Namespace`. `ClientNodeName` not `ClientName`.
Redact fields: `UserName`, `ClientNodeName`, `ClientIPAddress`.

### journal_search

`%SYS.Journal.Record` has **no SQL projection** (SQLCODE -30). `%SYS.Journal.File`
also has no SQL table. The `%SYS.Journal.Record` class properties do **not** include
`GlobalReference` — the assumed field does not exist. Use `%SYS.Journal.File:Search`
named query, but column set must be verified against a non-empty journal at
implementation time. See `research.md` for details.

**Implementation note**: `journal_search` is higher complexity than originally planned.
Verify `%SYS.Journal.File:Search` columns before implementing T034.

### namespace_mappings

All three SQL tables confirmed (SQLCODE 0). **Corrected column names**
(verified on IRIS 2026.2) — all three use `Database`, not `GlobalDatabase` /
`PackageDatabase` / `RoutineDatabase`:

```sql
SELECT Name, Database FROM Config.MapGlobals WHERE Namespace = :ns
SELECT Name, Database FROM Config.MapPackages WHERE Namespace = :ns
SELECT Name, Database FROM Config.MapRoutines WHERE Namespace = :ns
SELECT Name FROM Config.Namespaces WHERE Name = :ns  -- existence check
```

SQLCODE 100 on the Namespaces query = namespace not found → return `NAMESPACE_NOT_FOUND`.

### database_status

`SYS.Database` is **not a SQL table** (SQLCODE -30). Use the `SYS.Database:FreeSpace`
class query for runtime status and `SYS.Database:List` for mirror/encryption state:

```objectscript
Set rs = ##class(%ResultSet).%New("SYS.Database:FreeSpace")
Do rs.Execute("*")
// columns: DatabaseName, Directory, MaxSize, Size, ExpansionSize,
//          Available, Free, DiskFreeSpace, Status, SizeInt,
//          AvailableNum, DiskFreeSpaceNum, ReadOnly
```

Map: `DatabaseName`→`name`, `Directory`→`directory`, `Status` contains `"Mounted"`→
`mounted: true/false`, `AvailableNum`→`free_space_mb`, `ReadOnly`→`read_only`.
`mirror_state`: from `SYS.Database:List` `Mirrored` column — `"0"` → `"none"`.
Optional name filter: `Do rs.Execute(name)` (single database).

## dataPolicy Handling for view_processes

`view_processes` is the only new action that interacts with `dataPolicy`. The handling
logic in `observability.rs` (or `admin.rs`):

1. Call `dispatch_gate()` — if blocked, return error immediately.
2. If `dataPolicy == block`: return `DATA_POLICY_BLOCKED` before any IRIS call.
3. Execute IRIS query and parse response.
4. If `dataPolicy == redact`: iterate `processes` array and replace `username`,
   `client_name`, `client_ip` with `"[REDACTED]"`.
5. Return result.

`view_locks` skips step 2 and 4 (not PHI-gated).

## journal_search Bulk-PHI Hard-Block

`journal_search` must check `dataPolicy == allow` before any IRIS call, with no
`acknowledgePhi` bypass. This check lives in the action handler, not in `dispatch_gate()`.
Consistent with spec 051 FR-009 (`journal_search` is a hard-blocked bulk-PHI tool).

## Error Code Additions

| Code | Used by | Meaning |
|------|---------|---------|
| `MISSING_PARAMS` | `journal_search` | Neither `global_pattern` nor `time_range` provided |
| `NAMESPACE_NOT_FOUND` | `namespace_mappings` | Requested namespace does not exist in IRIS |
| `DATABASE_NOT_FOUND` | `database_status` | Requested database name not found |

All three are added to the error code registry comment in `gate.rs` and documented in
`AGENTS.md` Section 6.

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Zero-Install Binary | Pass | Uses `execute_via_generator` (HTTP); no new install step |
| II. ObjectScript Sanity | Pass | All APIs are %SYS SQL tables available in IRIS 2022+; verify exact table names during implementation |
| III. HTTP-First Execution | Pass | `execute_via_generator` is HTTP-only; no `IRIS_CONTAINER` required |
| IV. Test-First, Fixture-Driven | Pass | Unit tests precede implementation in all phases; integration tests in `tests/integration/` |
| V. Output Shape Parity | Pass | All five response shapes documented in this plan; error codes follow existing pattern |
| VI. Environment Guard | Pass | All five classified as `ToolCategory::Query`; `dispatch_gate()` called before every IRIS call |
| VII. Dependency Minimalism | Pass | No new crates; `serde_json`, `reqwest` already in workspace |

## Phase Structure

1. **Setup**: New `observability.rs` skeleton (or extend `admin.rs`) + action arm stubs in
   dispatcher + error code registry additions
2. **Foundational**: dataPolicy helper for `view_processes`; journal bulk-PHI hard-block
   guard; MISSING_PARAMS validation
3. **US1 (view_locks)**: Unit tests → implementation
4. **US2 (view_processes)**: Unit tests → implementation (dataPolicy block/redact/allow)
5. **US3 (journal_search)**: Unit tests → implementation (bulk-PHI guard, %SYS execution)
6. **US4 (namespace_mappings)**: Unit tests → implementation (NAMESPACE_NOT_FOUND)
7. **US5 (database_status)**: Unit tests → implementation (DATABASE_NOT_FOUND, mirror_state)
8. **Polish**: Integration tests, AGENTS.md update, `check_config` schema update, fmt/clippy
