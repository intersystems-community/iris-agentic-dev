---
name: iris-agentic-dev
tier: core
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

## Extra skills

The plugin and a bare `iris-agentic-dev skill install` ship the core skills only. The skills below are not installed by default. Read one with `skill_describe(name="<skill>")`, or install it with `iris-agentic-dev skill install <skill>` (`skill install --all` installs all of them).

- `ensemble-production`: start, stop and inspect Interoperability productions, logs, queues and message traces
- `iris-ai-hub`: AI Hub `%AI.*` classes on EAP builds, where the upstream docs are and how to check them against the installed build
- `iris-container-graceful-shutdown`: stop IRIS before `docker stop` so the WIJ stays clean and data survives restarts
- `iris-cpf-merge`: configure containers with CPF merge files, passwords and `Actions`
- `iris-devtester`: tests that need a live IRIS container through iris-devtester's `IRISContainer`
- `iris-docs`: look up IRIS documentation with `iris_doc_search`, Documatic URLs, `docs_introspect`
- `iris-embedded-python`: run Python inside IRIS and call Python libraries from ObjectScript
- `iris-linux-docker`: Linux bind-mount permissions for IRIS containers (UID 51773)
- `iris-objectscript-eval`: execute, compile and test ObjectScript through the MCP tools instead of `docker exec`
- `iris-mdx`: MDX queries against IRIS BI cubes: cube discovery, hierarchy paths, `%FILTER`, `%OR`, calculated members, and the 2026.2 `%MDX()` bug
- `iris-pgwire`: connect to IRIS over the PostgreSQL wire protocol (psycopg3 and other Postgres clients)
- `iris-product-features`: what IRIS and its products actually include, before you describe a feature
- `iris-query-plans`: read a query plan, stale indexes, `%BuildIndices`, `TUNE TABLE`, fixed and collected statistics
- `iris-vector-ai`: vector search, embeddings and HNSW indexes in IRIS SQL (not pgvector syntax)
- `iris-vscode-objectscript`: VS Code settings for ObjectScript against an IRIS container
- `iris-windows-iis-setup`: IIS for IRIS installed natively on Windows
- `irishealth-container`: IRIS for Health community (FHIR R4) and AI Hub containers
- `irispython-connector`: DB-API, SQLAlchemy and pandas from Python outside IRIS over TCP
- `objectscript-coverage`: line coverage with `iris_coverage`
- `objectscript-fewshot-fixes`: worked fixes for common ObjectScript mistakes (Quit in a loop, escaping, and others)
- `objectscript-loop-patterns`: `For`/`While`, `$Order` iteration, postfix `Quit`, `Return` vs `Quit`
- `objectscript-mac-routines`: `.mac` routine syntax, labels, `#include`, `$ZTRAP`, extrinsic functions
- `objectscript-navigation`: find your way around an unfamiliar codebase with the MCP and text tools
- `objectscript-repair`: coordinated fixes across several classes after a signature change or rename

## The two facts that trip people up

1. **Connection config lives in `.iris-agentic-dev.toml`, NOT the MCP `env` block.**
   Editing `env` in `~/.claude.json` on a running server does nothing — env is injected
   once at spawn. The `.toml` is the right lever.

2. **The server re-reads `.iris-agentic-dev.toml` on every tool call** (mtime check via
   `ConfigWatcher`). A config edit takes effect the next time any MCP tool is called —
   **no restart, no reconnect needed**. Do not tell the user "restart to apply."

## Start with `check_config` — do not guess

Before writing any config, call `check_config` and read what it reports.

| What you see                                                  | What it means                                                                            |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `connected: true, connection_source: "config_file"`           | A `.iris-agentic-dev.toml` chose the connection — the config is working                  |
| `connected: true, connection_source: "env_vars"`              | Connected via env vars (`IRIS_HOST`) — a config file is optional                         |
| `connected: true, connection_source: "auto_discovered"`       | Found on its own (Docker, a local port scan, Server Manager) — a config file is optional |
| `connected: true, connection_source: "iris_select_container"` | Switched mid-session with `iris_select_container`                                        |
| `connected: false`                                            | No connection found — you need a config file                                             |
| `config_parse_error` present                                  | The existing config has a TOML syntax error — fix that first                             |
| `server_manager.available: true`                              | Set `IRIS_SERVER_NAME` instead of writing a manual config                                |
| `atelier_rest: false`                                         | No Atelier REST — use the `docker exec` path (see below)                                 |

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

## `<CLASS DOES NOT EXIST>` for a system class

`Security.*`, `Config.*` and `SYS.*` classes exist only in `%SYS`. Web applications, users,
roles, namespaces and databases all live there. Called from `USER`, they fail with an error that
names the class and nothing else:

```text
ERROR: <CLASS DOES NOT EXIST> 150 Execute+7^IrisDevTmp.IrisDevRun...1 Security.Applications
```

Pass `namespace: "%SYS"` to `iris_execute`. `%SYS.*` and `%SYSTEM.*` are different: `%` packages
resolve in every namespace, so `##class(%SYSTEM.Security)` works from `USER`. `iris_execute` adds
a `hint` to the error when it sees this case.

Their SQL tables behave the same way. `SELECT Name FROM Security.Users` from `USER` fails with
SQLCODE -30 `Table 'SECURITY.USERS' not found`; pass `namespace: "%SYS"` to `iris_query`.

## `iris_execute_method` cannot keep an object between calls

Every call runs in a new process. `%New` returns an object reference such as `1060@Bench.Tracked`, and that reference is gone before the next call. A later `%Save` or `%Id` on the class gives `<METHOD DOES NOT EXIST>`, because those are instance methods, and `%DeleteId(1060)` gives "Object to Delete not found", because 1060 is the reference and not a row id.

