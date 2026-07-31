# Tasks: Multi-Instance Connection Pool (072)

**Input**: `specs/072-multi-instance-pool/plan.md`, `spec.md`

Three phases. 072-b and 072-c are independent of each other; both require 072-a.

---

## Phase 072-a: Foundation — Connection Pool + Server Registry

### Setup

- [x] T001 Create `crates/iris-agentic-dev-core/src/iris/connection_pool.rs` (stub module, add to `iris/mod.rs`)
- [x] T002 Create `crates/iris-agentic-dev-core/src/iris/servers_config.rs` (stub module)
- [x] T003 Create `crates/iris-agentic-dev-core/src/tools/server_tools.rs` (stub module, add to `tools/mod.rs`)

---

### Phase 072-a / Step 1: `servers_config.rs` — iad-native config file

**Blocks**: T012 (cascade loader)

#### Tests first

- [x] T004 [P] Write unit tests in `servers_config.rs`:
  - `load_native_config()` on missing file returns `ServersConfig::default()` (empty, no error)
  - `save_native_config` + `load_native_config` round-trips a config with two servers
  - `native_config_path()` returns a path ending in `iris-agentic-dev/servers.json`
  - Version field `1` is preserved on round-trip

#### Implementation

- [x] T005 Define `ServerEntry` and `ServersConfig` structs in `servers_config.rs` (serde Deserialize/Serialize)
- [x] T006 Implement `native_config_path()` — platform-correct: `~/.config/iris-agentic-dev/servers.json` (macOS/Linux), `%APPDATA%\iris-agentic-dev\servers.json` (Windows)
- [x] T007 Implement `load_native_config()` — returns `ServersConfig::default()` if file missing; returns parse error if file is malformed JSON
- [x] T008 Implement `save_native_config()` — atomic write (temp file + rename), creates parent dir if absent

**Checkpoint**: `cargo test -p iris-agentic-dev-core servers_config` — all tests green

---

### Phase 072-a / Step 2: `store_credential` in `server_manager.rs`

**Blocks**: T031 (`iris_add_server`)

#### Tests first

- [x] T009 [P] Write unit test in `server_manager.rs`: `store_credential` + `resolve_credential` round-trip (requires `init_platform_keystore()` called first). Mark `#[ignore]` (requires OS keychain)

#### Implementation

- [x] T010 Implement `store_credential(server_name, username, password)` in `server_manager.rs` — `keyring::Entry::new(SM_KEYCHAIN_SERVICE, &account).set_password(pw)`

---

### Phase 072-a / Step 3: `connection_pool.rs` — pool struct and cascade loader

**Blocks**: T016 (IrisTools integration)

#### Tests first

- [x] T011 [P] Write unit tests in `connection_pool.rs`:
  - `ConnectionPool::get(None)` on empty pool returns `IRIS_UNREACHABLE` error
  - `ConnectionPool::get(Some("x"))` on pool without `"x"` returns `SERVER_NOT_FOUND` error
  - `ConnectionPool::get(Some("x"))` on pool with `"x"` returns the correct `Arc<IrisConnection>`
  - `ConnectionPool::get(None)` with a default set returns the default connection
  - `ConnectionPool::len()` returns correct count
  - Cascade: iad-native entry wins over a same-named entry from VS Code source (unit test with mock data, no file I/O)

#### Implementation

- [x] T012 Implement `ConnectionPool` struct and `get()`, `names()`, `default_name()`, `len()` methods
- [x] T013 Implement `load_pool(config_file: Option<&Path>) -> ConnectionPool`:
  - Source 1: `load_native_config()` → build `IrisConnection` per server entry
  - Source 2: `parse_sm_settings()` for VS Code / Cursor paths (all platform paths)
  - Source 3: `[instance.*]` blocks from workspace toml (existing `load_fleet_config`)
  - Source 4: env vars `IRIS_HOST` / `IRIS_WEB_PORT` → name `"_env"`
  - Name dedup: first source wins for a given name
  - Default: `servers.json` `"default"` field; else `"_env"` if env-var source present; else first entry
- [x] T014 Add `SERVER_NOT_FOUND` error string constant alongside existing error strings in `mod.rs`

**Checkpoint**: `cargo test -p iris-agentic-dev-core connection_pool` — all unit tests green

---

### Phase 072-a / Step 4: Wire pool into `IrisTools`

#### Tests first

