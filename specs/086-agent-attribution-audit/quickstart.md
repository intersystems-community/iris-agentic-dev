# Quickstart: Telling agent work from human work in IRIS

**Feature**: 086-agent-attribution-audit | **Date**: 2026-08-27

Five minutes, an operator's path from "I cannot tell who did this" to a filterable trail. Every
command below was run against IRIS Community 2026.2.

## 1. See the marker arrive

From a working namespace:

```objectscript
Write %request.CgiEnvs("HTTP_USER_AGENT")
```

Before this feature that printed nothing. Now:

```text
iris-agentic-dev/1.2.7 (mcp; build-agent-7; claude-code/2.1.0)
```

`mcp` is an agent session; `cli` is a one-shot dispatch from a script, hook or CI step. The
middle field is whatever the operator set in `IRIS_AGENT_LABEL`. The last is the MCP client that
connected.

Set the label where the agent runs:

```bash
export IRIS_AGENT_LABEL="build-agent-7"
```

Keep the label ASCII. It ends up in audit `EventData`, and `%SYS.Audit::Export()` throws
`<ILLEGAL VALUE>` on a record containing a Unicode character (DP-446307), so a non-ASCII label
would break export of the whole log. Non-ASCII characters are stripped.

## 2. Filter agent traffic in the access log

`User-Agent` is recorded by IIS, Apache and the Web Gateway:

```bash
grep 'iris-agentic-dev/' access.log | grep '(mcp'          # agent sessions only
grep 'iris-agentic-dev/' access.log | grep -v '(mcp'       # scripted/CI dispatches
```

The Private Web Server keeps no access log — only an error log. On a PWS-only instance, read the
marker from ObjectScript (step 1) or from `%SYS.Audit` (step 4) instead.

## 3. Turn on the native audit events that matter

Auditing is already on out of the box (`Security.System.Get` reports `AuditEnabled=1`), but the
events that capture development work are off. Nothing here needs a restart. In `%SYS`:

```objectscript
Set p("Enabled")=1
Set tSC = ##class(Security.Events).Modify("%System","%System","RoutineChange",.p)
Write $SYSTEM.Status.GetErrorText(tSC)
```

Then read it back. Use the `%SYS.Audit` **List query**, not a `SELECT` — a `SELECT` on
`%SYS.Audit` can return stale data because it does not refresh the indices first, which the List
query does (DP-449511). Arguments are positional:
`BeginDateTime, EndDateTime, EventSources, EventTypes, Events, Usernames, SystemIDs, Pids, Namespaces, Authentications, Flags, JSONSearch, MaxRows`.

```objectscript
Set r = ##class(%ResultSet).%New("%SYS.Audit:List")
Do r.Execute("","","%System","","RoutineChange","","","","","","","",10)
While r.Next() { Write r.Get("UTCTimeStamp")," ",r.Get("Username")," ",r.Get("Description"),! }
```

Agent code changes appear immediately. This tool's ad-hoc execution has a distinctive
fingerprint — every `iris_execute` compiles a temp routine named `IrisDevTmp.IrisDevRun<hash>`:

```text
14:25:38.193 RoutineChange u=_SYSTEM ns=USER ^|^^/usr/irissys/mgr/user/|IrisDevTmp.IrisDevRunf420fb30d940.cls
```

Consider also `%System/%SQL/DDL`, `%System/%SQL/DynamicStatementDML` and `%System/%Login/Login`,
all off by default. (`LoginFailure` is on; `Login` is not.)

Turn the `%SQL/*` family on deliberately, not by reflex. SQL auditing raises global-reference
counts enough that IRIS's own performance tests fail with it enabled (DP-430959), so scope it to
the user you care about using selective SQL auditing where your version has it (DP-429579) rather
than enabling it instance-wide. And enable it before clients connect: on connections already open
when `%SYSTEM:%SQL:XDBCStatement` is switched on, records arrive with empty `EventData` until the
client reconnects (DP-418348, open). `RoutineChange` has no such cost and is the one to start with.

**Know the limit before you rely on it.** `RoutineChange` records carry `Username`, `Roles`,
`Namespace` and `RoutineSpec`, but `ClientIPAddress`, `ClientExecutableName` and `CSPSessionID`
come back **empty**. Native code-change audit tells you which _user_, never which _client_. That
is why the next step matters, and why distinct per-environment credentials are not a workaround
for a missing feature — they are the mechanism that makes this trail attributable.

### On IRIS 2026.3.0 and later: the platform audits its own MCP servers

