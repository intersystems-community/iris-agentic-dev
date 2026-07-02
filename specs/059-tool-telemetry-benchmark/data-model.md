# Data Model: Tool Telemetry and Benchmark Harness

## Session

One MCP server process lifetime.

| Field | Type | Notes |
|---|---|---|
| `id` | `Uuid` (v4) | Generated once in `IrisTools::new`/`new_with_toolset`; immutable for the process's life. |
| `started_at` | `DateTime<Utc>` equivalent (ISO8601 string, no `chrono` dependency added — use `std::time::SystemTime` + manual RFC3339 formatting, or format via existing `$ZDATETIME`-style helper if one already exists) | Set once at construction. |

Not a standalone struct with its own storage — `id`/`started_at` live as fields on
`IrisTools` (or a small `Session` struct embedded in it) and are stamped onto every
`ToolCallRecord` produced during that process's life.

## ToolCallRecord

A single durable telemetry entry. Supersedes `ToolCallEntry` (renamed/extended, not a
parallel type — existing `record_call` call sites populate this instead).

| Field | Type | Notes |
|---|---|---|
| `tool` | `String` | Tool name, e.g. `"iris_compile"`. |
| `success` | `bool` | Outcome. |
| `duration_ms` | `u64` | Wall-clock duration of the call. New field — `ToolCallEntry` today only has `timestamp: Instant`, no duration. |
| `timestamp` | `String` (ISO8601/RFC3339) for durable records; `std::time::Instant` retained internally for in-memory-only duration computation | The durable sink needs a wall-clock, portable timestamp; `Instant` is monotonic but not convertible to wall-clock without an anchor — capture `SystemTime::now()` alongside `Instant::now()` at call start. |
| `session_id` | `Uuid` | From the owning `Session`. |
| `params` | `Option<serde_json::Value>` | `None` when redacted per `DataPolicy`; `Some(...)` only when `DataPolicy::Allow`. |

**Validation rules**: `duration_ms` is always recorded (not subject to redaction —
redaction applies to `params` only, per FR-003). `tool`/`success`/`duration_ms`/
`timestamp`/`session_id` are always present; only `params` is conditional.

**Two representations**:
- In-memory (`VecDeque<ToolCallRecord>`, capacity `IRIS_TELEMETRY_BUFFER_SIZE`, default
  5000): authoritative for `agent_history`/`agent_stats`.
- Durable (best-effort side write): IRIS global `^IRISDEV("telemetry", session_id, seq_n)`
  as a `$LISTBUILD(tool, success, duration_ms, timestamp[, params_json])`, or a JSONL line
  under `<config_dir>/telemetry/<session_id>.jsonl` when disconnected. Same logical fields,
  different encodings — reads always resolve to one Rust `ToolCallRecord` value.

## BenchmarkTask

A single scoring unit, ported 1:1 from objectscript-coder's `bench/eval_tasks/jira_bugs/*.json`
schema (unchanged, to avoid a translation step per Assumption 2).

| Field | Type | Notes |
|---|---|---|
| `task_id` | `String` | e.g. `"jira-020"`. |
| `category` | `String` | e.g. `"jira_bugs"`. |
| `difficulty` | `String` | `"easy"`/`"medium"`/`"hard"`. |
| `description` | `String` | Human-readable summary. |
| `goal` | `String` | What the fix must accomplish. |
| `initial_code.files` | `Vec<{path: String, content: String}>` | Buggy source to seed the container with. |
| `test_code` | `{path: String, content: String}` | Test class verifying the fix. |
| `expected_behavior` | `String` | Free-text description. |
| `hints` | `Vec<String>` | Optional guidance surfaced to the model. |
| `success_criteria` | `{compile_success: bool, tests_pass: bool, max_patch_lines: u32, requires_symbol_preservation: bool}` | Pass/fail gate. |
| `metadata` | `{created_date, source, author, tags: Vec<String>, bug_pattern}` | Provenance, not scored. |

## BenchmarkResult

