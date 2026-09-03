# Feature Specification: Fresh Container Setup Actions

**Feature Branch**: `099-fresh-container-setup`
**Created**: 2026-09-02
**Status**: Draft

## Overview

Fresh IRIS containers require several first-boot steps before an agent can authenticate
and work normally: the forced-password-change flag must be cleared, locked accounts must
be unlocked, and occasionally users need to be re-enabled. These steps require knowing
specific `%SYS`-namespace API calls (`%SYSTEM.Security.ChangePassword`,
`Security.Users.Modify`) that are non-obvious and undiscoverable from the agent's
perspective.

This feature adds three new `iris_admin` actions — `clear_password_change_flag`,
`unlock_user`, and `fresh_container_setup` — that perform the standard first-boot
sequence so an agent can bring a fresh IRIS container to a working state without
needing to know the ObjectScript internals.

### Codebase context (for implementors)

- `iris_admin` dispatch: `mod.rs` ~line 7188 — `match action { ... }` pattern. New
  arms go in the same match. Impl functions live in
  `crates/iris-agentic-dev-core/src/tools/admin_tools.rs` as `admin::admin_*_impl`.
- Gate classification: `write_gate.rs` `mixed("iris_admin", ...)` table ~line 524.
  New actions must be added there as `WriteClass::Write`. The default fallthrough is
  `WriteClass::Destructive` — explicit entries prevent over-classification.
- Gate enforcement: `IRIS_WRITE_TOOLS_ENABLED` env var (not `IRIS_ADMIN_TOOLS`).
  The old tool description's `IRIS_ADMIN_TOOLS=1` wording is stale — write gate is
  the authoritative source.
- Connection pattern: `iris_opt = _iris_arc_hold.as_deref()` passed to impl functions;
  impl functions switch to `%SYS` namespace before calling Security APIs.

---

## User Scenarios & Testing

### User Story 1 — Clear the forced-password-change flag (Priority: P1)

An agent spinning up a fresh IRIS container finds that every authentication attempt is
rejected or the IRIS web portal forces a password change before the Atelier API is
usable. The agent needs to clear this flag programmatically.

**Why this priority**: This is the first blocker hit on every fresh container. Without
clearing it, no other iad tool works — the agent is completely locked out.

**Independent Test**: Spin up iris-dev-iris; call
`iris_admin action="clear_password_change_flag"`; verify a subsequent `iris_execute`
call succeeds.

**Acceptance Scenarios**:

1. **Given** a fresh IRIS container with forced-password-change set on `_SYSTEM`,
   **When** `iris_admin action="clear_password_change_flag"` is called,
   **Then** the flag is cleared and subsequent authentication succeeds.
2. **Given** an account where the flag is already cleared,
   **When** the action is called,
   **Then** it succeeds idempotently — no error.
3. **Given** an account that does not exist,
   **When** the action is called with that username,
   **Then** a structured error is returned naming the missing account.
4. **Given** write gate disabled (`IRIS_WRITE_TOOLS_ENABLED` not set),
   **When** the action is called,
   **Then** it returns `WRITE_TOOLS_DISABLED` before touching IRIS.

---

### User Story 2 — Full fresh container setup sequence (Priority: P1)

An agent bootstrapping a new IRIS container wants a single action that runs the entire
standard first-boot sequence: clear forced-change flag, unlock the account, verify
connectivity.

**Why this priority**: Reduces agent reasoning burden — the agent shouldn't need to
know the correct order of operations or which accounts to check.

**Independent Test**: Call `iris_admin action="fresh_container_setup"` against
iris-dev-iris; verify subsequent `iris_execute` and `iris_query` calls succeed.

**Acceptance Scenarios**:

1. **Given** a fresh IRIS container,
   **When** `iris_admin action="fresh_container_setup"` is called,
   **Then** the result lists each step performed and its outcome
   (`clear_password_change_flag: ok`, `unlock_user: ok`, etc.).
2. **Given** a step that is already satisfied (e.g. flag already cleared),
   **When** the action runs,
   **Then** it continues with remaining steps and reports each step's status —
   does not abort.
3. **Given** the action completes,
   **When** the result is returned,
   **Then** it includes `ready: true/false` indicating whether the container is
   in a usable state.

---

### User Story 3 — Unlock a locked account (Priority: P2)

An agent needs to unlock a specific IRIS user account locked due to failed login
attempts or administrative action.

**Acceptance Scenarios**:

1. **Given** a locked user account,
   **When** `iris_admin action="unlock_user" username="<name>"` is called,
   **Then** the account is unlocked and a success response is returned.
2. **Given** a username that is not locked,
   **When** the action is called,
   **Then** it returns success idempotently.

---

## Functional Requirements

