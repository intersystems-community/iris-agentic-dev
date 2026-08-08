# iris-agentic-dev for AI Hub

This directory contains IRIS class exports that wire
[iris-agentic-dev](https://github.com/intersystems-community/iris-agentic-dev)
MCP tools into AI Hub agents as ToolSet and Skill classes.

## Prerequisites

- IRIS AI Hub build 162 or later
- `iris-agentic-dev` binary installed and on `PATH` (or at `/opt/homebrew/bin/iris-agentic-dev`
  on Apple Silicon, `/usr/local/bin/iris-agentic-dev` on Intel Mac)

Install the binary via Homebrew or download from the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases).

## Import

From a terminal or `%SYS` terminal session in the target namespace:

```objectscript
Do $system.OBJ.Load("/path/to/IAD.ToolSet.xml", "ck")
```

The `"ck"` flags compile and display errors. All 7 classes should compile without errors.

Verify:

```objectscript
Write $system.OBJ.IsCompiled("IAD.ToolSet.IrisAgenticDev")
```

Returns `1` if compiled successfully.

## Environment Variables

Set these before running an agent. The ToolSet passes them to the `iris-agentic-dev`
process via `<Env>` entries so the binary can connect to your IRIS instance.

| Variable         | Description                                  | Example     |
| ---------------- | -------------------------------------------- | ----------- |
| `IRIS_HOST`      | IRIS server hostname or IP                   | `localhost` |
| `IRIS_WEB_PORT`  | IRIS web server port (Atelier REST endpoint) | `52773`     |
| `IRIS_USERNAME`  | IRIS username                                | `_SYSTEM`   |
| `IRIS_PASSWORD`  | IRIS password                                | `SYS`       |
| `IRIS_NAMESPACE` | Default namespace for tool calls             | `USER`      |

## ToolSets

### IAD.ToolSet.IrisAgenticDev

Full tool set — exposes all iris-agentic-dev tools including compile, execute, source
control, and write operations.

Example agent using the full toolset:

```objectscript
Set agent = ##class(IAD.Agent.ObjectScriptDev).%New()
Set sc = agent.Run("List all classes in the USER namespace that extend Ens.Request")
```

### IAD.ToolSet.IrisAgenticDevReadOnly

Read-only variant that excludes `iris_compile`, `iris_execute`, `iris_source_control`,
`iris_production_item`, and `iris_credential_manage`. Use this for agents that should
inspect and navigate code but never modify IRIS state.

`IAD.ToolSet.IrisAgenticDevReadOnly` extends `IAD.ToolSet.IrisAgenticDev` — it
inherits all connection config and adds exclusion filters on top.

## Skills

### IAD.Skill.ObjectScriptRepair

Hard-gate checklist for the 10 most common ObjectScript mistakes: `Quit` inside
loops, missing `..MethodName()` syntax for intra-class calls, raw status checks instead
of `$$$ISERR`, throwing `%Status` instead of `%Exception` objects, and others. This
agent applies the checklist before presenting any generated code.

### IAD.Skill.ObjectScriptGuardrails

All-in-one 13-item safety checklist that catches mistakes the repair skill does not
cover: `%INLIST` in ObjectScript method code (SQL-only), hand-written `Storage Default`
blocks (IRIS auto-generates these and writing one causes ERROR #5559), `'=` inside SQL
string literals, and arithmetic precedence traps. Works without an MCP connection — the
instructions themselves are the gate. Pair with `ObjectScriptRepair` for full coverage.

### IAD.Skill.InteropDebugging

Guides agents through IRIS Interoperability production lifecycle and log investigation.
Covers production start/stop/update via `iris_production`, queue and message tracing via
`iris_interop_query`, message body inspection via `iris_message_body`, and log
investigation via `iris_get_log` and `journal_search`. Includes safety rules (hot-apply
vs. restart, force-stop constraints, namespace awareness).

### IAD.Skill.IrisNavigation

Codebase discovery and namespace exploration using the read-only toolset. Covers symbol
anchoring (`iris_symbols`, `iris_symbols_local`), dependency tracing (`docs_introspect`,
`find_subclass_implementations`), dynamic dispatch resolution (`resolve_dynamic_dispatch`),
and Ensemble MessageMap routing extraction (`extract_message_map_routing`). Intended for
agents that need architectural understanding before making changes.

## Declarative Agent Example

`IAD.Agent.ObjectScriptDev` is a ready-to-run example that combines the full toolset
with the repair, guardrails, and navigation skills. Use it as a starting point:

```objectscript
// Run it directly
Set agent = ##class(IAD.Agent.ObjectScriptDev).%New()
Set sc = agent.Run("Inspect the class MyApp.Service and find all callers of its Process method")
```

To build your own agent, subclass `%AI.Agent` and set the `PROVIDER`, `APIKEY`,
`TOOLSETS`, and `SKILLS` parameters. Mix and match the IAD ToolSets and Skills as needed.

## HTTP Transport (Remote Connections)

By default, `iris-agentic-dev` runs as a stdio MCP server and connects to IRIS over
HTTP using the env vars above. For remote IRIS instances or non-default connection
options, see [docs/connecting.md](../../docs/connecting.md) — it covers the full
connection configuration including `--transport http` for HTTP server mode.

## Troubleshooting

### Zero tools found after import

Recompile `IAD.ToolSet.IrisAgenticDev` last — the read-only subclass must compile after
its parent:

```objectscript
Do $system.OBJ.Compile("IAD.ToolSet.IrisAgenticDev", "ck")
Do $system.OBJ.Compile("IAD.ToolSet.IrisAgenticDevReadOnly", "ck")
```

### `IRIS_UNREACHABLE` or connection refused

Check that the env vars are set correctly and that the IRIS web server is reachable from
the AI Hub host. Test with:

```bash
curl -u "$IRIS_USERNAME:$IRIS_PASSWORD" \
  "http://$IRIS_HOST:$IRIS_WEB_PORT/api/atelier/"
```

A `200` response with `{"status": {"errors": []}}` confirms the Atelier REST API is up.
