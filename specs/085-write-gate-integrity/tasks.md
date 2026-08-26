# Tasks: Write-Gate Integrity

**Input**: Design documents from `/specs/085-write-gate-integrity/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/check_config.md, quickstart.md

**Tests**: Required, not optional. The spec makes them functional requirements (FR-022 through
FR-030) because every previous round of #110 shipped with a green suite. Tests go first in every
phase and must fail before the implementation task in that phase is started.

**Organization**: Grouped by user story. US1 and US2 are both P1 and share the foundational
resolver, so Phase 2 is a hard gate for everything.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel — different files, no dependency on an incomplete task
- **[Story]**: US1–US7 from spec.md
- Every task names the exact file it touches

## Path Conventions

Two-crate Rust workspace. Core crate is `crates/iris-agentic-dev-core/`, binary crate is
`crates/iris-agentic-dev-bin/`. Test targets live under `tests/unit/` and `tests/integration/`
and must each be registered as a `[[test]]` in the owning crate's `Cargo.toml`.

Line numbers below are from commit `21a1bfb`. If a number has moved, find the named symbol —
the symbol name is authoritative, the line is a convenience.

---

## Phase 1: Setup

**Purpose**: Get the new module and test targets compiling so every later task has somewhere to land.

- [x] T001 Create `crates/iris-agentic-dev-core/src/tools/write_gate.rs` with the module doc
      comment and nothing else, and add `pub mod write_gate;` to
      `crates/iris-agentic-dev-core/src/tools/mod.rs` alongside the existing `pub mod` lines
- [x] T002 Register five new `[[test]]` targets in `crates/iris-agentic-dev-core/Cargo.toml`
      following the existing `name` + `path` convention: `test_gate_resolution`
      (`tests/unit/test_gate_resolution.rs`), `test_gate_classification`
      (`tests/unit/test_gate_classification.rs`), `test_docs_contract`
      (`tests/unit/test_docs_contract.rs`), `test_lockfile_sync`
      (`tests/unit/test_lockfile_sync.rs`), and `test_gate_enforcement_live`
      (`tests/integration/test_gate_enforcement_live.rs`, with
      `required-features = ["testing"]`)
- [x] T003 [P] Record the pre-change baseline so later regressions are attributable: run
      `docker ps --filter name=iris-dev-iris` to confirm the container, then
      `cargo test --features testing -- --include-ignored --test-threads=1` and
      the canonical coverage command from Constitution VIII
      (`cargo llvm-cov --summary-only -p iris-agentic-dev-core --features testing -- --include-ignored --test-threads=1`,
      with the `LLVM_COV`/`LLVM_PROFDATA` paths that command specifies), and record the TOTAL
      line-coverage number in `specs/085-write-gate-integrity/lift-results.md` so T073 compares
      like with like

**Checkpoint**: `cargo build` and `cargo test` are green and unchanged in behavior.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The gate value stops being process-global state and becomes data. Every user story
except US5 and US6 depends on this.

**CRITICAL**: No user story work begins until this phase is complete.

### Tests for Phase 2

- [x] T004 Write the precedence matrix in
      `crates/iris-agentic-dev-core/tests/unit/test_gate_resolution.rs`: for each combination of
      operator-env present/absent/opposite, config declared true/false/absent, `SystemMode`
      Live/Development/Test/Unknown, and namespace production/non-production, assert
      `resolve_gates` returns the expected `write_enabled` **and** the expected `write_source`.
      Every config in this test MUST be produced by `toml::from_str` on a config **string**, never
      a `WorkspaceConfig` struct literal (FR-022). Include the operator-env-already-set branch that
      no existing test reaches (FR-024) and the unchanged inference cases (FR-019)
- [x] T005 Add fail-closed and invariant tests to
      `crates/iris-agentic-dev-core/tests/unit/test_gate_resolution.rs`: an unresolvable config
      yields `write_enabled = false` with `write_source = FailClosed` (FR-005), and
      `destructive_enabled` is never true while `write_enabled` is false, for every input
      combination (FR-018 invariant, data-model.md §2)
- [x] T006 Add the disconnected-path test to
      `crates/iris-agentic-dev-core/tests/unit/test_gate_resolution.rs`: construct
      `ConnectionState::new_disconnected` with a resolution declaring writes off and assert the
      gate answer is off — the current code re-derives from the env var with `unwrap_or(true)`, so
      an unreachable server answers permissively. Then assert the complementary case with `None`
      for the `iris` parameter: writes resolved **on** but no connection returns `IRIS_UNREACHABLE`
      and not a gate error, so the new upstream check cannot mask an unreachable server
      (FR-012, Constitution IV `None`-iris rule)

### Implementation for Phase 2

- [x] T007 Define `GateSource` (seven variants — `InferredDefault` was added because the
      destructive tier is never inferred, and reporting that as `fail_closed` would tell an
      operator something failed when nothing did — with `serde(rename_all = "snake_case")`),
      `GateResolution` (four fields), and `OperatorEnvGates` (three fields) in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs` per data-model.md §1–2
- [x] T008 Implement the pure resolver in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs` with the signature
      `resolve_gates(operator, cfg, system_mode, namespace) -> GateResolution`. No `std::env`
      reads, no IO, no clock. Precedence is the `GateSource` list order; the existing inference
      chain from `crates/iris-agentic-dev-core/src/iris/connection.rs:143-147` moves in unchanged
      (FR-001, FR-003, FR-019)
- [x] T009 Implement the `OperatorEnvGates` process snapshot in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs`: a `OnceLock` captured once at
      process start, `"1"`/case-insensitive `"true"` parsing, plus a setter behind
      `#[cfg(any(test, feature = "testing"))]` so tests supply it as data instead of mutating the
      environment (FR-003)
