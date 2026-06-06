---
name: tdyar/iris-webgateway-setup
description: Use when adding a Web Gateway container to an IRIS Docker deployment — especially enterprise images (no built-in web server) or sharded clusters where the router node needs REST/Atelier/MCP access. Covers CSP.ini, Apache CSP.conf, port conflict avoidance, and the IRIS-side %Service_WebGateway security fix that causes 403 Access Denied.
license: MIT
compatibility: docker, iris, objectscript
state: draft
iris_version: ">=2022.1"
tags: [iris, webgateway, docker, sharding, csp, atelier, rest]
---

# IRIS Web Gateway Setup (Docker)

## When to use this skill

- Enterprise IRIS image (no private web server — `docker_only = true` pattern)
- Sharded cluster where the router node needs REST/Atelier access
- `iris-agentic-dev` tools fail with "no web port available"
- Getting 403 from CSP gateway when you know IRIS is running

---

## How IRIS web access works in Docker

```
Client → WebGateway container (Apache + CSP module) → iris-router:1972 (superserver)
                                                       ↕
                                          IRIS handles HTTP request
```

Enterprise IRIS images (e.g. `iris-ngpu:*`) have **no built-in HTTP server**. Port 52773 published from the container is dead — nothing is listening. You need a separate `webgateway` container that proxies via the superserver protocol.

---

## Three-file setup

### 1. `harness/webgateway/CSP.ini`

The gateway's server registry. The `LOCAL` entry points at the IRIS router.

```ini
[SYSTEM]
SM_TIMEOUT=300
SERVER_RESPONSE_TIMEOUT=300
QUEUED_REQUEST_TIMEOUT=300
No_Activity_Timeout=86400

[SYSTEM_INDEX]
LOCAL=Enabled

[LOCAL]
Ip_Address=iris-router          # Docker service name of the IRIS router
TCP_Port=1972                   # Superserver port — NOT 52773
Minimum_Server_Connections=3
Maximum_Server_Connections=100
Connection_Security_Level=0     # 0=no encryption on gateway↔IRIS link (fine for internal Docker net)
Username=CSPSystem
Password=SYS                    # Must match what you set in IRIS (see IRIS-side config below)

[APP_PATH_INDEX]
/=Enabled
/api=Enabled
/csp=Enabled
/isc=Enabled

[APP_PATH:/]
Default_Server=LOCAL

[APP_PATH:/api]
Default_Server=LOCAL

[APP_PATH:/csp]
Default_Server=LOCAL

[APP_PATH:/isc]
Default_Server=LOCAL
```

### 2. `harness/webgateway/CSP.conf`

Mounted over the container's default `/etc/apache2/conf-enabled/CSP.conf`. Without this override, the default only routes `/csp/bin/Systems/` and `/csp/bin/RunTime/` — `/api/atelier/` returns 404.

```apacheconf
CSPModulePath "${ISC_PACKAGE_INSTALLDIR}/bin/"
CSPConfigPath "${ISC_PACKAGE_INSTALLDIR}/bin/"

<Location "/csp/bin/Systems/">
    SetHandler csp-handler-sa
</Location>
<Location "/csp/bin/RunTime/">
    SetHandler csp-handler-sa
</Location>

<Location "/api">
    SetHandler csp-handler-sa
</Location>
<Location "/csp">
    SetHandler csp-handler-sa
</Location>
<Location "/isc">
    SetHandler csp-handler-sa
</Location>
<Location "/">
    CSP On
    SetHandler csp-handler-sa
</Location>

<Directory "${ISC_PACKAGE_INSTALLDIR}/bin/">
    AllowOverride None
    Options None
    Require all granted
    <FilesMatch "\.(log|ini|pid|exe)$">
        Require all denied
    </FilesMatch>
</Directory>
```

### 3. `compose.webgateway.override.yaml`

```yaml
services:
  webgateway:
    image: containers.intersystems.com/intersystems/webgateway:latest-cd
    hostname: webgateway
    ports:
      - "${WEB_GATEWAY_PORT:-52774}:80"   # 52774, NOT 52773 — the router already publishes 52773
    volumes:
      - ./webgateway/CSP.ini:/opt/webgateway/bin/CSP.ini
      - ./webgateway/CSP.conf:/etc/apache2/conf-enabled/CSP.conf
    depends_on:
      iris-router:
        condition: service_healthy
    healthcheck:
      test: ["CMD-SHELL", "curl -so /dev/null -w '%{http_code}' http://localhost/api/atelier/ | grep -qE '^[1234]'"]
      interval: 10s
      timeout: 5s
      retries: 10
      start_period: 15s
```

**Port note**: Use 52774 (not 52773) because the router container already publishes 52773 in most compose setups. Docker Compose merges port lists — you can't un-publish a port via override — so use a different host port and configure `iris-agentic-dev` to use it.

---

## IRIS-side configuration (CRITICAL)

The gateway→IRIS connection is authenticated via `%Service_WebGateway`. Without this step, IRIS returns `403 Forbidden / Access Denied` even though TCP connects fine.

Run once after the router is healthy:

