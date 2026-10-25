# Troubleshooting

---

## Symptom table

| Symptom                                                                                              | Likely cause                                                           | Fix                                                                                           |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| 404 on `/api/atelier` (Windows)                                                                      | IIS missing `/api` web application                                     | See [Windows IIS setup](connecting.md#windows-iis-api-web-application-required)               |
| `check_config` works but compile/search fail                                                         | Atelier web app `Recurse=0`                                            | Management Portal → Security → Web Apps → `/api/atelier` → enable **Recurse**                 |
| `Mapping not found for %Service_WebGateway//mcp/path`                                                | CSP application not registered in IRIS                                 | See [WebGateway: CSP app registration](#webgateway-csp-application-mapping-not-found)         |
| All tools fail, `check_config` shows `atelier_rest: false`                                           | NoPWS build (IRIS 2026.2.0AI+) or no WebGateway                        | Set `docker_only = true` in `.iris-agentic-dev.toml`                                          |
| All tools fail, namespace listing works                                                              | API version mismatch                                                   | Verify IRIS supports Atelier v8 (`iris-agentic-dev --verbose` shows detected version)         |
| 403 on write operations                                                                              | Insufficient permissions                                               | Use a user with `%DB_USER` or `%All` role                                                     |
| Connection delays on Windows                                                                         | `localhost` DNS issue                                                  | Use `host = "127.0.0.1"` in `.iris-agentic-dev.toml`                                          |
| `SERVER_MANAGER_CREDENTIAL_ERROR`                                                                    | Credential not in OS keychain                                          | VS Code → Server Manager → right-click server → **Reconnect**                                 |
| `SERVER_MANAGER_CREDENTIAL_ERROR` on Windows, after Reconnect                                        | Secret storage unseal failed                                           | See [Windows: Server Manager credentials](#windows-server-manager-credentials-cannot-be-read) |
| `SERVER_MANAGER_AMBIGUOUS`                                                                           | Multiple SM servers, no `IRIS_SERVER_NAME`                             | Set `IRIS_SERVER_NAME=<server-key>` (see `check_config` for available names)                  |
| `STALE_CONTENT` from `iris_doc`                                                                      | `expected` text doesn't match current file                             | Re-fetch the document (`mode=get`) and retry with current content                             |
| `SCOPE_REQUIRED` from `iris_search`                                                                  | Search called with no document scope                                   | Pass at least one category or document type in `scope`                                        |
| `CODE_EDIT_BLOCKED`                                                                                  | Touched `%Dictionary`, `$SYSTEM.OBJ`, or a code global — read or write | Read with `iris_doc` (get) or `iris_symbols`; write with `iris_doc` (put) + `iris_compile`    |
| `CHECKIN_BLOCKED` from `iris_source_control`                                                         | CheckIn disabled by default                                            | Set `IRIS_SCM_ALLOW_CHECKIN=1` to enable                                                      |
| `SCM_ADMIN_BLOCKED` from `iris_source_control`                                                       | Disconnect and Reconnect disabled by default                           | Set `IRIS_SCM_ALLOW_DISCONNECT=1` to enable (separate from the CheckIn switch)                |
| `HTTP_EXECUTION_FAILED` from `iris_execute`                                                          | Atelier execution failed and no Docker fallback                        | Verify Atelier endpoint reachable; set `IRIS_CONTAINER` for Docker fallback                   |
| `IRIS_UNREACHABLE`                                                                                   | No IRIS connection discoverable                                        | Run `check_config` to see discovery state; check host/port/credentials                        |
| `FILE_NOT_FOUND` from `iris_compile` or `compile`                                                    | Local file path does not exist on disk                                 | Check the file path — the compile command requires the file to exist locally before uploading |
| `UNKNOWN_PARAMETER`                                                                                  | Call passed a parameter the tool does not declare                      | Use one of the names the error lists; see [Unknown parameters](#unknown-parameters)           |
| `INVALID_ACTION`                                                                                     | `action`/`mode`/`what` value outside the tool's enum                   | Read the enum out of the tool's `inputSchema`; see [Unknown parameters](#unknown-parameters)  |
| `SESSION_STALE` on the CLI token `iris_ws_open` just printed                                         | Each `tool` call is its own process; the session died with the first   | Run open/exec/close as one [`batch` script](#session_stale-after-iris_ws_open-in-the-cli)     |
| `WRITE_TOOLS_DISABLED`, but the project's `.iris-agentic-dev.toml` says `write_tools_enabled = true` | The project file is not trusted, or the server is Live                 | See [Write tools are disabled in my project](#write-tools-are-disabled-in-my-project)         |
| `SERVICE_ACCOUNT_*`                                                                                  | The agent's code could not run as the service account                  | See [`SERVICE_ACCOUNT_*` errors](#service_account_-errors)                                    |
| `READ_ONLY_STRICT`                                                                                   | `read_only = "strict"` is set; the tool needs a scratch class          | See [`READ_ONLY_STRICT`](#read_only_strict)                                                   |
| `CONFIG_INVALID`                                                                                     | A configuration file does not parse                                    | See [`CONFIG_INVALID`](#config_invalid)                                                       |
| `NAME_CONFLICT` from `iris_add_server`                                                               | The name is a Server Manager profile                                   | See [`NAME_CONFLICT`](#name_conflict)                                                         |
| A call hangs, a keychain dialog is on screen                                                         | The OS keychain is waiting for approval                                | See [The keychain prompt](#the-keychain-prompt-blocks-a-call)                                 |
| `journal_search` returns `truncated: true`                                                           | The walk hit its record or time cap                                    | See [`journal_search` is truncated](#journal_search-is-truncated)                             |

---

## Write tools are disabled in my project

Symptom: a project's `.iris-agentic-dev.toml` says `write_tools_enabled = true` (or
`destructive_tools_enabled = true`, or `[policy.<name>] writeTools = true`), and a write tool
answers `WRITE_TOOLS_DISABLED` (or `DESTRUCTIVE_TOOLS_DISABLED`). At startup iad printed one line
on stderr: `iris-agentic-dev: 1 setting set aside (...)`.

A project file comes with the repository, so iad does not let it widen what the agent may do
until you accept it. Call `check_config` and read three fields:

- `config_sources.workspace_file.trust`: `untrusted` (never trusted), `lapsed` (trusted once, then
  changed; `changed` lists each setting with its old and new value), or `trusted`.
- `set_aside`: each setting iad read and did not act on, with its `reason` and the `lift` that
  would make it act. A write refused for this reason also carries `refusal.kind:
"untrusted_setting"` and the item's id in `refusal.set_aside`.
- `write_tools_source`: what decided the tier. `inferred_system_mode` means no trusted file
  declared one and the server's own mode did.

Fixes, in order of likelihood:

1. **The file is untrusted or its trust lapsed.** Read what trusting would apply, then trust it
   yourself at a terminal:

   ```bash
   iris-agentic-dev trust --show
   iris-agentic-dev trust
   ```

   Trust covers the exact loosening settings and the server address each applies to, so any
   later change to them, or pointing the file at another host, lapses it again. The agent cannot
   do this step for you, and you should not let it run `trust` in a shell you auto-approve; see
   [Trust](connecting.md#trust).

2. **The server is Live.** With nothing declared, a Live or Failover instance has its write tier
   off and a `live` template, whatever a project file says. Declare the tier in
   `~/.iris-agentic-dev.toml` (`[policy.<name>] writeTools = true`), or trust the project file.
   If you do not want writes there, this is the setting working.

3. **The project names a host your environment's credentials may not go to.** Your looser
   launch settings stay behind with the credentials (`set_aside` lists them as
   `credential_withheld`). Put the credentials in the file or point `IRIS_HOST` at that server.

4. **A launch setting says no.** `IRIS_WRITE_TOOLS_ENABLED=0` outranks every file, and
   `read_only = "strict"` removes the scratch-class tools whatever the tier.

5. **A file does not parse.** Both tiers close and `write_tools_source` is `fail_closed`; see
   [`CONFIG_INVALID`](#config_invalid).

The same applies to every loosening setting. A project's `tls_verify = false`, `ssh_host`,
`service_username`, `compile_time_gate = false`, `dataPolicy = "allow"` or `mcpTemplate = "dev"`
is set aside until you trust the file.

---

## `SERVICE_ACCOUNT_*` errors

With `IRIS_SERVICE_USERNAME` set, or a `service_username` on a server, the tools that run the
agent's code (`iris_execute`, `iris_query` writes, `iris_global` set/kill, `iris_execute_method`,
`global_kill`, `iris_test`, `iris_coverage` run/start, `iris_ws_open` and `iris_ws_exec`) run as
that account on every path, or the call is refused. They never fall back to the connection
account. `check_config.identity` lists the user each path would run as and each server's account.

| Code                                                                                              | Cause                                                                                                                                               | Fix                                                                                                                                                                                              |
| ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER`                                                         | `IRIS_SERVICE_USERNAME` is set and the named server has no account of its own                                                                       | Give the server a `service_username` in `servers.json`, `iris_add_server` or `~/.iris-agentic-dev.toml`                                                                                          |
| `SERVICE_ACCOUNT_WITHHELD`                                                                        | A project file named a host your environment's credentials may not go to, so the service account is withheld from it                                | Put credentials in the file, or point `IRIS_HOST` at that server                                                                                                                                 |
| `SERVICE_ACCOUNT_HTTP_DENIED`                                                                     | Atelier REST refused the account                                                                                                                    | Grant `%Development:USE` and database access; see [the layout that works](connecting.md#service-account-for-the-agents-code). iad does not fall back to `docker exec`, which would run as `%All` |
| `SERVICE_ACCOUNT_TERMINAL_DENIED`                                                                 | The terminal logged in as the account, then printed `Access Denied`                                                                                 | Grant `%Development:USE` and read access to the namespace's databases                                                                                                                            |
| `SERVICE_ACCOUNT_LOGIN_FAILED`                                                                    | The docker or SSH terminal could not log in as the account (it must exist, be enabled, and the container's OS user must be allowed to log in as it) | Create or enable the account, or opt in to the OS user (below)                                                                                                                                   |
| `DOCKER_TERMINAL_LOGIN_REQUIRED`                                                                  | The container's terminal asks for a user name and password, which iad does not send                                                                 | Enable operating-system authentication for `%Service_Terminal`, or use the Atelier path                                                                                                          |
| `SERVICE_ACCOUNT_IDENTITY_MISMATCH`                                                               | IRIS reported a different `$USERNAME` than the account iad asked for                                                                                | Check how the instance maps the OS user; iad refuses output it cannot attribute                                                                                                                  |
| `SESSION_IDENTITY_CHANGED`                                                                        | You changed or removed the service account while a WebSocket session was open                                                                       | Open a new session with `iris_ws_open`                                                                                                                                                           |
| `SERVICE_ACCOUNT_INVALID`                                                                         | The configured user name is empty or holds a control character                                                                                      | Fix the name                                                                                                                                                                                     |
| `NO_DOCKER_ROUTE_FOR_SERVER` (the `no_docker_route` field of an `HTTP_EXECUTION_FAILED` response) | A named server's HTTP path failed, and it has no `container` of its own to fall back to                                                             | Give the server a `container`, or fix the HTTP path. It never falls back to the default connection's container                                                                                   |
| `COVERAGE_PRIVILEGE_DENIED`                                                                       | `iris_coverage` `run`/`start` as an account without write access to the IRISSYS database's resource                                                 | Grant it, or accept that the line monitor is not available to that account                                                                                                                       |

**Running as the container's OS user instead.** Where the terminal login guard cannot work, the
operator can opt in with `IAD_EXEC_AS_OS_USER=1` in the environment that launches iad, or
`exec_as_os_user = true` (all servers) or `[policy.<name>] execAsOsUser = true` (one server) in
`~/.iris-agentic-dev.toml`. The response then says `"source": "os_user_opt_in"`. A project file
cannot set it; `check_config` lists the attempt under `identity.os_user_opt_in.ignored`. Treat it
as giving the agent the container's `%All` access.

---

## `READ_ONLY_STRICT`

Symptom: a read tool such as `iris_table_info`, `my_access`, `iris_production`, `journal_search`
or `iris_execute` answers `READ_ONLY_STRICT`, and is missing from `tools/list`.

`read_only = "strict"` (a configuration file) or `IAD_READ_ONLY=strict` (launch settings) is
set; `check_config.read_only_from` says which. Under strict, iad writes nothing to the server, so
every tool that runs its ObjectScript through a scratch class in the `IrisDevTmp` package is off,
and the write and destructive gates are closed on every server, whatever opens them elsewhere (a
write such as `iris_doc` put answers `READ_ONLY_STRICT` with `gate: "write_tools"`). `iris_doc`
reads, `iris_query`, `iris_search`, `iris_info`, `iris_servers` and `check_config` still work. To use the others, remove the setting (any file may set strict, and
a project file cannot unset it from the operator file). Strict together with `irisAudit = true` is
a configuration error, because the audit emission compiles a class: `iris-agentic-dev mcp` names
both settings on stderr and exits 2. See
[Strict read-only](connecting.md#strict-read-only-no-scratch-classes).

---

## `CONFIG_INVALID`

Symptom: every tool that talks to IRIS answers `CONFIG_INVALID`, with the `file`, `line` and
`column` of the problem, and writes are off.

A configuration file does not parse, or a value has the wrong type (`web_port = "abc"`). iad will
not fall back to discovery with a broken file: that would attach the agent to whichever IRIS
answers first with gates nobody chose. Fix the file at the position given; iad reads it again on
the next call, with no restart. `check_config` still works and lists the same text in
`config_parse_error` and `config_errors`. The file may be the operator file
(`~/.iris-agentic-dev.toml`), the project's, or the one `--config` names.

---

## `NAME_CONFLICT`

Symptom: `iris_add_server` answers `NAME_CONFLICT`.

The name is the name of a Server Manager profile in VS Code's `intersystems.servers`. An iad
entry with that name would be read as the profile and handed its password. Choose another name,
or run `iris_import_servers` to link the profile: the imported entry is marked
`credential = "server-manager"` and its password stays in the Server Manager's keychain.

---

## The keychain prompt blocks a call

Symptom: on macOS a call hangs, and a Keychain dialog (often behind another window) asks whether
`iris-agentic-dev` may use an item. It can recur after an upgrade, when the keychain sees a new binary.

iad keeps passwords under its own service, `iris-agentic-dev`, and reads the Server Manager's
`intersystems-server-credentials` item without writing to it. Choose **Always Allow** once. On a
host with nobody at the screen, or in CI, set `IAD_NO_KEYCHAIN=1` in the environment that launches
iad: it never touches the keychain, and passwords come from `IRIS_PASSWORD` or from `servers.json`
(owner-only, tied to the address they were saved for). A password saved for one address is not
found for another; if a moved server stopped authenticating, re-register it with a password.
In `check_config`, a Server Manager profile whose item could not be read has
`credential_status: "keychain_unavailable"`.

---

## `journal_search` is truncated

Symptom: `journal_search`, or `iris_admin` with `action="journal_search"`, answers
`truncated: true` and fewer records than you expect.

The search walks the current journal file from its first record and stops at 200,000 records read
or 20 seconds, whichever comes first, or at the result cap (`max_entries`, at most 500;
`max_records`, at most 1000). `truncated_reason` says which: `scan_cap`, `time_cap` or
`max_records`. `scanned` is how many records it read. **A truncated answer has not seen the rest
of the file**, so a missing record is not proof that nothing was journaled. Narrow `start`/`end`
(or `time_range`) and `global_pattern`, and read `scanned` to see how far it got. If instead the
call fails with `JOURNAL_SEARCH_TIMEOUT`, the walk outran the 30-second HTTP timeout; the error
names the journal file and its size. A refusal with `DATA_POLICY_BLOCKED` means the target
server's `dataPolicy` is not `allow`.

---

## Unknown parameters

Every tool declares its parameters and accepts nothing else. A name the tool does not
declare comes back as `UNKNOWN_PARAMETER`, with the full accepted list and a spelling
suggestion when the name is one edit away:

```json
{
  "success": false,
  "error_code": "UNKNOWN_PARAMETER",
  "error": "stream_inspect does not accept \"max_chars\". Accepted parameters: namespace, oid, server."
}
```

```json
{
  "success": false,
  "error_code": "UNKNOWN_PARAMETER",
  "error": "global_preview does not accept \"globl\". Accepted parameters: count, global, server. Did you mean \"global\"?"
}
```

The check runs before the tool does, so a rejected call changes nothing on the server. It
runs _after_ the write and destructive gates: with writes disabled, a misspelled key on
`global_kill` is answered by the gate, because the security refusal is the more important
one.

Through 1.3.2, 31 tools advertised an open object with no declared properties and dropped
unrecognised keys in silence. If a call that used to appear to work now fails with
`UNKNOWN_PARAMETER`, it was never doing what the parameter name suggested —
`stream_inspect(max_chars=10000)` returned the entire stream, every time.

A tool that reads a fixed set of values for `action`, `mode`, or `what` advertises them as
a JSON Schema `enum`. Anything outside the list is `INVALID_ACTION`, and the enum in the
tool's `inputSchema` is the authoritative list. `iris_admin.action` has 25 values; the
current set for every tool is in [docs/tools.md](tools.md).

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
iris-agentic-dev skill install           # Install official skill pack (all agents)
iris-agentic-dev skill install objectscript-review --agent claude-code
iris-agentic-dev skill list              # Show install status across agents
iris-agentic-dev skill status            # Show managed vs user-authored skill files
iris-agentic-dev --version               # Print version
```

### Shortcut subcommands

Run any IRIS operation directly from the terminal — no MCP client or AI session needed.

| Subcommand | Example                                                                 | What it does                                                                |
| ---------- | ----------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `exec`     | `iris-agentic-dev exec 'write $ZVersion,!'`                             | Execute ObjectScript inline, from `--file`, or from stdin (`-`)             |
| `compile`  | `iris-agentic-dev compile MyApp.Foo.cls`                                | Compile one or more `.cls`/`.mac` files; prints `OK:` or `ERROR:` per file  |
| `query`    | `iris-agentic-dev query 'SELECT Name FROM %Dictionary.ClassDefinition'` | Execute SQL; prints TSV (header + rows) to stdout                           |
| `doc`      | `iris-agentic-dev doc get MyApp.Foo`                                    | Read IRIS document UDL; `doc put MyApp.Foo --file f.cls` to write           |
| `tool`     | `iris-agentic-dev tool iris_info --args '{"what":"version"}'`           | Call any MCP tool by name without an MCP client                             |
| `batch`    | `iris-agentic-dev batch --file steps.json`                              | Run a list of tool calls in one process, so a session survives between them |

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

# Read a class definition — connection flags go before `get`/`put`, which are their own subcommands
iris-agentic-dev doc --namespace %SYS get %Dictionary.ClassDefinition

# Upload a class
iris-agentic-dev doc put MyApp.Foo --file MyApp.Foo.cls

# Call any tool
iris-agentic-dev tool check_config --args '{}'
```

---

## `SESSION_STALE` after `iris_ws_open` in the CLI

A WebSocket session is a live connection, and iad holds it in a pool in memory. The pool belongs to
one process, and `iris-agentic-dev tool` dispatches a single call and exits — so it takes the
session with it, and the token it printed refers to nothing:

```bash
iris-agentic-dev tool iris_ws_open --args '{"namespace":"USER"}'
```

Through 1.4.2 that succeeded and printed a token, and the next command answered
`SESSION_STALE: Session token references an unknown server or expired session`. Nothing had
expired. The session never reached the second process, and the message named a timeout that had not
happened. The three session tools are refused under `tool` now, with a pointer to what follows.

`batch` runs a list of tool calls in one process against one connection, so the pool survives from
step to step. `{{0.session}}` reads the `session` field out of step 0's response:

```json
[
  { "tool": "iris_ws_open", "args": { "namespace": "USER" } },
  {
    "tool": "iris_ws_exec",
    "args": { "session": "{{0.session}}", "code": "Set x=42" }
  },
  {
    "tool": "iris_ws_exec",
    "args": { "session": "{{0.session}}", "code": "Write x" }
  },
  { "tool": "iris_ws_close", "args": { "session": "{{0.session}}" } }
]
```

```bash
iris-agentic-dev batch --file ws-session.json
```

Every step prints `[<index>] <tool>: <response>`, and a step that reports failure stops the batch
and exits 1. Omit `--file` to read the script from stdin. A placeholder is
`{{<step-index>.<field>}}` and has to be the entire argument value, so the referenced field arrives
as whatever JSON type it really is rather than as a string.

`iris_get_log` and `iris_doc`'s source-control checkout prompt keep their state in the same
per-process way, so they need `batch` for the same reason.

When nothing has to persist between calls, `exec` is the simpler tool:

```bash
iris-agentic-dev exec 'Set x=42 Write x'
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

## Windows: Server Manager credentials cannot be read

Symptom: tools fail with `SERVER_MANAGER_CREDENTIAL_ERROR` on Windows even though the
server connects fine inside VS Code, and Reconnect does not help.

Start with the diagnostic — it prints every step of the unseal and stops at the one that
fails:

```powershell
iris-agentic-dev check-sm-credential <server-name> <username>
```

A healthy run looks like this:

```text
state.vscdb: C:\Users\you\AppData\Roaming\Code\User\globalStorage\state.vscdb
Value encoding: BufferJson
Payload is a safeStorage AES-GCM envelope (45 bytes)
Local State: C:\Users\you\AppData\Roaming\Code\Local State
OK: resolved password for iservice-base/_system
```

### How the secret is actually stored

Worth knowing, because the obvious mental model is wrong. VS Code stores
`JSON.stringify(safeStorage.encryptString(password))`, so `ItemTable.value` holds TEXT of
the form `{"type":"Buffer","data":[…]}`. The bytes inside are a Chromium OSCrypt envelope:
`v10` + a 12-byte nonce + AES-256-GCM ciphertext + a 16-byte tag. The AES-256 key lives in
`%APPDATA%\Code\Local State` under `os_crypt.encrypted_key`, base64-encoded and prefixed
with `DPAPI`.

DPAPI protects the **key**, one level in — never the stored value. Calling
`CryptUnprotectData` on the value fails for every user on every machine, which is why
older builds reported a bogus "running as a different Windows user" error here.

### What each failure means

| Diagnostic output                                      | Cause                                                            | Fix                                                                             |
| ------------------------------------------------------ | ---------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `key not found in state.vscdb`                         | Password was never saved for this server/user pair               | Reconnect the server in VS Code and choose to save the password                 |
| `Value encoding: Unknown`                              | A VS Code storage format this build does not recognise           | Open an issue with the printed encoding and hex prefix                          |
| `cannot read …\Local State`                            | Non-standard install layout (portable mode, a VS Code fork)      | Pass `--db-path` pointing at the real `state.vscdb`                             |
| `CryptUnprotectData failed on the Local State AES key` | Profile copied from another machine, or a different Windows user | Reconnect in VS Code as the account that runs the agent, so the key is resealed |
| `AES-GCM authentication failed`                        | Local State key and `state.vscdb` are out of sync                | Reconnect the server in VS Code to rewrite both                                 |

The username is lower-cased before lookup, matching what Server Manager writes. `_SYSTEM`
and `_system` resolve to the same key.

Cursor and other VS Code forks work the same way; `check-sm-credential` falls back to
`%APPDATA%\Cursor` and derives `Local State` from whichever database it opened.

---

## An error response has `hint_ref` pointing at a skill you do not have

iad adds `hint_ref: {skill, section, why}` next to `hint` on the errors it recognises (see
`docs/tools.md`, error codes). A client without iad's skills cannot follow it. Set
`IAD_CODING_PACK=off` in the server's environment: `hint_ref` goes, `hint` stays, and no restart
is needed because the variable is read per call.

## Skill eval: every skill scores zero

Symptom: `python -m tests.e2e.skill_eval` finishes, the table shows `0.00` in both arms for
every skill, and `--update-baseline` writes those zeros to
`tests/e2e/results/skill-baseline.json`.

It never meant the skills stopped working. It meant the scorer was never reached, and the
old code counted an unreachable scorer as `score: 0` — a failing grade rather than no
grade. Nine entries of fabricated zeros went into the committed baseline in August 2026
that way.

A scoring call that cannot be made now returns `scored: false` with no score at all, and
a pass rate is computed over the items that were scored. When nothing was scored the rate
is `None`, which prints as `—` and refuses to be compared to anything:

```text
skill                          mode      scored     base  skill   lift  Δ base  outcome
objectscript-review            —         0/24          —      —      —       —  not comparable
                               ↳ no scored items
```

The run also stops before it spends anything. `preflight` makes one small real scoring
call, resolves the `iris-agentic-dev` binary, and counts the tools it advertises:

```bash
python -m tests.e2e.skill_eval --preflight-only
# preflight ok — scorer claude-sonnet-4-6 via AWS_BEARER_TOKEN_BEDROCK,
#   tools 1.4.0+fa0b694f8725 at /opt/homebrew/bin/iris-agentic-dev
```

Exit 2 means nothing was spent; exit 1 means a run happened and something about it failed.

| Symptom                                                | Likely cause                                                            | Fix                                                                       |
| ------------------------------------------------------ | ----------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `preflight: no scorer credential`                      | No Anthropic or Bedrock credential in the environment                   | Set `ANTHROPIC_API_KEY` or `AWS_BEARER_TOKEN_BEDROCK`                     |
| `preflight: iris-agentic-dev not found`                | The binary is not on `PATH` and `IAD_BINARY` is unset                   | `cargo build` and set `IAD_BINARY=./target/debug/iris-agentic-dev`        |
| `tool surface: none` in the footer                     | The agent ran with no MCP server, so nothing under test was in the loop | Same as above — the surface is read from the binary the run resolved      |
| `not comparable: no scored items`                      | Every scoring call for that skill failed                                | Check the scorer credential; the pass rate is honestly absent, not zero   |
| `not comparable: task_ids differs`                     | The baseline entry was measured on a different task set                 | Rebaseline the skill: `--skill <name> --update-baseline`                  |
| `not comparable: no provenance recorded`               | A schema 1 baseline entry, which recorded no measurement conditions     | Rebaseline the skill                                                      |
| `Shards disagree on the regression threshold`          | Two nightly shards ran with different `--regression-threshold`          | Dispatch the whole matrix with one threshold; half a table is not a table |
| `WARNING: baseline entry for <skill> has no eval.yaml` | An entry outlived its eval config                                       | Delete the entry, or restore the config — the write keeps it either way   |

Every skill with an `eval.yaml` must be either measured or excused: an entry in `skills`,
or a reason in `ungated_skills` saying why it has none. A name in both is an error, and a
name in neither fails `test_every_eval_config_is_gated_or_declared`.

---

## Getting help

Issues and pull requests: [GitHub Issues](https://github.com/intersystems-community/iris-agentic-dev/issues)

Questions: [thomas.dyar@intersystems.com](mailto:thomas.dyar@intersystems.com)
