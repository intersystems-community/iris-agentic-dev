# Spec 073: Separate Destructive Tools Gate

## Problem

`write_tools_enabled` currently gates all write operations together — compiling a class,
executing ObjectScript, and killing a global all go through the same flag. This means
a user who wants to allow editing/executing on a dev server must also allow the
destructive tools (`global_kill`, `iris_admin`, `iris_credential_manage`, etc.).

Real-world risk: an agent that switches server context (now possible with the 072 pool)
could issue a `global_kill` against the wrong instance before the user notices.
The Railway incident (prod DB deleted in under 10 seconds by an agent) is the
canonical example of this failure mode.

## Goal

Add a `destructive_tools_enabled` config key that independently gates the 7 tools
tagged `destructive_hint = true`. Default: `false` on all connections. Must be opted
in explicitly.

This creates a two-tier write policy:

- Tier 1 (`write_tools_enabled = true`): compile, execute, source control, routine writes
- Tier 2 (`destructive_tools_enabled = true`): global kill, namespace create, server remove, admin writes, credential manage, lookup write/delete, skill forget

A user can enable tier 1 without tier 2. Enabling tier 2 implies tier 1 (no point killing
globals if you can't write anything).

## User stories

**US1**: As a developer on a dev IRIS instance, I want to compile and execute code without
enabling global kill or admin-write tools, so an agent can't wipe data while helping
me debug.

**US2**: As an admin running a maintenance script, I want to enable destructive tools
for a specific session, not globally, so the permission is scoped.

**US3**: As a project team member, I want the project `.iris-agentic-dev.toml` to default
to no destructive tools, so a new contributor who clones the repo can't accidentally
delete globals by running an agent.

## Config

### `.iris-agentic-dev.toml`

```toml
# Allow compile, execute, source control
write_tools_enabled = true

# Additionally allow global_kill, iris_admin writes, iris_credential_manage,
# iris_lookup_manage (write/delete), iris_namespace_create, iris_remove_server,
# skill_forget
destructive_tools_enabled = true
```

`destructive_tools_enabled` defaults to `false` regardless of `write_tools_enabled`.
Setting `destructive_tools_enabled = true` with `write_tools_enabled = false` is
an error — emit `DESTRUCTIVE_REQUIRES_WRITES` and refuse to start.

### Per-server policy blocks

```toml
[policy.dev]
allow = ["compile", "execute", "query"]
destructive_tools_enabled = true  # opt-in for this server only
```

When a policy block overrides `destructive_tools_enabled`, it takes precedence over
the top-level key for that server.

### Environment variable

`IRIS_DESTRUCTIVE_TOOLS_ENABLED=1` enables destructive tools for the session.
Works the same as the toml key. For CI pipelines that need it.

## Affected tools

The 7 tools with `destructive_hint = true` (set in 073 predecessor commit):

| Tool                     | What it does                                                             |
| ------------------------ | ------------------------------------------------------------------------ |
| `global_kill`            | Deletes an IRIS global (already token-gated via `global_preview`)        |
| `iris_admin`             | Wraps admin operations including process kill, cache clear, log truncate |
| `iris_credential_manage` | Creates, updates, or deletes Ensemble credentials                        |
| `iris_lookup_manage`     | Writes or deletes Ensemble lookup table entries                          |
| `iris_namespace_create`  | Creates a new IRIS namespace                                             |
| `iris_remove_server`     | Removes a server from the iad-native registry                            |
| `skill_forget`           | Removes a skill from the registry                                        |

## Error behavior

Blocked calls return the same JSON envelope as `write_tools_enabled` blocks:

```json
{
  "error_code": "DESTRUCTIVE_TOOLS_DISABLED",
  "message": "global_kill is a destructive tool. Set destructive_tools_enabled = true in .iris-agentic-dev.toml to enable it.",
  "tool": "global_kill"
}
```

## Backward compatibility

`write_tools_enabled = false` continues to block everything (including destructive tools),
because destructive tools are a subset of writes. No config migration needed for existing
users — they already have both tiers off.

Users who currently have `write_tools_enabled = true` get destructive tools blocked after
this change ships. This is a breaking change: document it in the release notes and
changelog. Affected users add `destructive_tools_enabled = true` to restore the old behavior.

## Implementation notes

- The gate lives in `crates/iris-agentic-dev-core/src/policy/gate.rs` — the same dispatch
  function that handles `write_tools_enabled` checks. Add a `destructive_set` of the 7 tool
  names; check it before the write gate.
- The `IrisConfig` struct (or equivalent) gains a `destructive_tools_enabled: bool` field,
  default `false`.
- Unit tests: cover all 7 destructive tools returning `DESTRUCTIVE_TOOLS_DISABLED` when the
  gate is off; cover them succeeding when the gate is on (mocked at the gate layer, not at
  IRIS).
- Integration test: use live IRIS, verify `global_kill` with gate off returns the error code.

## Out of scope

- UI for toggling the gate from inside a conversation (use config file)
- Per-tool destructive gates (the binary split covers the main need)
- Audit logging of destructive calls (future work, not gated on this)
