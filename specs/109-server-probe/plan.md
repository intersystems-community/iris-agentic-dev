# Implementation Plan: 098-server-probe

**Branch**: `098-server-probe`
**Spec**: `specs/098-server-probe/spec.md`
**Status**: Ready for tasks

---

## Constitution Check

| Principle                     | Status    | Notes                                                                                                                                                                  |
| ----------------------------- | --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary        | PASS      | No new dependencies; `futures` crate already present or `join_all` via tokio                                                                                           |
| II. ObjectScript Sanity Gate  | N/A       | No new ObjectScript APIs — pure HTTP probe (`GET /api/atelier/`) only                                                                                                  |
| III. HTTP-First Execution     | PASS      | Probe uses Atelier REST; no docker exec in probe path                                                                                                                  |
| IV. (if applicable)           | N/A       | —                                                                                                                                                                      |
| VIII. 90% Coverage Gate       | MUST MEET | Three test layers required; unit + binary + live IRIS                                                                                                                  |
| IX. Tool Lift Requirement     | N/A       | Modifications to existing tools (`iris_test_server` and `iris_servers`), not new tool creation. Constitution IX applies to "every new MCP tool"; no new tool is added. |
| X. ObjectScript Coverage Gate | N/A       | No ObjectScript in this feature                                                                                                                                        |

---

## Technical Context

**Files to change**:

- `crates/iris-agentic-dev-core/src/tools/server_tools.rs` — `TestServerParams`, new `IrisServersParams`, new `ProbeResult`, shared `probe_server()` function
- `crates/iris-agentic-dev-core/src/tools/mod.rs` — `iris_test_server` handler (lines ~7621-7843), `iris_servers` handler (lines ~7454-7458, dispatch ~10463-10484)
- `crates/iris-agentic-dev-core/src/iris/connection.rs` — optionally add `DiscoverySource::AdHoc`

**Key facts verified**:

- `TestServerParams` at `server_tools.rs:45` — currently `name: String` only
- `IrisConnection::new(base_url, namespace, username, password, source)` at `connection.rs:272`
- `IrisConnection::probe()` at `connection.rs:356` — mutates self, sets version/atelier_version
- `IrisConnection::probe_client()` at `connection.rs:813` — short-timeout client for probing
- `iris_servers` handler at `mod.rs:7458` — no params today; dispatch at `mod.rs:10463`
- `iris_test_server` dispatch at `mod.rs:10483-10485`

---

## Implementation Phases

### Phase 1: Shared probe_server() + TestServerParams ad-hoc (FR-001–FR-005)

**TDD order** (tests first):

1. Unit test: `TestServerParams` with all optional fields deserializes correctly (`name: None`, `host: Some("...")`)
2. Unit test: neither `name` nor `host` → validated error path
3. Live IRIS test (`#[ignore]`): ad-hoc probe against `localhost:52780` returns `reachable: true`

**Implementation**:

1. Change `TestServerParams.name` to `Option<String>`, add `host`, `web_port`, `username`, `password` fields
2. Add `ProbeResult` struct to `server_tools.rs`
3. Add `probe_server(host: &str, web_port: u16, namespace: &str, username: &str, password: &str) -> ProbeResult` using `IrisConnection::new` + `.probe()`
4. Refactor `iris_test_server` handler to:
   - Extract probe logic from named-server path into `probe_server()` call
   - Add ad-hoc branch: when `host` present, call `probe_server()` directly
   - Handle neither-present case

**Gate**: Live IRIS test passing before Phase 2.

---

### Phase 2: iris_servers probe=true (FR-006–FR-009)

**TDD order**:

1. Unit test: `IrisServersParams { probe: None }` and `probe: Some(true)` deserialize correctly
2. Binary invocation test (`#[ignore]`, `IAD_BINARY`): call `iris_servers` with no params → `reachable: null` per entry (regression guard)
3. Live IRIS test (`#[ignore]`): `iris_servers(probe=true)` with one server in pool → `reachable: bool`, `latency_ms` present

**Implementation**:

1. Add `IrisServersParams` struct to `server_tools.rs`
2. Change `iris_servers` handler signature to accept `Parameters<server_tools::IrisServersParams>`
3. When `probe=false` (default): identical behavior to today
4. When `probe=true`: fan out `probe_server()` calls via `tokio::time::timeout(5s, ...)` + `futures::future::join_all` (or `tokio::join!` macro for static — use `join_all` for dynamic pool)
5. Update dispatch at `mod.rs:10463`

**Gate**: Binary invocation regression test must pass (no `reachable` field change for default path) before merging Phase 2.

---

### Phase 3: Polish + coverage gate

1. Verify `cargo test -- --include-ignored --test-threads=1` passes all three layers
2. Run `cargo llvm-cov` — confirm ≥ 90% baseline maintained
3. Update tool descriptions in `mod.rs` for both `iris_test_server` and `iris_servers` to document new params
4. Check `docs/tools.md` — update `iris_test_server` and `iris_servers` entries

---

## Test Coverage Plan

| Layer  | Test                                                    | Location             | IRIS needed                |
| ------ | ------------------------------------------------------- | -------------------- | -------------------------- |
| Unit   | `TestServerParams` round-trip (all optional fields)     | `server_tools.rs`    | No                         |
| Unit   | `IrisServersParams` round-trip                          | `server_tools.rs`    | No                         |
| Unit   | Neither `name` nor `host` → error                       | `server_tools.rs`    | No                         |
| Binary | `iris_servers` no params → `reachable: null` unchanged  | `tests/` `#[ignore]` | No                         |
| Binary | `iris_test_server` ad-hoc params → valid response shape | `tests/` `#[ignore]` | No (mock unreachable host) |
| Live   | Ad-hoc probe `localhost:52780` → `reachable: true`      | `tests/` `#[ignore]` | Yes                        |
| Live   | `iris_servers(probe=true)` → `reachable` bool per entry | `tests/` `#[ignore]` | Yes                        |

---

## Risk Notes

- `iris_servers` currently has no params — adding `IrisServersParams` changes handler signature. Must verify dispatch macro in mod.rs accepts the change without breaking the `tools/list` schema for existing callers.
- `futures::future::join_all` — confirm `futures` crate in workspace `Cargo.toml` before coding. If not, use `tokio::task::JoinSet` instead (no new dep).
- `TestServerParams.name` changing from required to optional may break existing callers sending `{ "server": "name" }` via MCP. Verify param name in existing call site — spec says `name` field, tool description says `server` param. Reconcile before changing.
