# Tasks: Skill Regression and Lift Suite (040)

## Feature
A local CLI (`python -m tests.e2e.skill_eval`) that evaluates all skills in `light-skills/skills/` for fire-rate, lift, regression, and domain isolation. Reuses the 039 OpenCode harness and benchmark judge. Ships with eval.yaml for 8 skills (5 standard + 3 domain).

## Files
- `tests/e2e/skill_eval/__init__.py`
- `tests/e2e/skill_eval/__main__.py`
- `tests/e2e/skill_eval/evaluator.py`
- `tests/e2e/skill_eval/fire_rate.py`
- `tests/e2e/skill_eval/lift.py`
- `tests/e2e/skill_eval/isolation.py`
- `tests/e2e/skill_eval/cost_estimator.py`
- `tests/e2e/skill_eval/baseline.py`
- `tests/e2e/skill_eval/reporter.py`
- `tests/e2e/tasks/skills/*/eval.yaml` (8 files)
- `tests/e2e/skill_eval/test_*.py` (unit tests)
- `tests/e2e/skill_eval/test_integration.py`

---

## Phase 1: Setup

- [x] T001 Create `tests/e2e/skill_eval/` directory with `__init__.py` and `tests/e2e/tasks/skills/` directory
- [x] T002 Verify `benchmark/021/runner/judge.py` is importable from `tests/e2e/skill_eval/` context — add `sys.path` shim or package link as needed; document import pattern in `__init__.py`

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: All pure-logic modules unit-tested before any LLM invocation.

- [x] T003 Write unit tests for `SkillEvalConfig` loader in `tests/e2e/skill_eval/test_evaluator.py`: test `discover_skills()` returns all dirs in `light-skills/skills/`; test `load_eval_config()` returns config for a known skill, returns None for missing eval.yaml
- [x] T004 Implement `tests/e2e/skill_eval/evaluator.py`: `discover_skills(light_skills_dir) -> list[str]` scans subdirs with SKILL.md; `load_eval_config(skill_name, tasks_dir) -> SkillEvalConfig | None` reads `tests/e2e/tasks/skills/{skill}/eval.yaml`; `SkillEvalConfig` dataclass with fields: `skill`, `description`, `fire_rate_prompt`, `benchmark_tasks: list[str]`, `domain_skill: bool`, `isolation_prompt: str | None`
- [x] T005 [P] Write unit tests for baseline in `tests/e2e/skill_eval/test_baseline.py`: test `load_baseline()` returns empty dict for missing file; test `save_baseline()` round-trip; test `compute_diff()` detects regression (old lift 0.29, new 0.10 → delta -0.19); test `compute_diff()` marks new skills correctly
- [x] T006 [P] Implement `tests/e2e/skill_eval/baseline.py`: `load_baseline(path) -> dict`; `save_baseline(results: list[SkillResult], path)`; `compute_diff(old: dict, new: list[SkillResult]) -> list[dict]` with fields `{skill, old_lift, new_lift, delta}` sorted by abs(delta) desc; skills absent from old baseline get `new_skill: True`
- [x] T007 [P] Write unit tests for cost estimator in `tests/e2e/skill_eval/test_cost_estimator.py`: test estimate for 1 skill with 3 benchmark tasks × 3 runs = 18 task calls + 9 judge calls; test domain-only skill (fire-rate + isolation, no benchmark)
- [x] T008 [P] Implement `tests/e2e/skill_eval/cost_estimator.py`: `estimate(configs: list[SkillEvalConfig], runs: int) -> dict` returns `{task_runs, judge_calls, cost_usd, time_minutes}`; cost constants Sonnet ~$0.02/call, Haiku ~$0.001/call; `format_dry_run(estimate) -> str` formats the output block per contracts/cli.md
- [x] T009 [P] Write unit tests for reporter in `tests/e2e/skill_eval/test_reporter.py`: test `print_summary()` stdout contains skill name, lift, regression flag columns; test `write_result()` produces valid JSON matching EvalRun schema from data-model.md
- [x] T010 [P] Implement `tests/e2e/skill_eval/reporter.py`: `print_summary(eval_run: EvalRun)` prints table per contracts/cli.md; `write_result(eval_run: EvalRun, output_dir: str) -> str` writes `skill-eval-{timestamp}.json`; `EvalRun` and `SkillResult` dataclasses per data-model.md

