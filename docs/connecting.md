# Connecting to IRIS

iris-agentic-dev connects to IRIS via the Atelier REST API — the same API the VS Code
ObjectScript extension uses. No special IRIS configuration is required beyond what you
already have for VS Code.

**If something isn't working, call `check_config` first** — it shows exactly which
connection source won, what host/port/namespace was resolved, and whether Atelier REST
is available. It always succeeds, and it makes no IRIS network call, with one exception: when
`IRIS_SERVICE_USERNAME` is set it sends a single `GET /api/atelier/` with that account to report
whether it can log in.

```text
Call check_config and show me the result.
```

Or from the terminal:

```bash
iris-agentic-dev tool check_config --args '{}'
```

---

## Where configuration comes from

iad reads settings from four places, and they are not equal. The difference decides what a
repository can make your agent do.

| Source                | What it is                                                                                                                                                                                         | Trusted                                                                      |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| **Launch settings**   | The environment variables and flags iad was started with (`IRIS_*`, `IAD_*`, `--host`, `--config`). Approving a project's MCP server in a client means trusting the settings it launches iad with. | Always                                                                       |
| **Operator file**     | `~/.iris-agentic-dev.toml` in your home directory. Read wherever the project lives, never mistaken for a workspace file.                                                                           | Always                                                                       |
| **Workspace file**    | The nearest `.iris-agentic-dev.toml` found by walking up from the working directory. The walk stops before your home directory, so it never finds the operator file.                               | Only after you run `iris-agentic-dev trust`                                  |
| **Named config file** | A file given by `--config`, or a directory or file given by `OBJECTSCRIPT_WORKSPACE` or `--workspace`. Judged exactly like a workspace file.                                                       | Only after you run `iris-agentic-dev trust` (unless it is the operator file) |

iad reads the operator file every time and at most one workspace-layer file. The workspace-layer
file is, in order, `--config`, `OBJECTSCRIPT_WORKSPACE`, `--workspace`, then the walk up from the
working directory. A directory named that way means the `.iris-agentic-dev.toml` inside it. A
workspace-layer file that turns out to be the operator file is read once, as the operator file.
The legacy name `.iris-dev.toml` is read only when no `.iris-agentic-dev.toml` is found.

