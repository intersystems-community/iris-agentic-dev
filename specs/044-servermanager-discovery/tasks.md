# Tasks: Server Manager Connection Discovery & Policy (044)

**Input**: Design documents from `/specs/044-servermanager-discovery/`
**Branch**: `044-servermanager-discovery` (off `003-workspace-config`)

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Setup

**Purpose**: Add new source files, fixture files, and the `keyring` dependency.

- [X] T001 Add `keyring = { version = "4", features = ["v1"] }` and `keyring-core = { version = "1" }` to `crates/iris-agentic-dev-core/Cargo.toml` `[dependencies]`
- [X] T002 [P] Create `crates/iris-agentic-dev-core/src/iris/server_manager.rs` (empty module with `pub mod` stub)
- [X] T003 [P] Create `crates/iris-agentic-dev-core/src/iris/audit_log.rs` (empty module with `pub mod` stub)
- [X] T004 [P] Add fixture file `crates/iris-agentic-dev-core/tests/fixtures/sm_settings_single.json` — one server, no password field
- [X] T005 [P] Add fixture file `crates/iris-agentic-dev-core/tests/fixtures/sm_settings_multi.json` — three servers + `/default` key
- [X] T006 [P] Add fixture file `crates/iris-agentic-dev-core/tests/fixtures/sm_settings_malformed.json` — invalid JSON
- [X] T007 [P] Add fixture file `crates/iris-agentic-dev-core/tests/fixtures/sm_settings_deprecated_pw.json` — server with deprecated `password` field

**Checkpoint**: `cargo build` passes with new empty modules.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types shared by all user story phases.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T008 Add `ServerManagerProfile` struct (name, host, port, scheme, path_prefix, username, password_deprecated) to `crates/iris-agentic-dev-core/src/iris/server_manager.rs`
- [X] T009 Add `ConnectionPolicy` struct and `ToolCategory` enum to `crates/iris-agentic-dev-core/src/iris/workspace_config.rs`
- [X] T010 Add `ServerManager { server_name: String }` variant to `DiscoverySource` enum in `crates/iris-agentic-dev-core/src/iris/connection.rs`
- [X] T011 Add new error codes `SERVER_MANAGER_CREDENTIAL_ERROR`, `SERVER_MANAGER_AMBIGUOUS`, `POLICY_GATE` to the error response helpers in `crates/iris-agentic-dev-core/src/tools/mod.rs`
- [X] T012 Register `test_server_manager`, `test_policy_gate`, `test_audit_log`, `test_sm_e2e` in `crates/iris-agentic-dev-core/Cargo.toml` `[[test]]` sections

**Checkpoint**: `cargo build` passes; all new types compile.

---

## Phase 3: User Story 1 — Zero-Config Connection for VS Code Users (Priority: P1) 🎯 MVP

**Goal**: iris-agentic-dev discovers a Server Manager connection from VS Code settings.json and resolves credentials from the OS keychain without any `.iris-agentic-dev.toml`.

**Independent Test**: Delete `.iris-agentic-dev.toml`, set `IRIS_SERVER_NAME`, run `check_config` — see `server_manager.available: true` and `credential_status: "resolved"`, and tools work.

### Tests for User Story 1

> **Write these first — they must FAIL before implementation**

- [X] T013 [P] [US1] Write unit tests for `parse_sm_settings()` in `crates/iris-agentic-dev-core/tests/unit/test_server_manager.rs`
- [X] T014 [P] [US1] Write unit tests for credential resolution in `test_server_manager.rs` with fail-fast invariant
- [X] T015 [P] [US1] Write unit tests for server selection logic in `test_server_manager.rs`
- [X] T016 [US1] Write e2e test `test_sm_e2e.rs` (`#[ignore]`): full discovery + credential + tool call sequence

### Implementation for User Story 1

- [X] T017 [US1] Implement `parse_sm_settings(path: &Path) -> Vec<ServerManagerProfile>` in `server_manager.rs`
- [X] T018 [US1] Implement `sm_settings_path() -> Option<PathBuf>` in `server_manager.rs`
- [X] T019 [US1] Implement `resolve_credential(server_name, username)` using `keyring_core::Entry` with mock-store support
- [X] T020 [US1] Implement `select_server(profiles)` reading `IRIS_SERVER_NAME` env var
- [X] T021 [US1] Wire SM discovery into `crates/iris-agentic-dev-core/src/iris/discovery.rs`
- [X] T022 [US1] Verify T013–T015 unit tests pass — 18/18 GREEN

**Checkpoint**: US1 unit tests pass without IRIS. E2e test (`#[ignore]`) passes against real VS Code setup.

---

## Phase 4: User Story 4 — Server Manager Discovery in `check_config` (Priority: P2)

**Goal**: `check_config` shows a `server_manager` section listing discovered servers, credential status, and active policy.

**Independent Test**: Run `check_config` with Server Manager configured — response includes `server_manager` key with correct fields per `data-model.md` shape.

### Tests for User Story 4

- [X] T023 [P] [US4] Write unit tests in `test_server_manager.rs`: check_config SM section shape — all pass

### Implementation for User Story 4

- [X] T024 [US4] Implement `build_server_manager_config_json(profiles, active_name, cred_entries)` in `server_manager.rs`
- [X] T025 [US4] Wire into `check_config` handler in `crates/iris-agentic-dev-core/src/tools/mod.rs`
- [X] T026 [US4] Verify T023 unit tests pass — 3/3 GREEN

