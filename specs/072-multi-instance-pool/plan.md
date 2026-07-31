# Implementation Plan: Multi-Instance Connection Pool

**Branch**: `072-multi-instance-pool` | **Date**: 2026-07-30 | **Spec**: [spec.md](spec.md)

## Summary

Three independent sub-deliverables. 072-a is the foundation that 072-b and 072-c depend
on for server routing. 072-b (WS sessions) and 072-c (comparison + Pierre ports) are
independent of each other.

## Technical Context

**Language**: Rust 2021 edition (workspace)
**New dependencies**:

- `tokio-tungstenite` (WS client) — 072-b only
- `keyring` v4 already present; only `set_password` is new usage
- `uuid` — already in workspace (used by execute_via_generator for temp class names);
  use for WS session tokens
  **Testing**: `cargo test` (unit) + `cargo test -- --include-ignored` (integration, live IRIS)
  **Integration tests**: `--test-threads=1` required
  **Target platforms**: macOS arm64/x86_64, Linux x86_64, Windows x86_64

## Phase 072-a: Foundation

### 072-a Architecture

#### File layout

```text
crates/iris-agentic-dev-core/src/iris/
  connection_pool.rs       NEW — ConnectionPool struct, cascade loader
  servers_config.rs        NEW — iad-native servers.json read/write
  server_manager.rs        MODIFY — add store_credential(), iterate all profiles
crates/iris-agentic-dev-core/src/tools/
  mod.rs                   MODIFY — pool field on IrisTools, server param on all exec tools
  server_tools.rs          NEW — iris_servers, iris_add_server, iris_remove_server,
                                 iris_test_server, iris_import_servers
```

#### `ConnectionPool`

```rust
pub struct ConnectionPool {
    instances: HashMap<String, Arc<IrisConnection>>,
    default_name: Option<String>,
}

impl ConnectionPool {
    pub fn get(&self, name: Option<&str>) -> Result<Arc<IrisConnection>, McpError>;
    pub fn names(&self) -> Vec<&str>;
    pub fn default_name(&self) -> Option<&str>;
    pub fn len(&self) -> usize;
}
```

`get(None)` → default instance → `IRIS_UNREACHABLE` if no default.
`get(Some("x"))` → named instance → `SERVER_NOT_FOUND` if absent.

#### Cascade loader

```rust
pub fn load_pool(config_file: Option<&Path>) -> ConnectionPool
```

Reads sources in priority order, deduplicates by name (first wins for same name across
sources), builds `IrisConnection` per server. Credentials resolved lazily on first use
OR eagerly at startup (TBD — benchmark startup cost).

The existing single-connection startup path in `IrisTools::from_env()` becomes a call
to `load_pool()` with default pointing at the env-var-derived instance.

#### `servers_config.rs`

```rust
pub struct ServerEntry {
    pub host: String,
    pub port: u16,
    pub namespace: String,
    pub username: String,
    pub description: Option<String>,
    pub scheme: Option<String>,     // "http" | "https", default "http"
}

pub struct ServersConfig {
    pub version: u32,
    pub servers: HashMap<String, ServerEntry>,
    pub default: Option<String>,
}

pub fn load_native_config() -> ServersConfig;
pub fn save_native_config(cfg: &ServersConfig) -> Result<(), ConfigError>;
pub fn native_config_path() -> PathBuf;  // ~/.config/iris-agentic-dev/servers.json
```

`save_native_config` creates the directory if absent. Writes atomically (temp file +
rename).

#### `server_manager.rs` additions

```rust
/// Store a password in the OS keychain using the SM format.
pub fn store_credential(server_name: &str, username: &str, password: &str)
    -> Result<(), SmCredentialError>;
```

Also change `parse_sm_settings()` to return all profiles instead of needing
`select_server()` to filter — `select_server()` stays for the legacy single-connection
path but `load_pool()` calls `parse_sm_settings()` directly.

#### `IrisTools` changes

```rust
pub struct IrisTools {
    pub connection: Arc<Mutex<ConnectionState>>,  // unchanged — default/active connection
    pub pool: ConnectionPool,                     // NEW — all known instances
    tool_router: ToolRouter<IrisTools>,
    // ... rest unchanged
}
```

`pool` is populated at startup. `connection` is set to the default from the pool (same
as today for single-instance users).

Execution tools:

