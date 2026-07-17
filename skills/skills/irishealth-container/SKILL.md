---
name: irishealth-container
description: >
  Use when working with irishealth-community (FHIR R4) or irishealth AI Hub
  containers, OR when setting up VSCode/iris-dev extension against any IRIS
  container. Covers IRISContainer.health() and ai_hub() factory methods,
  four hard-won gotchas, the two-container architecture, no-ZPM FHIR setup,
  and the critical enterprise-vs-community web server split that affects
  Atelier REST and VSCode ObjectScript extension connectivity.
tags: [iris, irishealth, fhir, ai-hub, docker, container, devtester, vscode, atelier]
---

# irishealth Container Editions

---

## CRITICAL: Enterprise Images Have No Private Web Server — Use the Webgateway Container

| Image | `WebServer` in iris.cpf | Port 52773 direct | Atelier REST | Solution |
|---|---|---|---|---|
| `iris:2026.1` (enterprise) | `0` — disabled | ❌ | ✅ via webgateway | See below |
| `iris-community:2026.1` | `1` — enabled | ✅ | ✅ | Direct port 52773 |
| `irishealth-community` | `1` — enabled | ✅ | ✅ | Direct port 52773 |
| `irishealth:2026.2.0AI.*` | `0` — disabled | ❌ | ✅ via webgateway | See below |

Enterprise images have `WebServer=0` — the httpd binary and CSP.ini are not installed. However, **the `intersystems/webgateway` sidecar container DOES work** for Atelier REST. The webgateway uses `CSP On` (not `SetHandler`) to route all requests through the CSP module to IRIS via the superserver, and IRIS routes `/api/atelier/` internally.

**Three bugs that cause 404/403/500 during setup** (verified 2026-05-03 — see `iris-vscode-objectscript` skill for full detail):
1. CSP.ini race condition — patch after `Configuration_Initialized` appears, not immediately
2. Missing credentials in `[LOCAL]` — add `Username=_SYSTEM` and `Password=SYS` to CSP.ini `[LOCAL]` section; default tries CSPSystem which doesn't exist
3. Wrong Apache directive — use `CSP On` inside `<Location />`, NOT `SetHandler csp-handler-sa`

**Also required after first start:** `Do ##class(Security.Users).UnExpireUserPasswords("*")` in `%SYS` — fresh enterprise containers force a password change that blocks the webgateway connection.

Load the `iris-vscode-objectscript` skill for the complete working `webgateway-init.sh` and docker-compose.

---

## VSCode ObjectScript Extension — Server Configuration

The `intersystems.objectscript` extension connects via Atelier REST on port 52773 (the IRIS private web server), **not** the superserver port 1972.

```jsonc
// ~/Library/Application Support/Code/User/settings.json (macOS)
// %APPDATA%\Code\User\settings.json (Windows)
{
  "intersystems.servers": {
    "my-iris-dev": {
      "webServer": {
        "host": "localhost",
        "port": 52773,       // ← IRIS private web server, NOT superserver
        "pathPrefix": ""     // empty for default, "/iris" if behind a proxy
      },
      "username": "_SYSTEM",
      "description": "iris-community:2026.1 dev container"
    }
  },
  "objectscript.conn": {
    "server": "my-iris-dev",
    "ns": "USER",
    "active": true
  }
}
```

If your container maps 52773 to a different host port (e.g. `"64773:52773"`), use the host port in settings:

```jsonc
"webServer": {
  "host": "localhost",
  "port": 64773    // ← whatever the docker -p maps 52773 to on the host
}
```

**Verify the connection works before opening VSCode:**

```bash
curl -s -u "_SYSTEM:SYS" "http://localhost:64773/api/atelier/" | python3 -c \
  "import sys,json; d=json.load(sys.stdin); print(d['result']['content']['version'])"
# → IRIS 2026.1 (or similar) confirms Atelier REST is working
```

---

## Two Editions, Two Jobs

| | `IRISContainer.health()` | `IRISContainer.ai_hub()` |
|---|---|---|
| Image | `intersystemsdc/irishealth-community:latest` | `docker.iscinternal.com/.../irishealth:2026.2.0AI.159.0` |
| Port 1972 | ✅ SuperServer | ✅ SuperServer |
| Port 52773 | ✅ FHIR HTTP / web portal | ❌ WebServer=0 |
| FHIR R4 endpoint | ✅ pre-installed | ❌ (class exists, no HTTP) |
| `%AI.Agent`, `VECTOR` | ❌ | ✅ |
| License key | No | No |
| Registry | docker.io | docker.iscinternal.com |

