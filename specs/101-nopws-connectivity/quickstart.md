# Quickstart: 101-nopws-connectivity

## Scenario A: local NoPWS container, fully offline

You have an AI-branch IRIS container (`irishealth-ai:2026.3` or similar) with no
embedded web server. You want execution tools to work without setting up a webgateway
sidecar.

```toml
# .iris-agentic-dev.toml
container   = "my-iris-ai"
namespace   = "USER"
nopws       = true
docker_only = true
```

Verify with:

```
iris_test_server name="my-iris-ai"
```

Expected response includes `nopws: true`, `web_available: false`, and a message
explaining the NoPWS setup — no raw "connection refused".

Call `iris_execute`:

```json
{ "code": "Write $ZVERSION,!" }
```

Response includes `execution_path: "docker_exec_local"`.

---

## Scenario B: webgateway sidecar provides Atelier REST

You want full iad functionality (iris_doc, iris_source_control) and are willing to
run a webgateway sidecar container.

```toml
# .iris-agentic-dev.toml
container  = "my-iris-ai"
host       = "localhost"
web_port   = 52773
namespace  = "USER"
nopws      = true
# docker_only NOT set — HTTP is usable via the sidecar
```

The `nopws = true` flag suppresses raw "connection refused" if the sidecar is not yet
running, replacing it with a helpful message. Once the sidecar is up, Atelier REST is
used normally and `execution_path` returns `"atelier"`.

---

## Scenario C: remote container via SSH

You manage an AI-branch container on `baystate.example.com`. Your SSH keys are
pre-configured.

```toml
# .iris-agentic-dev.toml
container   = "irishealth-ai"
namespace   = "USER"
nopws       = true
docker_only = true
ssh_host    = "baystate.example.com"
```

`iris_execute` routes as:
`ssh -o StrictHostKeyChecking=no baystate.example.com docker exec -i irishealth-ai iris session IRIS -U USER`

Response includes `execution_path: "docker_exec_ssh"`.

---

## Scenario D: auto-detection

You do not know whether your container is NoPWS. You have `container` set but no
`nopws` flag.

```toml
container = "mystery-iris"
namespace = "USER"
```

Call `iris_test_server name="mystery-iris"`. If the web probe fails and Docker is local,
iad reads `iris.cpf` from the container and checks for `WebServer=0`. If found:

```json
{
  "nopws_detected": true,
  "nopws_evidence": "WebServer=0",
  "suggestion": "nopws = true\ndocker_only = true\ncontainer = \"mystery-iris\""
}
```

Paste the `suggestion` value into your `.iris-agentic-dev.toml` and retry.

---

## Scenario E: first-boot password clearing

A fresh AI-branch container may have a forced password change on first login. If docker
exec returns an IRIS login prompt instead of code output, clear it with:

```objectscript
Do $System.Security.ChangePassword("_SYSTEM","SYS","SYS")
```

This resets the `_SYSTEM` password to the same value, clearing the forced-change flag.
Only required once per fresh container.

---

## What each tool needs

| Tool                  | NoPWS local          | NoPWS + SSH    | Webgateway sidecar |
| --------------------- | -------------------- | -------------- | ------------------ |
| `iris_execute`        | `docker_only = true` | `ssh_host` set | Optional (Atelier) |
| `iris_compile`        | `docker_only = true` | `ssh_host` set | Optional (Atelier) |
| `iris_doc` get/put    | Not supported        | Not supported  | Required           |
| `iris_source_control` | Not supported        | Not supported  | Required           |
| `iris_query`          | Not supported        | Not supported  | Required           |
| `iris_test_server`    | Always works         | Always works   | Always works       |

`iris_execute` and `iris_compile` work in all NoPWS modes. Tools that require Atelier
REST (`iris_doc`, `iris_source_control`, `iris_query`) need a webgateway sidecar.
