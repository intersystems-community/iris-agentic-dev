# iris-agentic-dev

An MCP server that connects AI coding assistants — GitHub Copilot, Claude Code, OpenCode, and others — directly to a live InterSystems IRIS instance. The AI can compile classes, run ObjectScript, execute SQL, search the namespace, run unit tests, and inspect class definitions without leaving the chat.

Requires IRIS 2023.1 or later with the Atelier REST API enabled.

---

## Skills — AI that knows ObjectScript

iris-agentic-dev ships a library of benchmark-validated skills: concise instruction files that teach your AI assistant ObjectScript-specific patterns, common mistakes, and IRIS-specific behaviors. They work independently of the MCP server and improve results measurably.

Tested with Claude Sonnet 4.6 on 41 tasks from real ISC codebases:

| Benchmark suite | Baseline | With top skill | Lift |
|-----------------|----------|----------------|------|
| ObjectScript repair (22 tasks) | 73% | **100%** | +27% |
| Multi-file repair (5 tasks) | 80% | **100%** | +20% |
| IRIS SQL quirks (14 tasks) | 93% | **100%** | +7% |

The top-ranked skill is **`objectscript-review`** — a 205-word checklist that catches the 10 most common ObjectScript mistakes before the AI shows you any code.

### Install the top skills (60 seconds)

**Claude Code:**
```bash
mkdir -p ~/.claude/skills
for skill in objectscript-review objectscript-guardrails objectscript-sql-patterns; do
  mkdir -p ~/.claude/skills/$skill
  curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/light-skills/skills/$skill/SKILL.md \
    > ~/.claude/skills/$skill/SKILL.md
done
```

**OpenCode:**
```bash
mkdir -p ~/.config/opencode/skills
for skill in objectscript-review objectscript-guardrails objectscript-sql-patterns; do
  mkdir -p ~/.config/opencode/skills/$skill
  curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/light-skills/skills/$skill/SKILL.md \
    > ~/.config/opencode/skills/$skill/SKILL.md
done
```

**VS Code Copilot:** Install the extension (see below) — skills are wired in automatically.

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
| `objectscript-tdd` | Compile-test-fix development loop | |

See [`light-skills/`](./light-skills/) for the full list, benchmark results, and instructions for contributing your own skill.

> **Note on load-on-demand skills**: some skills hurt if loaded globally. `objectscript-loop-patterns` measured −19% lift when loaded for all tasks. Domain skills (`iris-vector-ai`, `iris-connectivity`, `ensemble-production`) should only be loaded when working in those areas. See [BENCHMARKING.md](./light-skills/BENCHMARKING.md).

---

## Installation

### VS Code extension (recommended)

The VS Code extension wires iris-agentic-dev directly into Copilot Agent Mode, reading your existing `objectscript.conn` and `intersystems.servers` configuration.