- [x] T015 [P] Write unit test: `IrisTools` constructed with a two-server pool; `pool.get(Some("b"))` returns the `"b"` connection, not the default

#### Implementation

- [x] T016 Add `pool: ConnectionPool` field to `IrisTools` struct in `mod.rs`
- [x] T017 Update `IrisTools::from_env()` / `IrisTools::new()` to call `load_pool()` and populate `pool` field. `connection` field set to pool's default (preserves existing single-connection behavior)
- [x] T018 Update `IrisTools::with_connection()` (test constructor) to accept a pool or build a trivial one-entry pool from the provided connection

---

### Phase 072-a / Step 5: Add `server` param to all execution tools

#### Tests first

- [x] T019 [P] Add integration test `e2e_server_param_default` (`#[ignore]`): call `iris_execute` with `server: null` — behaves identically to existing behavior
- [x] T020 [P] Add integration test `e2e_server_param_named` (`#[ignore]`): call `iris_execute` with `server: "iris-dev-iris"` (the dev container, also registered by name) — routes correctly, returns output

#### Implementation

- [x] T021 Add `server: Option<String>` to `ExecuteParams` (serde default `None`)
- [x] T022 In `iris_execute` handler: `let iris = self.resolve_server(server.as_deref()).await?;` where `resolve_server` is:

  ```rust
  async fn resolve_server(&self, name: Option<&str>) -> Result<Arc<IrisConnection>, McpError> {
      match name {
          None => self.get_iris_reloaded().await,
          Some(n) => self.pool.get(Some(n)),
      }
  }
  ```

- [x] T023 Add `server: Option<String>` and route via `resolve_server` to: `iris_query`, `iris_compile`, `iris_test`, `iris_source_control`, `iris_global`, `iris_search`, `iris_symbols`, `iris_symbols_local`
- [x] T024 Add `server: Option<String>` and route via `resolve_server` to: `iris_execute_method`, `iris_generate`, `iris_generate_class`, `iris_generate_test`, `iris_debug`, `iris_coverage`, `iris_get_log`, `iris_admin`, `iris_table_info`
- [x] T025 Add `server: Option<String>` and route via `resolve_server` to: `iris_macro`, `iris_message_body`, `iris_interop_query`, `iris_production`, `iris_production_item`, `iris_production_diff`, `iris_business_rule_info`
- [x] T026 Add `server: Option<String>` and route via `resolve_server` to: `docs_introspect`, `iris_doc`, `iris_doc_search`, `find_subclass_implementations`, `extract_message_map_routing`, `resolve_dynamic_dispatch`
- [x] T027 Update all affected tool descriptions to document the `server` param (one-line addition: see plan.md)

**Checkpoint**: `cargo test -p iris-agentic-dev-core -- --include-ignored --test-threads=1` green

---

### Phase 072-a / Step 6: Server management tools

#### Tests first

- [x] T028 [P] Write unit tests in `server_tools.rs`:
  - `iris_servers` with an empty pool returns an empty list (not an error)
  - `iris_servers` output includes `source` field per entry
  - `iris_remove_server` on a server sourced from VS Code returns `REMOVE_NOT_ALLOWED` error (unit test, mock pool)
- [x] T029 [P] Add integration test `e2e_server_add_remove` (`#[ignore]`): `iris_add_server` with a test name, `iris_servers` shows it, `iris_remove_server`, `iris_servers` does not show it. Cleans up after itself.
- [x] T030 [P] Add integration test `e2e_server_test` (`#[ignore]`): `iris_test_server` against the dev container returns Atelier version and IRIS version string.

#### Implementation

- [x] T031 Implement `iris_servers` tool: iterate `pool.names()`, return JSON array with `name`, `host`, `port`, `namespace`, `source`, `reachable` (lazy — do not connect, just report what pool knows; mark `reachable: null` if not tested)
- [x] T032 Implement `iris_add_server` tool: validate params, call `save_native_config` (merge new entry), call `store_credential`. Return `{"added": true, "name": "..."}`.
- [x] T033 Implement `iris_remove_server` tool: check source is `"iad-native"` (error if not), remove from config, remove from keychain via `keyring::Entry::delete_credential()`. Note: pool does not hot-reload — advise restart.
- [x] T034 Implement `iris_test_server` tool: build a one-shot `IrisConnection` for the named server (or pool entry), call `GET /api/atelier/`, return version info + latency.
- [x] T035 Implement `iris_import_servers` tool: read VS Code / Cursor `settings.json` via `parse_sm_settings()`, for each server not already in iad-native config: add to native config (no password in file), read password from existing keychain entry if present. Report `{"imported": N, "skipped": N, "no_keychain": [...]}`.
- [x] T036 Register all five tools in `tool_router` in `mod.rs`

