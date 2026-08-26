# Research: Write-Gate Integrity

**Feature**: 085-write-gate-integrity | **Date**: 2026-08-25

Every claim here was read out of the working tree at commit `21a1bfb` or reproduced against
live `iris-dev-iris` (localhost:52780). No API is asserted from memory.

## Current architecture, as built

The gate travels through a process-global environment variable. Four hops:

| Hop | Location                      | What happens                                                             |
| --- | ----------------------------- | ------------------------------------------------------------------------ |
| 1   | `workspace_config.rs:705-709` | toml `write_tools_enabled` → `set_var("IRIS_WRITE_TOOLS_ENABLED")`       |
| 2   | `connection.rs:133-148`       | `is_write_allowed()` reads that var, else `IRIS_ALLOW_PROD`, else infers |
| 3   | `tools/mod.rs:214`            | `ConnectionState::from_iris` caches `iris.is_write_allowed()`            |
| 4   | `tools/mod.rs:2669`           | `write_tools_enabled()` reads the cached bool                            |

Hop 1 is guarded by `if std::env::var(...).is_err()`, so it fires at most once per process.
Every subsequent config load is a no-op. That is defect 1, and it is structural: the
transport for the value is a write-once global.

`ConnectionState::new_disconnected` (`mod.rs:195-198`) re-derives the same value from the same
env var with a **different** default (`unwrap_or(true)` — absent means allowed), which is a second
reader of the same global with different semantics.

### The inference chain (`connection.rs:143-147`)

```rust
match &self.system_mode {
    SystemMode::Live => false,
    SystemMode::Development | SystemMode::Test => true,
    SystemMode::Unknown => !is_production_namespace(&self.namespace),
}
```

This is constitution Principle VI's documented chain and is unchanged by this feature. FR-019
keeps it; only its visibility changes.

### Enforcement today

Six call sites, all inside handlers, all the same four lines:

| Tool                                        | Location             |
| ------------------------------------------- | -------------------- |
| `iris_compile`                              | `mod.rs:2991`        |
| `iris_execute`                              | `mod.rs:3889`        |
| `iris_doc` (put/delete/insert/delete_lines) | `mod.rs:4179`        |
| `iris_query` (mode=write)                   | `mod.rs:4320`        |
| `global_kill`                               | `admin_tools.rs:338` |
| `iris_namespace_create`                     | `admin_tools.rs:141` |

Plus two tools removed from the router at startup when writes are off
(`mod.rs:2551-2556`): `iris_production_item`, `iris_credential_manage`. Removal is not
enforcement — it is invisible to a later reload, and it makes a completeness test pass for the
wrong reason (the tool is absent, so nothing checks it).

Everything else writes unchecked. `iris_ws_exec` is the worst case: `iris_ws_open` +
`iris_ws_exec` runs arbitrary ObjectScript, which is the `iris_execute` capability with none of
the `iris_execute` gate.

### Annotations already exist

`annotations(read_only_hint = true)` appears on ~40 tools and `destructive_hint = true` on at
least one (`mod.rs:5529`). This matters for the retrospective: the docs commit that introduced
the phantom keys was titled "document tool annotations, destructive gate, and write allowlist" —
one of its three subjects was real. It also matters for design: the annotations are an
independent declaration of the same fact the gate needs, so they can cross-check each other.

### The existing `tool_gate!` macro is the anti-pattern, again

`tools/gate_macro.rs` collapses the policy/dispatch preamble into a macro invoked per handler.
Same shape as the write guards: correct where invoked, silent where forgotten. Its own test
module contains four `#[test]` functions whose bodies are comments with no assertions
(`gate_macro.rs:89-131`). Not in scope to fix, but it is the same disease and should not be the
model for this feature.

## Decisions

### D1 — Resolve the gate as data, not through process env

**Decision**: Introduce a pure resolver and a value type.

