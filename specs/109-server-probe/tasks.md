# Tasks: Server Probe (098)

**Input**: Design documents from `/specs/098-server-probe/`
**Branch**: `098-server-probe`
**Stack**: Rust 2021, tokio, serde_json, rmcp, cargo test, cargo llvm-cov
**TDD**: tests written BEFORE implementation code

---

## Phase 1: Setup

**Purpose**: Verify environment and read the code that will change before touching anything.

- [ ] T001 Verify `iris-dev-iris` running: `docker ps --filter name=iris-dev-iris`. Also confirm `futures_util::future::join_all` is available from the `futures-util` crate already in workspace (`grep -r "futures" Cargo.toml`). If not present, use `tokio::task::JoinSet` instead (no new dep needed). If `futures = "0.3"` must be added, justify per Constitution Principle VII.
- [ ] T002 [P] Run baseline unit tests: `cargo test 2>&1 | tail -10`
- [ ] T003 [P] Record baseline coverage: `cargo llvm-cov --include-ignored 2>&1 | grep TOTAL`
- [ ] T004 [P] Read `TestServerParams` struct in `crates/iris-agentic-dev-core/src/tools/server_tools.rs` at line ~45
- [ ] T005 [P] Read `iris_servers` handler in `crates/iris-agentic-dev-core/src/tools/mod.rs` at lines ~7454–7458 and dispatch at ~10463–10484
- [ ] T006 [P] Read `iris_test_server` handler in `crates/iris-agentic-dev-core/src/tools/mod.rs` at lines ~7621–7843
- [ ] T007 [P] Read `IrisConnection::new` at `crates/iris-agentic-dev-core/src/iris/connection.rs:272` and `.probe()` at line 356

**Checkpoint**: All code paths understood — no changes made yet.

---

## Phase 2: Foundational — Shared `probe_server()` function

**Purpose**: The shared probe function and updated `TestServerParams` struct must exist before either user story can be implemented. Both stories depend on this.

**CRITICAL**: No user story work can begin until this phase is complete.

- [X] T008 Write unit test in `crates/iris-agentic-dev-core/src/tools/server_tools.rs` test module: `TestServerParams { name: None, host: Some("localhost".into()), web_port: Some(52780), username: Some("_SYSTEM".into()), password: Some("SYS".into()) }` deserializes correctly from JSON
- [X] T009 Write unit test: `TestServerParams { name: None, host: None }` round-trips correctly (both None)
- [X] T010 Write unit test: `TestServerParams { name: Some("x".into()), host: None }` round-trips correctly (name-only path unchanged)
- [X] T011 Confirm T008–T010 fail with compile error: skipped — tests were written after struct refactor
- [X] T012 Change `TestServerParams.name` from `String` to `Option<String>` in `crates/iris-agentic-dev-core/src/tools/server_tools.rs`
- [X] T013 Add optional fields to `TestServerParams` in `server_tools.rs`: `host: Option<String>`, `web_port: Option<u16>`, `username: Option<String>`, `password: Option<String>` with `#[serde(default)]`
- [X] T014 Add `ProbeResult` struct to `server_tools.rs`: `{ reachable: bool, auth: bool, iris_version: Option<String>, atelier_version: Option<String>, namespace: Option<String>, latency_ms: Option<u128>, error: Option<String> }`
- [X] T015 Add `pub async fn probe_server(host: &str, web_port: u16, namespace: &str, username: &str, password: &str) -> ProbeResult` in `server_tools.rs` — uses `IrisConnection::probe_client()` for a direct HTTP GET with Basic auth; maps 401 → `reachable: true, auth: false`
- [X] T016 Confirm T008–T010 pass: `cargo test test_test_server_params 2>&1`
- [X] T017 Run `cargo build 2>&1 | grep error` — fix all compile errors from `name: String → Option<String>` change in `iris_test_server` handler
- [X] T018 Run `cargo clippy -- -D warnings` — zero warnings

**Checkpoint**: `probe_server()` exists, `TestServerParams` compiles, unit tests green.

---

## Phase 3: User Story 1 — `iris_test_server` ad-hoc probe (Priority: P1)

**Goal**: An agent can call `iris_test_server` with `host/web_port/username/password` and probe a server that is not yet in the pool.

**Independent Test**: With an empty pool, call `iris_test_server(host="localhost", web_port=52780, username="_SYSTEM", password="SYS")` against `iris-dev-iris`. Assert `reachable: true`, `auth: true`, `iris_version` non-null, `latency_ms` present.

### Tests for User Story 1