**Checkpoint**: `cargo test -p iris-agentic-dev-core server_tools -- --include-ignored --test-threads=1` green

---

### Phase 072-a / Polish

- [x] T037 `cargo fmt --all -- --check` — zero diff
- [x] T038 `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings
- [x] T039 [P] Update `docs/tools.md`: add server management tools section; add `server` param note to `iris_execute` and `iris_query` entries. Update `docs/connecting.md` with `IRIS_WS_TIMEOUT_SECS` env var.
- [x] T040 Write "What's new" release notes entry for 072-a in `specs/072-multi-instance-pool/release-notes-a.md`
- [x] T041 Run `/no-ai-slop` on release notes — address all flagged items
- [x] T041b [P] **Coverage gate** (Constitution VIII): run `cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored --test-threads=1`. Verify total line coverage ≥ 90%, or document gap with same format as T028 in 071 tasks.md.

**Phase 072-a complete gate**: T037–T041b all done, all tests green.

---

## Phase 072-b: WebSocket Sessions

**Requires**: Phase 072-a complete.

### Setup

- [x] T042 Add `tokio-tungstenite = { version = "0.26", features = ["native-tls"] }` to `crates/iris-agentic-dev-core/Cargo.toml`
- [x] T043 Add `V7` variant to `AtelierVersion` enum in `connection.rs`. Update `version_str()` match arm (`V7 => "v7"`). Update detection: `Some(v) if v >= 8 => V8`, `Some(v) if v >= 7 => V7`. Update all existing match arms that need a `V7` arm.
- [x] T044 Create `crates/iris-agentic-dev-core/src/iris/ws_session.rs` (stub)
- [x] T045 Create `crates/iris-agentic-dev-core/src/tools/ws_tools.rs` (stub)

---

### Phase 072-b / Step 1: WS auth and session management

#### Tests first

- [x] T046 [P] Write unit tests in `ws_session.rs`:
  - `WsSessionPool::parse_token("ws:dev:USER:abc-123")` returns `Some(("dev", "USER", "abc-123"))`
  - `WsSessionPool::parse_token("bad-token")` returns `None`
  - `WsSessionPool::parse_token("ws:dev:USER:")` returns `None` (empty uuid)
  - Token format roundtrip: generate token, parse it, components match

- [x] T047 [P] Add integration test `e2e_ws_open_close` (`#[ignore]`): open WS session against dev container, verify token format, close. Checks IRIS version gate — skip if `atelier_version < V7`.
- [x] T048 [P] Add integration test `e2e_ws_exec_persistent` (`#[ignore]`): open session, `Set x = 42` in first exec, `Write x` in second exec — assert output is `42`. This is the core US4 test.
- [x] T049 [P] Add integration test `e2e_ws_stale_token` (`#[ignore]`): construct a token referencing a server name not in pool — assert `SESSION_STALE` error.

#### Implementation

- [x] T050 Implement `WsSession` struct with `write` / `read` split streams and `server_name`, `namespace`, `uuid` fields
- [x] T051 Implement `get_csp_session_cookie(conn: &IrisConnection) -> Result<String>` — GET `/api/atelier/` with Basic auth, extract `CSPSESSIONID` from Set-Cookie header
- [x] T052 Implement `WsSessionPool::open(conn, server_name, namespace)` — auth flow, WS connect, send `init`/`config` handshake, return token string
- [x] T053 Implement `WsSessionPool::exec(pool_ref, token, code)` — parse token, look up session, send `prompt` message, collect `output` frames until next `prompt`, return concatenated output. `SESSION_STALE` if server not in connection pool.
- [x] T054 Implement `WsSessionPool::close(pool_ref, token)` — send `interrupt`, drop WS connection, remove from pool map
- [x] T055 Add `ws_pool: Arc<WsSessionPool>` field to `IrisTools` struct

---

### Phase 072-b / Step 2: WS tool handlers

#### Tests first

- [x] T056 [P] Write unit test: `iris_ws_open` handler with `atelier_version = V1` returns `SESSION_WS_UNAVAILABLE` error (unit test, no network)