```rust
pub enum GateSource {
    OperatorEnv,          // operator exported the gate var before the process began
    ConfigFile,           // declared in .iris-agentic-dev.toml
    LegacyAllowProd,      // IRIS_ALLOW_PROD set (issue #26 override)
    InferredSystemMode,   // nothing declared; IRIS SystemMode decided
    InferredNamespace,    // nothing declared, SystemMode unknown; namespace decided
    FailClosed,           // resolution failed; forced off (FR-005)
}

pub struct GateResolution {
    pub write_enabled: bool,
    pub destructive_enabled: bool,
    pub write_source: GateSource,
    pub destructive_source: GateSource,
}

/// Pure. No env reads, no IO.
pub fn resolve_gates(
    operator: &OperatorEnvGates,        // snapshot, plain data
    cfg: Option<&WorkspaceConfig>,
    system_mode: SystemMode,
    namespace: &str,
) -> GateResolution
```

`OperatorEnvGates` is a snapshot of the two gate env vars plus `IRIS_ALLOW_PROD`, captured once
at process start into a `OnceLock` by the binary, with a test-only setter behind
`#[cfg(any(test, feature = "testing"))]`.

Variant order **is** the precedence order (FR-003), and the list must stay in step with
data-model.md §1 — six variants, including `LegacyAllowProd`, which is what preserves the
provenance of the issue #26 override instead of reporting it as a config decision.

**Rationale**: this is the whole fix for defect 1 and for the test gap that hid it. Today the
buggy branch is "env var already set", and every existing test calls `remove_var` first, so no
test can reach it (`workspace_config.rs:1583`, `:1598`, `:1613`, `:1627`). Turning that branch
into a **parameter** makes it reachable by construction — a test passes an `OperatorEnvGates` with
the var present and asserts config still wins where it should. It also removes the
`--test-threads=1` requirement for gate-resolution tests, since nothing mutates process state.

**Alternatives rejected**:

- **Keep the env var, drop the `is_err()` guard.** Fixes the stale value and breaks documented
  precedence: an operator-set var would be clobbered by the first config load. The guard exists
  for a reason; the problem is that a write-once global cannot express "operator vs. system".
- **`OnceLock` capture and keep everything else.** Preserves precedence but leaves the value
  traveling through a global that two readers interpret with different defaults
  (`mod.rs:195-198` vs `connection.rs:134-136`). Half a fix.
- **Thread the config through `IrisConnection`.** `is_write_allowed()` is a method on the
  connection, so this looks natural, but it makes the gate answer depend on having a connection.
  FR-012 forbids exactly that — an unreachable server currently yields a permissive answer.

### D2 — One declarative classification table

**Decision**: new module `tools/write_gate.rs`:

```rust
pub enum WriteClass { ReadOnly, Write, Destructive }

pub struct ToolClass {
    pub tool: &'static str,
    /// Per-action overrides, matched against the call's action/mode argument.
    pub actions: &'static [(&'static str, WriteClass)],
    /// Applies when `actions` is empty or no action matches.
    pub default: WriteClass,
}

pub const CLASSIFICATION: &[ToolClass] = &[ /* every registered tool */ ];
```

Per-action is not optional: `iris_doc` writes on four of its modes and reads on the rest,
`iris_query` writes only on `mode="write"`, `iris_lookup_manage`'s read actions (`get`,
`list_keys`, `list_tables`) are documented as unrestricted (`docs/tools.md:921`), and the
seventh destructive item in spec 073 is `skill(action="forget")` — an action, not a tool
(`docs/tools.md:1337`).

**Completeness is a test, not a convention** — three assertions over
`IrisTools::registered_tool_names()` for Baseline ∪ Nostub ∪ Merged:

1. Every registered tool name appears in `CLASSIFICATION` (no unclassified tool).
2. Every `CLASSIFICATION` entry names a registered tool (no stale entry hiding a rename).
3. Cross-check against the annotations already in the router: `read_only_hint = true` implies
   `ReadOnly`, `destructive_hint = true` implies `Destructive`. Two independent declarations of
   the same fact, so a contributor has to lie twice to ship an ungated write tool.

Assertion 3 is what the `skills_only_reference_tools_that_exist` test does for skills, applied
to the gate.