- [x] T010 Define `WriteClass`, `ToolClass`, and the `CLASSIFICATION: &[ToolClass]` table in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs`, with one entry for every name
      `IrisTools::registered_tool_names()` returns across Baseline, Nostub, and Merged. The write
      set must cover at minimum the tools verified ungated today — `iris_ws_exec`, `iris_global`
      set/kill, `iris_lookup_manage` set/delete, `iris_execute_method` — plus
      `iris_source_control`, `iris_production`, `iris_production_item`, `iris_lookup_transfer`,
      `iris_test`, `iris_generate`, `iris_generate_class`, `iris_generate_test`, `kb_index`,
      `skill_forget`, `skill_propose`, `skill_share`, `skill_community_install`, and the six
      already-guarded tools (FR-007, FR-013)
- [x] T011 Implement the classification lookup in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs`: resolve by tool name, then by the
      call's `action` or `mode` argument against the entry's per-action overrides, falling back to
      `default`. An unrecognised action falls to `default`, so unknown actions on a write-capable
      tool are gated. `iris_doc` writes on put/delete/insert/delete_lines, `iris_query` writes only
      on `mode="write"`, and `iris_lookup_manage` keeps `get`/`list_keys`/`list_tables` read-only
      (FR-009)
- [x] T012 Replace `write_tools_enabled: bool` with `gates: GateResolution` on `ConnectionState`
      in `crates/iris-agentic-dev-core/src/tools/mod.rs:181`, and update both constructors —
      `from_iris` (`:214`) and `new_disconnected` (`:195-198`) — to take the resolution instead of
      re-deriving it from the environment with two different defaults (FR-012).
      **Also carries `declared: DeclaredGates`** (set via `with_declared`): the `IRIS_CONTAINER`
      discovery path at `iris/discovery.rs:198-216` builds a _fresh_ `IrisConnection`, and
      `iris_select_container` re-resolves against a new namespace/`SystemMode`, so a declaration
      attached only to the connection would be silently dropped on this repo's own default
      configuration. The env var used to paper over both
- [x] T013 Rewrite `is_write_allowed()` in
      `crates/iris-agentic-dev-core/src/iris/connection.rs:133-148` to delegate to
      `resolve_gates`, so the inference chain has exactly one implementation and one reader
- [x] T014 Delete the environment exports from
      `crates/iris-agentic-dev-core/src/iris/workspace_config.rs:705-712` — both
      `IRIS_WRITE_TOOLS_ENABLED` and the reader-less `IRIS_DESTRUCTIVE_TOOLS_ENABLED` setter, plus
      the fail-open `return None` that preceded them — and return the declaration to the caller
      instead (FR-001, FR-002). **Deviation**: returned from the two `apply_*` wrappers
      (`apply_workspace_config_with_path`, `apply_explicit_config_file`, both now
      `(Option<IrisConnection>, Option<PathBuf>, DeclaredGates)`) rather than from
      `workspace_config_to_connection`, whose signature has ~35 test call sites against the
      wrappers' ~6. Same data, one twentieth the churn
- [x] T015 Add `DESTRUCTIVE_TOOLS_DISABLED` beside the existing
      `ERR_WRITE_GATE = "WRITE_TOOLS_DISABLED"` in
      `crates/iris-agentic-dev-core/src/tools/admin_tools.rs:13`, per the registry in
      data-model.md §5

**Checkpoint**: T004–T006 pass. The gate is resolvable as a value with no process-global state.

---

## Phase 3: User Story 1 - Declared gate actually blocks every write (Priority: P1) 🎯 MVP

**Goal**: With writes declared off, every write-capable tool refuses and nothing reaches IRIS.

**Independent Test**: With the gate off, call every write-capable tool against live IRIS; each
returns the write-gate error **and** the global, class, lookup entry, or namespace it would have
created does not exist afterward.

### Tests for User Story 1

- [x] T016 [US1] Write the write-refusal matrix in
      `crates/iris-agentic-dev-core/tests/integration/test_gate_enforcement_live.rs`, driven off
      `CLASSIFICATION` rather than a hand-written list, against live `iris-dev-iris`
      (localhost:52780). For each write-capable tool with the gate off: assert the response
      carries `WRITE_TOOLS_DISABLED`, then read back with `iris_global` get or `iris_query` and
      assert the side effect is **absent** — probe globals use the `^IADGate085` prefix. The
      absence assertion is the point of the test, not the error code (FR-010, FR-025, FR-026,
      SC-001)
- [x] T017 [US1] Add the `iris_ws_open` + `iris_ws_exec` case explicitly to
      `crates/iris-agentic-dev-core/tests/integration/test_gate_enforcement_live.rs`: open a
      session, execute `set ^IADGate085=1`, assert refusal, then assert the global does not exist.
      This is the complete bypass of the `iris_execute` gate and the highest-severity finding
- [x] T018 [US1] Add the read-only and gate-on cases to
      `crates/iris-agentic-dev-core/tests/integration/test_gate_enforcement_live.rs`: with the
      gate off every read-only tool still succeeds, and with the gate on the same write calls
      proceed and the expected state change is observable (US1 scenarios 3 and 4)

### Implementation for User Story 1

- [x] T019 [US1] Add the single gate check to `ServerHandler::call_tool` in
      `crates/iris-agentic-dev-core/src/tools/mod.rs:8213`, before
      `ToolCallContext::new(self, request, context)` consumes the request — `request.name` and
      `request.arguments` are both in hand there and nothing has touched IRIS yet (FR-008, FR-010,
      FR-011, FR-012)
