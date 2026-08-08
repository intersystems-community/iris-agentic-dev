# Spec 077 — HTTP Transport

## Overview

Enable `iris-agentic-dev mcp --transport http --port <N>` to start the MCP server
on a local HTTP port using the MCP streamable-HTTP transport (rmcp
`transport-streamable-http-server` feature). This lets AI Hub agents (and Claude
Desktop, and any other MCP client that speaks HTTP) connect to a single long-running
iris-agentic-dev process rather than spawning a new process per session.

## Problem

Today `iris-agentic-dev mcp` only speaks stdio. The `--transport` and `--port` CLI
flags are declared but unimplemented — they silently do nothing. AI Hub's
`<MCP><Remote>` element requires an HTTP MCP endpoint; without it the only AI Hub
integration path is stdio (Spec 076), which spawns a new process and re-runs
connection discovery on every agent session. A long-lived HTTP server also enables
Docker-based setups where the binary runs in a sidecar container, not on the IRIS
host.

## Goals

- `iris-agentic-dev mcp --transport http --port 8765` starts an HTTP MCP server
- Single process, multiple concurrent client connections
- All existing env vars and `.iris-agentic-dev.toml` discovery work unchanged
- Tested with an HTTP MCP client (rmcp test client or curl)
- 90%+ line coverage on the new transport branch

## Non-goals

- TLS / HTTPS (can be added later; document that a reverse proxy covers this)
- Authentication on the HTTP endpoint (same as stdio — caller controls access)
- WebSocket transport
- Load balancing or multi-instance pooling

## Functional requirements

### FR-001 — Transport flag

`iris-agentic-dev mcp --transport http` starts the streamable-HTTP transport.
`--transport stdio` (default) is unchanged. Any other value prints a clear error
and exits 1.

### FR-002 — Port flag

`--port N` sets the listen port. Default `8080`. Conflicts with any other process
on that port → clear error message, exit 1.

### FR-003 — Bind address

Listens on `127.0.0.1:<port>` by default. `--bind 0.0.0.0` flag allows external
connections (required for Docker sidecar use).

### FR-004 — Existing discovery unchanged

All connection discovery logic (env vars, `.iris-agentic-dev.toml`, Docker scan,
port scan) runs identically regardless of transport.

### FR-005 — Graceful shutdown

`SIGINT`/`SIGTERM` → drain in-flight requests → exit 0. Log the port being released.

### FR-006 — Startup log

On successful bind: `iris-agentic-dev mcp listening on http://127.0.0.1:<port>/mcp`

### FR-007 — Cargo feature gate

New transport enabled via `transport-streamable-http-server` feature in rmcp.
Added to workspace `Cargo.toml` features list. Not behind a separate feature flag
in iris-agentic-dev itself — always compiled in.

## Test requirements

### T-077-01 — Unit: invalid transport value

`McpCommand { transport: "grpc", .. }.run()` returns an error containing
`"unknown transport"`. Pure unit test, no IRIS required.

### T-077-02 — Integration: HTTP server starts and responds

Start `iris-agentic-dev mcp --transport http --port 18765` in a background task.
Send an MCP `initialize` request to `http://127.0.0.1:18765/mcp`. Verify HTTP 200
and a valid MCP `InitializeResult` in the response body. Shut down after.

### T-077-03 — Integration: tools/list over HTTP

After initialize, send `tools/list`. Verify at least 20 tools returned.

### T-077-04 — Integration: port conflict error

Bind a TCP listener on port 18766. Attempt to start iris-agentic-dev on the same
port. Verify it exits with a non-zero code and a useful error message.

### T-077-05 — E2E: AI Hub Remote MCP connection

In the aihub-iris-116 container, create a ToolSet with
`<MCP><Remote URL="http://host.docker.internal:18765/mcp"/>`. Call `check_config`
via an agent. Verify response contains connection info.

## Acceptance criteria

- `cargo build` succeeds with the new feature enabled
- T-077-01 through T-077-04 pass without a live IRIS container
- T-077-05 passes against aihub-iris-116
- `docs/connecting.md` gains an "HTTP transport" section
- Coverage ≥ 90% on `cmd/mcp.rs` (measured with `cargo-llvm-cov --features testing`)