**Checkpoint**: `check_config` returns correct `server_manager` section shape in all scenarios.

---

## Phase 5: User Story 2 — Per-Connection Tool Policy (Priority: P2)

**Goal**: `[policy.<server-name>]` blocks in `.iris-agentic-dev.toml` gate tool calls by category; blocked calls return `POLICY_GATE` error with allowed list.

**Independent Test**: Add `[policy.prod] allow = ["query"]` to a toml, call `iris_compile` on that connection — get `policy_gate: true` with `allowed_categories: ["query"]`.

### Tests for User Story 2

- [X] T027 [P] [US2] Write unit tests in `test_policy_gate.rs` — 19/19 GREEN
- [X] T028 [P] [US2] TOML policy parsing tests — covered in test_policy_gate.rs (parse_policy_toml_*)

### Implementation for User Story 2

- [X] T029 [US2] Extend `FleetConfig` toml deserialization: `[policy.<server-name>]` into `HashMap<String, ConnectionPolicy>`
- [X] T030 [US2] Implement `policy_gate()` + `tool_to_category()` in `server_manager.rs`
- [X] T031 [US2] Wire `policy_gate()` into `iris_compile` handler
- [X] T032 [US2] Wire `policy_gate()` into `iris_execute` handler
- [X] T033 [US2] Wire `policy_gate()` into `iris_query` handler
- [X] T034 [US2] Wire `policy_gate()` into `iris_source_control` handler
- [X] T035 [US2] `active_server_manager_policy()` helper added to `IrisTools`
- [X] T036 [US2] Verify T027–T028 unit tests pass — 19/19 GREEN

**Checkpoint**: Policy gate fires correctly for all gated tools; permitted tools unaffected; no-policy connections unaffected.

---

## Phase 6: User Story 3 — Audit Log (Priority: P3)

**Goal**: Every tool call on a policy-gated connection appends a JSONL entry to `~/.iris-agentic-dev/audit.jsonl`; write failure is non-blocking.

**Independent Test**: Configure `[policy.prod]`, make 3 tool calls (2 allowed, 1 blocked), open `audit.jsonl` — 3 entries with correct fields; remove write permission from audit dir, make call — no error returned.

### Tests for User Story 3

- [X] T037 [P] [US3] Write unit tests in `test_audit_log.rs` — 10/10 GREEN (incl SC-002 latency assert)

### Implementation for User Story 3

- [X] T038 [US3] Implement `AuditLogEntry` struct in `audit_log.rs`
- [X] T039 [US3] Implement `AuditLog::write()` in `audit_log.rs`: append JSONL, non-blocking on error
- [X] T040 [US3] Wire audit logging into iris_compile, iris_execute, iris_query, iris_source_control
- [X] T041 [US3] Verify T037 unit tests pass — 10/10 GREEN

**Checkpoint**: Audit log written for all gated connections; no-policy connections produce no log; write failures non-blocking.

---

## Phase 7: Polish & Cross-Cutting Concerns

- [ ] T042 [P] Update `README.md`: add "Server Manager Zero-Config" section, add `IRIS_SERVER_NAME` to env var table, add `[policy.<server>]` to config reference
- [ ] T043 [P] Update `crates/iris-agentic-dev-core/tests/unit/test_role_gate_handlers.rs` regression: verify policy gate + role gate interop in combined scenario (policy fires, role gate not reached)
- [X] T044 [P] SC-004 latency test in `test_server_manager.rs` — assert parse on non-existent path < 200ms — GREEN
- [X] T045 `cargo fmt --all` — clean
- [X] T046 `cargo clippy -D warnings` — clean
- [X] T047 Full unit test suite — 47/47 GREEN
- [ ] T048 Verify quickstart.md steps work end-to-end against a real VS Code Server Manager setup

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — blocks all user story phases
- **Phase 3 (US1)**: Depends on Phase 2
- **Phase 4 (US4)**: Depends on Phase 3 (needs SM discovery infrastructure)
- **Phase 5 (US2)**: Depends on Phase 2; can run in parallel with Phase 3/4
- **Phase 6 (US3)**: Depends on Phase 5 (needs policy gate to determine audit trigger)
- **Phase 7 (Polish)**: Depends on all prior phases

### Parallel Opportunities

- T002, T003, T004–T007 all parallel (different files)
- T008–T011 parallel within Phase 2 (different structs/files)
- T013–T016 parallel (different test concerns, same file — use separate `#[test]` functions)
- Phase 3 (US1) and Phase 5 (US2) can proceed in parallel after Phase 2
- T027, T028 parallel within Phase 5 tests
- T031–T034 parallel (different handler files)
- T042, T043 parallel in Polish

---

## Implementation Strategy

### MVP (US1 only — Phase 1 + 2 + 3)

1. Complete Phase 1 + 2
2. Complete Phase 3 (US1)
3. **STOP**: verify `check_config` discovers Server Manager connection, credentials resolve, tools work without `.iris-agentic-dev.toml`
4. Demo-able at this point

### Full Delivery Order

Phase 1 → Phase 2 → Phase 3 (US1) → Phase 4 (US4) → Phase 5 (US2) → Phase 6 (US3) → Phase 7

Total tasks: **48**

- Setup: 7
- Foundational: 5
- US1 (P1): 10
- US4 (P2 — check_config): 4
- US2 (P2 — policy): 10
- US3 (P3 — audit): 5
- Polish: 7
