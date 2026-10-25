# iris-agentic-dev

An MCP server and CLI that let an agent work against InterSystems IRIS, under limits set by the
person running it. These terms name the parties, files and connections those limits are about.

## Configuration sources

**Operator**:
The person who runs iad and is accountable for what it does on their IRIS servers.
_Avoid_: user (ambiguous with the IRIS user), developer

**Operator file**:
`~/.iris-agentic-dev.toml` in the operator's home directory, always read wherever the project lives,
and never treated as a workspace file.
_Avoid_: home file, global config, `~/.config/iris-agentic-dev/config.toml`

**Workspace file**:
The nearest `.iris-agentic-dev.toml` found by walking up from the working directory, stopping
before the home directory. Untrusted until the operator trusts it, wherever it sits.
_Avoid_: project file, repo file, local config

**Project directory**:
The one directory the tools that read the host's disk are confined to: `OBJECTSCRIPT_WORKSPACE`,
then `--workspace`, then the directory of the workspace file, then the working directory, made
canonical. Never the filesystem root or the home directory. With none, every host path is refused.
_Avoid_: workspace root, sandbox

**Launch settings**:
The environment variables and command-line flags iad was started with. Trusted, whoever wrote
them: approving a project's MCP server in a client means trusting the settings it launches iad with.
_Avoid_: env config, runtime config

**Named config file**:
A file given by `--config` or `OBJECTSCRIPT_WORKSPACE`. Judged like a workspace file: untrusted
unless it is the operator file or the operator has trusted it.
_Avoid_: explicit config

## Trust

**Loosening setting**:
A setting in a configuration file that widens what the agent may do or where credentials go: raising
the write or destructive gate, turning the compile-time gate off, allowing PHI reads, disabling TLS
validation, naming an SSH host, naming a per-server service account, or making a server's policy less
strict than the operator file's. Connection details, tool lists and anything that tightens are not
loosening settings.
_Avoid_: privileged setting, trust-relevant key, gate-raising key

**Trusted file**:
A workspace file or named config file whose loosening settings the operator has accepted. Trust
covers the file's path, the exact loosening settings it held when trusted, and the address of the
server each applies to; a change to any of them ends it.
_Avoid_: allowed file, approved workspace

**Operator-only setting**:
A setting honoured only from the operator file or launch settings, never from a workspace file,
trusted or not: trust entries, the opt-in to run agent-supplied code in an OS-authenticated session,
the HTTP transport token, the opt-in to serve local copies of bundled skills, and the opt-in to
keep full payloads in the local audit log.
_Avoid_: admin setting, protected key

**Trust entry**:
The record in the operator file that makes a file trusted: its canonical path and a fingerprint of its
loosening settings. Written only by the operator, through `iris-agentic-dev trust` at a terminal.
_Avoid_: allowlist entry

## Servers

**Server**:
A named IRIS that iad can reach, wherever it was declared: launch settings, a configuration file,
`servers.json` or VS Code Server Manager.
_Avoid_: instance (except for IRIS itself, as in "a Live instance"), pool member, connection, endpoint

**Endpoint**:
The address of a server's web server: scheme, host, port and path prefix, parsed once from whichever
source declared it. Two endpoints name the same server when they agree on all four, host compared
without case and every loopback name counting as one. Keychain items and Server Manager secrets are
keyed by it. It is where a server is, not the server: use _server_ for the thing itself.

**Default server**:
The server a call runs on when it names none.
_Avoid_: active connection, primary server

**Target server**:
The server a call actually runs on: the one it names in `server`, otherwise the default server. Every
limit on a call is judged against its target server.
_Avoid_: selected server, routed server

## Identity

**Identity**:
The IRIS account a call's code actually ran as, as IRIS reports it.
_Avoid_: user, login

**Connection account**:
The IRIS account iad connects to a server with, from launch settings or a configuration file. Not
necessarily the operator's own login.
_Avoid_: primary identity, primary user, admin user, operator account

**Service account**:
A restricted IRIS account the operator names for running agent-supplied code, separate from the
connection account.
_Avoid_: least-privilege user, exec user

