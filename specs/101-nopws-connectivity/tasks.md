# Tasks: NoPWS Connectivity (101)

**Input**: Design documents from `/specs/101-nopws-connectivity/`
**Branch**: `101-nopws-connectivity`
**Stack**: Rust 2021, tokio, serde/toml, reqwest, cargo test, cargo llvm-cov
**TDD**: tests written BEFORE implementation code

**Note**: Docker exec timeout is 30s (from existing `execute()` at `connection.rs:707`) — not 10s as in spec. SSH path: `ssh -o StrictHostKeyChecking=no <ssh_host> docker exec -i <container> iris session IRIS -U <ns>`.

**Note**: Three new `[[test]]` entries must be added to `crates/iris-agentic-dev-core/Cargo.toml` before the phase-gate commands will find these test binaries:

- `name = "nopws_101"`, `path = "tests/integration/nopws_101.rs"` (live IRIS)
- `name = "nopws_101_binary"`, `path = "tests/binary/nopws_101.rs"` (binary invocation, `#[ignore]`)
- `name = "nopws_skill_test"`, `path = "tests/skills/nopws_skill_test.rs"` (skill keywords)
  Add these entries as part of the first test-writing task in each phase. Binary invocation tests also require `cargo build --bin iris-agentic-dev` to complete before `IAD_BINARY` is valid.

---

## Phase 1: Setup

- [ ] T001 Verify `iris-dev-iris` running: `docker ps --filter name=iris-dev-iris`
- [ ] T002 Run baseline tests: `cargo test 2>&1 | tail -5`
- [ ] T003 Record baseline coverage: `cargo llvm-cov --include-ignored 2>&1 | grep TOTAL`

---

## Phase 2: Foundational — WorkspaceConfig new fields

**Purpose**: `nopws` and `ssh_host` fields must exist and be serde-wired before any routing logic can use them. FR-011 round-trip test required.

- [ ] T004 Read `crates/iris-agentic-dev-core/src/iris/workspace_config.rs` to locate `WorkspaceConfig` struct and `docker_only` field
- [ ] T005 Write unit test in `workspace_config.rs` test module using `toml::from_str`: parse `[instance.test]\nnopws = true\nssh_host = \"baystate\"\n` → assert `nopws == true`, `ssh_host == Some("baystate")`; parse without keys → assert defaults (`nopws=false`, `ssh_host=None`). This is the FR-011 serde silent-drop guard.
- [ ] T006 Confirm T005 fails: `cargo test test_workspace_config_nopws_fields 2>&1 | head -5`
- [ ] T007 Add `#[serde(default)] pub nopws: bool` and `pub ssh_host: Option<String>` to `WorkspaceConfig` in `workspace_config.rs`
- [ ] T008 Confirm T005 passes: `cargo test test_workspace_config_nopws_fields 2>&1`
- [ ] T009 Run `cargo clippy -- -D warnings` and `cargo fmt --all -- --check`

---

## Phase 3: User Story 1 — NoPWS flag + clear error messages in `iris_test_server` (P1)

**Story**: `nopws = true` in config; `iris_test_server` returns `nopws: true, web_available: false` with plain-language message and no raw "connection refused".

**Phase gate**: T018 (binary test) must pass before Phase 4.

- [ ] T010 [US1] Read `mod.rs:~7621–7843` to understand `iris_test_server` handler
  - **Test file naming**: iris.cpf detection tests (`probe_nopws_via_cpf`) belong in
    `test_nopws_detection.rs`; execute-path tests (docker exec early-branch,
    `execution_path` field) belong in `test_nopws_execute.rs`. T023, T043, and T053
    reference `test_nopws_detection.rs` for detection-path tests rather than
    `test_nopws_execute.rs`.
