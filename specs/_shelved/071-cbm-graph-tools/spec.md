# Feature Specification: CBM Graph Navigation Tools

**Spec Number**: 071
**Feature Branch**: `071-cbm-graph-tools`
**Created**: 2026-07-27
**Status**: Draft
**Depends on**: spec 070 (iris_symbols_local upgrade, for CBM-aligned name format)

## Problem Statement

`iris_symbols_local` answers "what symbols are in this file?" but cannot answer "what calls
this method?" or "what does this class depend on?" Those questions require a graph with
CALLS/ROUTES_TO/DEFINES edges across the full codebase.

`codebase-memory-mcp` (CBM) already builds that graph. ObjectScript/IRIS support landed in
PR #467 + #1060. CBM ships a CLI subcommand interface (`codebase-memory-mcp cli <tool>
<json>`) that exposes its graph query, trace, architecture, and snippet tools without
requiring an MCP session.

Wrapping that CLI in five new MCP tools gives Claude Code — and any other AI assistant
using iris-agentic-dev — the ability to navigate the ObjectScript call graph, trace data
flow, inspect architecture, and fetch exact code snippets, all without leaving the IRIS
development session.

---

## User Stories

### US1 — Graph search

An agent needs to find all IRIS classes that define a `ProcessInput` method. Rather than
scanning every `.cls` file, it calls `iris_graph_search` with a name pattern and gets back
qualified names, kinds, and file locations from the CBM graph index.

**Acceptance Scenarios**:

1. **Given** a CBM-indexed project, **When** `iris_graph_search(name_pattern="ProcessInput*")`
   is called, **Then** the response lists every node whose name matches the pattern, with
   `qualified_name`, `kind`, `file`, and `line`.
2. **Given** `label="ObjMethod"` filter, **When** called, **Then** only method nodes are
   returned.
3. **Given** no CBM binary on `PATH` and `CODEBASE_MEMORY_PATH` unset, **When** called,
   **Then** the error code is `CBM_NOT_FOUND` with a hint to install.
4. **Given** no indexed project for the current workspace, **When** called, **Then** the
   error code is `CBM_NOT_INDEXED` with a hint to run `iris_graph_index`.

---

### US2 — Call-path tracing

An agent investigating a bug needs to know whether `MyApp.Util:ParseDate` is reachable
from `MyApp.HL7.Router:OnMessage`. It calls `iris_graph_trace` and gets back the path
(or an empty result if no path exists).

**Acceptance Scenarios**:

1. **Given** a path exists between two qualified names, **When** `iris_graph_trace` is
   called with `from` and `to`, **Then** the response lists the ordered sequence of nodes
   and edges.
2. **Given** `mode="data_flow"`, **When** called, **Then** CBM traces data-flow edges
   instead of call edges.
3. **Given** no path exists, **When** called, **Then** the response is an empty path with
   `found: false`.

---

### US3 — Architecture summary

An agent onboarding a developer to a new production wants to explain how packages relate.
It calls `iris_graph_architecture` and gets back a summary of packages, entry points, and
dependency clusters.

**Acceptance Scenarios**:

1. **Given** an indexed project, **When** `iris_graph_architecture` is called, **Then** the
   response contains at least `packages` and `entry_points` fields.
2. **Given** `aspects=["dependencies"]` filter, **When** called, **Then** only dependency
   information is returned.

---

### US4 — Code snippet retrieval

An agent wants to read a method without knowing its file path or line number. It calls
`iris_graph_snippet` with the qualified name and gets back the source text, file, and line
range.

**Acceptance Scenarios**:

1. **Given** `qualified_name="MyApp.Foo.DoSomething"`, **When** `iris_graph_snippet` is
   called, **Then** the response includes `source`, `file`, `start_line`, `end_line`.
2. **Given** a qualified name that does not exist in the graph, **When** called, **Then**
   the error code is `CBM_SYMBOL_NOT_FOUND`.

---

### US5 — Index repository

An agent setting up a new workspace needs to build the CBM graph before graph queries will
work. It calls `iris_graph_index` and CBM indexes the current workspace.

This tool writes to the graph database, so it is gated behind `CBM_ALLOW_INDEX=1`.

**Acceptance Scenarios**:

1. **Given** `CBM_ALLOW_INDEX=1` is set, **When** `iris_graph_index` is called with `path`,
   **Then** CBM indexes the repository and returns a summary with file count and node count.
2. **Given** `CBM_ALLOW_INDEX` is absent or `0`, **When** called, **Then** the error code
   is `CBM_INDEX_GATED` with a message explaining how to enable it.
3. **Given** the `incremental=true` option, **When** called, **Then** CBM performs an
   incremental re-index (only changed files).

---

### US6 — Binary discovery

All five tools must locate the CBM binary without requiring manual configuration. The
discovery order is:

1. `CODEBASE_MEMORY_PATH` env var (explicit override)
2. `which codebase-memory-mcp` (on `PATH`)
3. Common install paths: `~/.local/bin/codebase-memory-mcp`,
   `/usr/local/bin/codebase-memory-mcp`

If none resolve, return `CBM_NOT_FOUND`.

**Acceptance Scenarios**:

1. `CODEBASE_MEMORY_PATH` set → that path used, even if binary not on `PATH`
2. `CODEBASE_MEMORY_PATH` unset, binary on `PATH` → `which` result used
3. Neither → `~/.local/bin` checked next
4. All fail → `CBM_NOT_FOUND`

---

## Tool Schemas

### `iris_graph_search`

```json
{
  "name_pattern": "ProcessInput*",
  "label": "ObjMethod",
  "project": "Users-tdyar-ws-myproject"
}
```

All fields optional. `name_pattern` supports glob (`*`, `?`). `project` defaults to the
CBM project name derived from the current working directory.

**Response**:

```json
{
  "nodes": [
    {
      "qualified_name": "MyApp.Router.ProcessInput",
      "kind": "ObjMethod",
      "file": "MyApp/Router.cls",
      "line": 42
    }
  ],
  "count": 1
}
```

---

### `iris_graph_trace`

```json
{
  "from": "MyApp.HL7.Router.OnMessage",
  "to": "MyApp.Util.ParseDate",
  "mode": "calls",
  "project": "Users-tdyar-ws-myproject"
}
```

`mode`: `"calls"` (default) | `"data_flow"` | `"cross_service"`

**Response**:

```json
{
  "found": true,
  "path": [
    { "qualified_name": "MyApp.HL7.Router.OnMessage", "kind": "ObjMethod" },
    { "qualified_name": "MyApp.HL7.Parser.Parse", "kind": "ObjMethod" },
    { "qualified_name": "MyApp.Util.ParseDate", "kind": "ObjMethod" }
  ]
}
```

---

### `iris_graph_architecture`

```json
{
  "aspects": ["packages", "entry_points", "dependencies"],
  "project": "Users-tdyar-ws-myproject"
}
```

`aspects` is optional; omit to get all.

**Response**: CBM `get_architecture` output, passed through.

---

### `iris_graph_snippet`

```json
{
  "qualified_name": "MyApp.Foo.DoSomething",
  "project": "Users-tdyar-ws-myproject"
}
```

**Response**:

```json
{
  "qualified_name": "MyApp.Foo.DoSomething",
  "source": "ClassMethod DoSomething(...)\n{\n    ...\n}",
  "file": "MyApp/Foo.cls",
  "start_line": 42,
  "end_line": 55
}
```

---

### `iris_graph_index`

```json
{
  "path": "/Users/tdyar/ws/myproject",
  "incremental": false,
  "project": "Users-tdyar-ws-myproject"
}
```

`path` defaults to current working directory. `incremental` defaults to `false`.
Gated: `CBM_ALLOW_INDEX=1` required.

**Response**:

```json
{
  "project": "Users-tdyar-ws-myproject",
  "files_indexed": 142,
  "nodes_created": 1847,
  "edges_created": 3201,
  "duration_ms": 4200
}
```

---

## Implementation Notes

### Module layout

New file: `crates/iris-agentic-dev-core/src/tools/graph.rs`

Register in `mod.rs` (near line 8100):

```rust
dispatch!("iris_graph_search", graph::GraphSearchParams, iris_graph_search);
dispatch!("iris_graph_trace", graph::GraphTraceParams, iris_graph_trace);
dispatch!("iris_graph_architecture", graph::GraphArchitectureParams, iris_graph_architecture);
dispatch!("iris_graph_snippet", graph::GraphSnippetParams, iris_graph_snippet);
dispatch!("iris_graph_index", graph::GraphIndexParams, iris_graph_index);
```

Add `pub mod graph;` to the `tools` module declaration.

Add to tool list (near line 2058):

```rust
"iris_graph_search",
"iris_graph_trace",
"iris_graph_architecture",
"iris_graph_snippet",
"iris_graph_index",
```

### CBM invocation

All tools call `codebase-memory-mcp cli <verb> <json>` via `std::process::Command`. Capture
stdout/stderr. On non-zero exit, surface stderr as the error message.

```rust
fn run_cbm(verb: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    let bin = find_cbm_binary()?;
    let output = Command::new(bin)
        .args(["cli", verb, &args.to_string()])
        .output()
        .map_err(|e| format!("CBM_NOT_FOUND: {e}"))?;
    if output.status.success() {
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}
```

### Project name derivation

When `project` is not supplied, derive from `std::env::current_dir()`:

```rust
fn default_project_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.replace('/', "-").trim_start_matches('-').to_string()))
        .unwrap_or_default()
}
```

### `iris_graph_index` gate

```rust
fn check_cbm_index_gate() -> Result<(), McpError> {
    if std::env::var("CBM_ALLOW_INDEX").as_deref() != Ok("1") {
        return Err(McpError::new(
            "CBM_INDEX_GATED",
            "iris_graph_index requires CBM_ALLOW_INDEX=1",
        ));
    }
    Ok(())
}
```

---

## Error Codes

| Code                   | Meaning                                                                        |
| ---------------------- | ------------------------------------------------------------------------------ |
| `CBM_NOT_FOUND`        | Binary not found — install `codebase-memory-mcp` or set `CODEBASE_MEMORY_PATH` |
| `CBM_NOT_INDEXED`      | Project not indexed — run `iris_graph_index` first                             |
| `CBM_SYMBOL_NOT_FOUND` | Qualified name not in graph                                                    |
| `CBM_INDEX_GATED`      | `iris_graph_index` called without `CBM_ALLOW_INDEX=1`                          |
| `CBM_ERROR`            | CBM returned a non-zero exit or unparseable response                           |

---

## Test Strategy

Tests are written before implementation (test-first).

### Unit tests (no CBM required)

In `graph.rs` `#[cfg(test)]`:

- `find_cbm_binary` respects `CODEBASE_MEMORY_PATH` override
- `find_cbm_binary` falls through discovery order correctly
- `default_project_name` derives project name from cwd
- `check_cbm_index_gate` returns `CBM_INDEX_GATED` when env var absent

### Integration tests (`#[ignore]` by default, require CBM)

In `tests/graph_tests.rs`:

- `iris_graph_search` returns nodes for `Users-tdyar-ws-iris-agentic-dev`
- `iris_graph_trace` returns a path (or `found: false` gracefully)
- `iris_graph_snippet` returns source for a known symbol
- `iris_graph_architecture` returns non-empty response
- `iris_graph_index` is blocked without `CBM_ALLOW_INDEX=1`

### MCP smoke test

After wiring, run the server and call each tool via `check_config` fixture to confirm tool
names appear in the tool list.

---

## Non-Goals

- No ObjectScript graph queries — CBM handles the graph; this spec only wraps the CLI
- No CBM schema changes — spec 071 does not modify CBM's graph model
- No streaming — all tools return a single JSON response
- No project management UI — list/delete projects are out of scope (use `cbm cli list_projects` directly)
- No live IRIS required — these tools operate on the disk-indexed graph
