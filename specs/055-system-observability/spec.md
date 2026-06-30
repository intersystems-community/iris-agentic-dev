# Feature Specification: System Observability Depth

**Feature Branch**: `055-system-observability`
**Created**: 2026-06-29
**Status**: Implemented
**Depends on**: 051 (PHI policy and env gates — merged), 052 (iris_global — merged)

## Overview

iris-agentic-dev has an `iris_admin` tool that covers namespace, database, user, and webapp
management but has no visibility into the real-time state of a running IRIS instance. AI
agents troubleshooting performance issues, diagnosing lock contention, or auditing a
production system cannot answer basic operational questions: who is holding a lock? which
processes are running? what did the journal record at time T?

This feature extends `iris_admin` with five read-only observability capabilities:

1. **view_locks** — read the active IRIS lock table (resource names, owners, type, mode)
2. **view_processes** — list active IRIS processes with PID, username, namespace, state,
   client info
3. **journal_search** — search the IRIS journal by global name pattern and/or time range,
   returning records with global ref, value, transaction ID, and timestamp
4. **namespace_mappings** — inspect all global/package/routine mappings for a namespace
5. **database_status** — per-database mount state, free space, journal state, mirroring
   state

All five operations are read-only (`ToolCategory::Query`) and permitted on all `mcpTemplate`
values including `live`. `view_processes` output passes through the `dataPolicy` filter
(spec 051) because it exposes usernames and client info. `journal_search` is a bulk-PHI
tool hard-blocked unless `dataPolicy = allow`. Terminate-process and mount/dismount are
explicitly out of scope for v1 — too destructive.

---

## Clarifications

### Session 2026-06-29

- Q: Journal records can number in the millions. What is the default and max result limit for journal_search? → A: Default `max_records=100`, max allowed=1000. Always require at least one filter (`global_pattern` OR `time_range`) — bare `journal_search` with no filters returns `MISSING_PARAMS` to prevent runaway queries. Consistent with spec 052 and 053 clamping patterns.
- Q: Process list may expose client IP, username, and query text. Should PHI policy apply? → A: Yes — `view_processes` output passes through `dataPolicy` filter (block/redact/allow) per spec 051 rules. With `dataPolicy=block`, process list is blocked. With `dataPolicy=redact`, usernames and client info are redacted. With `dataPolicy=allow`, full output returned. Consistent with how other data-bearing tools handle `dataPolicy`.
- Q: IRIS journal access requires %SYS namespace or specific privileges. Should journal_search work in any namespace or only %SYS? → A: `journal_search` always executes in `%SYS` namespace regardless of the connection namespace param — journal data lives in `%SYS`. If the connected user lacks `%SYS` privileges, IRIS returns a permission error which surfaces as `IRIS_EXECUTE_ERROR`. Document this in the spec assumptions.

### Session 2026-06-30

- Q: Should the five new functions live in a new `observability.rs` module or extend `admin.rs` directly? → A: New `observability.rs` module — five `pub async fn` implementations in a dedicated file, `pub mod observability` declared in `mod.rs`, dispatcher arms in the `iris_admin` match call `observability::*`. Keeps `admin.rs` focused on user/namespace/webapp management.
- Q: Should `journal_search` use the `%SYS.Journal.Record` SQL table or the ObjectScript `%SYS.Journal.File`/`%SYS.Journal.Record` class API? → A: SQL table (`%SYS.Journal.Record`). IRIS 2023+ is an acceptable floor — the version gap is small and SQL is cleaner to maintain in Rust. Glob→SQL `LIKE` translation applies (`*`→`%`, `?`→`_`). Time range uses `BETWEEN :from AND :to` with ISO 8601 input converted to IRIS `%TimeStamp` format.

---

## User Scenarios & Testing

### User Story 1 — View Active Lock Table (Priority: P1)

A developer troubleshooting deadlock or lock contention in an IRIS production system wants
to see which processes hold locks and on which resources.

**Why this priority**: Lock contention is the most common production blocker requiring
immediate visibility. Unlike processes, lock state is never sensitive — no PHI or user data
is in lock records. This is the lowest-risk, highest-value operation.

**Independent Test**: Call `iris_admin` with `action=view_locks`. On a live IRIS instance
with at least one active lock (e.g., from an open transaction), expect a `locks` array
containing at least one entry with `resource`, `owner_pid`, `lock_type`, and `lock_mode`.
On a quiet instance, expect an empty array (not an error).