### D3 — Enforce once, in `call_tool`

**Decision**: check the gate in `ServerHandler::call_tool` (`mod.rs:8213`) before building the
`ToolCallContext`. `request.name` and `request.arguments` are both in hand there, and nothing
has touched IRIS yet.

Return the refusal as a normal tool result carrying `WRITE_TOOLS_DISABLED` /
`DESTRUCTIVE_TOOLS_DISABLED`, not an `McpError` — Principle V (Output Shape Parity) and the
existing `err_json` shape that the four current guards already produce, so the reporter's
probes keep seeing the same response shape.

This satisfies FR-008 (single point), FR-010 (never reaches IRIS), FR-011 (call time, so a
reload takes effect inside a live session) and FR-012 (no connection needed to answer).

Consequence: the six in-handler guards and the two router removals get deleted. Deleting the
router removals is a behavior change worth naming — `iris_production_item` and
`iris_credential_manage` become **visible but refusing** instead of **absent**. That is strictly
better for an agent, which currently cannot tell "tool does not exist" from "tool is gated".

**Alternative rejected**: keep the macro-per-handler shape and add a completeness test that
greps handler bodies for the guard. Tested the idea against the failure history — it is a text
test over source, it cannot see a guard placed after an IRIS call, and it is exactly the
"remember to invoke it" property that produced four rounds of this issue.

### D4 — Fail closed at startup, exit non-zero

**Decision**: extract a pure validator, called by `mcp.rs` before discovery:

```rust
pub fn validate_gate_config(cfg: &WorkspaceConfig) -> Result<(), GateConfigError>
```

`mcp.rs` logs and `std::process::exit(2)` on `Err`. `workspace_config_to_connection` loses its
`return None` at `:695-703` entirely — that early return **is** the fail-open, because it skips
the export below it and drops the caller into the namespace inference.

