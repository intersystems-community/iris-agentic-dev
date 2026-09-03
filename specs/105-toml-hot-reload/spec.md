# Feature Specification: TOML and servers.json Hot-Reload

**Feature Branch**: `091-toml-hot-reload`
**Created**: 2026-09-02
**Status**: Draft

## Overview

The connection pool loads once at MCP server startup. Edits to `.iris-agentic-dev.toml`
or `~/.config/iris-agentic-dev/servers.json` have no effect until the MCP server
restarts — in Claude Desktop this means closing and relaunching the entire app. Worse,
`iris_add_server` writes an entry to `servers.json` but the running pool never picks it
up, leaving the agent unable to use the server it just registered. This spec adds a
`servers.json` file watcher to the existing `ConfigWatcher`-based reload loop and adds
a new `iris_reload_config` tool as an explicit reload trigger the agent can call after
`iris_add_server`.

---

## User Scenarios & Testing

### User Story 1 — Use a server immediately after iris_add_server (Priority: P1)

An agent calls `iris_add_server` to register a new IRIS instance. It then calls
`iris_reload_config`. Without leaving the Claude Desktop session, it calls
`iris_execute` targeting the new server by name. The call succeeds — no restart
required.

**Acceptance Scenarios**:

1. Given a running MCP session, When `iris_add_server` is called followed by
   `iris_reload_config`, Then `iris_servers` lists the new entry and `iris_execute`
   against it succeeds.
2. Given `iris_add_server` writes an entry to `servers.json`, When the session idles
   for 5 seconds (file-watcher tick), Then the pool has absorbed the entry without
   any explicit reload call.
3. Given the agent calls `iris_reload_config` when nothing has changed, Then the
   response is `{reloaded: false, reason: "no_change"}` — the call is a no-op.
4. Given `iris_reload_config` is called and config has changed, Then the response
   includes `{reloaded: true, servers_added: N, servers_removed: M}`.

### User Story 2 — Edit toml mid-session without restart (Priority: P1)

A developer edits `.iris-agentic-dev.toml` to change the default namespace while
Claude Desktop is open. The next tool call in the same session uses the updated
namespace without a restart.

**Acceptance Scenarios**:

1. Given `.iris-agentic-dev.toml` is edited with a new `namespace`, When the next
   tool call fires (triggering the watcher check), Then the connection pool reflects
   the new namespace.
2. Given `.iris-agentic-dev.toml` is deleted mid-session, When the next tool call
   fires, Then the connection falls back to env-var resolution — the last declared
   gate does not survive the deletion (same contract as the 085 T028 test).
3. Given `.iris-agentic-dev.toml` becomes unparseable mid-session, When the next
   tool call fires, Then the last known-good config stays in force and
   `check_config` reports `config_parse_error` (same contract as 085 T028
   unparseable test).

---

## Functional Requirements

- **FR-001**: The existing `ConfigWatcher` in `mcp.rs` watches
  `.iris-agentic-dev.toml`. A second `ConfigWatcher` instance must be created for
  `~/.config/iris-agentic-dev/servers.json` (the `native_config_path()`). Both are
  checked on every inbound tool call (the existing `get_iris_reloaded` check point).
- **FR-002**: When either watcher fires `has_changed()`, the pool re-merges
  `ServersConfig` from disk alongside the TOML connection config. New entries are
  added; removed entries are dropped; changed entries are updated.
- **FR-003**: New `iris_reload_config` tool: takes no required arguments. Checks both
  watchers. If either has changed, reloads the pool synchronously and returns
  `{reloaded: true, servers_added: N, servers_removed: M}`. If neither has changed,
  returns `{reloaded: false, reason: "no_change"}`. Always succeeds (never an error
  path).
- **FR-004**: `iris_reload_config` must be listed in `TOOL_NAMES` and wired in the
  `call_tool` dispatch block — the #111 pattern test applies.
- **FR-005**: The TOML watcher's existing edge-case contracts (deletion → fallback,
  unparseable → last-known-good gate) are unchanged. The servers.json watcher uses
  the same semantics: deletion clears iad-native entries, leaving vscode/fleet/env
  sources intact.
- **FR-006**: `check_config` gains a `servers_last_reload` field (ISO-8601 timestamp
  of the last successful reload, or null if never reloaded mid-session).
- **FR-007**: The file watcher check is O(1) — a stat call on each file. No fsevents
  daemon, no background thread, no `notify` crate dependency. Piggyback on the
  existing per-call check point in `get_iris_reloaded`.

---

## Key Entities

- **`ConfigWatcher`** (tools/mod.rs, already exists): mtime-based file change
  detector. Add a second instance for `servers.json` alongside the existing TOML
  watcher in `IrisTools`.
- **`iris_reload_config`** (tools/mod.rs): new tool. Explicit reload trigger. Returns
  structured diff of what changed.
- **`mcp.rs`** (`iris-agentic-dev-bin`): construction site for the second
  `ConfigWatcher`. Pass both watchers into `IrisTools`.
- **`servers_config::load_native_config`** (servers_config.rs): already exists.
  Called during reload to re-read `servers.json`.

---

## Success Criteria

- `iris_add_server` + `iris_reload_config` in the same MCP session enables immediate
  use of the new server without restarting.
- Editing `.iris-agentic-dev.toml` mid-session is picked up on the next tool call.
- `iris_reload_config` returns a structured diff, not a generic ack.
- All existing 085 config-reload edge-case tests continue to pass unchanged.
- Binary-invocation test: `initialize` + `tools/list` confirms `iris_reload_config`
  appears in the tool list.
- Live IRIS integration test: `iris_add_server` → `iris_reload_config` →
  `iris_execute` succeeds against the new server in one session.

---

## Out of Scope

- Push-based filesystem events (`notify` crate, FSEvents, inotify). Polling on each
  call is sufficient and avoids a background thread.
- Reloading mid-call (the watcher fires at call entry, not during execution).
- Restarting the MCP server process itself.
- Hot-reloading gate policy from the TOML (that contract already exists via 085 and
  is unchanged).

---

## Assumptions

- The `ConfigWatcher::has_changed` stat approach is fast enough that checking two
  files per call adds no perceptible latency.
- `servers.json` deletions are intentional user action; clearing iad-native entries
  on deletion matches user expectation.
- The 095 plaintext-password fallback lands before this spec, so `servers.json` may
  carry password fields. The reload path reads them as-is via the existing
  `load_native_config` path.