- [x] T020 [US1] Implement the refusal envelope in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs` as a normal tool result in the
      existing `err_json` shape carrying `WRITE_TOOLS_DISABLED`, not an `McpError`, so the
      reporter's published probes keep parsing the same response shape (Principle V, contract in
      `contracts/check_config.md`)
- [x] T021 [US1] Delete the four in-handler write guards from
      `crates/iris-agentic-dev-core/src/tools/mod.rs` — `iris_compile` (`:2991`), `iris_execute`
      (`:3889`), `iris_doc` (`:4179`), `iris_query` (`:4320`) — now that the check runs upstream
- [x] T022 [US1] Delete the two in-handler write guards from
      `crates/iris-agentic-dev-core/src/tools/admin_tools.rs` — `iris_namespace_create` (`:141`)
      and `global_kill` (`:338`) — keeping `ERR_WRITE_GATE` itself, which the new envelope uses
- [x] T023 [US1] Delete the two router removals at
      `crates/iris-agentic-dev-core/src/tools/mod.rs:2551-2556`, so `iris_production_item` and
      `iris_credential_manage` become visible-but-refusing instead of absent. Removal is not
      enforcement: it is invisible to a later reload and it makes the completeness test pass for
      the wrong reason (research.md D3)
- [x] T024 [US1] Assert T023 actually took effect, in
      `crates/iris-agentic-dev-core/tests/unit/test_gate_classification.rs`: with writes resolved
      off, `IrisTools::registered_tool_names()` still contains `iris_production_item` and
      `iris_credential_manage`, so the Phase 5 completeness test covers them instead of passing
      because they are absent. The constitution's Toolset Registration Rules sync clause needs no
      separate task here — `registered_tool_names()` derives from `tool_router.list_all()`
      (`crates/iris-agentic-dev-core/src/tools/mod.rs:2341`), so the hand-maintained mirror that
      clause describes no longer exists

**Checkpoint**: T016–T018 pass against live IRIS. SC-001 holds. This is the MVP.

---

## Phase 4: User Story 2 - Config edits change the gate, and reporting tells the truth (Priority: P1)

**Goal**: A config edit takes effect in both directions inside one process, and `check_config`
reports both the effective gate and what decided it.

**Independent Test**: One server process, one config file rewritten twice; the gate and the
reported value follow the file each time, and an actual write attempt agrees with the report.

### Tests for User Story 2

- [x] T025 [US2] Add the rewrite-twice test to
      `crates/iris-agentic-dev-bin/tests/integration/test_mcp_binary_config.rs`: one spawned
      server, config written `true` then `false` then `true`, asserting the reported value and an
      actual write attempt agree at each step. It must use cwd or `--workspace` discovery, not
      `--config`, because `check_reload`'s watcher is what is under test, and it must not
      `env_remove` the gate variable the way `spawn_mcp` (`:27-33`) does. No test in the repo does
      this today; that single omission is why three defects shipped together (FR-023, SC-002)
- [x] T026 [US2] Change the assertion at
      `crates/iris-agentic-dev-core/tests/integration/test_live_reload_e2e.rs:312` from
      `write_tools_enabled.is_some()` to an assertion on the **value**, and add the matching
      source-field assertion. A permanently hardcoded `true` passes the current test (FR-028)
- [x] T027 [P] [US2] Extend `crates/iris-agentic-dev-core/tests/unit/test_output_schema_shapes.rs`
      to assert `CheckConfigOk` carries `write_tools_enabled`, `write_tools_source`,
      `destructive_tools_enabled`, `destructive_tools_source`, **and** `server_version` — the last
      is written into the response body at `crates/iris-agentic-dev-core/src/tools/mod.rs:4715`
      and advertised first in the tool description, but is absent from the declared schema at
      `crates/iris-agentic-dev-core/src/tools/output_schemas.rs:3289`
- [x] T028 [US2] Add the reload edge cases to
      `crates/iris-agentic-dev-core/tests/integration/test_live_reload_e2e.rs`: config file
      deleted mid-run falls back to the documented default rather than retaining the last file
      value, and an unparseable file keeps the last known-good gate and never widens access

### Implementation for User Story 2

- [x] T029 [US2] Add `write_tools_source`, `destructive_tools_enabled`, and
      `destructive_tools_source` to the `check_config` response body in
      `crates/iris-agentic-dev-core/src/tools/mod.rs:4715`, keeping `write_tools_enabled`'s name,
      type, and meaning so existing probes keep parsing (FR-004, contracts/check_config.md)
- [x] T030 [US2] Add the same three fields plus the missing `server_version` to `CheckConfigOk` in
      `crates/iris-agentic-dev-core/src/tools/output_schemas.rs:3289`. Every new field goes into
      both the response body and the declared schema — the `server_version` gap is the same
      declared-contract-versus-actual-payload defect this feature exists to close
- [x] T031 [US2] Make the reload path replace `ConnectionState.gates` wholesale with a freshly
      computed `GateResolution` on every config load, in
      `crates/iris-agentic-dev-core/src/tools/mod.rs`, so the value follows the file in both
      directions for the lifetime of the process (FR-002)
- [x] T032 [US2] Update the gate reader at
      `crates/iris-agentic-dev-core/src/tools/mod.rs:2669` to read `gates` rather than the
      removed cached bool, and delete any remaining reader of `IRIS_WRITE_TOOLS_ENABLED` outside
      the `OperatorEnvGates` snapshot — verify with
      `git grep -n IRIS_WRITE_TOOLS_ENABLED crates/*/src`

**Checkpoint**: T025–T028 pass. SC-002 and SC-003 hold.

---

## Phase 5: User Story 3 - A new write-capable tool cannot ship ungated (Priority: P2)

**Goal**: Classification completeness is a test, not a convention.

**Independent Test**: Add a fake write-capable tool without declaring it; the suite goes red
naming that tool.

### Tests for User Story 3

- [x] T033 [US3] Assert forward completeness in
      `crates/iris-agentic-dev-core/tests/unit/test_gate_classification.rs`: every name from
      `IrisTools::registered_tool_names()` across Baseline, Nostub, and Merged appears in
      `CLASSIFICATION`, with the failure message naming the missing tools (FR-007, US3 scenario 1)
- [x] T034 [US3] Assert reverse completeness in
      `crates/iris-agentic-dev-core/tests/unit/test_gate_classification.rs`: every
      `CLASSIFICATION` entry names a tool the router actually registered, so a rename cannot leave
      a stale entry that silently stops matching
- [x] T035 [US3] Assert the annotation cross-check in
      `crates/iris-agentic-dev-core/tests/unit/test_gate_classification.rs`: `read_only_hint =
true` implies `ReadOnly` and `destructive_hint = true` implies `Destructive`. Two
      independent declarations, so a contributor has to lie twice. Do not derive one from the
      other — `c641d79` (#94) had to strip `read_only_hint` from six mutating tools that shipped
      advertising themselves read-only (US3 scenario 3, plan.md Complexity Tracking)
- [x] T036 [US3] Add the table-driven refusal test at the binary layer in
      `crates/iris-agentic-dev-bin/tests/integration/test_mcp_binary_config.rs`: with the gate off,
      iterate `CLASSIFICATION`'s write-capable entries over stdio `tools/call` and assert each
      returns the write-gate code. No live IRIS, so it runs on every CI job, and there is no
      per-tool test to forget (FR-026, US3 scenario 2)

### Implementation for User Story 3

- [x] T037 [US3] Demonstrate SC-006: add a throwaway write-capable tool to the router in
      `crates/iris-agentic-dev-core/src/tools/mod.rs` without a `CLASSIFICATION` entry, run
      `cargo test --features testing gate_classification`, record the failure message in
      `specs/085-write-gate-integrity/quickstart.md` §6, then revert the tool. Repeat with a tool
      classified `ReadOnly` while carrying `destructive_hint = true` to prove the cross-check
      fires

**Checkpoint**: T033–T036 pass. SC-006 demonstrated, not assumed.

---

## Phase 6: User Story 4 - Invalid gate configuration fails closed (Priority: P2)

**Goal**: The contradictory config exits non-zero instead of starting with writes enabled.

**Independent Test**: Start the binary with the contradictory config; assert a non-zero exit and
that no session is established.

### Tests for User Story 4

- [x] T038 [US4] Add `validate_gate_config` unit tests to
      `crates/iris-agentic-dev-core/tests/unit/test_gate_resolution.rs`: destructive on with
      writes off returns `Err(DestructiveRequiresWrites)`, and every other combination returns
      `Ok`. Configs parsed from TOML strings, per FR-022
- [x] T039 [US4] Replace the log-only assertion in
      `crates/iris-agentic-dev-bin/tests/integration/test_mcp_binary_config.rs:252`
      (`config_file_destructive_requires_write_logs_error`) with an assertion on the **resulting
      behavior**: exit code 2, `DESTRUCTIVE_REQUIRES_WRITES` on stderr, and no successful
      `initialize` handshake. The current test passes while the server does the opposite of what
      it logged (FR-027, SC-004)
- [x] T040 [US4] Script the reporter's three published reproductions as one binary-layer test in
      `crates/iris-agentic-dev-bin/tests/integration/test_reporter_repro.rs`, registering the
      `[[test]]` target in `crates/iris-agentic-dev-bin/Cargo.toml`: stale reporting after a config
      edit, a write that lands with the gate declared off, and the contradictory config starting
      with writes enabled. Each assertion is the behavior the reporter measured, not the returned
      error code. Mark `#[ignore]` and document the required env vars, since the write-lands case
      needs live `iris-dev-iris`. This is what makes SC-008 a test rather than a manual
      walk-through, which Constitution Principle IV requires of every success criterion
      (SC-008, FR-027)

### Implementation for User Story 4

- [x] T041 [US4] Implement the pure `validate_gate_config(cfg) -> Result<(), GateConfigError>` in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs`, with the single
      `DestructiveRequiresWrites` variant (data-model.md, Startup validation)
- [x] T042 [US4] Call `validate_gate_config` from
      `crates/iris-agentic-dev-bin/src/cmd/mcp.rs` before `discover_iris`, at both config entry
      points (`apply_explicit_config_file` and `apply_workspace_config_with_path`, `:136-150`),
      logging the code and `std::process::exit(2)` on `Err` — distinct from the existing `exit(1)`
      for an invalid transport at `:257` (FR-006)
- [x] T043 [US4] Delete the fail-open early return at
      `crates/iris-agentic-dev-core/src/iris/workspace_config.rs:695-703`. That `return None` is
      the defect: it skips the export below it and drops the caller into the permissive namespace
      inference, which is how the config documented as refused starts with writes enabled
      (FR-005)

**Checkpoint**: T038–T040 pass. SC-004 holds. SC-008 now has a test, not a walk-through.

---

## Phase 7: User Story 7 - Irreversible operations need a second key (Priority: P2)

**Goal**: The destructive tier is a real second gate, off until declared, meaningful only when
writes are on.

**Independent Test**: With writes on and the tier absent or off, call each of the seven
destructive items; each is refused and the data it would have destroyed still exists. Declare the
key on and the same calls succeed.

### Tests for User Story 7

- [x] T044 [US7] Add the destructive-tier refusal matrix to
      `crates/iris-agentic-dev-core/tests/integration/test_gate_enforcement_live.rs`: writes on,
      tier undeclared, all seven items refused with `DESTRUCTIVE_TOOLS_DISABLED`, and the target
      still present afterward — global for `global_kill`, entry for `iris_lookup_manage` delete,
      namespace for `iris_namespace_create` (FR-025, SC-009)
- [x] T045 [US7] Add the local-state cases to
      `crates/iris-agentic-dev-core/tests/integration/test_gate_enforcement_live.rs`:
      `iris_remove_server` is refused and the saved server is still listed, and
      `skill(action="forget")` is refused and the skill is still installed. These have no IRIS
      side effect to observe, so the surviving local artifact is the assertion (spec.md Edge Cases)
- [x] T046 [US7] Add the ordering and positive cases to
      `crates/iris-agentic-dev-core/tests/integration/test_gate_enforcement_live.rs`: with writes
      **off**, a destructive tool is refused with `WRITE_TOOLS_DISABLED` and not
      `DESTRUCTIVE_TOOLS_DISABLED`, because Destructive is a subset of Write; and with both
      declared on, the same calls proceed (US7 scenarios 2 and 3)

### Implementation for User Story 7

- [x] T047 [US7] Mark the destructive tier in `CLASSIFICATION` in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs`: `global_kill`, `iris_admin`,
      `iris_credential_manage`, `iris_lookup_manage` delete actions, `iris_namespace_create`,
      `iris_remove_server`, and `skill` with `action = "forget"` — the last is an action, not a
      tool. `iris_production_item` is the eighth router-stripped tool and classifies as `Write`,
      not `Destructive` (FR-018, data-model.md §3)
- [x] T048 [US7] Extend the `call_tool` check from T019 to evaluate the destructive tier at the
      same dispatch point in `crates/iris-agentic-dev-core/src/tools/mod.rs:8213`, emitting
      `DESTRUCTIVE_TOOLS_DISABLED` only when writes are on and the tier is off (FR-008, FR-018)
- [x] T049 [US7] Leave room for the per-server predicate FR-017 defers: structure the check in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs` so a server-scoped test can be added
      at the same call site without reopening this work, and note that seam in a comment citing
      spec 074

**Checkpoint**: T044–T046 pass. SC-009 holds. All P1 and P2 stories are complete.

---

## Phase 8: User Story 5 - Documented controls exist (Priority: P3)

**Goal**: Nothing in the shipped docs or a bundled skill names a control the binary does not have.

**Independent Test**: `cargo test docs_contract` is green, and it fails when a phantom identifier
is reintroduced.

### Tests for User Story 5

- [x] T050 [US5] Implement the error-code extractor in
      `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs`: pull every
      `SCREAMING_SNAKE_CASE` identifier out of `docs/tools.md`, `docs/connecting.md`, and
      `skills/**/SKILL.md`, and assert each is emitted somewhere under `crates/*/src`. Catches
      `DESTRUCTIVE_TOOLS_DISABLED`, `WRITE_SERVER_NOT_ALLOWED`, `WS_SESSION_NOT_FOUND`, and
      `WS_TERMINAL_NOT_SUPPORTED` (FR-015, FR-016a)
- [x] T051 [US5] Implement the config-key extractor in
      `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs`: pull keys from
      level-3 headings naming a snake_case key, and from toml fence lines, then assert each key
      both deserializes into the config structure **and has a reader that acts on it**. A mention
      is not enough — `IRIS_DESTRUCTIVE_TOOLS_ENABLED` is present in the sources today as a setter
      with no getter, so a presence grep is green on the exact defect this spec exists to fix
      (FR-014, research.md D5)
- [x] T052 [US5] Implement the environment-variable extractor in
      `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs`: match
      `\b(IRIS|IAD)_[A-Z0-9_]+\b` and assert each is read under `crates/*/src`. Catches
      `IRIS_WRITE_ALLOWED_SERVERS` and `IRIS_WS_TIMEOUT_SECS` (FR-015)
- [x] T053 [US5] Implement the tool-parameter extractor in
      `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs`: for each parameter row
      documented under a level-3 heading naming a tool, assert that tool's handler reads that key.
      Catches `max_chars` on `stream_inspect`, whose handler
      (`crates/iris-agentic-dev-core/src/tools/mod.rs:7977-7996`) reads only `oid`, `namespace`,
      and `server` (FR-016b)
- [x] T054 [US5] Implement the count extractor in
      `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs`: read the number out of the
      `read_only_hint` sentence at `docs/tools.md:1468` and compare it to the actual count from
      `tool_router.list_all()`. Every identifier in that sentence is real and the sentence is still
      wrong — it says 57, and `c641d79` cut it to 51 (FR-016)
- [x] T055 [US5] Implement `PLANNED(spec-NNN)` exemption handling in
      `crates/iris-agentic-dev-core/tests/unit/test_docs_contract.rs`: an identifier on a line
      carrying that marker is skipped, and the marker must cite a spec directory that exists under
      `specs/`. Exemptions live inline in the documentation so the reader of the docs sees them,
      not buried in a test file

### Documentation corrections for User Story 5

- [x] T056 [US5] Delete the per-server allowlist from `docs/tools.md:1512-1543`: the
      `write_allowed_servers` key, the `IRIS_WRITE_ALLOWED_SERVERS` variable, the
      `WRITE_SERVER_NOT_ALLOWED` code, and check-order steps 2 and 3, which describe checks that
      do not exist. Spec 074 stays open as the design of record (FR-017, SC-005)
- [x] T057 [US5] Correct the destructive-gate section of `docs/tools.md:1490-1511` to describe
      what T047–T048 actually implement, including the prose at `:1503` that promises the server
      "refuses to start" — now true, and true only because of Phase 6. This task also discharges
      FR-016's check-order half, which no extractor can reach: the six numbered steps at
      `docs/tools.md:1533-1540` contain no identifiers, so steps 2 and 3 being fictional is caught
      by review here and by nothing mechanical (FR-016, research.md D5)
- [x] T058 [US5] Fix the stale count at `docs/tools.md:1468` to the value T054 computes
- [x] T059 [US5] Correct the four identifiers inherited from spec 072 in `docs/tools.md`:
      `WS_SESSION_NOT_FOUND` → `SESSION_WS_DISCONNECTED`, `WS_TERMINAL_NOT_SUPPORTED` →
      `SESSION_WS_UNAVAILABLE` (both per
      `crates/iris-agentic-dev-core/src/iris/ws_session.rs:22-23`), remove `IRIS_WS_TIMEOUT_SECS`
      (the timeout is the hardcoded `WS_FRAME_TIMEOUT_SECS = 30` at `:27`), and remove `max_chars`
      from `stream_inspect`. Correcting the documentation is in scope; implementing the described
      behavior instead is explicitly not (FR-016c, spec.md Out of Scope)
- [x] T060 [US5] Remove the allowlist key and variable from `docs/connecting.md`, which is what
      recommended them to the reporter in the first place
- [x] T061 [US5] Remove the phantom claims from `skills/skills/iris-agentic-dev/SKILL.md:176-240`:
      the invented third tier that appears in no spec, and the two error codes it names. Bundled
      skills are a shipped surface (FR-016a)
- [x] T062 [US5] Wire `test_docs_contract` into the `doc-lint` job at
      `.github/workflows/ci.yml:439` so it runs on the job that already owns documentation, in
      addition to running as a plain `cargo test`. Note in the step comment that `doc-lint` today
      never opens `docs/tools.md`
- [x] T063 [US5] Run `markdownlint-cli2 --fix` then `prettier --write` on every `.md` touched by
      T056–T061 and confirm zero remaining errors

**Checkpoint**: `cargo test docs_contract` green. SC-005 holds — eight identifiers across two
surfaces now resolve or are marked planned.

**Scope note (T059)**: the spec predicted four bad identifiers. The parameter extractor found
**18**, because the first version of the check skipped every `AnyParams` tool — those advertise an
empty `properties` object, so the schema cannot answer "is this parameter passable" and the
evidence has to come from `p.get("name")` in the handler body. That is how `max_chars` slipped
through in the first place. Corrected beyond the four the task names: `iris_production(full_status
→ full)`, `iris_interop_query(item_name → component, class_name → message_class, limit 10/20 →
50)`, `iris_message_body(acknowledge_phi → acknowledgePhi)`, `iris_database_stats(db_path → db)`,
`query_audit_log(username → user, limit 50 → 100)`, `capability_matrix(namespaces` deleted — the
tool returns one user's roles, not a matrix`)`, `hl7_schema_inspect(version → schema)`,
`mermaid_class(classes → class`, plus the undocumented `depth`)`,
`iris_containers(workspace_root/namespace/username/password/edition`all deleted — the handler
reads only`action`and`name``), `iris_ws_exec(session_token → session, timeout_secs` deleted`),
`iris_ws_close(session_token → session)`. Two doc defaults were also wrong in a way no extractor
checks (`iris_interop_query.limit`,`query_audit_log.limit`); both fixed.

---

## Phase 9: User Story 6 - Official releases report an honest version (Priority: P3)

**Goal**: Lockfile drift fails the build instead of silently dirtying the version string.

**Independent Test**: Build from a clean checkout of a tag; the reported version equals the tag.

### Tests for User Story 6

- [x] T064 [US6] Write `crates/iris-agentic-dev-core/tests/unit/test_lockfile_sync.rs`: shell out
      to `cargo metadata --locked --format-version 1` and assert exit 0, surfacing stderr on
      failure so the message names the drifting package. `cargo metadata` resolves without
      compiling, so the test is fast (FR-029)

### Implementation for User Story 6

- [x] T065 [US6] Add `--locked` to every `cargo build` and `cargo test` invocation in
      `.github/workflows/ci.yml` (lines 28, 44, 53, 86, 190, 239, 366, 428). Today `--locked`
      appears exactly once in that file, at `:424` on `cargo install cargo-llvm-cov`, which
      protects the tool rather than the build (FR-020)
- [x] T066 [US6] Add `--locked` to `cargo zigbuild` in `.github/workflows/release.yml:43`, which
      is the invocation that produced `1.2.6+v1.2.6-dirty` on every published asset
- [x] T067 [US6] Verify SC-007 the only way it can be verified: `git clone --depth 1` the repo at
      the release tag into a fresh directory, run `cargo metadata --locked` then
      `cargo build --locked --release`, and assert `--version` has no `+...-dirty` suffix. The
      lockfile check must pass **before** the build, because cargo reconciles the lockfile during
      resolution and `crates/iris-agentic-dev-core/build.rs` runs after that (FR-021,
      quickstart.md §8)

**Checkpoint**: T064 passes and CI fails loudly on drift. SC-007 holds.

**T067 verification (run 2026-08-25)**, from `git clone --depth 1 --branch v1.2.6` into a fresh
directory:

1. `cargo metadata --locked` exits **101** — "the lock file needs to be updated but --locked was
   passed". The released tag genuinely does not resolve against its own lockfile.
2. Without `--locked`, resolution rewrites `Cargo.lock` (307 lines changed), so `git describe
--tags --always --dirty` returns `v1.2.6-dirty` before `build.rs` ever runs.
3. The binary built from that tree carries `SERVER_VERSION = 1.2.6+v1.2.6-dirty` — confirmed by
   `strings` on the artifact, since `--version` prints `CARGO_PKG_VERSION` and only `check_config`
   reports `SERVER_VERSION`.
4. The working tree passes `cargo metadata --locked` (T064 green), and `--locked` is now on every
   `cargo build`/`test`/`clippy`/`zigbuild` in `ci.yml` and `release.yml`, so this drift fails the
   build instead of silently renaming the release.

The last step — a clean clone of the _next_ tag reporting an unsuffixed `server_version` — can only
run once that tag exists. Steps 1-3 establish the causal chain and step 4 closes it.

---

## Phase 10: Benchmark Evidence — RELEASE GATE

**Constitution IX**: the phase carrying benchmark evidence MUST come before Polish and MUST be
labeled a release gate. It is not optional and cannot be deferred. No new tool ships here, so no
lift measurement is owed — but T023 changes which tools the server advertises when writes are off,
and that is a change to the advertised tool list.

- [x] T068 Run the GEPA benchmark harness against the advertised tool list and record the result in
      `specs/085-write-gate-integrity/lift-results.md`: no regression on any existing task, and an
      explicit note that `iris_production_item` and `iris_credential_manage` are now
      visible-but-refusing rather than absent when writes are off. If any task regresses, fix the
      tool descriptions before Polish begins (FR-022, FR-030, plan.md Constitution Check IX)

**Checkpoint**: `lift-results.md` written. The release gate is passed, not deferred.

---

## Phase 11: Polish & Cross-Cutting Concerns

- [x] T069 Walk all eight reproductions in `specs/085-write-gate-integrity/quickstart.md` against
      a locally built binary and confirm each now behaves as documented (SC-008)
- [x] T070 Verify no reader of the removed globals survives:
      `git grep -n "IRIS_WRITE_TOOLS_ENABLED\|IRIS_DESTRUCTIVE_TOOLS_ENABLED" crates/*/src`
      returns only the `OperatorEnvGates` snapshot in
      `crates/iris-agentic-dev-core/src/tools/write_gate.rs`
- [x] T071 [P] Run `cargo fmt --all -- --check` — no diff
- [x] T072 [P] Run `cargo clippy --all-targets --features testing -- -D warnings` — zero warnings
      **T069 verification (run 2026-08-25)** against `target/debug/iris-agentic-dev` and live
      `iris-dev-iris` (localhost:52780), one stdio session per section:

| §   | Result | What was measured                                                                                                                                                                                                                                                                                                              |
| --- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | pass   | `write_tools_enabled: false` / `write_tools_source: "config_file"`; `iris_ws_exec`, `iris_global` set, `iris_lookup_manage` set, `iris_execute_method`, `iris_doc` put all `WRITE_TOOLS_DISABLED`; `iris_query` read succeeds; afterwards `iris_global` get reports `defined: false` and the lookup table is `TABLE_NOT_FOUND` |
| 2   | pass   | one process, one directory: `false` → edit → `true` and the write lands (`defined: true`) → edit back → `false` and the next write is refused, with the second subscript absent                                                                                                                                                |
| 3   | pass   | row 3 measured directly (no config file, `USER`): `true`, source `inferred_namespace`, write succeeds. Rows 1, 2 and 4 are §1 and §2                                                                                                                                                                                           |
| 4   | pass   | `exit=2` and `DESTRUCTIVE_REQUIRES_WRITES` on stderr                                                                                                                                                                                                                                                                           |
| 5   | pass   | tier off: `global_kill`, `iris_lookup_manage` set/delete, `iris_remove_server`, `skill(forget)` all `DESTRUCTIVE_TOOLS_DISABLED` while `iris_doc` put succeeds; the global, the lookup entry and the saved server all survive. Tier on, same process: each proceeds                                                            |
| 6   | pass   | all three sabotages reproduce the documented message verbatim, including `Some(ReadOnly)` in 6b and both tiers reporting `iris_ws_exec` in 6c; reverted and green again                                                                                                                                                        |
| 7   | pass   | `cargo test --locked -p iris-agentic-dev-core --test test_docs_contract` — 8 passed                                                                                                                                                                                                                                            |
| 8   | pass   | fresh clone of the branch with these manifests, tagged and committed: `cargo metadata --locked` in sync, and the binary carries `SERVER_VERSION = 1.2.6` where the working-tree build of the same commit carries `1.2.6+v1.2.6-dirty`                                                                                          |

Three §5 rows were imprecise as published and are now corrected in `quickstart.md`: `set` is in the
destructive tier as well as `delete`, `global_kill` answers `CONFIRM_REQUIRED` once the tier lets it
through, and `iris_remove_server` needs a genuinely saved server for "still listed" to mean
anything. §8 gained the pre-tag form of the check, which is what was actually run.

- [x] T073 **Coverage gate** (Constitution VIII — NON-NEGOTIABLE): run the constitution's canonical
      command, not a variant of it —
      `LLVM_COV=~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-cov`
      `LLVM_PROFDATA=~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-profdata`
      `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo llvm-cov --summary-only -p iris-agentic-dev-core --features testing -- --include-ignored --test-threads=1`.
      Note the deviation from the constitution's literal text at `.specify/memory/constitution.md:202`,
      which exports `IRIS_PORT=52780`: no code reads `IRIS_PORT` (`git grep '"IRIS_PORT"' crates/*/src`
      is empty), the variable is `IRIS_WEB_PORT`, and with the wrong name
      `discovery_tests::discover_iris_returns_none_when_nothing_found` fails on any machine with a
      running IRIS container, because its skip guard tests `IRIS_WEB_PORT`. Correcting the
      constitution needs an amendment, not a task here — the deviation is recorded in plan.md
      Complexity Tracking.
      Omitting `-p iris-agentic-dev-core` measures the whole workspace, which is not the figure the
      gate is defined on. Confirm `write_gate.rs` is at or above 90% on its own, and that the crate
      TOTAL does not fall below the T003 baseline. The crate TOTAL is below the constitution's 90%
      before this feature starts, so this task cannot raise it to 90% on its own — that gap is
      recorded as an explicit exception in plan.md Complexity Tracking rather than silently
      substituted here. Add integration tests for uncovered branches before marking this complete
- [x] T074 Run the full suite the way the constitution requires:
      `cargo test --features testing -- --include-ignored --test-threads=1`. Parallel runs race on
      process environment and share one container, so `--test-threads=1` is not optional
- [x] T075 Confirm `^IADGate085` cleanup: with the gate on, kill any surviving probe globals and
      assert none remain, so the container is left as it was found

**T074** (run 2026-08-25, `--locked --no-fail-fast` added so a single failure could not hide the
rest): **4535 passed, 2 failed** across 143 test binaries against live `iris-dev-iris`.

Five pre-existing failures were found and four of them belonged to this feature. All four asserted
the behaviour 085 deliberately changed, and each is now fixed at the assertion rather than by
loosening it:

| Test                                                                 | Was                                                                    | Now                                                                                       |
| -------------------------------------------------------------------- | ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `interop_unit_tests::env_guard::write_tools_absent_when_live`        | asserted `iris_credential_manage`/`iris_production_item` vanish (T023) | renamed `write_tools_stay_registered_when_live`; asserts both stay advertised             |
| `admin_e2e_tests::test_admin_user_crud`, `test_admin_namespace_crud` | `IRIS_ADMIN_TOOLS=1` only — `iris_admin` is now destructive tier       | both gates declared in the test env; `test_admin_webapp_crud` given the same treatment    |
| `test_e2e::e2e_global_set_get_kill_roundtrip`                        | `iris_global` set/kill refused as destructive                          | new `call_tool_destructive` helper declares the tier, so the test measures the round trip |
| `test_e2e::e2e_lookup_manage_set_get_delete_roundtrip`               | `iris_lookup_manage` set/delete refused as destructive                 | same helper                                                                               |

The two remaining failures are `test_skill_manifest_sync::{install,registry}_manifest_covers_every_skill_on_disk`,
and they are not 085's: five skill directories (`iris-ai-hub`, `iris-global-archaeology`,
`iris-interop-debug`, `iris-rest-api`, `iris-sql-tuning`) exist untracked in the working tree and are
absent from `iris-agentic-dev.toml` and `skills.sh.json`. The same target passes 6/6 in the clean
pre-085 clone at `/tmp/iad-clean085`, which has no untracked skills — so this is a working-tree
condition, and adding those five to the manifests would ship unfinished skills under this feature's
diff. Flagged for Tom, deliberately not fixed here.

