# Troubleshooting

---

## Symptom table

| Symptom                                                    | Likely cause                                                     | Fix                                                                                   |
| ---------------------------------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| 404 on `/api/atelier` (Windows)                            | IIS missing `/api` web application                               | See [Windows IIS setup](connecting.md#windows-iis-api-web-application-required)       |
| `check_config` works but compile/search fail               | Atelier web app `Recurse=0`                                      | Management Portal → Security → Web Apps → `/api/atelier` → enable **Recurse**         |
| `Mapping not found for %Service_WebGateway//mcp/path`      | CSP application not registered in IRIS                           | See [WebGateway: CSP app registration](#webgateway-csp-application-mapping-not-found) |
| All tools fail, `check_config` shows `atelier_rest: false` | NoPWS build (IRIS 2026.2.0AI+) or no WebGateway                  | Set `docker_only = true` in `.iris-agentic-dev.toml`                                  |
| All tools fail, namespace listing works                    | API version mismatch                                             | Verify IRIS supports Atelier v8 (`iris-agentic-dev --verbose` shows detected version) |
| 403 on write operations                                    | Insufficient permissions                                         | Use a user with `%DB_USER` or `%All` role                                             |
| Connection delays on Windows                               | `localhost` DNS issue                                            | Use `host = "127.0.0.1"` in `.iris-agentic-dev.toml`                                  |
| `SERVER_MANAGER_CREDENTIAL_ERROR`                          | Credential not in OS keychain                                    | VS Code → Server Manager → right-click server → **Reconnect**                         |
| `SERVER_MANAGER_AMBIGUOUS`                                 | Multiple SM servers, no `IRIS_SERVER_NAME`                       | Set `IRIS_SERVER_NAME=<server-key>` (see `check_config` for available names)          |
| `STALE_CONTENT` from `iris_doc`                            | `expected` text doesn't match current file                       | Re-fetch the document (`mode=get`) and retry with current content                     |
| `SCOPE_REQUIRED` from `iris_search`                        | Search called with no document scope                             | Pass at least one category or document type in `scope`                                |
| `CODE_EDIT_BLOCKED`                                        | Attempted write to `%Dictionary`, `$SYSTEM.OBJ`, or code globals | Use `iris_doc` (put) + `iris_compile` instead                                         |
| `CHECKIN_BLOCKED` from `iris_source_control`               | CheckIn disabled by default                                      | Set `IRIS_SCM_ALLOW_CHECKIN=1` to enable                                              |
| `HTTP_EXECUTION_FAILED` from `iris_execute`                | Atelier execution failed and no Docker fallback                  | Verify Atelier endpoint reachable; set `IRIS_CONTAINER` for Docker fallback           |
| `IRIS_UNREACHABLE`                                         | No IRIS connection discoverable                                  | Run `check_config` to see discovery state; check host/port/credentials                |

---

## Verbose HTTP logging

```bash
iris-agentic-dev mcp --verbose 2>debug.log
```

A 404 on `/api/atelier/v8/...` usually indicates the `Recurse` setting or a missing `/api`
web application. A 401/403 is an authentication issue. Connection refused means the host or
port is wrong.

---

## Connection and config verification

Run `check_config` from your AI assistant:

```text
Call check_config and show me the result.
```

Or from the terminal:

```bash
iris-agentic-dev tool check_config --args '{}'
```

The output shows active connection state, which discovery source won, and the status of
each optional feature (Server Manager, containers, write gates).

---

## CLI commands

```bash
iris-agentic-dev mcp                     # Start the MCP server
iris-agentic-dev compile MyApp.Foo.cls   # Compile from the terminal
iris-agentic-dev init                    # Generate .iris-agentic-dev.toml from running containers
iris-agentic-dev install                 # Install packages from iris-dev.toml
iris-agentic-dev benchmark --skill <path> --baseline   # Run the skill benchmark harness
iris-agentic-dev --version               # Print version
```

### Shortcut subcommands

Run any IRIS operation directly from the terminal — no MCP client or AI session needed.

| Subcommand | Example                                                                 | What it does                                                               |
| ---------- | ----------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `exec`     | `iris-agentic-dev exec 'write $ZVersion,!'`                             | Execute ObjectScript inline, from `--file`, or from stdin (`-`)            |
| `compile`  | `iris-agentic-dev compile MyApp.Foo.cls`                                | Compile one or more `.cls`/`.mac` files; prints `OK:` or `ERROR:` per file |
| `query`    | `iris-agentic-dev query 'SELECT Name FROM %Dictionary.ClassDefinition'` | Execute SQL; prints TSV (header + rows) to stdout                          |
| `doc`      | `iris-agentic-dev doc get MyApp.Foo`                                    | Read IRIS document UDL; `doc put MyApp.Foo --file f.cls` to write          |
| `tool`     | `iris-agentic-dev tool iris_info --args '{"what":"version"}'`           | Call any MCP tool by name without an MCP client                            |

All shortcuts accept: `--host`, `--web-port`, `--namespace`, `--username`, `--password`,
`--container`. Env vars (`IRIS_HOST`, `IRIS_WEB_PORT`, etc.) are also honored.

```bash
# Print IRIS version
iris-agentic-dev exec 'write $ZVersion,!'

# Execute a file
iris-agentic-dev exec --file myscript.cos

# Pipe script via stdin
echo 'write $namespace,!' | iris-agentic-dev exec -

# Compile with explicit connection
iris-agentic-dev compile MyApp.MyClass.cls --host myserver --namespace PROD

# Query a different namespace
iris-agentic-dev query --namespace %SYS 'SELECT Name FROM Security.Users'

# Read a class definition
iris-agentic-dev doc get %Dictionary.ClassDefinition --namespace %SYS

# Upload a class
iris-agentic-dev doc put MyApp.Foo --file MyApp.Foo.cls

# Call any tool
iris-agentic-dev tool check_config --args '{}'
```

---

## WebGateway: CSP application mapping not found

Error: `Mapping not found for %Service_WebGateway//mcp/yourpath`

Two separate IRIS objects must be configured — both are required:

**1. `%Service_WebGateway` auth** — allows the gateway to reach IRIS at all.
Already covered by the `iris-webgateway-setup` skill. Symptom when missing: `403 Forbidden`.

**2. `Security.Applications` path registration** — maps each URL path to a namespace.
Symptom when missing: `Mapping not found` even though the gateway connects fine.

```objectscript
// Run in iris session IRIS -U %SYS
Set path = "/mcp/yourpath"
Set props("AutheEnabled")    = 96   // MUST be 96, not 64 — 64 returns HTTP 500
Set props("Enabled")         = 1
Set props("NameSpace")       = "USER"
Set props("Type")            = 0
Set sc = ##class(Security.Applications).Create(path, .props)
Write sc,!  // 1 = success
```

Verify:

```bash
echo 'write ##class(Security.Applications).Exists("/mcp/yourpath"),! halt' \
  | docker exec -i <container> runuser -u irisowner -- iris session IRIS -U '%SYS'
```

Full setup guide: `iris-webgateway-setup` skill (`~/.claude/skills/iris-webgateway-setup/SKILL.md`).

---

## NoPWS builds (IRIS 2026.2.0AI and later enterprise images)

All IRIS enterprise builds `2026.2.0AI.*` have no private web server (DPP-1192).
Port 52773 published from the container is dead — nothing is listening.

`check_config` detects this automatically from the version string and reports
`capabilities.atelier_rest: false` and `capabilities.compile_path: "docker_exec"`.
`iris_compile` routes to `docker exec` immediately — no 52773 probe, no retry.

**Required config** — Atelier REST is unavailable, so a container name is needed
for `docker exec` to work:

```toml
# .iris-agentic-dev.toml
docker_only = true
container = "your-container-name"
```

Community IRIS images (`iris-community:*`) still have PWS on 52773 — unaffected.

---

## Getting help

Issues and pull requests: [GitHub Issues](https://github.com/intersystems-community/iris-agentic-dev/issues)

Questions: [thomas.dyar@intersystems.com](mailto:thomas.dyar@intersystems.com)