**OS-authenticated session**:
An IRIS terminal session started without choosing an IRIS account (docker exec or SSH into
`iris session`), whose identity is whatever the instance maps the operating-system user to.
_Avoid_: docker identity, OS user

**Agent-supplied code**:
Code whose content or target the agent chose and IRIS runs: executed ObjectScript, a chosen class
method, write SQL and procedure calls, global sets and kills, and test classes. It runs as the
service account when one is configured.
_Avoid_: user code, arbitrary code

**Code store**:
The class and routine sources on a server and what writes them: putting and compiling documents, and
source control. It runs as the connection account so source-control actions keep that attribution.
_Avoid_: document store, source tree

## Policy

**System mode**:
IRIS's own label for an instance: Live, Test, Development or Failover. A fact about the server,
not something iad sets.
_Avoid_: environment, mode, live flag

**Environment template**:
The operator's per-server choice of which tool categories are blocked: dev, test or live
(`mcpTemplate`). When none is declared, the server's system mode chooses it.
_Avoid_: mcp template, profile, mode

**Server role**:
How a server relates to the work: a **workbench** is a server the agent builds on; a **subject** is a
server under study, where source-control writes are refused and other writes need confirmation.
_Avoid_: workspace (as a role), control-plane, instance role

**Effective policy**:
The policy a call is judged by on its target server, combined part by part from the operator file
and the workspace file. Each part takes the stricter of the two; a loosening part from an untrusted
file is dropped on its own, and the rest of that file's policy still applies. A trusted file's
part replaces the operator file's.
_Avoid_: merged policy, resolved policy, active policy

**Data policy**:
A server's rule for stored content the agent could read: block, redact or allow, strictest first.
_Avoid_: PHI policy

**Bulk-data tool**:
A tool that returns stored message, stream or journal content. Under block it returns at most the
content's metadata; under redact it returns the content with sensitive fields replaced, or is
refused if it cannot redact; under allow it returns the content whole. `iris_interop_query` is one
only for a call that names a message body (`body_class`, `body_where` or `body_select`).
_Avoid_: bulk-PHI tool

**SQL lexer**:
The one reader (`sql_lex`) every SQL validator uses, following IRIS: a quote is doubled to be
included, `\` is an ordinary character, and `--` and `/* */` are comments only outside a quote.
_Avoid_: SQL scanner, comment stripper

**Sensitive-named global**:
A global whose name matches the sensitive-name patterns; reading it needs an explicit
acknowledgement in the call.
_Avoid_: PHI global

**Tool category**:
The kind of work a tool does (compile, execute, query, search, docs, source control, debug, admin,
skill, kb); a server's `allow` list and environment template are written in categories.
_Avoid_: tool group, permission

**Acknowledgement**:
A flag the agent sets in its own call to lift a check meant to catch mistakes: `confirm` on a
subject server or on a destructive call, `acknowledgePhi` on a sensitive-named global. It protects nothing from the agent;
anything that must hold against the agent is lifted only by an operator setting. `force` is not an
acknowledgement: a forced call is judged by what it actually runs.
_Avoid_: override, bypass flag

## Refusals

**Refusal**:
iad declining a call before IRIS runs anything. Every refusal says which kind it is, so the agent
knows who can lift it.
_Avoid_: block, denial, gate error

**IRIS error**:
IRIS itself declining or failing a call, such as a missing privilege. Never called a refusal.
_Avoid_: IRIS refusal

**Acknowledgement required**:
A refusal the agent lifts by repeating the call with an acknowledgement.

**Operator limit**:
A refusal the operator lifts by changing a setting in the operator file, launch settings or a
trusted file.
_Avoid_: policy error, config block

**Untrusted setting**:
A refusal that a loosening setting would have lifted, had the file holding it been trusted. The
operator lifts it by trusting the file.
_Avoid_: ignored config

**Fixed rule**:
A refusal nobody lifts from configuration or from the call, such as the system blocklist or a
source-control write on a subject server.
_Avoid_: hard block, non-configurable gate

**Set-aside item**:
Something iad found in the configuration and did not act on: a loosening setting from an untrusted
file, or a credential withheld from a server it was not meant for. Each is reported with its source
and what would lift it, before any call is refused.
_Avoid_: ignored setting, dropped config, warning
