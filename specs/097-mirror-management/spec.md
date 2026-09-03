# Feature Specification: Mirror Management Tools

**Feature Branch**: `097-mirror-management`
**Created**: 2026-09-02
**Status**: Draft

## Overview

`iris_mirror_status` (v1.3.0) tells an agent whether an IRIS instance is a mirror member
and what role it holds. That is read-only. Agents doing fleet setup or ops automation also
need to act: add an async member to an existing mirror set, and promote a backup to primary
when the primary is unavailable.

This spec adds two new actions exposed through `iris_admin` (the merged admin dispatcher):

- `action=mirror_add_async` — join a running IRIS instance to an existing mirror set as
  an async (disaster-recovery) member
- `action=mirror_failover` — promote a backup member to primary (destructive; gated)

**Implementation approach** (grounded in codebase review):

- Both actions follow the same pattern as existing `iris_mirror_status_impl`:
  `execute_via_generator` with `ZN "%SYS"` prefix + `%SYSTEM.Mirror.*` or `SYS.Mirror.*`
  classmethods, pipe-delimited output, parsed in Rust.
- Actions go in the `iris_admin` match block in `mod.rs` (line ~7188), delegating to new
  `impl` functions in `admin_tools.rs`.
- Gate classification in `write_gate.rs`: `mirror_add_async` → `WriteClass::Write`;
  `mirror_failover` → `WriteClass::Destructive`. The `iris_admin` mixed-gate entry (line
  ~524) must be updated with explicit rows for each new action.
- **Research complete** (Phase 0): `SYS.Mirror` classmethod signatures verified against
  iris-dev-iris. See `specs/097-mirror-management/research.md` for full signatures.
  `%SYSTEM.Mirror` (read) and `SYS.Mirror` (write) are different classes.

---

## User Scenarios & Testing

### User Story 1 — Add async mirror member (Priority: P1)

An ops agent stands up a DR instance and needs to join it to an existing mirror set as
an async member. Today this requires hand-crafted `iris_execute` calls against `%SYS`,
knowledge of `SYS.Mirror` API signatures, and careful SSL configuration.

**Version compatibility note**: Mirror membership requires compatible IRIS versions between
members. A 2025.1 primary and a 2026.3 async member may be rejected by the Mirror API.
The tool must surface the version mismatch as a structured error — not raw ObjectScript.

**Independent Test**: Can be verified by unit-testing param validation and checking the
pre-condition response (`iris_mirror_status` returning `is_member=false`). Full round-trip
requires a live mirror set (integration test gated on `IRIS_MIRROR_PRIMARY` env var).

**Acceptance Scenarios**:

1. Given an IRIS instance not yet in any mirror, When `iris_admin action=mirror_add_async`
   is called with valid mirror name, primary host, and port, Then the result contains
   `success: true` and the instance has joined the named mirror set.
2. Given an instance already in a mirror, When `mirror_add_async` is called, Then
   `success: false` with an explanation — not a panic.
3. Given a version mismatch between primary and async candidate, When the API call fails,
   Then the error names the version incompatibility explicitly.
4. Given missing required parameters (`mirror_name` or `primary_host`), When called, Then
   `success: false` naming the missing field — same pattern as existing iris_admin actions.
5. Given SSL required by the mirror set and `ssl_enabled=false`, When called, Then the
   error explains SSL is required and what parameters to provide.

### User Story 2 — Failover mirror to backup (Priority: P2)

An ops agent detects the primary is unreachable and needs to promote the backup to primary.
This is destructive and irreversible — the old primary cannot rejoin as primary without
manual intervention.

**Independent Test**: Requires a live two-member mirror set. Unit test: call without
destructive gate, assert `DESTRUCTIVE_TOOLS_DISABLED` error.

**Acceptance Scenarios**:

1. Given this instance is a backup member and destructive gate is enabled, When
   `iris_admin action=mirror_failover` is called, Then the instance is promoted to primary
   and the result contains `success: true` and `new_role: "primary"`.
2. Given destructive gate is disabled, When `mirror_failover` is called, Then rejected with
   `error_code: DESTRUCTIVE_TOOLS_DISABLED` — same enforcement as other destructive actions.
3. Given this instance is already the primary, When `mirror_failover` is called, Then
   `success: false` explaining no failover is needed.
4. Given this instance is not a mirror member, When `mirror_failover` is called, Then
   `success: false` with a clear explanation.

---

## Functional Requirements

### mirror_add_async

- **FR-001**: `iris_admin action=mirror_add_async` accepts: `mirror_name` (string,
  required), `primary_host` (string, required), `primary_port` (integer, default 2188),
  `instance_name` (string, optional, default `"IRIS"` — the IRIS instance name on the
  primary failover member), `async_member_type` (integer, optional, default 0 — 0=DR,
  1=ReadOnly, 2=ReadWrite), `ssl_enabled` (bool, default false), `ssl_cert_file` (string,
  optional).
- **FR-002**: Implementation in `admin_tools.rs` as `iris_mirror_add_async_impl`, following
  the same `execute_via_generator` + `ZN "%SYS"` pattern as `iris_mirror_status_impl`.
- **FR-003**: Verified `SYS.Mirror` write classmethod signatures (from research.md, confirmed
  against iris-dev-iris):
  - `##class(SYS.Mirror).JoinMirrorAsAsyncMember(MirrorSetName, SystemName, InstanceName,
AgentAddress, AgentPort, AsyncMemberType, .LocalInfo, .SSLInfo) As %Status`
  - `##class(SYS.Mirror).BecomePrimary() As %Boolean`
    Both are called in `%SYS` namespace via `ZN "%SYS"` prefix.
