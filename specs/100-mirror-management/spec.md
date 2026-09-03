# Feature Specification: Mirror Management Actions

**Feature Branch**: `100-mirror-management`
**Created**: 2026-09-02
**Status**: Draft

## Overview

`iris_mirror_status` (added in spec 089) lets operators inspect mirror state, but iad
provides no tools to act on it. Operators troubleshooting a mirror outage need to add an
async member, trigger a planned failover, or retrieve recent mirror log entries — all
currently requiring a Management Portal session or direct ObjectScript access. This spec
adds three actions to `iris_admin`: `mirror_add_async`, `mirror_failover`, and
`mirror_log`. `mirror_failover` is a destructive operation and requires the destructive
gate; the other two are write operations guarded by `IRIS_ADMIN_TOOLS=1`.

---

## User Scenarios & Testing

### User Story 1 — Add async mirror member (Priority: P2)

An operator is expanding a mirror to add a DR async member. They call
`iris_admin action=mirror_add_async host=dr-host port=1972 instance_name=IRIS`. iad
executes the `%SYSTEM.Mirror` or `SYS.Mirror` classmethod in `%SYS`, and returns
`{success: true, action: "mirror_add_async", member: {host: "dr-host", port: 1972,
instance_name: "IRIS"}}`. The operator confirms with `iris_mirror_status`.

**Acceptance Scenarios**:

1. Given `IRIS_ADMIN_TOOLS=1` and a live mirror primary, When
   `iris_admin action=mirror_add_async host=<h> port=<p> instance_name=<i>` is called,
   Then the async member is added and `{success: true, action: "mirror_add_async"}` is
   returned.
2. Given `IRIS_ADMIN_TOOLS` is unset, When `mirror_add_async` is called, Then
   `{error_code: "ADMIN_WRITE_DISABLED"}` is returned.
3. Given the target host is unreachable, When `mirror_add_async` is called, Then the
   IRIS error text is surfaced in `{success: false, error_code: "MIRROR_ERROR",
error: "..."}`.
4. Given the instance is not a mirror member, When `mirror_add_async` is called, Then
   `{error_code: "NOT_MIRROR_MEMBER"}` is returned.

### User Story 2 — Planned mirror failover (Priority: P3)

An operator is doing a planned maintenance window and needs to fail over to the backup
member. They call `iris_admin action=mirror_failover`. Because this is destructive, iad
returns a confirmation token first. The operator confirms with the token. iad calls
`%SYSTEM.Mirror.Failover()` and returns `{success: true, action: "mirror_failover",
previous_role: "primary", new_role: "backup"}`.

**Acceptance Scenarios**:

1. Given `IRIS_ADMIN_TOOLS=1`, destructive gate enabled, and no prior confirmation token,
   When `mirror_failover` is called, Then `{error_code: "CONFIRM_REQUIRED",
confirm_token: "<token>", expires_in_seconds: 300}` is returned.
2. Given a valid confirmation token, When `mirror_failover` is called with
   `confirm=<token>`, Then failover is triggered and `{success: true,
action: "mirror_failover"}` is returned.
3. Given the destructive gate is disabled (`IRIS_DESTRUCTIVE_TOOLS=0`), When
   `mirror_failover` is called (even with a token), Then
   `{error_code: "DESTRUCTIVE_TOOLS_DISABLED"}` is returned.
4. Given an expired confirmation token, When `mirror_failover` is called, Then
   `{error_code: "CONFIRM_EXPIRED"}` is returned and a new token must be requested.
5. Given the instance is not a mirror primary, When `mirror_failover` is called, Then
   `{error_code: "NOT_MIRROR_PRIMARY"}` is returned before issuing any confirm token.

### User Story 3 — Mirror log retrieval (Priority: P2)

An operator investigating a mirror reconnect issue wants to see recent mirror log entries.
They call `iris_admin action=mirror_log limit=50`. iad queries the mirror log from `%SYS`
and returns a structured array of log entries with timestamps, severity, and message text.

**Acceptance Scenarios**:

1. Given a live mirror instance, When `iris_admin action=mirror_log` is called, Then a
   JSON array of log entries is returned, each with `{timestamp, severity, message}`.
2. Given `limit=N`, When `mirror_log` is called, Then at most N entries are returned,
   newest first.
3. Given `IRIS_ADMIN_TOOLS` is unset, When `mirror_log` is called, Then it succeeds —
   mirror log reads are read-only and do not require the write gate.
4. Given the instance has no mirror log, When `mirror_log` is called, Then
   `{entries: [], count: 0}` is returned — not an error.

