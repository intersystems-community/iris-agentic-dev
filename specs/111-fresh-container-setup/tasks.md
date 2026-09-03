# Tasks: Fresh Container Setup Actions (099)

**Input**: Design documents from `/specs/099-fresh-container-setup/`
**Spec**: `specs/099-fresh-container-setup/spec.md`
**Plan**: `specs/099-fresh-container-setup/plan.md`
**Branch**: `099-fresh-container-setup`

**Tech stack**: Rust 2021, `serde_json`, `tokio`, `rmcp` — no new crates.
**Key files**:

- `crates/iris-agentic-dev-core/src/tools/admin_tools.rs` — new impl functions
- `crates/iris-agentic-dev-core/src/tools/mod.rs` — dispatch arms (~line 7357) and INVALID_ACTION message
- `crates/iris-agentic-dev-core/src/tools/write_gate.rs` — `mixed("iris_admin", ...)` table (~line 524)
- `crates/iris-agentic-dev-core/tests/unit/test_gate_classification.rs` — gate unit tests
- `crates/iris-agentic-dev-core/tests/admin_e2e_tests.rs` — binary invocation tests
- New live IRIS test file: `crates/iris-agentic-dev-core/tests/integration/test_fresh_container_setup_live.rs`

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- All tasks include exact file paths

---

## Phase 1: Setup

**Purpose**: Verify the environment and create the new test file stubs. No code changes to existing files yet.

- [ ] T001 Verify `iris-dev-iris` is running: `docker ps --filter name=iris-dev-iris` and confirm port 52780 is accessible
- [ ] T002 [P] Create empty test file `crates/iris-agentic-dev-core/tests/integration/test_fresh_container_setup_live.rs` with module comment, `#![allow(unused)]`, and a placeholder `use` block importing `serde_json`
- [ ] T003 [P] Confirm the existing `crates/iris-agentic-dev-core/tests/admin_e2e_tests.rs` compiles without modification by running `cargo test --test admin_e2e_tests -- --list 2>&1 | head -5`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Wire the three new actions into `write_gate.rs` and assert that gate classification is correct before any impl function exists. These tasks MUST complete before any user story implementation.

**CRITICAL**: Tests in this phase come first. Gate entries must exist before dispatch arms, because the gate check happens before dispatch.

- [ ] T004 Write unit tests (in `crates/iris-agentic-dev-core/tests/unit/test_gate_classification.rs`) asserting that `iris_admin` actions `clear_password_change_flag`, `unlock_user`, and `fresh_container_setup` each classify as `WriteClass::Write` — not `WriteClass::Destructive`. Use the existing `classify("iris_admin", action)` call pattern already in that file. Run `cargo test -p iris-agentic-dev-core test_gate_classification` and confirm the new assertions **FAIL** (the actions are not yet in the table).
- [ ] T005 Add three entries to the `mixed("iris_admin", ...)` table in `crates/iris-agentic-dev-core/src/tools/write_gate.rs` (insert after the last `WriteClass::ReadOnly` entry, before the `WriteClass::Destructive` default at ~line 541):
  ```rust
  ("clear_password_change_flag", WriteClass::Write),
  ("unlock_user", WriteClass::Write),
  ("fresh_container_setup", WriteClass::Write),
  ```
  Run `cargo test -p iris-agentic-dev-core test_gate_classification` and confirm the new assertions **PASS**.

**Checkpoint**: Gate classification correct — user story work can begin.

---

## Phase 3: User Story 1 — Clear the forced-password-change flag (Priority: P1) — MVP

**Goal**: `iris_admin action="clear_password_change_flag"` clears the IRIS forced-password-change
flag for `_SYSTEM` (or a named user) by calling
`##class(%SYSTEM.Security).ChangePassword(Username, NewPassword, OldPassword, &Status)` in `%SYS`.
Optional `new_password` changes the credential at the same time.

