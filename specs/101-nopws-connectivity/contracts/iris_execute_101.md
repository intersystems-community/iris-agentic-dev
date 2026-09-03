# Contract: iris_execute Response Shape (101-nopws-connectivity)

## Summary of change

Every `iris_execute` response gains the `execution_path` field. No existing fields are
removed or renamed — this is an additive change (Constitution V compliant).

---

## Success response shapes

### Atelier REST path (unchanged behavior, new field)

```json
{
  "success": true,
  "output": "IRIS for UNIX 2026.2...\n",
  "namespace": "USER",
  "method": "http",
  "execution_path": "atelier",
  "auth_user": "_SYSTEM",
  "service_account_env": ""
}
```

### Docker exec local path

```json
{
  "success": true,
  "output": "IRIS for UNIX 2026.2...\n",
  "namespace": "USER",
  "method": "docker",
  "execution_path": "docker_exec_local"
}
```

### Docker exec SSH path

```json
{
  "success": true,
  "output": "IRIS for UNIX 2026.2...\n",
  "namespace": "USER",
  "method": "docker",
  "execution_path": "docker_exec_ssh"
}
```

---

## Error response shapes

### NoPWS: no container configured

```json
{
  "success": false,
  "error_code": "NOPWS_NO_CONTAINER",
  "error": "docker_only or nopws=true requires a container name. Set IRIS_CONTAINER env var or add container = \"<name>\" to .iris-agentic-dev.toml."
}
```

### SSH exec failed

```json
{
  "success": false,
  "error_code": "SSH_EXEC_FAILED",
  "error": "ssh baystate.example.com docker exec failed: <stderr>. Verify SSH connectivity and that Docker is running on the remote host.",
  "execution_path": "docker_exec_ssh"
}
```

### HTTP failed, no docker fallback

```json
{
  "success": false,
  "error_code": "HTTP_EXECUTION_FAILED",
  "error": "iris_execute: HTTP/Atelier execution failed (<cause>). No docker fallback available (IRIS_CONTAINER not set). Verify the Atelier REST endpoint is reachable and credentials have %Service_Object:USE.",
  "http_error": "<cause>"
}
```

---

## Routing decision flowchart

```
Call iris_execute
  └─ Read (docker_only, no_pws, ssh_host) from ConnectionState
       ├─ docker_only || no_pws?
       │    YES → early branch
       │         ├─ ssh_host set? → ssh ... docker exec → execution_path = "docker_exec_ssh"
       │         └─ else        → docker exec          → execution_path = "docker_exec_local"
       └─ NO → try HTTP (execute_via_generator)
                ├─ success → execution_path = "atelier"
                └─ failure → docker exec fallback
                              ├─ ssh_host → execution_path = "docker_exec_ssh"
                              └─ else     → execution_path = "docker_exec_local"
```

---

## Backward compatibility

`method` field is **kept** with existing values `"http"` and `"docker"`. The `method`
field predates this spec and callers may already depend on it. The new `execution_path`
field provides richer information (`"atelier"` vs `"http"` clarifies the HTTP path is
specifically Atelier REST; `"docker_exec_local"` vs `"docker_exec_ssh"` distinguishes
local from remote).

Callers that currently switch on `method` see no change. Callers that want to know
whether SSH was used should switch on `execution_path`.

---

## iris_test_server response additions

```json
{
  "name": "my-server",
  "reachable": false,
  "web_available": false,
  "nopws": true,
  "nopws_detected": false,
  "nopws_evidence": null,
  "suggestion": "nopws = true\ndocker_only = true\ncontainer = \"my-iris-ai\"",
  "latency_ms": 3001,
  "message": "NoPWS build: this IRIS instance has no embedded web server. Set docker_only = true to use docker exec for execution tools, or configure a webgateway sidecar for Atelier REST access. See skills/nopws-setup.md."
}
```

When `nopws_detected = true` from auto-detection and `nopws` config is false:

```json
{
  "name": "my-server",
  "reachable": false,
  "web_available": false,
  "nopws": false,
  "nopws_detected": true,
  "nopws_evidence": "WebServer=0",
  "suggestion": "# Add to .iris-agentic-dev.toml:\nnopws = true\ndocker_only = true",
  "latency_ms": 42
}
```