```rust
// Before:
let iris = self.get_iris_reloaded().await?;

// After (for tools that gain server param):
let iris = self.pool.get(server.as_deref())?;
// or fall back to default:
let iris = match server {
    Some(ref s) => self.pool.get(Some(s.as_str()))?,
    None => self.get_iris_reloaded().await?,
};
```

The fallback to `get_iris_reloaded()` when `server == None` preserves hot-reload
behavior (spec 034) for the default connection.

### Tool Descriptions: server param

Add to every affected tool description:

> `server` (optional): name of a registered IRIS instance to use for this call. If
> omitted, uses the default connection. Use `iris_servers` to list available instances.

### Server Tool Descriptions

**`iris_servers`**

> List all registered IRIS instances from all configuration sources. Shows name, host,
> port, namespace, the config source (iad-native, vscode, cursor, fleet, env), and
> whether each instance is currently reachable. Use `iris_add_server` to register new
> instances.

**`iris_add_server`**

> Register a new IRIS instance. Writes server details to
> `~/.config/iris-agentic-dev/servers.json` and stores the password in the OS keychain
> — the password never appears in any config file. Uses the same keychain format as VS
> Code Server Manager, so credentials are shared if both are installed.
> Params: `name` (unique identifier), `host`, `port`, `namespace`, `username`,
> `password`, `description` (optional), `scheme` (optional, default "http").

**`iris_remove_server`**

> Remove a server registered in the iad-native config. Also removes its keychain entry.
> Cannot remove servers sourced from VS Code settings (edit settings.json directly).

**`iris_test_server`**

> Test connectivity to a named server. Returns Atelier API version, IRIS version string,
> accessible namespaces, and round-trip latency in milliseconds. Does not change the
> active connection.

**`iris_import_servers`**

> Import IRIS server definitions from VS Code or Cursor settings into the iad-native
> config. Reads passwords from the existing keychain — no re-entry required. Skips
> servers already present in the iad-native config. Reports count imported and skipped.

## Phase 072-b: WebSocket Sessions

### New dependency

```toml
tokio-tungstenite = { version = "0.26", features = ["native-tls"] }
```

(Pin to workspace minimum TLS requirement — matches existing `reqwest` TLS feature.)

### File layout

```text
crates/iris-agentic-dev-core/src/iris/
  ws_session.rs            NEW — WsSession, WsSessionPool, auth flow
crates/iris-agentic-dev-core/src/tools/
  ws_tools.rs              NEW — iris_ws_open, iris_ws_exec, iris_ws_close
```

### `ws_session.rs`

```rust
pub struct WsSession {
    pub server_name: String,
    pub namespace: String,
    pub uuid: String,
    write: SplitSink<WsStream, Message>,
    read: SplitStream<WsStream>,
}

pub struct WsSessionPool {
    sessions: Mutex<HashMap<(String, String, String), WsSession>>,
}

impl WsSessionPool {
    pub async fn open(conn: &IrisConnection, server_name: &str, namespace: &str)
        -> Result<String, McpError>;  // returns token
    pub async fn exec(token: &str, code: &str) -> Result<String, McpError>;
    pub async fn close(token: &str) -> Result<(), McpError>;
    pub fn parse_token(token: &str) -> Option<(String, String, String)>;
}
```

Token format: `ws:<server-name>:<NAMESPACE>:<uuid>` — all three components URL-safe.

### Auth flow for WS upgrade

```rust
// Step 1: GET /api/atelier/ with Basic auth → extract CSPSESSIONID cookie
let session_cookie = get_csp_session_cookie(conn).await?;

// Step 2: WS upgrade with Cookie header
let url = format!("ws://{}:{}/api/atelier/v7/{}/terminal", host, port, namespace);
let request = http::Request::builder()
    .uri(&url)
    .header("Cookie", format!("CSPSESSIONID={}", session_cookie))
    .body(())?;
let (ws, _) = tokio_tungstenite::connect_async(request).await?;
```

### WS message protocol (internal)

```rust
#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    Init,
    Prompt { value: String },
    Output { data: String },
    Read { prompt: String },  // READ command — respond with user input or ""
    Color { data: String },   // syntax highlight — discard
}

#[derive(serde::Serialize)]
#[serde(tag = "type")]
enum ClientMessage {
    Config { namespace: String, #[serde(rename = "rawMode")] raw_mode: bool },
    Prompt { input: String },
    Interrupt,
}
```

