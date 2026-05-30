# iris-agentic-dev

Connect GitHub Copilot, Claude Code, OpenCode, or any MCP-compatible AI assistant directly to a live InterSystems IRIS instance. Your AI can compile, test, search, read, write, and debug ObjectScript without leaving the chat.

---

## How it works

iris-agentic-dev runs as a local MCP (Model Context Protocol) server. Your AI assistant calls its tools — `iris_compile`, `iris_doc`, `iris_execute`, etc. — and iris-dev executes them against your real IRIS instance over the Atelier REST API. The AI sees compile errors, class definitions, and execution output in-line, the same way it would with a local filesystem.

---

## Quick start — pick your setup

### Option A: IRIS in Docker (local dev)

```bash
# 1. Install iris-agentic-dev (Mac Apple Silicon)
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-macos-arm64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
xattr -d com.apple.quarantine /usr/local/bin/iris-agentic-dev 2>/dev/null

# 2. Let iris-agentic-dev find your container automatically
iris-agentic-dev init              # writes .iris-agentic-dev.toml from your running containers

# 3. Add to Claude Code (~/.claude/settings.json)
```
```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "command": "iris-agentic-dev",
      "args": ["mcp"],
      "env": { "OBJECTSCRIPT_WORKSPACE": "${workspaceFolder}" }
    }
  }
}
```

### Option B: Remote or server IRIS (no Docker)

```bash
# Set connection via env vars — no .iris-agentic-dev.toml needed
```
```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "command": "iris-agentic-dev",
      "args": ["mcp"],
      "env": {
        "IRIS_HOST": "iris.example.com",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "SYS",
        "IRIS_NAMESPACE": "MYAPP"
      }
    }
  }
}
```

For HTTPS or a non-root web gateway path:
```json
"IRIS_SCHEME": "https",
"IRIS_WEB_PORT": "443",
"IRIS_WEB_PREFIX": "irisaicore"
```

### Option C: VS Code Copilot Agent Mode

