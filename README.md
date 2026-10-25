# iris-agentic-dev

MCP server and CLI that connects AI coding agents to a live InterSystems IRIS instance.
Compile ObjectScript, run SQL, search namespaces, run unit tests, inspect productions —
from any MCP-compatible agent or from the command line.

Requires IRIS 2023.1 or later. Works with native IRIS on Windows or Linux, and with Docker.

---

## Pick your mode

### Mode 1 — MCP server

90+ tools exposed over the Model Context Protocol. Connect once; your agent calls tools
by name. Works with Claude Code, GitHub Copilot, OpenCode, or any MCP client.

```bash
iris-agentic-dev mcp
```

To expose only the IRIS tools without the skill/KB/learning-agent surface, pass
`--no-skills` (or set `IRIS_NO_SKILLS=true`). Useful when skills are installed
separately and you want to avoid duplicate tool names in the MCP client:

```bash
iris-agentic-dev mcp --no-skills
```

→ [Quick start: VS Code + Copilot](#quick-start-vs-code--github-copilot) |
[Quick start: Claude Code / OpenCode](#quick-start-claude-code--opencode) |
[Full connection docs](docs/connecting.md)

### Mode 2 — CLI tool dispatch

One tool call, one subprocess, no server. Good for agents that run shell commands,
CI scripts, and skill repos that need IRIS access without a persistent MCP process.

Arguments go in `--args` (or `-a`) as a JSON object. The tool name is the only positional.

```bash
iris-agentic-dev tool iris_query --args '{"sql":"SELECT TOP 5 ID FROM Sample.Person"}'
iris-agentic-dev tool iris_compile --args '{"target":"MyApp.Foo.cls"}'
iris-agentic-dev tool iris_execute --args '{"code":"Write ##class(%SYS.ProcessQuery).GetInfo()"}'
```

`tool --list` names every tool and `tool <name> --schema` prints its parameters, both without
connecting to IRIS — that is the fastest way to find out what a tool wants in `--args`:

```bash
iris-agentic-dev tool --list
iris-agentic-dev tool iris_query --schema
```

→ [Tool reference](docs/tools.md) |
[Annotated session: tracing an ADT^A03 through a production](docs/examples/adt-a03-discharge-flow.txt)

### Mode 3 — Skills only

No IRIS connection, no server. Installs ObjectScript instruction files into your agent's
skill directory. The binary and skills are independent — installing one doesn't install
the other.

```bash
iris-agentic-dev skill install
```

→ [Skill docs](#skills--improve-ai-output-for-objectscript) |
[Full skill reference](docs/skills.md)

---

## Install

### Mac (Homebrew)

```bash
brew tap intersystems-community/tap
brew install iris-agentic-dev
```

`brew upgrade iris-agentic-dev` updates the binary only — installed skills are
not touched by Homebrew. Run `iris-agentic-dev skill install` (or `--all`) after upgrading to
pick up new skills. Files installed before the `managed_by` marker was introduced
(or installed by other means) will be skipped as unrecognized; pass `--force` to
overwrite them and stamp them so later upgrades pick them up.

### Mac direct download (Apple Silicon)

```bash
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-macos-arm64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
xattr -d com.apple.quarantine /usr/local/bin/iris-agentic-dev 2>/dev/null
```

### Linux x86_64

```bash
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-linux-x86_64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
```

**Windows**: Download `iris-agentic-dev-windows-x86_64.exe` from the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
and place it on your PATH.

---

## Quick start: VS Code + GitHub Copilot

The VS Code extension handles binary discovery and connection config automatically.

**Prerequisites**: VS Code, GitHub Copilot,
[InterSystems ObjectScript extension](https://marketplace.visualstudio.com/items?itemName=intersystems-community.vscode-objectscript)

1. Install
   **[iris-agentic-dev for IRIS](https://marketplace.visualstudio.com/items?itemName=intersystems-community.vscode-iris-agentic-dev)**
   from the VS Code Marketplace
2. Reload VS Code

**iris-agentic-dev (IRIS)** now appears in **Copilot Chat → Agent mode → tools**. It reads
your existing `objectscript.conn` or `intersystems.servers` configuration — no additional
setup needed.

![iris-agentic-dev tools visible in the Copilot Configure Tools panel](docs/images/copilot-tools-panel.png)

To verify the connection, ask Copilot: _"Call check_config and show me the result."_

![check_config result showing connected: true, auto-discovered connection, and IRIS version](docs/images/check-config-result.png)

If VS Code has no IRIS connection yet, `objectscript.conn` in `settings.json` is the whole
config. Either point it at a Server Manager entry by name:

```json
"objectscript.conn": { "active": true, "server": "my-iris", "ns": "USER" }
```

or give the host and port directly, where `port` is the web server port, not 1972:

```json
"objectscript.conn": {
  "active": true,
  "host": "localhost",
  "port": 52773,
  "ns": "USER",
  "username": "_SYSTEM"
}
```

If the
[InterSystems Server Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.servermanager)
extension is installed, iris-agentic-dev reads your server list and retrieves credentials
from the OS keychain automatically — no additional config needed. Set `IRIS_SERVER_NAME`
if you have multiple servers configured.

> **Windows users**: iris-agentic-dev works with native IRIS on Windows — Docker is not
> required. If you hit a 404 on `/api/atelier`, see
> [Windows IIS setup](#windows-iis-api-web-application-required) below.

---

## Quick start: Claude Code / OpenCode

**Claude Code plugin** — the shortest route, and the only one that works on a machine with
nothing installed yet, because the plugin carries a setup skill that knows how to install the
binary:

```text
/plugin marketplace add intersystems-community/iris-agentic-dev
/plugin install iris-dev
> set up iris-agentic-dev against a local IRIS container
```

That brings the MCP server, the ten core skills plus the no-PWS setup skill, and the compile-on-save
hooks. The other bundled skills stay one `skill_describe` call away. The binary still has
to land on your PATH, which is what the third line is for: the setup skill installs it, finds or
starts an IRIS, clears the expired default password on a fresh container, and verifies with a
query. If you would rather do that part yourself, follow [Install](#install) first and the skill
will skip ahead.

The manual route below is still here, and is what to use for any other host.

After [installing the binary](#install), configure your agent:

**Claude Code** — register the server from the CLI:

```bash
claude mcp add iris-agentic-dev --scope user \
  -e IRIS_HOST=localhost -e IRIS_WEB_PORT=52773 \
  -e IRIS_USERNAME=_SYSTEM -e IRIS_PASSWORD="$IRIS_PASSWORD" -e IRIS_NAMESPACE=USER \
  -- iris-agentic-dev mcp
```

`claude mcp list` should then show `iris-agentic-dev … ✔ Connected`. Use `--scope project` to
write a committable `.mcp.json` in the current directory instead. The equivalent hand-written
entry in `~/.claude.json` is:

```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "command": "iris-agentic-dev",
      "args": ["mcp"],
      "env": {
        "IRIS_HOST": "localhost",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "<your _SYSTEM password>",
        "IRIS_NAMESPACE": "USER"
      }
    }
  }
}
```

**OpenCode** — add to `~/.config/opencode/config.json`:

```json
{
  "mcp": {
    "iris-agentic-dev": {
      "type": "local",
      "command": ["/usr/local/bin/iris-agentic-dev", "mcp"],
      "enabled": true,
      "environment": {
        "IRIS_HOST": "localhost",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "<your _SYSTEM password>",
        "IRIS_NAMESPACE": "USER"
      }
    }
  }
}
```

Note: OpenCode uses `"type": "local"` and `"environment"` (not `"type": "stdio"` and `"env"`).

**WSL2**: The Windows OpenCode GUI cannot spawn Linux ELF binaries. Use the Windows `.exe`
or invoke the Linux binary via `wsl.exe`:

```json
"command": ["wsl.exe", "-e", "/usr/local/bin/iris-agentic-dev", "mcp"]
```

---

## Quick start: Cursor IDE / Cursor CLI

Cursor is configured like other stdio MCP clients (`~/.cursor/mcp.json`), **not** like the
VS Code Copilot extension auto-registration path. Server Manager keychain and the Marketplace
extension behave differently (or not at all) in many Cursor setups.

See **[docs/cursor-quickstart.md](docs/cursor-quickstart.md)** for install, `mcp.json`, fleet
`server=` routing, and an explicit **Cursor vs VS Code + Copilot** comparison.

---

## Connecting to IRIS

### Native IRIS on Windows or Linux (no Docker)

Add a `.iris-agentic-dev.toml` file to your project root:

```toml
host = "localhost"
web_port = 80        # IIS default for IRIS 2024.1+; use 52773 for pre-2024.1
namespace = "USER"
username = "_SYSTEM"
# No password here: .iris-agentic-dev.toml lives in the project and gets committed.
# Put it in IRIS_PASSWORD in the MCP launch settings (see the MCP registration above).
```

#### Port reference

| IRIS version         | Web server | Default port |
| -------------------- | ---------- | ------------ |
| 2024.1+ on Windows   | IIS        | 80           |
| 2024.1+ on Linux     | Apache     | 80           |
| Pre-2024.1 (any OS)  | PWS        | 52773        |

#### Windows IIS: `/api` web application required

This is the most common failure on Windows. IIS needs an explicit `/api` web application
mapped to the IRIS Web Gateway module. Without it, `/api/atelier` returns 404 — even when
the Management Portal loads correctly.

**To fix:**

1. Open **IIS Manager** → expand your server → **Sites** → **Default Web Site**
2. Right-click → **Add Application**. Set alias: `api`, physical path:
   `C:\InterSystems\IRIS\CSP\bin` (adjust to your install path)
3. Add a wildcard script handler mapping: executable = `CSPms.dll`, no verb restriction
4. Verify `CSP.ini` contains an `[APP_PATH:/api]` section

See the [`iris-windows-iis-setup` skill](./skills/skills/iris-windows-iis-setup/SKILL.md)
for full step-by-step instructions with verification commands.

**`localhost` vs `127.0.0.1`**: On some older Web Gateway builds, using `localhost` causes
a brief connection error before each request. If you see connection delays, change to
`host = "127.0.0.1"`.

### Docker (community image)

Run `iris-agentic-dev init` in your project directory — it detects any running IRIS
containers and writes `.iris-agentic-dev.toml` automatically:

```bash
iris-agentic-dev init
```

Or configure manually:

```toml
container = "myapp-iris"
namespace = "MYAPP"
```

### Docker (enterprise image)

Enterprise IRIS images (`intersystems/iris`, `intersystems/irishealth`) ship without a
built-in web server. Run the ISC Web Gateway container alongside IRIS:

```yaml
services:
  iris:
    image: containers.intersystems.com/intersystems/iris:2026.1
    ports: ["127.0.0.1:4972:1972"]
  webgateway:
    image: containers.intersystems.com/intersystems/webgateway:2026.1
    ports: ["127.0.0.1:52773:80"]
    entrypoint: ["/bin/sh", "/init.sh"]
    volumes: ["./webgateway-init.sh:/init.sh:ro"]
```

See the [`iris-vscode-objectscript` skill](./skills/skills/iris-vscode-objectscript/SKILL.md)
for a working `webgateway-init.sh`.

### VS Code Server Manager (zero-config)

If the
[InterSystems Server Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.servermanager)
extension is installed, iris-agentic-dev reads your server list from VS Code's
`settings.json` and resolves credentials from the OS keychain — no `.iris-agentic-dev.toml`
needed.

**Single server configured:** auto-connects, no extra setup.

**Multiple servers configured:** set `IRIS_SERVER_NAME` to the map key from
`intersystems.servers`:

```bash
export IRIS_SERVER_NAME=dev-local
```

Use `check_config` to see which servers were detected and whether credentials resolved.

### Per-connection policy (fleet / operate mode)

Add `[policy.<server-name>]` blocks to `.iris-agentic-dev.toml` to restrict which tool
categories are permitted on a given server:

```toml
[policy.prod]
allow = ["query", "search", "docs"]
```

Blocked calls return `error_code: "POLICY_GATE"` with the list of allowed categories.
Available categories: `compile`, `execute`, `query`, `search`, `docs`, `source_control`,
`debug`, `admin`, `skill`, `kb`.

A project's `.iris-agentic-dev.toml` can tighten a policy at once but cannot loosen one (raise the
write or destructive tier, widen `allow`, set a looser `mcpTemplate` or `dataPolicy`, turn off
certificate checks) until you run `iris-agentic-dev trust` on it at a terminal. Your own
`~/.iris-agentic-dev.toml` is always trusted. With no `mcpTemplate` declared, the instance's IRIS
system mode chooses one, so a Live instance is covered by default. See
[Connecting: trust and effective policy](./docs/connecting.md#where-configuration-comes-from).

### Connection discovery order

iris-agentic-dev resolves the IRIS connection in this order — first match wins:

1. CLI flags (`--host`, `--web-port`, `--scheme`)
2. The project's `.iris-agentic-dev.toml` (the nearest one walking up from the working directory, or the one `--config` / `OBJECTSCRIPT_WORKSPACE` names), when it names a host or container
3. Environment variables (`IRIS_HOST`, etc.)
4. `~/.iris-agentic-dev.toml`, your own file, when there is no project file (`IRIS_HOST` outranks its connection settings)
5. `IRIS_CONTAINER`, then a scan of localhost ports (52773, 80, 8080, 41773, 51773, 8443, 443)
6. Running Docker containers
7. VS Code `settings.json` (`objectscript.conn`)
8. VS Code Server Manager (`intersystems.servers` + OS keychain credential)

Discovery sends no credential to a listener until it has shown it is IRIS. To skip it, name your
server with `IRIS_HOST`, `--host` or `container`. Details, and what a project file may and may not
change, are in [connecting.md](./docs/connecting.md#connection-discovery-order).

### Environment variables

| Variable                | Default      | Description                                           |
| ----------------------- | ------------ | ----------------------------------------------------- |
| `IRIS_HOST`             | `localhost`  | IRIS web gateway hostname                             |
| `IRIS_WEB_PORT`         | `52773`      | Web gateway port                                      |
| `IRIS_SCHEME`           | `http`       | `http` or `https`                                     |
| `IRIS_WEB_PREFIX`       | _(empty)_    | URL path prefix for non-root gateway installs         |
| `IRIS_USERNAME`         | `_SYSTEM`    | IRIS username                                         |
| `IRIS_PASSWORD`         | `SYS`        | IRIS password                                         |
| `IRIS_SERVICE_USERNAME` | _(empty)_    | Least-privilege account for the tools that run your code |
| `IRIS_SERVICE_PASSWORD` | _(empty)_    | Password for `IRIS_SERVICE_USERNAME`                  |
| `IRIS_NAMESPACE`        | `USER`       | Default namespace                                     |
| `IRIS_CONTAINER`        | _(empty)_    | Docker container name (required for ✦ tools)          |
| `IRIS_SERVER_NAME`      | _(empty)_    | Server Manager server name (multiple servers)         |
| `OBJECTSCRIPT_WORKSPACE`| `$PWD`       | Workspace root for `.iris-agentic-dev.toml` lookup    |
| `IRIS_ENABLED_TOOLS`    | _(empty)_    | Comma-separated allowlist — expose only these tools   |
| `IRIS_TLS_VERIFY`       | `true`       | `false` skips certificate checks for the server your launch settings name only; other servers have their own `tls_verify` |
| `IAD_NO_KEYCHAIN`       | _(unset)_    | `1` skips the OS keychain; passwords go to `servers.json` (owner-only), tied to their address |
| `IAD_READ_ONLY`         | _(unset)_    | `strict` stops iad writing scratch classes to the server; see [connecting.md](./docs/connecting.md#strict-read-only-no-scratch-classes) |
| `IAD_HTTP_TOKEN`        | _(unset)_    | Bearer token for `mcp --transport http` (at least 32 bytes); required off loopback |

Every variable, with the config-file equivalents and the settings only your own
`~/.iris-agentic-dev.toml` may set, is in [connecting.md](./docs/connecting.md#environment-variables).

### Privilege separation for arbitrary execution

Nine tools run code the agent wrote: `iris_execute`, `iris_query` (`mode="write"` and
`force`), `iris_global` (`set`/`kill`), `iris_execute_method`, `global_kill`, `iris_test`,
`iris_coverage` (`run`/`start`), `iris_ws_open` and `iris_ws_exec`. Under a `%All` account
they can edit class and routine code, even by indirection, and get around the SCM lock.

Set `IRIS_SERVICE_USERNAME` / `IRIS_SERVICE_PASSWORD` to a least-privilege IRIS account.
Those nine tools then run as that account on every path: Atelier REST, `docker exec`, NoPWS,
SSH and the WebSocket terminal. If the account cannot be used on a path, the call is refused
or fails with that account's privilege error. It never runs as the container's OS user
(`%All`) unless you set `IAD_EXEC_AS_OS_USER=1` in the environment, or `exec_as_os_user = true`
in `~/.iris-agentic-dev.toml`; a project file cannot.

Every response says who ran the code:

```json
"identity": {"user": "iadsvc", "source": "service_account_env", "path": "atelier",
             "verified": true, "server": "default"}
```

`verified` means the user is what IRIS reported (`$USERNAME`), not what iad sent.
`check_config` lists the identity each path and each server would use.

**A server named with `server=` uses its own account, not the environment's.** Give it a
`service_username` in `servers.json` (or `iris_add_server`) or in an `[instance.<name>]` block
of your own `~/.iris-agentic-dev.toml`; its password goes in the OS keychain. With
`IRIS_SERVICE_USERNAME` set, a server without one is refused these tools
(`SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER`). A named server never falls back to the default
connection's `IRIS_CONTAINER`; give it a `container` to allow a docker fallback.

**What the account needs.** The account has to reach `/api/atelier` and the terminal, and
both check the `%Development` resource, so it needs `%Development:USE`. An account without it
gets `SERVICE_ACCOUNT_HTTP_DENIED` over HTTP and `SERVICE_ACCOUNT_TERMINAL_DENIED` in a
terminal. `%Development:USE` also opens the debugger and the rest of the Atelier API to that
account; what stops it editing code is database permission, so lay the databases out like this:

1. Three databases: application code, application data and a scratch area.
2. A namespace for the agent whose **default routines database is the scratch database** and
   whose globals database is the application data. Map the application's packages and
   routines into it from the code database (`Config.MapPackages`, `Config.MapRoutines`).
3. Set each database's resource (`SYS.Database.ResourceName`; a new one is `%DB_%DEFAULT`).
4. A role with `%Development:USE`, `%DB_<code>:R`, `%DB_<data>:RW` and `%DB_<scratch>:RW`,
   and a user with that role and password authentication.

The compiler writes to the default routines database, so a namespace whose default is
read-only cannot compile the class iad generates (`ERROR #302`), however `IrisDevTmp` is
mapped. A class in a mapped application package cannot be saved (`<PROTECT>`); a class in an
unmapped package lands in the scratch database and can do no more than run as the account.

`iris_coverage` `run` and `start` also need write access to the IRISSYS database
(`%DB_IRISSYS:RW`): the line monitor's state is system-wide. Without it they answer
`COVERAGE_PRIVILEGE_DENIED`. Grant it only if you accept that account writing IRISSYS.

Not covered: `iris_query` in `mode="read"` can call a stored procedure as the connection's own
user. Only IRIS privileges limit that. Code-writing tools (`iris_doc` put,
`iris_source_control`, `iris_compile`) keep using the primary `IRIS_USERNAME` so audit stays
attributed to the real user.

---

## Skills — improve AI output for ObjectScript

Skills are concise instruction files that teach your AI assistant ObjectScript-specific
patterns and common mistakes. They work with or without the MCP server and require no IRIS
connection.

Skills and the MCP server are independent — installing the binary installs no skills.

**There is no measured effect yet, in either direction.** An earlier run reported the skills at
+0.098 [-0.034, +0.229] and the tools at +0.829 [+0.714, +0.944] over a bare model, on 41 graded
task-pairs. Both figures are void. That harness did not set `OPENCODE_DISABLE_CLAUDE_CODE=1`, so
opencode loaded my `~/.claude/CLAUDE.md` and about 160 personal skills into every session, in
every arm; the finding is in
[`specs/130-content-skills/research.md`](specs/130-content-skills/research.md).

On clean isolation, a 19-task ladder at three repeats passed 11 of 19 with the tools alone and 12
of 19 with the tools plus the matching skill (b=3, c=2). That is below the run's power floor, so it
supports no claim either way. Every task is scored by an ObjectScript check against live IRIS, not
by a model.

Older figures in this repo — including a +27% for `objectscript-review` on a 22-task repair
suite — came from a harness that used a model as the judge, on a different corpus. They are
not comparable and should not be read as current.

**Install skills:**

```bash
iris-agentic-dev skill install                                # the 10 core skills, Claude Code + OpenCode
iris-agentic-dev skill install --all                          # core and extra
iris-agentic-dev skill install iris-query-plans              # one skill, any tier
iris-agentic-dev skill install --agent copilot               # repo-scoped Copilot instructions
iris-agentic-dev skill list                                  # check install status
```

**VS Code Copilot**: The extension installs the binary only. Run
`iris-agentic-dev skill install --agent copilot` from a git repo root to install skills
into `.github/instructions/`.

→ [Full skill docs](docs/skills.md)

### Skill inventory

| Skill                        | What it does                                                                                    | Benchmark   |
| ---------------------------- | ----------------------------------------------------------------------------------------------- | ----------- |
| `objectscript-review`        | Hard-gate checklist: 10 most common AI mistakes in ObjectScript                                 | 100% repair |
| `objectscript-guardrails`    | All-in-one hard gate, works without MCP                                                         | 86% repair  |
| `objectscript-sql-patterns`  | IRIS SQL quirks: reserved words, SQLCODE, table naming, NULL handling                           | 100% SQL    |
| `iris-query-plans`           | Reading query plans; stale indexes, `%BuildIndices`, `TUNE TABLE`, outlier selectivity          |             |
| `objectscript-unit-test`     | Generates `%UnitTest` scaffolding from live class introspection                                 | 86% repair  |
| `objectscript-list-patterns` | `%List`, `$LISTBUILD`, `$LISTNEXT`, `$LISTTOSTRING` patterns                                    | 91% repair  |
| `objectscript-navigation`    | Codebase discovery using MCP introspection tools                                                | 82% repair  |
| `objectscript-tdd`           | Compile-test-fix loop for iterative development                                                 |             |
| `objectscript-debugging`     | Maps `.INT` offsets to `.CLS` source lines, reads error logs                                    |             |
| `objectscript-repair`        | Coordinated fixes across multiple dependent classes                                             |             |
| `iris-docs`                  | Fetches live IRIS class reference before implementing any API — eliminates hallucinated methods |             |
| `iris-vector-ai`             | IRIS vector search syntax (HNSW, `VECTOR_COSINE`, `TO_VECTOR`)                                  | domain      |
| `iris-connectivity`          | IRIS connection APIs from Python, Java, JDBC, ODBC                                              | domain      |
| `ensemble-production`        | Interoperability production lifecycle, logs, queues                                             | domain      |
| `iris-devtester`             | `IRISContainer` factory methods and test fixture patterns                                       | domain      |

> **Where the Benchmark column comes from**: the model-judged repair and SQL suites, not the
> graded holdout above. Treat it as a description of what each skill was aimed at, not as a
> measured effect — the graded run found no distinguishable lift from the skills as a set.
> One skill is known to hurt: `objectscript-loop-patterns` measured −19% when loaded for all
> tasks, which is why domain skills should load only in their domain. See
> [BENCHMARKING.md](./skills/BENCHMARKING.md).

See [`skills/`](./skills/) for the full list and how to contribute a skill, and
[`specs/121-benchmark-program/quickstart.md`](specs/121-benchmark-program/quickstart.md) to
run the graded benchmark yourself.

---

## Tools

Most tools work over the Atelier REST API and connect to any IRIS instance — no Docker
required unless noted. Tools marked ✦ require `IRIS_CONTAINER`. Tools marked 🔒 are
write-gated: they run only when the target server's write tier is on, which by default it is not on a
Live instance (see [Write protection](./docs/tools.md#write-protection)).

Tools that can reach patient data or IRIS internals are blocked before they run unless
you opt in. See [Data safety gates](./docs/tools.md#data-safety-gates) for what is
blocked and how to permit it.

### Code

| Tool                  | What it does                                                                                     |
| --------------------- | ------------------------------------------------------------------------------------------------ |
| `iris_compile`        | Compile a class, routine, or wildcard. Returns errors with line numbers.                         |
| `iris_doc`            | Read, write, delete, or check any IRIS document.                                                 |
| `iris_execute`        | Run ObjectScript, return output.                                                                 |
| `iris_execute_method` | Invoke a `ClassMethod` directly by class+method+args, no boilerplate.                            |
| `iris_query`          | Execute SQL, return rows as JSON. `mode=explain\|count\|write` for plans, counts, and gated DML. |
| `iris_test`           | Run `%UnitTest` tests, return structured pass/fail results.                                      |
| `iris_global`         | Read, write, kill, or list IRIS global nodes. Patient-data and system globals are gated.         |
| `iris_coverage`       | Measure ObjectScript line coverage via `%Monitor.System.LineByLine`.                            |
| `iris_source_control` ✦ | Check lock status, checkout, execute SCM actions.                                             |

### Search and introspection

| Tool                          | What it does                                                                       |
| ----------------------------- | ---------------------------------------------------------------------------------- |
| `iris_symbols`                | Search classes and methods via `%Dictionary`.                                      |
| `iris_symbols_local`          | Search `.cls`/`.mac`/`.inc` files on disk by glob — no IRIS connection required.   |
| `docs_introspect`             | Deep class inspection: methods, properties, XData, superclasses.                  |
| `iris_search`                 | Full-text search across the namespace. Supports regex and category filters.        |
| `iris_info`                   | Namespace discovery: documents, jobs, CSP apps, metadata.                         |
| `iris_macro`                  | Macro inspection: list, signature, definition, expand.                             |
| `iris_table_info`             | Inspect a SQL table: class-projected vs. DDL, backing storage globals.             |
| `resolve_dynamic_dispatch`    | Resolve `$classmethod`/`##class({var})` polymorphic dispatch to candidate classes. |
| `extract_message_map_routing` | Extract a compiled Ensemble `MessageMap` routing table.                            |
| `find_subclass_implementations` | Find all concrete subclass implementations of a method.                          |

### Debugging

| Tool          | What it does                                                           |
| ------------- | ---------------------------------------------------------------------- |
| `iris_debug`  | Map INT offsets to source lines, fetch error logs, capture error state. |
| `iris_get_log` | Retrieve a full result by `log_id` when a tool returns `truncated: true`. |
| `check_config` | Show active connection state — host, container, config file, write tool status. |

### Generation

| Tool                  | What it does                                                                   |
| --------------------- | ------------------------------------------------------------------------------ |
| `iris_generate`       | Build a context-rich prompt for generating ObjectScript. No API key required.  |
| `iris_generate_class` | Generate and compile a class from a description (requires LLM API key).        |
| `iris_generate_test`  | Generate `%UnitTest` scaffolding for an existing class.                         |

### Interoperability

| Tool                    | What it does                                                                                         |
| ----------------------- | ---------------------------------------------------------------------------------------------------- |
| `iris_production` ✦     | Start, stop, update, check, or recover a production.                                                 |
| `iris_interop_query` ✦  | Query production logs, queue depths, or message archive.                                             |
| `iris_production_item` 🔒 | Enable, disable, or get/set settings on a production config item.                                  |
| `iris_production_diff`  | Diff the running production config against the last source-controlled version.                       |
| `iris_message_body`     | Read a message body by ID. Blocked unless the connection permits patient data.                       |
| `iris_business_rule_info` | List or inspect Ensemble business rules.                                                           |
| `iris_credential_list`  | List Ensemble credentials (IDs/usernames only — passwords never returned).                           |
| `iris_credential_manage` 🔒 | Create, update, or delete an Ensemble credential.                                                |
| `iris_lookup_manage`    | Read, write, delete, or list Ensemble lookup table entries (write actions gated).                    |
| `iris_lookup_transfer`  | Export or import an Ensemble lookup table as XML (import gated).                                     |

### Administration

| Tool                   | What it does                                                                                      |
| ---------------------- | ------------------------------------------------------------------------------------------------- |
| `iris_admin`           | List namespaces, databases, users, roles, web apps; create/delete users; real-time observability. |
| `journal_search`       | Search the IRIS journal for SetKill records by global pattern and/or time range.                  |
| `iris_namespace_list`  | List all namespaces on the connected instance.                                                    |
| `iris_namespace_create` 🔒 | Create a new namespace.                                                                       |
| `iris_database_list`   | List databases and their paths.                                                                   |
| `iris_database_stats`  | Disk usage and block-level stats for a database.                                                  |
| `query_audit_log`      | Query the IRIS SQL audit log for recent activity.                                                 |
| `my_access`            | Show the current user's roles and resource permissions.                                           |
| `capability_matrix`    | Show which tools are enabled/disabled and why (gates, policy, container availability).            |
| `iris_containers` ✦    | List, select, or start IRIS Docker containers.                                                    |

### Learning agent, skills, and knowledge base

| Tool               | What it does                                                                                 |
| ------------------ | -------------------------------------------------------------------------------------------- |
| `agent_history`    | Recent tool-call history for the current session.                                            |
| `agent_stats`      | Learning agent status: skill count, pattern count, KB size.                                  |
| `telemetry_query`  | Query the durable telemetry record beyond the in-memory session.                             |
| `skill`            | Manage the learning agent skill registry: list, describe, search, forget, or propose.        |
| `skill_community`  | Browse or install community skills published to subscribed GitHub repos.                     |
| `kb`               | Index markdown/text into the IRIS knowledge base, or recall content by keyword.              |

→ [Full tool reference with error codes](docs/tools.md)

---

## Troubleshooting

| Symptom                                    | Likely cause                          | Fix                                                                     |
| ------------------------------------------ | ------------------------------------- | ----------------------------------------------------------------------- |
| 404 on `/api/atelier` (Windows)            | IIS missing `/api` web application    | See [Windows IIS setup](#windows-iis-api-web-application-required)      |
| `check_config` works but compile/search fail | Atelier `Recurse=0`                 | Management Portal → Security → Web Apps → `/api/atelier` → enable Recurse |
| All tools fail, namespace listing works    | API version mismatch                  | Verify IRIS supports Atelier v8                                         |
| 403 on write operations                    | Insufficient permissions              | Use a user with `%DB_USER` or `%All` role                               |
| Connection delays on Windows               | `localhost` DNS issue                 | Use `host = "127.0.0.1"` in `.iris-agentic-dev.toml`                   |
| `SERVER_MANAGER_CREDENTIAL_ERROR`          | Credential not in OS keychain         | VS Code → Server Manager → right-click server → **Reconnect**           |
| `SERVER_MANAGER_AMBIGUOUS`                 | Multiple SM servers, no server name   | Set `IRIS_SERVER_NAME=<server-key>`                                     |

For verbose HTTP logging:

```bash
iris-agentic-dev mcp --verbose 2>debug.log
```

→ [Full troubleshooting guide](docs/troubleshooting.md)

---

## Commands

```bash
iris-agentic-dev mcp                              # Start the MCP server
iris-agentic-dev tool <name> --args <json>        # Call a tool directly (no server)
iris-agentic-dev tool --list                      # List every tool, no connection
iris-agentic-dev tool <name> --schema             # Print one tool's parameters
iris-agentic-dev batch --file steps.json          # Several tool calls in one process
iris-agentic-dev compile MyApp.Foo.cls            # Compile from the terminal
iris-agentic-dev skill install [names|--all]      # Install core, all, or named skills
iris-agentic-dev skill list                       # Check skill install status
iris-agentic-dev init                             # Generate .iris-agentic-dev.toml
iris-agentic-dev benchmark --skill <path>         # Run the skill benchmark harness
iris-agentic-dev --version                        # Print version
```

---

## Documentation

| Guide                                          | Contents                                                                         |
| ---------------------------------------------- | -------------------------------------------------------------------------------- |
| [docs/getting-started.md](docs/getting-started.md) | Start here. Install, connect, first tool call, first errors, then your editor |
| [docs/connecting.md](docs/connecting.md)       | Native IRIS, Docker, Server Manager, policy gates, env vars, discovery order     |
| [docs/tools.md](docs/tools.md)                 | Full tool catalog with descriptions and error codes                              |
| [docs/skills.md](docs/skills.md)               | Skill inventory, benchmark results, CLI install reference                        |
| [docs/troubleshooting.md](docs/troubleshooting.md) | Symptom table, CLI commands, verbose logging                               |
| [docs/ecosystem-integration.md](docs/ecosystem-integration.md) | Patterns for downstream projects and skill repos              |
| [docs/cursor-quickstart.md](docs/cursor-quickstart.md) | Cursor IDE and Cursor CLI setup, fleet routing, vs VS Code + Copilot      |

---

## Contributing

Issues and pull requests are welcome. File bugs at the
[Issues tab](https://github.com/intersystems-community/iris-agentic-dev/issues).

To contribute a skill: write a `SKILL.md`, run the benchmark, submit a PR with your
results. See [BENCHMARKING.md](./skills/BENCHMARKING.md).

Questions: [thomas.dyar@intersystems.com](mailto:thomas.dyar@intersystems.com)
