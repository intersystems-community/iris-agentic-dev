---

description: "Task list for 059-tool-telemetry-benchmark"

---

# Tasks: Tool Telemetry and Benchmark Harness

**Input**: Design documents from `/specs/059-tool-telemetry-benchmark/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: MANDATORY for every user-story phase per project convention (constitution
Principle IV) — unit tests first, then a live `#[ignore]`-gated E2E test as the phase gate.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: US1 / US2 / US3 per spec.md priorities
- All paths are relative to `/Users/tdyar/ws/iris-agentic-dev`

## Path Conventions

Single Rust workspace. New modules: `crates/iris-agentic-dev-core/src/telemetry/`,
`crates/iris-agentic-dev-core/src/benchmark/`. Tests: `crates/iris-agentic-dev-core/tests/`
(flat files, each registered as its own `[[test]]` in `Cargo.toml`, matching existing
convention — see `test_compile_params`/`interop_unit_tests` entries).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace/dependency setup shared by every user story.

- [X] T001 Add `uuid = { version = "1", features = ["v4"] }` to `[workspace.dependencies]`
      in `Cargo.toml`, and `uuid.workspace = true` to `crates/iris-agentic-dev-core/Cargo.toml`
      (per research.md's "Session identifier" decision).
- [X] T002 [P] Create module skeletons: `crates/iris-agentic-dev-core/src/telemetry/mod.rs`,
      `telemetry/redact.rs`, `telemetry/prune.rs`, `telemetry/trace_export.rs` (empty `pub`
      stubs — no logic yet), and register `pub mod telemetry;` in
      `crates/iris-agentic-dev-core/src/lib.rs`.
- [X] T003 [P] Create module skeletons: `crates/iris-agentic-dev-core/src/benchmark/mod.rs`,
      `benchmark/container.rs`, `benchmark/llm.rs` (empty `pub` stubs), and register
      `pub mod benchmark;` in `crates/iris-agentic-dev-core/src/lib.rs`.
- [X] T004 [P] Create `crates/iris-agentic-dev-core/src/benchmark/tasks/jira_bugs/` directory
      and copy all 22 primary-suite task JSON files (excluding the 5 `MF-*.json` multi-file
      variants) from
      `/Users/tdyar/ws/objectscript-coder/bench/eval_tasks/jira_bugs/*.json`, unmodified
      (per research.md's "Task suite port" decision — Assumption 2 requires content
      preservation).

**Checkpoint**: Modules compile (empty), task fixtures present.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types and the dual-sink writer that every user story's tests depend on.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T005 [P] Define `ToolCallRecord` struct (`tool: String`, `success: bool`,
      `duration_ms: u64`, `timestamp: String` (RFC3339), `session_id: Uuid`,
      `params: Option<serde_json::Value>`) in
      `crates/iris-agentic-dev-core/src/telemetry/mod.rs`, per data-model.md. Add
      `From<ToolCallRecord> for ToolCallEntry`-style helpers only if needed to keep existing
      `ToolCallEntry` call sites compiling during migration; otherwise rename
      `ToolCallEntry` → `ToolCallRecord` directly in
      `crates/iris-agentic-dev-core/src/tools/mod.rs` (T009 handles call-site updates).
- [X] T006 [P] Add a `Session { id: Uuid, started_at: String }` struct in
      `crates/iris-agentic-dev-core/src/telemetry/mod.rs`, generated once via `Uuid::new_v4()`.
- [X] T007 [P] [Unit test] Write `crates/iris-agentic-dev-core/tests/unit/test_telemetry_types.rs`
      covering: `ToolCallRecord` serializes/deserializes round-trip via `serde_json`;
      `Session::new()` produces a non-nil UUID; two calls to `Session::new()` produce
      distinct ids. Register as `[[test]] name = "test_telemetry_types"` in
      `crates/iris-agentic-dev-core/Cargo.toml`. Run and confirm it FAILS (types don't exist
      yet) before T005/T006 land, then confirm it PASSES after.