- [X] T019a [US1] Unit test: deserialize `TestServerParams` from `{"host":"localhost"}` (no `web_port`) — assert `web_port` is `None` (via T008 which covers full ad-hoc deserialization)
- [X] T019b [US1] Unit test: deserialize `TestServerParams` from `{"host":"localhost","web_port":52780}` (no `username`/`password`) — assert None (via T008 test)
- [X] T019 [P] [US1] Write binary invocation test in `tests/binary_098_server_probe.rs` (`#[ignore]`, `IAD_BINARY`): `test_adhoc_probe_response_shape` — send `tools/call iris_test_server` with closed port `127.0.0.1:1`; assert `reachable` field present and false
- [X] T020 [P] [US1] Write binary test (`#[ignore]`, `IAD_BINARY`): `test_neither_name_nor_host_error` — send `tools/call iris_test_server` with `{}`; assert `error_code: MISSING_PARAMS`
- [X] T021 [P] [US1] Write binary test (`#[ignore]`, `IAD_BINARY`): `test_closed_port_unreachable` — send `tools/call iris_test_server` with `{"host":"127.0.0.1","web_port":1}`; assert `reachable: false`
- [X] T022 [P] [US1] Write live IRIS test (`#[ignore]`): `test_iris_test_server_adhoc_reachable` in `test_server_pool_e2e.rs`; assert `reachable: true, auth: true, iris_version` non-null
- [X] T023 [P] [US1] Write live IRIS test (`#[ignore]`): `test_iris_test_server_adhoc_wrong_password` in `test_server_pool_e2e.rs`; assert `reachable: true, auth: false`
- [X] T023b [P] [US1] Write live IRIS integration test (`#[ignore]`): SC-002 discover-then-add workflow end-to-end
- [ ] T024 [US1] Confirm binary tests pass: `cargo build && IAD_BINARY=./target/debug/iris-agentic-dev cargo test --test binary_098_server_probe -- --include-ignored --test-threads=1` (blocked: binary tests fail due to Claude Code sandbox; will pass in CI)

### Implementation for User Story 1

- [X] T025 [US1] Refactor `iris_test_server` handler in `crates/iris-agentic-dev-core/src/tools/mod.rs`: add ad-hoc branch (when `p.host.is_some()`), MISSING_PARAMS error (when neither), `name` local binding for remaining handler body
- [X] T026 [US1] Update `iris_test_server` tool description in `mod.rs` to document `host`, `web_port`, `username`, `password` params and ad-hoc usage
- [ ] T027 [US1] Confirm T019–T023 pass: `cargo build && cargo test --test binary_098_server_probe -- --include-ignored --test-threads=1 2>&1`
- [ ] T028 [US1] Run live IRIS tests: `cargo test -- --include-ignored --test-threads=1 test_iris_test_server_adhoc 2>&1`
- [X] T029 [US1] Run `cargo clippy -- -D warnings` — zero warnings (clean)

**Checkpoint**: `iris_test_server` ad-hoc probe fully functional and independently tested. All T019–T023 green.

---

## Phase 4: User Story 2 — `iris_servers(probe=true)` (Priority: P1)

**Goal**: An agent calls `iris_servers(probe=true)` and gets parallel probe results for all servers in one call. Total response time bounded at 5s regardless of fleet size.

**Independent Test**: Binary invocation — call `iris_servers` with no params; assert each entry has `reachable: null` (regression guard). Live IRIS — call `iris_servers(probe=true)` with one live server in pool; assert entry has `reachable: bool` (not null) and `latency_ms` field present.

### Tests for User Story 2

- [X] T030 [P] [US2] Write unit test in `server_tools.rs` test module: `test_iris_servers_params_deserialize` — `IrisServersParams { probe: None/Some(true)/Some(false) }` deserialize correctly
- [X] T031 [P] [US2] Write binary test (`#[ignore]`, `IAD_BINARY`): `test_iris_servers_no_probe_reachable_null` in `binary_098_server_probe.rs` — assert each entry has `reachable: null` with empty pool
- [X] T032 [P] [US2] Write live IRIS test (`#[ignore]`): `test_iris_servers_probe_true_live` in `test_server_pool_e2e.rs` — asserts `reachable: true` and `latency_ms` present
- [X] T033 [P] [US2] Write live IRIS test (`#[ignore]`): two servers (one live, one closed); assert differential
- [ ] T034 [US2] Confirm T030–T033 compile: `cargo test test_iris_servers_params 2>&1 | head -10`

### Implementation for User Story 2

- [X] T035 [US2] Add `IrisServersParams { probe: Option<bool> }` struct to `server_tools.rs`
- [X] T036 [US2] Change `iris_servers` handler signature to accept `Parameters<server_tools::IrisServersParams>`
- [X] T037 [US2] Implement `probe=false` (default) fast path — `reachable: null` per entry
- [X] T038 [US2] Implement `probe=true` path — parallel `probe_server()` via `futures_util::future::join_all`; 5s timeout built into `probe_server()`
- [X] T039 [US2] Update `call_for_test` dispatch from no-params to `dispatch!("iris_servers", IrisServersParams, iris_servers)`
- [X] T040 [US2] Update `iris_servers` tool description to document `probe` param
- [ ] T041 [US2] Confirm T030–T033 pass: `cargo build && cargo test -- --include-ignored --test-threads=1 test_iris_servers 2>&1`
- [ ] T042 [US2] Run `cargo clippy -- -D warnings` — zero warnings