`iris_ws_exec` collects `Output` frames until the next `Prompt` frame, returns
concatenated `data` as the result. Timeout: 30s (configurable via env
`IRIS_WS_TIMEOUT_SECS`).

### `AtelierVersion` V7 gate

In `iris_ws_open` handler:

```rust
if iris.atelier_version < AtelierVersion::V7 {
    return Err(mcp_error("SESSION_WS_UNAVAILABLE: WebSocket terminal requires IRIS
        2023.2+ (Atelier API v7+). Use iris_execute with use_session: true for
        session-like behavior on older IRIS versions."));
}
```

### Tool Descriptions

**`iris_ws_open`**

> Open a persistent WebSocket terminal session on IRIS. Returns a session token that
> identifies the live IRIS process. All local variables, object references, and in-memory
> state survive for the lifetime of the session — including `%RegisteredObject` instances
> that cannot be serialized with the `use_session`/`%ctx` mechanism.
> Requires IRIS 2023.2 or later (Atelier API v7+). For older IRIS, use `iris_execute`
> with `use_session: true`.
> Params: `server` (optional), `namespace` (optional, defaults to connection namespace).

**`iris_ws_exec`**

> Execute ObjectScript in a persistent WebSocket session. The IRIS process is reused
> across calls — variables and objects set in previous calls are still available.
> Params: `session` (token from `iris_ws_open`), `code` (ObjectScript to run).
> Returns `output` (what WRITE/print produced) and the `session` token unchanged.

**`iris_ws_close`**

> Close a WebSocket session and release the IRIS process.
> Params: `session` (token from `iris_ws_open`).

## Phase 072-c: Comparison Tools and Pierre Ports

### Cross-Instance Comparison

**`compare_document(document, server_a, server_b, namespace?)`**

Implementation: fetch class/routine source text from both servers via Atelier GET
`/document/{name}`, run `similar` crate diff (already in workspace? check), return
unified diff format.

If `similar` not in workspace: add `similar = "2"`. One-dep rule: justified because
diff output is the entire user value of these tools.

**`compare_namespace(namespace, server_a, server_b)`**

Fetch class list from both servers, compute symmetric difference + changed (by size/hash
or by full diff). Returns three lists: `only_in_a`, `only_in_b`, `different` (present in
both but content differs).

### Global Confirmation Pattern

Confirmation tokens: `HashMap<String, (Instant, String, String)>` keyed on
`(server_name, global_name)`, value is `(created_at, token, global)`. 5-minute TTL.
Token is a `uuid::Uuid::new_v4().to_string()`. Stored in `IrisTools` field
`confirm_tokens: Mutex<HashMap<String, ConfirmEntry>>`.

**`global_preview(global, server?, count?)`**

Runs `iris_global` read internally, returns top N entries, mints token. Response:

```json
{
  "entries": [...],
  "total_subscripts": 12345,
  "confirm_token": "abc-def-...",
  "confirm_expires": "2026-07-30T18:23:00Z"
}
```

**`global_kill(global, server?, confirm_token)`**

Validates token: exists, not expired, bound to same `(server, global)`. If invalid →
`CONFIRM_REQUIRED` / `CONFIRM_EXPIRED` / `CONFIRM_MISMATCH`. If valid → executes
`Kill ^{global}` via `iris_execute`, removes token from store.

### Namespace / Database Admin (ported from Pierre)

These wrap Atelier admin REST endpoints or ObjectScript system utilities:

| Tool                    | ObjectScript / Endpoint                                 |
| ----------------------- | ------------------------------------------------------- |
| `iris_namespace_list`   | `%SYS.Namespace:ListAll()`                              |
| `iris_namespace_create` | `%SYS.Namespace:Create()` — write gate required         |
| `iris_database_list`    | `##class(Config.Databases).List()`                      |
| `iris_database_stats`   | `^%SYSMON` or `%SYSTEM.SQL.Security` — TBD on live IRIS |

These require `write_tools_enabled` for destructive variants. Read-only list tools are
always available.

### Observability (ported from Pierre)

| Tool              | Implementation                                                                     |
| ----------------- | ---------------------------------------------------------------------------------- |
| `journal_search`  | `##class(SYS.Journal.File).GetJournalFile()` — query by date range, global pattern |
| `query_audit_log` | `%SYS.Audit` table query via `iris_query` internally                               |
| `stream_inspect`  | `%%class(%Stream.GlobalCharacter).%OpenId()` — read stream content by OID          |

