# Implementation Plan: Skill Regression and Lift Suite

**Branch**: `040-skill-regression` | **Date**: 2026-05-31 | **Spec**: [spec.md](./spec.md)

## Summary

A local CLI tool (`python -m tests.e2e.skill_eval`) that evaluates every skill in `light-skills/skills/` for fire-rate (does the LLM invoke it?), lift (does it improve pass rate?), regression (did lift drop since baseline?), and domain isolation (do load-on-demand skills stay quiet on unrelated tasks?). Reuses the 039 OpenCode harness for task execution and the existing benchmark judge (`benchmark/021/runner/judge.py`) for scoring — making results directly comparable to the existing leaderboard. Ships with `eval.yaml` configs for the 5 P1 skills; remaining 19 skills degrade gracefully to `no_task_coverage`.

---

## Technical Context

**Language/Version**: Python 3.11+
**Primary Dependencies**: existing `benchmark/021/runner/` (judge, fixtures, client), `tests/e2e/` (039 harness — isolated_env, opencode_runner, readme_validator), `pyyaml`, `anthropic` (Bedrock)
**Storage**: `tests/e2e/tasks/skills/*/eval.yaml` (configs), `tests/e2e/results/skill-eval-*.json` (run results), `tests/e2e/results/skill-baseline.json` (comparison baseline)
**Testing**: pytest unit tests for evaluator logic; integration test runs one skill end-to-end
**Target Platform**: macOS/Linux local only (not CI)
**Performance Goals**: Full 5-skill suite < 25 min; single-skill run < 5 min
**Constraints**: `--dry-run`/`--yes` guard on full-suite runs; baseline only updated with `--update-baseline`

---

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Zero-Install Binary | N/A | Test tooling, not a user-facing binary |
| II. ObjectScript Sanity | N/A | No new ObjectScript API calls; reuses existing judge |
| III. HTTP-First Execution | N/A | Test tooling |
| IV. Test-First, Fixture-Driven | PASS | Unit tests before evaluator logic; eval.yaml fixtures committed |
| V. Output Shape Parity | N/A | New output type |
| VI. Environment Guard | PASS | Dry-run guard on full-suite cost; IRIS via existing container |
| VII. Dependency Minimalism | PASS | No new dependencies beyond existing test infra |

No violations. Plan may proceed.

---

## Project Structure

```text
tests/e2e/
├── skill_eval/
│   ├── __init__.py
│   ├── __main__.py           # CLI entry point
│   ├── evaluator.py          # SkillEvaluator — orchestrates fire-rate + lift runs
│   ├── fire_rate.py          # Fire-rate measurement via 039 OpenCode harness
│   ├── lift.py               # Lift measurement: run benchmark task, call judge
│   ├── isolation.py          # Domain skill isolation guard
│   ├── cost_estimator.py     # Dry-run cost/time estimate
│   ├── baseline.py           # Read/write/diff skill-baseline.json
│   └── reporter.py           # Stdout summary table + EvalRun JSON writer
│
└── tasks/skills/
    ├── objectscript-review/eval.yaml
    ├── objectscript-guardrails/eval.yaml
    ├── objectscript-sql-patterns/eval.yaml
    ├── objectscript-list-patterns/eval.yaml
    ├── objectscript-unit-test/eval.yaml
    ├── iris-vector-ai/eval.yaml          # domain skill
    ├── iris-connectivity/eval.yaml       # domain skill
    └── ensemble-production/eval.yaml     # domain skill

tests/e2e/results/
└── skill-baseline.json       # updated only with --update-baseline

specs/040-skill-regression/
├── plan.md
├── research.md
├── data-model.md
├── contracts/cli.md
└── tasks.md
```

---

## Phase 1: Core infrastructure (no LLM calls)

**Goal**: All pure-logic modules unit-tested before any real LLM invocation.

### P1-A: SkillEvalConfig loader + discovery

`tests/e2e/skill_eval/evaluator.py`:
- `discover_skills(light_skills_dir)` — scan `light-skills/skills/`, return list of skill names
- `load_eval_config(skill_name, tasks_dir)` → `SkillEvalConfig | None` (None = no eval.yaml)
- Unit test: discover returns all 24 skills; load returns config for known skill, None for missing

### P1-B: Baseline read/write/diff

`tests/e2e/skill_eval/baseline.py`:
- `load_baseline(path)` → dict (empty dict if file absent)
- `save_baseline(results, path)` — writes new baseline from EvalRun skill results
- `compute_diff(old, new)` → list of `{skill, old_lift, new_lift, delta}` sorted by abs(delta) desc
- Unit test: round-trip, diff detection, new-skill marker, empty baseline

### P1-C: Cost estimator

`tests/e2e/skill_eval/cost_estimator.py`:
- `estimate(configs, runs_per_skill)` → `{task_runs, judge_calls, cost_usd, time_minutes}`
- Cost constants: Sonnet input $0.003/1K, output $0.015/1K; Haiku $0.0008/1K
- Unit test: estimate for 1 skill with 3 benchmark tasks, 3 fire-rate runs

### P1-D: Reporter

`tests/e2e/skill_eval/reporter.py`:
- `print_summary(eval_run)` — stdout table as shown in contracts/cli.md
- `write_result(eval_run, output_dir)` → path to JSON file
- Unit test: table renders correctly for mix of covered/uncovered/domain skills

---

## Phase 2: Fire-rate measurement (US1 + US3 fire side)

**Goal**: `fire_rate.py` runs a prompt via the 039 harness and checks whether the `skill` tool was called.

