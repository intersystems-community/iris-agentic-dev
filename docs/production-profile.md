# Production profile

This is for a security or compliance reviewer deciding whether iris-agentic-dev may run against an IRIS
instance that holds personal or health information (a UAT or LIVE environment). It says what the program
does with data, which settings reduce the risk, and what no setting protects. It describes the binary at the
version you install; check `iris-agentic-dev --version` and the release notes.

## What it is

A local process that speaks the Model Context Protocol over stdio to an AI coding assistant (Claude Code,
VS Code, opencode and others) and talks to IRIS over its HTTP (Atelier REST) interface, or by `docker exec`
for a local container. Over stdio it listens on no network port. The optional HTTP transport
(`mcp --transport http`) does, on `127.0.0.1` unless you pass `--bind`; it checks `Host`, `Origin` and a
bearer token on every request, and a bind to any other address will not start without a token
(see [connecting.md](connecting.md#http-transport)). It holds no data of its own beyond a short
in-memory log buffer and local files you configure: the audit log
(`~/.iris-agentic-dev/audit.jsonl`, payload text reduced to sizes unless you opt in), the optional
telemetry file, and `servers.json`.

## Data flow

| From          | To                                                                    | When                                                                                      | What travels                                             |
| ------------- | --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| the assistant | this process (stdio)                                                  | every tool call                                                                           | tool name and arguments                                  |
| this process  | the assistant (stdio)                                                 | every tool result                                                                         | whatever IRIS returned for that call                     |
| this process  | the configured IRIS instance                                          | every tool call that needs IRIS                                                           | the request, the configured username and password        |
| this process  | `EP91R43SFK-dsn.algolia.net` (the docs.intersystems.com search index) | only `iris_doc_search`                                                                    | the search text the assistant wrote; no IRIS credentials |
| this process  | `raw.githubusercontent.com`                                           | only on `skill install`/community-skill commands you run                                  | a request for a public skill file; no IRIS data          |
| this process  | `api.github.com`                                                      | only on package-manifest version resolution you run                                       | a public repository lookup; no IRIS data                 |
| this process  | `api.anthropic.com` or `api.openai.com`                               | only the code-generation tools, and only if `IRIS_GENERATE_CLASS_MODEL` and a key are set | the prompt for the generated class                       |

I found no other outbound destination in the source. I first missed the documentation search above,
because my search filtered out `docs.` hostnames, and the table now lists it. There is no telemetry upload and no update check. The
`OPENAI_API_KEY`/`ANTHROPIC_API_KEY` variables are read only by the code-generation tools; unset them and
that route is closed. The model decides which key is read: `ANTHROPIC_API_KEY` for a Claude model and
`OPENAI_API_KEY` for any other, and each key goes only to its own provider's URL. Skills are text files compiled into the binary.

**The assistant is the larger data path.** Everything a tool returns goes to the model provider behind your
assistant, under that provider's terms. This program cannot see or control that hop. The decision about
whether PHI may reach that provider is yours and is separate from this program.

## Settings that reduce the risk

Put these in `~/.iris-agentic-dev.toml`, your **operator file**, one `[policy.<server>]` block per
connection. The server name is the one you gave the connection. A `.iris-agentic-dev.toml` in a
repository can tighten any of them, and cannot loosen them until you run `iris-agentic-dev trust` on
it at a terminal (see [Trust](connecting.md#trust)). Do not put the profile in a repository file and
assume it holds against a repository you do not control; do put the operator file where only
administrators can change it.

```toml
[policy.prod-live]
# Only the categories the task needs. No admin, no execute, no source_control.
allow = ["compile", "search", "skill"]
# live: the most restrictive template. Leave it out and the server's IRIS system mode chooses it
# (a Live or Failover instance gets "live"), so a Live instance is covered even without this line.
mcpTemplate = "live"
# block (the default): journal_search, iris_message_body and stream_inspect, which return whole
# records, are refused or return metadata only.
dataPolicy = "block"
# Adds to the built-in list of globals that writes and kills may not touch. It does not stop reads.
globalBlocklist = ["^AppHealthData"]
# Write an audit record for every tool call (one-time setup in docs/agent-attribution.md).
irisAudit = true
```

Also:

- **Documentation search is outbound.** `iris_doc_search` sends text the model composed to a third-party
  search service. It belongs to the `docs` category with `iris_doc`, `docs_introspect` and `iris_generate`, so
  leaving `docs` out of `allow` closes it and also removes those tools. The example above leaves it out.
- **Read-only mode.** Set `write_tools_enabled = false` (or `[policy.<server>] writeTools = false`) for the
  server. Leaving it unset is not read-only: with nothing declared the write tier follows the instance's
  system mode, which is on for Development and Test. The destructive and admin tiers
  (`IRIS_DESTRUCTIVE_TOOLS_ENABLED`, `IRIS_ADMIN_TOOLS`) are off by default; leave them off.
- **Strict read-only.** Most read tools run their ObjectScript through a scratch class that iad puts,
  compiles and deletes in the `IrisDevTmp` package on the server, which is a write. Where iad must write
  nothing, such as a production read replica, set `read_only = "strict"` (or `IAD_READ_ONLY=strict`); the
  tools that need a scratch class are then off and unlisted. It cannot be combined with `irisAudit`. See
  [connecting.md](connecting.md#strict-read-only-no-scratch-classes).
- **Least-privilege IRIS account.** Create an account whose roles cannot read the protected tables and
  globals, and configure that one. This is the control that holds when everything else fails, because the
  server cannot do more than the account can. Do not use `_SYSTEM` or a `%All` account.
- **A service account for the agent's code.** Set `IRIS_SERVICE_USERNAME` and `IRIS_SERVICE_PASSWORD`, or a
  `service_username` per server. `iris_execute`, `iris_query` writes, `iris_global` set/kill,
  `iris_execute_method`, `global_kill`, `iris_test`, `iris_coverage` run/start and the WebSocket terminal
  then run as that account on every path, or the call is refused; each response says who ran it in
  `identity`. Without it they run as the connection account. The account still needs `%Development:USE`,
  and database permission is what stops it editing code; see
  [connecting.md](connecting.md#service-account-for-the-agents-code) for a layout that works. Read-mode
  `iris_query` can still call a stored procedure as the connection account.
- **Use TLS.** Set the connection scheme to `https` and leave certificate validation on. `tls_verify = false`
  is per connection, and a repository file's counts only after you trust it. `IRIS_TLS_VERIFY=false` and
  `IRIS_INSECURE` affect only the server your launch settings name. Do not set them for a server holding PHI.
- **No skills if you do not want them.** `IRIS_NO_SKILLS=true` removes the skill, knowledge-base and learning
  tools from the tool list. Skills served from a directory in the project are off unless you opt in with
  `local_skills`, so a repository cannot replace the guardrail skills.
- **No code-generation route.** Leave `IRIS_GENERATE_CLASS_MODEL`, `OPENAI_API_KEY` and `ANTHROPIC_API_KEY`
  unset in the environment of the process.
- **Credentials.** Keep them in the OS credential store or environment, not in the TOML file in the repository.
  Discovery sends no credential to a listener until it has shown it is IRIS, and a project file that names a
  host does not receive your environment's credentials unless that host is this machine or the server your
  launch settings name.
- **Audit.** `irisAudit` writes the caller marker (see `docs/agent-attribution.md`) so IRIS's own audit log
  shows which assistant made each call.
- **HTTP transport.** If you run it, set `IAD_HTTP_TOKEN` (at least 32 bytes) even on loopback, and name only
  the hosts and origins you need with `--allowed-host` and `--allowed-origin`.

## Fixed in 1.6.0: tool parameters that ran as ObjectScript

In 1.5.0 and earlier, about forty tool parameters were put into generated ObjectScript, SQL or a URL
without escaping. A crafted value could close the literal it landed in and run code. With write tools
off, such a value deleted a global through `iris_global` (`global_name`), `iris_database_stats` (`db`,
in `%SYS`), `iris_coverage` (`classes`) and `stream_inspect` (`oid`), and reached generated code
through about ten more read-only tools. `global_preview` also skipped the system blocklist and PHI
gates, and the standalone `journal_search` skipped the bulk-PHI gate.

1.6.0 fixes every site found (spec 137): names are checked against IRIS syntax before any
call, values are escaped for the literal they land in, SQL values are bound as parameters, and the
three tools go through the policy gate. A check in the release gates fails the build when a new
unescaped splice appears. Before 1.6.0, do not treat read-only mode as a boundary against text an
assistant was given to read.

## What 1.7.0 changed

A security review of 1.6 found places where a repository, a second server in the pool or a local process
could get more than the operator chose. The ones a reviewer asks about:

- **A repository file can no longer widen the agent's powers.** A project `.iris-agentic-dev.toml` or a
  `--config` file loosens nothing until the operator runs `iris-agentic-dev trust` at a terminal. What it
  tightens applies at once. `check_config` lists everything set aside.
- **Gates judge the server the call runs on.** Through 1.6 a gate could read the default connection while the
  call ran on another server; each call now resolves its target once and every gate, role and policy part
  is that server's.
- **Policy defaults follow the instance.** With no `mcpTemplate` or write tier declared, a Live or Failover
  instance gets the `live` template and writes off.
- **The agent's code runs as a service account or is refused**, never quietly as the connection account.
- **The compile-time gate** refuses documents that would run code while they compile.
- **Credentials go only where they were meant for:** discovery identifies IRIS first, keychain items are
  tied to the server's address, and a repository cannot name a host to receive your password.
- **The HTTP transport** requires a bearer token off loopback and checks `Host` and `Origin`.

## What no setting protects

- **`iris_query` and `iris_execute` are free-form.** They run SQL or ObjectScript the assistant composes.
  The protected-data checks look at global names and tool categories; they do not parse every query, and
  there is no automatic redaction of results from these tools. A permitted account that can select from a
  patient table can have that data returned to the assistant. Leave these tools out of `allow` if that matters.
- **The PHI gate is a speed bump, not a lock.** It stops the accidental case. `acknowledgePhi` removes it for
  a call, and indirection in ObjectScript (`Kill @name`) is not seen by a text check.
- **`dataPolicy = "redact"` is partial.** `iris_message_body` and `stream_inspect` replace the identifying
  fields of the HL7 v2 PID, NK1, GT1, IN1 and IN2 segments, and MSH-3 (names, ids, birth dates, addresses,
  phones, SSNs; the list is under `iris_message_body` in `docs/tools.md`), and refuse any other content. A
  result marked `redacted: true` is not de-identified: a name in a free-text field (OBX, NTE) or in a segment
  not on the list comes back as stored. `journal_search` is not redacted at all: it
  needs `allow`. `iris_query`, `iris_execute` and the other tools return what they read.
- **The trust prompt proves a terminal, not a person.** An agent whose shell runs commands in a
  pseudo-terminal can run `iris-agentic-dev trust`. What holds is that you approve that shell command.
- **The model provider's side.** Prompt injection from data the assistant reads (a field containing
  instructions) can steer tool calls. The policy limits what those calls can do; it does not prevent them.
- **Binaries are unsigned.** The macOS build carries an ad-hoc signature and the Windows release binary
  carries none. Verify the SHA-256 against the release or Homebrew formula, or build from source at a tag.

## Installing with a pinned version and no download

Do not use `releases/latest`: it moves with every release, so what you reviewed is not what you run. Pin a
tag, check the hash, and put the binary where only administrators can change it. Use the file for your
platform; `linux-x86_64` is shown.

```sh
base=https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.5.0
curl -fsSLO $base/iris-agentic-dev-linux-x86_64
curl -fsSLO $base/SHA256SUMS
sha256sum -c --ignore-missing SHA256SUMS
install -m 0755 iris-agentic-dev-linux-x86_64 /opt/iris-agentic-dev/iris-agentic-dev
```

On Windows, compare `(Get-FileHash .\iris-agentic-dev-windows-x86_64.exe).Hash` with the line in `SHA256SUMS`.
Where the host cannot reach GitHub, do the download and the check on another machine and copy the binary in.

The VS Code extension downloads the binary on first activation only when no binary is found. Setting
`iris-agentic-dev.serverPath` to the installed path, or having the binary on `PATH`, makes that a no download
install: the extension runs what is there and fetches nothing. The setting is checked before the `PATH` lookup,
and the managed download is the last resort. The extension checks that download against a SHA-256 it carries for
each release asset, and only the GitHub release hosts are followed; a hash mismatch deletes the file and installs
nothing. `serverPath` can be set in user or remote settings only, not in a workspace.

## Evidence

- Dependency scans and their results: `SECURITY.md`. The SBOM for each release is
  `iris-agentic-dev-<version>.cdx.json` on the release page.
- Gate behaviour is covered by tests that run against a live IRIS container (`cargo test --features testing`).
- No independent penetration test has been done.

## Suggested rollout

1. Run it against a non-production copy first with the profile above and read the audit records.
2. Connect production with an account limited to what the task needs, `mcpTemplate = "live"` and
   `write_tools_enabled = false` in your operator file, and a service account for the agent's code.
3. Re-review when the version changes: new tools are added over time, and `allow` is the list that bounds them.