- [ ] T011 [US1] Write unit test: configure `WorkspaceConfig { nopws: true, ... }`, call `iris_test_server` against a closed web port, assert response has `nopws: true`, `web_available: false`, `message` field containing "NoPWS" and remediation steps (not raw "connection refused")
- [ ] T012 [US1] Write binary invocation test in `tests/binary_101_nopws.rs` (`#[ignore]`, `IAD_BINARY`): set env to simulate `nopws=true` config, call `iris_test_server`, assert `nopws: true` in response (FR-012)
- [ ] T013 [US1] Write live IRIS test (`#[ignore]`, `--test-threads=1`): call `iris_test_server` against `iris-dev-iris` (has web server); assert `nopws_detected: false` (FR-013)
- [ ] T014 [US1] Confirm T011–T013 compile but fail
- [ ] T015 [US1] Add `nopws`, `web_available`, `nopws_detected`, `nopws_evidence`, `suggestion` fields to `iris_test_server` response
- [ ] T016 [US1] When `nopws = true` and web port unreachable: suppress raw connection error; return structured message with NoPWS explanation and `nopws: true`
- [ ] T017 [US1] When `nopws = true` and web port IS reachable (webgateway sidecar): connect normally via Atelier REST — `nopws` flag only suppresses the error, not the connection
- [ ] T018 [US1] Confirm T011–T013 pass: `cargo build && cargo test -- --include-ignored --test-threads=1 test_iris_test_server_nopws 2>&1`
- [ ] T_BINARY_TEST_SERVER [US1] `#[ignore]` Binary invocation — spawn `IAD_BINARY`, call `iris_test_server` via stdio JSON-RPC, assert response contains `nopws_detected` field (Layer 2 binary test per CLAUDE.md §Test Coverage Policy)
- [ ] T019 [US1] Run `cargo clippy -- -D warnings`

---

## Phase 4: User Story 2 — `iris_execute` docker exec fallback + `execution_path` (P1)

**Story**: `iris_execute` against local NoPWS container falls back to docker exec; response has `execution_path: "docker_exec_local"`.

**Phase gate**: T029 (live IRIS with closed web port) must pass before Phase 5.

- [ ] T020 [US2] Read `mod.rs:~3245–3301` (`iris_compile` docker exec pattern) and `mod.rs:~4307` (`iris_execute` docker exec branch location)
- [ ] T021 [US2] Read `connection.rs:685` (`IrisConnection::execute()`) to understand docker exec implementation
- [ ] T022 [US2] Write unit test: `iris_execute` with `docker_only = true`; assert response has `execution_path: "docker_exec_local"` field (even if IRIS call fails)
- [ ] T023 [US2] Write binary test (`#[ignore]`, `IAD_BINARY`): call `iris_execute "Write 1"` with `IRIS_CONTAINER` set and web port closed; assert response has `execution_path` field (docker exec path taken)
- [ ] T024 [US2] Write live IRIS test (`#[ignore]`, `--test-threads=1`): call `iris_execute "Write 1"` via HTTP path; assert `execution_path: "atelier"` in response (no behavior change on Atelier path)
- [ ] T025 [US2] Confirm T022–T024 compile but fail
- [ ] T026 [US2] In `mod.rs` `iris_execute` handler: add early-branch mirroring `iris_compile`: when `docker_only || no_pws`, skip Atelier REST, route to `iris.execute()`, set `execution_path = "docker_exec_local"` (or `"docker_exec_ssh"` when `ssh_host` set). **Note**: any spawned subprocess in an async context must use `tokio::process::Command`, not `std::process::Command` — the blocking variant will stall the tokio runtime. Verify the existing `execute()` in `connection.rs` uses `tokio::process::Command` (or wraps with `spawn_blocking`) before calling it from the async handler.
- [ ] T027 [US2] Add `execution_path: "atelier"` field to HTTP success path response
- [ ] T028 [US2] Add error handling: if docker exec fallback needed but no container configured,
      return structured error:
      `{ "success": false, "error_code": "NOPWS_NO_CONTAINER", "error": "NoPWS mode requires a
Docker container. Set IRIS_CONTAINER to the container name." }`.
      In the `docker_only || no_pws` early-branch in `iris_execute`: if `iris.execute()` returns
      `Err` containing "DOCKER_REQUIRED" (IRIS_CONTAINER not set), catch it and re-surface as
      `NOPWS_NO_CONTAINER` with the message above.
- [ ] T029 [US2] Write live IRIS test: set `IRIS_WEB_PORT` to closed port and `IRIS_CONTAINER=iris-dev-iris`, call `iris_execute "Write 1"`, assert `execution_path: "docker_exec_local"` and valid result
- [ ] T029b [US2][FR-016] Write unit test: call `iris_compile` with `docker_only = true`;
      assert response contains `execution_path: "docker_exec_local"` (or `"docker_exec_ssh"` when
      `ssh_host` set). Tests MUST FAIL before T029c. `iris_compile` already has `method:
"docker_exec"` — `execution_path` is the finer-grained addition.
- [ ] T029c [US2][FR-016] Add `execution_path: "docker_exec_local"` (or `"docker_exec_ssh"`)
      to the docker exec branch of `iris_compile` in `mod.rs:~3245–3301` for parity with
      `iris_execute`. Keep the existing `method: "docker_exec"` field for backward compat.
