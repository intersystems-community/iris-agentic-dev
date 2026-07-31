# Feature Specification: Multi-Instance Connection Pool

**Feature Branch**: `072-multi-instance-pool`
**Created**: 2026-07-30
**Status**: Draft
**Replaces**: `iris_select_container` atomic-swap model

## Background

iad currently manages one IRIS connection at a time. `iris_select_container` permanently
swaps the active connection for the entire session. This works for single-instance
development but breaks down in any real deployment:

- **Customer support**: simultaneously inspect production, staging, and a mirror — no
  session-wide swap, no re-authentication between instances.
- **Fleet operations**: query five regional nodes in one conversation without configuration
  edits. (See also `../opsreview` fleet pattern — agent on hub IRIS reaches remotes via
  named registry.)
- **Cross-instance comparison**: diff a class or production config between servers in one
  call.
- **Standalone use**: user has no VS Code at all, wants to add servers and store credentials
  without installing any editor.

Issue #32 (cwennerh) plus Pierre Abdelsayed's pre-release Server Manager MCP extension
(`servermanager-3.13.0-0D13.vsix`) both confirm the demand. Pierre's design has 128 tools
with `server: Optional<String>` on each one, routing through VS Code extension IPC. That
design cannot run standalone. This spec makes iad the server registry — no VS Code
required.

## Goals

1. **Connection pool**: all known servers loaded at startup into a `HashMap<String,
Arc<IrisConnection>>`. Per-call `server` param routes without side effects.
2. **iad-native server registry**: `~/.config/iris-agentic-dev/servers.json` owns the
   source of truth. Reads cascade from VS Code / Cursor settings for zero-migration
   upgrade path. **Same credential format** as VS Code Server Manager — keychain service
   `"intersystems-server-credentials"`, account
   `"credentialProvider:<server-name>/<username-lowercase>"`.
3. **Server management tools**: `iris_servers`, `iris_add_server`, `iris_remove_server`,
   `iris_test_server`, `iris_import_servers` — work entirely from CLI, no editor required.
4. **WebSocket terminal sessions**: persistent IRIS process via
   `/api/atelier/v7/{namespace}/terminal` (IRIS 2023.2+, Atelier API v7+). Full local
   variable and object reference persistence. Session token format
   `ws:<server-name>:<NAMESPACE>:<uuid>`. Fallback to `%ctx` (spec 071) for API < v7.
5. **Port Pierre's unique tools**: namespace/database admin, cross-instance comparison,
   graph/Mermaid visualization, global confirmation pattern (`global_kill`,
   `global_preview`), `resolve_storage`, `stream_inspect`, `journal_search`,
   `query_audit_log`, `my_access`, `capability_matrix`, HL7 schema tools.

## Non-Goals

- ODBC / SQL driver — HTTP-first constraint stays.
- Agent-to-agent fleet coordination (separate opsreview concern).
- VS Code extension protocol compatibility — iad is a standalone binary.

## User Stories

### US1 — Connect to a named server without config edits

> "I have three IRIS instances. I want to run a query on `prod` without touching any
> config file or restarting iad."

**Acceptance**: `iris_execute(server: "prod", code: "Write $ZV")` routes to the `prod`
connection. Active connection for other calls is unchanged.

### US2 — Add a server from the CLI

> "I have no VS Code. I want to register a new IRIS instance and store credentials
> securely."

**Acceptance**: `iris_add_server(name: "dev2", host: "iris-dev2.internal", port: 52780,
namespace: "USER", username: "admin", password: "***")` writes to
`~/.config/iris-agentic-dev/servers.json` and stores password in the OS keychain under
`"intersystems-server-credentials"` / `"credentialProvider:dev2/admin"`. Password does
NOT appear in the config file.

### US3 — Import existing VS Code Server Manager connections

> "I already have ten servers in VS Code. I don't want to retype them."

**Acceptance**: `iris_import_servers` reads `intersystems.servers` from the VS Code
`settings.json`, merges into `~/.config/iris-agentic-dev/servers.json`, and reads
passwords from the existing keychain entries (no prompt, no re-entry).

### US4 — Persistent session via WebSocket terminal

> "I want to open an object, call methods on it across multiple turns, and have it still
> there on turn 5."

**Acceptance**: `iris_ws_open(server: "dev", namespace: "USER")` returns a session token
`ws:dev:USER:<uuid>`. `iris_ws_exec(session: "<token>", code: "Set x = ##class(Foo).%New()
...")` runs in the same IRIS process. Object `x` is alive on the next call. Token encodes
server and namespace; stale-server detection returns `SESSION_STALE`.

