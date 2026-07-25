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

## Task → skill map

Load the skill(s) for what you're about to do. You don't need them all — just the ones for your task.

| Task                              | Load skill                                        |
| --------------------------------- | ------------------------------------------------- |
| Write or review ObjectScript      | `objectscript-review` + `objectscript-guardrails` |
| Fix a bug or runtime error        | `objectscript-debugging`                          |
| Navigate an unfamiliar codebase   | `objectscript-navigation`                         |
| Write or run %UnitTest tests      | `objectscript-unit-test`                          |
| TDD loop (write → compile → test) | `objectscript-tdd`                                |
| Measure test coverage             | `objectscript-coverage`                           |
| Write SQL or fix SQLCODE errors   | `objectscript-sql-patterns`                       |
| Work with `$LIST` / `$LISTBUILD`  | `objectscript-list-patterns`                      |
| Interoperability production       | `ensemble-production`                             |
| Python in IRIS                    | `iris-embedded-python` + `irispython-connector`   |
| Vector search                     | `iris-vector-ai`                                  |
| Connect from Python/Java/ODBC     | `iris-connectivity`                               |
| Fix / debug multi-class failures  | `objectscript-repair`                             |
| Look up IRIS docs                 | `iris-docs`                                       |
| Set up VS Code + ObjectScript ext | `iris-vscode-objectscript`                        |
| Docker / container setup          | `iris-linux-docker`                               |
| IIS / Windows setup               | `iris-windows-iis-setup`                          |

## The two facts that trip people up

1. **Connection config lives in `.iris-agentic-dev.toml`, NOT the MCP `env` block.**
   Editing `env` in `~/.claude.json` on a running server does nothing — env is injected
   once at spawn. The `.toml` is the right lever.

2. **The server re-reads `.iris-agentic-dev.toml` on every tool call** (mtime check via
   `ConfigWatcher`). A config edit takes effect the next time any MCP tool is called —
   **no restart, no reconnect needed**. Do not tell the user "restart to apply."

## Start with `check_config` — do not guess

Before writing any config, call `check_config` and read what it reports.

| What you see                                           | What it means                                                |
| ------------------------------------------------------ | ------------------------------------------------------------ |
| `connected: true, connection_source: "env_var"`        | Connected via env vars — a config file is optional           |
| `connected: true, connection_source: "docker"`         | Connected via Docker discovery — a config file is optional   |
| `connected: true, connection_source: "server_manager"` | VS Code Server Manager is working — no config needed         |
| `connected: false`                                     | No connection found — you need a config file                 |
| `config_parse_error` present                           | The existing config has a TOML syntax error — fix that first |
| `server_manager.available: true`                       | Set `IRIS_SERVER_NAME` instead of writing a manual config    |
| `atelier_rest: false`                                  | No Atelier REST — use the `docker exec` path (see below)     |

Write the config to the path reported as `config_watch_path`. Hot-reload is automatic.

## `IRIS_UNREACHABLE` — cause and fix

```text
IRIS_UNREACHABLE: no IRIS connection. Set IRIS_HOST and IRIS_WEB_PORT env vars,
or ensure IRIS is reachable on a discoverable port (52773, 41773, 51773, 8080).
```

**Common cause with OrbStack/Docker:** `container = "<name>"` triggers port-discovery
against a **fixed probe list (52773 / 41773 / 51773 / 8080)**. OrbStack maps the
container's 52773 to a _different, dynamic_ host port (e.g. 42773) — not on the list
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

Precedence: config file > env vars > auto-discovery. `OBJECTSCRIPT_WORKSPACE` overrides
where the config file is looked for (defaults to `$PWD`).

### HTTPS behind an enterprise web gateway

```toml
host       = "iris.corp.example.com"
web_port   = 443
scheme     = "https"
web_prefix = "irisaicore"
namespace  = "APP"
```

Atelier must then be reachable at `https://host:443/irisaicore/api/atelier/`.

### `docker_only` — skip HTTP entirely

```toml
container   = "my-iris"
namespace   = "USER"
docker_only = true
```

Every tool call goes through `docker exec`; no web port needed. Set this when the
container has no 52773 mapping, or when the build genuinely has no private web server
(Enterprise `2026.2.0AI`, DPP-1192). You usually do not need it — iad detects the NoPWS
case itself and reports `atelier_rest: false`. Community images do have PWS on 52773.

### Fleet / operate mode (multi-instance)

```toml
mode = "operate"

[instance.prod]
host      = "prod.example.com"
web_port  = 52773
namespace = "PROD"
username  = "svc"
password  = "secret"
role      = "subject"

[instance.dev]
container = "dev-iris"
namespace = "DEV"
role      = "workspace"
```

Roles: `workspace` (writes allowed), `subject` (read-only by default), `control-plane`.

### Per-connection policy

```toml
[policy.prod]
allow = ["query", "search", "docs"]
```

That blocks `compile`, `execute`, `source_control`, `debug`, `admin`, `skill`, and `kb`
on the `prod` Server Manager connection. Categories: `compile`, `execute`, `query`,
`search`, `docs`, `source_control`, `debug`, `admin`, `skill`, `kb`. Omit the
`[policy.*]` block (or the `allow` key) to permit everything.

### Config troubleshooting

- **`config_parse_error`** — TOML syntax. Usually an unquoted string (`host = iris.com`
  → `host = "iris.com"`) or a key promoted to a table (`[instance.prod.role]` → put
  `role = "subject"` inside `[instance.prod]`).
- **Connected to the wrong instance** — discovery found some other IRIS first. Set
  `IRIS_CONTAINER`, or write an explicit `host`/`container`.
- **`credential_status: "not_configured"`** — the credential is not in the OS keychain.
  VS Code → Server Manager → right-click the server → Reconnect, then re-run
  `check_config` and expect `credential_status: "resolved"`.
- **Several Server Manager servers and none selected** — set `IRIS_SERVER_NAME` to the
  map key from `intersystems.servers`.

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

## Adding skills for private or local packages

Ecosystem skills from public repos are registered in `skills-lock.json` with
`sourceType: "github"` — iad fetches them on demand from the package repo.

For a **private or not-yet-public** package, keep the skill local instead:

1. Add `skills/skills/<name>/SKILL.md` directly in the iad repo.
2. Add a `sourceType: "local"` entry to `skills-lock.json` (no `skillPath` needed —
   iad resolves it from the local tree):

```json
"my-private-pkg": {
  "source": "/path/to/iris-agentic-dev",
  "sourceType": "local",
  "computedHash": ""
}
```

1. Add the name to the appropriate grouping in `skills.sh.json`.

Anyone who clones iad gets the skill immediately — no github fetch, no auth.
When the repo goes public, flip `sourceType` to `"github"`, add `source` and
`skillPath`, and delete the local `SKILL.md`.

## Related skills

- **objectscript-review** — load when writing any ObjectScript; catches the 10 most common AI mistakes
- **objectscript-debugging** — when you hit a runtime error or need to map a .INT offset to source
- **objectscript-navigation** — when exploring an unfamiliar codebase
- **ensemble-production** — when working with Interoperability productions
- **objectscript-coverage** — when measuring test coverage with `iris_coverage`
