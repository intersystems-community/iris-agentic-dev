# Implementation Plan: iris_global Tool

**Branch**: `052-iris-global`
**Spec**: spec.md
**Depends on**: 051 (dispatch_gate, McpTemplate, DataPolicy, GlobalBlocklist — merged)

## Tech Stack

- Rust 1.92 (workspace edition 2021)
- `iris-agentic-dev-core` crate — tool handler lives here
- `serde_json` — param parsing, response building
- `reqwest` — HTTP calls to IRIS execute endpoint (same as iris_execute)
- ObjectScript execution via `IrisConnection::execute_via_generator()` (compile-and-query
  pattern, NOT `/api/atelier/v1/{ns}/action/execute` — see research.md)
- Policy gate: `dispatch_gate()` from `crates/iris-agentic-dev-core/src/policy/gate.rs`

## File Structure

```
crates/iris-agentic-dev-core/src/tools/global.rs     # NEW — handler
crates/iris-agentic-dev-core/src/tools/mod.rs         # MODIFY — register tool + category
crates/iris-agentic-dev-core/src/policy/env_gate.rs   # MODIFY — add Global category
tests/unit/test_iris_global_unit.rs                   # NEW — unit tests
tests/integration/test_iris_global_live.rs            # NEW — integration tests (#[ignore])
```

## Global Category in env_gate

The `ToolCategory` enum needs a `Global` variant (or reuse existing `Query` for reads and
`Execute` for writes). Simplest approach: classify `iris_global` as:

- `get` / `list` → `ToolCategory::Query` (permitted by all templates)
- `set` / `kill` → `ToolCategory::Execute` (blocked by live template)

This avoids a new enum variant — gate [1] already handles `Query` and `Execute` correctly.
`tool_to_category()` maps by tool name; for `iris_global` we need action-aware dispatch.

**Decision**: Pass `iris_global:get`, `iris_global:list` etc. as tool_name variants to
`tool_to_category()`. OR: check `action` param inside the gate before categorizing. Simplest
implementation: in `tool_to_category()`, return `Query` for `iris_global` by default, but
`dispatch_gate()` already has access to `params` — add a special-case in `env_gate.rs` that
upgrades `iris_global` to `Execute` category when `action` is `set` or `kill`.

**Chosen approach**: Add `iris_global` → `Query` in `tool_to_category()` in
`server_manager.rs`. In `check_env_gate()` in `env_gate.rs`, after `tool_to_category_pub()`
returns, add: if `tool_name == "iris_global"` and `params["action"]` is `"set"` or `"kill"`,
override `category` to `ToolCategory::Execute`. This requires passing `params` to
`check_env_gate()` (signature change: add `params: &serde_json::Value`). `dispatch_gate()`
already has `params` and calls `check_env_gate()` — forward it there. All existing
`check_env_gate` call sites must be updated to pass `&serde_json::Value::Null` or the real
params.

## ObjectScript Execution Strategy

All four operations use a single ObjectScript code block submitted to the execute endpoint:

```objectscript
// Built dynamically in Rust; namespace + global reference interpolated safely
Set sc = $$$OK
Try {
    // operation-specific code (get/set/kill/list)
    Write {json_result}, !
} Catch ex {
    Write "{""error"":""",ex.DisplayString(),"""}", !
}
```

Use `$Name()` for indirect references to avoid injection. Build global reference as
`^GlobalName(sub1,sub2)` where subscripts are quoted string literals in the generated code.

**Security note**: Subscript values MUST be sanitized before interpolation — escape any
`"` characters. Never interpolate `global_name` directly into `$Name()` — use `@gref`.

## Response Format

All actions return a JSON object:

```json
// get single node
{"success": true, "value": "node_value_or_null", "defined": true}

// get subtree
{"success": true, "nodes": [{"path": "^G(\"a\",\"b\")", "value": "v"}], "truncated": false}

// set
{"success": true}

// kill
{"success": true}

// list
{"success": true, "subscripts": ["a", "b", "c"], "truncated": false}
```

Error responses follow existing pattern:

```json
{ "success": false, "error_code": "IRIS_UNREACHABLE", "message": "..." }
```