**Which one wins.** For the connection (host, port, namespace, container), a workspace-layer
file is the file that chooses; with none, the operator file does, and `IRIS_HOST` outranks the
operator file's connection settings. Flags outrank every file. For gates and policy the two files
are combined, not replaced; see [Effective policy](#effective-policy). A file that does not parse
is an error, not a missing file: iad builds no connection, closes both write tiers, and every
IRIS tool answers `CONFIG_INVALID` with the file, line and column until it is fixed.

There is no other global file. `~/.config/iris-agentic-dev/config.toml` is not read. (The
registry of servers added with `iris_add_server` is `~/.config/iris-agentic-dev/servers.json`;
see [Adding servers](#adding-servers-via-iris_add_server).)

### Trust

A repository comes with its own `.iris-agentic-dev.toml`, so what it says is the repository's
choice, not yours. Until you accept a workspace or named config file, iad uses it for what it
cannot hurt (a host, a namespace, a tool list, anything that tightens) and ignores its
**loosening settings**. A loosening setting widens what the agent may do or where credentials
go:

- raising the write or destructive tier (`write_tools_enabled = true`,
  `destructive_tools_enabled = true`, `[policy.<s>] writeTools = true` or `destructiveTools = true`);
- turning the compile-time gate off (`compile_time_gate = false`, `compileTimeGate = false`);
- a looser policy: `[policy.<s>] mcpTemplate = "dev"` or `"test"`, an `allow` list (it loosens
  when it names a category the operator file's list leaves out), `dataPolicy = "allow"` or
  `"redact"`, a `dataPolicyKillAllowlist`;
- `tls_verify = false`, on the connection or on an `[instance.<name>]` block;
- `ssh_host`, which names the machine your code runs on;
- `service_username`, which names the account your code runs as;
- `[instance.<name>] role = "workbench"`, which lifts a subject server's confirmation rule.

Connection details, tool lists, `read_only = "strict"`, `globalBlocklist`, `irisAudit` and
anything else that only tightens are not loosening settings and need no trust.

Run trust at a terminal:

```bash
iris-agentic-dev trust            # the workspace file iad would load from here
iris-agentic-dev trust ./other.toml
iris-agentic-dev trust --show     # print the loosening settings and their effect; write nothing
iris-agentic-dev trust --list     # trust entries in the operator file, and whether each still matches
iris-agentic-dev trust --revoke /path/to/.iris-agentic-dev.toml
```

`trust` prints each loosening setting with its effect and the server it applies to, then asks you
to type the word `trust`. Anything else exits 1 and writes nothing. The entry goes into
`~/.iris-agentic-dev.toml` as a `[[trust]]` table: the file's canonical path, a format number,
and the sorted loosening settings, each with the server address it applies to. Your comments and
ordering in that file are kept.

Trust is exact. Change a loosening setting, add one, or repoint it at another address, and the
entry no longer matches: trust has **lapsed**, every loosening setting of that file is set aside
again, and `check_config` names what changed (`trust lapsed: policy.prod.allow changed from
["query"] to ["query", "execute"]`). A file whose only change is a tightening, a comment or a connection detail keeps
its trust. A file with no loosening settings needs none; `trust` says so and writes nothing.

A trust entry loosens, so a running iad holds it back: the file is trusted at the next start. Run
`trust`, then restart the MCP server.

Exit codes: `0` done or nothing to do, `1` you declined, `2` refused (no terminal, the file
does not parse, or the path is the operator file). `--show`, `--list` and `--revoke` need no
terminal, since they can only reveal or tighten.

> **The terminal check proves a terminal, not a person.** An agent whose shell runs commands in a
> pseudo-terminal can pass it. The boundary that holds is you approving the shell command in
> your client. Do not auto-approve `iris-agentic-dev trust`, and do not give an agent a shell
> that you have not read the commands of. No MCP tool writes a trust entry; an agent that wants a
> file trusted can run `trust --show` and ask you to run the real command.

### Editing the operator file while iad runs

iad reads the operator file again on every call, and applies an edit at once only where it
**tightens**: a gate switched off, `read_only = "strict"` added, a template moved toward `live`,
a narrower `allow`, a `[[trust]]` entry removed. An edit that **loosens** waits for the next start:
a gate switched on, `read_only = "strict"` removed or the file deleted, a looser template, `allow`
or `dataPolicy`, a shorter blocklist, a role of `workbench`, `tls_verify = false`, `ssh_host`, a
service account, `exec_as_os_user`, `local_skills`, `audit_payloads`, `telemetry_sink = "iris"`, a removed `http_token`, or a
new or changed `[[trust]]` entry. iad writes one stderr line for each held setting, and
`check_config.set_aside` lists it with kind `held_for_restart` and the lift `restart iad`.

The reason is that an agent with a file tool can write the operator file. Restart the MCP server
to take a loosening edit up. A workspace file keeps its own rule: its loosening settings are set
aside until you run `trust` on it.

### Operator-only settings

Some settings are honoured only from the operator file or launch settings, never from a workspace
file, trusted or not. A workspace file that sets one gets it listed in `set_aside` as
`operator_only`.

| Setting                                                      | Launch form                               | What it does                                                                                                 |
| ------------------------------------------------------------ | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `[[trust]]`                                                  | none; written by `iris-agentic-dev trust` | The trust entries                                                                                            |
| `exec_as_os_user = true`, `[policy.<s>] execAsOsUser = true` | `IAD_EXEC_AS_OS_USER=1`                   | Run the agent's code as the container's OS user; see [Service account](#service-account-for-the-agents-code) |
| `[transport] http_token = "..."`                             | `IAD_HTTP_TOKEN`, `--auth-token-file`     | The bearer token of the [HTTP transport](#http-transport)                                                    |
| `local_skills = true`                                        | `IAD_LOCAL_SKILLS=1`                      | Serve `skills/skills` from the working directory; see [skills.md](skills.md#serving-skills-from-a-directory) |
| `audit_payloads = true`                                      | `IAD_AUDIT_PAYLOADS=1`                    | Keep full `code`, `query` and `args` text in the local audit log; the default records sizes                  |
| `telemetry_sink = "iris"`                                    | `IAD_TELEMETRY_SINK=iris`                 | Also keep telemetry in IRIS; see [Telemetry](#telemetry). The default is the local file                      |

### Effective policy

Each tool call is judged against its **target server**: the one it names in `server`, otherwise
the default server. The policy for that server is built part by part from the operator file and
the workspace file, and the result is the **effective policy**. `check_config` shows every part
with where it came from.

| Part                                                | An untrusted or lapsed workspace file                      | A trusted workspace file      |
| --------------------------------------------------- | ---------------------------------------------------------- | ----------------------------- |
| `mcpTemplate`                                       | the stricter of the two wins: dev, then test, then live    | replaces the operator file's  |
| `allow` (tool categories)                           | intersection; absent means every category                  | replaces the operator file's  |
| `dataPolicy`                                        | the stricter wins: block, then redact, then allow          | replaces the operator file's  |
| `writeTools`, `destructiveTools`, `compileTimeGate` | the stricter wins                                          | replaces the operator file's  |
| `globalBlocklist`                                   | union of both files                                        | union of both files           |
| `irisAudit`                                         | on if either file turns it on                              | on if either file turns it on |
| `dataPolicyKillAllowlist`                           | operator file or a trusted file only, custom patterns only | as the operator file's        |
| `role`                                              | `subject` wins over `workbench`                            | replaces the operator file's  |

A loosening part from an untrusted file is dropped by itself; the rest of that file's policy still
applies. Launch settings (`IRIS_WRITE_TOOLS_ENABLED`, `IRIS_DESTRUCTIVE_TOOLS_ENABLED`,
`IAD_COMPILE_TIME_GATE`) outrank every file. One exception keeps a repository from borrowing your
authority: when a workspace file points the connection at a remote host that your environment's
credentials may not go to, your looser launch settings stay behind with the credentials, and
`check_config` lists them in `set_aside`.

**The template comes from the system mode when you declare none.** IRIS labels each instance
with a **system mode**: Live, Test, Development or Failover. When no file declares `mcpTemplate`
for a server, iad reads the mode and picks the template: Live and Failover give `live`, Test
gives `test`, Development and an unset mode give `dev`. A server whose mode iad cannot read is
judged by its namespace for the default server (a production-looking name is `live`) and as
`live` for any other server. `check_config` shows `from: "system_mode"` on the part.

| Template | Blocks                                    |
| -------- | ----------------------------------------- |
| `dev`    | nothing                                   |
| `test`   | the `execute` and `compile` categories    |
| `live`   | `execute`, `compile` and `source_control` |

Writes count as `execute` here: `iris_global` `set`/`kill` and `iris_query` `mode="write"` are
refused on a `test` or `live` server. The write tier follows the same mode when you declare
nothing: off on Live, on on Development and Test, and decided by the namespace name when the mode
is unset or unreadable. The destructive tier is never inferred; it stays off until you declare it.
`check_config` reports what decided each in `write_tools_source` and `destructive_tools_source`.

### Server roles

A server has a **role**. `workbench` (the default) is a server the agent builds on. `subject` is
a server under study: a source-control write is refused outright
(`SUBJECT_SCM_WRITE_BLOCKED`), and any other write is refused until the call repeats with
`confirm: true` (`SUBJECT_CONFIRM_REQUIRED`).

```toml
[instance.legacy-prod]
host = "10.1.2.3"
web_port = 52773
role = "subject"
```

The role is set on an `[instance.<name>]` block. `workspace` (the pre-1.7 spelling) and
`control-plane` read as `workbench` with a note, and `workspace` is accepted through 1.7.x; any
other value reads as `subject`, because an unknown word fails closed. `check_config` lists the
note in `config_errors`. `confirm` is an
acknowledgement: it lifts a check meant to catch the agent's mistakes, so do not read it as a
barrier against the agent. A `subject` role on a looser file's server is not a loosening setting
(it tightens); `role = "workbench"` is.

### Set-aside items

When iad finds something in configuration and does not act on it, it records a **set-aside
item**, writes one line to stderr at startup (`iris-agentic-dev: 2 settings set aside (...)`), and
lists each in `check_config.set_aside` before any call is refused. Each has an `id`, a `kind`, the
`setting` as written, the `file` and `server`, a `reason` and a `lift` that says what would make iad
act on it.

| Kind                  | Meaning                                                                                         | Lift                                                              |
| --------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `untrusted_setting`   | A loosening setting from an untrusted or lapsed file                                            | `iris-agentic-dev trust <file>` in a terminal                     |
| `operator_only`       | An operator-only setting in a workspace file                                                    | The operator sets it (`iris-agentic-dev trust --show` says where) |
| `held_for_restart`    | An edit to the operator file made during the session that loosens a setting                     | Restart iad; edits that tighten apply at once                     |
| `credential_withheld` | A credential kept from a server it was not meant for                                            | Put credentials in the file, or point `IRIS_HOST` at that server  |
| `shadowed_server`     | A workspace file redefines a server the operator file already defines; the operator file's wins | Rename the server in the workspace file                           |

A refusal that a set-aside item would have prevented carries the item's id in
`refusal.set_aside`; see [Refusals](tools.md#refusals).

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
# no password: this file is committed with the project; set IRIS_PASSWORD in the MCP launch settings
```

`port` is accepted as an alias for `web_port` — both are equivalent in the toml file.

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

See the [`iris-windows-iis-setup` skill](../skills/skills/iris-windows-iis-setup/SKILL.md)
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
    ports: ["127.0.0.1:4972:1972"]
  webgateway:
    image: containers.intersystems.com/intersystems/webgateway:2026.1
    ports: ["127.0.0.1:52773:80"]
    entrypoint: ["/bin/sh", "/init.sh"]
    volumes: ["./webgateway-init.sh:/init.sh:ro"]
```

See the [`iris-vscode-objectscript` skill](../skills/skills/iris-vscode-objectscript/SKILL.md)
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

> **Windows limitation:** VS Code's `SecretStorage` on Windows uses DPAPI-encrypted blobs
> stored inside VS Code's own process. Those blobs are not accessible to external processes,
> so iris-agentic-dev cannot read Server Manager credentials on Windows regardless of whether
> they are stored. Use `.iris-agentic-dev.toml` instead — see
> [Native IRIS on Windows or Linux](#native-iris-on-windows-or-linux-no-docker) above.

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

---

## Adding servers via iris_add_server

`iris_add_server` registers an IRIS instance in
`~/.config/iris-agentic-dev/servers.json` so it can be referenced by name in any tool
call via the `server` parameter.

**Keychain path (macOS / Windows with keychain):** the credential is stored in the OS
keychain under iad's own service, `iris-agentic-dev`, and the entry in `servers.json` holds no
password. The keychain account includes the server's address (`scheme://host:port/prefix`), so a
password saved for one address cannot be found for another. iad never writes to, or deletes from,
the Server Manager's keychain service.

**Headless path (Claude Desktop MCP subprocess, Remote SSH, Linux CI):** when no OS
keychain is available, the credential is stored in plaintext in `servers.json` as a
fallback. The file is readable by you only (mode `0600`, in a `0700` directory), and the
password is recorded with the address it was saved for (`password_endpoint`) and used only
while the entry still points there. The response includes `stored_plaintext: true` and a
warning:

```json
{
  "added": true,
  "credential_store": "file",
  "stored_plaintext": true,
  "warning": "No OS keychain here: the password is in servers.json (owner-only, used only for this endpoint).",
  "note": "Call iris_reload_pool to make this server routable via the `server` param."
}
```

`credential_store` says where the call left the password: `keychain`, `file` or `none`. Set
`IAD_NO_KEYCHAIN=1` to skip the OS keychain on a host that has one.

Registering a name again at a **different address** drops the password the old address had
(`password_cleared: true`), so a moved entry never sends it to a new host. Pass a password for
the new address if it needs one.

`iris_add_server` refuses a name that is a Server Manager profile (`NAME_CONFLICT`): an entry
of that name would be read as the profile. Pick another name, or run `iris_import_servers` to
link the profile. `iris_import_servers` marks the entries it writes `credential =
"server-manager"`; their password stays in the Server Manager's keychain and is read only
while the entry's address equals the profile's. `iris_remove_server` deletes iad's own item and
never the Server Manager's.

For production workloads, prefer VS Code Server Manager (keychain-backed) or
`.iris-agentic-dev.toml` with file permissions restricted to your user. The plaintext
fallback is intended for development and CI contexts where the OS keychain is not
available.

`iris_servers` marks entries that use the plaintext fallback with
`has_plaintext_credential: true` so you can identify them and migrate credentials when
needed.

### The keychain

iad keeps two kinds of item in the OS keychain, both under its own service, `iris-agentic-dev`: a
server's password and a server's service-account password. The account name of each item holds
the server's name, the user and its address (`scheme://host:port/prefix`), so a password saved for
one address is never found for another. iad reads the Server Manager's service,
`intersystems-server-credentials`, and never writes to or deletes from it.

On macOS the first read of an item by a new binary (after an upgrade, for one) can raise an
approval dialog from the Keychain. Choose **Always Allow** once. A session with no one at the screen
cannot answer it, and a call that needs the item waits. On such a host, and in CI, set
`IAD_NO_KEYCHAIN=1` and keep the password in the environment or in `servers.json`, which iad writes
owner-only and ties to the address. See
[The keychain prompt](troubleshooting.md#the-keychain-prompt-blocks-a-call).

### The servers.json file

Each entry holds the parts of one base URL plus the namespace and username to use with it.
Only `host`, `port`, `namespace`, and `username` are required; `scheme` defaults to `http`.

```json
{
  "version": 1,
  "default": "dev",
  "servers": {
    "dev": {
      "host": "localhost",
      "port": 52780,
      "namespace": "USER",
      "username": "_SYSTEM",
      "description": "Dev container",
      "scheme": "http"
    },
    "hs-test": {
      "host": "gateway.example.com",
      "port": 8080,
      "namespace": "HSCUSTOM",
      "username": "hsadmin",
      "web_prefix": "/hs20261"
    }
  }
}
```

You can edit the file by hand — iad picks the change up on the next call, and
`iris_reload_pool` forces it immediately. Unknown keys are ignored, so a file written by a
newer iad still loads on an older one.

### Instances behind a web gateway

`web_prefix` is the path the IRIS web server is served under. Without it, `hs-test` above
would be called at `http://gateway.example.com:8080/api/atelier/`, which is either a 404 or,
worse, some other instance sharing the gateway. With it, iad calls
`http://gateway.example.com:8080/hs20261/api/atelier/`.

This is how HealthShare instances and multi-instance gateway deployments are usually
published. If Atelier answers at a path rather than at the root, you need the prefix — the
symptom of a missing one is a 404 from every tool while the host and port look right.

Leading and trailing slashes are optional and multi-segment prefixes work, so `hs20261`,
`/hs20261/`, and `/gw/hs20261` are all fine. The value is a path only: a scheme or host in it
is refused at registration. `webServer.pathPrefix`, the VS Code Server Manager spelling, is
accepted as an alias for the same key, so a block copied out of `settings.json` loads
unchanged. `iris_import_servers` carries the prefix across automatically.

The equivalent for a server declared in `.iris-agentic-dev.toml` is `web_prefix` on the
`[instance.<name>]` block.

---

## Per-connection policy (fleet / operate mode)

Add `[policy.<server-name>]` blocks to your `~/.iris-agentic-dev.toml` to limit what a server
allows. A project file may carry the same blocks; where it is not trusted, its blocks can
only tighten (see [Effective policy](#effective-policy)). Use `[policy.default]` for the default
connection.

```toml
[policy.prod]
allow = ["query", "search", "docs"]
mcpTemplate = "live"
dataPolicy = "block"
globalBlocklist = ["^AppHealthData"]
irisAudit = true
```

| Key                              | Values                                     | What it does                                                                                                                                    |
| -------------------------------- | ------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `allow`                          | list of categories                         | Only these tool categories run. Omit it to permit every category.                                                                               |
| `mcpTemplate`                    | `"dev"`, `"test"`, `"live"`                | Which categories the environment blocks; see [Effective policy](#effective-policy). Omit it and the server's system mode chooses.               |
| `dataPolicy`                     | `"block"` (default), `"redact"`, `"allow"` | What the bulk-data tools (`journal_search`, `iris_message_body`, `stream_inspect`, and `iris_interop_query` calls that name a body) may return. |
| `globalBlocklist`                | list of global names or patterns           | Globals writes and kills may not touch, added to the built-in list. It never stops a read.                                                      |
| `dataPolicyKillAllowlist`        | list of patterns                           | Globals a kill may touch although the built-in list would stop it. Custom patterns only.                                                        |
| `writeTools`, `destructiveTools` | `true` / `false`                           | The write and destructive tiers for this server only.                                                                                           |
| `compileTimeGate`                | `true` / `false`                           | The compile-time gate for this server; see [Compile-time gate](#compile-time-gate).                                                             |
| `irisAudit`                      | `true` / `false`                           | Emit a `%SYS.Audit` record per call.                                                                                                            |
| `execAsOsUser`                   | `true`                                     | Operator-only: run the agent's code as the container's OS user on this server.                                                                  |

Blocked calls return an error whose `error_code` names the gate (`POLICY_GATE` with the allowed
categories, `ENV_GATE_BLOCKED`, `DATA_POLICY_BLOCKED`, `SYSTEM_BLOCKLIST`) and a `refusal` object
saying who can lift it; see [Refusals](tools.md#refusals). Available categories: `compile`,
`execute`, `query`, `search`, `docs`, `source_control`, `debug`, `admin`, `skill`, `kb`.

### What `dataPolicy` governs

`dataPolicy` governs iad's own tools: the bulk-data tools named above decide, per call, whether
they return, redact or refuse the content they read. It is not an access control on the data in
IRIS. `iris_execute` runs ObjectScript as the connection's account and can read
`^Ens.MessageBody*` directly, as it always could through classes and SQL (`iris_query` on
`Ens.MessageBody` and its subclasses). Setting `dataPolicy = "block"` does not stop those reads.

To bound what an agent can read, limit what the account is allowed to read (a service account with
only the privileges the work needs), and use `allow` to leave the `execute` and `query` categories
off a server where the agent should only use the data tools.

### Compile-time gate

IRIS runs some constructs while a document compiles (preprocessor directives, braced class
parameters, projections, code in `.mac` and `.inc` files and in class XML), as the request that
compiles it, and the service account does not limit that. The compile-time gate reads the text of
`iris_doc` puts and of `iris_compile` targets before IRIS compiles them, and, for a compile by
name, the stored source of the target and of every include it pulls in. It refuses what would run
at compile time. It is on by default. `compile_time_gate = false` (or
`[policy.<s>] compileTimeGate = false`, or `IAD_COMPILE_TIME_GATE=0` in launch settings) turns it
off, and is a loosening setting. `check_config.compile_time_gate` says whether it is on and where
that came from. A refusal is `COMPILE_TIME_EXEC_BLOCKED` and an error result; a document that
exists but whose stored source cannot be read is `STORED_SOURCE_UNREADABLE`. Code that runs only
at call time (`CodeMode = expression` or `call`) is not refused.

IRIS also compiles a target's out-of-date superclasses with it. Before a compile, iad walks the
`Extends` chain of each class to be compiled, on the target server. If a superclass outside the
library packages (`%` classes, `Ens`, `EnsLib`, `EnsPortal`, `HS`, `HSLIB` and `HSMOD`; `HS.Local`
is yours) is out of date, by the compiler's own test (`$SYSTEM.OBJ.IsUpToDate`), the compile is
refused with `COMPILE_SUPERCLASS_STALE`, naming the superclass. Compile that superclass first; its
own compile is checked like any other. A walk that cannot run is refused too
(`SUPERCLASS_CHECK_FAILED`). Turning the compile-time gate off turns this check off with it.

### `irisAudit` — opt-in `%SYS.Audit` emission

Set `irisAudit = true` in a `[policy.<server>]` block to emit one `%SYS.Audit` record
per tool call. Off by default. The event definition must exist in `%SYS` before emission
can succeed — see [docs/agent-attribution.md](agent-attribution.md) for the one-time setup
command and the trust model.

```toml
[policy.prod]
irisAudit = true
```

For flat single-server configs (no Server Manager), use `[policy.default]` as the
catchall key:

```toml
host = "localhost"
web_port = 52780

[policy.default]
irisAudit = true
```

When emission fails (event definition absent or disabled), the tool warns once and
counts subsequent failures. The failure count appears in `check_config` as
`iris_audit_failures` when it is non-zero.

For multi-instance fleet workflows (`mode = "operate"`), see the
[ecosystem integration guide](ecosystem-integration.md) for the full `[instance.*]` config
format and role-gate behavior.

---

## Connection discovery order

iris-agentic-dev resolves the IRIS connection in this order, and the first match wins:

1. CLI flags (`--host`, `--web-port`, `--scheme`)
2. The workspace-layer file, when it names a host or container: the nearest `.iris-agentic-dev.toml`
   walking up from the working directory, or the file `--config`, `OBJECTSCRIPT_WORKSPACE` or
   `--workspace` names. Its connection settings apply whether or not it is trusted; only its
   loosening settings wait for trust
3. Environment variables (`IRIS_HOST`, etc.). A server named by `IRIS_HOST` is used even when it
   does not answer, so calls fail against it rather than reach another server
4. `~/.iris-agentic-dev.toml`, when the walk up finds no workspace file: your personal default.
   `IRIS_HOST` outranks its connection settings, though its tool and gate settings still apply. A
   live reload follows the same rule, and `check_config` reports `connection_source: env_vars`
   with the file still in `config_file`
5. `IRIS_CONTAINER`: the web port of the named Docker container
6. A scan of localhost ports (52773, 80, 8080, 41773, 51773, 8443, 443)
7. Running Docker containers (scored by workspace name similarity)
8. VS Code `settings.json` (`objectscript.conn`)
9. VS Code Server Manager (`intersystems.servers` plus the OS keychain credential)

A workspace `.iris-agentic-dev.toml` comes with the repository, so the host it names is the
repository's choice. When the file names a host but no `username`/`password`, iad sends your
`IRIS_USERNAME`/`IRIS_PASSWORD` only if that host is this machine (`localhost`, `127.0.0.1`, `::1`)
or the server `IRIS_HOST`, `IRIS_SCHEME`, `IRIS_WEB_PORT` and `IRIS_WEB_PREFIX` name: the same
scheme, host, port and prefix, not just the same host. Otherwise it sends the defaults, and
`check_config` reports `credentials_withheld` (and a `credential_withheld` item in `set_aside`)
with what to do: put the credentials in the file, or point `IRIS_HOST` at that server.

### What discovery sends

Discovery sends no credential to a listener until it has shown it is IRIS. For each candidate
it asks, with no credentials and no cookies, whether `/api/atelier/` answers with an IRIS
version, whether `/csp/sys/UtilHome.csp` sets a `CSPSESSIONID-…` or `IRISSessionToken` cookie
(every instance with a web server does, locked-down ones included), or whether
`/api/monitor/metrics` serves `iris_` metrics. A bare `401 Basic` proves nothing: any web
server sends one. The first listener that identifies itself gets one credentialed request. The
others are listed in `check_config` under `discovery.skipped`, with the port and the reason.

This is a fingerprint, not authentication: a hostile process on the same machine can imitate
IRIS on purpose. On a shared host, name your server (`IRIS_HOST`, `--host`, or `IRIS_CONTAINER`
in your launch settings) instead of relying on discovery. An instance that blocks all three
signs is no longer found automatically; set `IRIS_HOST`.

A `.vscode/settings.json` named server (`objectscript.conn.server`) gets a Server Manager
keychain password only when its address equals the user-level Server Manager profile of the same
name. A repository that names a host of its own gets no keychain password; `check_config`
lists the withheld secret in `set_aside`.

### Discovery that needs no connection

Two paths skip resolution entirely: `tool --list` and `tool <name> --schema` read the tool
router, not IRIS. A harness can enumerate the surface and read a tool's parameters before it
has a container, credentials, or a reachable port — useful when the thing being set up is the
connection itself. Every other `tool` invocation resolves a connection as above and fails with
the usual diagnostic when it cannot. See [Discovery from a
shell](tools.md#discovery-from-a-shell).

---

## Environment variables

| Variable                         | Default               | Description                                                                                                                                                                                   |
| -------------------------------- | --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `IRIS_HOST`                      | `localhost`           | IRIS web gateway hostname                                                                                                                                                                     |
| `IRIS_WEB_PORT`                  | `52773`               | Web gateway port                                                                                                                                                                              |
| `IRIS_SCHEME`                    | `http`                | `http` or `https`                                                                                                                                                                             |
| `IRIS_WEB_PREFIX`                | _(empty)_             | URL path prefix for non-root gateway installs                                                                                                                                                 |
| `IRIS_USERNAME`                  | `_SYSTEM`             | IRIS username                                                                                                                                                                                 |
| `IRIS_PASSWORD`                  | `SYS`                 | IRIS password                                                                                                                                                                                 |
| `IRIS_NAMESPACE`                 | `USER`                | Default namespace                                                                                                                                                                             |
| `IRIS_CONTAINER`                 | _(empty)_             | Docker container name — required for Docker-dependent tools. It belongs to the default connection: a server named with `server=` needs a `container` of its own.                              |
| `IRIS_SERVICE_USERNAME`          | _(empty)_             | Least-privilege account the tools that run your code use on the default connection; see [Service account](#service-account-for-the-agents-code)                                               |
| `IRIS_SERVICE_PASSWORD`          | _(empty)_             | Password for `IRIS_SERVICE_USERNAME`                                                                                                                                                          |
| `IAD_EXEC_AS_OS_USER`            | _(empty)_             | `1` runs the agent's code as the container's OS user when the service account cannot log in to a terminal. Environment or home-file only; a project file cannot set it.                       |
| `IRIS_SERVER_NAME`               | _(empty)_             | Server Manager server name when multiple are configured                                                                                                                                       |
| `IRIS_TLS_VERIFY`                | `true`                | Set `false` (or `0`) to skip TLS certificate validation for the server the launch settings name (`IRIS_HOST`, `--host`, discovery). Other servers have their own `tls_verify`.                |
| `IRIS_INSECURE`                  | `false`               | Blunter alias: `true` (or `1`) skips TLS certificate validation for that same server and outranks `IRIS_TLS_VERIFY`. Any other value, including a typo, means validate.                       |
| `IRIS_WRITE_TOOLS_ENABLED`       | _(unset)_             | `1` or `0`: the write tier, outranking every file. Unset, the files and the system mode decide.                                                                                               |
| `IRIS_DESTRUCTIVE_TOOLS_ENABLED` | _(unset)_             | `1` or `0`: the destructive tier, outranking every file. Unset, it is off until a file declares it.                                                                                           |
| `IRIS_ADMIN_TOOLS`               | _(unset)_             | `1` allows `iris_admin`'s user, namespace and web application actions after the tier gate.                                                                                                    |
| `IRIS_ALLOW_PROD`                | _(unset)_             | Legacy: `1` opens the write tier on a Live instance when nothing else declares it. Prefer `write_tools_enabled`.                                                                              |
| `IAD_COMPILE_TIME_GATE`          | _(on)_                | `0` or `off` turns the [compile-time gate](#compile-time-gate) off for the default server; `1` or `on` keeps it on everywhere.                                                                |
| `IAD_READ_ONLY`                  | _(unset)_             | `strict` for [strict read-only](#strict-read-only-no-scratch-classes).                                                                                                                        |
| `IAD_HTTP_TOKEN`                 | _(unset)_             | Bearer token for the [HTTP transport](#http-transport) (at least 32 bytes). Operator-only.                                                                                                    |
| `IAD_MAX_RESPONSE_BYTES`         | `134217728` (128 MiB) | The most bytes iad reads from one IRIS response. A larger one is refused as `RESPONSE_TOO_LARGE`; the largest answer measured on a live instance was 1.6 MB. Operator-only.                   |
| `IAD_LOCAL_SKILLS`               | _(unset)_             | `1` serves `skills/skills` from the working directory. Operator-only; see [skills.md](skills.md#serving-skills-from-a-directory).                                                             |
| `IAD_AUDIT_PAYLOADS`             | _(unset)_             | `1` keeps full `code`, `query` and `args` text in the local audit log. Operator-only.                                                                                                         |
| `IAD_TELEMETRY_SINK`             | `local`               | `iris` also keeps telemetry in the IRIS global; see [Telemetry](#telemetry). Operator-only.                                                                                                   |
| `IAD_NO_KEYCHAIN`                | _(unset)_             | Set to `1` to skip the OS keychain: passwords given to `iris_add_server` go to `servers.json` (mode `0600`), bound to the address they were saved for.                                        |
| `OBJECTSCRIPT_WORKSPACE`         | `$PWD`                | Workspace root for `.iris-agentic-dev.toml` lookup                                                                                                                                            |
| `IRIS_SEARCH_SYNC_TIMEOUT`       | `30`                  | Seconds to wait for synchronous search before falling back to async polling                                                                                                                   |
| `IRIS_DISABLED_TOOLS`            | _(empty)_             | Comma-separated tool names to exclude, e.g. `iris_source_control,iris_admin`                                                                                                                  |
| `IRIS_ENABLED_TOOLS`             | _(empty)_             | Comma-separated allowlist — when set, ONLY these tools remain, regardless of `--toolset`. Empty means no allowlist, not "expose zero tools." `IRIS_DISABLED_TOOLS` wins for any name in both. |
| `IRIS_NO_SKILLS`                 | `false`               | Set to `true` to remove all skill, KB, and learning-agent tools from `tools/list` and skip `--subscribe` fetching. Equivalent to `--no-skills`.                                               |

---

## Service account for the agent's code

`iris_execute`, `iris_query` (`write`/`force`), `iris_global` (`set`/`kill`),
`iris_execute_method`, `global_kill`, `iris_test`, `iris_coverage` (`run`/`start`),
`iris_ws_open` and `iris_ws_exec` run the code the agent wrote. With a service account set
they run as it on every path, or the call is refused.

| Where                             | Service account                                                                                |
| --------------------------------- | ---------------------------------------------------------------------------------------------- |
| Default connection, `_env`        | `IRIS_SERVICE_USERNAME` / `IRIS_SERVICE_PASSWORD`                                              |
| A server in `servers.json`        | its `service_username`, password in the keychain (`service_password` is the headless fallback) |
| An `[instance.<name>]` block      | its `service_username`; honoured from your own file or a trusted project file only             |
| Any other server, env account set | none: refused, `SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER`                                       |
| Any other server, no env account  | the server's own user, as before                                                               |

`iris_add_server` takes `service_username`, `service_password` and `container`. The service
password is stored the way the server's own password is: in iad's keychain item for that
account at that address, and in `servers.json` (owner-only, tied to `password_endpoint`) only on
a host with no keychain.

**How each path logs in.** Atelier REST sends the account's credentials, through a cookie jar
of its own (IRIS reads a session cookie before the `Authorization` header, so a jar shared
with your own user would run the code as you). `docker exec`, NoPWS and SSH cannot send a
user name to `iris session`, so the first line iad writes drops to the account inside IRIS and
ends the process if it cannot: `$SYSTEM.Security.Login("<account>")`, then a check of
`$USERNAME`. No password is written to the terminal. The process cannot log back in as
`irisowner`. The WebSocket terminal logs in with the account's credentials and asks
`$USERNAME` once after the prompt.

The account needs `%Development:USE` on every path. Without it Atelier answers 401
(`SERVICE_ACCOUNT_HTTP_DENIED`), and a terminal prints `Access Denied` after the login
(`SERVICE_ACCOUNT_TERMINAL_DENIED`). When Atelier refuses the account, iad does **not** fall
back to `docker exec`, which would run the code as `%All`.

**A layout that works.** `%Development:USE` also opens the debugger and the rest of the Atelier
API to the account. What stops it editing code is database permission, so lay the databases out
like this:

1. Three databases: application code, application data and a scratch area.
2. A namespace for the agent whose **default routines database is the scratch database** and whose
   globals database is the application data. Map the application's packages and routines into
   it from the code database (`Config.MapPackages`, `Config.MapRoutines`).
3. Give each database its own resource (`SYS.Database.ResourceName`; a new one is `%DB_%DEFAULT`).
4. A role with `%Development:USE`, `%DB_<code>:R`, `%DB_<data>:RW` and `%DB_<scratch>:RW`, and a
   user with that role and password authentication.

The compiler writes to the default routines database, so a namespace whose default is read-only
cannot compile the class iad generates (`ERROR #302`), however `IrisDevTmp` is mapped. A class in
a mapped application package cannot be saved (`<PROTECT>`); a class in an unmapped package lands
in the scratch database and can do no more than run as the account. `iris_coverage` `run` and
`start` also need read-write access to the IRISSYS database's resource, because the line monitor's
state is system-wide; without it they answer `COVERAGE_PRIVILEGE_DENIED`.

**Per-server accounts.** A server you name with `server=` uses its own account, never the
environment's. Give it `service_username` in `servers.json` (or `iris_add_server`) or in an
`[instance.<name>]` block of your own `~/.iris-agentic-dev.toml`; a `service_username` in a project
file counts only after you trust that file, since it names the account your code runs as. Its
password goes in the keychain. A server with no account of its own while `IRIS_SERVICE_USERNAME`
is set is refused (`SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER`); iad does not fall back to that
server's own user. If a project file names a remote host that your environment's credentials may
not go to, the service account is withheld from it too (`SERVICE_ACCOUNT_WITHHELD`). A named
server never falls back to the default connection's `IRIS_CONTAINER`; give it a `container` to
allow a `docker exec` route.

**What a service account does not cover.** `iris_query` in `mode="read"` can call a stored
procedure as the connection account, and only IRIS privileges limit that. The code store (`iris_doc`
put, `iris_compile`, `iris_source_control`) keeps running as the connection account, so source
control attributes edits to a real user.

**Running as the container's OS user instead.** Where the login guard cannot work, set
`IAD_EXEC_AS_OS_USER=1` in the environment your MCP client launches iad with, or
`exec_as_os_user = true` (all servers) or `[policy.<name>] execAsOsUser = true` (one server)
in `~/.iris-agentic-dev.toml`. The response then says `"source": "os_user_opt_in"`. A project
`.iris-agentic-dev.toml` is ignored for this setting, and `check_config` lists it under
`identity.os_user_opt_in.ignored`.

`check_config` returns an `identity` object: the account, the opt-in and where it was set, the
user each path would run as (`http` also reports `login`, from one `GET /api/atelier/` that sends
no code), each server's account, and the tools covered.

---

## TLS certificate validation

Over `https`, certificates are validated against the **operating system** trust store, so a
gateway serving a cert from a CA you installed locally (mkcert, a corporate internal CA)
works without extra configuration. If `curl` on the same machine accepts the cert,
iris-agentic-dev does too.

To connect to a gateway whose cert cannot be validated — self-signed, expired, or a hostname
mismatch — turn validation off for **that connection**. Every connection carries its own
setting; none reaches another server.

```toml
# ~/.iris-agentic-dev.toml, or a project file you have trusted
host = "iris.internal"
web_port = 443
scheme = "https"
tls_verify = false
```

| Connection                                                  | Where `tls_verify` comes from                                                                                                         |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| The server the launch settings name (`IRIS_HOST`, `--host`) | `IRIS_TLS_VERIFY` / `IRIS_INSECURE`. A project file naming that same server (same scheme, host, port, prefix) gets the same setting.  |
| The file's own connection                                   | `tls_verify` in that file. `false` counts only from `~/.iris-agentic-dev.toml` or a project file you ran `iris-agentic-dev trust` on. |
| An `[instance.<name>]` block                                | `tls_verify` in the block, under the same rule.                                                                                       |
| A `servers.json` entry                                      | `"tls_verify": false` on the entry. You edit that file by hand; `iris_add_server` has no such parameter.                              |
| A Server Manager profile                                    | Always validates. Profiles carry no such setting.                                                                                     |

Leave `tls_verify` out to validate. `IRIS_INSECURE=true` outranks `IRIS_TLS_VERIFY`; any
unrecognised value means validate, so a typo fails closed with a handshake error rather than
quietly skipping the check. A project file's `tls_verify = false` that you have not trusted is
listed in `set_aside` and does nothing.

`check_config` reports the default connection's state as `tls_verify`, and every server's
setting with where it came from under `tls`. It makes no network calls, so this is the only
place to see that validation is off before a bad cert goes unremarked:

```json
{
  "tls_verify": false,
  "tls": {
    "default": { "tls_verify": false, "source": "config_file" },
    "servers": { "lab": { "tls_verify": false, "source": "servers_json" } }
  }
}
```

`source` is `default` (nothing turned validation off), `launch_env`, `config_file`,
`servers_json` or `server_manager`.

---

## Credentials you don't want to repeat in every project

Put them in the operator file, `~/.iris-agentic-dev.toml`. It is read wherever the project lives,
and a project's own `.iris-agentic-dev.toml` outranks it for the connection. Do not put a
password in a file that is committed with a project; use `IRIS_PASSWORD` in your client's launch
settings or the OS keychain through `iris_add_server`.

```toml
# ~/.iris-agentic-dev.toml: defaults for every project without its own host
username = "_SYSTEM"
password = "<password>"
```

The operator file is the only place a trusted setting can come from for every project at once, so
keep it owner-only (`chmod 600`).

---

## Restricting writes on shared servers

Set `write_tools_enabled = false` to put a server in read-only mode: compile, execute, doc-write,
source control and global-write tools all return a clear error instead of modifying anything.
Query, search and doc-read tools continue to work. It tightens, so it takes effect from any
configuration file, a project file included.

```toml
# .iris-agentic-dev.toml on a shared dev or production server
host = "shared-iris"
web_port = 52773
namespace = "USER"
username = "_SYSTEM"
# the password comes from IRIS_PASSWORD in the launch settings, not from this file
write_tools_enabled = false
```

This is the recommended default for any server that more than one person connects to, or any
server that isn't purely local.

The opposite direction needs you. Where nothing is declared, the write tier follows the server's
system mode (off on Live, on on Development and Test; see [Effective policy](#effective-policy)),
so `write_tools_enabled = true` is only needed to open a Live server. A project file that says
`write_tools_enabled = true` is a loosening setting: until you run `iris-agentic-dev trust` on it,
iad sets it aside and keeps the tier the system mode gives. If writes are off in a project that
asks for them, see
[Write tools are disabled in my project](troubleshooting.md#write-tools-are-disabled-in-my-project).
Per server, `[policy.<name>] writeTools` and `destructiveTools` do the same for one server.

### Strict read-only: no scratch classes

Reading over HTTP has a cost: Atelier has no execute endpoint, so most read tools run their
ObjectScript through a scratch class that iad puts, compiles and deletes in the `IrisDevTmp`
package. The class is gone after the call and nothing the user can observe changes, but it is a
write to the server. For a server where iad must write nothing at all, such as a production read
replica under change control, set

```toml
read_only = "strict"
```

in any configuration file, or `IAD_READ_ONLY=strict` in the launch settings. It only tightens, so
a project file may set it and no file can lift another's. Under strict, every tool that can reach a
scratch class is hidden from `tools/list` and refused with `READ_ONLY_STRICT`: the 37 tools marked
in `write_gate::CLASSIFICATION` (`iris_table_info`, `global_preview`, `my_access`, `iris_global`,
`iris_production`, `iris_execute`, `iris_test`, `journal_search` and the rest of the schema,
interoperability and administration readers). `iris_doc` (reads), `iris_query`, `iris_search`,
`iris_info`, `iris_servers` and `check_config` stay. Telemetry is kept in the local file.
`check_config` reports `read_only` and `read_only_from`.

Strict also closes the write and destructive gates on every server. It outranks
`write_tools_enabled = true`, `destructive_tools_enabled = true`, `[policy.<name>] writeTools`,
`IRIS_WRITE_TOOLS_ENABLED=1` and `IRIS_ALLOW_PROD`, wherever they are set: a write or destructive
call is refused with `READ_ONLY_STRICT`, and `check_config` shows both gates closed with source
`read_only_strict`. Remove the setting and restart to open them again.

Strict together with `irisAudit = true` on any server is a configuration error
(`READ_ONLY_STRICT_WITH_AUDIT`): the audit emission compiles a class on the server. `iris-agentic-dev mcp`
names both settings on stderr and exits 2, and `check_config` lists the error under `config_errors`.

### Telemetry

The durable record of every tool call goes to `~/.iris-agentic-dev/telemetry/<session>.jsonl`
(mode 600, in a directory of mode 700). That is the whole default: a read on a Live server does
not compile a class, and nothing is written to `^IRISDEV("telemetry")` in `USER`.

Keeping the record in IRIS as well, for history across machines, is an operator opt-in:
`telemetry_sink = "iris"` in the operator file, or `IAD_TELEMETRY_SINK=iris` in the launch
settings. A workspace file cannot turn it on; its `telemetry_sink = "iris"` is listed in
`set_aside` as `operator_only`. Turning it on in the operator file while a server runs waits for
the next start. `check_config` reports the setting as `telemetry_sink` (`local` or `iris`).

With the opt-in, the IRIS write is a put, a compile, a query and a delete of a scratch class per
call, so it runs only on the server the call acted on (not the default one) and only where that
server's writes are on. It does not run under strict read-only, or on a server whose role is
`subject` (a server under change control, where each record would be a class compiled per call).
In those cases the record is in the local file alone. `telemetry_query` and
`telemetry_export_trace` read the local file, and the IRIS global too under the same conditions,
and return a call that both hold once.

### What the tiers guard, and what they don't

`IRIS_WRITE_TOOLS_ENABLED=1` and `IRIS_DESTRUCTIVE_TOOLS_ENABLED=1` switch on the write and
destructive tiers. `iris_admin`'s user, namespace and web application actions,
`clear_password_change_flag`, `unlock_user` and `fresh_container_setup` also need
`IRIS_ADMIN_TOOLS=1`, which the handler checks after the tier gate. With both tiers on and
`IRIS_ADMIN_TOOLS` unset, those actions are still refused.

The tiers are an accident guard for tool calls, not a security boundary. For the destructive tier,
`iris_execute` is write tier and a literal `Kill ^global` in its code raises it to destructive.
With the destructive tier off, it can still run `##class(Security.Applications).Delete("/x")` in
`%SYS`. The code-edit guard and the system blocklist look at the code too, but by text, and
indirection gets past a text check. For a hard limit, run the agent's code as a
[service account](#service-account-for-the-agents-code) whose IRIS privileges are the limit.

## Tool subset (allowlist)

Restrict the tool surface to a named set:

```toml
# .iris-agentic-dev.toml
# Expose only these tools; empty means no allowlist (all tools visible)
enabled_tools = "iris_query,iris_search,iris_compile"
```

The equivalent env var is `IRIS_ENABLED_TOOLS` (see the table above). `IRIS_DISABLED_TOOLS`
wins for any name present in both lists.

See also: `provides.tools` in a project manifest (`docs/ecosystem-integration.md`) for
declaring a team-wide tool subset that installs automatically.

## Any MCP client (Antigravity, Cline, Continue, Zed, Claude Desktop)

The VS Code extension is a convenience wrapper. The MCP server itself is one static
binary, so any client that can launch a subprocess and speak MCP over stdio can use it —
including VS Code forks whose extension gallery does not carry the extension.

**Point the client at the binary:**

```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "command": "/usr/local/bin/iris-agentic-dev",
      "args": ["mcp"]
    }
  }
}
```

Most clients accept that shape; the key name and file location differ (look for "MCP" or
"Tools" in the AI settings). Connection details come from `.iris-agentic-dev.toml` in the
project root, so nothing IRIS-specific belongs in the client config — though an `env` block
of `IRIS_*` variables works too if the client supports one.

**Finding the binary.** Install it standalone (Homebrew, or the
[latest release](https://github.com/intersystems-community/iris-agentic-dev/releases/latest))
and use that path. If the extension already downloaded one, use the fixed-name copy it keeps
under the editor's extension storage —
`<storage>/intersystems-community.vscode-iris-agentic-dev/bin/iris-agentic-dev` (`.exe` on Windows). The
extension replaces that file on every update, so a client registered with it follows the extension.
Do not register the version-named directory beside it (`iris-agentic-dev-<version>/`): the client
keeps launching that version after the extension moves on. If a client is already registered that
way, the extension offers to update it when it starts, and changes nothing unless you choose
**Update**.

| Platform | `<storage>`                                             |
| -------- | ------------------------------------------------------- |
| macOS    | `~/Library/Application Support/Code/User/globalStorage` |
| Linux    | `~/.config/Code/User/globalStorage`                     |
| Windows  | `%APPDATA%\Code\User\globalStorage`                     |

Forks substitute their own directory for `Code` (`Cursor`, `Windsurf`, and so on). The
extension's Output channel logs the full path when it starts. Setting
`iris-agentic-dev.serverPath` in the editor's settings points the extension at a binary you manage
instead, which also tells you where it is.

**Verify before wiring anything.** The binary answers on its own, so a failure here is a
connection problem, not a client-config problem:

```bash
iris-agentic-dev tool check_config --args '{}'
```

Then, once the client is configured, ask it: `Call check_config and show me the result.` If
the tools do not appear at all, the client never launched the binary — check its MCP log for
the command line it tried.

### Claude Code on Windows: the plugin and the extension together

The Claude Code plugin (`/plugin install iris-dev`) brings the core skills, the compile-on-save
hooks and a server registration that runs `iris-agentic-dev mcp` by name. It does not install
or update the binary; it runs whichever `iris-agentic-dev` is on PATH and, from 1.6.4, says at
session start when that binary is older than the plugin. On a Mac, Homebrew keeps the binary
current (`brew upgrade iris-agentic-dev`). On Windows there is no Homebrew, but the VS Code
extension downloads every release and keeps the current one at a fixed path, so put that folder on
PATH once and the plugin always runs what the extension last downloaded:

1. Install the VS Code extension and open VS Code once, so it downloads the binary. Its Output
   channel logs the path; with stock VS Code it is
   `%APPDATA%\Code\User\globalStorage\intersystems-community.vscode-iris-agentic-dev\bin`.
2. Add that folder to your user PATH. In PowerShell:

   ```powershell
   $dir = "$env:APPDATA\Code\User\globalStorage\intersystems-community.vscode-iris-agentic-dev\bin"
   $user = [Environment]::GetEnvironmentVariable('Path', 'User')
   [Environment]::SetEnvironmentVariable('Path', "$user;$dir", 'User')
   ```

   Then open a new terminal and check `iris-agentic-dev --version`.

3. Install the plugin in Claude Code: `/plugin marketplace add intersystems-community/iris-agentic-dev`,
   then `/plugin install iris-dev`.
4. Remove any `iris-agentic-dev` entry you added to `~/.claude.json` by hand, or Claude Code runs
   two servers and lists every tool twice. The plugin's registration replaces it.
5. Restart Claude Code. After that, an extension update brings a new binary to the same path, and
   the next Claude Code session runs it.

---

## HTTP transport

By default, `iris-agentic-dev mcp` communicates over stdio — the standard MCP channel
for Claude Code and similar CLI-launched clients. For AI Hub remote MCP connections
(or any client that connects via HTTP rather than launching a child process), use the
HTTP transport instead:

```bash
iris-agentic-dev mcp --transport http --port 8080
```

The server binds on `127.0.0.1:8080` and accepts MCP connections at `/mcp`. `--bind` changes the
address; a bind to anything but loopback needs a token (see below).

### Token, Host and Origin

Every request passes a guard before the MCP server sees it, on every bind address:

- **Host.** `localhost`, `127.0.0.1` and `[::1]` are always served. A bind to a single address
  serves that address too. A bind to `0.0.0.0` or `::` also serves `host.docker.internal`, any IP
  literal, and each `--allowed-host <name>` (repeatable). Any other `Host` gets `403`.
- **Origin.** A request with no `Origin` header passes, since command-line and SDK clients send
  none. An `Origin` passes only when it is the same origin as the `Host` or is listed with
  `--allowed-origin <origin>` (repeatable). `Origin: null` and every other origin get `403`.
- **Token.** `Authorization: Bearer <token>`, compared in constant time. A missing or wrong token
  gets `401`.

The token is at least 32 bytes and comes from the operator, never from a project file. In order:

1. `IAD_HTTP_TOKEN` in the environment of the process.
2. `--auth-token-file <path>`, a file holding the token. No flag takes the token itself, because a
   value on the command line shows in `ps`.
3. `[transport] http_token = "..."` in `~/.iris-agentic-dev.toml`. The same key in a project's
   `.iris-agentic-dev.toml` is set aside and listed in `check_config`.

Generate one with `openssl rand -hex 32`. On a loopback bind the token is optional, and when one
is set it is required there too. A bind to any other address (`--bind 0.0.0.0`, `::`, or a LAN
address) with no token does not start: the server prints why and exits with status 2.

```bash
export IAD_HTTP_TOKEN="$(openssl rand -hex 32)"
iris-agentic-dev mcp --transport http --port 8080 --bind 0.0.0.0 \
  --allowed-host host.docker.internal
```

### Sessions

Each MCP session has its own stored results. `iris_get_log` lists and fetches what that session's
calls stored, `agent_history` reports that session's calls, and a pending source-control dialog can
be resumed only by the session that started it. Sessions share one set of servers: the connection
state, the pool (an `iris_reload_pool` through one session reaches the others), the credentials and
the configuration. A token authorises every session equally; it does not separate clients.

### AI Hub remote MCP

To connect an AI Hub agent to iris-agentic-dev over HTTP, start the server with
`--transport http`, then configure a `<Remote>` element in your ToolSet XData:

```xml
<ToolSet>
  <MCP Name="IrisAgenticDev">
    <Remote Url="http://127.0.0.1:8080/mcp"/>
  </MCP>
</ToolSet>
```

That works as written on a loopback bind with no token. Once a token is set, which a bind to
any other address requires, the client must send `Authorization: Bearer <token>` on every
request. An AI Hub instance in a container reaches the host as `host.docker.internal`, which a
`0.0.0.0` bind already serves; any other name it uses needs `--allowed-host`.

See [`contrib/aihub/README.md`](../contrib/aihub/README.md) for the full AI Hub
ToolSet setup guide.

---

## NoPWS — AI branch containers (2026.3+, irishealth-ai:\*)

AI branch IRIS builds (`irishealth-ai:*`, `iris-ai:*`, IRIS 2026.3+ Enterprise AI
editions) ship without an embedded web server (`WebServer=0` in `iris.cpf`). Atelier
REST is unavailable on these images, so the standard HTTP path fails with "connection
refused."

**Detect NoPWS:** Call `iris_test_server` — it reports `nopws: true` and
`nopws_detected` when it detects this condition. Or probe manually:

```bash
docker exec <container> sh -c \
  "grep -i WebServer /usr/irissys/iris.cpf 2>/dev/null || true"
```

Output `WebServer=0` confirms NoPWS.

**Configure for NoPWS:**

```toml
docker_only = true
nopws = true
container = "my-iris-ai-container"
```

Setting either `docker_only = true` or `nopws = true` activates the `docker exec`
path, bypassing Atelier REST entirely. `iris_execute` and `iris_compile` work;
tools requiring Atelier REST (`iris_doc`, `iris_source_control`) return
`NOPWS_ATELIER_REQUIRED`.

**Remote containers via SSH:**

```toml
docker_only = true
nopws = true
container = "iris-ai-prod"
ssh_host = "prodhost.internal"
```

Setting `ssh_host` routes `docker exec` through `ssh -o BatchMode=yes -o
StrictHostKeyChecking=accept-new <ssh_host>`. A host seen for the first time is recorded in
`known_hosts`; a host whose key has changed is refused (`SSH_HOST_KEY_MISMATCH`) and nothing
runs. Add `ssh_strict = true` to refuse an unknown host too, and run `ssh <ssh_host>` once
yourself to record its key first. `BatchMode=yes` turns a question iad cannot answer into a
failure instead of a hang. `ssh_host` takes effect from your own `~/.iris-agentic-dev.toml`
or a project file you have trusted with `iris-agentic-dev trust`; an untrusted project file
cannot choose the host your code runs on.

Every `iris_execute` and `iris_compile` response includes an `execution_path` field
(`"atelier"`, `"docker_exec_local"`, or `"docker_exec_ssh"`) to confirm which path
ran. See [skills/nopws-setup](../skills/skills/iris-agentic-dev/nopws-setup/SKILL.md)
for the full setup guide.

---

## Windows (Docker)

The native Windows binary is not yet signed. Windows users can run iris-agentic-dev
as a Docker container instead — see [docs/windows-docker.md](windows-docker.md) for
the full setup guide and Claude Code MCP config.