**Independent Test**: `IRIS_WRITE_TOOLS_ENABLED=1 iris_admin action="clear_password_change_flag"` against
`iris-dev-iris` returns `{"success":true,"username":"_SYSTEM","flag_cleared":true}` and a
subsequent `iris_execute` succeeds.

### Tests for User Story 1

> Write these FIRST — confirm they FAIL before adding the impl function.

- [ ] T006 [P] [US1] Write unit test in `crates/iris-agentic-dev-core/tests/unit/test_fresh_container_setup_unit.rs` (new file):
  - `test_clear_password_flag_defaults`: parse `serde_json::json!({})` into params; assert `username` defaults to `"_SYSTEM"`, `password` to `"SYS"`, `new_password` equals `password` when omitted.
  - `test_clear_password_flag_explicit_new_password`: parse `{"new_password":"newpass"}`; assert `new_password == "newpass"`.
    Run `cargo test -p iris-agentic-dev-core test_clear_password_flag` — tests should **FAIL** (function not yet wired).
- [ ] T007 [P] [US1] Write binary invocation test in `crates/iris-agentic-dev-core/tests/admin_e2e_tests.rs` (add to existing file):
  - `test_clear_password_change_flag_write_gate_blocks` (`#[ignore]`): spawn binary via `mcp_exchange`, send `tools/call iris_admin action="clear_password_change_flag"` with NO `IRIS_WRITE_TOOLS_ENABLED`; assert response contains `"WRITE_TOOLS_DISABLED"`.
    Run to confirm it **FAILS** (dispatch arm not present yet, would get INVALID_ACTION not WRITE_TOOLS_DISABLED — this is the expected pre-impl failure).
- [ ] T008 [US1] Write live IRIS test in `crates/iris-agentic-dev-core/tests/integration/test_fresh_container_setup_live.rs`:
  - `test_clear_password_change_flag_idempotent` (`#[ignore]`): call `iris_admin action="clear_password_change_flag"` with `IRIS_WRITE_TOOLS_ENABLED=1` against `iris-dev-iris` localhost:52780; assert `success:true` and `flag_cleared:true`. (Idempotent — safe even if flag is already cleared.)
    Run to confirm it **FAILS** (not yet implemented).

### Implementation for User Story 1

- [ ] T009 [US1] Add `admin_clear_password_change_flag_impl` to `crates/iris-agentic-dev-core/src/tools/admin_tools.rs`:
  - Signature: `pub async fn admin_clear_password_change_flag_impl(iris: Option<&IrisConnection>, username: &str, password: &str, new_password: &str) -> Result<CallToolResult, McpError>`
  - Execute in `%SYS` namespace: `##class(%SYSTEM.Security).ChangePassword(username, new_password, password, .sc)` — note verified arg order: `(Username, NewPassword, OldPassword, &Status)`.
  - On IRIS return value `1` (success): `ok_json({"success":true,"username":username,"flag_cleared":true})`.
  - On IRIS return value `0` or error: `err_json("PASSWORD_CHANGE_FAILED", ...)`.
  - On HTTP failure: `err_json("IRIS_UNREACHABLE", ...)`.
- [ ] T010 [US1] Add dispatch arm to `iris_admin` match block in `crates/iris-agentic-dev-core/src/tools/mod.rs` (~line 7350, before the `_ =>` arm):
  ```rust
  "clear_password_change_flag" => {
      let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("_SYSTEM");
      let password = p.get("password").and_then(|v| v.as_str()).unwrap_or("SYS");
      let new_password = p.get("new_password").and_then(|v| v.as_str()).unwrap_or(password);
      admin::admin_clear_password_change_flag_impl(iris_opt, username, password, new_password).await
  }
  ```
