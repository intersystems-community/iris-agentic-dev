# Implementation Plan: Tool Telemetry and Benchmark Harness

**Branch**: `059-tool-telemetry-benchmark` | **Date**: 2026-07-01 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/059-tool-telemetry-benchmark/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/commands/plan.md` for the execution workflow.

## Summary

A single Rust telemetry layer inside `iris-agentic-dev` records every MCP tool call
(name, success/failure, duration, timestamp, session id) to an unbounded in-memory ring
buffer (authoritative for live `agent_history`/`agent_stats`) plus a best-effort durable
sink — an IRIS global when a live connection exists, a local append-only file otherwise.
That same record feeds two consumers: (1) a native benchmark harness, ported from
`objectscript-coder`'s `objectscript_mcp/benchmark` runner, that replays the primary
`jira_bugs` repair task suite against `iris-agentic-dev`'s own `iris_compile`/`iris_execute`/
`iris_test` tools and reports `pass_rate`/`baseline_pass_rate`/`lift` (absolute difference),
fixing `light-skills/BENCHMARKING.md`'s broken private-repo Quick Start; and (2) a
trace-record export in the exact `{from, to, via, count, ts}` shape `058-iris-graph`'s
`record_trace` action already expects, aggregating repeated identical call edges.

## Technical Context

**Language/Version**: Rust 2021 (workspace `edition = "2021"`, matches
`crates/iris-agentic-dev-core`)  
**Primary Dependencies**: `reqwest` 0.12 (already a dependency, used for Anthropic/OpenAI
calls in `generate.rs` and Atelier REST elsewhere), `serde`/`serde_json` (already deps),
`tokio` (already a dep, `full` features). New dependency: `uuid` (v4 session ids — not yet
in the workspace; small, no transitive bloat, justified under Principle VII since no
existing crate provides UUID generation). No new HTTP/LLM SDK — the benchmark harness
reuses the exact Anthropic/OpenAI request shapes already implemented in `generate.rs`'s
`LlmClient` rather than adding a bedrock/anthropic SDK crate.  
**Storage**: Dual-sink durable telemetry — IRIS global (name TBD in research.md, mirroring
but not identical to objectscript-coder's `^TOOLCALLS(session_id, seq_n)`) when
`ConnectionState` has a live `IrisConnection`; local append-only JSONL file under the
workspace/config data directory (mirroring `.iris-agentic-dev.toml`'s co-location
convention) when disconnected. In-memory: `VecDeque<ToolCallRecord>` behind
`Arc<Mutex<_>>`, same pattern as the existing `history: Arc<Mutex<VecDeque<ToolCallEntry>>>`
field on `IrisTools`, with a larger/config-driven capacity superseding the current
hardcoded 50.  
**Testing**: `cargo test` with the existing two-tier pattern — unit tests for pure
functions (redaction, aggregation, lift math, pruning policy) run unconditionally; live
integration tests in `crates/iris-agentic-dev-core/tests/integration/` marked `#[ignore]`
and gated on a running `iris-dev-iris` container, run via
`cargo test -- --ignored` per Constitution Principle IV.  
**Target Platform**: Same as the rest of iris-agentic-dev — cross-platform CLI/MCP-server
binary (macOS/Linux primary, Docker-based IRIS dependency for live tests).  
**Project Type**: Single Rust workspace (existing `crates/iris-agentic-dev-core` +
`crates/iris-agentic-dev-bin`); no new crate — telemetry/benchmark modules added to
`iris-agentic-dev-core`.  
**Performance Goals**: SC-005 — recording a tool call adds no user-perceptible delay;
durable-sink writes MUST be non-blocking relative to the tool call's own response (spawn
a background write or otherwise decouple, per FR-014's "MUST NOT block").  
**Constraints**: FR-011/SC-006 — pruning must never corrupt or drop data for a
benchmark run still in progress; FR-014 — telemetry write failure must never fail the
underlying tool call; the benchmark harness (FR-005/FR-006) must run against a clean
public clone with zero private-repo references and zero dependency on the Python MCP
server.  
**Scale/Scope**: v1 ports one task suite (`jira_bugs`, ~26 tasks per
`bench/eval_tasks/jira_bugs/` in objectscript-coder) into iris-agentic-dev's own repo;
telemetry volume is single-machine, single-process-lifetime per session, with
age/size-based pruning — no multi-machine or centralized-service scale target (Assumption 3).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

**Post-Phase-1 re-check**: All findings below are now resolved by research.md/data-model.md/
contracts — no gate remains open.
- II (NEEDS VERIFICATION → **PASS**): the durable IRIS-global sink was verified live
  against `iris-dev-iris` (`^IRISDEV("telemetry",...)` read/write, no compile step) —
  see research.md's first section. The benchmark harness's IRIS interaction reuses
  `iris_compile`/`iris_execute`/`iris_test` in-process rather than any new/unverified API.
