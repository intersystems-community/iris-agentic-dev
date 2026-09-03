# Feature Specification: Fresh Container First-Boot Setup

**Feature Branch**: `097-fresh-container-setup`
**Created**: 2026-09-02
**Status**: Draft

## Overview

Fresh IRIS community containers ship with a "must change password" flag set on `_SYSTEM`.
Every programmatic connection attempt fails with an opaque `Unexpected error: 1` until a
human clears that flag through Management Portal — which is unreachable because the web
port requires authentication. This chicken-and-egg blocks all agent-driven container setup.
Adding `action = "fresh_container_setup"` and `action = "clear_password_change_flag"` to
`iris_admin` breaks the cycle by executing the remediation through docker exec, bypassing
the web layer entirely.

---

## User Scenarios & Testing

### User Story 1 — First-boot setup via agent (Priority: P1)

A developer asks their agent to connect iad to a freshly started community container. The
agent calls `iris_add_server`, then `iris_test_server`. The test returns `Unexpected error:
1` — the classic first-boot failure. The agent calls
`iris_admin action=fresh_container_setup container_name=my-iris`. iad runs the
`Security.Users` ObjectScript via docker exec in `%SYS`, clears `ChangePassword`, and
returns `{setup_complete: true, user: "_SYSTEM", changed_password: false}`. The agent
re-runs `iris_test_server` — connection succeeds.

**Acceptance Scenarios**:

1. Given a fresh container with `ChangePassword=1` on `_SYSTEM`, When
   `iris_admin action=fresh_container_setup container_name=<name>` is called, Then the
   response is `{setup_complete: true, user: "_SYSTEM", changed_password: false}` and
   subsequent `iris_test_server` calls succeed.
2. Given the same fresh container and a `new_password` parameter, When
   `fresh_container_setup` is called, Then `ChangePassword` is cleared and the password is
   updated; response includes `changed_password: true`.
3. Given a container where the user already cleared the flag manually, When
   `fresh_container_setup` is called again, Then the action is idempotent — it succeeds
   with `{setup_complete: true, already_clear: true}`.
4. Given an unknown container name, When `fresh_container_setup` is called, Then the
   response is `{success: false, error_code: "CONTAINER_NOT_FOUND"}`.
5. Given `action = "clear_password_change_flag"` with `username=SuperUser`, When called on
   a running server reachable via HTTP, Then `ChangePassword` is cleared for that user via
   the normal Atelier REST path.

### User Story 2 — Flag-only reset for non-\_SYSTEM users (Priority: P2)

An operator provisioned a new application account and set a temporary password. The user's
first login sets `ChangePassword=1`. The operator wants to clear it without changing the
password. They call `iris_admin action=clear_password_change_flag username=AppUser`. The
flag is cleared; the user's password is unchanged.

**Acceptance Scenarios**:

1. Given a live server reachable via HTTP, When
   `iris_admin action=clear_password_change_flag username=AppUser` is called with
   `IRIS_ADMIN_TOOLS=1`, Then `Security.Users` `ChangePassword` is set to 0 for that user
   and `{success: true, user: "AppUser"}` is returned.
2. Given `IRIS_ADMIN_TOOLS` is unset, When `clear_password_change_flag` is called, Then
   `{error_code: "ADMIN_WRITE_DISABLED"}` is returned — same gate as all other admin write
   operations.

---

## Functional Requirements

- **FR-001**: `iris_admin action=fresh_container_setup` accepts `container_name` (required),
  `username` (default `_SYSTEM`), and `new_password` (optional). It runs ObjectScript in
  `%SYS` via docker exec rather than HTTP, so it works before web auth is functional.
- **FR-002**: The ObjectScript sequence is:
  `Set tSC=##class(Security.Users).%OpenId(<user>,.tUser)` →
  `Set tUser.ChangePassword=0` → optionally `Set tUser.Password=<new>` →
  `Set tSC=tUser.%Save()`. Error text from `$System.Status.GetErrorText(tSC)` is
  surfaced in the response.
- **FR-003**: `fresh_container_setup` does not require `IRIS_ADMIN_TOOLS=1` — it is
  gated on docker exec availability (i.e., the container must be local and reachable via
  `docker exec`). Rationale: the web layer is non-functional, so the normal HTTP admin
  gate cannot apply; docker exec is itself a privilege gate.
- **FR-004**: `action = "clear_password_change_flag"` accepts `username` (required) and
  operates via the normal HTTP/Atelier REST path. It requires `IRIS_ADMIN_TOOLS=1`.
- **FR-005**: Both actions run in the `%SYS` namespace — `Security.Users` lives there.
- **FR-006**: The `fresh_container_setup` response includes
  `{setup_complete: bool, user: string, changed_password: bool, already_clear: bool}`.
  `already_clear` is true when `ChangePassword` was already 0 before the call.
- **FR-007**: Unit test: parse the action dispatch and assert the docker exec code path is
  selected when `container_name` is present. Binary-invocation test: send `tools/call`
  with a non-existent container name, assert `CONTAINER_NOT_FOUND`. Live IRIS test:
  `#[ignore]` — run against `iris-dev-iris` after manually setting `ChangePassword=1`.

---

## Key Entities

- **`admin.rs`** (tools/admin.rs): add `fresh_container_setup_impl` and
  `clear_password_change_flag_impl`; dispatch from the `iris_admin` action router.
- **Docker exec helper** (docker.rs or inline): reuse or extend the existing docker exec
  path from spec 092/093 (docker-exec-fallback) — same mechanism, different payload.
- **`IrisAdminResponse`** (output_schemas.rs): extend to cover the
  `fresh_container_setup` response shape.

---

## Success Criteria

- A fresh community container can be fully set up for iad use in one agent turn with no
  human interaction with Management Portal.
- `clear_password_change_flag` works for any user on a live HTTP-reachable server.
- Neither action affects the IRIS_ADMIN_TOOLS gate in a way that surprises callers —
  `fresh_container_setup` bypasses it with a documented rationale; `clear_password_change_flag`
  obeys it.
- All three test layers (unit, binary invocation, live IRIS) are green.

---

## Out of Scope

- Automating the full first-boot sequence including namespace creation and SSL setup
  (that is a separate SOP skill — see spec 099).
- Supporting non-Docker remote IRIS instances for `fresh_container_setup` (remote
  instances must use `clear_password_change_flag` once the network path is open).
- Password complexity validation (IRIS enforces this; iad surfaces the error verbatim).

---

## Assumptions

- The docker exec approach reuses the mechanism introduced in spec 092/093
  (`docker_only=true` path). The container must be running and accessible by the process
  that runs iad.
- `Security.Users` `%OpenId` and `%Save` are available in all IRIS versions ≥ 2020.1.
- The `ChangePassword` property name is stable — verified against `Security.Users` class
  definition in 2026.2 community.