1. Download `vscode-iris-agentic-dev-*.vsix` from the [releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
2. In VS Code: Extensions (`Ctrl+Shift+X`) → `...` → **Install from VSIX**
3. Reload VS Code — **iris-agentic-dev (IRIS)** appears in Copilot Chat → Agent mode → tools

No additional configuration needed if you already use the InterSystems VS Code extensions.

If the [InterSystems Server Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.servermanager) extension is installed, credentials are retrieved from the OS keychain. Click the 🔑 icon at the password prompt to store credentials for future sessions.

### Binary (Claude Code, OpenCode, other MCP clients)

**Homebrew (Mac):**
```bash
brew tap intersystems-community/iris-agentic-dev
brew install iris-agentic-dev
```

**Direct download:**
```bash
# Mac Apple Silicon
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-macos-arm64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
xattr -d com.apple.quarantine /usr/local/bin/iris-agentic-dev 2>/dev/null

# Mac Intel: replace "arm64" with "x86_64"

# Linux x86_64
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-linux-x86_64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
```

**Windows:** Download `iris-agentic-dev-windows-x86_64.exe` from the [releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest) and place it on your PATH.

---

## MCP server setup

### Claude Code

Add to `~/.claude/settings.json`:

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

If IRIS is running in Docker, iris-agentic-dev can discover the container automatically:

```bash
iris-agentic-dev init    # writes .iris-agentic-dev.toml from your running containers
```

With a `.iris-agentic-dev.toml` in your workspace root, you can omit the `env` block from the config above.

### OpenCode

Add to `~/.config/opencode/config.json`:

```json
{
  "mcp": {
    "iris-agentic-dev": {
      "type": "local",
      "command": ["/opt/homebrew/bin/iris-agentic-dev", "mcp"],
      "enabled": true,
      "environment": {
        "IRIS_HOST": "your-iris-host",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "SYS",
        "IRIS_NAMESPACE": "USER"
      }
    }
  }
}
```

Note: OpenCode uses `"type": "local"` and `"environment"` rather than `"type": "stdio"` and `"env"`.

After restarting OpenCode, call `check_config` in a session to verify the connection.

**WSL2:** The Windows OpenCode GUI cannot spawn Linux ELF binaries. If you see `iris-agentic-dev failed` in the MCP list, either use the Windows binary or invoke the Linux binary via `wsl.exe`:

```json
"command": ["wsl.exe", "-e", "/usr/local/bin/iris-agentic-dev", "mcp"]
```

---

## Configuration

### `.iris-agentic-dev.toml`

Per-project config. Commit it so the whole team uses the same connection without environment variable setup.

```toml
# Docker container
container = "myapp-iris"
namespace = "MYAPP"

# Remote IRIS (use instead of container)
# host = "iris.example.com"
# web_port = 52773
# scheme = "https"
# web_prefix = "irisaicore"    # for non-root web gateway paths
```

Generate from running containers: `iris-agentic-dev init`

### Connection discovery order

iris-agentic-dev resolves the IRIS connection in this order — first match wins:

1. CLI flags (`--host`, `--web-port`, `--scheme`)
2. `.iris-agentic-dev.toml` in the workspace root
3. Environment variables (`IRIS_HOST`, etc.)
4. VS Code `settings.json` (`objectscript.conn` / `intersystems.servers`)
5. Running Docker containers (scored by workspace name similarity)
6. Localhost port scan (52773, 41773, 51773, 8080)

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `IRIS_HOST` | `localhost` | IRIS web gateway hostname |
| `IRIS_WEB_PORT` | `52773` | Web gateway port |
| `IRIS_SCHEME` | `http` | `http` or `https` |
| `IRIS_WEB_PREFIX` | _(empty)_ | URL path prefix for non-root gateway installs |
| `IRIS_USERNAME` | `_SYSTEM` | IRIS username |
| `IRIS_PASSWORD` | `SYS` | IRIS password |
| `IRIS_NAMESPACE` | `USER` | Default namespace |
| `IRIS_CONTAINER` | _(empty)_ | Docker container name — required for Docker-dependent tools |
| `OBJECTSCRIPT_WORKSPACE` | `$PWD` | Workspace root for `.iris-agentic-dev.toml` lookup |

### Enterprise containers

Enterprise IRIS images (`intersystems/iris`, `intersystems/irishealth`) ship with `WebServer=0`. Run the ISC Web Gateway container alongside IRIS:

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

See the [`iris-vscode-objectscript` skill](./light-skills/skills/iris-vscode-objectscript/SKILL.md) for a working `webgateway-init.sh`.

---

## Tools

Most tools work over the Atelier REST API and connect to any IRIS instance. Tools marked ✦ require `IRIS_CONTAINER` to be set.

**Code**

| Tool | What it does |
|------|-------------|
| `iris_compile` | Compile a class, routine, or wildcard. Returns errors with line numbers. |
| `iris_doc` | Read, write, delete, or check any IRIS document. |
| `iris_execute` | Run ObjectScript, return output. |
| `iris_query` | Execute SQL, return rows as JSON. |
| `iris_test` | Run `%UnitTest` tests, return structured pass/fail results. |
| `iris_source_control` ✦ | Check lock status, checkout, execute SCM actions. |

**Search and introspection**

| Tool | What it does |
|------|-------------|
| `iris_symbols` | Search classes and methods via `%Dictionary`. |
| `docs_introspect` | Deep class inspection: methods, properties, XData, superclasses. |
| `iris_search` | Full-text search across the namespace. Supports regex and category filters. |
| `iris_info` | Namespace discovery: documents, jobs, CSP apps, metadata. |
| `iris_macro` | Macro inspection: list, signature, definition, expand. |

**Debugging**

| Tool | What it does |
|------|-------------|
| `iris_debug` | Map INT offsets to source lines, fetch error logs, capture error state. |
| `iris_get_log` | Retrieve a full result by `log_id` when a tool returns `truncated: true`. |
| `check_config` | Show active connection state — host, container, config file, write tool status. |

**Generation**

| Tool | What it does |
|------|-------------|
| `iris_generate` | Build a context-rich prompt for generating ObjectScript. No API key required. |
| `iris_generate_class` | Generate and compile a class from a description (requires LLM API key). |
| `iris_generate_test` | Generate `%UnitTest` scaffolding for an existing class. |

**Interoperability** ✦

| Tool | What it does |
|------|-------------|
| `iris_production` | Start, stop, update, check, or recover a production. |
| `iris_interop_query` | Query production logs, queue depths, or message archive. |

**Administration**

| Tool | What it does |
|------|-------------|
| `iris_admin` | List namespaces, databases, users, roles, web apps; create/delete users (requires `IRIS_ADMIN_TOOLS=1`). |
| `iris_containers` ✦ | List, select, or start IRIS Docker containers. Hot-swaps the active connection without a session restart. |

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `check_config` works but compile/search fail | Atelier web app `Recurse=0` | Management Portal → Security → Web Apps → `/api/atelier` → enable **Recurse** |
| All tools fail, namespace listing works | API version mismatch | Verify IRIS supports Atelier v8 (`iris-agentic-dev --verbose` shows detected version) |
| 403 on write operations | Insufficient permissions | Use a user with `%DB_USER` or `%All` role |

For verbose HTTP logging:

```bash
iris-agentic-dev mcp --verbose 2>debug.log
```

A 404 on `/api/atelier/v8/...` usually indicates the Recurse setting. A 401/403 is an authentication issue. Connection refused means the host or port is wrong.

---

## Commands

```bash
iris-agentic-dev mcp                     # Start the MCP server
iris-agentic-dev compile MyApp.Foo.cls   # Compile from the terminal
iris-agentic-dev init                    # Generate .iris-agentic-dev.toml from running containers
iris-agentic-dev --version               # Print version
```

---

## Contributing

Issues and pull requests are welcome. File bugs at the [Issues tab](https://github.com/intersystems-community/iris-agentic-dev/issues).

To contribute a skill — write a `SKILL.md`, run the benchmark, submit a PR with your results. See [BENCHMARKING.md](./light-skills/BENCHMARKING.md).

Questions: [thomas.dyar@intersystems.com](mailto:thomas.dyar@intersystems.com)