2026.3.0 adds a `%MCP` audit type with `%System/%MCP/ToolCall` and `%System/%MCP/ToolDiscovery`.
The event type ships enabled (DP-452957), but that alone records nothing: recording is gated
twice, and you also need an audit policy applied to the MCP server or tool set. In ISC's words,
"enabling the system audit event type alone does not automatically audit all MCP tool calls"
(DP-445295). Read the records with:

```objectscript
Set r = ##class(%ResultSet).%New("%SYS.Audit:List")
Do r.Execute("","","%System","%MCP","","","","","","","","",10)
While r.Next() { Write r.Get("UTCTimeStamp")," ",r.Get("Event")," ",r.Get("Username")," ",r.Get("EventData"),! }
```

Read the scope carefully. Those events fire for MCP servers **IRIS hosts** — tools IRIS itself
serves to a client. iris-agentic-dev is an external MCP server that talks to IRIS over Atelier
REST, so its calls produce no `%MCP` records; to IRIS it is an HTTP client. DP-445295 puts it
plainly: "REST requests and other access paths continue to use their existing auditing and logging
mechanisms."

So an empty `%MCP` result means one of three things — no hosted MCP activity, no audit policy
applied, or an external agent that was never in scope. It never means no agent touched the
instance. That is what steps 1–2 and step 4 are for. ISC also recommends OpenTelemetry over the
audit database for high-volume MCP observability, so do not plan on `%MCP` as a per-call ledger.

## 4. Optional: have the tool record its own calls

This adds the tool name and MCP client identity that native records omit. It is opt-in and off by
default, and it is self-reported — a different client, or a rebuilt binary, writes nothing. Read
it as intent, not as proof.

Create the event once, as an operator, in `%SYS`. The tool will not do this for you: creating an
audit event is a `%SYS` security write, and an agent that can enable its own auditing can disable
it.

```objectscript
Set tSC = ##class(Security.Events).Create("iris-agentic-dev","Tool","ToolCall","iris-agentic-dev tool invocation",1)
Write $SYSTEM.Status.GetErrorText(tSC)
```

Then enable it for the connection you care about, in `.iris-agentic-dev.toml`:

```toml
[policy.prod-iris]
mcpTemplate = "Live"
irisAudit = true
```

Read the records back, again through the List query:

```objectscript
Set r = ##class(%ResultSet).%New("%SYS.Audit:List")
Do r.Execute("","","iris-agentic-dev","","","","","","","","","",10)
While r.Next() { Write r.Get("UTCTimeStamp")," ",r.Get("ClientIPAddress")," ",r.Get("EventData"),! }
```

```text
2026-08-27 08:33:46.221 192.168.215.1 tool=iris_execute mode=mcp ua=iris-agentic-dev/1.2.7 (mcp; build-agent-7; claude-code/2.1.0)
```

One naming trap: the CSP session appears as `CSPSessionID` in the `%SYS.Audit` table but as
`SessionID` in the List query's result set. `Get("CSPSessionID")` returns empty from the query.

If you skip the `Create`, nothing breaks: no records are written, no tool call fails, and the
tool prints the exact command above as the remediation. Note that `$SYSTEM.Security.Audit`
returns 0 both when the event is missing and when it is disabled, and IRIS supplies no error
text for either — so the tool's own message is the only diagnostic.

## 5. Limit agents on an environment

The marker is caller-asserted. Anyone can send any `User-Agent`, so treat it as auditing evidence
and as a basis for default-deny rules, not as a boundary against a hostile caller.

What actually holds:

- **Distinct credentials and roles per environment.** Give the agent an IRIS user whose roles
  permit what it should do on that instance and nothing more. This is also what makes the native
  audit trail attributable, since native records identify the user and not the client.
- **Gateway or reverse-proxy rules keyed on the marker**, for default-deny of agent traffic to an
  environment. Real for accidents and process violations; not for an attacker.
- **The non-configurable code-edit refusal.** Editing class or routine code through arbitrary
  execution is refused outright, which forces code changes down the document-and-compile path —
  which is precisely what makes a `RoutineChange` trail meaningful.

What does not hold: this tool's own write and destructive gates. They run in the agent's process
against the agent's local config, so `curl`, Postman, another MCP server, or a rebuilt binary
ignores them entirely. Their value is per-tool granularity that IRIS resources cannot express,
not environment enforcement.

## 6. Turning it back off

```objectscript
Set p("Enabled")=0
Do ##class(Security.Events).Modify("%System","%System","RoutineChange",.p)
Do ##class(Security.Events).Delete("iris-agentic-dev","Tool","ToolCall")
```

Records already written remain — audit history is immutable, and deleting the event definition
does not remove them.
