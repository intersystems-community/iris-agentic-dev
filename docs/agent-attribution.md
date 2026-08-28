# Agent attribution and audit

This guide explains what IRIS can see when `iris-agentic-dev` calls it, how to tell agent
traffic from developer traffic in logs and audit records, and how to restrict agents on
specific environments.

## What IRIS can see — and what it cannot

Every HTTP caller reaches IRIS through the Web Gateway. The gateway process exposes itself
to IRIS as `CSPa24.so` (or the equivalent module), so `$System.Process.ClientExecutableName()`
is always the gateway module and can never distinguish an agent from a developer's IDE.
`$System.ClientNodeName()` is always the gateway host for the same reason.

The only field IRIS records that a caller controls is **`User-Agent`**, readable inside any
HTTP request as:

```objectscript
Write %request.CgiEnvs("HTTP_USER_AGENT")
```

Before this feature, `iris-agentic-dev` sent no `User-Agent` header. IRIS saw
`HTTP_USER_AGENT` as empty, so the Postman-vs-Chrome filtering technique the Web Gateway docs
describe could not work — the gap was in the tool, not in IRIS.

## The caller marker

Every IRIS-bound request from `iris-agentic-dev` now sends a `User-Agent` header in this form:

```text
iris-agentic-dev/<version> (<mode>[; <label>][; <client>/<version>])
```

| Field     | Source                               | Example         |
| --------- | ------------------------------------ | --------------- |
| `version` | Workspace crate version              | `1.2.7`         |
| `mode`    | How the binary was invoked           | `mcp` or `cli`  |
| `label`   | `IRIS_AGENT_LABEL` env var, optional | `build-agent-7` |
| `client`  | MCP client's `clientInfo.name`       | `claude-code`   |

**Full examples:**

| Situation                                 | Marker                                                           |
| ----------------------------------------- | ---------------------------------------------------------------- |
| MCP session, no label, no `clientInfo`    | `iris-agentic-dev/1.2.7 (mcp)`                                   |
| MCP session, label set, client identified | `iris-agentic-dev/1.2.7 (mcp; build-agent-7; claude-code/2.1.0)` |
| One-shot CLI dispatch                     | `iris-agentic-dev/1.2.7 (cli)`                                   |
| CLI dispatch with `IRIS_AGENT_LABEL` set  | `iris-agentic-dev/1.2.7 (cli; nightly-ci)`                       |

### Label sanitizing

The label is stripped of CR, LF, and other control characters before use, and capped at 64
characters. Injection attempts survive as visible text rather than being silently dropped, so a
label like `"team a\r\nX-Evil: 1"` becomes `iris-agentic-dev/1.2.7 (cli; team a X-Evil: 1)`
in the log — the injected header name is recorded as plain text and cannot split the request.

### Setting the label

```sh
export IRIS_AGENT_LABEL=prod-deploy-agent
iris-agentic-dev mcp
```

Fleet setups can stamp each agent instance with a unique label so their lines in the access log
are distinguishable even when they share credentials.

## Filtering agent traffic in access logs

**IIS / Apache / Web Gateway** all log `User-Agent`. Filter lines that contain
`iris-agentic-dev/`:

```sh
# Apache / Web Gateway combined log format
grep 'iris-agentic-dev/' /path/to/access.log

# IIS W3C log (User-Agent is in the cs(User-Agent) column)
Select-String 'iris-agentic-dev/' access.log
```

**PWS-only deployments** (the IRIS private web server): PWS keeps only an error log, not an
access log. The marker is still visible inside ObjectScript via `%request.CgiEnvs` and in
`%SYS.Audit` records (if enabled), but it does not appear in any log file.

## Native IRIS audit events

IRIS ships with `%SYS.Audit` and `Security.Events` but the relevant events are off by default.
After 24 hours of heavy agent traffic on a freshly started container, the only audit records
were login failures — not because auditing was broken but because `RoutineChange` and the SQL
events are not enabled out of the box.

### Enabling RoutineChange

`RoutineChange` fires once every time code is compiled. Because `iris-agentic-dev` routes all
code edits through Atelier document PUT (enforced by the code-edit guard inside `iris_execute`),
every compile produced by an agent call appears in this audit trail.

```objectscript
; Enable without a restart
Set p("Enabled") = 1
Do ##class(Security.Events).Modify("%System", "%System", "RoutineChange", .p)

; Verify
Do ##class(Security.Events).Get("%System", "%System", "RoutineChange", .p)
Write p("Enabled")   ; 1
```

