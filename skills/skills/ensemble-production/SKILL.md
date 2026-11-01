---
name: ensemble-production
tier: extra
description: Manage and observe IRIS Interoperability productions — lifecycle, logs, queues, and message tracing. Covers pyprod Python API, ObjectScript Ens.Director, and MCP tool flow.
trigger: When asked about a production status, to start/stop/restart a production, investigate message failures, check queue backlogs, or define a production declaratively in Python
---

## When to use productions — scenario mapping

| Scenario                                                | Pattern                                |
| ------------------------------------------------------- | -------------------------------------- |
| Receive files/messages, validate, route to destinations | Production                             |
| Transform between formats (HL7 ↔ JSON ↔ XML)            | Production + Business Process          |
| Call external API or write to DB as part of a workflow  | Production + Business Operation        |
| Simple scheduled script                                 | Task Scheduler (not productions)       |
| Real-time REST endpoint                                 | CSP/REST application (not productions) |

**Four components at a glance:**

- **Service** — data in; receives/polls external input and injects messages into the production
- **Process** — routing/transform; contains business logic, orchestrates multi-step workflows
- **Operation** — data out; sends messages to external systems (APIs, files, databases)
- **Message** — the carrier; extend `Ens.Request` / `Ens.Response` for typed payloads

---

## Write what the editors can open

Analysts open processes, transforms and rules in the Management Portal's BPL Editor, DTL Editor and Rule
Editor, and in the newer editors under `/ui/interop/`. Those editors open three kinds of class only. In an
ObjectScript project, write these by default:

| You need                  | Write                                                               | Opens in    |
| ------------------------- | ------------------------------------------------------------------- | ----------- |
| A business process        | a class extending `Ens.BusinessProcessBPL` with `XData BPL`         | BPL Editor  |
| A data transformation     | a class extending `Ens.DataTransformDTL` with `XData DTL`           | DTL Editor  |
| Decision or routing logic | a class extending `Ens.Rule.Definition` with `XData RuleDefinition` | Rule Editor |

Give a rule class its `RuleAssistClass` parameter: `Ens.Rule.GeneralBusinessRuleAssist` for a rule a BPL
`<rule>` calls, `EnsLib.MsgRouter.RuleAssist` for the rule a router names in `BusinessRuleName`. Without it
the rule still opens, as type "(unknown)".

A class extending `Ens.BusinessProcess` with `OnRequest`, or `Ens.DataTransform` with a `Transform()`
method, compiles and runs, but no editor opens it: the editor answers `'X' is not a BPL business process`.
Write one only when the developer asks for code, and tell them the editor will not open it.

### Moving code into BPL and rules

| In code                                          | In BPL and rules                                                                                                                                                               |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `SendRequestSync(target, req, .resp, 5)`         | `<call target="Agent" async="0" timeout="5">` with `<request>` and `<response>`. On a timeout the `<response>` block is skipped                                                |
| A call that may fail without failing the process | the `<call>` inside `<scope>` with `<faulthandlers><catchall>`. A failed `<call>` outside a scope ends the process                                                             |
| `If`/`ElseIf` choosing an outcome                | a general rule. The `<rule>` puts its return value in `resultLocation` and the rules that fired in `reasonLocation`, comma-separated, as `rule#1(duplicate):when#1`            |
| A list of reasons                                | each rule's `assign` adds to a context property: `Reasons_"duplicate;"`                                                                                                        |
| A lookup (member exists, duplicate)              | a `[ Final ]` class method in a subclass of `Ens.Rule.FunctionSet`, called in the condition by its bare name: `IsDup(Input)`. A qualified name does not compile in a condition |
| An UPDATE or any other write                     | a `<code>` step calling a class method, or a business operation. Never in a rule                                                                                               |

Keep rule functions read-only: the Rule Editor's Test button runs them for real.

A general rule's context is the process's generated context class, `<process class>.Context`, so compile the
BPL class before the rule. Inside the rule, context properties are named bare (`Input`, `Reasons`).