`manifest_tests::test_resolve_github_specific_range` failed on the earlier coverage attempt with
`GitHub API returned 403 Forbidden` and passed on this run: unauthenticated rate limiting, not a
defect.

**A fifth 085-owned failure surfaced during T073**, hidden until then by a stale binary:
`interop_e2e_tests` resolves the server it spawns from `target/llvm-cov-target/debug` **before**
`target/debug`, and that copy predated the destructive tier, so the tests in that file were
handshaking a build without gates. `cargo llvm-cov clean --workspace` deleted it, the file fell
through to the current build, and `test_lookup_crud` failed on `DESTRUCTIVE_TOOLS_DISABLED` for
`iris_lookup_manage(action="set")` — the same class as the two `test_e2e` round trips above. Its
`mcp_exchange` now declares both gates on the child. Recorded in lift-results.md under T073, because
the lesson is about the measurement, not the test: a spawn-based e2e target can pass against a binary
from an earlier commit and nothing says so.

**T075**: `^IADGate085` held three nodes, not one — the root, `sec5` and `row3`. `iris_global` kill
took `sec5` with the tier on, then `global_preview` issued a confirm token and `global_kill` removed
the global whole. `Write $Data(^IADGate085)` now answers `0`. Both destructive paths in the
quickstart got exercised getting there.