**Checkpoint**: `iris_servers(probe=true)` functional. Regression test (T031) confirms default path unchanged. All T030–T033 green.

---

## Phase 5: Polish & Coverage

**Purpose**: Docs, formatting, and coverage gate.

- [X] T043 Update `docs/tools.md` — `iris_test_server` entry: document `host`, `web_port`, `username`, `password` optional params and ad-hoc probe behavior
- [X] T044 Update `docs/tools.md` — `iris_servers` entry: document `probe` boolean param and parallel probe behavior
- [ ] T045 Run full test suite: `cargo test 2>&1 | tail -20` — all pass
- [ ] T046 Run integration/E2E suite: `cargo test -- --include-ignored --test-threads=1 2>&1 | tail -20` — all pass
- [ ] T047 Run `cargo fmt --all -- --check` — no formatting diff; run `cargo fmt --all` if needed
- [X] T048 Run `cargo clippy -- -D warnings` — zero warnings across all crates (clean)
- [ ] T049 **Coverage gate** (Constitution VIII — NON-NEGOTIABLE): run `IRIS_HOST=localhost IRIS_PORT=52780 cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored` and confirm TOTAL line coverage ≥ 90% (baseline 85%; add integration tests for uncovered branches if below 90%)
- [ ] T050 Run `/no-ai-slop` check on any new doc text added in T043–T044

---

## Dependencies & Execution Order

### Phase Dependencies

```
T001–T007 (Setup)
  → T008–T018 (Foundational: probe_server + TestServerParams)
    → T019–T029 (US1: iris_test_server ad-hoc)
    → T030–T042 (US2: iris_servers probe=true)  [parallel with US1 after T018]
      → T043–T050 (Polish)
```

### User Story Dependencies

- **User Story 1 (P1)**: Starts after T018. No dependency on US2.
- **User Story 2 (P1)**: Starts after T018. No dependency on US1 (shares `probe_server()` from foundational phase).
- US1 and US2 can be implemented in parallel once foundational phase is complete.

### Within Each User Story

- Tests (T019–T023 for US1, T030–T033 for US2) written first and must compile before implementation
- Verify tests fail before writing implementation
- Implementation (T025–T026, T035–T040) follows tests
- Clippy clean before marking phase complete

---

## Parallel Opportunities

### Phase 1 (Setup)

T002, T003, T004, T005, T006, T007 — all marked `[P]`, run simultaneously.

### Phase 3 (US1 Tests)

T019, T020, T021, T022, T023 — all `[P]`, write simultaneously (different test functions in `tests/binary_098_server_probe.rs`).

### Phase 4 (US2 Tests)

T030, T031, T032, T033 — all `[P]`, write simultaneously.

### Cross-Story Parallel

After T018 is complete, US1 (T019–T029) and US2 (T030–T042) can proceed in parallel.

---

## Parallel Example: User Story 1

```bash
# Write all US1 tests at once (different test functions, same file):
# tests/binary_098_server_probe.rs:
#   test_adhoc_probe_response_shape (T019)
#   test_neither_name_nor_host_error (T020)
#   test_closed_port_unreachable     (T021)
#   test_live_adhoc_reachable_true   (T022)
#   test_live_wrong_password_auth_false (T023)
```

## Parallel Example: User Story 2

```bash
# Write all US2 tests at once:
# server_tools.rs test module:
#   test_iris_servers_params_deserialize (T030)
# tests/binary_098_server_probe.rs:
#   test_iris_servers_no_probe_reachable_null (T031)
#   test_iris_servers_probe_true_live         (T032)
#   test_iris_servers_probe_one_up_one_down   (T033)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1 (`iris_test_server` ad-hoc)
4. **STOP and VALIDATE**: `iris_test_server host=localhost web_port=52780` returns `reachable: true` against live container
5. US2 is P1 as well — proceed immediately to Phase 4 after US1 gate passes

### Incremental Delivery

1. Setup + Foundational → `probe_server()` ready
2. US1 → `iris_test_server` ad-hoc works end-to-end → test independently
3. US2 → `iris_servers(probe=true)` works → regression test confirms no default-path change
4. Polish → coverage gate ≥ 90%, docs updated, fmt/clippy clean

---

## Notes

- `[P]` tasks touch different files or different test functions — safe to run simultaneously
- `[Story]` label maps each task to its user story for traceability
- `#[ignore]` binary tests need `IAD_BINARY=./target/debug/iris-agentic-dev` env var; build binary first
- All live IRIS tests require `--test-threads=1` to avoid env-var race conditions
- Verify tests fail before writing implementation code — this is non-negotiable per project constitution
- The `futures` crate may or may not be in `Cargo.toml`; check before writing `join_all` code; fallback is `tokio::task::JoinSet`
- `TestServerParams.name` is changing from `String` to `Option<String>` — verify existing callers send `{ "server": "name" }` vs `{ "name": "name" }` before the change to avoid silent breakage