### US5 — Cross-instance diff

> "Show me what's different in Ens.MessageHeader between prod and staging."

**Acceptance**: `compare_document(document: "Ens.MessageHeader.cls", server_a: "prod",
server_b: "staging")` returns a unified diff without requiring manual copy-paste.

### US6 — Kill a global with confirmation

> "I want to kill ^TempData on dev, but I want to see what's in it first."

**Acceptance**: `global_preview(global: "TempData", server: "dev")` shows top entries and
mints a 5-minute confirmation token. `global_kill(global: "TempData", server: "dev",
confirm_token: "<token>")` executes. Without the token, `global_kill` refuses.

## Architecture

### Connection Pool

```rust
pub struct ConnectionPool {
    instances: HashMap<String, Arc<IrisConnection>>,
    default: Option<String>,  // name of the default instance
}
```

- Populated at startup from all config sources (see Read Cascade below).
- `get(name: Option<&str>) -> Result<Arc<IrisConnection>>`: `None` → default; `Some(n)`
  → named instance or `SERVER_NOT_FOUND` error.
- Immutable after init — no runtime mutation. `iris_add_server` writes to disk; the pool
  does not hot-reload (restart iad to pick up changes). This keeps the pool free of
  locks in hot paths.

`IrisTools` gains `pool: ConnectionPool` alongside the existing `connection` field.
Execution tools that take a `server` param call `pool.get(server)` instead of
`get_iris_reloaded()`. Tools that do NOT take `server` continue to use the single active
connection (full backward compatibility).

### Read Cascade (startup, highest-priority first)

1. `~/.config/iris-agentic-dev/servers.json` — iad-native (always written here by tools)
2. `~/Library/Application Support/Code/User/settings.json` → `intersystems.servers`
   (VS Code, macOS)
3. `~/.config/Code/User/settings.json` (VS Code, Linux)
4. `%APPDATA%\Code\User\settings.json` (VS Code, Windows)
5. `~/.config/Cursor/User/settings.json` (Cursor)
6. `.iris-agentic-dev.toml` `[instance.*]` blocks (workspace fleet config)
7. `IRIS_HOST` / `IRIS_WEB_PORT` env vars (legacy single-instance)

Name collision: iad-native wins. Within the same source, first wins. A server named
`"_default"` (or the only server from env vars) becomes the default.

### iad-native servers.json Schema

```json
{
  "version": 1,
  "servers": {
    "prod": {
      "host": "iris-prod.internal",
      "port": 52780,
      "namespace": "USER",
      "username": "admin",
      "description": "Production IRIS"
    },
    "dev": {
      "host": "localhost",
      "port": 52780,
      "namespace": "USER",
      "username": "_SYSTEM"
    }
  },
  "default": "dev"
}
```

`intersystems.servers`-compatible field names — passwords are **never** written to this
file. They live in the OS keychain only.

### Credential Storage

- **Service name**: `"intersystems-server-credentials"` (same as VS Code Server Manager)
- **Account format**: `"credentialProvider:<server-name>/<username-lowercase>"`
- **Read**: existing `resolve_credential()` in `server_manager.rs` — unchanged
- **Write**: `keyring::Entry::new(service, account).set_password(pw)` — `keyring` v4
  already supports this. New `store_credential(server_name, username, password)` function
  alongside `resolve_credential`.

Same keychain service name = credentials interoperable with VS Code automatically.

### AtelierVersion V7

Add `V7` to the `AtelierVersion` enum in `connection.rs`:

```text
V8 → v8   (async work queue)
V7 → v7   (WebSocket terminal — /api/atelier/v7/{namespace}/terminal)
V2 → v2
V1 → v1
```

Detection: version integer `>= 7` → `V7`; `>= 8` → `V8` (V8 stays the highest).

### WebSocket Terminal Protocol

Endpoint: `GET /api/atelier/v7/{namespace}/terminal` with `Upgrade: websocket`.

Auth: Basic auth does NOT work on the WS upgrade. Must:

1. `GET /api/atelier/` with Basic auth → extract `CSPSESSIONID` cookie.
2. WS upgrade with `Cookie: CSPSESSIONID=<value>`.

Message sequence:

```json
server → {"type":"init"}
client → {"type":"config","namespace":"USER","rawMode":false}
server → {"type":"prompt","value":"USER>"}
client → {"type":"prompt","input":"Set x = 42  Write x"}
server → {"type":"output","data":"42"}
server → {"type":"prompt","value":"USER>"}
```