- [x] T076 Write the release-notes entry for the breaking change: the destructive tier now
      defaults to **off**, so an operator relying on one of the seven items must declare
      `destructive_tools_enabled = true`. Also note the two documented controls removed and that
      spec 074 stays open. Run `/no-ai-slop` on the entry before it ships (constitution, Release
      Notes)

**T076**: written to `docs/release-notes/v1.2.7.md` and run through `/no-ai-slop` — the parallel
reviewer returned no second opinion. Covers the five defects, the destructive-tier default flip with
all seven items listed, the three documented controls deleted (`write_allowed_servers`,
`IRIS_WRITE_ALLOWED_SERVERS`, `WRITE_SERVER_NOT_ALLOWED`) plus `IRIS_WS_TIMEOUT_SECS`, and spec 074
staying open with a marked seam.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: no dependencies
- **Phase 2 (Foundational)**: depends on Phase 1 — **blocks Phases 3, 4, 5, 6, 7**
- **Phase 3 (US1, P1)**: depends on Phase 2. The MVP
- **Phase 4 (US2, P1)**: depends on Phase 2. Independent of Phase 3, though T025's write-attempt
  assertion is only meaningful once T019 lands
- **Phase 5 (US3, P2)**: depends on Phase 2 for the table and Phase 3 for T036's refusal assertion
- **Phase 6 (US4, P2)**: depends on Phase 2 only
- **Phase 7 (US7, P2)**: depends on Phase 3 — it extends the same dispatch check
- **Phase 8 (US5, P3)**: T050–T055 depend on Phase 2 only; T057 must land **after** Phase 6 and
  Phase 7, because it documents behavior those phases create
