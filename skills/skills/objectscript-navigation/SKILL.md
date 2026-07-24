---
name: objectscript-navigation
description: Deep codebase discovery using MCP and text tools. Use this to bridge the gap between raw MCP tool data and high-level architectural understanding.
---

# ObjectScript Semantic Navigation Skill

This skill provides strategies for mapping out ObjectScript codebases using the available MCP tools.

## Toolset Awareness

Some tools listed below are only available in the merged toolset. If a tool is absent
(e.g. `iris_doc_search`, `iris_execute_method`), switch to the merged toolset before
proceeding — these tools are not available in the standard toolset.

## Discovery Strategy

### 1. Symbol Anchoring

Before editing, always locate the **Symbol Anchor** (the definition of the class or method you intend to change).

- **Tool**: `iris_symbols(query)` — searches the live IRIS namespace by name pattern (supports `*` wildcards)
- **Tool**: `iris_symbols_local(workspace_path)` — parses `.cls` files on disk via tree-sitter when IRIS is unavailable
- **Tool**: `iris_search` — full-text regex search across all IRIS documents in a namespace; use when symbol lookup is too narrow and you need to find arbitrary patterns in source
- **Strategy**: Don't just search for the name; check the `containerName` to confirm you are in the correct package.

### 2. Dependency Tracing

When changing a signature, use **Upstream Tracing** to find every caller.

- **Tool**: `docs_introspect(class_name)` — fetches the full class definition from `%Dictionary`, including all methods, parameters, return types, and inheritance chain
- **Tool**: `iris_symbols(query="ClassName.*")` — list all members of a class
- **Tool**: `iris_execute_method` — invoke a ClassMethod directly without ObjectScript boilerplate; useful for spot-checking return values during tracing (merged toolset only)
- **Tool**: `iris_doc_search` — searches docs.intersystems.com via Algolia when you need reference material on a system class or API (merged toolset only)
- **Strategy**: Read each caller's containing file with the `Read` tool before modifying. If a method is called from multiple packages, plan all edits before touching any file.

### 3. Hierarchical Exploration

ObjectScript heavily uses inheritance (`Extends`).

- **Strategy**: If a method is missing in the current file, check the `Extends` list.
- **Tool**: `docs_introspect(class_name)` — reveals the full inherited interface including superclass chain.
- **Tool**: `find_subclass_implementations` — find all subclass overrides of a method in the full inheritance hierarchy; essential when changing a base class method that subclasses may override differently
- **Strategy**: Run `iris_symbols("SuperClass.*")` to list members inherited from a superclass.

### 4. Dynamic Dispatch Resolution

When code uses `$classmethod(var, method)` or `$method(obj, method)` patterns, the target class is not known statically.

- **Tool**: `resolve_dynamic_dispatch` — find all classes that implement a given method; use this to enumerate every possible dispatch target before concluding what a call site does.

### 5. Interoperability Routing

For Ensemble/Interoperability codebases, message routing is defined in MessageMap tables, not plain method calls.

- **Tool**: `extract_message_map_routing` — extract Ensemble MessageMap routing tables from a production or business process; use this before tracing message flow through an interop codebase.

## Bridging the Gap

When the model receives a list of symbols from `iris_symbols`, it should not just "see" a list. It should follow this reasoning:

1. "I see `Package.Class:Method` at line 10."
2. "I will now call `docs_introspect('Package.Class')` to confirm the method signature and see what it overrides."
3. "I will search for `Package.Class` with `iris_symbols` and `Grep` to find classes that `Extend` it."

## Guidelines

- Never assume a symbol is unique until verified by `iris_symbols`.
- Always call `docs_introspect` on any class you did not write before editing it — the live definition may differ from what you can infer from filenames alone.
- Use `Read` to open `.cls` files on disk when you need line-level context around a symbol.
- Use `Grep` (regex across the workspace) when `iris_symbols` results are too broad.
- Use `iris_search` when you need to find a pattern across all documents in the namespace (not just classes — includes routines, includes, and CSP files).

## Related skills

- **iris-agentic-dev** — connection setup and tool reference
- **objectscript-debugging** — once you've found the error location, use this to correlate .INT offsets and logs
- **objectscript-repair** — when navigation reveals a multi-class failure that needs coordinated fixes
- **ensemble-production** — if the codebase you're navigating is an Interoperability production