**Acceptance Scenarios**:

1. **Given** `action=view_locks`, **When** IRIS has active locks, **Then** the response
   includes a `locks` array with entries containing `resource`, `owner_pid`, `lock_type`,
   `lock_mode`, and `owner_username`.
2. **Given** `action=view_locks`, **When** no locks are active, **Then** the response
   returns `{success: true, locks: [], count: 0}` (not an error).
3. **Given** `action=view_locks` on `mcpTemplate=live`, **Then** the call succeeds
   (read-only, permitted on live template).
4. **Given** `action=view_locks` with `dataPolicy=block`, **Then** the call succeeds —
   lock table does not contain PHI or user data, `dataPolicy` does not gate it.

---

### User Story 2 — List Active Processes (Priority: P1)

A platform engineer managing an IRIS instance wants to see all running IRIS processes with
their PID, username, namespace, state, and client connection details.

**Why this priority**: Process visibility is foundational for diagnosing hung processes,
identifying which application is causing load, and understanding the active session count.
Combined with view_locks, these two operations cover the most common operational triage
scenarios.

**Independent Test**: Call `iris_admin` with `action=view_processes` and
`dataPolicy=allow`. Expect a `processes` array with at least one entry (the connection
process itself). Each entry should include `pid`, `username`, `namespace`, `state`, and
`client_name`.

**Acceptance Scenarios**:

1. **Given** `action=view_processes` with `dataPolicy=allow`, **When** called, **Then** the
   response includes a `processes` array with entries containing `pid`, `username`,
   `namespace`, `state`, `client_name`, `client_ip`, and `routine`.
2. **Given** `action=view_processes` with `dataPolicy=block` (default), **Then**
   `DATA_POLICY_BLOCKED` is returned — process list exposes usernames and client IPs.
3. **Given** `action=view_processes` with `dataPolicy=redact`, **Then** `username`,
   `client_name`, and `client_ip` fields are replaced with `[REDACTED]`; `pid`,
   `namespace`, `state`, and `routine` are retained.
4. **Given** `action=view_processes` on `mcpTemplate=live` with `dataPolicy=allow`, **Then**
   the call succeeds (read-only operation, permitted on live).
5. **Given** optional `namespace` filter param, **When** provided, **Then** only processes
   in that namespace are returned.

---

### User Story 3 — Search Journal Records (Priority: P2)

A developer investigating a data integrity issue wants to search the IRIS journal for
changes to a specific global within a time window to reconstruct what happened.

**Why this priority**: Journal search is highly specialized and requires `%SYS` privilege.
P2 because it is rarely needed in normal operations and carries meaningful PHI risk (journal
contains all global writes, including PHI globals). However, it is uniquely valuable for
debugging data loss and auditing global changes.

**Independent Test**: Call `iris_admin` with `action=journal_search`,
`global_pattern="IrisDevTest.*"`, and `time_range={"from": "2026-06-29T00:00:00Z", "to":
"2026-06-30T00:00:00Z"}` with `dataPolicy=allow`. Expect a `records` array (may be empty).
Call with no `global_pattern` and no `time_range` — expect `MISSING_PARAMS`.

**Acceptance Scenarios**:

1. **Given** `action=journal_search` with no `global_pattern` and no `time_range`, **Then**
   `MISSING_PARAMS` error is returned — at least one filter is required.
2. **Given** `action=journal_search` with `dataPolicy` not equal to `allow`, **Then**
   `DATA_POLICY_BLOCKED` is returned — journal is a bulk-PHI source with no
   `acknowledgePhi` bypass.
3. **Given** `action=journal_search` with `dataPolicy=allow` and a valid `global_pattern`,
   **Then** a `records` array is returned with entries containing `global_ref`, `value`,
   `transaction_id`, `timestamp`, and `operation` (`SET` or `KILL`).
4. **Given** `max_records` exceeds 1000, **Then** it is clamped to 1000; `truncated: true`
   is set in the response if the result is capped.
5. **Given** the connected user lacks `%SYS` privileges, **Then** `IRIS_EXECUTE_ERROR` is
   returned with the IRIS permission message.
6. **Given** both `global_pattern` and `time_range` are provided, **Then** results are
   filtered to match both constraints (AND semantics).