- [ ] T011 [US1] Update the INVALID*ACTION error message in the `* =>`arm at`crates/iris-agentic-dev-core/src/tools/mod.rs`(~line 7352) to include`clear_password_change_flag` in the action list.
- [ ] T012 [US1] Run all US1 tests and confirm they pass:
  - `cargo test -p iris-agentic-dev-core test_clear_password_flag` (unit, no IRIS)
  - `IAD_BINARY=./target/debug/iris-agentic-dev cargo test --test admin_e2e_tests test_clear_password_change_flag_write_gate_blocks -- --ignored` (binary invocation)
  - `IRIS_WRITE_TOOLS_ENABLED=1 IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --test test_fresh_container_setup_live test_clear_password_change_flag_idempotent -- --ignored --test-threads=1` (live IRIS)

**Checkpoint**: US1 fully functional and independently testable.

---

## Phase 4: User Story 3 — Unlock a locked account (Priority: P2, implement before US2)

> US3 (unlock_user) is implemented before the full `fresh_container_setup` (US2) because
> `fresh_container_setup` calls both primitives. Completing US3 here unblocks US2.

**Goal**: `iris_admin action="unlock_user" username="<name>"` resets `InvalidLoginAttempts=0` via
`##class(Security.Users).Modify(username, .props)` in `%SYS`, unblocking a locked account idempotently.

**Independent Test**: `iris_admin action="unlock_user" username="_SYSTEM"` returns `{"success":true,"username":"_SYSTEM","unlocked":true}`.

### Tests for User Story 3

> Write FIRST — confirm they FAIL before impl.

- [ ] T013 [P] [US3] Add unit tests to `crates/iris-agentic-dev-core/tests/unit/test_fresh_container_setup_unit.rs`:
  - `test_unlock_user_missing_username`: call the dispatch layer with no `username` param; assert `INVALID_PARAMS` error code is returned before any IRIS call.
    Run `cargo test -p iris-agentic-dev-core test_unlock_user_missing_username` — expect **FAIL**.
- [ ] T014 [P] [US3] Add binary invocation test `test_unlock_user_write_gate_blocks` (`#[ignore]`) to `crates/iris-agentic-dev-core/tests/admin_e2e_tests.rs`: spawn binary, send `tools/call iris_admin action="unlock_user" username="_SYSTEM"` without write gate; assert `WRITE_TOOLS_DISABLED`.
- [ ] T015 [US3] Add live IRIS test `test_unlock_user_idempotent` (`#[ignore]`) to `crates/iris-agentic-dev-core/tests/integration/test_fresh_container_setup_live.rs`: call `iris_admin action="unlock_user" username="_SYSTEM"` with write gate enabled; assert `success:true` and `unlocked:true`.

### Implementation for User Story 3

- [ ] T016 [US3] Add `admin_unlock_user_impl` to `crates/iris-agentic-dev-core/src/tools/admin_tools.rs`:
  - Signature: `pub async fn admin_unlock_user_impl(iris: Option<&IrisConnection>, username: &str) -> Result<CallToolResult, McpError>`
  - Execute in `%SYS`: `Set props("InvalidLoginAttempts")=0  Set sc=##class(Security.Users).Modify(username,.props)`.
  - On `sc=1`: `ok_json({"success":true,"username":username,"unlocked":true})`.
  - On `sc≠1` or error text: `err_json("UNLOCK_FAILED", ...)`.
  - On HTTP failure: `err_json("IRIS_UNREACHABLE", ...)`.
- [ ] T017 [US3] Add dispatch arm in `crates/iris-agentic-dev-core/src/tools/mod.rs` (~line 7350):
  ```rust
  "unlock_user" => {
      let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("");
      if username.is_empty() {
          return err_json("INVALID_PARAMS", "username is required for unlock_user");
      }
      admin::admin_unlock_user_impl(iris_opt, username).await
  }
  ```
- [ ] T018 [US3] Update the INVALID_ACTION message in `crates/iris-agentic-dev-core/src/tools/mod.rs` to include `unlock_user`.
- [ ] T019 [US3] Run all US3 tests and confirm they pass (same three layers as T012 pattern).

**Checkpoint**: US3 fully functional and independently testable.

---

## Phase 5: User Story 2 — Full fresh container setup sequence (Priority: P1)