```xml
<!-- XData BPL of MyApp.IntakeBP -->
<process language='objectscript' request='Ens.StringRequest' response='Ens.StringResponse'>
<context>
<property name='Input' type='%String' instantiate='0'/>
<property name='Outcome' type='%String' instantiate='0'/>
<property name='Reasons' type='%String' instantiate='0'/>
<property name='Why' type='%String' instantiate='0'/>
</context>
<sequence>
<assign property='context.Input' value='request.StringValue' action='set'/>
<rule name='decide' rule='MyApp.IntakeRule' resultLocation='context.Outcome' reasonLocation='context.Why'/>
<scope>
<call name='agent' target='Agent' async='0' timeout='5'>
<request type='Ens.StringRequest'><assign property='callrequest.StringValue' value='context.Input' action='set'/></request>
<response type='Ens.StringResponse'/>
</call>
<faulthandlers><catchall/></faulthandlers>
</scope>
<assign property='response.StringValue' value='context.Outcome_"|"_context.Reasons' action='set'/>
</sequence>
</process>
```

```xml
<!-- XData RuleDefinition of MyApp.IntakeRule, RuleAssistClass = "Ens.Rule.GeneralBusinessRuleAssist" -->
<ruleDefinition alias="" context="MyApp.IntakeBP.Context" production="">
<ruleSet name="" effectiveBegin="" effectiveEnd="">
<rule name="duplicate">
<when condition="IsDup(Input)">
<assign property="Reasons" value="Reasons_&quot;duplicate;&quot;"></assign>
<return>&quot;invalid&quot;</return>
</when>
</rule>
<return>&quot;further_review&quot;</return>
</ruleSet>
</ruleDefinition>
```

```xml
<!-- XData DTL of MyApp.UpperDTL -->
<transform sourceClass='Ens.StringRequest' targetClass='Ens.StringResponse' create='new' language='objectscript'>
<assign value='$ZCONVERT(source.StringValue,"U")' property='target.StringValue' action='set'/>
</transform>
```

A routing rule (`EnsLib.MsgRouter.RuleAssist`, context `EnsLib.MsgRouter.RoutingEngine`) sends with
`<send transform="" target="OpA">` inside a `<when condition='StartsWith(Document.StringValue,"A")'>`, and
an `<otherwise>` for the rest.

---

## Context Detection

**If you have `iris_production` MCP tool available** → use the MCP Process Flow below.

**If writing Python code** (`.py` file, or `iris_execute` tool with embedded Python) → use the Python / pyprod API section.

**If writing ObjectScript code** (no MCP tools) → use the ObjectScript API section.

---

## Python / pyprod API

### Execution context — CRITICAL

There are **two distinct execution contexts** for pyprod. Using the wrong one will fail:

| What you're doing                         | Context                                               | How to run                                                         |
| ----------------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------ |
| `director` lifecycle calls                | **Embedded Python** — `import iris` must be in scope  | Use `iris_execute` tool, or a `.py` file run inside `iris session` |
| Declarative `Production` class definition | **External CLI** — `import iris` must NOT be in scope | `intersystems_pyprod /path/to/script.py` from a terminal           |

**Never pass a `director` script to `intersystems_pyprod` CLI** — it will fail because `iris` is not bound.
**Never call `iris_execute` with a declarative Production script** — it cannot load production XML from inside an embedded session.

### Install

```bash
pip install "intersystems-pyprod>=0.2.0"
```

---

### `director` module — lifecycle management (embedded Python only)

```python
from intersystems_pyprod import director

# --- Check status first (always) ---
# Returns 3-tuple: (status, production_name, state)
# status: IRIS %Status string — "1" means success
# production_name: str or None — None when state is 2 (stopped)
# state: "1"=running, "2"=stopped, "3"=suspended, "4"=troubled
status, prod_name, state = director.get_production_status()

if state == "2":  # stopped — state is a string, not int
    director.start_production("MyApp.Production")
elif state == "1":
    print(f"Running: {prod_name}")
```

## Config change: hot update first

```python
# Hot-apply config changes; the production keeps running
director.update_production()

# Heavier: stop and start the whole production. Nothing is lost, but every host goes down
director.stop_production()
director.start_production("MyApp.Production")
```

See "Choosing update, host restart or stop" below for when each one fits.

### Full director API

```python
# Lifecycle
director.start_production("MyApp.Production")       # start by class name
director.stop_production(timeout=10, force=False)   # graceful stop
director.restart_production()                       # stop + start
director.update_production()                        # hot-apply config — preferred

# Items — do_update=True hot-applies immediately (production can stay running)
director.enable_config_item("MyService", enable=True, do_update=True)

# Inspect all productions
status, names, details = director.list_all_productions()
# details[name] = {"status": ..., "last_start_time": ..., "last_stop_time": ...}

# Messages for a host (most recent first)
msgs = director.get_host_messages("MyService", max_results=100)

# Inject a message into a running production (adapterless service only)
status, svc = director.create_business_service("MyApp.MyAdapterlessService")
status, response = svc.process_input(my_message)
```