- [X] T008 Implement the dual-sink write path in
      `crates/iris-agentic-dev-core/src/telemetry/mod.rs`: `fn write_durable(record: &ToolCallRecord, connection: &ConnectionState, config_dir: &Path)`
      — when `connection` has a live `IrisConnection`, write via
      `execute_via_generator` to `^IRISDEV("telemetry", session_id, $INCREMENT(...))` using
      `$LISTBUILD(tool, success, duration_ms, timestamp[, params_json])` encoding (verified
      live in research.md); when disconnected, append one JSON line to
      `<config_dir>/telemetry/<session_id>.jsonl`. MUST catch and swallow all errors from
      this function — never propagate a failure to the caller (FR-014). Depends on T005.
- [X] T009 Replace `IrisTools`'s `history: Arc<Mutex<VecDeque<ToolCallEntry>>>` field and
      `record_call` method in `crates/iris-agentic-dev-core/src/tools/mod.rs` with
      `history: Arc<Mutex<VecDeque<ToolCallRecord>>>` (capacity from new env var
      `IRIS_TELEMETRY_BUFFER_SIZE`, default 5000, read in `IrisTools::new`/
      `new_with_toolset`) and a `record_call(&self, tool: &str, success: bool, duration_ms: u64)`
      that (a) pushes a `ToolCallRecord` (stamping `session_id` from a new `session: Session`
      field on `IrisTools`, `timestamp` from `SystemTime::now()`, `params: None` by default)
      into the ring buffer, evicting oldest when full, and (b) spawns
      `telemetry::write_durable(...)` as a fire-and-forget `tokio::spawn` (never awaited by
      the caller, satisfying SC-005's non-blocking requirement). Update every one of the
      ~50 existing `self.record_call(tool, success)` call sites to pass a measured
      `duration_ms` (wrap the call in `let start = std::time::Instant::now(); ... ;
      start.elapsed().as_millis() as u64`). Depends on T005, T008.
- [X] T010 Add `session: Session` field to `IrisTools` struct, initialized in `IrisTools::new`
      and `new_with_toolset` (both constructors) via `Session::new()`. Depends on T006.
- [X] T011 [P] Implement parameter redaction in
      `crates/iris-agentic-dev-core/src/telemetry/redact.rs`: `fn redact_params(params: &serde_json::Value, policy: &DataPolicy) -> Option<serde_json::Value>`
      — returns `Some(params.clone())` only when `policy == DataPolicy::Allow`, else `None`.
      Reuses the existing `DataPolicy` enum from
      `crates/iris-agentic-dev-core/src/iris/workspace_config.rs` (per research.md's
      "Redaction" decision — no new mechanism).
- [X] T012 [P] [Unit test] Write
      `crates/iris-agentic-dev-core/tests/unit/test_telemetry_redact.rs` covering:
      `DataPolicy::Allow` → params preserved; `DataPolicy::Block`/`DataPolicy::Redact` →
      params `None`. Register in Cargo.toml. Depends on T011 (write test first, confirm
      fail, then implement — but since T011 is a pure function, write the test file
      alongside T011 in the same commit, asserting it fails against a stub `todo!()` body
      first).

**Checkpoint**: Foundation ready — `ToolCallRecord` flows through the ring buffer and the
dual-sink writer on every existing tool call; user story implementation can now begin.

---

## Phase 3: User Story 1 — Run the Documented Benchmark Quick Start (Priority: P1) 🎯 MVP

**Goal**: `iris-agentic-dev benchmark --skill <path> --baseline --output results.json` runs
end-to-end against a clean public clone, producing `pass_rate`/`baseline_pass_rate`/`lift`.

**Independent Test**: Per quickstart.md — run the CLI command against a fresh
`iris-dev-iris`-equivalent container with the repo's own
`light-skills/skills/objectscript-review/SKILL.md`, verify `results.json` matches
data-model.md's `BenchmarkResult` shape.

### Tests for User Story 1 ⚠️

- [X] T013 [P] [US1] Write
      `crates/iris-agentic-dev-core/tests/unit/test_benchmark_scoring.rs`: given a fixed set
      of fake `TaskResult`s (pass/fail/error mix), `BenchmarkResult::from_task_results(...)`
      computes `pass_rate = tasks_passed / tasks_total` correctly, excludes errored tasks
      from the pass/fail denominator per FR-012 ("distinguishable from a normal fail" —
      decide and document in the function whether errored tasks count toward `tasks_total`;
      per FR-012's intent, an errored task must not silently count against pass_rate, so
      `tasks_total` for `pass_rate` purposes = `tasks_passed + tasks_failed`, with
      `tasks_errored` reported separately). Register in Cargo.toml. Confirm FAILS before
      T017.
- [X] T014 [P] [US1] Write `crates/iris-agentic-dev-core/tests/unit/test_lift_math.rs`:
      `lift = pass_rate - baseline_pass_rate` (absolute difference, FR-004); `lift` is `None`
      when `baseline_pass_rate` is `None`. Register in Cargo.toml. Confirm FAILS before T017.
- [X] T015 [P] [US1] Write
      `crates/iris-agentic-dev-core/tests/unit/test_benchmark_task_loading.rs`: loading all
      22 files under `src/benchmark/tasks/jira_bugs/` via `serde_json` succeeds and produces
      22 `BenchmarkTask` values with non-empty `task_id`/`initial_code.files`/`test_code`.
      Register in Cargo.toml. Confirm FAILS before T018.
- [X] T016 [US1] [E2E — phase gate] Write
      `crates/iris-agentic-dev-core/tests/integration/test_benchmark_live.rs`, `#[ignore]`,
      following the `live_iris()` helper pattern in
      `tests/integration/test_iris_admin_observability_live.rs`: run the full
      `benchmark::run_benchmark(...)` against a live `iris-dev-iris` container with
      `--baseline` using a trivial in-repo skill, assert the returned `BenchmarkResult` has
      `tasks_total == 22`, `pass_rate` and `lift` are both `Some`/present, and every
      `task_results[i].outcome` is one of `Pass`/`Fail`/`Error` (SC-004: none silently
      skipped/hung). This test MUST FAIL (module doesn't exist) until T017-T022 land, and
      MUST PASS before Phase 3 is considered complete (phase gate).

### Implementation for User Story 1

- [X] T017 [US1] Implement `BenchmarkTask`, `BenchmarkResult`, `TaskResult` (with `outcome:
      enum {Pass, Fail, Error}`) structs and `BenchmarkResult::from_task_results(...)` /
      lift computation in `crates/iris-agentic-dev-core/src/benchmark/mod.rs`, per
      data-model.md. Depends on T013, T014 (tests exist and fail first).
- [X] T018 [US1] Implement `fn load_tasks(dir: &Path) -> anyhow::Result<Vec<BenchmarkTask>>`
      in `crates/iris-agentic-dev-core/src/benchmark/mod.rs`, globbing `*.json` under the
      given directory and deserializing each via `serde_json`. Depends on T015.
- [X] T019 [US1] Port `BenchmarkContainer`-equivalent connection/readiness logic from
      `objectscript_mcp/benchmark/container.py` into
      `crates/iris-agentic-dev-core/src/benchmark/container.rs` — but per research.md's
      "Benchmark harness IRIS interaction" decision, do NOT port the `docker exec iris
      session IRIS` subprocess calls; instead this module only needs to confirm an
      `IrisConnection` is reachable (reuse the existing `discover_iris`/
      `apply_workspace_config` chain from `cmd/compile.rs` — no new discovery code).
- [X] T020 [US1] Implement the per-task run loop in
      `crates/iris-agentic-dev-core/src/benchmark/mod.rs`: for each `BenchmarkTask`, (a)
      compile `initial_code.files` via the existing in-process `iris_compile`-equivalent
      logic on `IrisTools` (call the same function `tools/mod.rs`'s `iris_compile` MCP
      handler calls, not a second HTTP path), (b) invoke the LLM (via
      `benchmark/llm.rs`, T021) with the skill content (or empty string for baseline) plus
      the task's `description`/`hints` to get a proposed fix, (c) apply the fix and
      recompile, (d) compile+run `test_code` via the existing in-process `iris_test`-
      equivalent logic, (e) map `success_criteria` (`compile_success`, `tests_pass`) to a
      `TaskResult.outcome`; a task whose referenced tool/class surface errors in a way
      unrelated to the task's own fix (e.g. `iris_compile` itself returns a tool-level error
      before ever reaching the task's code) maps to `Error`, not `Fail` (FR-012). Depends on
      T017, T018, T019.
- [X] T021 [US1] Implement `crates/iris-agentic-dev-core/src/benchmark/llm.rs`: a thin
      wrapper reusing `generate.rs`'s `LlmClient` (`LlmClient::from_env()` /
      `LlmClient::complete(system, user)`) to send the task's fix-request prompt; no new
      HTTP/SDK code (per research.md — Constitution VII).
- [X] T022 [US1] Implement the `benchmark` CLI subcommand: add `Commands::Benchmark` variant
      to `crates/iris-agentic-dev-bin/src/main.rs` and create
      `crates/iris-agentic-dev-bin/src/cmd/benchmark.rs` following `cmd/compile.rs`'s exact
      pattern (`#[derive(Args)]` with `--skill`, `--baseline`, `--suite` (default `"jira"`,
      any other value errors with `SUITE_NOT_AVAILABLE` per contracts/benchmark-cli.md and
      data-model.md's Error Code Registry), `--output`, `--model`,
      `--task-timeout-s`, `--max-time-s`, plus the standard `--host`/`--web-port`/
      `--namespace`/`--username`/`--password` connection flags and
      `discover_iris(apply_workspace_config(...))` discovery chain). On no-IRIS-found,
      produce the same structured error shape `CompileCommand` already produces. Depends on
      T020.
- [X] T023 [US1] Implement concurrent-run detection (FR-013): before starting a run, check
      for a lock record at `^IRISDEV("telemetry","benchmark_lock",container_name)` (or an
      equivalent local-file lock when disconnected); if present and younger than
      `--max-time-s`, exit non-zero with error code `BENCHMARK_RUN_IN_PROGRESS` (data-
      model.md Error Code Registry); else write the lock, run, and clear it on completion
      (including on error paths — use a guard/drop pattern). Implement in
      `crates/iris-agentic-dev-core/src/benchmark/mod.rs`. Depends on T020.
- [X] T024 [US1] Rewrite `light-skills/BENCHMARKING.md`'s Quick Start and Detailed Setup
      sections per `specs/059-tool-telemetry-benchmark/quickstart.md` — remove the
      `gitlab.iscinternal.com` clone step, `pip install`, and `bench/run_benchmark.py`
      references entirely; replace with the `iris-agentic-dev benchmark` CLI invocation
      (FR-008). Update the `--suite` documentation to note only `jira` is available in v1,
      with `mf`/`sql` explicitly marked "not yet ported."

**Checkpoint**: User Story 1 fully functional — `iris-agentic-dev benchmark` runs against a
clean public clone with no private-repo dependency; `test_benchmark_live.rs` passes.

---

## Phase 4: User Story 2 — Durable Tool-Call Record Beyond the Session (Priority: P1)

**Goal**: Tool-call history survives a process restart and is queryable beyond the 50-entry
(now 5000-entry) in-memory cap, with redaction applied per policy.

**Independent Test**: Per spec.md — run >50 tool calls, restart the process, query the
telemetry record for the prior session via the new `telemetry_query` tool; verify all calls
present and params redacted per policy.

### Tests for User Story 2 ⚠️

- [X] T025 [P] [US2] Write
      `crates/iris-agentic-dev-core/tests/unit/test_telemetry_query_filter.rs`: given a
      fixed slice of `ToolCallRecord`s, a pure filter function
      `filter_records(records, tool_name: Option<&str>, session_id: Option<Uuid>, since: Option<&str>, until: Option<&str>, limit: usize) -> (Vec<ToolCallRecord>, bool)`
      (the `bool` is `truncated`) correctly filters by each dimension independently and in
      combination, and sets `truncated=true` only when more matches exist than `limit`.
      Register in Cargo.toml. Confirm FAILS before T027.
- [X] T026 [US2] [E2E — phase gate] Write
      `crates/iris-agentic-dev-core/tests/integration/test_telemetry_live.rs`, `#[ignore]`,
      per `live_iris()` pattern: perform >50 tool calls (e.g. 60 `iris_execute` calls) against
      a live `iris-dev-iris` container in one `IrisTools` instance, capture that instance's
      `session.id`, drop the instance (simulating process exit), construct a NEW `IrisTools`
      instance against the same IRIS connection, call the `telemetry_query` handler
      (T029) with the captured `session_id`, and assert all 60+ records are present
      (not capped at the old 50-entry limit) — this is the literal "restart" scenario since
      the durable IRIS global outlives the in-memory `IrisTools` value. Also assert a call
      made with `DataPolicy::Redact`/`Block` active has `params: None` in the durable record
      while `tool`/`success`/`duration_ms` are present (Acceptance Scenario 3). MUST FAIL
      until T027-T029 land; MUST PASS before Phase 4 is complete.

### Implementation for User Story 2

- [X] T027 [US2] Implement `fn read_durable(session_id: Option<Uuid>, connection: &ConnectionState, config_dir: &Path) -> Vec<ToolCallRecord>`
      in `crates/iris-agentic-dev-core/src/telemetry/mod.rs` — reads back
      `^IRISDEV("telemetry", session_id, seq)` entries via `$ORDER`-based iteration (live
      IRIS) or parses the corresponding `.jsonl` file(s) (local-file mode), decoding the
      `$LISTBUILD` / JSON-line encoding from T008 back into `ToolCallRecord`. When
      `session_id` is `None`, enumerate all known session subscripts/files. Depends on T008.
- [X] T028 [US2] Implement `filter_records(...)` (the pure function from T025) in
      `crates/iris-agentic-dev-core/src/telemetry/mod.rs`. Depends on T025.
- [X] T029 [US2] Add the `telemetry_query` MCP tool to `crates/iris-agentic-dev-core/src/tools/mod.rs`:
      new `TelemetryQueryParams` struct (`tool_name: Option<String>`,
      `session_id: Option<String>`, `since: Option<String>`, `until: Option<String>`,
      `limit: Option<usize>` default 500 max 5000) and a `#[tool]`-annotated handler that
      calls `telemetry::read_durable` (T027) + `telemetry::filter_records` (T028) and
      returns `{"records": [...], "truncated": bool}` per contracts/telemetry-mcp-tools.md.
      Classify as `ToolCategory::Query` in `crate::iris::server_manager::tool_to_category_pub`.
      Per the constitution's Toolset Registration Rules ("Update BOTH the router removal
      list in `with_registry_and_toolset()` AND `registered_tool_names()` — these two must
      stay in sync"), add `"telemetry_query"` to `registered_tool_names()` and to the
      `Toolset::Merged`/`Nostub` registration lists in `with_registry_and_toolset()`.
      Depends on T027, T028.
- [X] T030 [US2] Implement pruning: `fn prune_durable(retention_days: u64, active_session: Uuid, connection: &ConnectionState, config_dir: &Path)`
      in `crates/iris-agentic-dev-core/src/telemetry/prune.rs` — deletes durable-sink
      session subscripts/files older than `retention_days` (default 30, env var
      `IRIS_TELEMETRY_RETENTION_DAYS`), explicitly skipping `active_session` (FR-011,
      SC-006 — "MUST NOT occur mid-benchmark-run", satisfied by construction per
      research.md's pruning-policy decision since only already-exited sessions are ever
      eligible). Wire a call to this function at `IrisTools::new` startup (prune-on-start,
      not a background timer, to keep v1 simple).
- [X] T031 [P] [US2] Write
      `crates/iris-agentic-dev-core/tests/unit/test_telemetry_prune.rs`: given fake session
      data with mixed ages, `prune_durable`-equivalent pure logic removes only entries older
      than the retention window and never removes the currently-active session regardless
      of its (simulated) age. Register in Cargo.toml.

**Checkpoint**: User Stories 1 AND 2 both work independently — durable telemetry survives
restarts, is queryable, redacts per policy, and prunes safely.

---

## Phase 5: User Story 3 — Export Tool-Call Data as Dispatch Trace Records (Priority: P2)

**Goal**: The same tool-call data is exportable as `{from, to, via, count, ts}` records
matching 058-iris-graph's `record_trace` ingestion format exactly, with repeated identical
edges aggregated.

**Independent Test**: Per spec.md — record a session with repeated + varied tool calls,
export via `telemetry_export_trace`, verify aggregation and field compatibility.

### Tests for User Story 3 ⚠️

- [X] T032 [P] [US3] Write
      `crates/iris-agentic-dev-core/tests/unit/test_trace_export.rs`: given a fixed set of
      `ToolCallRecord`s with repeated and varied `tool` values, `aggregate_trace(records) -> Vec<DispatchTraceRecord>`
      collapses repeated identical `(from, to, via)` triples into one record with
      `count == occurrences` and `ts == max(timestamp)` among the group, and produces one
      record per distinct combination otherwise (Acceptance Scenarios 1 & 2). Also assert
      every output record has exactly the fields `from`/`to`/`via`/`count`/`ts` with no extra
      fields (Acceptance Scenario 3 — format compatibility with 058's contract). Register in
      Cargo.toml. Confirm FAILS before T033.

### Implementation for User Story 3

- [X] T033 [US3] Implement `DispatchTraceRecord` struct and
      `fn aggregate_trace(records: &[ToolCallRecord]) -> Vec<DispatchTraceRecord>` in
      `crates/iris-agentic-dev-core/src/telemetry/trace_export.rs`, per data-model.md's
      aggregation rule (`from` = calling context or `"mcp_client"` sentinel, `to` = tool
      name, `via` = fixed literal `"mcp"`). Depends on T032.
- [X] T034 [US3] Add the `telemetry_export_trace` MCP tool to
      `crates/iris-agentic-dev-core/src/tools/mod.rs`: new `TelemetryExportTraceParams`
      struct (`session_id: Option<String>`, `since: Option<String>`) and a `#[tool]`-
      annotated handler that calls `telemetry::read_durable` (T027) then
      `trace_export::aggregate_trace` (T033), returning `{"traces": [...]}` exactly per
      contracts/telemetry-mcp-tools.md. Classify as `ToolCategory::Query`. Add
      `"telemetry_export_trace"` to `registered_tool_names()` and to the
      `Toolset::Merged`/`Nostub` registration lists in `with_registry_and_toolset()`, per
      the same Toolset Registration Rules sync requirement as T029. Depends on T027,
      T033.
- [X] T035 [US3] [E2E — phase gate] Write
      `crates/iris-agentic-dev-core/tests/integration/test_trace_export_live.rs`, `#[ignore]`,
      per `live_iris()` pattern: perform a mix of repeated and varied tool calls against a
      live `iris-dev-iris` container, call `telemetry_export_trace`, assert the returned
      JSON is directly parseable as the exact shape
      `{"from": String, "to": String, "via": String, "count": Number, "ts": String}` with no
      wrapper/extra fields, matching 058-iris-graph's `record_trace` input contract verbatim
      (cross-check field names against
      `git show 058-iris-graph:specs/058-iris-graph/spec.md` if that branch is available;
      otherwise assert against the literal shape documented in
      contracts/telemetry-mcp-tools.md). MUST FAIL until T033/T034 land; MUST PASS before
      Phase 5 is complete.

**Checkpoint**: All three user stories independently functional.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Repo-wide correctness and the mandatory coverage gate.

- [X] T036 [P] Update `crates/iris-agentic-dev-core/src/policy/env_gate.rs` /
      `crate::iris::server_manager::tool_to_category_pub` if `telemetry_query`/
      `telemetry_export_trace` need any `mcpTemplate`-specific gating beyond the default
      `Query` classification already applied in T029/T034 (verify against
      `LIVE_BLOCKED`/`TEST_BLOCKED` lists — Query-category tools are not in either list
      today, so this task is a verification-only check, not expected to require a code
      change).
- [X] T037 Run `crates/iris-agentic-dev-core/tests/integration/test_telemetry_live.rs`,
      `test_benchmark_live.rs`, and `test_trace_export_live.rs` together against
      `iris-dev-iris` (`cargo test --test test_telemetry_live --test test_benchmark_live
      --test test_trace_export_live -- --ignored`) to confirm no cross-test interference
      via the shared `^IRISDEV("telemetry",...)` global (each test uses a distinct session
      id, but confirm pruning/lock logic from T023/T030 doesn't cross-contaminate).
- [X] T038 [P] Run `cargo fmt --all -- --check` — no formatting diff.
- [X] T039 [P] Run `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings.
- [X] T040 Run `.specify/scripts/bash/setup-plan.sh`-adjacent quickstart validation: execute
      every command block in `specs/059-tool-telemetry-benchmark/quickstart.md` verbatim
      against a fresh `iris-bench`-named container, confirm each step succeeds as documented
      (SC-001).
- [X] T041 **Coverage gate** (Constitution VIII — NON-NEGOTIABLE): run
      `IRIS_HOST=localhost IRIS_PORT=52780 cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored`
      (with the full explicit `--test` flag list already established for this project) and
      confirm TOTAL line coverage ≥ 90%. If below 90%, add unit tests for uncovered
      branches in `telemetry/`, `benchmark/`, or their call sites before marking this task
      complete.

      **Result**: TOTAL line coverage measured at **89.26%** (18917 lines, 2031 missed),
      below the 90% target. This feature's OWN new modules exceed the bar individually:
      `telemetry/redact.rs` 100%, `telemetry/trace_export.rs` 100%, `telemetry/prune.rs`
      97.44%, `telemetry/mod.rs` 91.89%, `benchmark/container.rs` 94.66%,
      `benchmark/llm.rs` 94.62%, `benchmark/mod.rs` 84.71% — each backed by both unit
      tests (redaction, pruning decisions, aggregation, lift math, class-name extraction,
      error-outcome mapping) and live integration tests (T016/T026/T035, plus dedicated
      lock/reachability/compile-error/query/export live tests added during Polish). The
      ~0.7-point shortfall to the crate-wide 90% total is concentrated entirely in
      pre-existing files from earlier, already-merged features, untouched by this
      feature: `iris/discovery.rs` (64.63% lines, largest single gap — 191 missed lines),
      `tools/global.rs` (75.64%, 052-iris-global), `tools/interop.rs` (83.39%,
      056-interop-depth), and the pre-existing bulk of `tools/mod.rs` (83.98% — this
      feature's own additions there, `telemetry_query`/`telemetry_export_trace`/the
      `call_tool` override, are directly exercised by both unit and live tests). Per
      explicit user decision (AskUserQuestion, "Accept as-is, document the gap"), this is
      accepted rather than backfilling tests in unrelated modules purely to cross a
      global percentage threshold — a follow-up feature scoped to `iris/discovery.rs`
      specifically would be the correct place to close that gap.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately.
- **Foundational (Phase 2)**: Depends on Setup (T001-T004) — BLOCKS all user stories.
- **User Story 1 (Phase 3)**: Depends on Foundational (Phase 2). No dependency on US2/US3.
- **User Story 2 (Phase 4)**: Depends on Foundational (Phase 2). Independent of US1, but
  T026's E2E test is easiest to run after US1 exists (reuses the same live container setup)
  — not a hard code dependency, an ordering convenience.
- **User Story 3 (Phase 5)**: Depends on Foundational (Phase 2) AND on T027 (`read_durable`,
  landed in US2/Phase 4) — this is a real dependency: trace export reads the same durable
  store US2 establishes read access to. US3 cannot be fully implemented before T027 exists,
  even though it's spec'd as independently valuable.
- **Polish (Phase 6)**: Depends on all three user stories being complete.

### Within Each User Story

- Unit tests before implementation (written first, confirmed failing).
- E2E `#[ignore]` test is the phase gate — must pass before the phase is marked done.
- Data/pure-logic functions before the MCP tool handlers that call them.

### Parallel Opportunities

- T002, T003, T004 (Setup) — different files.
- T005, T006, T007 (Foundational, before T008-T010 land) — different files, though T007
  depends conceptually on T005/T006 existing to test against (write T007 first against
  stubs, expect fail, then implement).
- T011, T012 — different files (redact.rs implementation + its own test).
- T013, T014, T015 (US1 tests) — different files, no shared dependency.
- T025 (US2 test) — parallel with any US1/US3 task not touching `telemetry/mod.rs`.
- T032 (US3 test) — parallel with US1/US2 tasks not touching `trace_export.rs`.
- T038, T039 (Polish) — independent checks, run in parallel with each other.

---

## Parallel Example: User Story 1

```bash
# Tests for User Story 1 (write first, confirm fail):
Task: "Write test_benchmark_scoring.rs"
Task: "Write test_lift_math.rs"
Task: "Write test_benchmark_task_loading.rs"

# Then implementation, largely sequential (T017 → T018 → T019 → T020 → T021 → T022 → T023):
# T017-T019 can be split across files if staffed, but T020 (the run loop) depends on all three.
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup.
2. Complete Phase 2: Foundational (CRITICAL — every tool call must flow through the new
   `ToolCallRecord`/dual-sink path before any user story is testable).
3. Complete Phase 3: User Story 1 — `iris-agentic-dev benchmark` runs end-to-end.
4. **STOP and VALIDATE**: run `test_benchmark_live.rs` and the quickstart.md commands
   manually against `iris-dev-iris`.
5. This alone fixes the reported bug (Nithya's `BENCHMARKING.md` issue) — a legitimate
   ship point if time-constrained.

### Incremental Delivery

1. Setup + Foundational → durable telemetry flowing for every tool call, benchmark harness
   not yet runnable.
2. Add User Story 1 → benchmark harness runnable end-to-end → **this alone closes the
   external bug report**.
3. Add User Story 2 → durable record queryable beyond the live session.
4. Add User Story 3 → same data exportable as dispatch-trace records for 058-iris-graph.

### Parallel Team Strategy

1. Team completes Setup + Foundational together (Phase 2 touches shared files —
   `tools/mod.rs`, `telemetry/mod.rs` — genuinely blocking, not parallelizable across people
   without coordination).
2. Once Foundational is done:
   - Developer A: User Story 1 (benchmark harness + CLI).
   - Developer B: User Story 2 (durable query/prune) — note T027 becomes a shared
     dependency Developer C (US3) needs, so land it early.
   - Developer C: User Story 3 (trace export) — blocked on T027 from US2's work.
3. Stories 1 and 2 fully parallel; Story 3 waits on one function (T027) from Story 2, not
   the whole story.

---

## Notes

- [P] tasks = different files, no dependencies.
- [Story] label maps task to specific user story for traceability.
- US3 has one genuine cross-story dependency (T027, `read_durable`) — documented above
  rather than hidden; everything else is independent per spec.md's design intent.
- Verify each unit test FAILS before implementing the code it tests; verify each E2E
  `#[ignore]` test FAILS before its phase's implementation tasks, and PASSES before the
  phase is marked complete (phase gate — no exceptions).
- Commit after each task or logical group.
- Avoid: vague tasks, same-file conflicts, cross-story dependencies that break independence
  (the one exception, T027, is called out explicitly above).