1. Install the binary (see [Installation](#installation) below)
2. Download `vscode-iris-agentic-dev-*.vsix` from the [releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
3. In VS Code: Extensions (`Ctrl+Shift+X`) → `...` → **Install from VSIX**
4. Reload VS Code — **iris-agentic-dev (IRIS)** appears automatically in Copilot Chat → Agent mode → tools

The extension reads your existing `objectscript.conn` and `intersystems.servers` config — no extra setup if you already use the InterSystems VS Code extensions.

### Option D: OpenCode

OpenCode uses `~/.config/opencode/config.json` with an `mcp` section. The format differs from Claude Code in a few key ways:

| Setting | Claude Code `settings.json` | OpenCode `config.json` |
|---------|----------------------------|------------------------|
| Section key | `mcpServers` | `mcp` |
| Server type | `"type": "stdio"` | `"type": "local"` |
| Credentials | `"env": {...}` | `"environment": {...}` |
| Enable flag | not needed | `"enabled": true` required |

Add this to `~/.config/opencode/config.json`:

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

Replace `your-iris-host` with your IRIS hostname (use `localhost` for a local instance). For Homebrew-installed binaries, the path is `/opt/homebrew/bin/iris-agentic-dev`. For a manual install, use the full path to the binary.

**With a Docker container** — add `IRIS_CONTAINER` to the `environment` block to enable tools that need direct container access:

```json
"environment": {
  "IRIS_HOST": "localhost",
  "IRIS_WEB_PORT": "52773",
  "IRIS_USERNAME": "_SYSTEM",
  "IRIS_PASSWORD": "SYS",
  "IRIS_NAMESPACE": "USER",
  "IRIS_CONTAINER": "my-iris-container"
}
```

**Using `.iris-agentic-dev.toml` instead** — you can omit the `environment` block entirely and put connection settings in a workspace config file instead. See [Workspace config (.iris-agentic-dev.toml)](#workspace-config-iris-agentic-devtoml) below.

**Verify the connection** — after restarting OpenCode, call the `check_config` tool in a session. It should return `"connected": true`.

**WSL2 setup** — WSL2 has two distinct configurations depending on whether you're running OpenCode from Windows or from inside WSL2:

| Setup | OpenCode location | Binary to use | IRIS_HOST |
|-------|------------------|---------------|-----------|
| OpenCode Windows GUI | Windows process | Windows binary (`.exe`) | `localhost` (with mirrored networking) or Windows host IP |
| OpenCode TUI inside WSL2 | Linux process | Linux binary | `localhost` (with mirrored networking) or `$(cat /etc/resolv.conf \| grep nameserver \| awk '{print $2}')` |

**Important**: The Windows OpenCode GUI process cannot spawn Linux ELF binaries directly — even if the path looks like a WSL path. If you see `iris-agentic-dev failed` in the OpenCode MCP list when using the Windows GUI, the binary path is probably pointing to the Linux binary. Fix: use the Windows binary path:

```json
"command": ["C:\\Users\\yourname\\bin\\iris-agentic-dev.exe", "mcp"]
```

Or invoke the Linux binary via `wsl.exe` from the Windows config:

```json
"command": ["wsl.exe", "-e", "/usr/local/bin/iris-agentic-dev", "mcp"]
```

With mirrored networking (`networkingMode = mirrored` in `.wslconfig`), `localhost` works transparently in both directions — no need to find the Windows host IP.

#### Troubleshooting

**MCP tools not triggering / "failure connecting" errors**

Most connection issues trace to one of these:

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `check_config` works but compile/search fail | Atelier web app `Recurse=0` | Management Portal → Security → Web Apps → `/api/atelier` → enable **Recurse** |
| All tools fail, namespace listing works | API version mismatch | Check your IRIS version supports v8 (`iris-agentic-dev --verbose` shows which version was detected) |
| 403 errors on write operations | User lacks write permissions | Use a user with `%DB_USER` or `%All` role |
| MCP works in CLI/TUI but not in GUI | OpenCode GUI beta issue | Use the CMD/TUI interface; report to the OpenCode team |

**Diagnosing with `--verbose`**

Run with verbose logging to see the exact HTTP calls:

```bash
iris-agentic-dev mcp --verbose 2>debug.log
# Trigger a failing tool in OpenCode
cat debug.log
```

The log shows which URL is being called and the HTTP status code. A 404 on `/api/atelier/v8/...` usually means the Recurse setting; a 401/403 is authentication; a connection refused means the host/port is wrong.

---

## Installation

### Mac

```bash
# Apple Silicon (M1/M2/M3):
sudo mkdir -p /usr/local/bin
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-macos-arm64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
xattr -d com.apple.quarantine /usr/local/bin/iris-agentic-dev 2>/dev/null

# Intel Mac: replace "arm64" with "x86_64" above
```

### Linux

```bash
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-linux-x86_64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
```

### Windows

1. Download `iris-dev-windows-x86_64.exe` from the [releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
2. Save it somewhere permanent, e.g. `C:\Users\yourname\bin\iris-agentic-dev.exe`
3. In VS Code User Settings (JSON), set the binary path:
```json
"iris-agentic-dev.serverPath": "C:\\Users\\yourname\\bin\\iris-dev.exe"
```

> **WSL2**: Use the Windows binary. Set `IRIS_HOST` to the Windows host IP — `localhost` in WSL2 resolves to the Linux VM, not the Windows host.

---

## Tools

iris-agentic-dev exposes 23 tools to your AI assistant:

| Tool | Needs Docker? | What it does |
|------|:---:|-------------|
| `iris_compile` | — | Compile a class, routine, or wildcard (`MyApp.*.cls`). Returns errors with line numbers. |
| `iris_execute` | — | Run arbitrary ObjectScript and return output. |
| `iris_query` | — | Execute SQL, return rows as JSON. |
| `iris_doc` | — | Read, write, delete, or check any IRIS document. SCM checkout handled via chat dialog. |
| `iris_symbols` | — | Search classes and methods via `%Dictionary`. |
| `docs_introspect` | — | Deep class inspection: methods, properties, XData, superclasses. |
| `iris_search` | — | Full-text search across the namespace. Supports regex and category filters. |
| `iris_info` | — | Namespace discovery: documents, jobs, CSP apps, metadata. |
| `iris_macro` | — | Macro inspection: list, signature, definition, expand. |
| `iris_debug` | — | Map INT errors to source lines, fetch error logs, capture error state. |
| `iris_generate` | — | Build a context-rich prompt for the AI to generate ObjectScript. No API key needed. |
| `iris_generate_class` | — | Generate and compile a class from a description (requires LLM API key). |
| `iris_generate_test` | — | Generate `%UnitTest` scaffolding for an existing class. |
| `iris_source_control` | ✓ | Check lock status, checkout, execute SCM actions. |
| `iris_test` | — | Run `%UnitTest` tests and return structured pass/fail results. Works over HTTP with or without `IRIS_CONTAINER`. |
| `iris_production` | ✓ | Start, stop, update, check, or recover an Interoperability production. |
| `iris_interop_query` | ✓ | Query production logs, queue depths, or message archive. |
| `iris_containers` | ✓ | List, select, or start IRIS Docker containers. `iris_select_container` now hot-swaps the active connection — no session restart required. |
| `iris_admin` | — | IRIS administration: list namespaces, databases, users, roles, web apps; check permissions; create/delete users, namespaces, webapps (requires `IRIS_ADMIN_TOOLS=1`). |
| `iris_get_log` | — | Retrieve a stored result by `log_id` from the progressive disclosure store. With `id`: returns the full result (paginated with `limit`/`offset`). Without `id`: lists all stored log entries. Use when a tool returns `truncated: true`. |
| `check_config` | — | Inspect active IRIS connection state — host, container, config file, last loaded time, write tools status. Always succeeds; never returns `IRIS_UNREACHABLE`. Use to diagnose connection issues or verify hot-reload completed. |
| `skill` | ✓ | Manage the local skills registry (list, describe, search, forget). |
| `skill_community` | ✓ | Browse community skills. |
| `kb` | ✓ | Index markdown files into a searchable knowledge base. |

Tools marked **✓ Needs Docker** require `IRIS_CONTAINER` to be set. Tools without the mark work over Atelier REST and work with any IRIS instance — local or remote.

---

## Configuration reference

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `IRIS_HOST` | `localhost` | IRIS web gateway hostname |
| `IRIS_WEB_PORT` | `52773` | Web gateway port |
| `IRIS_SCHEME` | `http` | `http` or `https` |
| `IRIS_WEB_PREFIX` | _(empty)_ | URL path prefix (e.g. `irisaicore` for `/irisaicore/api/atelier/`) |
| `IRIS_USERNAME` | `_SYSTEM` | IRIS username |
| `IRIS_PASSWORD` | `SYS` | IRIS password |
| `IRIS_NAMESPACE` | `USER` | Default namespace |
| `IRIS_CONTAINER` | _(empty)_ | Docker container name — required for Docker-dependent tools |
| `OBJECTSCRIPT_WORKSPACE` | `$PWD` | Workspace root for `.iris-agentic-dev.toml` lookup |
| `IRIS_LOG_STORE_MAX` | `50` | Max entries in the progressive disclosure log store. Oldest entry evicted when full. |
| `IRIS_LOG_TTL_MINUTES` | `60` | Minutes before a log entry expires. Expired entries return `LOG_EXPIRED`. |
| `IRIS_INLINE_COMPILE` | `20` | `iris_compile`: max distinct error/warning entries returned inline before truncation. |
| `IRIS_INLINE_SEARCH` | `30` | `iris_search`: max result entries returned inline before truncation. |
| `IRIS_INLINE_INFO` | `30` | `iris_info` (what=documents): max document entries returned inline before truncation. |
| `IRIS_INLINE_ERROR_LOGS` | `20` | `debug_get_error_logs`: max log entries returned inline before truncation. |

### `.iris-agentic-dev.toml` (per-project config)

Drop this file in your project root and commit it so teammates get the same setup automatically.

```toml
# Local Docker container
container = "myapp-iris"
namespace = "MYAPP"

# Remote IRIS (alternative to Docker)
# host = "iris.example.com"
# web_port = 52773
# scheme = "https"          # for TLS
# web_prefix = "irisaicore" # for non-root gateway path
```

Generate from your running containers: `iris-agentic-dev init`

### Enterprise containers (intersystems/iris, intersystems/irishealth)

Enterprise images ship with `WebServer=0` — no private web server. The standard solution is the ISC Web Gateway container alongside IRIS. iris-agentic-dev auto-detects it.

```yaml
# docker-compose snippet
services:
  iris:
    image: containers.intersystems.com/intersystems/iris:2026.1
    ports: ["4972:1972"]
  webgateway:
    image: containers.intersystems.com/intersystems/webgateway:2026.1
    ports: ["52773:80"]            # iris-agentic-dev scans port 52773
    entrypoint: ["/bin/sh", "/init.sh"]
    volumes: ["./webgateway-init.sh:/init.sh:ro"]
```

Three non-obvious setup gotchas in fresh containers — see [`iris-vscode-objectscript` skill](./light-skills/skills/iris-vscode-objectscript/SKILL.md) for the complete working `webgateway-init.sh`.

### Connection discovery order

iris-agentic-dev resolves the connection in this order — first match wins:

1. CLI flags (`--host`, `--web-port`, `--scheme`)
2. `.iris-agentic-dev.toml` in the workspace root
3. Environment variables (`IRIS_HOST`, etc.)
4. VS Code `settings.json` (`objectscript.conn` / `intersystems.servers`)
5. Docker containers (scored by workspace name similarity)
6. Localhost port scan (52773, 41773, 51773, 8080)

### VS Code: Server Manager integration

If you use the InterSystems VS Code extensions, iris-agentic-dev reads your server definitions automatically. Your `objectscript.conn` should reference a named server so the full definition (including `pathPrefix` for non-standard gateways) is picked up:

```json
"objectscript.conn": { "active": true, "server": "your-server-name" }
```

If iris-agentic-dev can't find your server: `View > Output > iris-agentic-dev` shows which servers were found and where.

**Secure credential storage**: If the [InterSystems Server Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.servermanager) extension is installed, iris-agentic-dev uses it to retrieve credentials from the OS keychain. On first use you'll be prompted for username and password. When the password prompt appears, **click the 🔑 key icon** before pressing Enter to store it in the keychain — subsequent VS Code restarts will then be fully silent. Pressing Enter without clicking 🔑 uses the password for the current session only. Server Manager is optional; without it iris-agentic-dev falls back to credentials in your server definition or MCP env vars.

---

## Commands

```bash
iris-agentic-dev mcp           # Start the MCP server (used by Claude Code / Copilot)
iris-agentic-dev compile MyApp.Foo.cls   # Compile from the terminal
iris-agentic-dev init          # Generate .iris-agentic-dev.toml from running containers
iris-agentic-dev --version     # Check version
```

---

## Contributing

Issues and PRs welcome. File bugs at the **Issues** tab — visible to the team and helps prioritization.

Questions or urgent issues: [thomas.dyar@intersystems.com](mailto:thomas.dyar@intersystems.com)