### State values — get_production_status()

| state value | meaning        | production_name              |
| ----------- | -------------- | ---------------------------- |
| `"1"`       | running        | set to production class name |
| `"2"`       | stopped        | `None`                       |
| `"3"`       | suspended      | set to production class name |
| `"4"`       | troubled       | set to production class name |
| `"5"`       | NetworkStopped | not checked                  |

State is always a **string** — compare with `== "1"`, not `== 1`.

---

### Declarative production definition (external CLI only)

```python
from intersystems_pyprod import Production, ServiceItem, ProcessItem, OperationItem

iris_package_name = "MyPkg"  # must be set before the Production subclass

class MyProduction(Production):
    description = "My integration production"
    services = [
        ServiceItem(
            "InboundFileService",
            "EnsLib.File.PassthroughService",
            host_settings={"TargetConfigNames": "FileRouter"},
            adapter_settings={"FilePath": "/data/in", "DeleteFromServer": 0},
        )
    ]
    processes = [
        ProcessItem("FileRouter", f"{iris_package_name}.FileRouterBP")
    ]
    operations = [
        OperationItem(
            "OutboundFileOp",
            "EnsLib.File.PassthroughOperation",
            adapter_settings={"FilePath": "/data/out"},
        )
    ]
```

Load with:

```bash
intersystems_pyprod /path/to/my_production.py
```

**Rules for declarative definitions:**

