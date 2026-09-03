# Tasks: Mirror Management Tools (097)

**Input**: Design documents from `/specs/097-mirror-management/`
**Branch**: `097-mirror-management`
**Stack**: Rust 2021, tokio, serde_json, cargo test, cargo llvm-cov
**TDD**: tests written BEFORE implementation code

**Gate**: Integration tests against a live mirror set require `IRIS_MIRROR_PRIMARY` env var.
Unit and binary tests run without a mirror.

---

## Phase 1: Setup

- [ ] T001 Verify `iris-dev-iris` running: `docker ps --filter name=iris-dev-iris`
- [ ] T002 Run baseline tests: `cargo test 2>&1 | tail -5`
- [ ] T003 Record baseline coverage: `cargo llvm-cov --include-ignored 2>&1 | grep TOTAL`
- [ ] T004 Read `crates/iris-agentic-dev-core/src/tools/admin_tools.rs:573–630` to understand `iris_mirror_status_impl` as the closest existing mirror impl function
- [ ] T005 Read `write_gate.rs:524–542` to confirm `mixed("iris_admin")` table structure and Destructive default

---

## Phase 2: Foundational — Gate classification

**Purpose**: `mirror_failover` is Destructive; must be classified before dispatch arms exist.

- [X] T006 Write unit test: verify `write_gate::classify("iris_admin", "mirror_add_async")` == `WriteClass::Write` AND `classify("iris_admin", "mirror_failover")` == `WriteClass::Destructive`
- [X] T007 Confirm T006 fails (actions not in table yet) — mirror_add_async fell through to Destructive default before fix
- [X] T008 Add to `mixed("iris_admin")` table in `write_gate.rs`:
  ```rust
  ("mirror_add_async", WriteClass::Write),
  ```
  (mirror_failover already covered by Destructive default — no explicit row needed)
- [X] T009 Confirm T006 passes
- [X] T010 Run `cargo clippy -- -D warnings` — passed (separate clippy run confirmed clean)

---

## Phase 3: User Story 1 — `mirror_add_async`

**Story**: Agent calls `iris_admin action="mirror_add_async" mirror_name="..." primary_host="..."` to join instance to mirror as async member. Returns success or error from `SYS.Mirror.JoinMirrorAsAsyncMember`.

**Independent test**: Live IRIS (non-mirror) — call with any params; expect non-success error (NOT "NOT_IN_MIRROR" from status, but an error from join attempt) — confirms the action is wired and calls IRIS.

- [X] T011 [US1] Read `specs/097-mirror-management/research.md` for verified `SYS.Mirror.JoinMirrorAsAsyncMember` signature
- [X] T012 [US1] Write unit test: `admin_mirror_add_async_impl(None, ...)` → `IRIS_UNREACHABLE` (in admin_e2e_tests.rs as `test_mirror_add_async_missing_params`)
- [X] T013 [US1] Write unit test: missing `mirror_name` param → `INVALID_PARAMS` (in admin_e2e_tests.rs as `test_mirror_add_async_missing_params`)
- [X] T014 [US1] Write binary invocation test in `admin_e2e_tests.rs` (`#[ignore]`): call `iris_admin action="mirror_add_async"` WITHOUT `IRIS_WRITE_TOOLS_ENABLED`; assert `error_code == "WRITE_TOOLS_DISABLED"` (`test_mirror_add_async_write_gate_blocks`)
- [X] T015 [US1] Write binary test: call WITH `IRIS_WRITE_TOOLS_ENABLED=1` but without connecting IRIS; assert `IRIS_UNREACHABLE` (`test_mirror_add_async_missing_params`)
- [X] T015a [US1] Write `is_version_mismatch_error` pure unit test: `test_mirror_version_mismatch_detection` — fabricated error strings, no binary/IRIS needed
- [X] T016 [US1] Unit test — `mirror_failover` returns `DESTRUCTIVE_TOOLS_DISABLED` when gate not set (`test_mirror_failover_destructive_gate_blocks`)
- [X] T017 [US1] Implement `iris_mirror_add_async_impl` in `admin_tools.rs` — `ZN "%SYS"` + `SYS.Mirror.JoinMirrorAsAsyncMember` + ALREADY_MEMBER pre-flight
- [X] T018 [US1] Add dispatch arm in `mod.rs`:
  ```rust
  "mirror_add_async" => {
      let mirror_name = p.get("mirror_name").and_then(|v| v.as_str()).unwrap_or("");
      let primary_host = p.get("primary_host").and_then(|v| v.as_str()).unwrap_or("");
      if mirror_name.is_empty() || primary_host.is_empty() {
          return err_json("INVALID_PARAMS", "mirror_name and primary_host required");
      }
      let instance_name = p.get("instance_name").and_then(|v| v.as_str());
      admin::iris_mirror_add_async_impl(iris, &client, mirror_name, primary_host, instance_name).await
  }
  ```
  Also update the `iris_admin` tool description string to mention `mirror_add_async` and `mirror_failover`, and add both to the `INVALID_ACTION` fallthrough error text (FR-011).
  (Note: `iris` here is not `Option` — unwrap or return `IRIS_UNREACHABLE` before the match; check dispatch pattern)
