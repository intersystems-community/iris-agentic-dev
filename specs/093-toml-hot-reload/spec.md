# Feature Specification: toml Pool Hot-Reload

**Feature Branch**: `093-toml-hot-reload`
**Created**: 2026-09-02
**Status**: Draft
**Input**: User description: "toml Pool Hot-Reload — pool loaded once at startup; edits
(including iris_add_server writes) have no effect until MCP host restarts."

## User Scenarios & Testing

### User Story 1 - Agent adds a server and uses it immediately (Priority: P1)

An agent calls `iris_add_server` to register a new IRIS instance, then calls
`iris_reload_pool`, then calls `iris_test_server` on the new entry — all in the same
session, without restarting Claude Desktop.

**Why this priority**: Without this, `iris_add_server` is useless in an agent session.
The user must restart the MCP host every time they add a server, which defeats the
purpose of the tool.

**Independent Test**: Write a new `[servers.test-reload]` entry to a temp toml, call
`iris_reload_pool`, assert the new entry appears in `iris_servers` output.

**Acceptance Scenarios**:

1. **Given** a new server entry written to toml by `iris_add_server`, **When**
   `iris_reload_pool` is called, **Then** the pool is rebuilt and the new server appears
   in `iris_servers`.
2. **Given** a server entry removed from toml manually, **When** `iris_reload_pool` is
   called, **Then** that server no longer appears in `iris_servers`.
3. **Given** `iris_reload_pool` is called and the toml file does not exist, **When** it
   returns, **Then** `servers_loaded: 0` and no error — the pool is empty but valid.
4. **Given** `iris_reload_pool` is called and the toml has a parse error, **When** it
   returns, **Then** the response contains the parse error and the existing pool is left
   intact (fail-safe: never wipe a working pool on a bad file).

---

### User Story 2 - Manual toml edit takes effect without tool call (Priority: P2)

A developer edits `~/.iris-agentic-dev.toml` in a text editor. Within one tool call
cycle — the next time any iad tool is called — the pool reflects the change automatically.

**Why this priority**: Reduces friction for power users who prefer editing toml directly.
Reuses the existing `ConfigWatcher` mtime mechanism from 034.

**Independent Test**: Write a new server entry to toml directly (no tool call), then call
`iris_servers` — assert the new server appears without calling `iris_reload_pool`.

**Acceptance Scenarios**:

1. **Given** a manual toml edit adding a new server, **When** any iad tool is next called,
   **Then** the new server is available — without calling `iris_reload_pool` explicitly.
2. **Given** a manual toml edit with a parse error, **When** any iad tool is next called,
   **Then** the error is logged and the existing pool is preserved.

---

### Edge Cases

- What happens when `iris_reload_pool` is called concurrently from two tool calls?
  → Arc swap is atomic; one reload wins, the other sees the updated pool.
- What happens when pool reload removes the server currently in use by a running tool?
  → That tool holds an `Arc<IrisConnection>`; it completes normally. The removed entry
  is just absent from new lookups.
- What happens when `iris_add_server` writes a duplicate name already in the pool?
  → toml write wins (overwrite); reload replaces the old entry.

## Requirements

### Functional Requirements

- **FR-001**: Add `iris_reload_pool` as a new MCP tool. Returns
  `{success, servers_loaded, servers: [...names...], note}` where `note` states the MCP
  protocol constraint: newly added servers are routable immediately, but do not appear in
  the model's tool-list until the next `initialize` handshake.
- **FR-002**: `iris_reload_pool` rebuilds the pool by calling `load_pool` with the same
  config path used at startup. The new pool replaces the old one atomically via
  `Arc::new(load_pool(...))` — same pattern as connection hot-reload in 034.
- **FR-003**: The existing `ConfigWatcher` mtime check in `check_reload` (called on every
  tool invocation) is extended to also rebuild the pool when a file change is detected.
- **FR-004**: Pool rebuild on parse error is fail-safe: the existing `Arc<ConnectionPool>`
  is preserved; the error is returned in the `iris_reload_pool` response or logged for
  background reload.
- **FR-005**: `iris_reload_pool` is classified as read-only in the write gate (reads toml,
  does not modify IRIS state).
- **FR-006**: `iris_servers` reflects the post-reload pool state immediately after
  `iris_reload_pool` completes.
- **FR-007**: The `iris_reload_pool` response includes a `note` field documenting the MCP
  protocol limitation on tool-list refresh, and a suggestion: "To see new servers in
  the model's tool list, restart Claude Desktop (or re-run `initialize`)."

### Key Entities

- **`iris_reload_pool` tool**: new MCP tool; triggers synchronous pool rebuild.
- **`Arc<ConnectionPool>`**: swapped atomically on reload; held by `IrisTools.pool`.
- **`ConfigWatcher`**: existing mtime watcher; extended to trigger pool swap on file
  change alongside the existing connection re-probe.
- **`load_pool(config_file)`**: existing function in `connection_pool.rs`; reused as-is.

### Test Requirements (non-negotiable — three layers)

- **TR-001 Unit**: toml round-trip — parse a toml string with two `[instance.*]` entries
  via `toml::from_str`, pass the result to `load_pool` with a temp file path, assert pool
  contains both names; repeat with one removed, assert pool shrinks. No live IRIS needed.
- **TR-002 Binary invocation** (`#[ignore]`, `IAD_BINARY` env): spawn binary, call
  `iris_add_server` to write a new entry to a temp toml, call `iris_reload_pool`, call
  `iris_servers` — assert new server name appears in response. No live IRIS needed.
- **TR-003 Live IRIS** (`#[ignore]`, `iris-dev-iris` localhost:52780, `--test-threads=1`):
  write a new `[instance.*]` entry to `.iris-agentic-dev.toml`, call `iris_reload_pool`,
  call `iris_test_server` against the new entry — assert reachable/error response returned
  (not `SERVER_NOT_FOUND`).

## Success Criteria

### Measurable Outcomes

- **SC-001**: `iris_add_server` + `iris_reload_pool` + `iris_test_server <new-name>` all
  succeed in the same session without any MCP host restart.
- **SC-002**: A manual toml edit is reflected within one subsequent tool call (background
  watcher path — no explicit `iris_reload_pool` required).
- **SC-003**: A toml parse error during reload does not wipe the existing pool — verified
  by a unit test that asserts pool unchanged after a bad-toml reload.
- **SC-004**: Binary-invocation test covers `iris_reload_pool` end-to-end against the
  compiled binary (catches "wired but ignored" regressions like the #111 pattern).