**Goal**: `iris_admin action="fresh_container_setup"` runs `clear_password_change_flag` then
`unlock_user` in sequence, returns a `FreshSetupResult` with per-step status, `success`, and
`ready` fields. Continues on per-step errors rather than aborting.

**Independent Test**: `iris_admin action="fresh_container_setup"` against `iris-dev-iris` returns
`{"success":true,"ready":true,"steps":[...]}` and subsequent `iris_execute` and `iris_query` succeed.

### Tests for User Story 2

> Write FIRST — confirm they FAIL before impl.

- [ ] T020 [P] [US2] Add unit tests to `crates/iris-agentic-dev-core/tests/unit/test_fresh_container_setup_unit.rs`:
  - `test_fresh_setup_result_ready_all_ok`: build a `FreshSetupResult` where all steps are `SetupStepStatus::Ok`; assert `success=true`, `ready=true`.
  - `test_fresh_setup_result_not_ready_on_error`: build a `FreshSetupResult` with one step `SetupStepStatus::Error`; assert `success=false`, `ready=false`.
  - `test_fresh_setup_result_json_shape`: serialize to JSON, assert fields `success`, `ready`, `steps[].action`, `steps[].status`, `steps[].detail` all present.
    Run to confirm **FAIL** (structs not defined yet).
- [ ] T021 [P] [US2] Add binary invocation test `test_fresh_container_setup_write_gate_blocks` (`#[ignore]`) to `crates/iris-agentic-dev-core/tests/admin_e2e_tests.rs`: send `tools/call iris_admin action="fresh_container_setup"` without `IRIS_WRITE_TOOLS_ENABLED`; assert `WRITE_TOOLS_DISABLED`.
- [ ] T022 [US2] Add live IRIS test `test_fresh_container_setup_idempotent` (`#[ignore]`) to `crates/iris-agentic-dev-core/tests/integration/test_fresh_container_setup_live.rs`:
  - Call `iris_admin action="fresh_container_setup"` with write gate enabled against `iris-dev-iris`.
  - Assert `success:true`, `ready:true`, `steps` array has exactly two entries: `clear_password_change_flag` and `unlock_user`, both `"ok"`.
  - Call again (idempotency check): assert same result.
  - After the call, verify a subsequent `iris_execute` call succeeds (container is usable).

### Implementation for User Story 2

- [ ] T023 [US2] Define `FreshSetupResult`, `SetupStep`, and `SetupStepStatus` structs/enums in `crates/iris-agentic-dev-core/src/tools/admin_tools.rs` (or a submodule if preferred). Derive `serde::Serialize`. `SetupStepStatus` variants: `Ok`, `Skipped`, `Error` — serialize as lowercase strings `"ok"`, `"skipped"`, `"error"`.
- [ ] T024 [US2] Add `admin_fresh_container_setup_impl` to `crates/iris-agentic-dev-core/src/tools/admin_tools.rs`:
  - Signature: `pub async fn admin_fresh_container_setup_impl(iris: Option<&IrisConnection>, username: &str, password: &str, new_password: &str) -> Result<CallToolResult, McpError>`
  - Step 1: call `admin_clear_password_change_flag_impl` — record `SetupStep { action: "clear_password_change_flag", status, detail }`.
  - Step 2: call `admin_unlock_user_impl` — record `SetupStep { action: "unlock_user", status, detail }`.
  - Continues on per-step errors (never early-returns on step failure).
  - `success = all steps ok/skipped`, `ready = no step has error`.
  - Return `ok_json(FreshSetupResult { ... })`. (Note: even if a step fails, the outer tool call returns `Ok` with `success:false` in the JSON body — does not return an MCP error.)
- [ ] T025 [US2] Add dispatch arm in `crates/iris-agentic-dev-core/src/tools/mod.rs` (~line 7350):
  ```rust
  "fresh_container_setup" => {
      let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("_SYSTEM");
      let password = p.get("password").and_then(|v| v.as_str()).unwrap_or("SYS");
      let new_password = p.get("new_password").and_then(|v| v.as_str()).unwrap_or(password);
      admin::admin_fresh_container_setup_impl(iris_opt, username, password, new_password).await
  }
  ```