---

### User Story 4 — Inspect Namespace Mappings (Priority: P2)

A developer configuring or debugging a multi-namespace IRIS installation wants to see the
global, package, and routine mapping configuration for a given namespace.

**Why this priority**: Namespace mappings are rarely checked but essential for understanding
why a global or class is unexpectedly missing or resolving to the wrong database. Read-only,
no PHI risk.

**Independent Test**: Call `iris_admin` with `action=namespace_mappings`,
`namespace="USER"`. Expect a `mappings` object with `globals`, `packages`, and `routines`
arrays.

**Acceptance Scenarios**:

1. **Given** `action=namespace_mappings` with a valid `namespace`, **Then** the response
   includes a `mappings` object containing `globals`, `packages`, and `routines` arrays,
   each with `name` and `database` fields.
2. **Given** the namespace does not exist, **Then** a structured error `NAMESPACE_NOT_FOUND`
   is returned (not a crash).
3. **Given** `namespace` param is omitted, **Then** the connection's active namespace is
   used as the default.
4. **Given** `action=namespace_mappings` on `mcpTemplate=live`, **Then** the call succeeds
   (read-only).

---

### User Story 5 — Query Database Status (Priority: P2)

A platform operator monitoring an IRIS installation wants to see the current mount state,
free space, journal participation, and mirroring state for every database.

**Why this priority**: Database status is essential for proactive monitoring — detecting
low free space, dismounted databases, or mirror synchronization lag before they cause
service degradation. Read-only, no PHI risk.

**Independent Test**: Call `iris_admin` with `action=database_status`. Expect a `databases`
array with at least one entry containing `name`, `directory`, `mounted`, `free_space_mb`,
`journal_state`, and `mirror_state`.

**Acceptance Scenarios**:

1. **Given** `action=database_status`, **Then** the response includes a `databases` array
   with entries containing `name`, `directory`, `mounted`, `free_space_mb`, `journal_state`,
   and `mirror_state`.
2. **Given** `action=database_status`, **When** a database is not mounted, **Then** its
   entry shows `mounted: false` and omits filesystem fields that are unavailable.
3. **Given** optional `name` filter param, **When** provided, **Then** only the matching
   database entry is returned (or `DATABASE_NOT_FOUND` if absent).
4. **Given** `action=database_status` on `mcpTemplate=live`, **Then** the call succeeds
   (read-only).

---

### Edge Cases

- **view_locks on quiet instance**: No locks active returns `locks: []`, not an error.
- **view_processes race condition**: A process may exit between enumeration start and finish;
  partial results are acceptable — never return a structured error for a vanished PID.
- **journal_search time range format**: Accept ISO 8601 strings only
  (`"2026-06-29T10:00:00Z"`). Horolog input is not supported in v1. Rust converts ISO 8601
  to `YYYY-MM-DD HH:MM:SS` before binding to the SQL `TimeStamp BETWEEN` clause.
- **journal_search on unmounted journal**: If the journal file for the requested time range
  is not mounted, IRIS returns a permission/file error — surface as `IRIS_EXECUTE_ERROR`
  with the IRIS message.
- **namespace_mappings for %SYS**: Permitted — %SYS is a valid namespace for mappings
  inspection. The system blocklist from spec 051 applies only to `iris_global` global reads,
  not namespace config queries.
- **database_status mirroring absent**: On non-mirrored installations, `mirror_state` is
  `"none"` (not null/absent) so consumers do not need nil-checks.
- **Large process lists**: IRIS can have hundreds of processes. No cap is enforced on
  `view_processes` — return all. Callers can apply the optional `namespace` filter.
- **journal_search with only time_range**: Valid — scans all globals in the time window up
  to `max_records`. Can be slow on active systems; document in tool description.
- **dataPolicy=redact for view_locks**: Lock resources are not personal data; `redact`
  behaves the same as `allow` for view_locks (no fields require redaction).
- **All five actions on mcpTemplate=test**: All permitted — all are `ToolCategory::Query`.

---

## Requirements

### Functional Requirements

- **FR-001**: `iris_admin` MUST support five new `action` values: `view_locks`,
  `view_processes`, `journal_search`, `namespace_mappings`, `database_status`.