---

## Functional Requirements

- **FR-001**: `action = "mirror_add_async"` accepts `host` (required), `port` (required,
  integer), and `instance_name` (required). Requires `IRIS_ADMIN_TOOLS=1`. Executes in
  `%SYS`. Investigate `%SYSTEM.Mirror` and `SYS.Mirror` class signatures before
  implementation — do not assume method names from `iris_mirror_status` (089 used
  `%SYSTEM.Mirror` read-only methods; write methods may differ).
- **FR-002**: `action = "mirror_failover"` follows the confirm-token pattern used by
  other destructive operations in `admin_tools.rs` (`ConfirmEntry`, 300-second expiry).
  In addition to the write gate, it must pass the destructive gate check
  (`ERR_DESTRUCTIVE_GATE`). Pre-check: verify the instance is a mirror primary before
  issuing a confirm token.
- **FR-003**: `action = "mirror_log"` accepts `limit` (optional, default 100, max 1000).
  Read-only; no gate required. Query via ObjectScript against the mirror log global or
  `%SYSTEM.Mirror` log accessor — confirm the actual API in the 2026.2 class reference
  before implementation.
- **FR-004**: All three actions run in `%SYS` namespace — mirror management APIs require
  it.
- **FR-005**: `mirror_failover` response includes `previous_role` and `new_role` if the
  failover completes synchronously; if IRIS initiates the failover asynchronously, the
  response notes `async: true` and advises the caller to poll `iris_mirror_status`.
- **FR-006**: `mirror_log` entries include at minimum `{timestamp: string, severity:
string, message: string}`. Additional fields from the IRIS log structure may be
  included if available.
- **FR-007**: Three test layers required:
  - Unit: parse action dispatch; assert `mirror_failover` without a confirm token returns
    `CONFIRM_REQUIRED`; assert `mirror_log` without the write gate succeeds (mock-free,
    pure logic).
  - Binary invocation: `tools/call mirror_failover` without destructive gate → assert
    `DESTRUCTIVE_TOOLS_DISABLED`; `tools/call mirror_log` → assert valid JSON shape.
  - Live IRIS (`#[ignore]`): run `mirror_log` against `iris-dev-iris` and assert at
    least one entry is returned (standalone IRIS is not mirrored, so `mirror_add_async`
    and `mirror_failover` live tests require a dedicated mirror test environment).

---

## Key Entities

- **`admin.rs`** (tools/admin.rs): add `mirror_add_async_impl`, `mirror_failover_impl`,
  `mirror_log_impl`; dispatch from `iris_admin` action router.
- **`admin_tools.rs`** (tools/admin_tools.rs): reuse `ConfirmEntry` and
  `ERR_DESTRUCTIVE_GATE` for `mirror_failover`.
- **`IrisAdminResponse`** (output_schemas.rs): extend to cover the three new action
  response shapes.
- **`%SYSTEM.Mirror`** / **`SYS.Mirror`**: IRIS system classes in `%SYS` — verify actual
  method signatures against the IRIS 2026.2 class reference before coding.

---

## Success Criteria

- `mirror_add_async` adds an async member on a live mirror primary and the new member
  appears in `iris_mirror_status` output.
- `mirror_failover` requires a valid confirmation token and the destructive gate; without
  either, it refuses with the correct error code.
- `mirror_log` returns structured entries that an agent can summarize for an operator
  without further parsing.
- All three test layers are green.

---

## Out of Scope

- Mirror creation from scratch (adding the first two primary/backup members) — that is a
  separate, larger operation.
- Removing a mirror member.
- Async member promotion to backup (requires `SYS.Mirror` arbitration — document as
  future work).
- Mirror configuration (network credentials, journal location) — Management Portal only
  for now.

---

## Assumptions

- `%SYSTEM.Mirror.Failover()` exists and is callable from ObjectScript in `%SYS`. Verify
  against IRIS 2026.2 class reference; if the method signature differs, adjust FR-002
  accordingly.
- The mirror log is accessible without a mirror configuration being active — `mirror_log`
  should return an empty array gracefully on a non-mirror instance rather than an error.
- The confirm-token mechanism in `admin_tools.rs` is reusable as-is; no new infrastructure
  is needed for `mirror_failover`.
- A dedicated mirror test environment for full live integration tests of `mirror_add_async`
  and `mirror_failover` is out of scope for the initial PR — those tests are added as
  `#[ignore]` stubs with a TODO pointing to the test environment setup.