- [ ] T026 [US2] Update the INVALID_ACTION message in `crates/iris-agentic-dev-core/src/tools/mod.rs` to include `fresh_container_setup`.
- [ ] T027 [US2] Run all US2 tests and confirm they pass (same three layers).

**Checkpoint**: All three actions implemented; US1, US2, US3 all independently pass three-layer tests.

---

## Phase 6: Tool Lift Measurement (Constitution IX)

**Purpose**: Measure whether the new `iris_admin` actions improve agent task success rate.

> **Lift applicability note**: `iris_admin` is not a standalone user-facing tool — it is an
> action dispatcher. The new `clear_password_change_flag`, `unlock_user`, and
> `fresh_container_setup` actions are invoked by agents, not directly by users. Constitution IX
> lift measurement is therefore scoped to the `iris_admin` tool description and action routing
> accuracy (does the agent pick the right action for a fresh-container task), not to end-user
> UX metrics. If the GEPA eval harness has no scenarios covering admin actions, record that
> explicitly in `lift-results.md` and note that coverage should be added before the next
> `iris_admin` change.

- [ ] T_LIFT_MEASURE [US2] Run GEPA eval harness; record A/B results in
      `specs/099-fresh-container-setup/lift-results.md` for `iris_admin` (new actions). Required
      before merge per Constitution IX. If no harness scenarios cover `iris_admin` admin actions,
      document that gap explicitly in `lift-results.md` as a known coverage hole — do not skip
      the task, record the N/A with justification.

---

## Phase 7: Polish & Cross-Cutting Concerns

- [ ] T028 [P] Add `test_fresh_container_setup_unit.rs` to the `[dev-dependencies]` / test module registration in `crates/iris-agentic-dev-core/Cargo.toml` (or the `tests/unit/mod.rs` if one exists) so it is included in `cargo test --lib`.
- [ ] T029 [P] Register `tests/integration/test_fresh_container_setup_live.rs` in the integration test manifest (`[[test]]` entry or inline in `Cargo.toml` if required by project convention — check existing integration test entries).
- [ ] T030 Update `docs/tools.md` — add the three new actions to the `iris_admin` section with parameter tables, example JSON, and a note that all three require `IRIS_WRITE_TOOLS_ENABLED=1`.
- [ ] T031 Run `cargo fmt --all -- --check` — zero formatting diff. Fix any issues.
- [ ] T032 Run `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings. Fix any issues.
- [ ] T033 Run the full gate classification suite to confirm no regressions: `cargo test -p iris-agentic-dev-core test_gate_classification`
- [ ] T034 Run the full unit suite (no IRIS required): `cargo test -p iris-agentic-dev-core --lib && cargo test -p iris-agentic-dev-core --tests unit`
- [ ] T035 Run the full three-layer test suite with live IRIS (`--test-threads=1` required):
  ```bash
  IRIS_WRITE_TOOLS_ENABLED=1 IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
    cargo test --test test_fresh_container_setup_live -- --ignored --test-threads=1
  IAD_BINARY=./target/debug/iris-agentic-dev \
    cargo test --test admin_e2e_tests -- --ignored --test-threads=1
  ```
- [ ] T036 **Coverage gate** (Constitution VIII — NON-NEGOTIABLE): build with coverage instrumentation and run full suite including ignored tests:
  ```bash
  cargo build -p iris-agentic-dev-bin
  IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_WRITE_TOOLS_ENABLED=1 \
  IAD_BINARY=./target/debug/iris-agentic-dev \
    cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored --test-threads=1
  ```
  Confirm TOTAL line coverage ≥ 90%. If below 90%, add tests for uncovered branches before marking complete.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately.
- **Phase 2 (Foundational)**: Depends on Phase 1 completion — **BLOCKS all user story implementation**. Gate entries must be in `write_gate.rs` before dispatch arms exist.
- **Phase 3 (US1 — clear_password_change_flag)**: Depends on Phase 2. No dependency on US2 or US3.
- **Phase 4 (US3 — unlock_user)**: Depends on Phase 2. No dependency on US1, but should complete before US2.
- **Phase 5 (US2 — fresh_container_setup)**: Depends on both US1 (Phase 3) and US3 (Phase 4) being implemented, since `fresh_container_setup` calls both primitives.
- **Phase 6 (Lift)**: Depends on all three user stories complete.
- **Phase 7 (Polish)**: Depends on Phase 6 (Lift) complete.

### User Story Dependencies

- **US1 (P1 — clear_password_change_flag)**: Independent — can start after Phase 2.
- **US3 (P2 — unlock_user)**: Independent — can start after Phase 2, in parallel with US1.
- **US2 (P1 — fresh_container_setup)**: Depends on US1 and US3 impl functions existing (T009, T016). Tests can be written in parallel.

### Within Each Phase

- Unit tests written **first**, confirmed failing before impl.
- Binary invocation tests written in parallel with unit tests (different file).
- Live IRIS tests written in parallel with unit tests (different file).
- Impl functions before dispatch arms (dispatch calls the impl).
- Dispatch arms before INVALID_ACTION message update.
- All three layers pass before phase checkpoint.

### Parallel Opportunities

- T002 and T003 (Phase 1): fully parallel.
- T006 and T007 (US1 tests): parallel — different files.
- T013 and T014 (US3 tests): parallel — different files.
- T020 and T021 (US2 tests): parallel — different files.
- T028 and T029 and T030 (Polish): parallel — different files.
- T031 and T032 (fmt + clippy): parallel.

---

## Parallel Execution Example: Phase 3 (US1)

```bash
# Parallel: write tests in different files simultaneously
Task T006: Write unit tests in tests/unit/test_fresh_container_setup_unit.rs
Task T007: Write binary invocation test in tests/admin_e2e_tests.rs
Task T008: Write live IRIS test stub in tests/integration/test_fresh_container_setup_live.rs