| Field | Type | Notes |
|---|---|---|
| `pass_rate` | `f64` | `tasks_passed / tasks_total`. |
| `baseline_pass_rate` | `Option<f64>` | `None` unless `--baseline` was requested. |
| `lift` | `Option<f64>` | `pass_rate - baseline_pass_rate` (absolute difference, FR-004); `None` when no baseline. |
| `tasks_passed` | `u32` | |
| `tasks_total` | `u32` | |
| `tasks_errored` | `u32` | Distinct from failed — FR-012's stale-task case. |
| `iris_version` | `String` | |
| `elapsed_s` | `f64` | |
| `task_results` | `Vec<TaskResult>` | Per-task detail. |

### TaskResult (nested)

| Field | Type | Notes |
|---|---|---|
| `task_id` | `String` | |
| `outcome` | `enum { Pass, Fail, Error }` | `Error` = FR-012's stale-task-surface case (e.g. tool renamed since the task was written), distinguishable from a normal `Fail`. |
| `iterations` | `u32` | |
| `elapsed_s` | `f64` | |
| `reason` | `String` | Failure/error detail. |

**State transitions**: none — a `TaskResult` is produced once per task per run, not
mutated afterward.

## DispatchTraceRecord

The exported shape, matching 058-iris-graph's `record_trace` ingestion contract exactly
(verified in research.md — no translation permitted per Assumption 4).

| Field | Type | Notes |
|---|---|---|
| `from` | `String` | Tool-level granularity for this feature: the calling tool name (or a fixed sentinel like `"mcp_client"` when there is no calling tool, i.e. a top-level invocation). |
| `to` | `String` | The called tool name. |
| `via` | `String` | Fixed literal distinguishing this feature's edges, e.g. `"mcp"`, from other `record_trace` sources (e.g. Pierre's dispatch tracer uses `"direct"`/`"dispatch"`/`"workmgr"`). |
| `count` | `u64` | Aggregated occurrence count — repeated identical `(from, to, via)` triples collapse into one record with an incremented count (FR-009), not duplicated. |
| `ts` | `String` (ISO8601) | Timestamp of the most recent occurrence in the aggregation window, not the first. |

**Aggregation rule**: group `ToolCallRecord`s by `(from, to, via)`, emit one
`DispatchTraceRecord` per group with `count = group.len()` and `ts = max(group.timestamp)`.

## Error Code Registry (this feature)

Per the constitution's Error Code Registry requirement ("All error codes used in tool
responses MUST be documented in `data-model.md`... New error codes MUST follow
SCREAMING_SNAKE_CASE"). Reuses the existing standard codes (`IRIS_UNREACHABLE`,
`INVALID_PARAMS`, `INVALID_ACTION`) wherever they already fit; the two conditions below have
no existing equivalent and are new for this feature.

| Code | Used by | Condition |
|---|---|---|
| `BENCHMARK_RUN_IN_PROGRESS` | `iris-agentic-dev benchmark` CLI (contracts/benchmark-cli.md) | A benchmark run is started while a lock record at `^IRISDEV("telemetry","benchmark_lock",container_name)` (or local-file equivalent) is still held by another active run against the same container (FR-013). Non-zero exit, distinct from `IRIS_UNREACHABLE`. |
| `SUITE_NOT_AVAILABLE` | `iris-agentic-dev benchmark` CLI `--suite` flag | `--suite` is given a value other than `jira` (the only suite ported in v1 — `mf`/`sql` are explicitly deferred per the Clarifications session). Non-zero exit before any container interaction. |
| `IRIS_UNREACHABLE` | `telemetry_query`, `telemetry_export_trace`, `benchmark` CLI | Reused as-is (existing standard code) when no IRIS connection is discoverable and the operation requires one (the durable-sink IRIS-global path specifically; local-file-mode reads do not require this). |
| `INVALID_PARAMS` | `telemetry_query` | Reused as-is when `limit` exceeds the documented max (5000) or `since`/`until`/`session_id` fail to parse. |
