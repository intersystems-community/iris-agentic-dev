---
name: ensemble-production
description: Manage and observe IRIS Interoperability productions — lifecycle, logs, queues, and message tracing. Covers pyprod Python API, ObjectScript Ens.Director, and MCP tool flow.
trigger: When asked about a production status, to start/stop/restart a production, investigate message failures, check queue backlogs, or define a production declaratively in Python
---

## Context Detection

**If you have `iris_production` MCP tool available** → use the MCP Process Flow below.

**If writing Python code** (`.py` file, or `iris_execute` tool with embedded Python) → use the Python / pyprod API section.

**If writing ObjectScript code** (no MCP tools) → use the ObjectScript API section.

---

## Python / pyprod API

### Execution context — CRITICAL

There are **two distinct execution contexts** for pyprod. Using the wrong one will fail:

| What you're doing | Context | How to run |
|---|---|---|
| `director` lifecycle calls | **Embedded Python** — `import iris` must be in scope | Use `iris_execute` tool, or a `.py` file run inside `iris session` |
| Declarative `Production` class definition | **External CLI** — `import iris` must NOT be in scope | `intersystems_pyprod /path/to/script.py` from a terminal |

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

## CRITICAL: use `update_production()`, not stop+start

```python
# CORRECT — hot-apply config changes, zero downtime, no message loss
director.update_production()

# WRONG — drops in-flight messages, causes unnecessary downtime
director.stop_production()
director.start_production("MyApp.Production")
```

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

| state value | meaning | production_name |
|---|---|---|
| `"1"` | running | set to production class name |
| `"2"` | stopped | `None` |
| `"3"` | suspended | set to production class name |
| `"4"` | troubled | set to production class name |

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

---

## ObjectScript API (no MCP tools)

When writing ObjectScript code to manage productions directly:

### Start / stop / verify

```objectscript
// Start a production
Set sc = ##class(Ens.Director).StartProduction("MyApp.Productions.Main")
If $$$ISERR(sc) { Quit sc }

// Verify it started — GetProductionState returns $$$EnsProductionRunning etc.
Set state = ##class(Ens.Director).GetProductionState(.sc)
If state '= $$$EnsProductionRunning {
    Quit $$$ERROR($$$GeneralError, "Production did not reach running state")
}

// Graceful stop (timeout seconds, force=0)
Set sc = ##class(Ens.Director).StopProduction(30, 0)

// Hot-apply config changes — NO DOWNTIME, preferred over restart
Set sc = ##class(Ens.Director).UpdateProduction()
```

### Check running state

```objectscript
// Current production name
Set prodName = ##class(Ens.Director).GetActiveProductionName()

// State constants: $$$EnsProductionRunning, $$$EnsProductionStopped,
//                 $$$EnsProductionTroubled, $$$EnsProductionSuspended
Set state = ##class(Ens.Director).GetProductionState(.sc)
```

### Key rules for ObjectScript

- **Use `##class(Ens.Director)`** — NOT `$$Start^Ens.Director`, NOT `%Start()` on the class
- **Use `$$$ISERR(sc)`** to check %Status returns — NOT `If sc = 0` or `If sc`
- **Use `UpdateProduction()`** for config changes, NOT stop+start (avoids message loss)
- **Never use force=1** in StopProduction unless graceful timeout has elapsed
- **`EnableConfigItem` not `StartConfigItem`** — `StartConfigItem` / `StopConfigItem` do not exist; use `EnableConfigItem(name, 1/0, 1)`

---

## MCP Tool Process Flow

### Investigating a production problem

1. **Check status first** — call `interop_production_status` with `full_status=true`
   to see which components are running, faulted, or disabled.

2. **Check queues** — call `interop_queues` if you suspect backlog or blocked messages.
   High queue depth on a specific component indicates a bottleneck or fault in that component.

3. **Search messages** — call `interop_message_search` to find specific messages by body
   content, session ID, sender, or time range. This is the fastest way to trace a failed
   transaction end-to-end.

4. **Check logs** — call `interop_logs` filtered to the component and time window of interest.
   Look for `ERROR` or `WARNING` severity entries.

### Making a configuration change

1. Call `interop_production_needs_update` — if it returns `false`, no action needed.
2. If update needed, call `interop_production_update` (hot-apply, no downtime).
3. Confirm with `interop_production_status` that all components are still running.

### Restarting a production

Only restart if status shows the production is stopped or stuck:

```
# Graceful stop (waits for in-flight messages)
interop_production_stop(timeout=30, force=false)

# Start with the production class name
interop_production_start(production="MyApp.Productions.Main", namespace="MYNS")

# Confirm
interop_production_status(full_status=true)
```

### Recovering a faulted production

If the production is in an error state (stuck, partially started), call `interop_production_recover`.
This performs the equivalent of the Management Portal "Recover" button.

## Tool Reference

| Tool | When to use |
|------|------------|
| `interop_production_status` | Always first — baseline state before any action |
| `interop_production_start` | Start a stopped production |
| `interop_production_stop` | Graceful or forced stop |
| `interop_production_update` | Hot-apply config changes (no restart needed) |
| `interop_production_needs_update` | Check before deciding whether to update |
| `interop_production_recover` | Un-stick a faulted/partially-started production |
| `interop_logs` | Component-level log entries (filter by component + severity) |
| `interop_queues` | Queue depth per component — spot bottlenecks |
| `interop_message_search` | Trace specific messages by content, session, or time |

## Safety Rules

- **Never force-stop** (`force=true`) unless graceful stop has timed out. Force-stop drops
  in-flight messages.
- **Always check `interop_production_needs_update` before `interop_production_update`** — calling
  update when not needed is a no-op, but it's good hygiene to confirm first.
- **Namespace matters** — every tool accepts a `namespace` parameter. Default is `USER`.
  Productions in `HSCUSTOM` or application-specific namespaces require the correct namespace.
- **Do not restart to fix a config change** — use `update` instead. Restart loses in-flight messages.

## Output Format

When reporting production state:
> **Production**: `MyApp.Productions.Main` — RUNNING
> **Components**: 12 running, 0 faulted, 2 disabled
> **Queue depth**: BusinessProcess.OrderHandler: 0, BusinessOperation.SendHL7: 3

When tracing a message failure:
> **Session** `12345`: Failed at `BusinessOperation.SendHL7` — ERROR: Connection refused
> **Fix**: Check the outbound adapter host/port configuration for SendHL7