- **Phase 9 (US6, P3)**: fully independent of every other phase. Can be done first if a quick win
  is wanted
- **Phase 10 (Benchmark, release gate)**: depends on Phase 3 (T023 changes the advertised list)
  and on Phases 4–9 being delivered. MUST precede Polish — Constitution IX
- **Phase 11 (Polish)**: depends on Phase 10 passing

### Critical Path

T001 → T002 → T004 → T007 → T008 → T010 → T012 → T016 → T019 → T048 → T073

### Parallel Opportunities

- T003 runs alongside T001–T002
- Phases 4, 6, and 9 can proceed concurrently with Phase 3 once Phase 2 is done
- T050–T055 (five extractors, one file, so sequential among themselves) can proceed concurrently
  with all enforcement work
- T071 and T072 run together
- Within Phase 2, T013 and T014 touch different files and can run in parallel after T008

### Sequencing Traps

- **Same file, so not parallel**: T004/T005/T006/T038 all edit
  `tests/unit/test_gate_resolution.rs`; T016/T017/T018/T044/T045/T046 all edit
  `tests/integration/test_gate_enforcement_live.rs`; T050–T055 all edit
  `tests/unit/test_docs_contract.rs`; T025/T036/T039 all edit
  `crates/iris-agentic-dev-bin/tests/integration/test_mcp_binary_config.rs`