Once enabled, agent compile events look like:

```text
14:25:52.448  RoutineChange  u=_SYSTEM  ns=USER  IrisDevTmp.IrisDevRunf420fb30d940.cls
```

The temp-class name `IrisDevTmp.IrisDevRun<hash>` is the fingerprint for `iris_execute`: every
invocation compiles a temporary routine with that prefix, so agent execution is already
distinctive in the `RoutineChange` trail without any additional tooling.

**What `RoutineChange` records carry vs. what it does not:**

| Available                           | Not available                       |
| ----------------------------------- | ----------------------------------- |
| Username, Roles, Namespace          | ClientIPAddress — always empty here |
| RoutineSpec (class or routine name) | ClientExecutableName — always empty |
| Timestamp, Process info             | CSPSessionID — always empty         |

`RoutineChange` is written by IRIS, so it is trustworthy — no caller can skip it. But it cannot
tell you which client sent the request (that information is not propagated into the audit record
for native events). For client-level attribution, use the `irisAudit` key (below) or query the
Web Gateway access log.

### Enabling SQL auditing

`%System/%SQL/*` events are off by default. There are 20 of them; enabling them all on a
production system adds a write to every query. Consider enabling selectively:

```objectscript
; Enable only DDL events (CREATE TABLE, ALTER, DROP) — lower volume
Set p("Enabled") = 1
Do ##class(Security.Events).Modify("%System", "%SQL", "DDL", .p)

; Check all SQL events and their state
Do ##class(%ResultSet).%New("%SYS.Audit.Security:List")
```

On busy systems, `DynamicStatementDML` (covers JDBC and `%SQL.Statement` executes) generates
one record per query. Measure volume in a test environment before enabling on production.

## opt-in `%SYS.Audit` emission (`irisAudit`)

Set `irisAudit = true` in a `[policy.<server>]` block to make `iris-agentic-dev` emit a
`%SYS.Audit` record for every tool call on that connection:

```toml
[policy.prod-iris]
mcpTemplate = "Live"
irisAudit = true   # emit %SYS.Audit records for every tool call
```

For flat single-server configs (no named server entry), use the `default` catchall key:

```toml
# flat .iris-agentic-dev.toml (host/port at top level — no [server.*] section)
host = "my-iris-server"
web_port = 52773

[policy.default]
irisAudit = true
```

Each record carries the full caller marker in `EventData`, plus `ClientIPAddress` and
`CSPSessionID` that native `RoutineChange` records omit:

```text
EventSource    = iris-agentic-dev
EventType      = Tool
Event          = ToolCall
EventData      = tool=iris_execute mode=mcp ua=iris-agentic-dev/1.2.7 (mcp; build-agent-7; claude-code/2.1.0)
ClientIPAddress = 192.168.215.1
CSPSessionID    = eFnNTL6kBm
```

**Before enabling**, the `Security.Events` entry must exist on the target IRIS instance:

```objectscript
Do ##class(Security.Events).Create("iris-agentic-dev", "Tool", "ToolCall", "iad tool invocation", 1)
```

If the entry is absent when a tool call fires, `iris-agentic-dev` logs a warning with the exact
`Create` call to run and skips emission — it never fails the tool call because of a missing
audit event definition.

**Read records back:**

```objectscript
Set rs = ##class(%ResultSet).%New("%SYS.Audit:List")
Do rs.%Execute()
While rs.%Next() {
    If rs.Get("EventSource") = "iris-agentic-dev" {
        Write rs.Get("UTCTimeStamp"), " ", rs.Get("Event"), " ", rs.Get("EventData"), !
    }
}
```

Note: `%SYS.Audit` has a known replication lag (`DP-449511`) — records are written
asynchronously and may not appear immediately after the tool call returns.

### Trust asymmetry

`RoutineChange` and `%SQL/*` records are **written by IRIS** and cannot be skipped by a
caller. `irisAudit` records are **written by `iris-agentic-dev`** — a non-iad caller or a
rebuilt binary writes none. Both belong in a complete attribution picture but they are not equal
evidence. Combine them: native events for what IRIS did, `irisAudit` for which agent and which
tool did it.

## Restricting agents per environment

The marker is caller-asserted. An operator can use it to build default-deny rules in the Web
Gateway or a reverse proxy:

