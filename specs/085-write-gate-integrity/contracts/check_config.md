# Contract: `check_config` response, and the gate refusal envelope

**Feature**: 085-write-gate-integrity | **Date**: 2026-08-25

Two contracts change. One tool response gains fields; one error envelope gains a code and a new
emission site.

## 1. `check_config` — added fields

Declared output schema: `CheckConfigOk` (`output_schemas.rs:3289`). Response body built at
`mod.rs:4695-4720`.

### Before

```json
{
  "server_version": "1.2.6",
  "connected": true,
  "connection_source": "http",
  "write_tools_enabled": true
}
```

`write_tools_enabled` reflects `ConnectionState.write_tools_enabled`, which came from
`is_write_allowed()`, which read a process-global env var. When a config edit could not change that
var, this field reported `true` forever while `config_loaded_at` advanced — the defect the reporter
is looking at.

### After

```json
{
  "server_version": "1.2.7",
  "connected": true,
  "connection_source": "http",
  "write_tools_enabled": false,
  "write_tools_source": "config_file",
  "destructive_tools_enabled": false,
  "destructive_tools_source": "inferred_default"
}
```

| Field                       | Type     | Required | Notes                                                          |
| --------------------------- | -------- | -------- | -------------------------------------------------------------- |
| `write_tools_enabled`       | `bool`   | yes      | Unchanged name, now sourced from `GateResolution` (FR-001)     |
| `write_tools_source`        | `string` | yes      | `GateSource` wire value (FR-004)                               |
| `destructive_tools_enabled` | `bool`   | yes      | New. The key has been accepted since v1.0.0 and never reported |
| `destructive_tools_source`  | `string` | yes      | New (US7 scenario 4)                                           |

**Backward compatibility**: additive only. `write_tools_enabled` keeps its name, type, and
meaning; what changes is that it now tells the truth. Existing consumers — including the
reporter's probes and `tests/integration/test_live_reload_e2e.rs:312` — keep parsing.

### Pre-existing schema defect to fix in the same change

`server_version` is written into the response body at `mod.rs:4715` and is **absent from
`CheckConfigOk`**, while the tool's own description advertises it first in the field list. Same
class of defect as the docs: the declared contract and the actual payload disagree. US6 makes
`server_version` load-bearing (it is how an operator identifies an official build), so it gets
added to the struct here.

This is also the trap the four new fields must avoid: they go into **both** the `json!` body and
`CheckConfigOk`, and the structured-output tests in `test_output_schema_shapes.rs` are the check
that they did.

### Invariants a test must assert

1. `write_tools_source` is one of the seven `GateSource` wire values — never absent, never empty.
2. `destructive_tools_enabled == true` implies `write_tools_enabled == true` (data-model invariant).
3. The reported `write_tools_enabled` agrees with what an actual write attempt does, in the same
   session, for every configuration in the quickstart matrix (SC-003). Reporting and enforcement
   read the same `GateResolution`, so this is structural rather than coincidental — the test exists
   to keep it that way.

## 2. Gate refusal envelope

Emitted from the single dispatch point in `call_tool`, before the router runs and before any IRIS
call (FR-008, FR-010).

```json
{
  "error_code": "WRITE_TOOLS_DISABLED",
  "error": "iris_ws_exec is write-capable and write tools are disabled (source: config_file). Set write_tools_enabled = true in .iris-agentic-dev.toml to allow writes."
}
```

```json
{
  "error_code": "DESTRUCTIVE_TOOLS_DISABLED",
  "error": "iris_remove_server is a destructive tool and the destructive tier is disabled (source: inferred_default). Set destructive_tools_enabled = true in .iris-agentic-dev.toml to allow it."
}
```

| Property        | Requirement                                                                                       |
| --------------- | ------------------------------------------------------------------------------------------------- |
| Shape           | The existing `err_json` envelope the six current guards emit — same field names (Principle V)     |
| Transport       | A normal `CallToolResult`, **not** an `McpError`, so probes see the same response shape as today  |
| Message content | Names the tool, the gate, the deciding source, and the key to set                                 |
| Ordering        | Write gate is checked first — a destructive tool with writes off returns `WRITE_TOOLS_DISABLED`   |
| Side effects    | None. FR-025 tests assert the target global/class/lookup entry/namespace does not exist afterward |

## 3. Startup rejection

Not a tool response — a process contract.

| Condition                                                             | Exit code | stderr                             |
| --------------------------------------------------------------------- | --------- | ---------------------------------- |
| `destructive_tools_enabled = true` with `write_tools_enabled = false` | `2`       | `DESTRUCTIVE_REQUIRES_WRITES: ...` |
| Invalid `--transport` (existing)                                      | `1`       | unchanged                          |
| Normal shutdown                                                       | `0`       | unchanged                          |

Today this condition exits `0`, logs the code, and serves requests with writes **enabled**. The
existing test asserts only that the string appears in stderr, which is why that shipped (FR-027).
