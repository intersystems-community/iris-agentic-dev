# Implementation Plan: GEPA-Inspired Skill Optimization Loop

**Branch**: `030-gepa-skill-optimizer` | **Date**: 2026-07-09 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/030-gepa-skill-optimizer/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/commands/plan.md` for the execution workflow.

## Summary

A new `optimizer/` module inside `iris-agentic-dev-core` makes `skill_optimize` real:
(1) `analyze.rs` diagnoses non-passing `TaskResult`s from a benchmark run — paged in
bounded batches through the existing `generate.rs::LlmClient` — into structured
`FailurePattern`s; (2) `mutate.rs` uses the same `LlmClient` to propose a bounded number
of candidate skill-text rewrites addressing those patterns; (3) each candidate is scored
by calling `benchmark::run_suite` in-process against a held-out task subset plus every
task in that skill's persisted regression-lock set; (4) the winning candidate (if any) is
returned as a diff with scores under `apply: false`, and only written to the skill file
when a follow-up call passes `apply: true` referencing that winning candidate. No new
external service, LLM SDK, or optimization library — every piece this feature adds is a
consumer of infrastructure 059-tool-telemetry-benchmark already shipped
(`benchmark::run_suite`, `benchmark::load_embedded_tasks`, the concurrent-run lock, and
`telemetry::read_durable`/`filter_records`).

## Technical Context

**Language/Version**: Rust 2021 (workspace `edition = "2021"`, matches
`crates/iris-agentic-dev-core`).
**Primary Dependencies**: None new. Reuses `generate.rs::LlmClient` (already used by
`benchmark/llm.rs`), `serde`/`serde_json`, `tokio` — all existing workspace dependencies.
**Storage**: Per-skill `RegressionLockSet` persisted as a JSON sidecar file,
`<skill_dir>/.optimizer-lock.json`, colocated with each skill's `SKILL.md` (mirrors the
telemetry module's own convention of colocating durable state near the thing it tracks
— see 059's `.iris-agentic-dev/telemetry/` convention). No IRIS-global storage needed for
this feature specifically — the lock set is small (bounded by task-suite size, Assumption
4) and skill-local, unlike telemetry's per-session volume.
**Testing**: `cargo test` with the existing two-tier pattern — unit tests for pure
functions (batch-paging logic, candidate-vs-baseline comparison, lock-set update rules)
run unconditionally; live integration tests in
`crates/iris-agentic-dev-core/tests/integration/` marked `#[ignore]` and gated on a
running `iris-dev-iris` container plus a configured LLM API key, run via
`cargo test -- --ignored`, per Constitution Principle IV.
**Target Platform**: Same as the rest of iris-agentic-dev — cross-platform CLI/MCP-server
binary.
**Project Type**: Single Rust workspace (existing `crates/iris-agentic-dev-core`); no new
crate — a new `optimizer/` module added beside `telemetry/` and `benchmark/`.
**Performance Goals**: Not a live-tool-call-latency-sensitive path (unlike 059's
telemetry write) — `skill_optimize` is an explicit, human-invoked, multi-minute operation
(it runs the benchmark harness, potentially several times, per call). No SC targets
response latency; SC targets correctness (never regress a locked task, never write
without `apply: true`).
**Constraints**: FR-004/FR-005 — propose/apply is a hard two-step gate, no auto-apply
path may exist. FR-006 — every candidate must be checked against the full regression-lock
set, not just the held-out subset, before it can win. FR-008/FR-009 — no new LLM SDK or
scoring mechanism; both reuse existing code paths exactly.
**Scale/Scope**: One skill per call, default 3 candidates per round, default batch size 5
for diagnosis, held-out subset drawn from the existing 22-task `jira_bugs` suite (059).
No multi-skill batch mode, no cross-skill sharing of lock sets (Assumption 6).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Zero-Install Binary | PASS | Pure Rust, built into the existing binary. This is precisely why tool-description optimization (which would need a rebuild) is out of scope — skill-text optimization needs no install step beyond what `benchmark` already requires. |
| II. ObjectScript Sanity | PASS (inherited) | This feature adds no new IRIS interaction beyond calling `benchmark::run_suite`, already verified live by 059. The regression-lock sidecar is a local JSON file, not IRIS state. |
| III. HTTP-First Execution | PASS (inherited) | Scoring goes through `benchmark::run_suite`, which is already fully HTTP (059's research.md). This feature adds no Docker-exec path. |
| IV. Test-First, Fixture-Driven | PASS | Unit tests for batch-paging, lock-set update rules, and candidate-selection logic run with no IRIS/LLM; live integration tests (diagnosis + full optimize round) are `#[ignore]`-gated. |
| V. Output Shape Parity | PASS | `skill_optimize`'s response shape (candidate diff, held-out score, held-out set size, locked-task detail) is new — no pre-existing shape to match, since the tool was previously a stub. Documented in contracts/skill-optimize-tool.md as the canonical shape going forward. |
| VI. Environment Guard | PASS | `skill_optimize` stays classified `ToolCategory::Skill` (its existing classification, unchanged) — same tier as `skill_propose`/`skill_share`. |
| VII. Dependency Minimalism | PASS | Zero new dependencies — the entire point of Clarifications' first two decisions was avoiding a new LLM SDK/optimization-service dependency by reusing `generate.rs::LlmClient` and `benchmark::run_suite` directly. |
| VIII. 90% Coverage Gate | PASS | Polish phase includes a coverage-check task; new `optimizer/` modules covered by the same `cargo llvm-cov --include-ignored` invocation as the rest of the crate. |

*A plan with any FAIL gate MUST NOT proceed to implementation.*

## Project Structure

### Documentation (this feature)

```text
specs/030-gepa-skill-optimizer/
├── spec.md               # Feature specification (rewritten 2026-07-09)
├── plan.md               # This file
├── research.md           # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md         # Phase 1 output
├── contracts/            # Phase 1 output
└── tasks.md              # Phase 2 output
```

### Source Code (repository root)

```text
crates/iris-agentic-dev-core/src/
├── optimizer/                     # NEW — this feature
│   ├── mod.rs                     # FailurePattern, SkillCandidate, OptimizationRound, RegressionLockSet, run_optimize orchestration
│   ├── analyze.rs                 # diagnose_failures — batched LLM summarization of non-passing TaskResults (FR-001/FR-002)
│   ├── mutate.rs                  # generate_candidates — LLM-proposed skill-text rewrites (FR-003c/FR-008)
│   └── lock.rs                    # RegressionLockSet read/write (JSON sidecar), lock-check scoring (FR-006/FR-007)
├── tools/mod.rs                   # MODIFIED — skill_optimize handler replaces its NOT_IMPLEMENTED stub
├── benchmark/mod.rs                # UNCHANGED — consumed as-is (run_suite, load_embedded_tasks, acquire_lock/release_lock/decide_lock)
└── telemetry/mod.rs                # UNCHANGED — consumed as-is (read_durable, filter_records)

crates/iris-agentic-dev-core/tests/
├── unit/test_optimizer_analyze.rs         # NEW — batch-paging logic, no LLM/IRIS
├── unit/test_optimizer_lock.rs            # NEW — lock-set update rules, candidate-vs-locked-task exclusion logic
├── unit/test_optimizer_selection.rs       # NEW — winner-selection threshold logic (pure function, no LLM/IRIS)
└── integration/test_optimizer_live.rs     # NEW — #[ignore], full skill_optimize round against iris-dev-iris + a real LLM
```

**Structure Decision**: Single project (existing Rust workspace). One new sibling module
(`optimizer/`) inside `iris-agentic-dev-core`, alongside `telemetry/` and `benchmark/` —
no new crate. `tools/mod.rs`'s existing `skill_optimize` handler is replaced in place
(same tool name, same `SkillNameParams`-derived input shape extended with `apply: bool`
and round-selection fields — see contracts/skill-optimize-tool.md), not added as a new
tool name.

## Complexity Tracking

*No new dependencies, no Constitution deviations — this table is intentionally empty.*

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|---------------------------------------|
| — | — | — |