#### Implementation

- [x] T057 Implement `iris_ws_open` handler: version gate, call `ws_pool.open()`, return `{"session": "<token>", "server": "...", "namespace": "..."}`
- [x] T058 Implement `iris_ws_exec` handler: call `ws_pool.exec()`, return `{"output": "...", "session": "<token>"}`
- [x] T059 Implement `iris_ws_close` handler: call `ws_pool.close()`, return `{"closed": true}`
- [x] T060 Register `iris_ws_open`, `iris_ws_exec`, `iris_ws_close` in tool router

---

### Phase 072-b / Lift Measurement (required before Polish — Constitution IX)

- [X] T061a Write benchmark task file `benchmark/021/tasks/MUL-03.yaml` (content from plan.md §Benchmark Tasks). Add `MUL` to `VALID_CATEGORIES` in `benchmark/021/runner/task_loader.py` if not already done.
- [x] T061b Run lift measurement with `MUL-03` task against baseline and tool-assisted; record results in `specs/072-multi-instance-pool/lift-results.md`. Target lift ≥ +0.20. If below threshold, iterate on `iris_ws_open`/`iris_ws_exec` tool descriptions before continuing.

### Phase 072-b / Polish

- [x] T061 `cargo fmt --all -- --check` — zero diff
- [x] T062 `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings
- [x] T063 [P] Update `docs/tools.md`: add WebSocket session tools section; note fallback to `use_session` for pre-v7 IRIS
- [x] T064 Write "What's new" release notes entry in `specs/072-multi-instance-pool/release-notes-b.md`
- [x] T065 Run `/no-ai-slop` on release notes
- [x] T065b [P] **Coverage gate** (Constitution VIII): run `cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored --test-threads=1`. Verify total line coverage ≥ 90%, or document gap.
  - Result (2026-07-31): **60.46% line** — gap is 29.54 pp below 90% gate.
  - Root cause (pre-existing, not introduced by 072): `test_e2e.rs` and other tests exercise tools via a spawned `iris-agentic-dev` subprocess over MCP. The subprocess generates its own profraw data that `cargo llvm-cov` never merges — so all tool-dispatch paths exercised by the subprocess are invisible to coverage. This was 89% in v0.9.5, dropped to 64% in v0.9.7 when `execute_via_generator` moved more paths behind the subprocess boundary. Fix requires a `scripts/coverage.sh` that launches the instrumented binary via MCP and merges the subprocess profraw files. Until that script exists the 90% gate cannot be met structurally.

**Phase 072-b complete gate**: T061a–T065b done, e2e WS tests green.

---

## Phase 072-c: Comparison Tools and Pierre Ports

**Requires**: Phase 072-a complete. Independent of 072-b.

### Setup

- [x] T066 Check `cargo tree -p iris-agentic-dev-core | grep similar` — if absent, add `similar = "2"` to `crates/iris-agentic-dev-core/Cargo.toml`
- [x] T067 Create `crates/iris-agentic-dev-core/src/tools/comparison_tools.rs` (stub)
- [x] T068 Create `crates/iris-agentic-dev-core/src/tools/admin_tools.rs` (stub — namespace/db/security/HL7/Mermaid/observability)

---

### Phase 072-c / Step 1: Cross-instance comparison

#### Tests first

- [x] T069 [P] Write unit test: `unified_diff("a\nb\nc\n", "a\nX\nc\n")` produces a diff containing `"-b"` and `"+X"` (pure logic, no IRIS)
- [x] T070 [P] Add integration test `e2e_compare_document_same` (`#[ignore]`): compare a class against itself on the dev container — assert diff is empty
- [x] T071 [P] Add integration test `e2e_compare_document_diff` (`#[ignore]`): if two server entries point to different namespaces with known different content, assert diff is non-empty (or use two entries pointing to same server — force a pre-known diff by temporarily modifying a class in one namespace)

#### Implementation

- [x] T072 Implement `compare_document(document, server_a, server_b, namespace?)` — fetch source from both servers via Atelier GET, run `similar::TextDiff`, return `{"diff": "...", "same": bool, "document": "..."}`
- [x] T073 Implement `compare_namespace(namespace, server_a, server_b)` — class list from both, compute `only_in_a`, `only_in_b`, `different` (by comparing source text or size). Return counts and lists.
- [x] T074 Register `compare_document`, `compare_namespace` in tool router

