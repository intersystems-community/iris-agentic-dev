# iris-agentic-dev

Connect GitHub Copilot, Claude Code, and other AI coding assistants directly to a live InterSystems IRIS instance. The AI can compile classes, run ObjectScript, execute SQL, search the namespace, run unit tests, and inspect class definitions — without leaving the chat.

Works with IRIS installed natively on Windows or Linux, and with Docker. Requires IRIS 2023.1 or later.

**The core of iris-agentic-dev is a binary** — install it and wire it into any MCP-compatible AI tool. The VS Code extension is optional convenience that auto-configures the binary for GitHub Copilot using your existing `objectscript.conn` connection.

---

## Quick start: VS Code + GitHub Copilot

The VS Code extension handles binary discovery and connection config automatically — no manual setup needed.

**Prerequisites**: VS Code, GitHub Copilot, [InterSystems ObjectScript extension](https://marketplace.visualstudio.com/items?itemName=intersystems-community.vscode-objectscript)

1. Install the **[iris-agentic-dev for IRIS](https://marketplace.visualstudio.com/items?itemName=intersystems-community.vscode-iris-agentic-dev)** VS Code extension from the Marketplace
2. Reload VS Code

**iris-agentic-dev (IRIS)** now appears in **Copilot Chat → Agent mode → tools**. It reads your existing `objectscript.conn` or `intersystems.servers` configuration — no additional setup needed.

![iris-agentic-dev tools visible in the Copilot Configure Tools panel](docs/images/copilot-tools-panel.png)

To verify the connection, ask Copilot: *"Call check_config and show me the result."*

![check_config result showing connected: true, auto-discovered connection, and IRIS version](docs/images/check-config-result.png)

If the [InterSystems Server Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.servermanager) extension is installed, iris-agentic-dev reads your server list and retrieves credentials from the OS keychain automatically — no additional config needed. Set `IRIS_SERVER_NAME` if you have multiple servers configured.

> **Windows users**: iris-agentic-dev works with native IRIS on Windows — Docker is not required. If you hit a 404 on `/api/atelier`, see [Windows IIS setup](#windows-iis-api-web-application-required) below.

---

## Quick start: Claude Code / OpenCode

**Install the binary:**

```bash
# Mac (Homebrew)
brew tap intersystems-community/iris-agentic-dev
brew install iris-agentic-dev

# Mac direct download (Apple Silicon)
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-macos-arm64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
xattr -d com.apple.quarantine /usr/local/bin/iris-agentic-dev 2>/dev/null

# Linux x86_64
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-linux-x86_64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
```

**Windows**: Download `iris-agentic-dev-windows-x86_64.exe` from the [releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest) and place it on your PATH.

**Configure Claude Code** — add to `~/.claude.json`:

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
        "IRIS_PASSWORD": "SYS",
        "IRIS_NAMESPACE": "USER"
      }
    }
  }
}
```

**Configure OpenCode** — add to `~/.config/opencode/config.json`:

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
        "IRIS_PASSWORD": "SYS",
        "IRIS_NAMESPACE": "USER"
      }
    }
  }
}
```

Note: OpenCode uses `"type": "local"` and `"environment"` (not `"type": "stdio"` and `"env"`).

**WSL2**: The Windows OpenCode GUI cannot spawn Linux ELF binaries. Use the Windows `.exe` or invoke the Linux binary via `wsl.exe`:

```json
"command": ["wsl.exe", "-e", "/usr/local/bin/iris-agentic-dev", "mcp"]
```

---

## Connecting to IRIS

### Native IRIS on Windows or Linux (no Docker)

Add a `.iris-agentic-dev.toml` file to your project root:

```toml
host = "localhost"
web_port = 80        # IIS default for IRIS 2024.1+; use 52773 for pre-2024.1
namespace = "USER"
username = "_SYSTEM"
password = "SYS"
```

#### Port reference

| IRIS version | Web server | Default port |
|---|---|---|
| 2024.1+ on Windows | IIS | 80 |
| 2024.1+ on Linux | Apache | 80 |
| Pre-2024.1 (any OS) | Private Web Server (PWS) | 52773 |

#### Windows IIS: `/api` web application required

This is the most common failure on Windows. IIS needs an explicit `/api` web application mapped to the IRIS Web Gateway module. Without it, `/api/atelier` returns 404 — even when the Management Portal loads correctly.