To create, save and delete a row, run it in one `iris_execute` block:

```objectscript
Set o = ##class(Bench.Tracked).%New()
Set o.Name = "a"
Set sc = o.%Save()
Set id = o.%Id()
Set sc2 = ##class(Bench.Tracked).%DeleteId(id)
Write $Data(^Bench.Tracked("live", id))
```

Use `iris_execute_method` for class methods that take plain arguments and return a value.

## Loading an XML export

`iris_doc(mode="put")` takes an XML export (`<Export>` with a `<Class>` inside) when you put it
under the class's `.cls` name. IRIS imports it, stores it as UDL, and `compile=true` compiles it.
Two things go wrong:

- A `.xml` document name is refused with `ERROR #16006: Document ... name is invalid`.
- The `<?xml ...?>` declaration must be on its own line, followed by a newline. An export squashed
  onto one line is refused with `ERROR #16021: Illegal Header Line`. iad does not show that error
  yet; you see only the compile failure that follows, because nothing was stored. If a put of an
  XML export stores nothing, check the line breaks first.

```text
iris_doc(mode="put", name="MyApp.Greeter.cls", content="<?xml version=\"1.0\" encoding=\"UTF8\"?>\n<Export ...>...</Export>", compile=true)
```

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

### Multi-instance connection pool

Every tool accepts an optional `server` parameter that routes the call to a named IRIS
instance without changing the default connection:

```text
iris_execute(server="prod", code="Write $ZVersion")
iris_query(server="staging", query="SELECT COUNT(*) FROM MyApp.Patient")
```

Manage the pool with the server management tools:

```text
iris_servers()                        # list all registered instances + reachability
iris_add_server(name="prod", host="iris.example.com", port=52773,
                namespace="LIVE", username="svc", password="...")
iris_test_server(name="prod")         # probe: version, Atelier API, latency
iris_remove_server(server="prod")     # ☠ destructive — requires destructive_tools_enabled
iris_import_servers()                 # pull VS Code Server Manager entries into iad
```

`iris_servers` shows `reachable: null` until you pass `probe: true` or call
`iris_test_server` — reachability is not probed at startup.

### Write protection — two tiers

**Tier 1** (`write_tools_enabled = true`) — compile, execute, source control, routine
writes. With nothing declared, the target server's IRIS system mode decides: off on Live,
on on Development and Test. With it off, every write-capable tool returns
`WRITE_TOOLS_DISABLED` before it reaches IRIS.

**Tier 2** (`destructive_tools_enabled = true`) — `global_kill`, `iris_global(kill)`,
`iris_admin` writes, `iris_credential_manage`, `iris_lookup_manage` (write/delete),
`iris_namespace_create`, `iris_remove_server`, `skill_forget`. Default: **false**
regardless of tier 1. Declaring it with `write_tools_enabled = false` is a startup
error: the server logs `DESTRUCTIVE_REQUIRES_WRITES` and exits 2.

```toml
write_tools_enabled       = true
destructive_tools_enabled = true          # only needed for ☠ tools
```

Both gates are resolved once per config load and checked once per tool call, so a tool
cannot be write-capable and ungated. Precedence: an env var the operator exported before
the process started beats the config files, which beat IRIS `SystemMode`, which beats the
namespace heuristic. `check_config` reports which one decided, as `write_tools_source`
and `destructive_tools_source`.

**A project file cannot raise either tier.** The operator's own `~/.iris-agentic-dev.toml` is
always trusted. A `.iris-agentic-dev.toml` in the project (or one named by `--config`) is
trusted only after the operator runs `iris-agentic-dev trust` on it at a terminal; until then
its `write_tools_enabled = true` is set aside, and `check_config` lists it under `set_aside`
with the command that would lift it. A file can always turn a tier off. You cannot run `trust`
for the operator: ask them to.

The tiers are per server. `[policy.<name>] writeTools` and `destructiveTools` set them for one
server, and each call is judged against the server it runs on.

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
role      = "workbench"
```

Roles: `workbench` (the default; a server the agent builds on) and `subject` (a server under
study: source-control writes are refused, and any other write needs `confirm: true`).
`workspace` is the pre-1.7 spelling of `workbench`; any other word reads as `subject`.

### Per-connection policy

```toml
[policy.prod]
allow = ["query", "search", "docs"]
```

That blocks `compile`, `execute`, `source_control`, `debug`, `admin`, `skill`, and `kb`
on the `prod` Server Manager connection. Categories: `compile`, `execute`, `query`,
`search`, `docs`, `source_control`, `debug`, `admin`, `skill`, `kb`. Omit the
`[policy.*]` block (or the `allow` key) to permit everything. A project file's `allow` can only
narrow what the operator's file allows; a looser `mcpTemplate` or `dataPolicy` in it waits for
trust.

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
- **`WRITE_TOOLS_DISABLED`** — a 🔒 tool was called with tier 1 off. `check_config`'s
  `write_tools_source` names what decided it; fix that input, not the toml, if the source
  is `operator_env` or `inferred_system_mode`. If the project's toml says
  `write_tools_enabled = true` and `set_aside` lists it, the file is not trusted: the operator
  runs `iris-agentic-dev trust --show`, then `iris-agentic-dev trust`, in a terminal.
- **`set_aside` is not empty** — iad read a setting and did not act on it. Each item has a
  `reason` and a `lift`. Do not edit the file to get around it, and do not run `trust` yourself.
- **`DESTRUCTIVE_TOOLS_DISABLED`** — a ☠ tool was called without
  `destructive_tools_enabled = true`. Add that key to the toml for this connection.
- **`DESTRUCTIVE_REQUIRES_WRITES`** — the toml declares `destructive_tools_enabled = true`
  with `write_tools_enabled = false`. The server refuses to start. Set both or neither.

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