## Test Strategy

### Unit tests (no IRIS needed)

- Param validation: missing required params → error
- Gate wiring: mock `dispatch_gate()` returns `Err(...)` → handler returns gate error
- Response building: test the JSON builders directly
- Global name normalization: `^MyApp` and `MyApp` → same internal form
- Gate parity (2026-10-10 amendment): every gate test runs each name in BOTH forms, bare and
  caret-prefixed, and the table of names that are not names (FR-016). A gate test that passes only
  bare names is incomplete — that gap is how the caret bypass shipped.
- Subscript escaping: `"quoted"` in subscript → proper escape in ObjectScript

### Integration tests (#[ignore])

- Round-trip: set → get → kill on `IrisDevTest.Global` namespace
- Subtree: write N nodes, `get` with `subtree:true`, verify count
- PHI gate: `action=get`, `global_name="PAPMI"`, no `acknowledgePhi` → `PHI_GATE_BLOCKED`
- System blocklist: `action=get`, `global_name="%SYS"` and `"^%SYS"` → `SYSTEM_BLOCKLIST`
- Gate parity on the live tool: `oddDEF` and `^oddDEF` (get), `PAPMIPatient` and `^PAPMIPatient`, and
  `global_preview` of both → refused. Read actions only, so a regression cannot write.
- Kill allowlist: works if configured in `.iris-agentic-dev.toml`

## Toolset Registration

`iris_global` uses `execute_via_generator` (HTTP-only, no Docker). It belongs in
**`Toolset::Merged`** per the constitution's Toolset Registration Rules. Both of the
following must be updated in sync:

1. `registered_tool_names()` — add `"iris_global"`
2. `with_registry_and_toolset()` router — add `iris_global` to the `Merged` toolset removal
   list (the list of tool names removed from Baseline stubs when Merged is active)

---

## Constitution Check

| Principle                      | Status  | Notes                                                                                                           |
| ------------------------------ | ------- | --------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary         | ✅ Pass | Uses `execute_via_generator` (HTTP); no new install step                                                        |
| II. ObjectScript Sanity        | ✅ Pass | `$Get`, `$Data`, `Kill`, `$Order`, `$Query`, `$ZH` all verified against IRIS 2026.2.0L — see research.md        |
| III. HTTP-First                | ✅ Pass | `execute_via_generator` is HTTP-only; no `IRIS_CONTAINER` required                                              |
| IV. Test-First, Fixture-Driven | ✅ Pass | Unit tests precede implementation in all phases; integration tests in `tests/integration/`                      |
| V. Output Shape Parity         | ✅ Pass | All four response shapes documented in data-model.md; `INVALID_SUBSCRIPT` registered                            |
| VI. Environment Guard          | ✅ Pass | `set`/`kill` classified as `Execute` (write-gated); `get`/`list` as `Query`; all pass through `dispatch_gate()` |
| VII. Dependency Minimalism     | ✅ Pass | No new crates; `serde_json`, `reqwest`, `uuid` already in workspace                                             |

---

## Phase Structure

1. **Setup**: New `tools/global.rs` skeleton + register in `mod.rs` + `tool_to_category` update
2. **Foundational**: Env gate classification for `iris_global` (action-aware)
3. **US1 (get)**: Unit tests → implementation
4. **US2 (set)**: Unit tests → implementation
5. **US3 (kill)**: Unit tests → implementation
6. **US4 (list)**: Unit tests → implementation
7. **Polish**: Integration tests, `check_config` inventory, AGENTS.md update

## Amendment 2026-10-10: one canonical name (FR-015 to FR-021)

One function, `canonical_global_name(raw) -> Result<String, Value>` in `tools/global.rs`, strips one
`^` and validates (FR-016). `dispatch_gate` calls it on `global_name`, `globalName` and `global`
before gates [3] and [4] and passes the result on; the three handlers call it again. `matches_pattern`
strips one `^` from the name (FR-017) so it is correct for any other caller. `scrub_params` reads
`global` too (FR-020). `global_preview` returns before minting a token when the name is refused
(FR-021). No change to the gate order or the error codes of the existing gates; the new code is
`INVALID_GLOBAL_NAME`.
