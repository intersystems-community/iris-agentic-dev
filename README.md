# iris-agentic-dev

Connect GitHub Copilot, Claude Code, and other AI coding assistants directly to a live
InterSystems IRIS instance. The AI can compile classes, run ObjectScript, execute SQL,
search the namespace, run unit tests, and inspect class definitions — without leaving the
chat.

Works with IRIS installed natively on Windows or Linux, and with Docker. Requires
IRIS 2023.1 or later.

---

## Quick start: VS Code + GitHub Copilot

This is the fastest path if you already use VS Code with the InterSystems ObjectScript
extension.

**Prerequisites**: VS Code, GitHub Copilot,
[InterSystems ObjectScript extension](https://marketplace.visualstudio.com/items?itemName=intersystems-community.vscode-objectscript)

1. Download `vscode-iris-agentic-dev-*.vsix` from the
   [releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
2. In VS Code: Extensions (`Ctrl+Shift+X`) → `...` → **Install from VSIX**
3. Reload VS Code

**iris-agentic-dev (IRIS)** now appears in **Copilot Chat → Agent mode → tools**. It reads
your existing `objectscript.conn` or `intersystems.servers` configuration — no additional
setup needed.

![iris-agentic-dev tools visible in the Copilot Configure Tools panel](docs/images/copilot-tools-panel.png)

To verify the connection, ask Copilot: *"Call check_config and show me the result."*

![check_config result showing connected: true, auto-discovered connection, and IRIS version](docs/images/check-config-result.png)

> **Windows users**: iris-agentic-dev works with native IRIS on Windows — Docker is not
> required. If you hit a 404 on `/api/atelier`, see the
> [Windows IIS setup](docs/connecting.md#windows-iis-api-web-application-required) guide.

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

**Windows**: Download `iris-agentic-dev-windows-x86_64.exe` from the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
and place it on your PATH.

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

**WSL2**: The Windows OpenCode GUI cannot spawn Linux ELF binaries. Use the Windows `.exe`
or invoke the Linux binary via `wsl.exe`:

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

See the [`iris-windows-iis-setup` skill](./light-skills/skills/iris-windows-iis-setup/SKILL.md) for full step-by-step instructions with verification commands.

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

See the [`iris-vscode-objectscript` skill](./light-skills/skills/iris-vscode-objectscript/SKILL.md) for a working `webgateway-init.sh`.

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
| `IRIS_SERVICE_USERNAME` | *(empty)* | Restricted service account for arbitrary-execution tools (see below) |
| `IRIS_SERVICE_PASSWORD` | *(empty)* | Password for `IRIS_SERVICE_USERNAME` |
| `IRIS_NAMESPACE` | `USER` | Default namespace |
| `IRIS_CONTAINER` | *(empty)* | Docker container name — required for Docker-dependent tools |
| `IRIS_SERVER_NAME` | *(empty)* | Server Manager server name when multiple are configured |
| `OBJECTSCRIPT_WORKSPACE` | `$PWD` | Workspace root for `.iris-agentic-dev.toml` lookup |

### Privilege separation for arbitrary execution

`iris_execute`, `iris_execute_method`, `iris_query` (`mode="write"`), and `iris_global`
(`set`/`kill`) can run arbitrary ObjectScript/SQL. Under a `%All` account these can edit class
and routine code — even by indirection (`$classmethod`, `$method("%Sa"_"ve")`, `xecute`) —
bypassing the SCM lock and the `CODE_EDIT_BLOCKED` string filter, since no static text filter
can be exhaustive against a fully-privileged identity.

Set `IRIS_SERVICE_USERNAME` / `IRIS_SERVICE_PASSWORD` to a **least-privilege** IRIS account
(no `%Development` resource, code database mounted read-only). Those four tools then authenticate
as that account, so code edits fail with `<PROTECT>` at the IRIS privilege layer regardless of
indirection. Code-writing tools (`iris_document` put, `iris_source_control`, `iris_compile`)
deliberately keep using the primary `IRIS_USERNAME`, so SCM checkouts and audit stay attributed
to the real user. When unset, all tools use the primary connection (unchanged behaviour).

---

## Skills — improve AI output for ObjectScript

Skills are concise instruction files that teach your AI assistant ObjectScript patterns and
common mistakes. The top skill (`objectscript-review`) brings the repair benchmark from 73%
to **100%** on 22 tasks.

```bash
mkdir -p ~/.claude/skills
for skill in objectscript-review objectscript-guardrails objectscript-sql-patterns; do
  mkdir -p ~/.claude/skills/$skill
  curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/light-skills/skills/$skill/SKILL.md \
    > ~/.claude/skills/$skill/SKILL.md
done
```

See [docs/skills.md](docs/skills.md) for the full skill inventory, benchmark results, and
loading cautions.

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

Issues and pull requests are welcome at the
[Issues tab](https://github.com/intersystems-community/iris-agentic-dev/issues).

To contribute a skill — write a `SKILL.md`, run the benchmark, submit a PR with results.
See [BENCHMARKING.md](./light-skills/BENCHMARKING.md).

Questions: [thomas.dyar@intersystems.com](mailto:thomas.dyar@intersystems.com)
