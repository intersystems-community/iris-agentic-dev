# Requirements Checklist: 055-system-observability

All items checked — requirements verified against spec.md as of 2026-06-29.

## Functional Requirements

- [x] FR-001 — `iris_admin` supports five new action values: `view_locks`, `view_processes`,
      `journal_search`, `namespace_mappings`, `database_status`
- [x] FR-002 — All five actions classified as `ToolCategory::Query`; permitted on all
      `mcpTemplate` values
- [x] FR-003 — All five actions pass through `dispatch_gate()` before any IRIS call
- [x] FR-004 — `view_locks` returns `locks` array with `resource`, `owner_pid`,
      `lock_type`, `lock_mode`, `owner_username`; empty array is valid; not gated by
      `dataPolicy`
- [x] FR-005 — `view_processes` behavior per `dataPolicy`: allow=full output,
      block=`DATA_POLICY_BLOCKED`, redact=sensitive fields replaced with `[REDACTED]`
- [x] FR-006 — `view_processes` supports optional `namespace` filter param
- [x] FR-007 — `journal_search` requires at least one of `global_pattern` or `time_range`;
      neither present returns `MISSING_PARAMS`
- [x] FR-008 — `journal_search` hard-blocked when `dataPolicy != allow`; `acknowledgePhi`
      does not bypass
- [x] FR-009 — `journal_search` always executes in `%SYS` namespace
- [x] FR-010 — `journal_search` `max_records` defaults to 100, clamps to 1000; sets
      `truncated: true` when capped
- [x] FR-011 — `journal_search` records include `global_ref`, `value`, `transaction_id`,
      `timestamp`, `operation`
- [x] FR-012 — `namespace_mappings` returns `mappings` with `globals`, `packages`,
      `routines` sub-arrays each with `name` and `database`
- [x] FR-013 — `namespace_mappings` defaults `namespace` to connection's active namespace
- [x] FR-014 — `namespace_mappings` returns `NAMESPACE_NOT_FOUND` for non-existent
      namespace
- [x] FR-015 — `database_status` returns `databases` array with `name`, `directory`,
      `mounted`, `free_space_mb`, `journal_state`, `mirror_state`; non-mirrored uses
      `"none"` for `mirror_state`
- [x] FR-016 — `database_status` supports optional `name` filter; returns
      `DATABASE_NOT_FOUND` when not matched
- [x] FR-017 — All five actions implemented in `admin.rs` or companion `observability.rs`;
      extend `iris_admin` dispatcher via new action enum values (not new tool names)
- [x] FR-018 — Error codes `MISSING_PARAMS`, `NAMESPACE_NOT_FOUND`, `DATABASE_NOT_FOUND`
      added to error code registry in `gate.rs` and `AGENTS.md` Section 6
- [x] FR-019 — `check_config` or `iris_admin` tool schema includes five new action names

## Success Criteria

- [x] SC-001 — `view_locks` returns lock entries in under 500ms on live IRIS
- [x] SC-002 — `view_locks` returns empty array on quiet IRIS
- [x] SC-003 — `view_processes` with `dataPolicy=allow` returns all fields including
      `username` and `client_ip`
- [x] SC-004 — `view_processes` with `dataPolicy=block` returns `DATA_POLICY_BLOCKED`
      with no IRIS call made
- [x] SC-005 — `view_processes` with `dataPolicy=redact` redacts `username`,
      `client_name`, `client_ip`; retains `pid`, `namespace`, `state`
- [x] SC-006 — `journal_search` with no filters returns `MISSING_PARAMS`
- [x] SC-007 — `journal_search` with `dataPolicy=block` blocked even with
      `acknowledgePhi=true`
- [x] SC-008 — `journal_search` with `dataPolicy=allow` and `global_pattern` returns
      records with required fields
- [x] SC-009 — `journal_search` with `max_records=5000` clamps to 1000, sets
      `truncated: true`
- [x] SC-010 — `namespace_mappings` for valid namespace returns three sub-arrays
- [x] SC-011 — `namespace_mappings` for non-existent namespace returns
      `NAMESPACE_NOT_FOUND`
- [x] SC-012 — `database_status` returns entries with all required fields
- [x] SC-013 — All five actions succeed on `mcpTemplate=live`
- [x] SC-014 — All five action names visible in tool schema or `check_config` output

## User Stories

- [x] US1 — view_locks (P1) — acceptance scenarios defined and testable independently
- [x] US2 — view_processes (P1) — dataPolicy gating defined; acceptance scenarios testable
- [x] US3 — journal_search (P2) — MISSING_PARAMS guard, bulk-PHI hard-block, %SYS
      execution defined
- [x] US4 — namespace_mappings (P2) — NAMESPACE_NOT_FOUND, namespace default defined
- [x] US5 — database_status (P2) — DATABASE_NOT_FOUND, mirror_state="none" defined

## Clarifications Recorded

- [x] journal_search result limit — max_records=100 default, 1000 max, MISSING_PARAMS on
      no-filter call
- [x] view_processes PHI policy — dataPolicy applies; block/redact/allow behavior defined
- [x] journal_search namespace — always executes in %SYS; permission error surfaces as
      IRIS_EXECUTE_ERROR