---

### Phase 072-c / Step 2: Global confirmation pattern

#### Tests first

- [x] T075 [P] Write unit tests for confirmation token logic:
  - Fresh token is valid
  - Token expired after 5 minutes (mock `Instant`)
  - Token for `(server_a, global_a)` is rejected for `(server_a, global_b)` — `CONFIRM_MISMATCH`
  - `global_kill` without token returns `CONFIRM_REQUIRED`
  - `global_kill` on a production-mode connection returns `IRIS_WRITE_BLOCKED` (write gate test)

- [x] T076 [P] Add integration test `e2e_global_kill_confirm` (`#[ignore]`): `global_preview` on a known-empty global (e.g. `^IrisAgentDevTest`), get token, `global_kill` with token — assert no error. Cleans up after itself.

#### Implementation

- [x] T077 Add `confirm_tokens: Mutex<HashMap<String, ConfirmEntry>>` to `IrisTools` (see plan.md for `ConfirmEntry` shape)
- [x] T078 Implement `global_preview(global, server?, count?)` — runs `iris_global` internally, mints token, stores in `confirm_tokens`, returns entries + token + expiry
- [x] T079 Implement `global_kill(global, server?, confirm_token)` — **write-gated** (Constitution §VI): check `write_tools_enabled` before execution; validate token; execute `Kill ^{global}` via exec path; remove token
- [x] T079b [P] Write unit test: `global_kill` on a connection with `write_tools_enabled = false` returns write-gate error, does not attempt execution
- [x] T080 Register `global_preview`, `global_kill` in tool router (write-gate classification: `global_kill` is write-capable)

---

### Phase 072-c / Step 3: Namespace / database admin

**Pre-condition**: Verify `%SYS.Namespace.ListAll()`, `Config.Databases.List()`, and
`%SYS.Namespace.Create()` against live IRIS before T082–T084 (research.md §ObjectScript
API Verification). Mark rows VERIFIED before starting implementation.

- [x] T081 [P] Add integration test `e2e_namespace_list` (`#[ignore]`): `iris_namespace_list` returns at least `USER` and `%SYS`
- [x] T082 Implement `iris_namespace_list(server?)` — `##class(%SYS.Namespace).ListAll()` via exec
- [x] T083 Implement `iris_database_list(server?)` — `##class(Config.Databases).List()` via exec
- [x] T084 Implement `iris_namespace_create(name, db_path?, server?)` — **write-gated** (Constitution §VI): check `write_tools_enabled` before execution; `##class(%SYS.Namespace).Create()`
- [x] T084b [P] Write unit test: `iris_namespace_create` on a connection with `write_tools_enabled = false` returns write-gate error
- [x] T085 Implement `iris_database_stats(db?, server?)` — resolve "TBD" API in research.md before this task; returns size, free space, journal status
- [x] T086 Register namespace/DB tools in tool router (write-gate classification: `iris_namespace_create` is write-capable)

---

### Phase 072-c / Step 4: Observability tools