- [ ] T030 [US2] Confirm T022–T029c pass: `cargo test -- --include-ignored --test-threads=1 test_iris_execute_nopws 2>&1`
- [ ] T031 [US2] Run `cargo clippy -- -D warnings`

---

## Phase 5: User Story 4 — NoPWS auto-detection in `iris_test_server` (P2)

**Story**: `iris_test_server` detects NoPWS by reading iris.cpf via docker exec when web probe fails; returns `nopws_detected: true` with evidence.

- [ ] T032 [US4] Write unit tests in `test_nopws_detection.rs` (new file, separate from
      `test_nopws_execute.rs`): mock `docker exec ... grep WebServer iris.cpf` output containing
      `WebServer=0`; assert `nopws_detected: true`, `nopws_evidence` contains the matched line
- [ ] T033 [US4] Write unit test: docker exec returns `WebServer=1`; assert `nopws_detected: false`
- [ ] T034 [US4] Write unit test: docker exec fails (container not found); assert `nopws_detected: false` and `unreachable: true` (no false positive)
- [ ] T035 [US4] Confirm T032–T034 fail
- [ ] T036 [US4] Implement iris.cpf auto-detection in `iris_test_server`: when web probe fails and `container` is configured, try `docker exec <container> grep WebServer /usr/irissys/iris.cpf`, then `/usr/local/etc/irissys/iris.cpf` — first hit wins; parse `WebServer=0` → `nopws_detected: true` with evidence and ready-to-paste toml snippet
- [ ] T037 [US4] Handle detection failure gracefully: docker unavailable, permission denied, unexpected output → `nopws_detected: false`, no false positive
- [ ] T038 [US4] Confirm T032–T034 pass
- [ ] T039 [US4] Run `cargo clippy -- -D warnings`

---

## Phase 6: User Story 3 — SSH path for remote containers (P2)

**Story**: `ssh_host` set in config; docker exec routes via `ssh <ssh_host> docker exec ...`.

- [ ] T040 [US3] Write unit test: `WorkspaceConfig { ssh_host: Some("test-host"), container: Some("iris"), ... }`; assert constructed command string starts with `ssh -o StrictHostKeyChecking=no test-host docker exec`
- [ ] T041 [US3] Write unit test: `ssh_host` set but `container` not set → error "ssh_host requires container to be set"
- [ ] T042 [US3] Confirm T040–T041 fail
- [ ] T043 [US3] Add `ssh_host: Option<String>` to `IrisConnection`; populate from `WorkspaceConfig.ssh_host` in `workspace_config_to_connection()`
- [ ] T044 [US3] In `connection.rs` docker exec command builder: when `self.ssh_host.is_some()`,
      prefix command with `ssh -o StrictHostKeyChecking=no <ssh_host>`.
      **Line-ending convention**: use the same convention as existing `execute()` in
      `connection.rs:702` — `\r\n` between lines and `Halt\r\n` as the terminator (NOT
      `\nhalt\n`). FR-007 specifies `\r\n`; the SSH path must match.
      **Security note for docs**: `StrictHostKeyChecking=no` bypasses SSH host key verification
      to enable non-interactive use. Document in `docs/connecting.md`: "Setting `ssh_host`
      bypasses SSH host key verification (`StrictHostKeyChecking=no`) to enable non-interactive
      use. Ensure you trust the remote host before setting this option."
- [ ] T045 [US3] Set `execution_path = "docker_exec_ssh"` when SSH path is used
- [ ] T046 [US3] Confirm T040–T041 pass
- [ ] T047 [US3] Run `cargo clippy -- -D warnings`

---

## Phase 7: User Story 5 — NoPWS setup skill (P1)

**Story**: Agent reads skill; can detect NoPWS, set up webgateway sidecar, and clear first-boot password without asking for help.

- [ ] T048 [US5] Write `skills/skills/iris-agentic-dev/nopws-setup/SKILL.md` (<300 lines) covering: (1) NoPWS detection commands, (2) plain-language explanation of NoPWS + affected iad tools, (3) Option A (webgateway sidecar: pull `containers.intersystems.com/intersystems/webgateway`, network bridge, CSP.conf snippet, verify Atelier API), (4) Option B (`docker_only = true` + `nopws = true` config), (5) first-boot password clearing (`Do $System.Security.ChangePassword("_SYSTEM","SYS","SYS")`), (6) error recognition table
- [ ] T049 [US5] Verify skill keywords in description/frontmatter: "NoPWS", "No Private Web Server", "AI branch", "connection refused", "webgateway sidecar", "irishealth-ai" — all present (FR-015)
- [ ] T050 [US5] Human review: read skill end-to-end; confirm steps are complete and actionable; mark as reviewed
- [ ] T051 [US5] Run `markdownlint-cli2 --fix "skills/skills/iris-agentic-dev/nopws-setup/SKILL.md" && prettier --write "skills/skills/iris-agentic-dev/nopws-setup/SKILL.md"`

