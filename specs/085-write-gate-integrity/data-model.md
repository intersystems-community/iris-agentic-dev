# Data Model: Write-Gate Integrity

**Feature**: 085-write-gate-integrity | **Date**: 2026-08-25

Five entities. Two are new value types, one is the tool classification table, one is an existing
struct that changes shape, and one is the error-code registry.

## 1. GateSource

Which input decided a gate's value. Reported by `check_config` (FR-004) so a future mismatch is
diagnosable instead of a four-round issue.

| Variant              | Meaning                                                                | Wire value               |
| -------------------- | ---------------------------------------------------------------------- | ------------------------ |
| `OperatorEnv`        | Operator exported the gate env var before the process began            | `"operator_env"`         |
| `ConfigFile`         | Declared in `.iris-agentic-dev.toml`                                   | `"config_file"`          |
| `LegacyAllowProd`    | `IRIS_ALLOW_PROD` set (issue #26 override)                             | `"legacy_allow_prod"`    |
| `InferredSystemMode` | Nothing declared; IRIS `SystemMode` decided it                         | `"inferred_system_mode"` |
| `InferredNamespace`  | Nothing declared, `SystemMode` unknown; namespace decided it           | `"inferred_namespace"`   |
| `InferredDefault`    | Nothing declared and nothing infers it; the documented default applied | `"inferred_default"`     |
| `FailClosed`         | Resolution failed; forced off (FR-005)                                 | `"fail_closed"`          |

Ordering in the list **is** the precedence order (FR-003). `serde(rename_all = "snake_case")`.

Validation: `InferredSystemMode`, `InferredNamespace`, and `InferredDefault` are the only variants
reachable with no declaration; all three are permitted and all three keep today's behavior
unchanged (FR-019).

`InferredDefault` exists because the destructive tier is never inferred from `SystemMode` or from
the namespace — it is simply off until declared. Reporting that as `fail_closed` would tell an
operator something failed when nothing did, which is the same class of dishonest reporting this
feature exists to remove. It is the wire value `contracts/check_config.md` already shows for an
undeclared tier. `FailClosed` stays reserved for a gate forced off because resolution could not be
trusted: an unparseable config, or the invariant clamp in §2.

## 2. GateResolution

The resolved answer for one connection context. Immutable once produced; replaced wholesale on
config reload rather than mutated (which is what makes FR-002 hold in both directions).

| Field                 | Type         | Notes                              |
| --------------------- | ------------ | ---------------------------------- |
| `write_enabled`       | `bool`       | Effective write gate               |
| `write_source`        | `GateSource` | What decided `write_enabled`       |
| `destructive_enabled` | `bool`       | Effective destructive tier         |
| `destructive_source`  | `GateSource` | What decided `destructive_enabled` |

**Invariant (FR-018)**: `destructive_enabled == true` implies `write_enabled == true`. The
resolver enforces it rather than trusting callers — `destructive_enabled` is computed as
`declared_destructive && write_enabled`. The contradictory _declaration_ is rejected earlier, at
startup, by `validate_gate_config`; this invariant is the belt to that suspenders, and it is what
makes US7 scenario 3 hold.

Produced by:

```rust
pub fn resolve_gates(
    operator: &OperatorEnvGates,
    cfg: Option<&WorkspaceConfig>,
    system_mode: SystemMode,
    namespace: &str,
) -> GateResolution
```

Pure — no env reads, no IO, no clock. That is the whole point: the branch that carries the #110
defect ("env var already set") becomes an argument a test can supply, instead of process state
every existing test clears away (FR-024).

### OperatorEnvGates

A snapshot, captured once at process start, of what the operator set — as distinct from what the
system set later while loading a config. Conflating those two is the current defect.

| Field                 | Type           | Source env var                   |
| --------------------- | -------------- | -------------------------------- |
| `write_tools_enabled` | `Option<bool>` | `IRIS_WRITE_TOOLS_ENABLED`       |
| `destructive_enabled` | `Option<bool>` | `IRIS_DESTRUCTIVE_TOOLS_ENABLED` |
| `allow_prod`          | `bool`         | `IRIS_ALLOW_PROD`                |

Parsing: `"1"` or case-insensitive `"true"` → `true`; any other present value → `false`; absent →
`None`. Held in a `OnceLock`, with a setter behind `#[cfg(any(test, feature = "testing"))]` so
tests construct it directly rather than mutating the environment.

## 3. Tool classification

### WriteClass

| Variant       | Gate required                                 |
| ------------- | --------------------------------------------- |
| `ReadOnly`    | none                                          |
| `Write`       | `write_enabled`                               |
| `Destructive` | `write_enabled` **and** `destructive_enabled` |

`Destructive` is a subset of `Write`, not a sibling — so a destructive tool with writes off is
refused with `WRITE_TOOLS_DISABLED`, not `DESTRUCTIVE_TOOLS_DISABLED`. That ordering is what US7
scenario 3 asserts.

### ToolClass

| Field     | Type                                    | Notes                                                       |
| --------- | --------------------------------------- | ----------------------------------------------------------- |
| `tool`    | `&'static str`                          | Registered tool name                                        |
| `actions` | `&'static [(&'static str, WriteClass)]` | Per-action overrides (FR-009); empty for single-class tools |
| `default` | `WriteClass`                            | Applies when `actions` is empty or no action matches        |

Lookup: find the entry by tool name, then look for an action override keyed on the call's
`action` or `mode` argument, else `default`. An unknown action falls to `default`, which for
write-capable tools means "gated" — unknown actions fail closed.

`CLASSIFICATION: &[ToolClass]` is the single source of truth (FR-007) for both enforcement and the
completeness test.

**Destructive tier** (FR-018, from spec 073, marked ☠ in `docs/tools.md`):

| Item                       | Note                                                           |
| -------------------------- | -------------------------------------------------------------- |
| `global_kill`              | Already write-gated; gains the tier                            |
| `iris_admin`               |                                                                |
| `iris_credential_manage`   | Currently stripped from the router instead of gated            |
| `iris_lookup_manage`       | Per-action — `get`, `list_keys`, `list_tables` stay read-only  |
| `iris_namespace_create`    | Already write-gated; gains the tier                            |
| `iris_remove_server`       | Local state, not IRIS — test asserts the saved server survives |
| `skill(action = "forget")` | An **action**, not a tool. Local state.                        |

`iris_production_item` is the eighth tool currently stripped from the router. It is not in spec
073's tier; it classifies as `Write`.

**Two judgment calls made while filling the table**, recorded here because neither is derivable
from spec 073 and both change what a call is allowed to do:

- `iris_global` `kill` is `Destructive`, not `Write`. Spec 073 lists the standalone `global_kill`
  tool in the tier but predates `iris_global`, which reaches the same `KILL` through an `action`
  argument. Classifying it as merely `Write` would leave the documented tier bypassable by picking
  the other spelling of the same operation.
- `iris_lookup_manage` `export` is `ReadOnly`. The per-action list in spec 073 names
  `get`/`list_keys`/`list_tables`; `export` serialises a lookup table out and touches nothing in
  IRIS, so grouping it with `set`/`delete` would gate a read.

### Validation rules (the completeness test, FR-026)

1. Every name from the router's registry appears in `CLASSIFICATION`.
2. Every `CLASSIFICATION` entry names a tool the router registered — catches a rename leaving a
   stale entry that silently stops matching.
3. `read_only_hint == true` ⟹ `ReadOnly`; `destructive_hint == true` ⟹ `Destructive`. Two
   independent declarations, cross-checked. Not derived from each other, because `c641d79` proved
   the annotations can be wrong.

## 4. ConnectionState (existing — changes shape)

`crates/iris-agentic-dev-core/src/tools/mod.rs:181`

| Field                       | Change                                  |
| --------------------------- | --------------------------------------- |
| `write_tools_enabled: bool` | **Replaced** by `gates: GateResolution` |
| `declared: DeclaredGates`   | **Added** — see below                   |
| everything else             | unchanged                               |

Both constructors change. `from_iris` currently calls `iris.is_write_allowed()`, which reads the
env var; `new_disconnected` currently reads the same var with the opposite default
(`unwrap_or(true)` — absent means allowed). Two readers, two defaults, one global. Both take a
`GateResolution` instead, so the disconnected path stops being accidentally permissive (FR-012).

### Why `declared` is on the state and not just the connection

`DeclaredGates` (two `Option<bool>`s, `Copy`) is what the config file _said_, kept alongside the
resolution it produced. The resolution has to be recomputed whenever its inputs change, and two
paths change them after the config load:

- `iris_select_container` swaps in a connection with a different namespace and `SystemMode`.
  Re-resolving without the declaration would fall back to inference and silently widen the gate.
- The `IRIS_CONTAINER` branch of discovery (`iris/discovery.rs:198-216`) constructs a **fresh**
  `IrisConnection` rather than passing the config's one through, so a declaration attached only to
  `IrisConnection` would be dropped on the container path — which is this repo's own default
  configuration.

Set through `with_declared` after either constructor, so a caller that has no declaration (tests,
`IrisTools::new`) does not have to invent one. `DeclaredGates::default()` means _nothing declared_,
which is distinct from _declared false_ (FR-001).

## 5. Error code registry

Per the constitution's Error Code Registry rule, `SCREAMING_SNAKE_CASE`, declared here.

| Code                          | Status                                           | Meaning                                                                                                                                  |
| ----------------------------- | ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `WRITE_TOOLS_DISABLED`        | exists — reused                                  | Write-capable call refused; write gate off. `ERR_WRITE_GATE`, `admin_tools.rs:13`                                                        |
| `DESTRUCTIVE_TOOLS_DISABLED`  | **new** — implements                             | Destructive-tier call refused; writes on, tier off. Documented since v1.0.0 (`docs/tools.md:1496`, `:1614`); has never existed in source |
| `DESTRUCTIVE_REQUIRES_WRITES` | exists as a log string — becomes an emitted code | Contradictory declaration. Today it is logged and the server continues; it becomes a startup rejection with exit 2 (FR-006)              |
| `WRITE_SERVER_NOT_ALLOWED`    | **deleted from docs**                            | Never existed. Spec 074 stays open (FR-017)                                                                                              |

Also removed from documentation, not from behavior: `IRIS_WRITE_ALLOWED_SERVERS` and the
`write_allowed_servers` key — neither has ever existed in source.

### Inherited corrections (FR-016c)

The docs-contract test flags four identifiers from spec 072 and one stale count. In scope to
correct in the documentation; the described behavior is explicitly not implemented here.

| Documented                       | Reality                                                         | Action                      |
| -------------------------------- | --------------------------------------------------------------- | --------------------------- |
| `WS_SESSION_NOT_FOUND`           | code emits `SESSION_WS_DISCONNECTED` (`ws_session.rs:22`)       | correct the docs            |
| `WS_TERMINAL_NOT_SUPPORTED`      | code emits `SESSION_WS_UNAVAILABLE` (`ws_session.rs:23`)        | correct the docs            |
| `IRIS_WS_TIMEOUT_SECS`           | hardcoded `WS_FRAME_TIMEOUT_SECS = 30` (`ws_session.rs:27`)     | remove from docs            |
| `max_chars` on `stream_inspect`  | handler reads only `oid`, `namespace`, `server` (`mod.rs:7977`) | remove from docs            |
| "57 tools" with `read_only_hint` | 51 since `c641d79` removed six                                  | correct, and assert by test |

## Startup validation

```rust
pub fn validate_gate_config(cfg: &WorkspaceConfig) -> Result<(), GateConfigError>
```

Pure. One variant today: `DestructiveRequiresWrites`. Called by `mcp.rs` before discovery; on
`Err` it logs the code and `std::process::exit(2)` — distinct from the existing `exit(1)` for an
invalid transport (`mcp.rs:257`). This replaces the `return None` at `workspace_config.rs:695-703`,
which is the fail-open: it skips the export below it and drops the caller into the permissive
namespace inference.