- VII (NEEDS JUSTIFICATION → **PASS, justified**): the sole new dependency (`uuid`) is
  justified in Complexity Tracking below; no other new crate was introduced during Phase 1
  design (the benchmark harness's LLM calls reuse `generate.rs`'s existing request
  builders, confirmed in research.md).

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Zero-Install Binary | PASS | Telemetry and benchmark harness are pure Rust, built into the existing binary; the local-file durable sink requires no external install. The benchmark harness needs a running IRIS container, same as every other tool — no new install step beyond what already exists. |
| II. ObjectScript Sanity | NEEDS VERIFICATION | The benchmark harness's `BenchRunner`-equivalent (installing/running ported test classes) and any IRIS-global durable-sink read/write must be verified against a live `iris-dev-iris` container in Phase 0 research, not assumed from the Python `runner.py` implementation. |
| III. HTTP-First Execution | PASS | The benchmark harness is fully HTTP — compile via Atelier PUT+compile (same as `iris_compile`'s local-file branch) and test-running via a direct `$ClassMethod`/`$Method` introspection/invocation over `execute_via_generator` (mirroring objectscript-coder's own `IrisLight.BenchRunner.RunAll` mechanism, since the ported task suite's test classes extend `%RegisteredObject`, not `%UnitTest.TestCase` — verified live, see research.md). No Docker exec, no new exception to Principle III's HTTP-first default. |
| IV. Test-First, Fixture-Driven | PASS | Unit tests for redaction, lift math, aggregation, and pruning logic run with no IRIS; live integration tests for the durable IRIS-global sink and the end-to-end benchmark run are `#[ignore]`-gated per the existing pattern. |
| V. Output Shape Parity | PASS | `BenchmarkResult`'s `pass_rate`/`baseline_pass_rate`/`lift` fields and per-task detail match `BENCHMARKING.md`'s documented "Understanding Results" shape and the existing Python `runner.py` output exactly (FR-004). |
| VI. Environment Guard | PASS | `record_trace`-equivalent export and any telemetry write path affecting IRIS globals classify as Admin/Execute per the same `dispatch_gate`/`check_env_gate` model already used for `iris_query mode=write` (057) — no new gate mechanism. |
| VII. Dependency Minimalism | NEEDS JUSTIFICATION | One new crate (`uuid`, for session identifiers) proposed — see Complexity Tracking. No LLM SDK crate added; the benchmark harness's LLM calls reuse `generate.rs`'s existing raw-`reqwest` Anthropic/OpenAI request builders rather than adding a boto3/anthropic-sdk equivalent. |
| VIII. 90% Coverage Gate | PASS | Polish phase includes a coverage-check task; new telemetry/benchmark modules are covered by the same `cargo llvm-cov --include-ignored` invocation as the rest of the crate. |

*A plan with any FAIL gate MUST NOT proceed to implementation.*

## Project Structure

### Documentation (this feature)

```text
specs/[###-feature]/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
crates/iris-agentic-dev-core/src/
├── telemetry/                    # NEW — this feature
│   ├── mod.rs                    # ToolCallRecord, session id, ring buffer + dual-sink writer
│   ├── redact.rs                 # parameter redaction via existing policy/data_policy_gate model
│   ├── prune.rs                  # age/size-based pruning, run-in-progress protection (FR-011)
│   └── trace_export.rs           # aggregation into {from,to,via,count,ts} (FR-009/FR-010)
├── benchmark/                    # NEW — this feature
│   ├── mod.rs                    # BenchmarkTask/BenchmarkResult, run_benchmark orchestration
│   ├── container.rs              # ported from objectscript_mcp/benchmark/container.py
│   ├── llm.rs                    # thin wrapper reusing generate.rs's LlmClient for skill-vs-baseline prompts
│   └── tasks/jira_bugs/*.json    # ported from bench/eval_tasks/jira_bugs/ (objectscript-coder)
├── tools/mod.rs                  # MODIFIED — record_call call sites write through telemetry::record()
├── policy/                       # MODIFIED — reuse for redaction; no new gate file
└── iris/workspace_config.rs      # MODIFIED — durable-sink IRIS global name / local file path config

crates/iris-agentic-dev-core/tests/
├── unit/test_telemetry_unit.rs           # NEW — redaction, pruning, aggregation, lift math (no IRIS)
├── unit/test_benchmark_unit.rs           # NEW — task loading, pass/fail scoring, error-vs-fail (no IRIS)
└── integration/test_telemetry_live.rs    # NEW — #[ignore], durable IRIS-global sink round-trip
└── integration/test_benchmark_live.rs    # NEW — #[ignore], full benchmark run against iris-dev-iris

light-skills/BENCHMARKING.md      # MODIFIED — Quick Start rewritten against the native harness
```

**Structure Decision**: Single project (existing Rust workspace). Two new sibling modules
(`telemetry/`, `benchmark/`) inside `iris-agentic-dev-core` — no new crate, no new
top-level directory outside the existing `crates/` tree. `tools/mod.rs`'s existing
`record_call` call sites are updated to also feed the new telemetry layer rather than
being replaced, keeping the in-memory ring buffer as the single write path both `agent_history`
and the durable sink read from.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|---------------------------------------|
| New dependency: `uuid` crate (Principle VII) | Session identifiers (FR-002/Key Entities: Session) need a collision-resistant id generated once per process lifetime. | Hand-rolling an id from `std::time` + PID risks collisions across fast-restarting processes in CI/benchmark loops, which would corrupt the durable record's session scoping (FR-002's session-scoped queries) — the exact bug class this feature exists to avoid. `uuid` is a single small, widely-vetted dependency (no transitive dependency bloat beyond it), a materially better tradeoff than a hand-rolled scheme that must itself be tested for uniqueness. |