---

## Phase 8: FR-010 — Atelier-required tools error

**Purpose**: Tools that cannot fall back to docker exec must return a clear NoPWS error (not silent failure).

- [ ] T_UNIT_NOPWS_ATELIER Unit test — call `iris_execute` with `nopws=true` and Atelier-only code path active (not `docker_only=true`); assert response contains `NOPWS_ATELIER_REQUIRED` error code and no HTTP connection is attempted. Covers the case where NoPWS is declared but no docker exec route is available.
- [ ] T052 Write unit tests in `test_nopws_detection.rs` (detection-path tests): call
      `iris_doc(mode="put")`, `iris_source_control`, and `iris_doc_search` each against a
      `nopws=true` config with no webgateway; assert each response contains
      `"error_code": "NOPWS_ATELIER_REQUIRED"` and message "NoPWS: this tool requires Atelier
      REST API". Tests MUST FAIL before T053.
- [ ] T053 Implement NoPWS guard in Atelier-required tools (`iris_doc` put/get,
      `iris_source_control`, `iris_doc_search`): when `docker_only || nopws = true` and web
      port unreachable, return structured error `{ "success": false, "error_code":
"NOPWS_ATELIER_REQUIRED", "error": "NoPWS: this tool requires Atelier REST API. Set up a
webgateway sidecar or use docker_only = true for supported tools." }`. `iris_doc_search`
      has no docker exec fallback and must be included here (FR-010).
- [ ] T054 Confirm T052 passes
- [ ] T055 Run `cargo clippy -- -D warnings`

---

## T_LIFT: Tool Lift Measurement (Constitution IX gate)

Run before Polish. Both `iris_execute` and `iris_test_server` are agent-facing MCP tools gaining new response fields — lift ≥ +0.20 required before merge.

- [ ] T_LIFT Run GEPA eval harness: A/B baseline vs. new tool descriptions for `iris_execute` (with `execution_path` field) and `iris_test_server` (with `nopws_detected`, `suggestion`, `unreachable` fields); record raw scores and delta in `specs/101-nopws-connectivity/lift-results.md`. Merge is blocked if lift < +0.20 on either tool.

---

## Phase 9: Polish & Coverage

- [ ] T056 Run full test suite: `cargo test 2>&1 | tail -20`
- [ ] T057 Run integration/e2e: `cargo test -- --include-ignored --test-threads=1 2>&1 | tail -20`
- [ ] T058 Run coverage: `cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored 2>&1 | grep TOTAL` — assert ≥ 90.00%
- [ ] T059 Run `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` — both clean
- [ ] T060 Confirm T_LIFT gate passed (lift-results.md present and both tool deltas ≥ +0.20)
- [ ] T061 Run `markdownlint-cli2 --fix "skills/skills/iris-agentic-dev/nopws-setup/SKILL.md" && prettier --write "skills/skills/iris-agentic-dev/nopws-setup/SKILL.md"` (final pass)

---

## Dependencies

```
T001–T003 → T004–T009 (baseline → WorkspaceConfig fields)
T004–T009 → T010–T019 (nopws field → iris_test_server NoPWS messages)
T004–T009 → T020–T031 (nopws field → iris_execute docker exec fallback + iris_compile execution_path parity FR-016)
T020–T031 → T032–T039 (docker exec path → auto-detection uses it)
T004–T009 → T040–T047 (ssh_host field → SSH path)
T048–T051 (skill — independent, can start after T001)
T052–T055 (FR-010 guard — independent after T009)
T010–T055 → T056–T061 (all impl → polish)
```

## Parallel Opportunities

- T010–T019 (US1) and T020–T031 (US2) — parallel after T009
- T048–T051 (skill) — fully independent, can start any time
- T040–T047 (SSH path, US3) — parallel with US1+US2 after T009

## MVP Scope

T001–T031 + T048–T051: NoPWS flag, `iris_execute` fallback, `execution_path` field, and skill. This is the minimum for an operator to use a local NoPWS container with iad. Auto-detection (US4), SSH (US3), and FR-010 guard can follow.