- [x] T087 [P] Add integration test `e2e_journal_search` (`#[ignore]`): search journal for entries in last 60 seconds — returns result (may be empty; test that it doesn't error)
- [x] T088 Implement `journal_search(start?, end?, global_pattern?, server?)` — query `SYS.Journal.File`/`SYS.Journal.Record` via exec
- [x] T089 Implement `query_audit_log(user?, event_type?, start?, end?, server?)` — `%SYS.Audit` table query via `iris_query` internally
- [x] T090 Implement `stream_inspect(oid, server?)` — `%Stream.GlobalCharacter.%OpenId(oid)`, read content, return as string or hex for binary
- [x] T091 Register observability tools in tool router

---

### Phase 072-c / Step 5: Security tools

- [x] T092 [P] Add integration test `e2e_my_access` (`#[ignore]`): `my_access` returns current username and at least one role
- [x] T093 Implement `my_access(server?)` — `$username`, role list from `%SYS.Security.Users`
- [x] T094 Implement `capability_matrix(user?, server?)` — cross `(user/role, resource)` from `%SYS.Security` classes
- [x] T095 Register security tools in tool router

---

### Phase 072-c / Step 6: HL7 schema tools

- [x] T096 [P] Add integration test `e2e_hl7_schema_list` (`#[ignore]`): `hl7_schema_list` — if `EnsLib.HL7.Schema` absent, returns `HL7_NOT_AVAILABLE` (not an error crash)
- [x] T097 Implement `hl7_schema_list(server?)` — `##class(EnsLib.HL7.Schema).GetSchemaList()` via exec; catch class-not-found, return `HL7_NOT_AVAILABLE`
- [x] T098 Implement `hl7_schema_inspect(schema, segment?, server?)` — structure query
- [x] T099 Register HL7 tools in tool router

---

### Phase 072-c / Step 7: Mermaid and resolve_storage

- [x] T100 [P] Write unit test: `build_mermaid_class_diagram(["Foo extends Bar", "Foo has Baz"])` generates valid Mermaid `classDiagram` syntax (pure logic test)
- [x] T101 [P] Add integration test `e2e_mermaid_class` (`#[ignore]`): `mermaid_class("%Library.Persistent")` returns a string starting with `classDiagram`
- [x] T102 Implement `mermaid_class(class, depth?, server?)` — walk `%Dictionary.CompiledClass` superclass chain + key associations via `iris_query` internally, build Mermaid string
- [x] T103 Implement `mermaid_production(production, server?)` — walk `Ens.Config.Production` items via `iris_query`, build Mermaid flowchart
- [x] T104 Implement `resolve_storage(class, server?)` — query `%Dictionary.CompiledStorage` for global map + subscript structure
- [x] T105 Register `mermaid_class`, `mermaid_production`, `resolve_storage` in tool router

---

### Phase 072-c / Lift Measurement (required before Polish — Constitution IX)

- [X] T109 Write benchmark task files for `MUL-01` and `MUL-02` in `benchmark/021/tasks/` (YAML content from plan.md §Benchmark Tasks). Add `MUL` to `VALID_CATEGORIES` in `task_loader.py` if not already done by T061a.
- [x] T110 Run lift measurement with `MUL-01` and `MUL-02` tasks against baseline and tool-assisted; record results in `specs/072-multi-instance-pool/lift-results.md` (append to any prior entries from 072-b). Target lift ≥ +0.20 on at least one task. If below threshold, iterate on tool descriptions before continuing.

### Phase 072-c / Polish

- [x] T106 `cargo fmt --all -- --check` — zero diff
- [x] T107 `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings
- [x] T108 [P] Update `docs/tools.md`: add all new tool sections
- [x] T111 Write "What's new" release notes entry in `specs/072-multi-instance-pool/release-notes-c.md`
- [x] T112 Run `/no-ai-slop` on release notes
- [x] T112b [P] **Coverage gate** (Constitution VIII): run `cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored --test-threads=1`. Verify total line coverage ≥ 90%, or document gap.
  - Result (2026-07-31): **60.46% line** — same run as T065b. Root cause: pre-existing subprocess profraw gap (see T065b).

**Phase 072-c complete gate**: T106–T112b done, all tests green, lift results recorded.

---

## Dependencies & Execution Order

```text
T001–T003 (setup)
  └─► T004–T008 (servers_config)
        └─► T009–T010 (store_credential)
  └─► T011–T014 (connection_pool)
        └─► T015–T018 (wire pool into IrisTools)
              └─► T019–T027 (server param on exec tools)
              └─► T028–T036 (server management tools)
  └─► T037–T041b (072-a polish + coverage gate) ─── 072-a COMPLETE

072-a COMPLETE
  ├─► T042–T060 (072-b implementation)
  │     └─► T061a–T061b (072-b lift measurement — before polish)
  │           └─► T061–T065b (072-b polish + coverage gate) ─── 072-b COMPLETE
  └─► T066–T105 (072-c implementation)
        └─► T109–T110 (072-c lift measurement — before polish)
              └─► T106–T112b (072-c polish + coverage gate) ─── 072-c COMPLETE
```

Note: T109 (write YAML files + add MUL category) MUST complete before T110 (run
measurement). T061a performs the same for 072-b.

### Within each phase: tests first, then implementation

Total tasks: 120 (112 original + T041b, T061a, T061b, T065b, T079b, T084b, T112b, T025 moved to T109/T110)
Test tasks (marked [P]): T004, T009, T011, T015, T019, T020, T028, T029, T030, T039, T041b,
T046, T047, T048, T049, T056, T061b, T065b, T069, T070, T071, T075, T076, T079b, T081,
T084b, T087, T092, T096, T100, T101, T112b (32 test/gate tasks)
