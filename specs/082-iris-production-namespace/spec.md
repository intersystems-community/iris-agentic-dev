# 082 — iris_production namespace parameter and description fix

## Problem

`iris_production` accepts an optional `namespace` call-time parameter at the code
level (line 6210 of `tools/mod.rs`) but the tool **description never mentions it**
and the input schema (`AnyParams` — open `{"type":"object"}`) provides no
`namespace` property. Agents therefore never pass it, so every `iris_production`
call defaults to the connection namespace — which is correct in the common case
but unrecoverable when the agent needs to target a different namespace (e.g. a
fleet with separate interop and dev namespaces).

Reported in #103 (v1.0.0): Gabriel's agent reported NO_PRODUCTION against USER
even though the production ran in IRISAPP. That specific `unwrap_or("USER")`
regression was fixed in a later commit (`fn default_namespace()` removal, #96
general fix) but the undocumented schema gap remains — agents see no hint that
`namespace` is valid.

The reporter noted this is a specific case of #96 (general namespace default);
\#96 was closed as fixed by removing serde defaults. Issue #103 stays open because
the tool description and schema still omit `namespace`.

## Root cause

`iris_production` uses `Parameters<AnyParams>` — a fully open JSON object schema —
rather than a typed struct. This keeps the handler flexible but means:

1. The `inputSchema` surfaced to clients has no `namespace` property.
2. The tool description makes no mention of `namespace`.
3. Agents querying the schema to understand valid parameters cannot discover it.

## Fix

**Three changes, all in `crates/iris-agentic-dev-core/src/tools/mod.rs`:**

### 1. Add `namespace` to the tool description

Append to the existing description string:

```text
`namespace` (optional): IRIS namespace for production operations. Defaults to
the connection namespace. Use when the interop production lives in a different
namespace than the default connection.
```

### 2. Add `namespace` to the inputSchema

Define a typed `IrisProductionParams` struct (or extend the existing
`interop::ProductionNameParams` pattern) that declares `namespace:
Option<String>` and generates the right JSON Schema. Wire it alongside the
existing `AnyParams` approach — the simplest path is to keep `AnyParams` for
parsing but add an explicit `schemars::JsonSchema`-derived schema block, or
switch the parameter type entirely.

The preferred approach: replace `Parameters<AnyParams>` with a typed struct that
covers all current ad-hoc fields (`action`, `production_name`, `full`,
`namespace`, `server`, `autostart`), matching the pattern used by `iris_execute`
and `iris_compile`. This makes the schema machine-readable and prevents future
parameter drift.

### 3. Verify namespace fallback is correct for all actions

Confirm that `resolve_namespace(ns_param, conn_ns)` is applied to every action
branch — currently all eight branches (status, start, stop, update, check,
recover, get_autostart, set_autostart) call it correctly. No code change needed,
just verified by the unit test layer.

## Design decisions (non-interactive)

**Q: Should `namespace` default to the connection namespace or USER?**
A: Connection namespace, always. `resolve_namespace(None, conn_ns)` returns
`conn_ns` when no explicit namespace is passed — this is correct and already
implemented. The fix is making it discoverable, not changing the logic.

**Q: Typed struct or keep AnyParams?**
A: Typed struct preferred. `iris_production` has a stable parameter surface
(8 actions with 4 distinct parameter combinations). A typed struct gives
schema-level documentation at zero runtime cost and catches typos at the
serialization boundary. `AnyParams` was a pragmatic choice during initial
development; this spec retires it for this tool.

**Q: What about `iris_production_item`, `iris_production_diff`, `iris_lookup_table`?**
A: Out of scope. They already document `namespace` in their descriptions and/or
have typed params. Address independently if gaps exist.

## Out of scope

- Changing the namespace resolution semantics (the fallback to connection
  namespace is correct).
- `iris_interop_query` namespace — it already documents `namespace` in its
  description.
- #101 (general namespace PR) — that PR addresses the serde default removal
  broadly; this spec is narrowly scoped to the `iris_production` schema gap.

## Test plan

### Layer 1 — Unit tests (no IRIS)

Add to `crates/iris-agentic-dev-core/tests/unit/` (new file
`test_iris_production_params.rs` or inline in existing
`test_workspace_config.rs`):

1. **Schema documents `namespace`**: call `IrisTools::new(None)` and inspect
   the `inputSchema` of the `iris_production` tool from `tool_router.list_all()`.
   Assert it contains a `namespace` property.
2. **Tool description mentions namespace**: same tool, assert
   `description.contains("namespace")`.

### Layer 2 — Binary invocation (no IRIS)

Add to `test_mcp_binary_config.rs`:

1. **T-082-01**: `tools/list` response for `iris_production` includes `namespace`
   in `inputSchema.properties`. Spawn binary, send `tools/list`, find the
   `iris_production` entry, assert `inputSchema.properties.namespace` exists.

### Layer 3 — Live IRIS integration (`#[ignore]`)

Add to `crates/iris-agentic-dev-core/tests/integration/test_e2e.rs` or a new
`test_production_namespace.rs`:

1. **T-082-02**: `iris_production(action=status)` with no `namespace` param
   returns a result scoped to the connection namespace (not USER) — assert
   `response.namespace == iris.namespace`. Requires live `iris-dev-iris`.
2. **T-082-03**: `iris_production(action=status, namespace=USER)` with an explicit
   different namespace returns a result scoped to USER — verifies the explicit
   override path. Requires live `iris-dev-iris`.

## Files to change

| File                                                                      | Change                                                               |
| ------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `crates/iris-agentic-dev-core/src/tools/mod.rs`                           | Add `IrisProductionParams` typed struct; wire it; update description |
| `crates/iris-agentic-dev-core/tests/unit/test_iris_production_params.rs`  | New — Layer 1 tests                                                  |
| `crates/iris-agentic-dev-bin/tests/integration/test_mcp_binary_config.rs` | Add T-082-01                                                         |
| `crates/iris-agentic-dev-core/tests/integration/test_e2e.rs`              | Add T-082-02, T-082-03                                               |

## Success criteria

1. `tools/list` response for `iris_production` includes `namespace` as a
   documented optional property in `inputSchema`.
2. The tool description explicitly mentions `namespace` and its semantics.
3. All three test layers pass (unit schema check, binary invocation, live IRIS).
4. No behavior change for callers that omit `namespace` — they still get the
   connection namespace.
5. `cargo clippy -- -D warnings` and `cargo fmt -- --check` both clean.