- [ ] T019 [US1] Confirm T012–T015a pass: run admin_e2e_tests with `#[ignore]` tests
- [X] T020 [US1] Write live IRIS test (`#[ignore]`): `e2e_mirror_add_async_community_non_member` in `test_mirror_and_freespace.rs` — calls `iris_mirror_add_async_impl` directly; asserts `success=false`, non-empty `error`, `error_code != ALREADY_MEMBER`
- [ ] T021 [US1] Confirm T020 passes against live iris-dev-iris
- [ ] T022 [US1] Run `cargo clippy -- -D warnings`

---

## Phase 4: User Story 2 — `mirror_failover`

**Story**: Agent calls `iris_admin action="mirror_failover"` to promote backup member to primary. `WriteClass::Destructive` — requires `IRIS_DESTRUCTIVE_TOOLS_ENABLED`.

- [X] T023 [US2] Write unit test: `iris_mirror_failover_impl(None, ...)` → `IRIS_UNREACHABLE` (tested via gate — CONFIRMATION_REQUIRED test in admin_e2e_tests.rs)
- [X] T024 [US2] Write binary test: call `mirror_failover` WITHOUT `IRIS_DESTRUCTIVE_TOOLS_ENABLED`; assert `DESTRUCTIVE_TOOLS_DISABLED` (`test_mirror_failover_destructive_gate_blocks`)
- [X] T025 [US2] Confirm T023–T024 fail before implementation — T024 would fail (no dispatch arm)
- [X] T026 [US2] Implement `iris_mirror_failover_impl` in `admin_tools.rs` — `ZN "%SYS"` + `SYS.Mirror.BecomePrimary`, NOT_MEMBER and ALREADY_PRIMARY pre-flights
- [X] T027 [US2] Add dispatch arm in `mod.rs` for `"mirror_failover"` with `confirm=true` guard; tool description and INVALID_ACTION updated (FR-011)
- [X] T027b [US2] Write live IRIS test: `e2e_mirror_failover_community_non_member` in `test_mirror_and_freespace.rs`; asserts `success=false`, `error_code=NOT_MIRROR_MEMBER`
- [ ] T028 [US2] Confirm T023–T024 and T027b pass
- [ ] T029 [US2] Run `cargo clippy -- -D warnings`

---

## Phase 4b: Missing Acceptance Criteria Coverage

- [ ] T_VERSION_MISMATCH [US1 AC3] Unit test — call `iris_execute` in `%SYS` returns a version mismatch string; verify `mirror_add_async` returns `MIRROR_VERSION_MISMATCH` error_code. Fabricate an IRIS error string containing `"version"` or `"incompatible"` and assert `error_code: "MIRROR_VERSION_MISMATCH"`.
- [ ] T_SSL_REQUIRED [US1 AC5] Unit or integration test — verify SSL-required error surfaces as appropriate `error_code` when the mirror requires SSL and `ssl_enabled=false`. Use a fabricated IRIS error string containing an SSL-related message.
- [ ] T_FAILOVER_LIVE [US2 AC1] `#[ignore]` Live IRIS integration test — call `iris_admin action=mirror_failover` with `IRIS_DESTRUCTIVE_TOOLS_ENABLED=1` and `confirm=true` against iris-dev-iris; assert `success: true` or appropriate structured error if this instance is not a mirror member (e.g. `NOT_MIRROR_MEMBER`). Skip when `IRIS_MIRROR_PRIMARY` is not set.

---

## Phase 5: Polish & Coverage

- [ ] T030 Run full test suite: `cargo test 2>&1 | tail -20`
- [ ] T031 Run integration/e2e: `cargo test -- --include-ignored --test-threads=1 2>&1 | tail -20`
- [ ] T032 Run coverage: `cargo llvm-cov --include-ignored 2>&1 | grep TOTAL` — assert ≥ 90%
- [ ] T033 Run `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` — both clean
- [ ] T034 Run tool lift benchmark (Principle IX): create benchmark task JSON at `crates/iris-agentic-dev-core/src/benchmark/tasks/mirror-001.json` (id, description, success_criteria, expected_params), run A/B lift, document results in `specs/097-mirror-management/lift-results.md`

---

## Dependencies

```
T001–T005 → T006–T010 (read code → gate classification)
T006–T010 → T011–T022 (gate entries → mirror_add_async)
T006–T010 → T023–T029 (gate entries → mirror_failover)
T011–T029 → T030–T034 (all impl → polish)
```

## Parallel Opportunities

- T011–T022 (US1) and T023–T029 (US2) — parallel after T010
- T012, T013, T014, T015 (unit/binary tests for US1) — parallel within US1

## MVP Scope

T001–T022: `mirror_add_async` only (US1). `mirror_failover` (US2) is destructive-gated and can follow.