Session token: `ws:<server-name>:<NAMESPACE>:<uuid>`. Session pool:
`HashMap<(String,String,String), WsSession>` keyed on `(server_name, namespace, uuid)`.

Stale detection:

- If `server_name` not in connection pool → `SESSION_STALE`
- If WS connection closed → `SESSION_WS_DISCONNECTED` (re-open is automatic on next
  `iris_ws_exec`)

Fallback: if `atelier_version < V7`, `iris_ws_open` returns `SESSION_WS_UNAVAILABLE` and
documents that `use_session`/`%ctx` (spec 071) is the alternative.

## New Tools

### Server Management

| Tool                  | Params                                                      | Description                                                                                                                                                                |
| --------------------- | ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `iris_servers`        | —                                                           | List all known servers from all sources. Shows name, host, port, namespace, source (iad-native / vscode / fleet / env), and reachability.                                  |
| `iris_add_server`     | `name`, `host`, `port`, `namespace`, `username`, `password` | Register a server. Writes to iad-native config + OS keychain. Password never written to disk.                                                                              |
| `iris_remove_server`  | `name`                                                      | Remove from iad-native config. Removes keychain entry. Cannot remove servers sourced from VS Code (edit VS Code settings instead).                                         |
| `iris_test_server`    | `name`                                                      | Attempt connection. Returns Atelier version, IRIS version, namespace, round-trip latency.                                                                                  |
| `iris_import_servers` | `source?`                                                   | Import from VS Code / Cursor settings. Merges into iad-native config. Reads existing keychain passwords (no re-entry). Reports count imported / skipped (already present). |

### WebSocket Terminal

| Tool            | Params                  | Description                                                                                                                                            |
| --------------- | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `iris_ws_open`  | `server?`, `namespace?` | Open a WebSocket terminal session. Returns session token `ws:<server>:<NS>:<uuid>`.                                                                    |
| `iris_ws_exec`  | `session`, `code`       | Execute ObjectScript in an existing WS session. Full persistent IRIS process — all locals, objects survive. Returns output and the same session token. |
| `iris_ws_close` | `session`               | Close WS session and release the IRIS process.                                                                                                         |

### Cross-Instance Comparison

| Tool                | Params                              | Description                                                           |
| ------------------- | ----------------------------------- | --------------------------------------------------------------------- |
| `compare_document`  | `document`, `server_a`, `server_b`  | Unified diff of a class/routine between two servers.                  |
| `compare_namespace` | `namespace`, `server_a`, `server_b` | List classes/routines present in one but not the other, or differing. |

### Global Confirmation Pattern (ported from Pierre)

| Tool             | Params                               | Description                                                                                       |
| ---------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------- |
| `global_preview` | `global`, `server?`, `count?`        | Show top N entries of a global. Mints a 5-minute confirmation token bound to `(server, global)`.  |
| `global_kill`    | `global`, `server?`, `confirm_token` | Kill the global. Requires token from `global_preview`. Without token, returns `CONFIRM_REQUIRED`. |

### Ported from Pierre (missing from iad)

These tools exist in Pierre's SM-MCP with `server: Optional<String>`. Porting to Rust
with the same `server` routing param:

| Tool                    | Category           |
| ----------------------- | ------------------ |
| `iris_namespace_list`   | Namespace/DB admin |
| `iris_namespace_create` | Namespace/DB admin |
| `iris_database_list`    | Namespace/DB admin |
| `iris_database_stats`   | Namespace/DB admin |
| `resolve_storage`       | Schema             |
| `stream_inspect`        | Globals            |
| `journal_search`        | Observability      |
| `query_audit_log`       | Observability      |
| `my_access`             | Security           |
| `capability_matrix`     | Security           |
| `hl7_schema_list`       | HL7                |
| `hl7_schema_inspect`    | HL7                |
| `mermaid_class`         | Graph/Viz          |
| `mermaid_production`    | Graph/Viz          |

Full tool descriptions and parameter schemas defined in `plan.md`.

## Existing Tools — `server` Parameter

Every execution tool gains `server: Option<String>`:

- `iris_execute`, `iris_query`, `iris_compile`, `iris_test`, `iris_source_control`
- `iris_global`, `iris_search`, `iris_symbols`, `iris_symbols_local`
- `iris_interop_query`, `iris_production`, `iris_production_item`, `iris_production_diff`
- `iris_execute_method`, `iris_generate`, `iris_generate_class`, `iris_generate_test`
- `iris_debug`, `iris_coverage`, `iris_get_log`, `iris_admin`, `iris_table_info`
- `iris_macro`, `iris_message_body`, `iris_business_rule_info`
- `docs_introspect`, `iris_doc`, `iris_doc_search`
- `find_subclass_implementations`, `extract_message_map_routing`, `resolve_dynamic_dispatch`