- **T021–T023 must follow T019.** Deleting the handler guards before the dispatch check exists
  leaves a window where nothing enforces the gate
- **T058 must follow T054**, which computes the correct number
- **T037 must be reverted before Phase 10.** It deliberately breaks the build

---

## Implementation Strategy

### MVP (Phases 1–3)

Setup, Foundational, US1. At that checkpoint the gate declared in the config blocks every
write-capable tool and nothing reaches IRIS — the substance of #110 and of SC-001. Stop and
validate against live IRIS before going further.

### Then, in order of what the reporter sees

1. Phase 4 (US2) — the stale reporting he is looking at right now. SC-002, SC-003
2. Phase 6 (US4) — smallest change, worst failure direction. SC-004
3. Phase 7 (US7) — the destructive tier, the documented key that has never had a reader. SC-009
4. Phase 5 (US3) — converts the sweep into a property. SC-006
5. Phase 8 (US5) — closes the surface the whole issue came from. SC-005
6. Phase 9 (US6) — the honest version string. SC-007

Then Phase 10, the benchmark release gate, and only then Polish. Constitution IX puts the gate
before Polish precisely so it cannot become the thing that gets dropped when the branch is nearly
done.

### Notes

- Tests go first in every phase and must be observed failing. Every previous round of this issue
  shipped with a green suite, which is why FR-022 through FR-030 are requirements rather than
  advice
- Nothing here mocks IRIS. Live `iris-dev-iris` only (Constitution, Testing Philosophy)
- Enforcement tests assert the **absent side effect**, not the returned error code. That
  distinction is the whole difference between what the current tests check and what the reporter
  measured
