# Data Model: GEPA-Inspired Skill Optimization Loop

## FailurePattern

One diagnosed non-passing task, produced by `optimizer::analyze::diagnose_failures`.

| Field | Type | Notes |
|---|---|---|
| `task_id` | `String` | Matches `TaskResult.task_id` from the triggering benchmark run. |
| `outcome` | `enum { Fail, Error }` | Copied from the `TaskResult` — `Pass` tasks never produce a `FailurePattern`. |
| `summary` | `String` | LLM-synthesized natural-language explanation of the likely cause, produced per-batch (spec FR-001/FR-002). |
| `tool_calls` | `Vec<TelemetrySummary>` | Condensed view of the matching session's `ToolCallRecord`s for this task (tool name, success, duration — not full params, to keep the diagnosis prompt small). |

### TelemetrySummary (nested)

| Field | Type | Notes |
|---|---|---|
| `tool` | `String` | From `telemetry::ToolCallRecord.tool`. |
| `success` | `bool` | |
| `duration_ms` | `u64` | |

## SkillCandidate

One proposed replacement skill text, produced by `optimizer::mutate::generate_candidates`.

| Field | Type | Notes |
|---|---|---|
| `round` | `u32` | Which generation round within this `OptimizationRound` produced it (v1: always `1` — multi-round refinement is a future extension, not v1 scope). |
| `text` | `String` | Full candidate `SKILL.md` content. |
| `held_out_pass_rate` | `Option<f64>` | Set once scored via `benchmark::run_suite` against the held-out subset; `None` before scoring. |
| `held_out_set_size` | `usize` | Number of tasks in the held-out subset used to compute `held_out_pass_rate` (spec FR-012). |
| `locked_task_results` | `Vec<{task_id: String, passed: bool}>` | Per-locked-task pass/fail from scoring this candidate against the current `RegressionLockSet` (spec FR-006). |
| `passes_all_locked` | `bool` | `true` iff every entry in `locked_task_results` has `passed == true` — the FR-006 exclusion gate. |

## OptimizationRound

The result of one `skill_optimize` invocation.

| Field | Type | Notes |
|---|---|---|
| `skill_name` | `String` | |
| `failure_patterns` | `Vec<FailurePattern>` | Empty when the skill has zero non-passing tasks (spec FR-011). |
| `candidates` | `Vec<SkillCandidate>` | Empty when `failure_patterns` is empty. |
| `baseline_held_out_pass_rate` | `f64` | The current (pre-optimization) skill text's `pass_rate` on the same held-out subset, for comparison. |
| `winner` | `Option<usize>` | Index into `candidates` of the winning candidate, if any — `None` when no candidate both beats baseline AND `passes_all_locked` (spec Acceptance Scenario 3, User Story 3). |
| `applied` | `bool` | `true` iff this call was made with `apply: true` and actually wrote the skill file. |

**State transitions**: an `OptimizationRound` is produced once per `skill_optimize` call
and is not mutated afterward. A propose-only round (`apply: false`) with a `winner` is
held in `IrisTools`'s process-local `last_proposal` map (research.md) so a subsequent
`apply: true` call for the same skill name can reference it; a fresh propose-only call for
the same skill replaces the previous entry.

## RegressionLockSet

Persisted per-skill, at `<skill_dir>/.optimizer-lock.json`.

| Field | Type | Notes |
|---|---|---|
| `locked_task_ids` | `Vec<String>` | Grows only — a task id is added when a candidate that fixes it is applied (spec FR-007); never removed in v1 (no un-locking mechanism). |
| `history` | `Vec<{applied_at: String, task_ids_added: Vec<String>, held_out_pass_rate: f64}>` | One entry per applied round, for audit — not read by the scoring logic itself, only appended to. |

**Validation rules**: `locked_task_ids` MUST NOT contain duplicates (a task already locked
by an earlier round that's fixed again by coincidence is not re-added). File absence is
equivalent to an empty `RegressionLockSet` (no lock file yet == no locked tasks) — reading
a missing sidecar file is not an error condition.

## Error Code Registry (this feature)

Per the constitution's Error Code Registry requirement. Reuses existing standard codes
(`IRIS_UNREACHABLE`, `INVALID_PARAMS`) wherever they already fit; the codes below are new
for this feature.

| Code | Used by | Condition |
|---|---|---|
| `LLM_NOT_CONFIGURED` | `skill_optimize` | `LlmClient::from_env()` returns `None` — no `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` (or equivalent) available for diagnosis or candidate generation (spec FR-013). |
| `NO_PENDING_PROPOSAL` | `skill_optimize` with `apply: true` | No prior propose-only (`apply: false`) call's winning candidate is available for the given skill name in the current process (spec FR-005). |
| `NOTHING_TO_OPTIMIZE` | `skill_optimize` | The named skill's benchmark run has zero non-passing tasks (spec FR-011) — not an error exactly, but a distinct, non-`winner` response state signaled via this code in the `note` field rather than `error`. |
| `BENCHMARK_RUN_IN_PROGRESS` | `skill_optimize` (reused from 059) | Concurrent-run lock held by another run against the same container (research.md — reuses 059's existing lock). |
| `IRIS_UNREACHABLE` | `skill_optimize` (reused from 059) | No IRIS connection discoverable. |