`server: None` → current active connection (existing behavior, fully backward-compatible).
`server: Some("prod")` → pool lookup, error if not found, no side effects on other calls.

Tools that are inherently single-instance (meta, config, container management) do NOT
get `server`: `check_config`, `iris_containers`, `iris_select_container`,
`iris_credential_list`, `iris_credential_manage`, telemetry, benchmark, skills, KB.

## Error Codes

Full registry in `data-model.md`. Summary:

| Code                      | Meaning                                                            |
| ------------------------- | ------------------------------------------------------------------ |
| `SERVER_NOT_FOUND`        | `server` param names an instance not in the pool                   |
| `SERVER_UNREACHABLE`      | Named server is known but currently unreachable                    |
| `REMOVE_NOT_ALLOWED`      | Server sourced from VS Code / fleet config — cannot remove via iad |
| `SESSION_STALE`           | WS session token references a server no longer in pool             |
| `SESSION_WS_DISCONNECTED` | WS connection dropped; auto-reconnect attempted                    |
| `SESSION_WS_UNAVAILABLE`  | IRIS Atelier API < v7; WS terminal not supported                   |
| `SESSION_IN_USE`          | Concurrent exec on same session token                              |
| `SESSION_TIMEOUT`         | No response within `IRIS_WS_TIMEOUT_SECS` seconds (default: 30)    |
| `CONFIRM_REQUIRED`        | Destructive tool called without confirmation token                 |
| `CONFIRM_EXPIRED`         | Confirmation token expired (5-minute window)                       |
| `CONFIRM_MISMATCH`        | Confirmation token bound to different resource                     |
| `IMPORT_NO_SOURCE`        | `iris_import_servers` found no VS Code / Cursor settings           |
| `HL7_NOT_AVAILABLE`       | `EnsLib.HL7.Schema` not installed on this IRIS instance            |

## Constitution Check

| Principle                      | Status             | Notes                                                                                                                                                                                           |
| ------------------------------ | ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary         | PASS               | No new IRIS classes. `servers.json` is a client-side file.                                                                                                                                      |
| II. ObjectScript Sanity        | NEEDS VERIFICATION | 11 new ObjectScript APIs (namespace admin, observability, security, HL7, Mermaid, storage). All listed in `research.md` §API Verification. MUST verify before 072-c implementation tasks begin. |
| III. HTTP-First Execution      | PASS               | Pool uses HTTP connections. WS terminal is HTTP upgrade over same port.                                                                                                                         |
| IV. Test-First, Fixture-Driven | PASS               | Unit tests for pool resolution, cascade parsing, credential ops. Integration tests `#[ignore]`.                                                                                                 |
| V. Output Shape Parity         | PASS               | `server` is additive param. New tools are new. No breaking schema changes.                                                                                                                      |
| VI. Environment Guard          | PASS               | `global_kill` and `iris_namespace_create` are write-gated (tasks T079, T084). Write-gate unit tests T079b, T084b. All other new tools are read-only.                                            |
| VII. Dependency Minimalism     | PASS               | `tokio-tungstenite` justified in `research.md` §Dependency Justification (RFC 6455 — no stdlib alternative). `uuid` already in workspace. `similar` to be verified at T066.                     |
| VIII. 90% Coverage Gate        | PASS               | Coverage-check tasks T041b, T065b, T112b in Polish of each phase. Pool and cascade parser are unit-testable. WS session covered by integration tests.                                           |
| IX. Tool Lift Requirement      | PASS               | Benchmark tasks MUL-01/02/03 in plan.md. Lift measurement phases added before Polish for 072-b (T061a–T061b) and 072-c (T109–T110). Results in `lift-results.md`.                               |
| X. ObjectScript Coverage       | N/A                | No new ObjectScript shipped to IRIS.                                                                                                                                                            |

## Delivery Phases

**072-a: Foundation** — connection pool, server registry, management tools, `server` param
on all execution tools. No WS. No new Pierre ports. Ships as a standalone milestone.

**072-b: WebSocket Sessions** — `iris_ws_open/exec/close`, WS session pool, `V7` enum
value, `SESSION_WS_UNAVAILABLE` fallback. Requires 072-a.

**072-c: Cross-Instance and Pierre Ports** — comparison tools, `global_kill/preview`,
namespace/database admin, observability, security, HL7, Mermaid. Requires 072-a.
Order of 072-b and 072-c is independent.