`tests/e2e/skill_eval/fire_rate.py`:
- `measure_fire_rate(config, n_runs, isolated_env_factory, model)` → `float`
- Spawns `n_runs` isolated OpenCode sessions with the skill installed
- Counts sessions where `skill` tool appears in event stream tool calls
- Returns `fire_rate = hits / n_runs`

Uses `IsolatedEnv` + `ReadmeValidator` (or local fallback) from 039 for skill installation.
Uses `collect_events` from 039 `opencode_runner`.

Unit test: mock `collect_events` returning events with/without skill tool call, verify rate computation.
E2E test: run `objectscript-review` fire-rate task once, assert `fire_rate > 0`.

**Phase gate**: `test_fire_rate_single_skill` passes.

---

## Phase 3: Lift measurement (US1 lift side)

**Goal**: `lift.py` runs a benchmark task via the 039 harness, formats the transcript for the judge, and returns a pass/fail verdict.

`tests/e2e/skill_eval/lift.py`:
- `run_task_and_score(task_yaml_path, skill_name, condition, isolated_env_factory, model)` → `{score, reasoning, transcript}`
- Loads task from `benchmark/021/tasks/{task_id}.yaml`
- Applies fixtures via `tests/e2e/fixtures.py` (Atelier REST)
- Runs OpenCode via 039 harness
- Formats event stream as transcript for judge: `{tool_calls: [...], response: str, tool_call_count: int}`
- Calls `benchmark.runner.judge.score_result(task_dict, result_dict)` → score
- `measure_lift(config, n_runs, isolated_env_factory, model)` → `{pass_rate_baseline, pass_rate_skill, lift}`

Unit test: mock `collect_events` + `score_result`, verify lift computation.
E2E test: run `objectscript-review` on `DBG-01` baseline vs skill, assert `lift > 0`.

**Phase gate**: `test_lift_objectscript_review` passes (lift > 0 on DBG-01).

---

## Phase 4: Domain isolation guard (US3)

**Goal**: `isolation.py` runs a general repair task with a domain skill installed and asserts `fire_rate = 0.0`.

`tests/e2e/skill_eval/isolation.py`:
- `check_isolation(config, n_runs, isolated_env_factory, model)` → `float` (isolation_fire_rate)
- Runs `isolation_prompt` from `eval.yaml` with the domain skill installed
- Returns fire rate on the isolation task (expected: 0.0)

Unit test: mock events with no skill calls, verify 0.0 return.
E2E test: `iris-vector-ai` on a Return-in-loop task, assert isolation_fire_rate = 0.0.

**Phase gate**: `test_isolation_iris_vector_ai` passes.

---

## Phase 5: CLI, eval.yaml configs, full integration

### P5-A: CLI entry point

`tests/e2e/skill_eval/__main__.py`:
- Parses args (see contracts/cli.md)
- Full-suite run without `--yes`: calls `cost_estimator.estimate()`, prints dry-run output, prompts confirmation
- `--dry-run`: estimate only, exit 0
- Orchestrates: discover → load configs → run fire-rate + lift + isolation → write results → compare baseline → print summary
- Exit codes per contracts/cli.md

### P5-B: eval.yaml for all 8 initial skills

Write `eval.yaml` for:
- `objectscript-review` — benchmark_tasks: [DBG-01, DBG-02, DBG-03, MOD-01, MOD-02]
- `objectscript-guardrails` — benchmark_tasks: [DBG-01, DBG-02, MOD-01]
- `objectscript-sql-patterns` — benchmark_tasks: [GEN-03, GEN-04]
- `objectscript-list-patterns` — benchmark_tasks: [MOD-03, MOD-04]
- `objectscript-unit-test` — benchmark_tasks: [GEN-01, GEN-02]
- `iris-vector-ai` — domain_skill: true, no benchmark_tasks, isolation_prompt: Return-in-loop
- `iris-connectivity` — domain_skill: true, isolation_prompt: %List task
- `ensemble-production` — domain_skill: true, isolation_prompt: SQL fix task

### P5-C: Integration test

`tests/e2e/skill_eval/test_skill_eval_integration.py`:
- `test_dry_run_no_llm_calls` — dry-run exits 0 with cost estimate, no LLM calls
- `test_single_skill_objectscript_review` — `--skill objectscript-review` produces SkillResult with fire_rate > 0 and lift > 0
- `test_regression_detection` — inject a fake baseline with higher lift, assert regression_flag

**Phase gate**: integration tests pass.

---

## Phased delivery

| Phase | Deliverable | Gate |
|-------|-------------|------|
| 1 | Config loader, baseline, cost estimator, reporter — all unit tested | All unit tests pass |
| 2 | fire_rate.py — real OpenCode call | `test_fire_rate_single_skill` passes |
| 3 | lift.py — benchmark task + judge | `test_lift_objectscript_review` passes |
| 4 | isolation.py — domain guard | `test_isolation_iris_vector_ai` passes |
| 5 | CLI + 8 eval.yaml + integration tests + full run | Full suite runs, results written |

---

## Risks and mitigations

| Risk | Mitigation |
|------|-----------|
| LLM non-determinism makes lift unstable | 3 runs minimum; ±10pp tolerance in SC-006; results include raw scores not just averages |
| Benchmark judge model (Haiku) produces inconsistent scores | Use same judge as leaderboard (already validated); log reasoning for manual review |
| IRIS not running locally | Same guard as 039: check `IRIS_CONTAINER` and `IRIS_WEB_PORT` before starting, clear error message |
| Benchmark tasks use BENCHMARK namespace; 039 uses USER | All eval runs use USER namespace; fixture loading via Atelier REST to USER |