- **FR-002**: All five actions MUST be classified as `ToolCategory::Query` — permitted on
  `mcpTemplate=dev`, `mcpTemplate=test`, and `mcpTemplate=live`. No new `ToolCategory`
  variant is needed.
- **FR-003**: All five actions MUST pass through `dispatch_gate()` (spec 051) before any
  IRIS call.
- **FR-004**: `view_locks` MUST return a `locks` array with entries containing `resource`,
  `owner_pid`, `lock_type`, `lock_mode`, and `owner_username`. Empty array is valid when no
  locks are active. `dataPolicy` does not gate `view_locks`.
- **FR-005**: `view_processes` MUST return a `processes` array. With `dataPolicy=allow`,
  all fields are returned: `pid`, `username`, `namespace`, `state`, `client_name`,
  `client_ip`, `routine`. With `dataPolicy=block`, the call MUST be blocked
  (`DATA_POLICY_BLOCKED`). With `dataPolicy=redact`, `username`, `client_name`, and
  `client_ip` MUST be replaced with `[REDACTED]`.
- **FR-006**: `view_processes` MUST support an optional `namespace` filter param that
  restricts results to processes in that namespace. Absent means all namespaces.
- **FR-007**: `journal_search` MUST require at least one of `global_pattern` (string,
  glob-style) or `time_range` (object with `from` and `to` ISO 8601 strings). A call with
  neither MUST return `MISSING_PARAMS`.
- **FR-008**: `journal_search` MUST be hard-blocked when `dataPolicy` is not `allow`.
  `acknowledgePhi: true` MUST NOT bypass this block (journal is a bulk-PHI source).
- **FR-009**: `journal_search` MUST always execute in `%SYS` namespace regardless of the
  connection namespace param.
- **FR-010**: `journal_search` `max_records` MUST default to 100 and be clamped to a
  maximum of 1000. When results are capped, the response MUST include `truncated: true`.
- **FR-011**: `journal_search` records MUST include `global_ref`, `value`,
  `transaction_id`, `timestamp` (ISO 8601), and `operation` (`SET` or `KILL`).
- **FR-012**: `namespace_mappings` MUST return a `mappings` object with `globals`,
  `packages`, and `routines` sub-arrays, each entry containing at minimum `name` and
  `database` fields.
- **FR-013**: `namespace_mappings` `namespace` param MUST default to the connection's
  active namespace when omitted.
- **FR-014**: `namespace_mappings` MUST return `NAMESPACE_NOT_FOUND` when the requested
  namespace does not exist (not a raw IRIS execute error).
- **FR-015**: `database_status` MUST return a `databases` array with entries containing
  `name`, `directory`, `mounted`, `free_space_mb`, `journal_state`, and `mirror_state`
  (use `"none"` for `mirror_state` on non-mirrored instances).
- **FR-016**: `database_status` MUST support an optional `name` filter param that restricts
  results to a single database, returning `DATABASE_NOT_FOUND` if the name is not found.
- **FR-017**: All five actions MUST be implemented in a new `observability.rs` module
  (`crates/iris-agentic-dev-core/src/tools/observability.rs`), declared as
  `pub mod observability` in `mod.rs`. The `iris_admin` dispatcher arms call
  `observability::*` functions. Not separate MCP tool names — `iris_admin` remains the
  single tool entry point.
- **FR-018**: Error codes `MISSING_PARAMS`, `NAMESPACE_NOT_FOUND`, and `DATABASE_NOT_FOUND`
  MUST be added to the error code registry comment in `gate.rs` and to `AGENTS.md`
  Section 6.
- **FR-019**: `check_config` response or the `iris_admin` tool schema description MUST
  include the five new action names so agents know they are available.

### Key Entities

- **LockEntry**: Active IRIS lock — `resource`, `owner_pid`, `lock_type`, `lock_mode`,
  `owner_username`.
- **ProcessEntry**: Active IRIS process — `pid`, `username`, `namespace`, `state`,
  `client_name`, `client_ip`, `routine`.
- **JournalRecord**: IRIS journal entry — `global_ref`, `value`, `transaction_id`,
  `timestamp`, `operation`.
- **NamespaceMappings**: Mapping config for a namespace — `globals`, `packages`, `routines`
  sub-arrays each with `name` and `database`.
- **DatabaseStatusEntry**: Per-database state — `name`, `directory`, `mounted`,
  `free_space_mb`, `journal_state`, `mirror_state`.

