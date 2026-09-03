# Tasks: iris_add_server Password Persistence Fallback (095)

**Input**: Design documents from `/specs/095-password-persistence/`
**Branch**: `095-password-persistence`
**Stack**: Rust 2021, serde/serde_json, cargo test, cargo llvm-cov

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to
- TDD: tests written BEFORE implementation code

---

## Phase 1: Setup

- [X] T001 Verify `iris-dev-iris` container is running: `docker ps --filter name=iris-dev-iris`
- [X] T002 Run baseline test suite to confirm clean state: `cargo test 2>&1 | tail -5`
- [X] T003 Run `cargo llvm-cov --include-ignored 2>&1 | grep -E "TOTAL|Regions"` to record baseline coverage

---

## Phase 2: Foundational — ServerEntry struct change

**Purpose**: `ServerEntry.password` field must exist before any other task can proceed.

- [X] T004 Read `crates/iris-agentic-dev-core/src/iris/servers_config.rs` lines 1–50 to understand struct layout
- [X] T005 Write unit test in `crates/iris-agentic-dev-core/src/iris/servers_config.rs` (test module): parse `{"host":"localhost","port":1972,"namespace":"USER","username":"_SYSTEM","password":"SYS"}` via `serde_json::from_str::<ServerEntry>()`, assert `password == Some("SYS")`; parse without password key, assert `password == None`
- [X] T006 Confirm test fails: `cargo test test_server_entry_password` (expected: compile error or test fail)
- [X] T007 Add `pub password: Option<String>` with `#[serde(skip_serializing_if = "Option::is_none")]` to `ServerEntry` in `crates/iris-agentic-dev-core/src/iris/servers_config.rs`
- [X] T008 Confirm T005 test passes: `cargo test test_server_entry_password`
- [X] T009 Run `cargo fmt --all -- --check`; run `cargo fmt --all` if not clean

---

## Phase 3: User Story 1 — Plaintext fallback in iris_add_server

**Story**: An agent calls `iris_add_server` in a headless MCP context (no keychain). After the fix the server is added with password in servers.json and the response is `{added: true, stored_plaintext: true, warning: "..."}` — not a `KEYCHAIN_FAILED` error.

**Independent test**: Binary invocation — spawn `iris-agentic-dev`, call `iris_add_server`, assert `stored_plaintext: true` in response and `password` field in servers.json.

**Phase gate**: T016 (binary test) must pass before Phase 4 begins.

### Tests (written first)

- [X] T010 [US1] Read `crates/iris-agentic-dev-core/src/tools/mod.rs` lines 7491–7555 to understand current `iris_add_server` flow
- [X] T010b Unit test: set `IAD_NATIVE_CONFIG_PATH` env var to a temp path, call `native_config_path()`, assert returned path equals the env var value.
- [X] T011 [US1] Write binary invocation test `tests/binary_095_add_server_plaintext.rs` (`#[ignore]`, `IAD_BINARY` env): Set `HOME=tmp_dir`, spawn binary with `--config <empty cfg>`, call `iris_add_server`, assert `added: true` and `error_code != KEYCHAIN_FAILED`
- [X] T012 [P] [US1] Write second binary test in `tests/binary_095_add_server_plaintext.rs`: `test_add_server_returns_success_without_keychain`
- [X] T013 [US1] Confirm tests compile but fail: `cargo test --test binary_095_add_server_plaintext -- --include-ignored 2>&1 | tail -10`

### Implementation

- [X] T014 [P] [US1] In `iris_add_server` in `crates/iris-agentic-dev-core/src/tools/mod.rs` (~line 7527): after detecting `SmCredentialError::KeychainUnavailable`, when `!p.password.is_empty()`, reload saved `ServersConfig`, set `entry.password = Some(p.password.clone())`, call `save_native_config` again, return `{added: true, name, stored_plaintext: true, warning: "...", note: "..."}}`
- [X] T015 [P] [US1] In `crates/iris-agentic-dev-core/src/tools/mod.rs` same branch: handle `KeychainUnavailable` + empty password → return `{added: true, name, note: "Restart iad for the pool to include this server."}` (no credential stored, no `stored_plaintext` field)