- **FR-004**: Version mismatch errors are caught by pattern-matching the ObjectScript error
  string and re-surfaced as `{ "success": false, "error_code": "MIRROR_VERSION_MISMATCH",
"error": "..." }`.
- **FR-005**: If the instance is already a mirror member, `mirror_add_async` returns
  `{ "success": false, "error_code": "ALREADY_MEMBER", "mirror_name": "..." }` without
  calling the Mirror API.

### mirror_failover

- **FR-006**: `iris_admin action=mirror_failover` takes one param: `confirm` (bool,
  required). Must be `true`; if `false` or missing, return
  `{ "success": false, "error_code": "CONFIRMATION_REQUIRED", "error": "confirm must be
true to execute failover" }`. This prevents accidental failover from agents that omit
  the flag.
- **FR-007**: Gate: classified as `WriteClass::Destructive` in `write_gate.rs`
  `CLASSIFICATION` table, `iris_admin` mixed entry. Blocked when `destructive_enabled =
false` — enforced by the existing `call_tool` dispatch before reaching the match arm.
- **FR-008**: Pre-flight via `iris_mirror_status`: if `is_primary = true`, return early
  with `{ "success": false, "error_code": "ALREADY_PRIMARY" }`.
- **FR-009**: If not a mirror member, return `{ "success": false, "error_code":
"NOT_MIRROR_MEMBER" }`.
- **FR-010**: On success, return `{ "success": true, "new_role": "primary" }`.

### Shared

- **FR-011**: Both actions added to the `iris_admin` tool description string (line ~7164
  in `mod.rs`) and to the `INVALID_ACTION` fallthrough error list (line ~7352).
- **FR-012**: Both actions added to `write_gate.rs` `CLASSIFICATION` `iris_admin` mixed
  entry with correct `WriteClass`.

### Tests (three layers per project constitution)

- **FR-013 Unit**: Parse action params from `serde_json::json!({ "action":
"mirror_add_async", ... })` — assert missing `mirror_name` returns INVALID_PARAMS;
  assert missing `primary_host` returns INVALID_PARAMS.
- **FR-014 Binary invocation** (`#[ignore]`, `IAD_BINARY`): spawn iad subprocess, call
  `iris_admin action=mirror_failover` without destructive gate set — assert response
  contains `DESTRUCTIVE_TOOLS_DISABLED`. Call `mirror_add_async` without live IRIS —
  assert tool is wired (response shape valid, not a panic).
- **FR-015 Live IRIS integration** (`#[ignore]`, iris-dev-iris, `--test-threads=1`): call
  `iris_admin action=mirror_add_async` on community iris-dev-iris — assert `success: false`
  with a clear error (community is not a mirror set). Skip when `IRIS_MIRROR_PRIMARY` env
  var IS set (full round-trip test environment supersedes this community test).

---

## Key Entities

- `mirror_add_async`: new `iris_admin` action. Implementation: `iris_mirror_add_async_impl`
  in `admin_tools.rs`. Classification: `WriteClass::Write`.
- `mirror_failover`: new `iris_admin` action. Implementation: `iris_mirror_failover_impl`
  in `admin_tools.rs`. Classification: `WriteClass::Destructive`.
- `SYS.Mirror` write classmethods: distinct from `%SYSTEM.Mirror` (read-only). Signatures
  verified in Phase 0 research — see `specs/097-mirror-management/research.md`.

---

## Success Criteria

- An agent can join a fresh IRIS instance to an existing mirror set by calling
  `iris_admin action=mirror_add_async` with mirror name, primary host, and port.
- Version mismatch is surfaced as a named error — not a raw ObjectScript stack trace.
- `mirror_failover` is blocked by the destructive gate when `IRIS_DESTRUCTIVE_TOOLS_ENABLED`
  is not set.
- All parameter validation errors name the missing or invalid field.
- Three test layers pass: unit validation, binary gate check, live non-member error.

---

## Out of Scope

- **Version compatibility matrix**: which IRIS versions can be mixed in a mirror set is
  an ISC support question. The tool surfaces the API error; compatibility is the operator's.
- **Automated failover policies**: iad executes actions agents request; it does not
  monitor or initiate.
- **Mirror set creation**: creating a new mirror from scratch (designating a primary) is
  not covered here. This spec covers joining an existing set only.
- **Removing a member**: `SYS.Mirror.RemoveMember()` is a separate destructive action.
- **Synchronous mirror members (backup)**: async is the DR use case; sync backup has
  stricter requirements and is deferred.

---

## Assumptions

- `SYS.Mirror` write classmethods are verified against live IRIS (see research.md):
  `JoinMirrorAsAsyncMember` and `BecomePrimary` are callable from ObjectScript executed
  via `execute_via_generator` in `%SYS` namespace.
- `%SYSTEM.Mirror` (read) and `SYS.Mirror` (write) are different classes — do not confuse
  them. `iris_mirror_status_impl` uses `%SYSTEM.Mirror`; new write actions use `SYS.Mirror`.
- The existing `iris_admin` mixed-gate pattern (one entry in `CLASSIFICATION` with a list
  of read-only action overrides and a `Destructive` default) is the correct extension point
  for new actions.
