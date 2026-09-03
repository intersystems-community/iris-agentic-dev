# Data Model: 101-nopws-connectivity

## WorkspaceConfig additions

Two new fields in `crates/iris-agentic-dev-core/src/iris/workspace_config.rs`:

```rust
/// When true, this IRIS instance has no embedded web server (AI-branch builds,
/// 2026.3+). Suppresses "connection refused" errors on the Atelier REST endpoint
/// and routes execution through docker exec. Set nopws = true + docker_only = true
/// for fully offline operation, or nopws = true alone when a webgateway sidecar
/// provides Atelier REST on the web port.
#[serde(default)]
pub nopws: bool,

/// SSH hostname for remote docker exec. When set, docker exec runs as:
///   ssh -o StrictHostKeyChecking=no <ssh_host> docker exec -i <container> ...
/// Requires `container` to be set. Uses the system SSH config for key auth and
/// ProxyJump chains — iad does not manage credentials.
pub ssh_host: Option<String>,
```

Both fields added to `WorkspaceConfig` immediately after `docker_only`.

### TOML representation

```toml
# NoPWS: true for AI-branch IRIS builds without embedded web server
nopws = true

# SSH host: route docker exec through SSH (for remote containers)
ssh_host = "baystate.example.com"
```

---

## IrisConnection additions

Field added to `IrisConnection` in `crates/iris-agentic-dev-core/src/iris/connection.rs`:

```rust
/// SSH hostname for remote docker exec routing (FR-009).
/// Populated from WorkspaceConfig.ssh_host at connection build time.
pub ssh_host: Option<String>,
```

`workspace_config_to_connection()` propagates `cfg.ssh_host.clone()` into the
`IrisConnection` when building a docker exec connection.

---

## ExecutionPath enum (logical — serialized as string)

Three values, returned as the `execution_path` field in every `iris_execute` response:

| Value                 | Meaning                                                              |
| --------------------- | -------------------------------------------------------------------- |
| `"atelier"`           | HTTP/Atelier REST via `execute_via_generator` — default path         |
| `"docker_exec_local"` | `docker exec -i <container> iris session ...` on local Docker socket |
| `"docker_exec_ssh"`   | `ssh <host> docker exec -i <container> iris session ...`             |

---

## NoPWS detection result (embedded in iris_test_server response)

Fields added to the `iris_test_server` JSON response:

```json
{
  "nopws": false,
  "web_available": true,
  "nopws_detected": false,
  "nopws_evidence": null,
  "suggestion": null,
  "unreachable": false
}
```

| Field            | Type             | Description                                                                                                     |
| ---------------- | ---------------- | --------------------------------------------------------------------------------------------------------------- |
| `nopws`          | `bool`           | Value from WorkspaceConfig (`nopws = true` in toml)                                                             |
| `web_available`  | `bool`           | Whether the Atelier REST endpoint responded successfully                                                        |
| `nopws_detected` | `bool`           | Auto-detection result (iris.cpf or superserver probe)                                                           |
| `nopws_evidence` | `Option<String>` | Quoted evidence line from iris.cpf, e.g. `"WebServer=0"`                                                        |
| `suggestion`     | `Option<String>` | Ready-to-paste TOML snippet when NoPWS confirmed                                                                |
| `unreachable`    | `bool`           | True when web probe failed and Docker was not available to probe iris.cpf — NoPWS cannot be confirmed or denied |

---

## iris_execute response additions

The `execution_path` field is added to all `iris_execute` response shapes:

```json
{
  "success": true,
  "output": "...",
  "namespace": "USER",
  "execution_path": "atelier",
  "method": "http"
}
```

`method` is kept for backward compatibility (`"http"` / `"docker"`). `execution_path`
provides finer-grained routing information (`"atelier"` / `"docker_exec_local"` /
`"docker_exec_ssh"`).

---

## Error codes introduced

| Code                     | Tool                              | Meaning                                                                                                                         |
| ------------------------ | --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `NOPWS_NO_CONTAINER`     | `iris_execute`, `iris_compile`    | `nopws = true` or docker_only detected but no container name available — `IRIS_CONTAINER` not set and `container` not in config |
| `SSH_EXEC_FAILED`        | `iris_execute`                    | `ssh_host` set but SSH command returned non-zero; includes stderr in error message                                              |
| `NOPWS_ATELIER_REQUIRED` | `iris_doc`, `iris_source_control` | Tool requires Atelier REST; not available on NoPWS without webgateway sidecar                                                   |

Standard error codes reused:

- `DOCKER_REQUIRED` — container needed but IRIS_CONTAINER not set
- `IRIS_UNREACHABLE` — no connection at all

---

## Routing decision logic (iris_execute)

```
1. Read (docker_only, no_pws, ssh_host) from locked ConnectionState
2. If docker_only OR no_pws:
   a. If ssh_host is set → execution_path = "docker_exec_ssh"
   b. Else → execution_path = "docker_exec_local"
   c. Return early via iris.execute() or iris.execute_ssh()
3. Else: try HTTP via execute_via_generator → execution_path = "atelier"
4. HTTP fails → fallback to docker exec → execution_path = "docker_exec_local"
   (or "docker_exec_ssh" if ssh_host set)
```

This mirrors the existing `iris_compile` early-branch pattern exactly.

---

## Combination rule: `no_pws` (runtime) vs `nopws` (config)

Two sources can independently trigger the docker exec route:

- `no_pws: bool` — set at runtime by `derive_capabilities()` when the version string
  contains `"2026.2.0AI"`. Lives in `ConnectionState`.
- `nopws: bool` — set in `.iris-agentic-dev.toml` by the operator. Lives in
  `WorkspaceConfig`. Propagated into `ConnectionState` at connection build time.

**Rule**: Either flag alone is sufficient. The early-branch condition in `iris_execute`
and `iris_compile` is:

```rust
if docker_only || no_pws {
    // routes to docker exec — nopws (config) is loaded into no_pws at connection init
```

The config flag (`nopws = true`) is loaded into `ConnectionState.no_pws` during
`workspace_config_to_connection()`, so a single `no_pws` check in the execution path
covers both sources. The config flag takes precedence in the sense that it is resolved
before any version probe; `derive_capabilities()` supplements it for instances where the
operator has not pre-configured `nopws = true`.