---

## Success Criteria

- **SC-001**: `view_locks` on a live IRIS with active locks returns at least one lock entry
  in under 500ms.
- **SC-002**: `view_locks` on a quiet IRIS returns `{success: true, locks: [], count: 0}`.
- **SC-003**: `view_processes` with `dataPolicy=allow` returns all process fields including
  `username` and `client_ip`.
- **SC-004**: `view_processes` with `dataPolicy=block` returns `DATA_POLICY_BLOCKED` with
  no IRIS call made.
- **SC-005**: `view_processes` with `dataPolicy=redact` returns process entries with
  `username`, `client_name`, and `client_ip` replaced by `[REDACTED]`; `pid`, `namespace`,
  and `state` are intact.
- **SC-006**: `journal_search` with no filters returns `MISSING_PARAMS`.
- **SC-007**: `journal_search` with `dataPolicy=block` returns `DATA_POLICY_BLOCKED` even
  when `acknowledgePhi: true` is passed.
- **SC-008**: `journal_search` with `dataPolicy=allow` and a valid `global_pattern` returns
  records containing `global_ref`, `timestamp`, and `operation` fields.
- **SC-009**: `journal_search` with `max_records=5000` clamps to 1000 and sets
  `truncated: true` when the result is capped.
- **SC-010**: `namespace_mappings` for a valid namespace returns `globals`, `packages`, and
  `routines` sub-arrays.
- **SC-011**: `namespace_mappings` for a non-existent namespace returns
  `NAMESPACE_NOT_FOUND` (not a crash or generic IRIS error).
- **SC-012**: `database_status` returns at least one database entry with `name`, `mounted`,
  `free_space_mb`, `journal_state`, and `mirror_state` fields present.
- **SC-013**: All five new actions succeed on `mcpTemplate=live` (permitted by gate).
- **SC-014**: All five new action names appear in the `iris_admin` tool schema or
  `check_config` output.

---

## Assumptions

- All five operations execute via HTTP ObjectScript execution (`execute_via_generator`),
  the same transport used by existing `iris_admin` actions.
- `view_locks` reads from IRIS system process/lock APIs — prefer SQL via `%SYS` tables
  (e.g., `%SYS.ProcessQuery`) over direct global access where SQL views exist.
- `view_processes` uses `%SYS.ProcessQuery` SQL view or equivalent `%SYS` table, executing
  in `%SYS` namespace.
- `journal_search` uses the `%SYS.Journal.Record` SQL table in `%SYS` namespace.
  IRIS 2023+ is the minimum version requirement — the SQL table is available on all
  supported versions at that floor. The connected IRIS user must have `%SYS` namespace
  access; lack of access surfaces as `IRIS_EXECUTE_ERROR`.
- `namespace_mappings` queries `Config.MapGlobals`, `Config.MapPackages`, and
  `Config.MapRoutines` tables in `%SYS` namespace.
- `database_status` queries `Config.Databases` and `SYS.Database` tables in `%SYS`
  namespace for mount state, size, and journal configuration.
- `journal_search` `global_pattern` is a glob pattern (`*` wildcard). Rust translates
  `*` → SQL `%` and `?` → `_`, escaping any literal `%` and `_` in the input before
  binding to `GlobalRef LIKE :pattern`. Time range uses `TimeStamp BETWEEN :from AND :to`
  with ISO 8601 input converted to IRIS `%TimeStamp` format (`YYYY-MM-DD HH:MM:SS`).
  Horolog input is not supported in v1 — ISO 8601 only.
- Terminate-process and mount/dismount database are explicitly out of scope for v1.
  Both are documented as follow-on candidates (`iris_admin action=terminate_process`,
  `action=mount_database`).
- `view_locks` does not gate on `dataPolicy` — lock table entries contain process IDs and
  resource names, not PHI or personal data.
- `dataPolicy=redact` for `view_processes` redacts `username`, `client_name`, and
  `client_ip` by replacing with `[REDACTED]`. All other fields (`pid`, `namespace`,
  `state`, `routine`) are retained.
- New error codes `MISSING_PARAMS`, `NAMESPACE_NOT_FOUND`, and `DATABASE_NOT_FOUND` are
  added to the error code registry comment in `gate.rs` and to `AGENTS.md` Section 6.
