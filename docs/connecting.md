# Connecting to IRIS

iris-agentic-dev connects to IRIS via the Atelier REST API — the same API the VS Code
ObjectScript extension uses. No special IRIS configuration is required beyond what you
already have for VS Code.

**If something isn't working, call `check_config` first** — it shows exactly which
connection source won, what host/port/namespace was resolved, and whether Atelier REST
is available. No IRIS network calls are made; it always succeeds.

```text
Call check_config and show me the result.
```

Or from the terminal:

```bash
iris-agentic-dev tool check_config --args '{}'
```

---

## Native IRIS on Windows or Linux (no Docker)

Run `iris-agentic-dev init` in your project root to generate a documented
`.iris-agentic-dev.toml` template with all available options:

```bash
iris-agentic-dev init
```

Or create it manually:

Add a `.iris-agentic-dev.toml` file to your project root:

```toml
host = "localhost"
web_port = 80        # IIS default for IRIS 2024.1+; use 52773 for pre-2024.1
namespace = "USER"
username = "_SYSTEM"
password = "SYS"
```

### Port reference

| IRIS version        | Web server               | Default port |
| ------------------- | ------------------------ | ------------ |
| 2024.1+ on Windows  | IIS                      | 80           |
| 2024.1+ on Linux    | Apache                   | 80           |
| Pre-2024.1 (any OS) | Private Web Server (PWS) | 52773        |

### Windows IIS: `/api` web application required

This is the most common failure on Windows. IIS needs an explicit `/api` web application
mapped to the IRIS Web Gateway module. Without it, `/api/atelier` returns 404 — even when
the Management Portal loads correctly.

**To fix:**

1. Open **IIS Manager** → expand your server → **Sites** → **Default Web Site**
2. Right-click → **Add Application**. Set alias: `api`, physical path:
   `C:\InterSystems\IRIS\CSP\bin` (adjust to your install path)
3. Add a wildcard script handler mapping: executable = `CSPms.dll`, no verb restriction
4. Verify `CSP.ini` contains an `[APP_PATH:/api]` section

See the [`iris-windows-iis-setup` skill](../light-skills/skills/iris-windows-iis-setup/SKILL.md)
for full step-by-step instructions with verification commands.

**`localhost` vs `127.0.0.1`**: On some older Web Gateway builds, using `localhost` causes
a brief connection error before each request. If you see connection delays, change the
config to `host = "127.0.0.1"`.

---

## Docker (community image)

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

---

## Docker (enterprise image)

Enterprise IRIS images (`intersystems/iris`, `intersystems/irishealth`) ship without a
built-in web server. Run the ISC Web Gateway container alongside IRIS:

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

See the [`iris-vscode-objectscript` skill](../light-skills/skills/iris-vscode-objectscript/SKILL.md)
for a working `webgateway-init.sh`.

---

## VS Code Server Manager (zero-config)

If the [InterSystems Server Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.servermanager)
extension is installed, iris-agentic-dev reads your server list from VS Code's
`settings.json` and resolves credentials from the OS keychain automatically — no
`.iris-agentic-dev.toml` needed.

<!-- SCREENSHOT: ../docs/images/server-manager-sidebar.png
     Show the VS Code Explorer sidebar (or the InterSystems panel — the purple ISC icon in
     the activity bar). Expand the ObjectScript or InterSystems Servers tree so at least one
     server entry is visible with its hostname/port and namespace shown. If the server shows
     a green connected indicator or a lock icon, include that. Right-click the server so the
     context menu is visible — show "Add Server", "Edit Settings", and "Reconnect" items.
     This illustrates both where servers are defined and how to reconnect if credentials
     are stale. Crop to the sidebar panel only. -->

![InterSystems Server Manager sidebar showing a connected server](../docs/images/server-manager-sidebar.png)

**Single server configured:** auto-connects, no extra setup.

**Multiple servers configured:** set `IRIS_SERVER_NAME` to the map key from
`intersystems.servers`:

```bash
export IRIS_SERVER_NAME=dev-local
```

Credentials are stored under keychain service `"intersystems-server-credentials"` — the
auth provider ID used by Server Manager in all VS Code-compatible forks (Cursor, Windsurf,
VS Code Insiders). If a credential is missing, iris-agentic-dev fails fast with a message
directing you to reconnect in VS Code (right-click the server → **Reconnect**) rather than
silently falling through to other discovery sources.