---

## Factory Methods

```python
from iris_devtester import IRISContainer

# FHIR R4 — Foundation.Install + InstallInstance baked at build time
# No ZPM, no network. Ready in ~90 seconds.
with IRISContainer.health() as iris:
    h = iris.fhir_health_check()
    print(h.report())
    # Endpoint: http://localhost:52773/csp/healthshare/demo/fhir/r4
    # FHIR: R4 v4.0.1 — 148 resource types

# AI Hub — %AI.Agent, %AI.MCP.Service, VECTOR SQL, SuperServer only
# Requires docker.iscinternal.com registry access
with IRISContainer.ai_hub(build="159") as iris:
    conn = iris.get_connection()   # port 1972 only
    # iris.cls("%AI.Agent") accessible
```

---

## FHIR Setup (No ZPM, No Network)

The `health()` container has this baked at build time:

```objectscript
set $NAMESPACE = "HSLIB"
do:'##class(%SYS.Namespace).Exists("demo") ##class(HS.Util.Installer.Foundation).Install("demo")
set $NAMESPACE = "demo"
set appKey = "/csp/healthshare/demo/fhir/r4"
set strategyClass = "HS.FHIRServer.Storage.Json.InteractionsStrategy"
set metadataPackages = $LISTBUILD("hl7.fhir.r4.core@4.0.1","hl7.fhir.us.core@3.1.0")
do ##class(HS.FHIRServer.Installer).InstallInstance(appKey, strategyClass, metadataPackages)
```

~30 seconds, zero network calls.

---

## 4 Hard-Won Gotchas

### Gotcha 1: `/durable` Volume Ownership

Named Docker volumes mount as `root`. `irisowner` (uid 51773) cannot write.
`ai_hub()` defaults to `tmpfs:/durable`. For persistence, pass `durable_path`:

```python
iris = IRISContainer.ai_hub(durable_path="/host/path/durable")
# Host path must be pre-chowned: chown 51773:51773 /host/path/durable
```

docker-compose init-container pattern (from grongierisc template):
```yaml
services:
  init-permissions:
    image: alpine:latest
    volumes: [iris-data:/dur]
    command: sh -c "chown -R 51773:51773 /dur"
  iris:
    depends_on:
      init-permissions:
        condition: service_completed_successfully
volumes:
  iris-data:
```

### Gotcha 2: Double-Start Bug

The `-a` hook in `/iris-main` runs **after** IRIS is already started. Any startup script under `-a` that calls `iris start IRIS quietly` will fail with "database already running" and exit. Scripts under `-a` must assume IRIS is live — poll port 1972, do not call `iris start`.

### Gotcha 3: No Web Server in Enterprise Image

`irishealth:2026.2.0AI.*` has `WebServer=0` in `iris.cpf`. No `csp/bin/` directory. Port 52773 not available. FHIR HTTP endpoint not reachable. For both `%AI.*` and FHIR HTTP: use two-container split.

### Gotcha 4: `%AI.*` in Read-Only irislib

`%AI.Agent`, `%AI.MCP.Service`, `%AI.ToolSet`, `%AI.Policy.Authorization` live in `irislib` — a read-only database. Cannot export as UDL/XML. Cannot transplant to community image.

---

## Two-Container Split (docker-compose)

```yaml
services:
  fhir:
    image: intersystemsdc/irishealth-community:latest
    ports: ["1972:1972", "52773:52773"]
    # FHIR R4: http://localhost:52773/csp/healthshare/demo/fhir/r4

  ai_hub:
    image: docker.iscinternal.com/docker-intersystems/intersystems/irishealth:2026.2.0AI.159.0
    ports: ["11972:1972"]
    volumes:
      - type: tmpfs
        target: /durable
        tmpfs: {uid: 51773, gid: 51773}
    # %AI.Agent: connect to localhost:11972 via DBAPI
```

---

## FHIRContainerHealth

```python
from iris_devtester.containers.models import FHIRContainerHealth

h = iris.fhir_health_check()
h.fhir_version       # "4.0.1"
h.endpoint           # "http://localhost:52773/csp/healthshare/demo/fhir/r4"
h.resource_types_count  # 148
h.ready              # True if accessible and fhir_version set
h.report()           # human-readable summary
```