```objectscript
// In iris session IRIS -U %SYS (or via iris_execute in %SYS)

// 1. Enable unauthenticated + password on the WebGateway service
set sc=##class(Security.Services).Get("%Service_WebGateway",.svcProps)
set svcProps("AutheEnabled")=96  // 32 (Password) + 64 (Unauthenticated)
set sc=##class(Security.Services).Modify("%Service_WebGateway",.svcProps)
write "Service SC: ",sc,!   // 1 = success

// 2. Set the CSPSystem password to match CSP.ini Password= value
set sc=##class(Security.Users).Get("CSPSystem",.props)
set props("Password")="SYS"
set props("PasswordNeverExpires")=1
set sc=##class(Security.Users).Modify("CSPSystem",.props)
write "User SC: ",sc,!      // 1 = success
```

Or as a `.cos` file piped into `iris session`:

```bash
docker cp configure_webgateway.cos harness-iris-router-1:/tmp/
docker exec harness-iris-router-1 sh -c \
  'runuser -u irisowner -- iris session IRIS -U %SYS < /tmp/configure_webgateway.cos'
```

---

## iris-agentic-dev config after setup

Update `.iris-agentic-dev.toml` to use the webgateway:

```toml
docker_only = false
host = "localhost"
web_port = 52774       # matches WEB_GATEWAY_PORT in compose override
web_prefix = ""
scheme = "http"
username = "SuperUser"  # _SYSTEM may not work after reset_passwords.sh runs
password = "SYS"
```

Also set `IRIS_CONTAINER` in `.iris-agentic-dev.toml` (or the env) so that `iris_execute` can fall back to docker exec for long-running operations:

```toml
container = "harness-iris-router-1"   # exact docker container name
```

**Why `IRIS_CONTAINER` matters**: `iris_execute` uses HTTP to the webgateway by default. The HTTP path has a hard timeout (~30s). Long-running tests (`%UnitTest.Manager`, big queries, batch jobs) exceed this and fail with `DOCKER_REQUIRED`. With `container` set, `iris_execute` retries via `docker exec`, which has no HTTP timeout.

---

## Diagnostic sequence for 403 / "Server Unavailable"

| Symptom | Cause | Fix |
|---------|-------|-----|
| `curl: (52) Empty reply from server` | Apache running but `/api` not routed to CSP | Mount custom `CSP.conf` with `<Location "/api">` |
| `500 Server is currently unavailable` | CSP.ini loaded but gateway can't reach IRIS | Check `Ip_Address=` and `TCP_Port=` in `[LOCAL]` section |
| `403 Forbidden / Access Denied` in CSP.log | `%Service_WebGateway` auth mismatch | Run IRIS-side config (set `AutheEnabled=96`, set `CSPSystem` password) |
| `401 Unauthorized` | Gateway→IRIS connected; user auth needed | Correct — pass `-u SuperUser:SYS` in curl (not `_SYSTEM`) |
| Port already allocated on 52773 | Router already owns that host port | Use `WEB_GATEWAY_PORT=52774` in the webgateway compose service |

### Check CSP gateway log

```bash
docker logs harness-webgateway-1 2>&1 | tail -30
docker exec harness-webgateway-1 cat /opt/webgateway/logs/CSP.log | tail -20
```

### Verify IRIS-side service status

```bash
# Is %Service_WebGateway enabled?
echo "write ##class(Security.Services).Get(\"%Service_WebGateway\",.p) write p(\"Enabled\"),!,p(\"AutheEnabled\"),! halt" \
  | docker exec -i harness-iris-router-1 runuser -u irisowner -- iris session IRIS -U '%SYS'
# Expected: Yes / 96
```

---

## CRITICAL rules

- **CSP.conf must cover `/api`** — the container default only covers `/csp/bin/Systems/` and `/csp/bin/RunTime/`. Atelier and MCP use `/api/atelier/`, which 404s without the override.
- **`%Service_WebGateway` not `%Service_CSP`** — the relevant IRIS security service for the standalone webgateway container is `%Service_WebGateway`. `%Service_CSP` is for the embedded gateway (not present in enterprise images).
- **Port 52773 is already taken by the router** — Docker Compose merges ports lists, so you can't remove the router's 52773 binding via override. Use 52774 for the webgateway.
- **CSP.ini is bind-mounted** — edits to `CSP.ini` on the host take effect after `docker compose restart webgateway`. No rebuild needed.
- **IRIS-side config survives container restart** but NOT `down -v` (volume wipe). Add the IRIS-side steps to your cluster startup script.
- **`SuperUser` not `_SYSTEM` for Atelier REST** — after `reset_passwords.sh` runs on the cluster, `_SYSTEM` may no longer have a working password for REST. Use `SuperUser:SYS` (or whatever `IRIS_PASSWORD` is set to) as the REST credential. Configure in `.iris-agentic-dev.toml` as `username = "SuperUser"`.
- **Healthcheck must accept 401** — the webgateway healthcheck should accept any HTTP 1xx–4xx response as "healthy" (401 = gateway works, IRIS just wants auth). A check that requires 200 will always fail if REST needs credentials.
