# NoPWS Setup

**NoPWS** (No Private Web Server) applies to AI branch IRIS builds: `irishealth-ai:*`,
`iris-ai:*`, and IRIS 2026.3+ Enterprise AI editions. These images ship without the
embedded Apache web server (`WebServer=0` in `iris.cpf`), so Atelier REST is unavailable
and the standard `iris_execute` → HTTP path fails with "connection refused".

## How to detect NoPWS

Check the container's `iris.cpf`:

```bash
docker exec <container> sh -c \
  "grep -i WebServer /usr/irissys/iris.cpf 2>/dev/null || \
   grep -i WebServer /usr/local/etc/irissys/iris.cpf 2>/dev/null"
```

Output `WebServer=0` → NoPWS active. If no output, path may differ; try both locations.

`iris_test_server` also reports `nopws: true` and `nopws_detected` when it detects this
condition via the sentinel URL or version string.

## Configuration

Set these in `.iris-agentic-dev.toml` in your workspace root:

```toml
# Required: tell iad to skip Atelier REST and route through docker exec
docker_only = true
nopws = true

# Required: container to exec into
container = "my-iris-ai-container"

# Optional: if the container is on a remote host, add ssh routing
# ssh_host = "myserver.internal"
```

`nopws = true` is semantically equivalent to `docker_only = true` for routing purposes.
Setting either one activates the docker exec path. Setting `nopws = true` is preferred
when the reason is a NoPWS build (documents intent).

## Tools that work in NoPWS mode

| Tool                  | Works? | Note                                |
| --------------------- | ------ | ----------------------------------- |
| `iris_execute`        | Yes    | Routes through `docker exec`        |
| `iris_compile`        | Yes    | Routes through `docker exec`        |
| `iris_test_server`    | Yes    | Reports NoPWS fields                |
| `iris_doc`            | No     | Requires Atelier REST               |
| `iris_source_control` | No     | Requires Atelier REST               |
| `iris_doc_search`     | Yes    | External Documatic — no IRIS needed |
| `iris_query`          | No     | Requires Atelier REST               |

## First-boot password

Fresh AI branch containers ship with a default `_SYSTEM` password that must be changed
before any connection works. Use the Management Portal or run:

```bash
docker exec <container> iris session IRIS -U %SYS \
  "Do ##class(Security.Users).ChangePassword(\"_SYSTEM\",\"SYS\",\"SYS\")"
```

This sets the password to `SYS`. Then set `IRIS_PASSWORD=SYS` or add `password = "SYS"`
to your `.iris-agentic-dev.toml`.

## Remote containers (SSH routing)

When the container runs on a remote host, add `ssh_host`:

```toml
docker_only = true
nopws = true
container = "iris-ai-prod"
ssh_host = "prodhost.internal"
```

`iris_execute` and `iris_compile` will prefix every `docker exec` call with
`ssh -o StrictHostKeyChecking=no <ssh_host>`. The SSH user must have Docker socket
access on the remote host.

**Security note:** `StrictHostKeyChecking=no` is required for non-interactive use.
Ensure you trust the remote host before setting `ssh_host`.

## Webgateway sidecar (optional)

To restore full Atelier REST access alongside NoPWS, add a webgateway sidecar:

```yaml
# docker-compose excerpt
webgateway:
  image: containers.intersystems.com/intersystems/webgateway:latest
  ports:
    - "8080:80"
  environment:
    - CSP_CONF_FILE=/webgateway.conf
  volumes:
    - ./webgateway.conf:/webgateway.conf:ro
```

Point `host` and `port` in `.iris-agentic-dev.toml` at the webgateway port (8080 above).
Remove `docker_only` and `nopws` once the gateway is reachable.

## Execution path field

Every `iris_execute` and `iris_compile` response now includes `execution_path`:

- `"atelier"` — normal HTTP/Atelier REST path
- `"docker_exec_local"` — docker exec on local host
- `"docker_exec_ssh"` — docker exec via SSH tunnel

Use this field to confirm which path was taken when diagnosing connectivity issues.