# Sequential: impl after tests confirmed failing
Task T009: Add admin_clear_password_change_flag_impl to admin_tools.rs
Task T010: Add dispatch arm to mod.rs
Task T011: Update INVALID_ACTION message
Task T012: Run all three test layers — confirm green
```

---

## Implementation Strategy

### MVP First (US1 alone)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (gate entries + tests)
3. Complete Phase 3: US1 (`clear_password_change_flag`)
4. **STOP and VALIDATE**: all three test layers green for US1
5. The most common fresh-container blocker is now solved.

### Full Delivery

1. Phase 1 + 2 → Foundation ready
2. Phase 3 (US1) + Phase 4 (US3) in parallel → both primitives done
3. Phase 5 (US2 — `fresh_container_setup`) → composite action complete
4. Phase 6 (Lift measurement) → GEPA results recorded
5. Phase 7 (Polish + coverage gate) → ready to merge

---

## Notes

- `[P]` = different files, no incomplete-task dependencies — truly parallelizable.
- `[Story]` label maps each task to the user story for traceability and independent delivery.
- `--test-threads=1` is **required** for all IRIS integration/e2e runs (race condition prevention).
- `IRIS_WRITE_TOOLS_ENABLED` is the authoritative gate env var — `IRIS_ADMIN_TOOLS` is stale.
- The `unlock_user` implementation uses `Security.Users.Modify` with `InvalidLoginAttempts=0` — `UnlockUser` does not exist on IRIS 2026.2 (verified in research.md).
- Arg order for `%SYSTEM.Security.ChangePassword` is `(Username, NewPassword, OldPassword, &Status)` — not the order stated in spec FR-002. research.md has the verified signature.
- `fresh_container_setup` must never return an MCP-level error for per-step failures — errors are surfaced inside `steps[]` so callers see which steps succeeded.
