---
author: tdyar
description: Step-by-step guide for setting up .iris-agentic-dev.toml — diagnose the
  connection problem first, then generate the right config block for the user's scenario.
iris_version: '>=2022.1'
license: MIT
metadata:
  version: 1.0.0
name: tdyar/iad-config-setup
state: reviewed
tags:
- iris-agentic-dev
- config
- setup
- connection
- toml
---

# IAD Config Setup — .iris-agentic-dev.toml

**Always run `check_config` first.** Never guess — read what the tool reports.

## Workflow

### Step 1 — Diagnose

```
check_config → look at: connected, connection_source, config_watch_path, server_manager
```

| What you see | What it means |
|---|---|
| `connected: true, connection_source: "env_var"` | Already connected via env vars — config optional |
| `connected: true, connection_source: "docker"` | Already connected via Docker — config optional |
| `connected: true, connection_source: "server_manager"` | VS Code Server Manager is working — no config needed |
| `connected: false` | No connection found — config required |
| `config_parse_error` present | Existing config has a syntax error — fix it |
| `server_manager.available: true` | VS Code Server Manager detected — use `IRIS_SERVER_NAME` instead of manual config |

### Step 2 — Choose Config Scenario

Ask the user: **How are you running IRIS?**

| Scenario | Config pattern |
|---|---|
| Docker container (named) | `container = "my-iris"` |
| Direct host:port | `host = "hostname"` + `web_port = 52773` |
| VS Code Server Manager | No config — set `IRIS_SERVER_NAME=<name>` env var |
| HTTPS enterprise gateway | `scheme = "https"` + `web_prefix = "irisaicore"` (if behind reverse proxy) |
| Community IRIS (no web server) | `container = "my-iris"` + `docker_only = true` |

### Step 3 — Generate Config

Write to the path shown in `config_watch_path` from `check_config`. Hot-reload is automatic.

---

## Config Reference

### Minimal: Named Docker container

```toml
container = "my-iris"
namespace = "USER"
```

Hot-reload fires automatically when you save. No restart needed.

### Direct host connection

```toml
host = "iris.example.com"
web_port = 52773
namespace = "PROD"
username = "_SYSTEM"
password = "SYS"
```

### HTTPS with path prefix (enterprise web gateway)

```toml
host = "iris.corp.example.com"
web_port = 443
scheme = "https"
web_prefix = "irisaicore"
namespace = "APP"
username = "apiuser"
password = "secret"
```

The Atelier API must be reachable at `https://host:443/irisaicore/api/atelier/`.

### Docker-only (no web server)

```toml
container = "iris-community"
namespace = "USER"
docker_only = true
```

All tool calls use `docker exec` instead of HTTP. No web port needed.

### Fleet / Operate mode (multi-instance)

```toml
mode = "operate"

[instance.prod]
host = "prod.example.com"
web_port = 52773
namespace = "PROD"
username = "svc"
password = "secret"
role = "subject"

[instance.dev]
container = "dev-iris"
namespace = "DEV"
role = "workspace"
```

Roles: `workspace` (write allowed), `subject` (read-only by default), `control-plane`.

### Per-connection policy (Server Manager connections)

```toml
[policy.prod]
allow = ["query", "search", "docs"]
```

Blocks `compile`, `execute`, `source_control`, `debug`, `admin`, `skill`, `kb` on the `prod` Server Manager server.
Omit `allow` entirely to permit everything (or omit the `[policy.*]` block).

Tool categories: `compile`, `execute`, `query`, `search`, `docs`, `source_control`, `debug`, `admin`, `skill`, `kb`

---

## Env Vars (Alternative to Config File)

| Var | Default | Purpose |
|---|---|---|
| `IRIS_HOST` | — | Host for direct connection |
| `IRIS_WEB_PORT` | `52773` | Web port |
| `IRIS_NAMESPACE` | `USER` | Default namespace |
| `IRIS_USERNAME` | `_SYSTEM` | Username |
| `IRIS_PASSWORD` | `SYS` | Password |
| `IRIS_CONTAINER` | — | Docker container name |
| `IRIS_SERVER_NAME` | — | Server Manager server to use when multiple are configured |
| `OBJECTSCRIPT_WORKSPACE` | `$PWD` | Override workspace root (where to look for config file) |

Priority: config file > env vars > auto-discovery.

---

## Troubleshooting

**`config_parse_error` in check_config output**

TOML syntax error. Common causes:
- Missing quotes around string values: `host = iris.com` → `host = "iris.com"`
- Wrong bracket style: `[instance.prod.role]` → `role = "subject"` inside `[instance.prod]`

**Connected via wrong source (e.g. picking up community IRIS instead of target)**

Set `IRIS_CONTAINER` or write a config file with explicit `host`/`container`.

**Server Manager `credential_status: "not_configured"`**

Credential not in OS keychain. In VS Code → Server Manager → right-click server → Reconnect.
After reconnecting, call `check_config` again to verify `credential_status: "resolved"`.

**Multiple Server Manager servers, no `IRIS_SERVER_NAME` set**

`check_config` will show `server_manager.available: true` with multiple servers.
Set `IRIS_SERVER_NAME=<name>` (the map key from `intersystems.servers`) to select one.

**`docker_only = true` needed when**

- Using IRIS Community Edition without web server
- Container has no port 52773 mapped
- Only docker exec is available (e.g. remote dev container)
