# Spec 074: Write Server Allowlist

## Problem

With 072's multi-instance pool, a single session can address multiple IRIS servers
by name. An agent that knows "prod" is a valid server name can switch to it and issue
writes without any additional friction.

The existing `write_tools_enabled` flag is a per-connection setting in the toml file.
That works when each project has its own toml, but a fleet config or a session with
many servers registered doesn't prevent writes from going to a production instance
when the user only intended to write to dev.

## Goal

Add a `write_allowed_servers` config key: a list of server names that are permitted
to receive write calls. Any `server:` parameter naming a server outside the list blocks
the call immediately, before it reaches IRIS.

```toml
write_allowed_servers = ["dev", "staging"]
```

With this config, `iris_execute(server: "prod", ...)` returns an error.
`iris_execute(server: "dev", ...)` proceeds normally (subject to the existing
`write_tools_enabled` and `destructive_tools_enabled` gates).

Omitting `write_allowed_servers` preserves existing behavior — no server-name
filtering.

## User stories

**US1**: As a developer with dev, staging, and prod servers registered, I want writes
blocked to prod regardless of what I (or an agent) type, so I can't accidentally mutate
prod while the session has prod in context.

**US2**: As a team lead checking a production issue, I want to call `iris_execute` against
prod without write access from the same session where writes to dev are fine, so I can
inspect without touching.

**US3**: As a CI pipeline, I want to set `write_allowed_servers = ["ci-iris"]` so the
agent runner can never write to any server except the dedicated CI instance, even if
a prompt injection attempts to reroute writes.

## Config

### `.iris-agentic-dev.toml`

```toml
write_allowed_servers = ["dev", "staging"]
```

The list is case-insensitive, matched against the `server` parameter of the tool call.

When `write_allowed_servers` is set:

- Any write-capable tool call with `server: "X"` where `"X"` is not in the list returns
  `WRITE_SERVER_NOT_ALLOWED`.
- Read-only tools (`read_only_hint = true`) are unaffected — they work against any server.
- The default server (when `server` is omitted) is also checked: if the active connection's
  name is not in the list, writes are blocked.

Empty list `write_allowed_servers = []` blocks writes to all named servers including the
default. Equivalent to `write_tools_enabled = false` but scoped to name-based routing.

### Environment variable

`IRIS_WRITE_ALLOWED_SERVERS=dev,staging` — comma-separated, same semantics as the toml key.

## Error behavior

```json
{
  "error_code": "WRITE_SERVER_NOT_ALLOWED",
  "message": "Writes to server 'prod' are not allowed. write_allowed_servers is set to [\"dev\", \"staging\"].",
  "server": "prod",
  "tool": "iris_execute"
}
```

## Interaction with other gates

The check order for write tool calls:

1. `write_tools_enabled` — if false, block immediately
2. `write_allowed_servers` — if set and server not in list, block
3. `destructive_tools_enabled` — if false and tool is destructive, block
4. `policy.<server-name>.allow` — category gate
5. Execute

Read-only tools skip steps 1–3 entirely.

## Default server name

When `server` is omitted, the call goes to the active (default) connection. For the
allowlist check, use the server name from `IRIS_SERVER_NAME` or the key in the
`[instance.*]` block that matches the current connection. If the default connection has
no name (env-var or direct toml connection), skip the allowlist check — the allowlist
applies only to named pool entries.

## Implementation notes

- Gate lives in `policy/gate.rs` alongside the other gates. Needs access to
  `write_allowed_servers: Option<Vec<String>>` from config.
- Must distinguish read-only tools by checking against the tool's `annotations.read_only_hint`.
  This requires the gate to know the annotation for the calling tool — pass `is_read_only: bool`
  into `dispatch_gate` or expose the annotation lookup from the tool registry.
- Unit tests: allowlist blocks writes to unlisted servers; read-only tools pass through;
  allowlist absent means no filtering; default-connection-no-name passes through.
- The check must happen before any IRIS network call.

## Out of scope

- Allowlisting read tools (read-only tools are never blocked by this feature)
- Per-tool server allowlists (too granular; the binary read/write split is sufficient)
- Dynamic allowlist updates without restart (config is read at startup)