```apache
# Apache: block requests that lack the iris-agentic-dev marker on /api/atelier/
RewriteEngine On
RewriteCond %{HTTP_USER_AGENT} !iris-agentic-dev/
RewriteRule ^/api/atelier/ - [F,L]
```

This is useful for blocking **anonymous** clients. It is **not** a security boundary against a
hostile caller who can send any `User-Agent` string they like. For a genuine security boundary
that a motivated attacker cannot bypass, use distinct IRIS credentials and roles per
environment — the original partner advice remains correct:

> "There is no built-in way to differentiate a programmer from an agent other than to use
> different credentials."

Distinct credentials mean:

- An agent account can be disabled on a production environment without touching developer access.
- `RoutineChange` and `%SQL` audit records carry the agent's username, making the trail
  attributable even when `ClientIPAddress` is empty.
- Role restrictions (`%DB_<database>_WRITE`, `%Development`) can be removed from the agent
  account on environments where code compilation should be blocked entirely.

The marker and `irisAudit` emission add visibility. Credentials and roles add enforcement.
Use both.

### A note on the write and destructive tool gates

The write and destructive gates (`write_tools_enabled`, `destructive_tools_enabled` and their
per-tool equivalents in `[policy.<server>]`) run inside the agent's own process, evaluated
against the agent's own config file. A `curl` command, a Postman request, another MCP server,
or a rebuilt binary ignores them entirely. They cannot enforce "no agents on this environment"
— only IRIS-side controls can do that.

Their real value is different: per-tool granularity that IRIS resource permissions cannot
express. You can deny `iris_ws_exec` while allowing `iris_doc` reads, in the same credential
session. IRIS roles operate at the privilege level, not the individual-tool level.

Use both layers for what each does best: IRIS credentials and roles for environment-level
enforcement; the tool gates for fine-grained operational safety within a permitted session.

## `docker_only` connections

Connections configured with `docker_only = true` route all operations through
`docker exec … iris session` rather than HTTP. No HTTP request exists, so no `User-Agent`
header exists. `iris-agentic-dev` logs a warning once when a tool call is made on such a
connection, naming the transport and the consequence.

`docker_only = true` is mandatory for Enterprise 2026.2.0AI builds, which ship without a
private web server (DPP-1192). On those instances, agent attribution depends entirely on the
`RoutineChange` audit trail (which captures the IRIS username) and on network-level controls.

## Reference: which requests carry the marker

| Path                              | Marker present after this release |
| --------------------------------- | --------------------------------- |
| Atelier document read/write       | Yes (was already present)         |
| Atelier action/query (execute)    | Yes (was already present)         |
| Discovery probe (`/api/atelier/`) | Yes — T013                        |
| Localhost port scan               | Yes — T013                        |
| Docker container probe            | Yes — T013                        |
| Search `sync_client`              | Yes — T016                        |
| Batch document fetch              | Yes — T017                        |
| CSP cookie fetch (WS pre-auth)    | Yes — T014                        |
| WebSocket upgrade handshake       | Yes — T015                        |
| `docker exec` (docker_only)       | No — no HTTP request exists       |
| LLM provider calls                | No — not IRIS-bound               |
| Registry / package downloads      | No — not IRIS-bound               |

## FAQ

**I run multiple agent instances against the same IRIS server. Can I tell them apart?**

Set a unique `IRIS_AGENT_LABEL` per instance — the label appears in the marker and in every
`irisAudit` record. Fleet configs can stamp each agent with its container name, job ID, or
environment tier so their lines in the access log and audit table are distinguishable even when
they share credentials.

**Can an attacker spoof the User-Agent to impersonate iris-agentic-dev?**

Yes. Any HTTP client can send any `User-Agent` string. Treat the marker as an audit and
filtering aid, not a security boundary. For genuine enforcement — blocking a class of caller
that a motivated attacker cannot bypass — use distinct IRIS credentials and roles per
environment, and consider network-level controls (firewall rules, mTLS) at the Web Gateway.

**The `irisAudit` records aren't appearing in `%SYS.Audit` immediately after a tool call.**

`%SYS.Audit` has a known replication lag (DP-449511) — records are written asynchronously.
Wait a few seconds and re-query. If records never appear, confirm the `Security.Events` entry
exists (`Do ##class(Security.Events).Exists("iris-agentic-dev","Tool","ToolCall",.e,.s) Write e`)
and that `irisAudit = true` is set on the correct server key in `[policy.<server>]`.