### Security (ported from Pierre)

| Tool                | Implementation                                          |
| ------------------- | ------------------------------------------------------- |
| `my_access`         | `$username`, `%SYS.Security.Users:Get()`, role list     |
| `capability_matrix` | Cross `(user/role, resource)` — `%SYS.Security` classes |

These read-only, no write gate needed. Return structured JSON.

### HL7 Schema (ported from Pierre)

| Tool                 | Implementation                                     |
| -------------------- | -------------------------------------------------- |
| `hl7_schema_list`    | `##class(EnsLib.HL7.Schema).GetSchemaList()`       |
| `hl7_schema_inspect` | `##class(EnsLib.HL7.Schema).GetSegmentStructure()` |

Only available if `EnsLib.HL7.Schema` exists on the instance. Return `HL7_NOT_AVAILABLE`
if class absent.

### Graph / Mermaid (ported from Pierre)

**`mermaid_class(class, depth?, server?)`**

Walks superclass chain and key relationships via `%Dictionary.CompiledClass` queries,
emits Mermaid `classDiagram` syntax. No external dependency — string generation only.

**`mermaid_production(production, server?)`**

Walks `Ens.Config.Production` items, emits Mermaid `flowchart LR` showing components
and connections. Supplements (not replaces) existing `iris_production` tool.

### `resolve_storage` (ported from Pierre)

**`resolve_storage(class, server?)`**

Returns the global map for a `%Persistent` class: what global name and subscript
structure holds the data, derived from `%Dictionary.CompiledStorage`. Useful when
debugging unexpected global usage.

### `stream_inspect` (ported from Pierre)

**`stream_inspect(oid, server?)`**

Read a `%Stream` object by its OID (from a prior `iris_global` or `iris_query` result).
Returns content as a string. Handles `%Stream.GlobalCharacter` and
`%Stream.GlobalBinary` (binary returns hex-encoded).

## Benchmark Tasks

### MUL-01 (connection pool routing)

```yaml
id: MUL-01
category: MUL
description: "Two IRIS instances are registered: 'dev' and 'prod'. Count classes in USER namespace on each. Report both counts without switching the active connection."
expected_behavior: "Agent calls iris_execute twice with server: 'dev' and server: 'prod' respectively. Both calls succeed. Agent reports two distinct counts. Active connection does not change."
```

### MUL-02 (server management round-trip)

```yaml
id: MUL-02
category: MUL
description: "Register a new IRIS instance named 'test' at localhost:52780, list all servers, then remove 'test'."
expected_behavior: "Agent calls iris_add_server, then iris_servers (confirming 'test' appears), then iris_remove_server. iris_servers after removal does not show 'test'."
```

### MUL-03 (WS persistent session)

```yaml
id: MUL-03
category: MUL
description: "Open a WebSocket session, set a variable x = 42 in one call, read it back in a second call."
expected_behavior: "Agent calls iris_ws_open, then iris_ws_exec twice with the same session token. Second call outputs 42 without re-setting x."
```

## `similar` Crate Decision

`similar` (diff library) is not currently in the workspace. Adding it is justified for
072-c comparison tools — the entire user value of `compare_document` is the diff output.
Check `cargo tree` before adding; if `similar` is already a transitive dependency, use
it directly. Otherwise add to workspace `Cargo.toml`.

## Backward Compatibility

- All existing tools with no `server` param: unchanged behavior.
- `iris_select_container`: still works. The pool does not affect it. The session-wide
  swap remains available for users who want it. Not deprecated — it has a different
  semantic (change the default for the session).
- `check_config` / `iris_info`: continue to reflect the single active connection.
- `AtelierVersion` enum: new `V7` variant inserted between `V2` and `V8`. Detection
  logic updated. No existing code breaks — `V8` detection was `>= 8` and stays so.

## Startup Performance

Loading all configured servers at startup adds HTTP round-trips. Options:

1. **Lazy**: build `ConnectionPool` entries without connecting; connect on first use.
2. **Eager with timeout**: attempt all connections at startup with 2s timeout; mark
   unreachable ones but don't block startup.

Option 1 is safer — no startup latency regression. `iris_servers` triggers reachability
checks explicitly. Choose option 1 for 072-a, revisit if users want startup validation.