Use `check_config` to see which servers were detected and whether credentials resolved:

```json
{
  "server_manager": {
    "available": true,
    "servers": [{ "name": "dev-local", "active": true, "credential_status": "resolved" }]
  }
}
```

---

## Per-connection policy (fleet / operate mode)

Add `[policy.<server-name>]` blocks to `.iris-agentic-dev.toml` to restrict which tool
categories are permitted on a given Server Manager server:

```toml
[policy.prod]
allow = ["query", "search", "docs"]
```

Blocked calls return `error_code: "POLICY_GATE"` with the list of allowed categories.
Omit the block entirely to permit everything. Available categories: `compile`, `execute`,
`query`, `search`, `docs`, `source_control`, `debug`, `admin`, `skill`, `kb`.

For multi-instance fleet workflows (`mode = "operate"`), see the
[fleet roles spec](../specs/003-workspace-config/) for the full `[instance.*]` config
format and role-gate behavior.

---

## Connection discovery order

iris-agentic-dev resolves the IRIS connection in this order — first match wins:

1. CLI flags (`--host`, `--web-port`, `--scheme`)
2. `.iris-agentic-dev.toml` in the workspace root
3. Environment variables (`IRIS_HOST`, etc.)
4. VS Code `settings.json` (`objectscript.conn` / `intersystems.servers`)
5. VS Code Server Manager keychain (`intersystems.servers` + OS keychain credential)
6. Running Docker containers (scored by workspace name similarity)
7. Localhost port scan (52773, 41773, 51773, 8080)

---

## Environment variables

| Variable                   | Default     | Description                                                                  |
| -------------------------- | ----------- | ---------------------------------------------------------------------------- |
| `IRIS_HOST`                | `localhost` | IRIS web gateway hostname                                                    |
| `IRIS_WEB_PORT`            | `52773`     | Web gateway port                                                             |
| `IRIS_SCHEME`              | `http`      | `http` or `https`                                                            |
| `IRIS_WEB_PREFIX`          | _(empty)_   | URL path prefix for non-root gateway installs                                |
| `IRIS_USERNAME`            | `_SYSTEM`   | IRIS username                                                                |
| `IRIS_PASSWORD`            | `SYS`       | IRIS password                                                                |
| `IRIS_NAMESPACE`           | `USER`      | Default namespace                                                            |
| `IRIS_CONTAINER`           | _(empty)_   | Docker container name — required for Docker-dependent tools                  |
| `IRIS_SERVER_NAME`         | _(empty)_   | Server Manager server name when multiple are configured                      |
| `OBJECTSCRIPT_WORKSPACE`   | `$PWD`      | Workspace root for `.iris-agentic-dev.toml` lookup                           |
| `IRIS_SEARCH_SYNC_TIMEOUT` | `30`        | Seconds to wait for synchronous search before falling back to async polling  |
| `IRIS_DISABLED_TOOLS`      | _(empty)_   | Comma-separated tool names to exclude, e.g. `iris_source_control,iris_admin` |

---

## Global config file

Credentials you don't want to repeat in every project can go in a global config
file. Project-local `.iris-agentic-dev.toml` always takes precedence.

| Platform    | Path                                          |
| ----------- | --------------------------------------------- |
| Mac / Linux | `~/.config/iris-agentic-dev/config.toml`      |
| Windows     | `%USERPROFILE%\.iris-agentic-dev\config.toml` |

```toml
# Global defaults — apply to every project that has no local .toml
username = "_SYSTEM"
password = "SYS"
```

---

## Restricting writes on shared servers

Set `write_tools_enabled = false` in `.iris-agentic-dev.toml` to put the server
in read-only mode — compile, execute, doc-write, source control, and global-write
tools all return a clear error instead of modifying anything. Query, search, and
doc-read tools continue to work.

```toml
# .iris-agentic-dev.toml on a shared dev or production server
host = "shared-iris"
web_port = 52773
namespace = "USER"
username = "_SYSTEM"
password = "SYS"
write_tools_enabled = false
```

This is the recommended default for any server that more than one person connects
to, or any server that isn't purely local.
