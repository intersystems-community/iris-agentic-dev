# Tasks: TOML Pool Hot-Reload (093)

**Input**: Design documents from `/specs/093-toml-hot-reload/`
**Branch**: `093-toml-hot-reload`
**Stack**: Rust 2021, tokio, rmcp, cargo test, cargo llvm-cov
**TDD**: tests written BEFORE implementation code

---

## Phase 1: Setup

- [ ] T001 Verify `iris-dev-iris` container running: `docker ps --filter name=iris-dev-iris`
- [ ] T002 Run baseline test suite: `cargo test 2>&1 | tail -5`
- [ ] T003 Record baseline coverage: `cargo llvm-cov --include-ignored 2>&1 | grep TOTAL`
- [X] T003b Write unit test asserting that a successful `iris_reload_pool` response includes a `servers_loaded` integer field (FR-001 requirement): deserialize the JSON response and assert `response["servers_loaded"].is_number()` is true; run as a binary invocation test (`IAD_BINARY`, `#[ignore]`) so no live IRIS is required

---

## Phase 2: Foundational — Pool field refactor

**Purpose**: `IrisTools.pool` must support atomic swap from `&self` before any reload logic can be added.

- [X] T004 Read `crates/iris-agentic-dev-core/src/tools/mod.rs` lines 2270–2340 to understand `IrisTools` struct and pool field initialization
- [X] T005 Grep all uses of `self.pool` in `mod.rs` to enumerate callsites that need updating
- [X] T006 Write unit test asserting `IrisTools.pool` can be swapped while a reader holds a reference (i.e., `Arc<RwLock<Arc<ConnectionPool>>>` semantics) — compile-time test is sufficient
- [X] T007 Change `pub pool: Arc<ConnectionPool>` to `pub pool: Arc<RwLock<Arc<ConnectionPool>>>` in `IrisTools` struct (`mod.rs:~2274`)
- [X] T008 Update all `self.pool` read callsites to `self.pool.read().unwrap().clone()` (get inner `Arc<ConnectionPool>`)
- [X] T009 Update pool initialization in `new()` / `build()` to wrap: `Arc::new(RwLock::new(Arc::new(load_pool(...))))`
- [X] T010 Run `cargo build 2>&1 | grep error` — fix all compile errors
- [X] T011 Run `cargo test 2>&1 | tail -10` — all existing tests must pass
- [X] T012 Run `cargo fmt --all -- --check` and `cargo clippy -- -D warnings`

---

## Phase 3: User Story 1 — `iris_reload_pool` tool

**Story**: Agent calls `iris_add_server`, then `iris_reload_pool`, then `iris_test_server` on new server — all in same session, no restart.

**Independent test**: Binary invocation — spawn binary, write entry to temp toml, call `iris_reload_pool`, call `iris_servers`, assert new server name appears.

- [X] T013 [US1] Read `load_pool` signature in `connection_pool.rs:191` and `write_gate.rs` to understand registration pattern
- [X] T014 [US1] Write binary invocation test T093-B1/B2/B3 in `crates/iris-agentic-dev-bin/tests/integration/binary_093_reload_pool.rs` (`#[ignore]`, `IAD_BINARY`): T093-B1: tool in tools/list; T093-B2: success JSON shape; T093-B3: TOML_PARSE_ERROR on bad config
- [X] T015 [US1] T093-B3 covers: call `iris_reload_pool` with a bad toml — assert `success: false`, `error_code: "TOML_PARSE_ERROR"`, and `note` mentions preserved pool
- [X] T016 [US1] T093-B2 covers: call `iris_reload_pool` with no config file — assert `servers_loaded: 0 or more`, `success: true`
- [X] T018 [US1] Added `iris_reload_pool` tool in `mod.rs` with TOML pre-validation
- [X] T019 [US1] Parse error handled: pre-validates TOML via `load_fleet_config_from_str` before swapping pool
- [X] T020 [US1] Registered `iris_reload_pool` in write_gate.rs as `ro("iris_reload_pool")`
- [X] T021 [US1] Tool wired via `#[tool_router]` macro — no manual dispatch needed; updated toolset counts (Baseline 83→84, Merged 80→81, total 92→93)
- [X] T022 [US1] Confirm T093-B1/B2/B3 binary tests pass: `cargo build && IAD_BINARY=./target/debug/iris-agentic-dev cargo test --test binary_093_reload_pool -- --include-ignored --test-threads=1`
- [X] T_LIVE_093_01 [US1] Write live IRIS test (`#[ignore]`, `iris-dev-iris` localhost:52780, `--test-threads=1`) in `crates/iris-agentic-dev-core/tests/integration/test_server_pool_e2e.rs`
- [ ] T024 [US1] Run `cargo clippy -- -D warnings` and fix warnings (deferred to Phase 5 polish)

---

## Phase 4: User Story 2 — Background ConfigWatcher pool reload

**Story**: Manual toml edit reflected within one subsequent tool call — no explicit `iris_reload_pool` needed.

**Independent test**: Write new server entry to toml directly, call `iris_servers`, assert new server appears without calling `iris_reload_pool`.

- [X] T025 [US2] Read `check_reload` in `mod.rs:2918` to understand existing mtime-change handling
- [X] T026 [US2] Write unit test: `test_config_watcher_fires_on_server_entry_added` in `config_watcher_tests` module in `mod.rs` — verifies watcher fires when config file content changes (precondition for pool swap)
- [X] T027 [US2] Extend `check_reload` in `mod.rs`: after connection swap at line ~3067, call `connection_pool::load_pool(None)`, wrap in `Arc::new`, swap via `*self.pool.write().unwrap()` — same pattern as `iris_reload_pool`
- [X] T028 [US2] Fail-safe: parse errors in `check_reload` return early before pool swap (line ~3012-3016) — existing pool preserved on parse error; log warning via tracing::info
- [X] T029 [US2] Write integration test `test_background_pool_reload` in `test_server_pool_e2e.rs` (`#[ignore]`, live IRIS, `--test-threads=1`): write `[instance.*]` entry to temp config, call `iris_servers` (triggers `check_reload`), assert new server appears without `iris_reload_pool`
- [X] T030 [US2] Confirm T029 passes against `iris-dev-iris`: `cargo test -- --include-ignored --test-threads=1 test_background_pool_reload 2>&1`
- [X] T031 [US2] Run `cargo clippy -- -D warnings`

---

## Phase 5: Polish & Coverage

- [ ] T032 Run full test suite: `cargo test 2>&1 | tail -20`
- [ ] T033 Run integration/e2e: `cargo test -- --include-ignored --test-threads=1 2>&1 | tail -20`
- [ ] T034 Run coverage: `cargo llvm-cov --include-ignored 2>&1 | grep TOTAL` — assert ≥ 90%
- [ ] T035 Run `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` — both clean
- [ ] T036 Run tool lift benchmark (Constitution Principle IX — `iris_reload_pool` is a new tool): document baseline and post-implementation lift in `specs/093-toml-hot-reload/lift-results.md`

---

## Dependencies

```
T001–T003 → T004–T012 (baseline → pool field refactor)
T004–T012 → T013–T024 (pool swappable → iris_reload_pool tool)
T004–T012 → T025–T031 (pool swappable → background watcher)
T013–T031 → T032–T036 (all impl done → polish)
```

## Parallel Opportunities

- T014, T015, T016 (three binary test cases) — parallel after T013
- T025–T031 (US2) can start after T012, in parallel with T013–T024 if two agents available

## MVP Scope

T001–T024: `iris_reload_pool` tool only (US1). Background watcher (US2) is a P2 enhancement.