- **FR-001**: `iris_admin` gains three new actions: `clear_password_change_flag`,
  `fresh_container_setup`, `unlock_user`. Added to the `match action { ... }` block
  in `mod.rs` and delegated to `admin::admin_*_impl` functions in `admin_tools.rs`.

- **FR-002**: `clear_password_change_flag` params: `username` (default: `_SYSTEM`),
  `password` (default: `SYS`), `new_password` (optional, default: same as `password`).
  Implementation in `%SYS` (IRIS signature is `(Username, NewPassword, OldPassword, &Status)`):
  `Set result=##class(%SYSTEM.Security).ChangePassword(username, new_password_or_password, password, .sc)`.
  When `new_password` is omitted, both `NewPassword` and `OldPassword` are set to `password`
  — clears the force-change flag without changing the credential. When `new_password` is
  provided, it becomes the `NewPassword` arg (password change + flag clear in one call).

- **FR-003**: `unlock_user` params: `username` (required). Implementation in `%SYS`:
  set `props("InvalidLoginAttempts")=0` then
  `Set sc=##class(Security.Users).Modify(username, .props)`.
  (`Security.Users.UnlockUser` does NOT exist on IRIS 2026.2 — verified in research.)

- **FR-004**: `fresh_container_setup` params: `username` (default: `_SYSTEM`),
  `password` (default: `SYS`), `new_password` (optional). Runs in order:
  1. `clear_password_change_flag` (username, password)
  2. `unlock_user` (username)
     Returns a `FreshSetupResult` with per-step status. Continues on per-step errors.

- **FR-005**: All three actions execute in `%SYS` namespace. The `iris_execute`
  call (or docker exec stdin) must include `ZN "%SYS"` before the Security API calls.

- **FR-006**: All three actions classified `WriteClass::Write` in the `mixed("iris_admin")`
  table in `write_gate.rs`. They must be listed explicitly — do not rely on the
  `WriteClass::Destructive` default fallthrough.

- **FR-007**: Gate enforcement: blocked when `IRIS_WRITE_TOOLS_ENABLED` is not set.
  Returns `WRITE_TOOLS_DISABLED` error before any IRIS call. (Not `IRIS_ADMIN_TOOLS` —
  that env var is stale; `write_gate.rs` is authoritative.)

- **FR-008**: All three actions work via both execution paths: Atelier REST and
  docker exec (NoPWS / `docker_only=true`, spec 091).

- **FR-009**: Error responses: structured `{ success: false, error: string,
error_code: string }` — never raw ObjectScript error strings.

- **FR-010**: Enabling unauthenticated web access (editing `Security.Applications`)
  is out of scope for v1.

---

## Test Layers (three required)

1. **Unit** — parse params via `serde_json::from_value`; assert `clear_password_change_flag`
   with no `username` param defaults to `_SYSTEM`; assert `unlock_user` with missing
   `username` returns `INVALID_PARAMS`.

2. **Binary invocation** (`#[ignore]`, `IAD_BINARY` env) — spawn `iris-agentic-dev`,
   send `tools/call iris_admin action="fresh_container_setup"` **without** write gate
   enabled; assert response is `WRITE_TOOLS_DISABLED`.

3. **Live IRIS integration** (`#[ignore]`, iris-dev-iris, `--test-threads=1`) — call
   `iris_admin action="unlock_user" username="_SYSTEM"` with write gate enabled; assert
   `success: true` (idempotent — safe even if account is already unlocked).

---

## Key Entities

- **FreshSetupResult**:
  - `success`: bool — true if all steps completed without error
  - `ready`: bool — true if container appears usable after the sequence
  - `steps`: array of `{ action: string, status: "ok" | "skipped" | "error", detail: string }`

---

## Success Criteria

- A fresh IRIS container reaches a working state via
  `iris_admin action="fresh_container_setup"` with no prior knowledge of IRIS
  security APIs.
- The action is idempotent — calling it on an already-configured container is safe.
- All three actions pass three-layer tests: unit, binary invocation, live IRIS.
- The `write_gate.rs` `mixed("iris_admin")` table includes explicit `WriteClass::Write`
  entries for all three new actions — a test asserts this.

---

## Out of Scope

- Enabling unauthenticated web access (`Security.Applications` edits).
- Creating new users (covered by existing `create_user` action).
- Windows container or NoPWS docker exec path (spec 091 covers that integration;
  this spec covers the ObjectScript calls only).

---

## Assumptions

- Default `_SYSTEM` password on fresh ISC community containers is `SYS`. The action
  accepts an override for non-default images.
- `%SYSTEM.Security.ChangePassword` is present in all IRIS community builds (not
  Enterprise-only) — confirmed in research against iris-dev-iris 2026.2.
- `Security.Users.Modify` is available in `%SYS` on all builds. (`Security.Users.UnlockUser`
  does not exist — verified in research.)