**Checkpoint**: `pytest tests/e2e/skill_eval/test_evaluator.py tests/e2e/skill_eval/test_baseline.py tests/e2e/skill_eval/test_cost_estimator.py tests/e2e/skill_eval/test_reporter.py` — all pass.

---

## Phase 3: US1 — Full skill evaluation suite (Priority: P1)

### Goal
Running the suite against covered skills produces fire-rate, lift, and regression results for each.

### Independent test criteria
`test_single_skill_objectscript_review` passes: fire_rate > 0, lift > 0, SkillResult written to JSON.

### Tasks

- [x] T011 [US1] Write unit tests for `fire_rate.py` in `tests/e2e/skill_eval/test_fire_rate.py`: mock `collect_events` returning events with `skill` tool call → fire_rate = 1.0; mock events without skill call → fire_rate = 0.0; test averaging over N runs
- [x] T012 [US1] Implement `tests/e2e/skill_eval/fire_rate.py`: `measure_fire_rate(config: SkillEvalConfig, n_runs: int, openai_key: str, model: str) -> float` — creates `IsolatedEnv`, installs skill via `ReadmeValidator` (falls back to local), runs `collect_events(fire_rate_prompt, env_vars, model)`, checks whether any event has `part["tool"] == "skill"` (exact match on OpenCode's built-in skill tool, not a suffix check); returns hits/n_runs
- [x] T013 [US1] Write unit tests for `lift.py` in `tests/e2e/skill_eval/test_lift.py`: mock `collect_events` + `score_result`; verify baseline run passes no skill, skill run installs skill; verify pass_rate = (score≥2 count)/total; verify lift = skill_rate - baseline_rate
- [x] T014 [US1] Implement `tests/e2e/skill_eval/lift.py`: `run_task_and_score(task_id, skill_name_or_none, isolated_env, model) -> dict` — loads `benchmark/021/tasks/{task_id}.yaml`, applies fixtures via `tests/e2e/fixtures.py` (Atelier REST to USER namespace), runs OpenCode via `collect_events`, formats transcript, calls `score_result(task_dict, result_dict)` from `benchmark/021/runner/judge.py`, returns `{score, reasoning}`; `measure_lift(config, n_runs, openai_key, iris_host, iris_web_port, model) -> dict` with `{pass_rate_baseline, pass_rate_skill, lift}`
- [x] T015 [US1] Write E2E integration test `test_fire_rate_objectscript_review` in `tests/e2e/skill_eval/test_integration.py`: skip if no `OPENAI_API_KEY`; run `measure_fire_rate` for `objectscript-review` fire_rate_prompt with 3 runs; assert `fire_rate >= 0.5`
- [x] T016 [US1] Write E2E integration test `test_lift_objectscript_review` in `tests/e2e/skill_eval/test_integration.py`: skip if no `OPENAI_API_KEY` or no IRIS; run `measure_lift` for `objectscript-review` on `DBG-01`; assert `lift > 0`

**Phase gate**: `test_fire_rate_objectscript_review` and `test_lift_objectscript_review` both pass.

---

## Phase 4: US2 — Regression detection (Priority: P1)

### Goal
Running the suite after a lift drop sets `regression_flag: true` for the affected skill and includes it in the regressions list.

### Independent test criteria
`test_regression_detection` passes: inject fake baseline with higher lift → `regression_flag: true` for that skill.

### Tasks

- [x] T017 [US2] Write unit test `test_regression_detection` in `tests/e2e/skill_eval/test_integration.py`: create a fake baseline JSON with `objectscript-review` lift = 0.99; run evaluator with `regression_threshold=0.05`; assert result has `regression_flag: true` and `lift_delta < 0`
- [x] T018 [US2] Write unit test `test_no_regression_on_improvement` in `tests/e2e/skill_eval/test_integration.py`: create baseline with lift = 0.10; run with known higher lift; assert `regression_flag: false` and `lift_delta > 0`
- [x] T019 [US2] Write unit test `test_update_baseline_writes_diff` in `tests/e2e/skill_eval/test_integration.py`: run with `--update-baseline`; assert `skill-baseline.json` is written; assert diff output includes old vs new lift for changed skills
- [x] T020 [US2] Implement regression comparison in `tests/e2e/skill_eval/evaluator.py`: `compare_to_baseline(result: SkillResult, baseline: dict, threshold: float) -> SkillResult` — sets `regression_flag`, `lift_delta`, `new_skill` fields on result by comparing against stored baseline entry

**Phase gate**: `test_regression_detection` passes.

---

## Phase 5: US3 — Domain skill isolation (Priority: P2)

### Goal
Domain skills produce `isolation_fire_rate: 0.0` on general repair tasks and `fire_rate > 0.0` on their own domain tasks.

### Independent test criteria
`test_isolation_iris_vector_ai` passes: `iris-vector-ai` isolation_fire_rate = 0.0 on Return-in-loop task.

### Tasks

- [x] T021 [US3] Write unit tests for `isolation.py` in `tests/e2e/skill_eval/test_isolation.py`: mock `collect_events` with no skill calls → isolation_fire_rate = 0.0; with skill calls → isolation_fire_rate > 0 (unexpected)
- [x] T022 [US3] Implement `tests/e2e/skill_eval/isolation.py`: `check_isolation(config: SkillEvalConfig, n_runs: int, openai_key: str, model: str) -> float` — installs the domain skill, runs `isolation_prompt` via `collect_events`, returns fire_rate on that prompt (expected 0.0); identical to `fire_rate.measure_fire_rate` but uses `isolation_prompt` and semantics are inverted (lower is better)
- [x] T023 [US3] Write E2E test `test_isolation_iris_vector_ai` in `tests/e2e/skill_eval/test_integration.py`: skip if no key; run `check_isolation` for `iris-vector-ai` with Return-in-loop isolation_prompt (3 runs); assert `isolation_fire_rate == 0.0`
- [x] T024 [P] [US3] Write `tests/e2e/tasks/skills/iris-vector-ai/eval.yaml`: domain_skill: true, fire_rate_prompt: a VECTOR_COSINE query task, isolation_prompt: Return-in-For-loop fix (same as SKILL-01)
- [x] T025 [P] [US3] Write `tests/e2e/tasks/skills/iris-connectivity/eval.yaml`: domain_skill: true, fire_rate_prompt: Python IRIS connection task, isolation_prompt: ObjectScript %List fix task
- [x] T026 [P] [US3] Write `tests/e2e/tasks/skills/ensemble-production/eval.yaml`: domain_skill: true, fire_rate_prompt: start/stop a production task, isolation_prompt: SQL reserved-word fix task

**Phase gate**: `test_isolation_iris_vector_ai` passes.

---

## Phase 6: CLI, eval.yaml configs, full integration

- [x] T027 Implement `tests/e2e/skill_eval/__main__.py`: CLI entry point per contracts/cli.md — argparse for all flags including `--skill`, `--category`, `--dry-run`, `--yes`, `--update-baseline`, `--regression-threshold`, `--runs`, `--output`, `--model`; full-suite run without `--yes` calls `cost_estimator.estimate()` and prompts confirmation; `--dry-run` exits 0 after printing estimate; `--update-baseline` calls `baseline.save_baseline()` + prints `compute_diff()` output; if `skill-baseline.json` absent after run, auto-create it and print "No baseline found — created from this run." (FR-006 first-run handling); assembles EvalRun from all SkillResults including `no_task_coverage` entries for uncovered skills; calls `reporter.print_summary()` and `reporter.write_result()`; exits 1 if any regressions, 0 otherwise; `--category` filters skills to those whose name starts with the given prefix (e.g., `objectscript`, `iris`, `domain` matches `domain_skill: true`)
- [x] T028 [P] Write `tests/e2e/tasks/skills/objectscript-review/eval.yaml`: fire_rate_prompt: "Use the objectscript-review skill to review and fix the Return-in-For bug: [buggy FindFirst method]"; benchmark_tasks: [DBG-01, DBG-02, DBG-03, MOD-01, MOD-02]; domain_skill: false
- [x] T029 [P] Write `tests/e2e/tasks/skills/objectscript-guardrails/eval.yaml`: fire_rate_prompt: "Review this ObjectScript method for common mistakes before presenting it: [method with multiple issues]"; benchmark_tasks: [DBG-01, DBG-02, MOD-01]; domain_skill: false
- [x] T030 [P] Write `tests/e2e/tasks/skills/objectscript-sql-patterns/eval.yaml`: fire_rate_prompt: "Write an ObjectScript SQL query using %INLIST with a large set"; benchmark_tasks: [GEN-03, GEN-04]; domain_skill: false
- [x] T031 [P] Write `tests/e2e/tasks/skills/objectscript-list-patterns/eval.yaml`: fire_rate_prompt: "Write ObjectScript to iterate a %List and build a new filtered list"; benchmark_tasks: [MOD-03, MOD-04]; domain_skill: false
- [x] T032 [P] Write `tests/e2e/tasks/skills/objectscript-unit-test/eval.yaml`: fire_rate_prompt: "Use the objectscript-unit-test skill to write a %UnitTest for User.HarnessTestClass"; benchmark_tasks: [GEN-01, GEN-02]; domain_skill: false
- [x] T033 Write E2E test `test_dry_run_no_llm_calls` in `tests/e2e/skill_eval/test_integration.py`: run `python -m tests.e2e.skill_eval --dry-run` as subprocess; assert exit code 0; assert stdout contains "Estimated cost" and "Run with --yes"; assert no LLM calls were made (no charges)
- [x] T034 Write E2E test `test_single_skill_full_run` in `tests/e2e/skill_eval/test_integration.py`: skip if no key + IRIS; run `--skill objectscript-review --yes`; assert exit 0 or 1; assert `skill-eval-*.json` written to results dir; assert result has `fire_rate >= 0.5` and `lift` field populated

**Phase gate**: `test_dry_run_no_llm_calls` and `test_single_skill_full_run` both pass.

---

## Dependency Graph

```
T001-T002 (setup)
    ↓
T003-T010 (foundational — all pure-logic modules + unit tests)
    ↓
T011-T016 (US1 — fire-rate + lift + E2E) ──────────┐
    ↓                                               │
T017-T020 (US2 — regression detection)              │
    ↓                                               │
T021-T026 (US3 — domain isolation)                  │
    ↓                                               │
T027-T034 (CLI + eval.yaml configs + integration) ←─┘
```

T005-T010 parallelizable (different modules). T024-T026 parallelizable (different eval.yaml files). T028-T032 parallelizable (different eval.yaml files).

## MVP Scope

T001–T016 (Phases 1–3) deliver a working single-skill evaluator for `objectscript-review` with fire-rate + lift measurement. Can be run immediately with `--skill objectscript-review`. All remaining phases add regression detection, domain isolation, remaining skills, and the full CLI.

## Total: 34 tasks across 6 phases
- Phase 1 (Setup): 2 tasks
- Phase 2 (Foundational): 8 tasks
- Phase 3 (US1 — full eval): 6 tasks
- Phase 4 (US2 — regression): 4 tasks
- Phase 5 (US3 — isolation): 6 tasks
- Phase 6 (CLI + configs + integration): 8 tasks