**To fix:**

1. Open **IIS Manager** → expand your server → **Sites** → **Default Web Site**
2. Right-click → **Add Application**. Set alias: `api`, physical path: `C:\InterSystems\IRIS\CSP\bin` (adjust to your install path)
3. Add a wildcard script handler mapping: executable = `CSPms.dll`, no verb restriction
4. Verify `CSP.ini` contains an `[APP_PATH:/api]` section

See the [`iris-windows-iis-setup` skill](./skills/skills/iris-windows-iis-setup/SKILL.md) for full step-by-step instructions with verification commands.

**`localhost` vs `127.0.0.1`**: On some older Web Gateway builds, using `localhost` causes a brief connection error before each request. If you see connection delays, change the config to `host = "127.0.0.1"`.

### Docker (community image)

Run `iris-agentic-dev init` in your project directory — it detects any running IRIS containers and writes `.iris-agentic-dev.toml` automatically:

```bash
iris-agentic-dev init
```

Or configure manually:

```toml
container = "myapp-iris"
namespace = "MYAPP"
```

### Docker (enterprise image)

Enterprise IRIS images (`intersystems/iris`, `intersystems/irishealth`) ship without a built-in web server. Run the ISC Web Gateway container alongside IRIS:

```yaml
services:
  iris:
    image: containers.intersystems.com/intersystems/iris:2026.1
    ports: ["4972:1972"]
  webgateway:
    image: containers.intersystems.com/intersystems/webgateway:2026.1
    ports: ["52773:80"]
    entrypoint: ["/bin/sh", "/init.sh"]
    volumes: ["./webgateway-init.sh:/init.sh:ro"]
```

See the [`iris-vscode-objectscript` skill](./skills/skills/iris-vscode-objectscript/SKILL.md) for a working `webgateway-init.sh`.

### VS Code Server Manager (zero-config)

If the [InterSystems Server Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.servermanager) extension is installed, iris-agentic-dev reads your server list from VS Code's `settings.json` and resolves credentials from the OS keychain automatically — no `.iris-agentic-dev.toml` needed.

**Single server configured:** auto-connects, no extra setup.

**Multiple servers configured:** set `IRIS_SERVER_NAME` to the map key from `intersystems.servers`:

```bash
export IRIS_SERVER_NAME=dev-local
```

Credentials are stored under keychain service `"intersystems-server-credentials"` — the auth provider ID used by Server Manager in all VS Code-compatible forks (Cursor, Windsurf, VS Code Insiders). If a credential is missing, iris-agentic-dev fails fast with a message directing you to reconnect in VS Code (right-click the server → **Reconnect**) rather than silently falling through to other discovery sources.

Use `check_config` to see which servers were detected and whether credentials resolved:

```json
{
  "server_manager": {
    "available": true,
    "servers": [
      { "name": "dev-local", "active": true, "credential_status": "resolved" }
    ]
  }
}
```

### Per-connection policy (fleet / operate mode)

Add `[policy.<server-name>]` blocks to `.iris-agentic-dev.toml` to restrict which tool categories are permitted on a given Server Manager server:

```toml
[policy.prod]
allow = ["query", "search", "docs"]
```

Blocked calls return `error_code: "POLICY_GATE"` with the list of allowed categories. Omit the block entirely to permit everything. Available categories: `compile`, `execute`, `query`, `search`, `docs`, `source_control`, `debug`, `admin`, `skill`, `kb`.

For multi-instance fleet workflows (`mode = "operate"`), see the [fleet roles spec](./specs/003-workspace-config/) for the full `[instance.*]` config format and role-gate behavior.

### Connection discovery order

iris-agentic-dev resolves the IRIS connection in this order — first match wins:

1. CLI flags (`--host`, `--web-port`, `--scheme`)
2. `.iris-agentic-dev.toml` in the workspace root
3. Environment variables (`IRIS_HOST`, etc.)
4. VS Code `settings.json` (`objectscript.conn` / `intersystems.servers`)
5. VS Code Server Manager keychain (`intersystems.servers` + OS keychain credential)
6. Running Docker containers (scored by workspace name similarity)
7. Localhost port scan (52773, 41773, 51773, 8080)

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `IRIS_HOST` | `localhost` | IRIS web gateway hostname |
| `IRIS_WEB_PORT` | `52773` | Web gateway port |
| `IRIS_SCHEME` | `http` | `http` or `https` |
| `IRIS_WEB_PREFIX` | *(empty)* | URL path prefix for non-root gateway installs |
| `IRIS_USERNAME` | `_SYSTEM` | IRIS username |
| `IRIS_PASSWORD` | `SYS` | IRIS password |
| `IRIS_NAMESPACE` | `USER` | Default namespace |
| `IRIS_CONTAINER` | *(empty)* | Docker container name — required for Docker-dependent tools |
| `IRIS_SERVER_NAME` | *(empty)* | Server Manager server name when multiple are configured |
| `OBJECTSCRIPT_WORKSPACE` | `$PWD` | Workspace root for `.iris-agentic-dev.toml` lookup |

---

## Skills — improve AI output for ObjectScript

Skills are concise instruction files that teach your AI assistant ObjectScript-specific patterns and common mistakes. They work with or without the MCP server.

Tested with Claude Sonnet 4.6 on the ObjectScript repair suite (22 tasks):

| Benchmark suite | Baseline | With top skill | Lift |
|-----------------|----------|----------------|------|
| ObjectScript repair (22 tasks) | 73% | **100%** | +27% |

The top skill is **`objectscript-review`** — a 205-word checklist that catches the 10 most common ObjectScript mistakes before the AI writes any code.