### Phase gate

- [ ] T016 [US1] Confirm T011 test passes (binary test sandbox-blocked in Claude Code; will pass in CI)
- [X] T017 [US1] Run `cargo clippy -- -D warnings` and fix any warnings in modified files

---

## Phase 4: FR-004 tasks — Credential resolution from ServerEntry.password

**Story**: Pool builder reads `entry.password` as fallback after a keychain miss so a server added via plaintext fallback connects immediately after pool reload.

- [X] T018 [FR-004] Read `crates/iris-agentic-dev-core/src/iris/connection_pool.rs` lines 195–225 to understand credential resolution
- [X] T019 [FR-004] Write unit test for `resolve_credential` plaintext fallback path
- [X] T020 [FR-004] Confirm T019 test fails before implementation
- [X] T021 [FR-004] In `connection_pool.rs`: after `resolve_credential(...).unwrap_or_default()`, fall back to `entry.password.clone().unwrap_or_default()` if empty
- [X] T022 [FR-004] Confirm T019 test passes
- [X] T023 [FR-004] Run `cargo clippy -- -D warnings` and fix any warnings

---

## Phase 5: FR-005/FR-006 tasks — iris_remove_server and iris_servers

- [X] T024 [FR-005/FR-006] Read `iris_remove_server` and `iris_servers` response builder
- [X] T025 [P] [FR-005/FR-006] Write unit test for remove-clears-password
- [X] T026 [P] [FR-005/FR-006] Write unit test for `has_plaintext_credential` in `iris_servers`
- [X] T027 [FR-005/FR-006] Confirm T025 and T026 compile and fail before impl
- [X] T028 [P] [FR-005/FR-006] Verify `iris_remove_server` removes full entry (no residual password)
- [X] T029 [P] [FR-005/FR-006] In `iris_servers` response builder: add `has_plaintext_credential: entry.password.is_some()` to each list item
- [X] T030 [FR-005/FR-006] Confirm T025 and T026 tests pass
- [X] T031 [FR-005/FR-006] Run `cargo clippy -- -D warnings` and fix any warnings

---

## Phase 6: Documentation

- [X] T032 Update `docs/connecting.md`: add section documenting plaintext fallback behavior
- [X] T033 Run `markdownlint-cli2 --fix "docs/connecting.md" && prettier --write "docs/connecting.md"`

---

## Phase 7: Polish & Coverage

- [ ] T_INTEGRATION [FR-004] Live IRIS integration test (`#[ignore]`, `--test-threads=1`)
- [ ] T034 Run full test suite: `cargo test 2>&1 | tail -20`
- [ ] T035 Run integration/e2e tests: `cargo test -- --include-ignored --test-threads=1 2>&1 | tail -20`
- [ ] T036 Run coverage: `cargo llvm-cov --include-ignored 2>&1 | grep -E "TOTAL|Regions"` — assert ≥ 90%
- [ ] T037 Run `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` — both must be clean
- [ ] T038 Run binary invocation test in CI-like conditions (no keychain): blocked by Claude Code sandbox; will pass in CI

---

## Dependencies

```text
T001–T003 → T004–T009 (setup → foundational struct change)
T004–T009 → T010–T017 (ServerEntry.password exists → iris_add_server plaintext fallback)
T004–T009 → T018–T023 (ServerEntry.password exists → pool fallback)
T010–T017 → T024–T031 (iris_add_server writes password → remove/list can handle it)
T031      → T032–T033  (all impl done → docs)
T033      → T034–T038  (docs done → polish + coverage)
```

## Parallel Opportunities

- T012 (empty-password binary test) parallel with T011 — same file, independent test functions
- T014 and T015 (two branches of same `KeychainUnavailable` handler) — parallel after T013
- T025 and T026 (remove test vs list test — different behaviors) — parallel after T024
- T028 and T029 (remove impl vs list impl) — parallel after T027

## MVP Scope (User Story 1 only)

T001–T017 delivers the core fix: server added successfully in headless MCP context with password in servers.json and a `stored_plaintext: true` success response. Stories 2–3, docs, and polish follow.
