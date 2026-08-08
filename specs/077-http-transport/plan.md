# Plan 077 — HTTP Transport

## Tech stack

- Rust 2021 edition
- `rmcp` 1.6 — add `transport-streamable-http-server` feature
- `axum` (pulled in by rmcp's http transport feature, no direct dep needed)
- `tokio` (already a dep)
- Test: `reqwest` (already used in integration tests) or raw `tokio::net::TcpStream`

## Architecture

All changes are in `crates/iris-agentic-dev-bin/src/cmd/mcp.rs`. The existing
stdio path is untouched. A new branch in `McpCommand::run()` handles `--transport http`.

```text
McpCommand::run()
  if transport == "stdio"  → existing stdio path (unchanged)
  if transport == "http"   → new HTTP path (this spec)
  else                     → error + exit 1
```

### HTTP path (new)

```rust
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, session::local::LocalSessionManager,
};
use rmcp::transport::streamable_http_server::axum::Router as McpRouter;

// Build the IrisTools service (identical to stdio path)
let tools = IrisTools::with_registry_and_toolset(...)?;

// Bind
let addr = SocketAddr::from((bind_ip, self.port));
let listener = TcpListener::bind(addr).await
    .map_err(|e| anyhow!("Failed to bind {}:{}: {}", bind_ip, self.port, e))?;

// Build axum router with MCP handler
let session_mgr = LocalSessionManager::default();
let config = StreamableHttpServerConfig::default();
let router = McpRouter::new(tools, session_mgr, config);

tracing::info!("iris-agentic-dev mcp listening on http://{}/mcp", addr);

axum::serve(listener, router).await?;
```

### Bind flag

`--bind <addr>` added to `McpCommand`. Default `"127.0.0.1"`. Parsed with
`addr.parse::<IpAddr>()`.

### Feature flag in Cargo.toml

```toml
# workspace Cargo.toml
rmcp = { version = "1.6", features = [
  "server", "macros", "schemars",
  "transport-io",
  "transport-streamable-http-server",   # <-- new
] }
```

No separate feature gate on the iris-agentic-dev side — always compiled in.

## File changes

```text
Cargo.toml                                     — add transport-streamable-http-server feature
crates/iris-agentic-dev-bin/src/cmd/mcp.rs     — wire --transport http branch + --bind flag
docs/connecting.md                             — new "HTTP transport" section
```

## Test file

```text
crates/iris-agentic-dev-core/tests/http_transport_tests.rs   (new, integration)
```

## Phases

### Phase 1 — Tests first

Write `tests/http_transport_tests.rs` with T-077-01 through T-077-04.
All tests will fail (T-077-01 may pass since the error path can be tested with
a stub). Mark T-077-02 through T-077-05 `#[ignore]` (require live process).

### Phase 2 — Cargo feature + mcp.rs

Add `transport-streamable-http-server` to workspace `Cargo.toml`.
Implement the `--transport http` branch and `--bind` flag in `mcp.rs`.
`cargo build` must succeed.

### Phase 3 — Run unit test

T-077-01 (invalid transport): run without `--include-ignored`. Must pass.

### Phase 4 — Run integration tests

T-077-02 through T-077-04 against local process. Use `--test-threads=1`.

### Phase 5 — E2E test against AI Hub

T-077-05: manually verify (or scripted via `iris_execute`) that the AI Hub
Remote MCP connection works against aihub-iris-116.

### Phase 6 — Documentation

Add "HTTP transport" section to `docs/connecting.md`. Run markdownlint + prettier.

## Key decisions

- **rmcp's built-in HTTP transport** rather than writing our own axum router —
  keeps the implementation to ~30 lines and ensures protocol compliance
- **127.0.0.1 default bind** — explicit opt-in for external access prevents
  accidentally exposing the endpoint on a shared network
- **No auth on the HTTP endpoint** — same posture as stdio (process-level access
  controls). Documented in README as "put behind a reverse proxy if you need auth"
- **Same IrisTools instance shared across connections** — the tools struct is
  Clone+Send+Sync (it's Arc-backed internally), so concurrent HTTP clients share
  the connection pool
- **Port conflict → clear error** — `TcpListener::bind` gives a usable OS error;
  wrap it with the port number for actionability
