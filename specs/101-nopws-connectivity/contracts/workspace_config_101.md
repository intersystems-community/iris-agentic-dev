# Contract: WorkspaceConfig TOML Schema (101-nopws-connectivity)

## New fields

These two fields are added to the top-level `.iris-agentic-dev.toml` schema.

### `nopws`

- **Type**: `bool`
- **Default**: `false`
- **Serde**: `#[serde(default)]`
- **TOML key**: `nopws`
- **Purpose**: Declare that the configured IRIS instance has no embedded web server
  (AI-branch builds, 2026.3+). Suppresses raw "connection refused" errors on the
  Atelier REST probe and enables the docker exec route for all execution tools.

```toml
nopws = true
```

When `nopws = true` and the web port is unreachable, `iris_test_server` returns a
structured message with remediation steps instead of a raw connection error.

When `nopws = true` and `docker_only = true`, all execution bypasses HTTP entirely.

When `nopws = true` but the web port IS reachable (webgateway sidecar in use),
Atelier REST is used normally — `nopws` only suppresses the error path, not successful
connections.

### `ssh_host`

- **Type**: `Option<String>`
- **Default**: `null` (absent)
- **TOML key**: `ssh_host`
- **Purpose**: Route docker exec commands through SSH to a remote Docker host.
  When set, every `docker exec` call is prefixed with
  `ssh -o StrictHostKeyChecking=no <ssh_host>`.
  Requires `container` to be set in the same config block.

```toml
ssh_host = "baystate.example.com"
container = "irishealth-ai"
nopws = true
docker_only = true
```

`StrictHostKeyChecking=no` is required because the MCP subprocess is non-interactive
and cannot respond to host-key prompts. SSH key authentication must be pre-configured
in the user's SSH config or ssh-agent.

---

## Complete NoPWS example: local docker exec

```toml
# irishealth-ai or iris-ai with no embedded web server
container = "my-iris-ai"
namespace = "USER"
nopws     = true
docker_only = true
```

## Complete NoPWS example: webgateway sidecar

```toml
# Webgateway sidecar provides Atelier REST on port 52773
container  = "my-iris-ai"
host       = "localhost"
web_port   = 52773
namespace  = "USER"
nopws      = true
# docker_only stays false — HTTP is usable via the sidecar
```

## Complete NoPWS example: remote container via SSH

```toml
container  = "my-iris-ai"
namespace  = "USER"
nopws      = true
docker_only = true
ssh_host   = "baystate.example.com"
```

---

## Interaction with existing fields

| Field                | Interaction with `nopws`                                                                             |
| -------------------- | ---------------------------------------------------------------------------------------------------- |
| `docker_only = true` | Redundant when `nopws = true` without webgateway — both skip HTTP. Recommended together for clarity. |
| `host` / `web_port`  | Used when webgateway sidecar is in place. `nopws = true` suppresses error only when probe fails.     |
| `container`          | Required for docker exec. When absent with `nopws = true`, tools return `NOPWS_NO_CONTAINER`.        |
| `ssh_host`           | Requires `container`. Without `container`, returns clear error at startup or first call.             |

---

## Serde silent-drop prevention

Both fields MUST be tested via `toml::from_str` round-trip (FR-011), not struct literal
construction. The test in `workspace_config.rs` must look like:

```rust
let cfg: WorkspaceConfig = toml::from_str(
    "nopws = true\nssh_host = \"baystate\"\ncontainer = \"my-iris\""
).expect("must parse");
assert!(cfg.nopws);
assert_eq!(cfg.ssh_host.as_deref(), Some("baystate"));
```

This catches a serde field renaming or missing `#[serde(default)]` that a struct
literal test would miss.