The multi-file and SQL-quirks suites referenced in earlier versions of this table are not
yet ported to the current native benchmark harness (`iris-agentic-dev benchmark`) — only
the repair suite above is runnable today. See
[BENCHMARKING.md](./skills/BENCHMARKING.md) to run it yourself, including a
[Limitations](./skills/BENCHMARKING.md#limitations) section covering contamination
risk, single-run variance, and single-model validation caveats on these numbers.

**VS Code Copilot:** Skills are included automatically when you install the extension.

**Claude Code:**

```bash
mkdir -p ~/.claude/skills
for skill in objectscript-review objectscript-guardrails objectscript-sql-patterns; do
  mkdir -p ~/.claude/skills/$skill
  curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/$skill/SKILL.md \
    > ~/.claude/skills/$skill/SKILL.md
done
```

**OpenCode:**

```bash
mkdir -p ~/.config/opencode/skills
for skill in objectscript-review objectscript-guardrails objectscript-sql-patterns; do
  mkdir -p ~/.config/opencode/skills/$skill
  curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/$skill/SKILL.md \
    > ~/.config/opencode/skills/$skill/SKILL.md
done
```

### Skill inventory

| Skill | What it does | Benchmark |
|-------|-------------|-----------|
| `objectscript-review` | Hard-gate checklist: 10 most common AI mistakes in ObjectScript | 🥇 100% repair |
| `objectscript-guardrails` | All-in-one hard gate, works without MCP | 86% repair |
| `objectscript-sql-patterns` | IRIS SQL quirks: reserved words, SQLCODE, table naming, NULL handling | 100% SQL |
| `objectscript-unit-test` | Generates `%UnitTest` scaffolding from live class introspection | 86% repair |
| `objectscript-list-patterns` | `%List`, `$LISTBUILD`, `$LISTNEXT`, `$LISTTOSTRING` patterns | 91% repair |
| `objectscript-navigation` | Codebase discovery using MCP introspection tools | 82% repair |
| `objectscript-tdd` | Compile-test-fix loop for iterative development | |
| `objectscript-debugging` | Maps `.INT` offsets to `.CLS` source lines, reads error logs | |
| `objectscript-repair` | Coordinated fixes across multiple dependent classes | |
| `iris-docs` | Fetches live IRIS class reference before implementing any API — eliminates hallucinated methods | |
| `iris-vector-ai` | IRIS vector search syntax (HNSW, `VECTOR_COSINE`, `TO_VECTOR`) | domain |
| `iris-connectivity` | IRIS connection APIs from Python, Java, JDBC, ODBC | domain |
| `ensemble-production` | Interoperability production lifecycle, logs, queues | domain |
| `iris-devtester` | `IRISContainer` factory methods and test fixture patterns | domain |

"repair" scores are reproducible today via `iris-agentic-dev benchmark --suite jira`.
"SQL" and "domain" scores predate the current native harness and are not yet
re-verifiable — see [BENCHMARKING.md](./skills/BENCHMARKING.md#additional-suites-not-yet-ported).

See [`skills/`](./skills/) for the full list, benchmark results, and how to contribute a skill.

> **Note**: some skills hurt if loaded globally. `objectscript-loop-patterns` measured −19% lift when loaded for all tasks. Domain skills (`iris-vector-ai`, `iris-connectivity`, `ensemble-production`) should only be loaded when working in those areas. See [BENCHMARKING.md](./skills/BENCHMARKING.md).

---

## Tools

Most tools work over the Atelier REST API and connect to any IRIS instance — no Docker
required unless noted. Tools marked ✦ require `IRIS_CONTAINER`. Tools marked 🔒 are
write-gated (suppressed on Live instances unless `IRIS_ALLOW_PROD=1`).

### Code

| Tool | What it does |
|------|-------------|
| `iris_compile` | Compile a class, routine, or wildcard. Returns errors with line numbers. |
| `iris_doc` | Read, write, delete, or check any IRIS document. |
| `iris_execute` | Run ObjectScript, return output. |
| `iris_execute_method` | Invoke a `ClassMethod` directly by class+method+args, no boilerplate. String-returning methods only (v1). |
| `iris_query` | Execute SQL, return rows as JSON. `mode=explain\|count\|write` for query plans, row-count estimates, and gated DML. |
| `iris_test` | Run `%UnitTest` tests, return structured pass/fail results. |
| `iris_global` | Read, write, kill, or list IRIS global nodes. PHI and system-blocklist gates enforced. |
| `iris_coverage` | Measure ObjectScript line coverage via `%Monitor.System.LineByLine`. `mode=run`: start+RunTest+stop+report in one call. `mode=check`: verify monitor is available. Returns per-class and total coverage percentages. Requires `gmheap ≥ 256` (Management Portal > Configuration > Advanced Memory) and IRIS restart. |
| `iris_source_control` ✦ | Check lock status, checkout, execute SCM actions. |

### Search and introspection

| Tool | What it does |
|------|-------------|
| `iris_symbols` | Search classes and methods via `%Dictionary`. |
| `iris_symbols_local` | Search `.cls`/`.mac`/`.inc` files on disk by glob pattern — no IRIS connection required. |
| `docs_introspect` | Deep class inspection: methods, properties, XData, superclasses. |
| `iris_search` | Full-text search across the namespace. Supports regex and category filters. |
| `iris_info` | Namespace discovery: documents, jobs, CSP apps, metadata. |
| `iris_macro` | Macro inspection: list, signature, definition, expand. |
| `iris_table_info` | Inspect a SQL table: class-projected vs. DDL, backing storage globals, optional row count. |
| `resolve_dynamic_dispatch` | Resolve `$classmethod`/`##class({var})` polymorphic dispatch to compiled candidate classes, with confidence scores. |
| `extract_message_map_routing` | Extract a compiled Ensemble `MessageMap` routing table (MessageType → Method) from a BusinessProcess/Router. |
| `find_subclass_implementations` | Find all concrete subclass implementations of a method across the full inheritance hierarchy. |

### Debugging

| Tool | What it does |
|------|-------------|
| `iris_debug` | Map INT offsets to source lines, fetch error logs, capture error state. |
| `iris_get_log` | Retrieve a full result by `log_id` when a tool returns `truncated: true`. |
| `check_config` | Show active connection state — host, container, config file, write tool status. |

### Generation

| Tool | What it does |
|------|-------------|
| `iris_generate` | Build a context-rich prompt for generating ObjectScript. No API key required. |
| `iris_generate_class` | Generate and compile a class from a description (requires LLM API key). |
| `iris_generate_test` | Generate `%UnitTest` scaffolding for an existing class. |

### Interoperability

| Tool | What it does |
|------|-------------|
| `iris_production` ✦ | Start, stop, update, check, or recover a production. |
| `iris_interop_query` ✦ | Query production logs, queue depths, or message archive. |
| `iris_production_item` 🔒 | Enable, disable, or get/set settings on an individual production config item. Works via HTTP, no Docker required. |
| `iris_production_diff` | Diff the running production config against the last source-controlled version. |
| `iris_message_body` | Read a message body by ID (plain-text or stream-backed). PHI-gated. |
| `iris_business_rule_info` | List or inspect Ensemble business rules (`Ens.Rule.RuleSet`). |
| `iris_credential_list` | List Ensemble credentials (IDs/usernames only — passwords never returned). |
| `iris_credential_manage` 🔒 | Create, update, or delete an Ensemble credential. |
| `iris_lookup_manage` | Read, write, delete, or list Ensemble lookup table entries (write actions gated). |
| `iris_lookup_transfer` | Export or import an Ensemble lookup table as XML (import gated). |

### Administration

| Tool | What it does |
|------|-------------|
| `iris_admin` | List namespaces, databases, users, roles, web apps; create/delete users (requires `IRIS_ADMIN_TOOLS=1`). |
| `iris_containers` ✦ | List, select, or start IRIS Docker containers. Hot-swaps the active connection without a session restart. |

### Learning agent, skills, and knowledge base

| Tool | What it does |
|------|-------------|
| `agent_history` | Recent tool-call history for the current session (tool, success, duration, timestamp). |
| `agent_stats` | Learning agent status: skill count, pattern count, KB size. |
| `telemetry_query` | Query the durable telemetry record beyond the in-memory session — by tool name, session id, or time range. |
| `telemetry_export_trace` | Export recorded tool calls as `{from, to, via, count, ts}` dispatch-trace records, aggregated. |
| `skill` | Manage the learning agent skill registry: list, describe, search, forget, or propose (mines recent calls into a new skill). |
| `skill_community` | Browse or install community skills published to subscribed GitHub repos. |
| `kb` | Index markdown/text into the IRIS knowledge base, or recall content by keyword. |

See [`skills/BENCHMARKING.md`](./skills/BENCHMARKING.md) for the benchmark harness.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| 404 on `/api/atelier` (Windows) | IIS missing `/api` web application | See [Windows IIS setup](#windows-iis-api-web-application-required) above |
| `check_config` works but compile/search fail | Atelier web app `Recurse=0` | Management Portal → Security → Web Apps → `/api/atelier` → enable **Recurse** |
| All tools fail, namespace listing works | API version mismatch | Verify IRIS supports Atelier v8 (`iris-agentic-dev --verbose` shows detected version) |
| 403 on write operations | Insufficient permissions | Use a user with `%DB_USER` or `%All` role |
| Connection delays on Windows | `localhost` DNS issue | Use `host = "127.0.0.1"` in `.iris-agentic-dev.toml` |
| `SERVER_MANAGER_CREDENTIAL_ERROR` | Credential not in OS keychain | VS Code → Server Manager → right-click server → **Reconnect** |
| `SERVER_MANAGER_AMBIGUOUS` | Multiple SM servers, no `IRIS_SERVER_NAME` | Set `IRIS_SERVER_NAME=<server-key>` (see `check_config` for available names) |

For verbose HTTP logging:

```bash
iris-agentic-dev mcp --verbose 2>debug.log
```

A 404 on `/api/atelier/v8/...` usually indicates the Recurse setting or a missing `/api` web application. A 401/403 is an authentication issue. Connection refused means the host or port is wrong.

---

## Commands

```bash
iris-agentic-dev mcp                     # Start the MCP server
iris-agentic-dev compile MyApp.Foo.cls   # Compile from the terminal
iris-agentic-dev init                    # Generate .iris-agentic-dev.toml from running containers
iris-agentic-dev install                 # Install packages from iris-dev.toml
iris-agentic-dev benchmark --skill <path> --baseline   # Run the skill benchmark harness
iris-agentic-dev --version               # Print version
```

---

## Documentation

| Guide | Contents |
|-------|----------|
| [docs/connecting.md](docs/connecting.md) | Native IRIS, Docker, Server Manager, policy gates, env vars, discovery order |
| [docs/tools.md](docs/tools.md) | Full tool catalog with descriptions and error codes |
| [docs/skills.md](docs/skills.md) | Skill inventory, benchmark results, install instructions |
| [docs/troubleshooting.md](docs/troubleshooting.md) | Symptom table, CLI commands, verbose logging |

---

## Contributing

Issues and pull requests are welcome. File bugs at the [Issues tab](https://github.com/intersystems-community/iris-agentic-dev/issues).

To contribute a skill — write a `SKILL.md`, run the benchmark, submit a PR with your results. See [BENCHMARKING.md](./skills/BENCHMARKING.md).

Questions: [thomas.dyar@intersystems.com](mailto:thomas.dyar@intersystems.com)