**Rationale**: the current code documents its own deviation in a comment ("callers that need to
surface this as a hard error can inspect the log") and those callers were never written. Moving
the decision to the one caller that can exit removes the ambiguity. Exit code 2 distinguishes
config rejection from the existing `exit(1)` at `mcp.rs:257` (invalid transport).

Startup already has the shape needed: `apply_explicit_config_file` and
`apply_workspace_config_with_path` (`mcp.rs:136-150`) are the two entry points, both called
before `discover_iris`.

### D5 — Docs integrity test: four extractors, over two surfaces

A single-extractor grep for error codes catches 4 of the 8 known unbacked identifiers. The
forensic review found the other 4 in spec **072** — which had plan, tasks, lift results, and a
full implementation. So the extractor set has to be wider than the defect that prompted it.

**Decision**: one test, four extractors, run over `docs/tools.md`, `docs/connecting.md`, and
`skills/**/SKILL.md`:

| Extractor       | Pattern                                             | Assertion                          | Catches today                                                                                                 |
| --------------- | --------------------------------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Error codes     | `` `SCREAMING_SNAKE` `` in prose and the code table | emitted somewhere in crate sources | `DESTRUCTIVE_TOOLS_DISABLED`, `WRITE_SERVER_NOT_ALLOWED`, `WS_SESSION_NOT_FOUND`, `WS_TERMINAL_NOT_SUPPORTED` |
| Config keys     | `` ### `snake_case` `` headings + toml fence lines  | deserializes **and has a reader**  | `write_allowed_servers`, and `destructive_tools_enabled` until FR-018 lands                                   |
| Env vars        | `\b(IRIS\|IAD)_[A-Z0-9_]+\b`                        | read somewhere in crate sources    | `IRIS_WRITE_ALLOWED_SERVERS`, `IRIS_WS_TIMEOUT_SECS`                                                          |
| Tool parameters | param rows under a `` ### `tool_name` `` heading    | the handler reads that key         | `max_chars` on `stream_inspect`                                                                               |

Two of these need care:

- **Config keys assert a reader, not a mention.** `IRIS_DESTRUCTIVE_TOOLS_ENABLED` is present in
  the sources at `workspace_config.rs:711-712` — as a setter with no getter anywhere. A
  presence grep is green on the exact defect this spec exists to fix. The assertion is "some
  code path outside the export site consumes this", which for config keys means the resolver
  from D1 branches on the field.
- **Counts.** `docs/tools.md:1468` claims `read_only_hint` is on 57 tools. `c641d79` (2026-08-18,
  fixes #94) removed it from six mutating tools, so it is 51. Every identifier in that sentence
  is real. A fifth mini-extractor reads the number out of the sentence and compares it to
  `list_all()`.

Exemptions take the form `PLANNED(spec-NNN)` inline in the documentation, so an exemption is
visible to the reader of the docs rather than buried in a test file.

**What this still misses** — stated plainly, because pretending otherwise is how this feature
repeats:

- **Prose behavior claims.** `docs/tools.md:1503` says the server "refuses to start" and names
  `DESTRUCTIVE_REQUIRES_WRITES`, which exists as a log string. Every extractor above passes; the
  code logs and continues. Only the US4 behavioral test catches it.
- **The check-order block** at `docs/tools.md:1533-1540` is six numbered prose steps with no
  identifiers to extract. Steps 2 and 3 are fictional and nothing mechanical will say so.

The docs test raises the floor. It is not a substitute for the enforcement matrix, and no
requirement in this spec delegates behavior verification to it.

**Where it runs**: as a step in the existing `doc-lint` job (`ci.yml:439`), which already checks
out the repo and needs no toolchain, plus as a normal `cargo test` unit test so it fails locally
before CI. Worth noting what `doc-lint` does today: greps for a wrong settings path, asserts
`~/.claude.json` appears in the README, and `json.loads` every json fence in the README. It never
opens `docs/tools.md`, and no test in either crate reads any documentation file.

### D6 — Lockfile drift

**Decision**: `--locked` on every `cargo build`/`cargo test` in `ci.yml` (lines 28, 44, 53, 86,
190, 239, 366, 428) and on `cargo zigbuild` in `release.yml:43`. Today `--locked` appears
exactly once in either file, at `ci.yml:424`, on `cargo install cargo-llvm-cov` — which
protects the tool, not the build.

Plus a test that shells out to `cargo metadata --locked --format-version 1` and asserts exit 0.
`cargo metadata` resolves without compiling, so the test is fast and its failure message names
the drifting package.

**Rationale**: `build.rs` runs `git describe --tags --always --dirty` after cargo has already
rewritten the lockfile during resolution, so drift is invisible until it shows up as
`1.2.6+v1.2.6-dirty` in `server_version` — which `check_config`'s own description advertises as
the way to identify an official build.

## API verification (Principle II)

This feature adds no new ObjectScript. The gate is enforced before any IRIS call, and the
integration tests use existing verified paths.

| Thing used                                                                              | How verified                                                   |
| --------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `is_write_allowed()` inference chain                                                    | Read at `connection.rs:133-148`; matches Principle VI          |
| SystemMode probe SQL `%Library.Global_Get('%SYS', ...)`                                 | Unchanged, `connection.rs:247`; live-probed on `iris-dev-iris` |
| `iris_ws_open` + `iris_ws_exec` bypass                                                  | Reproduced live against 1.2.6 and 1.2.1 released binaries      |
| `iris_global` set/kill, `iris_lookup_manage` set/delete, `iris_execute_method` bypasses | Reproduced live, gate provably active in the same session      |
| `registered_tool_names()` as the registry accessor                                      | `mod.rs:2341`, derives from `tool_router.list_all()`           |

Side-effect absence (FR-025) is read back with `iris_global` get and `iris_query`, both
read-only and both unaffected by the gate — so the assertion itself cannot be blocked by the
thing it is verifying.

## Dependencies (Principle VII)

**No new crates.** The classification table is `const` data, the resolver is a pure function,
the docs test uses `regex` (already a dev-dependency), and the lockfile test uses
`std::process::Command` against the `cargo` already on PATH.
