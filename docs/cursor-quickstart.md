# Cursor IDE / Cursor CLI quick start

This guide covers running **iris-agentic-dev** (iad) as an MCP server inside
**Cursor**. Cursor is a VS Code fork, but its agent MCP path is **not** the same
as the VS Code + GitHub Copilot extension experience.

If you only need Copilot in VS Code, use the
[Marketplace extension](https://marketplace.visualstudio.com/items?itemName=intersystems-community.vscode-iris-agentic-dev)
instead — see the [README quick start](../README.md#quick-start-vs-code--github-copilot).

---

## Cursor vs VS Code + Copilot (read this first)

|                                                   | **VS Code + Copilot**                                                                 | **Cursor (IDE / CLI)**                                                                                                                                                                                            |
| ------------------------------------------------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| How MCP is registered                             | Extension implements `McpServerDefinitionProvider`; Copilot picks it up automatically | **`~/.cursor/mcp.json`** (manual). Cursor does not use the Copilot tool panel                                                                                                                                     |
| Binary install                                    | Extension auto-downloads into VS Code global storage                                  | Install the binary yourself (Homebrew, release tarball, or build from source) and put it on `PATH` (or use an absolute `command`)                                                                                 |
| Connection config                                 | Often zero-config via ObjectScript `objectscript.conn` / Server Manager               | Prefer **`.iris-agentic-dev.toml`** (project or home) + `--workspace`                                                                                                                                             |
| Server Manager + OS keychain                      | Documented zero-config path when credentials resolve                                  | **Often fails on Linux** (`KEYCHAIN_FAILED` / empty password → HTTP 401). ObjectScript sidebar login uses a different store and can still work while MCP does not                                                 |
| `server="<Server Manager name>"`                  | May work when keychain resolves                                                       | Prefer **fleet** names from `[instance.*]` in toml (`source: "fleet"` in `iris_servers`). SM short names frequently 401 in Cursor remote/Linux sessions                                                           |
| Skills install (`iris-agentic-dev skill install`) | Copilot / Claude Code / OpenCode targets                                              | **No Cursor/`*.mdc` target** yet. Optional: `npx skills add intersystems-community/iris-agentic-dev`                                                                                                              |
| Tool catalog size                                 | Full list                                                                             | Full list is fine on current releases (large `outputSchema` payloads that caused Cursor to show **0 tools** were fixed — see issue [#113](https://github.com/intersystems-community/iris-agentic-dev/issues/113)) |

**Bottom line:** treat Cursor as a **stdio MCP client** (like Claude Code), not as
“install the VSIX and you’re done.” Installing the VS Code extension _inside_
Cursor is optional and **untested** as a substitute for `mcp.json`.

---

## 1. Install the binary

Same options as the [main install section](../README.md#install). Homebrew when
available; otherwise curl the release binary (common on Linux / shared hosts).

### Mac (Homebrew)

```bash
brew tap intersystems-community/tap
brew install iris-agentic-dev
```

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

### Linux arm64

```bash
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-linux-aarch64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
```

If `/usr/local/bin` is not writable, install under your home directory instead
(for example `~/.local/bin/iris-agentic-dev`) and point `command` in `mcp.json`
at that absolute path.

### Windows

Download `iris-agentic-dev-windows-x86_64.exe` from the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
and place it on your PATH.

Confirm:

```bash
iris-agentic-dev --version
```

---

## 2. Wire Cursor MCP (`~/.cursor/mcp.json`)

Create or merge:

```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "type": "stdio",
      "command": "iris-agentic-dev",
      "args": ["mcp", "--workspace", "/home/YOU"]
    }
  }
}
```

Notes:

- Prefer an **absolute path** for `command` if the binary is not on the PATH that
  Cursor’s MCP process inherits (common with GUI-launched Cursor).
- `--workspace` should be the directory that contains `.iris-agentic-dev.toml`
  (often your home directory for a personal fleet file, or a project root).
- After editing `mcp.json`, **restart the iris-agentic-dev MCP server** in
  Cursor Settings → MCP (or reload the window).

Equivalent layout for Cursor CLI / agent sessions: the same `mcp.json` is used
when the CLI loads your Cursor MCP config.

---

## 3. Configure IRIS (recommended: toml)

### Simple (single instance)

In the `--workspace` directory, create `.iris-agentic-dev.toml`:

```toml
host = "localhost"
web_port = 52773
scheme = "http"
namespace = "USER"
username = "_SYSTEM"
password = "SYS"
```

Use `https` / port `443` for TLS gateways. Do **not** commit real passwords.

### Fleet / operate mode (multiple instances, `server=` routing)

For several IRIS namespaces or hosts behind one MCP session:

```toml
mode = "operate"

# Default when a tool omits `server`
host = "gateway.example.com"
web_port = 443
scheme = "https"
namespace = "DEV"
username = "your.user"
password = "your-password"

[instance.dev]
host = "gateway.example.com"
web_port = 443
scheme = "https"
namespace = "DEV"
username = "your.user"
password = "your-password"
role = "workspace"

[instance.prod]
host = "gateway.example.com"
web_port = 443
scheme = "https"
namespace = "PROD"
username = "your.user"
password = "your-password"
role = "subject"
```

Then call tools with an optional `server` argument:

```text
server = "dev"    # routes to [instance.dev]
(omit server)     # uses top-level host/namespace
```

**Verify the pool:**

1. Call `iris_servers`.
2. Confirm fleet entries show `source: "fleet"`.
3. Call `iris_query` (or similar) with `server="<instance>"` and confirm the
   namespace matches that instance — **without** editing top-level `host` /
   `namespace`.

This guide assumes a build that includes the fleet pool fix
([#123](https://github.com/intersystems-community/iris-agentic-dev/issues/123) /
[#124](https://github.com/intersystems-community/iris-agentic-dev/pull/124)):
`load_fleet_config` must accept the toml **file** path so `[instance.*]` joins
the connection pool. On older builds, `iris_servers` may show no `source: "fleet"`
rows and `server=` returns `SERVER_NOT_FOUND`; upgrade before relying on named
routing.

**Pool source priority (first name wins):** `iad-native` → VS Code/Cursor Server
Manager → fleet `[instance.*]` → env. If the same name exists in two sources,
the earlier source wins. Prefer distinct fleet names (for example a common
prefix) so they are not shadowed by Server Manager or native entries.

**Credentials in toml:** values are **literal**. There is no `${ENV}` expansion.
Omitting top-level `password` can fall back to `IRIS_PASSWORD` for the **default**
connection only; `[instance.*]` passwords are not filled from the environment
today. For team sharing, ship a **secret-free** stub (hosts, namespaces, instance
names) and have each developer add credentials locally — or generate a private
file from a template outside iad.

**Role gates:** in `mode = "operate"`, `role = "subject"` blocks write tools unless
confirmed per tool docs. Role matching uses the **default** connection for the
gate in some paths; prefer aligning top-level namespace with the instance you
write against when unsure.

---

## 4. Sanity checks

Ask the agent (or call tools directly):

1. **`check_config`** — `connected`, `host`, `namespace`, `config_file`,
   `server_version`, `write_tools_enabled`.
2. **`iris_servers`** — list names and `source` (`fleet` vs `vscode` vs
   `iad-native`).
3. **`iris_query`** with `SELECT $Namespace` — with and without `server=`.

Expect on the order of **~70+ tools** enabled when no `IRIS_ENABLED_TOOLS`
allowlist is set. If Cursor shows **0 tools**, update past the `#113`
`outputSchema` strip, remove any accidental empty allowlist, and restart MCP.

---

## 5. What not to rely on in Cursor (yet)

These are documented or convenient in **VS Code + Copilot**, but are weak or
broken for many Cursor setups:

1. **Marketplace extension as the only setup** — Copilot auto-registration does
   not replace `~/.cursor/mcp.json` for Cursor Agent.
2. **Server Manager keychain as the credential store for MCP** — especially on
   Linux / remote SSH. `check_config` may list SM servers with
   `credential_status: "error"`; `server="<sm-name>"` then 401s. Put credentials
   in `.iris-agentic-dev.toml` (or a working keychain setup you have verified).
3. **`iris_add_server` / `iris_import_servers` / `server=` SM names** without
   verifying keychain resolution first.
4. **`iris-agentic-dev skill install --agent cursor`** — not implemented; use
   community skill pack install if you need Cursor rules.
5. **TOML `${ENV}` placeholders for per-developer passwords** — not supported;
   do not put secret expansion syntax in shared files expecting iad to resolve it.

---

## 6. Cursor CLI notes

- Use the same binary and the same `~/.cursor/mcp.json`.
- Ensure the CLI process can resolve `command` (absolute path is safest).
- `--workspace` still controls which `.iris-agentic-dev.toml` is loaded; agents
  should not rewrite that file when `server=` fleet routing is available.

---

## Related docs

- [Connecting to IRIS](connecting.md)
- [Tools reference](tools.md) (`server` parameter on many tools)
- [Ecosystem integration](ecosystem-integration.md) (operate / fleet overview)
- [Troubleshooting](troubleshooting.md)
- VS Code extension README: [`vscode-iris-agentic-dev/README.md`](../vscode-iris-agentic-dev/README.md)