- `iris_package_name` must be a module-level variable defined **before** the `Production` subclass
- `host_settings` = business logic settings (`Target="Host"` in XML)
- `adapter_settings` = adapter settings (`Target="Adapter"` in XML)
- Unknown keys in `host_settings`/`adapter_settings` produce warnings at load time, not errors — check output carefully
- Do not call `director.*` inside a declarative script; it is run by the external CLI, not inside IRIS
- The Python file is the source of truth. pyprod generates an ordinary `Ens.Production` class, but a
  setting changed in the Portal's Production Configuration page is overwritten the next time the file
  loads (round-trip editing was closed as Won't Do). Change the file, and tell an analyst who asks for a
  Portal edit. A pyprod business process is a Python class, so the BPL Editor does not open it either

---

## ObjectScript API (no MCP tools)

When writing ObjectScript code to manage productions directly:

### Start / stop / verify

```objectscript
// Start a production
Set sc = ##class(Ens.Director).StartProduction("MyApp.Productions.Main")
If $$$ISERR(sc) { Quit sc }

// Verify it started: state 1 is running, 2 is stopped
Set sc = ##class(Ens.Director).GetProductionStatus(.prodName, .state)
If $$$ISERR(sc) { Quit sc }
If state '= 1 {
    Quit $$$ERROR($$$GeneralError, "Production did not reach running state")
}

// Graceful stop (timeout seconds, force=0)
Set sc = ##class(Ens.Director).StopProduction(30, 0)

// Hot-apply config changes — NO DOWNTIME, preferred over restart
Set sc = ##class(Ens.Director).UpdateProduction()
```

### Check running state

```objectscript
// Name and state of the production in this namespace. When none runs, state is 2 and name is ""
Set sc = ##class(Ens.Director).GetProductionStatus(.prodName, .state)

// Is this one running? Returns 1 or 0
Set running = ##class(Ens.Director).IsProductionRunning("MyApp.Productions.Main")
```

State values: 1 running, 2 stopped, 3 suspended, 4 troubled, 5 NetworkStopped. `EnsConstants.inc` names them `$$$eProductionStateRunning`, `$$$eProductionStateStopped` and so on. `Ens.Director` has no `GetProductionState` method; calling it fails with `<METHOD DOES NOT EXIST>`.

`GetActiveProductionName()` is not a running check. It returns the production that was started last, and it still returns that name after the production stops.

`StartProduction` on a production that already runs returns `ErrProductionAlreadyRunning`. To make a start idempotent, check `GetProductionStatus` first and start only when state is 2.

### Key rules for ObjectScript

- **Use `##class(Ens.Director)`** — NOT `$$Start^Ens.Director`, NOT `%Start()` on the class
- **Use `$$$ISERR(sc)`** to check %Status returns — NOT `If sc = 0` or `If sc`
- **Use `UpdateProduction()`** for config changes, NOT stop+start (avoids message loss)
- **Never use force=1** in StopProduction unless graceful timeout has elapsed
- **`EnableConfigItem` not `StartConfigItem`** — `StartConfigItem` / `StopConfigItem` do not exist; use `EnableConfigItem(name, 1/0, 1)`

---

## MCP Tool Process Flow

Two tools cover all of interop: `iris_production` for lifecycle (an `action` argument
selects the operation) and `iris_interop_query` for reading traffic (a `what` argument
selects logs, queues, or messages).

### Investigating a production problem

1. **Check status first** — `iris_production(action="status", full=true)` to see which
   components are running, faulted, or disabled.

2. **Check queues** — `iris_interop_query(what="queues")` if you suspect backlog or blocked
   messages. High queue depth on one component indicates a bottleneck or fault there.

3. **Search messages** — `iris_interop_query(what="messages", ...)` to trace a transaction
   by `source`, `target`, `message_class`, `session_id`, or body content (`body_class` +
   `body_where`, or `search_table`). Body content is PHI-gated: on a server with
   `dataPolicy = "allow"` add `acknowledgePhi=true` or the call returns `PHI_ACK_REQUIRED`.
   Fastest way to follow a failed transaction end-to-end.

4. **Check logs** — `iris_interop_query(what="logs", component="FileRouter",
log_type="error,warning")`.

### Making a configuration change

1. `iris_production(action="check")` — if no update is pending, no action needed.
2. If an update is pending, `iris_production(action="update")` (hot-apply, no downtime).
3. Confirm with `iris_production(action="status")` that all components are still running.

To enable, disable, or reconfigure a single item without touching the rest of the
production, use `iris_production_item(action="enable"|"disable"|"get_settings"|"set_settings")`.

### Choosing update, host restart or stop

Pick the smallest action that applies the change:

1. **Hot update** — settings changed: `iris_production(action="update")`. The production keeps
   running.
2. **One host** — one item is stuck or needs new code:
   `iris_production_item(action="disable")`, then `action="enable"`. The rest keep running.
3. **Controlled stop and start** — the production itself is stuck, or the change needs a full
   restart.

What a stop does to messages, observed on IRIS 2026.2 with six messages queued to a
one-at-a-time operation:

- **Graceful** (`force=false`): the operation finishes the message in hand and the rest stay
  queued. After the next start all six are delivered.
- **Force** (`force=true`): the interrupted message goes back on the queue (requeued) and runs
  again after the next start. All six ran once each, because the side effect came after the
  interruption point. Code whose side effect happens before a force stop would do it twice, so
  make it idempotent or use a graceful stop.

Neither loses a message.

```text
# Graceful stop (waits for the message in hand)
iris_production(action="stop", timeout=30, force=false)

# Start with the production class name
iris_production(action="start", production_name="MyApp.Productions.Main", namespace="MYNS")

# Confirm
iris_production(action="status", full=true)
```

### Recovering a faulted production

If the production is stuck or partially started, `iris_production(action="recover")` —
the equivalent of the Management Portal "Recover" button.

## Tool Reference

| Call                                          | When to use                                               |
| --------------------------------------------- | --------------------------------------------------------- |
| `iris_production(action="status", full=true)` | Always first — baseline state before any action           |
| `iris_production(action="start")`             | Start a stopped production (`production_name`)            |
| `iris_production(action="stop")`              | Graceful or forced stop (`timeout`, `force`)              |
| `iris_production(action="update")`            | Hot-apply config changes (no restart needed)              |
| `iris_production(action="check")`             | Check before deciding whether to update                   |
| `iris_production(action="recover")`           | Un-stick a faulted/partially-started production           |
| `iris_production(action="get_autostart")`     | Read the autostart production for the namespace           |
| `iris_production(action="set_autostart")`     | Set autostart (`production`, `enabled`)                   |
| `iris_production_item(action="enable")`       | Enable or disable one business host                       |
| `iris_production_item(action="set_settings")` | Change one item's settings without a full update          |
| `iris_interop_query(what="logs")`             | Component-level log entries (`component`, `log_type`)     |
| `iris_interop_query(what="queues")`           | Queue depth per component — spot bottlenecks              |
| `iris_interop_query(what="messages")`         | Trace messages by source, target, class, session, or body |

Message _status_ is not a filter on `iris_interop_query`. To find discarded or errored
messages, query the header table:
`iris_query(sql="SELECT TOP 50 ID, Status, TargetConfigName FROM Ens.MessageHeader WHERE Status = 5 ORDER BY ID DESC")`.

## Interop depth tools

> **Toolset note:** `iris_message_body`, `iris_business_rule_info`, and `iris_production_diff`
> are available in the **merged toolset only**. `iris_lookup_manage`, `iris_lookup_transfer`,
> `iris_credential_list`, and `iris_credential_manage` are available in all toolsets.

| Tool                      | What it does                                                              |
| ------------------------- | ------------------------------------------------------------------------- |
| `iris_message_body`       | Read a message body by ID                                                 |
| `iris_business_rule_info` | List or inspect business rules                                            |
| `iris_production_diff`    | Diff running production config against last source-controlled version     |
| `iris_lookup_manage`      | Read/write/delete/list Ensemble lookup table entries                      |
| `iris_lookup_transfer`    | Export/import a lookup table as XML                                       |
| `iris_credential_list`    | List all Ensemble credential IDs and usernames (passwords never returned) |
| `iris_credential_manage`  | Create/update/delete credentials                                          |

### `iris_message_body(message_id)`

Read a message body by ID. PHI-gated: pass `acknowledgePhi=true` if the body contains
protected health information. Handles both plain-text and stream-backed bodies.

### `iris_business_rule_info(action, rule_name?)`

- `action="list"` — list the rule classes (`Ens.Rule.Definition` subclasses) and any legacy
  `Ens.Rule.RuleSet` rows in the namespace, each tagged `kind: class` or `kind: legacy`
- `action="get"` with `rule_name` (the rule class name) — return the rule type, context, and each
  rule's conditions and actions

### `iris_production_diff`

Diff the running production config against the last source-controlled version. Returns
`changes: [{item_name, item_type, status}]` where `status` is `added`, `removed`, or
`modified`. Returns `NO_SCM` if no source control adapter is configured.

### `iris_lookup_manage(action, table, key?)`

Read, write, delete, or list Ensemble lookup table entries.

- `get` and `list` are always available
- `set` and `delete` are write-gated **and destructive-gated** (requires both
  `write_tools_enabled = true` and `destructive_tools_enabled = true`)

### `iris_lookup_transfer(action, table)`

Export or import a lookup table as XML.

- `export` is always available
- `import` is write-gated

### `iris_credential_list`

List all Ensemble credential IDs and usernames. Passwords are never returned.

### `iris_credential_manage(action, id)`

Create, update, or delete Ensemble credentials. Write-gated **and destructive-gated**
(requires both `write_tools_enabled = true` and `destructive_tools_enabled = true`).

---

## Safety Rules

- **Prefer a graceful stop** (`force=false`). A force stop requeues the interrupted message and
  runs it again after restart, which repeats any side effect it had already made.
- **Always run `iris_production(action="check")` before `iris_production(action="update")`** —
  calling update when not needed is a no-op, but it's good hygiene to confirm first.
- **Namespace matters** — every tool accepts a `namespace` parameter. Default is `USER`.
  A production in any namespace other than `USER` needs that namespace passed.
- **Do not restart to fix a config change** — use `update` instead. A restart takes every host
  down to change one setting.

## Code-based processes

When the developer asks for a code-based process (`Ens.BusinessProcess` with `OnRequest`), or you are
changing one that exists, these hold. The editors will not open it; say so.

### OnRequest signature — must use `%Library.Persistent`

IRIS validates method override signatures against the base class. The base
`OnRequest` in `Ens.BusinessProcess` declares the request as `%Library.Persistent`,
so your override must match exactly.

```objectscript
// Wrong — IRIS rejects the override at compile time
Method OnRequest(req As MyPkg.MyMsg, output response As %Library.Persistent) As %Status
{
    // ...
}

// Right — match the base signature, then coerce inside
Method OnRequest(req As %Library.Persistent, output response As %Library.Persistent) As %Status
{
    Set typedReq = req  ; IRIS coerces automatically — full typed access works
    // ...
}
```

### OnResponse required in BusinessProcess

Even if your Business Process only sends fire-and-forget requests, IRIS invokes
`OnResponse` whenever an async response arrives. Omitting the method causes a
`"Not implemented"` log entry for every response — fill it with a stub if you
don't need it.

```objectscript
// Wrong — omit OnResponse entirely → "Not implemented" noise in logs

// Right — stub it out if unused
Method OnResponse(
    request As %Library.Persistent,
    ByRef response As %Library.Persistent,
    callrequest As %Library.Persistent,
    callresponse As %Library.Persistent,
    completionkey As %String) As %Status
{
    Return $$$OK
}
```

## Common gotchas

### Ens.Request storage — leave it to the compiler

When subclassing `Ens.Request` or `Ens.Response`, do not write an `XData Storage` block. The
compiler generates one that puts the body in `^Ens.MessageBodyD`. A hand-written
`<DataLocation>` pointing anywhere else fails at compile time (IRIS 2026.2):

```text
ERROR #5477: Keyword signature error in MyPkg.MyMsg:Storage:Default,
keyword 'DataLocation' must be '^Ens.MessageBodyD'
```

Fix: delete the Storage block and compile again. See `objectscript-guardrails` on Storage.

Long class names are fine. IRIS hashes a long package or class name into a short global name:
`Test127.VeryLongPackageName.Data.PatientRecordHistory` compiled and saved on 2026.2, with its
data in a hashed global of 31 characters or fewer. No rename needed.

### iris_production start returning "Invalid Production"

Check the cause before anything else: the class must compile (`iris_doc(mode="put",
compile=true)` or `iris_compile`), and the `namespace` argument must be the one the production
lives in. If both are right and the tool still refuses, report it as an iris-agentic-dev issue
with the tool's output. Do not route around the refusal with `iris_execute`.

## Testing without the Management Portal

Use `iris_execute` to programmatically send a test message to a running production.
This replaces the Portal "Test" UI and works in automated scripts.

```objectscript
// 1. Instantiate your request message
Set req = ##class(MyPkg.MyRequest).%New()
Set req.SomeField = "test value"

// 2. Get a handle to the adapterless service (an item with PoolSize="0")
Set sc = ##class(Ens.Director).CreateBusinessService("MyPkg.MyService", .svc)
If $$$ISERR(sc) { Write $System.Status.GetErrorText(sc) Quit }

// 3. Send the message — response populated synchronously
Set sc = svc.ProcessInput(req, .resp)
If $$$ISERR(sc) { Write $System.Status.GetErrorText(sc) Quit }

Write "Response: ", resp.SomeResponseField
```

For services that use an adapter (file, HTTP, etc.) you cannot call `ProcessInput` directly. Add an
adapterless service item (`PoolSize="0"`) to the production for testing, and send from it to the process
by its config item name. `Ens.Director` has no `SendRequestSync`; the service object does:

```objectscript
Set sc = ##class(Ens.Director).CreateBusinessService("TestSvc", .svc)
If $$$ISERR(sc) { Write $System.Status.GetErrorText(sc) Quit }
Set req = ##class(MyPkg.MyRequest).%New()
Set req.SomeField = "test value"
Set sc = svc.SendRequestSync("MyProcessConfigName", req, .resp, 30)
If $$$ISERR(sc) { Write $System.Status.GetErrorText(sc) Quit }
Write "Done: ", resp.SomeResponseField
```

`StartProduction` and `StopProduction` move the current device of the job that calls them. In a script
that writes its results, save `$IO` before the call and `Use` it again after.

---

## Output Format

When reporting production state:

> **Production**: `MyApp.Productions.Main` — RUNNING
> **Components**: 12 running, 0 faulted, 2 disabled
> **Queue depth**: BusinessProcess.OrderHandler: 0, BusinessOperation.SendHL7: 3

When tracing a message failure:

> **Session** `12345`: Failed at `BusinessOperation.SendHL7` — ERROR: Connection refused
> **Fix**: Check the outbound adapter host/port configuration for SendHL7

---

## Related skills

- **iris-agentic-dev** — connection setup and toolset configuration
- **objectscript-navigation** — when you need to find and read the production's class implementations
- **objectscript-debugging** — when a business operation or process is failing with a runtime error
- **irispython-connector** — when the production uses Python business operations
