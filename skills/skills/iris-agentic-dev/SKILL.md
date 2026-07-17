---
name: iris-agentic-dev
description: Configure, connect, and troubleshoot the iris-agentic-dev MCP server (iris_execute/iris_query/iris_doc/docs_introspect/kb/etc.) against an IRIS container or instance. Use when its tools return IRIS_UNREACHABLE, when pointing it at a new IRIS, or when a connection edit "isn't taking effect." Covers the .iris-agentic-dev.toml config, live file-watch reload, the OrbStack port-discovery gotcha, and the docker-exec fallback.
author: tdyar
managed_by: iris-agentic-dev
---

# iris-agentic-dev MCP

`iris-agentic-dev` is a Rust binary that runs an MCP server (`iris-agentic-dev mcp`)
exposing IRIS dev tools — `iris_execute`, `iris_query`, `iris_doc`, `docs_introspect`,
`iris_compile`, `kb`, `iris_global`, etc. Registered per-project in `~/.claude.json`
under `mcpServers`, each with an `env: { IRIS_CONTAINER: "<name>" }`. It talks to IRIS
over the **Atelier REST API** (needs the web server / web gateway — port 52773-family).

## The two facts that trip people up

1. **Connection config lives in `.iris-agentic-dev.toml`, NOT the MCP `env` block.**
   Editing `env` in `~/.claude.json` on a running server does nothing — env is injected
   once at spawn. The `.toml` is the right lever.

2. **The server re-reads `.iris-agentic-dev.toml` on every tool call** (mtime check via
   `ConfigWatcher`). A config edit takes effect the next time any MCP tool is called —
   **no restart, no reconnect needed**. Do not tell the user "restart to apply."

## `IRIS_UNREACHABLE` — cause and fix

```text
IRIS_UNREACHABLE: no IRIS connection. Set IRIS_HOST and IRIS_WEB_PORT env vars,
or ensure IRIS is reachable on a discoverable port (52773, 41773, 51773, 8080).
```

**Common cause with OrbStack/Docker:** `container = "<name>"` triggers port-discovery
against a **fixed probe list (52773 / 41773 / 51773 / 8080)**. OrbStack maps the
container's 52773 to a *different, dynamic* host port (e.g. 42773) — not on the list
→ `IRIS_UNREACHABLE`, even though the container is healthy.

**Fix — add `host` + `web_port` to the project `.iris-agentic-dev.toml`:**

```toml
container = "my-iris"
host      = "localhost"
web_port  = 42773        # host-side mapped port for container's 52773
namespace = "USER"
username  = "_SYSTEM"
password  = "SYS"
```

Find the mapped port:

```bash
docker port <container-name> | grep 52773
# -> 52773/tcp -> 0.0.0.0:42773
```

⚠ **OrbStack host ports are dynamic** — if the container is recreated the port may
change. If the MCP breaks after a recreate, re-run `docker port` and update `web_port`.
A durable fix is pinning the port in compose/run config.

After editing `.toml`, call any MCP tool — it reconnects immediately.

## `.iris-agentic-dev.toml` key reference

Generate a documented sample: `iris-agentic-dev init`

| Key                     | Notes                                                                                   |
| ----------------------- | --------------------------------------------------------------------------------------- |
| `container`             | Docker container name — enables auto port-discovery (probe list above)                  |
| `host`                  | e.g. `"localhost"` — overrides discovery; use with `web_port`                           |
| `web_port`              | Atelier REST port (host-side). Community default 52773; Enterprise + web gateway varies |
| `web_prefix`            | URL path prefix (e.g. `"iris"` when Atelier is at `/iris/api/atelier/`)                 |
| `scheme`                | `"http"` (default) or `"https"`                                                         |
| `namespace`             | Default namespace for tool calls                                                        |
| `username` / `password` | Prefer `IRIS_USERNAME` / `IRIS_PASSWORD` env vars over committing credentials           |

Same names as CLI flags: `--host` (`IRIS_HOST`), `--web-port` (`IRIS_WEB_PORT`),
`--namespace`, `--username`, `--password`, `--toolset` (`baseline`/`nostub`/`merged`).

## Atelier REST requirement

Works with:

- Community Edition images (PWS on 52773)
- Enterprise + ISC Web Gateway container (auto-detected)

**Not supported**: Enterprise standalone (`intersystems/iris`, no web gateway) — no
Atelier REST endpoint.

## Fallback: `docker exec` when MCP is unavailable

```bash
cat > /tmp/script.txt <<'EOF'
zn "USER"
write ##class(%SYSTEM.Version).GetVersion(),!
halt
EOF
docker exec -i <container> iris session IRIS < /tmp/script.txt
```

**Parser gotchas** (avoid when building scripts):

- `$listnext(...)` inside a `while` condition confuses the terminal parser — use `for` loops instead
- `printf`-style `%`-escaping breaks multi-line scripts — use a heredoc file, not `printf`

## Toolset flag

`--toolset merged` (default) exposes the full tool set including interop and container tools.
`--toolset baseline` — standard ObjectScript dev tools only.
`--toolset nostub` — excludes preview/stub tools.
