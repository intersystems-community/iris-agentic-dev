# Feature Specification: Skill Regression and Lift Suite

**Feature Branch**: `040-skill-regression`
**Created**: 2026-05-31
**Status**: Draft
**Input**: Structured evaluation of all 24 skills in `light-skills/skills/` — fire-rate, lift vs baseline, regression detection, and negative-lift guard. Runs locally via the OpenCode harness (039). Results stored as comparable JSON for trend tracking.

## Clarifications

### Session 2026-05-31

- Q: Which pass/fail mechanism should lift measurement use — assertion patterns (039 harness) or the existing benchmark judge? → A: Existing benchmark judge (LLM scores 0–3, pass = score ≥ 2) — matches how leaderboard numbers were measured, ensures comparability.
- Q: How should skills map to benchmark tasks — auto-convention or explicit? → A: Explicit mapping in each skill's `eval.yaml` — author declares which benchmark task IDs apply. Prevents fragile auto-matching.
- Q: Must all 24 skills have eval.yaml before running, or can coverage be partial? → A: Partial coverage allowed — ship eval.yaml for P1 skills first (objectscript-review, objectscript-guardrails, objectscript-sql-patterns, objectscript-list-patterns, objectscript-unit-test). Remaining skills degrade to `no_task_coverage: true` and appear in a coverage gap report.
- Q: Should the suite have cost-awareness before running? → A: Yes — `--dry-run` flag prints estimated LLM call count and cost, requires `--yes` to actually execute. Prevents surprises on long/expensive runs.
- Q: Should the suite auto-update the README leaderboard when scores change? → A: No — suite outputs a diff report (old vs new scores) only; README update is a separate manual authoring step.

## Problem Statement

The skill leaderboard in `light-skills/README.md` records benchmark scores from a specific point in time against a specific model. There is no automated way to:

- Verify that a skill still fires when it should (LLM must invoke the `skill` tool on a matching task)
- Measure whether a skill's lift has changed as models improve or skills are edited
- Catch regressions — a skill that previously helped now hurts
- Verify that domain skills (vector, connectivity, Ensemble) don't load themselves on unrelated tasks

Without this, the leaderboard becomes stale and misleading. The fix: a runnable local suite that re-measures every skill and flags regressions.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Run the full skill evaluation suite (Priority: P1)

A developer wants to know whether all 24 skills are still behaving as expected after editing a skill, updating the model, or adding new benchmark tasks. They run a single command, wait for results, and get a per-skill report showing fire rate, lift, and any regressions since the last run.

**Why this priority**: This is the core value. Without it, skill quality is unknown.

**Independent Test**: Run suite against all 24 skills — JSON results file written — each skill entry has `fire_rate`, `pass_rate_baseline`, `pass_rate_skill`, `lift`, `regression_flag`.

**Acceptance Scenarios**:

1. **Given** all 24 skills are present in `light-skills/skills/`, **When** the suite runs, **Then** every skill produces a result entry with all four metrics populated (not null/unknown).
2. **Given** `objectscript-review` has a known lift of +29% from the leaderboard, **When** the suite runs with the same task class, **Then** the result shows `lift > 0` and `regression_flag: false`.
3. **Given** `objectscript-loop-patterns` had −19% lift historically, **When** the suite runs, **Then** the result captures the negative lift accurately and `regression_flag` reflects whether it changed direction.
4. **Given** a skill was edited since the last run, **When** results are compared to the stored baseline, **Then** the comparison report lists it under "changed" with old vs new lift.

### Edge Cases

- A skill has no matching benchmark task → suite skips lift measurement, records `lift: null`, still runs fire-rate test with a purpose-built task.
- A skill fires on 0/5 tasks (fire_rate = 0.0) → suite reports this without failing; it is data, not an error.
- The baseline file does not exist (first run) → suite writes results and marks `regression_flag: false` for all skills (no prior data to compare).

---

### User Story 2 — Detect a regressing skill (Priority: P1)

A developer edits `objectscript-review` to tighten the Return-in-loop rule. They re-run the suite and see `regression_flag: true` for that skill — the lift dropped from +29% to +10%. They investigate the edit.

**Why this priority**: Regressions are silent bugs. Without detection, a well-meaning edit can hurt users.

**Independent Test**: Manually lower the stored baseline lift for one skill, re-run the suite, assert `regression_flag: true` for that skill.

**Acceptance Scenarios**:

1. **Given** stored baseline shows `objectscript-review` at `lift: 0.29`, **When** the new run produces `lift: 0.10`, **Then** `regression_flag: true` appears and the delta is reported.
2. **Given** lift improved (0.29 → 0.35), **When** results are compared, **Then** `regression_flag: false` and `lift_delta: +0.06` is recorded.
3. **Given** a skill had no prior baseline entry, **When** results are written, **Then** `regression_flag: false` and `new_skill: true` is set.

---

### User Story 3 — Verify domain skill isolation (Priority: P2)

A developer confirms that `iris-vector-ai`, `iris-connectivity`, and `ensemble-production` do NOT self-activate on general ObjectScript repair tasks.

**Why this priority**: The harm from domain skills firing globally is documented (−19% lift pattern). This guard catches accidental trigger widening.

**Independent Test**: Run each domain skill against a standard repair task with no other skills installed — assert `fire_rate: 0.0`.

**Acceptance Scenarios**:

1. **Given** `iris-vector-ai` is the only installed skill, **When** the LLM is given an ObjectScript method repair task (no vector content), **Then** the `skill` tool is NOT called — `fire_rate: 0.0`.
2. **Given** `ensemble-production` is the only installed skill, **When** the LLM is given an ObjectScript SQL fix task, **Then** `fire_rate: 0.0`.
3. **Given** `iris-connectivity` is the only installed skill, **When** given a `%List` pattern task, **Then** `fire_rate: 0.0`.
4. **Given** a domain skill is given a matching task (e.g., `iris-vector-ai` with a VECTOR_COSINE query task), **When** the suite runs, **Then** `fire_rate > 0.0` — confirming the skill fires correctly in its own domain.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The suite MUST evaluate all skills present in `light-skills/skills/` — discovered by directory scan, not a hardcoded list.
- **FR-002**: For each skill, the suite MUST run the `fire_rate_prompt` field declared in `tests/e2e/tasks/skills/{skill-name}/eval.yaml` as the purpose-built fire-rate task designed to match the skill's trigger description.
- **FR-003**: For skills with benchmark task IDs declared in their `eval.yaml` (`benchmark_tasks` field), the suite MUST run both a baseline pass (no skill) and a skill pass on those tasks to compute lift. The mapping is explicit per skill — there is no auto-convention from skill name to task category.
- **FR-004**: Fire-rate MUST be computed as: (runs where `skill` tool was invoked) / (total runs). Minimum 3 runs per skill for statistical validity.
- **FR-005**: Lift MUST be computed as: `pass_rate_skill − pass_rate_baseline` across the matched benchmark tasks. Pass/fail verdict uses the existing benchmark judge (LLM scores 0–3; pass = score ≥ 2) — the same mechanism used to produce the leaderboard numbers in `light-skills/README.md`, ensuring results are directly comparable.
- **FR-006**: The suite MUST compare results to a stored baseline file (`tests/e2e/results/skill-baseline.json`). If the file is absent (first run), the suite MUST automatically create it from the current run's results, setting `regression_flag: false` for all skills, and print a notice: "No baseline found — created from this run."
- **FR-007**: `regression_flag` MUST be set `true` when `lift_delta < −0.05` (lift dropped more than 5 percentage points). Threshold is configurable via `--regression-threshold`.
- **FR-008**: Domain skills (those declared `domain_skill: true` in their `eval.yaml`) MUST be tested against their `isolation_prompt` (a general repair task unrelated to the skill's domain) and the fire rate on that task recorded separately as `isolation_fire_rate`. The `eval.yaml` flag is the authoritative runtime check.
- **FR-009**: Results MUST be written to `tests/e2e/results/skill-eval-{timestamp}.json` with schema: `{skill, fire_rate, isolation_fire_rate, pass_rate_baseline, pass_rate_skill, lift, lift_delta, regression_flag, new_skill, no_task_coverage, task_ids_used}`.
- **FR-010**: The suite MUST print a human-readable summary table to stdout: skill, lift, regression flag, sorted by lift descending.
- **FR-011**: The suite MUST support `--skill <name>` to run a single skill and `--category <name>` to run all skills in a category (e.g., objectscript, iris, domain).
- **FR-012**: Skills with no `eval.yaml` (or with an `eval.yaml` that declares no tasks) MUST produce a result with `fire_rate: null`, `lift: null`, `no_task_coverage: true` — not silently skipped. The initial implementation ships `eval.yaml` for the five P1 skills (objectscript-review, objectscript-guardrails, objectscript-sql-patterns, objectscript-list-patterns, objectscript-unit-test); all others start as `no_task_coverage: true`. A coverage gap report in the run summary lists uncovered skills with a prompt to add `eval.yaml`.
- **FR-013**: The baseline file MUST only be updated when `--update-baseline` is explicitly passed — never auto-updated, to prevent silent regression masking. When `--update-baseline` is passed, the suite MUST also print a diff report showing every skill's old vs new lift value. The README leaderboard is never touched by the suite — updating it is a separate manual step by the author.
- **FR-014**: The suite MUST support `--dry-run` — print the estimated LLM call count (task runs + judge calls) and approximate cost in USD before executing. Running the full suite without `--dry-run` MUST require explicit `--yes` confirmation or produce a cost estimate first and prompt for confirmation. This applies to full-suite runs only; `--skill <name>` single-skill runs do not require confirmation.

### Key Entities

- **SkillEvalConfig**: Stored in `tests/e2e/tasks/skills/{skill-name}/eval.yaml`. Fields: `skill` (name), `fire_rate_prompt` (task prompt for fire-rate test), `benchmark_tasks` (list of task IDs from `benchmark/021/tasks/` explicitly chosen by the skill author), `isolation_task_id` (for domain skills — ID of a general repair task that should NOT trigger this skill).
- **SkillResult**: Per-skill output — all fields from FR-009 schema.
- **EvalRun**: Full run output — `{run_id, model, timestamp, skills: [SkillResult], summary: {regressions, improvements, uncovered}}`.
- **BaselineFile**: Prior run used for regression comparison — `tests/e2e/results/skill-baseline.json`. Updated only with `--update-baseline`.

---

## Success Criteria *(mandatory)*

- **SC-001**: Running the suite produces a result entry for all 24 skills — zero silently skipped.
- **SC-002**: `objectscript-review` shows `lift > 0` and `regression_flag: false` against repair benchmark tasks.
- **SC-003**: All three domain skills show `isolation_fire_rate: 0.0` on general repair tasks.
- **SC-004**: A simulated regression (manually reduce stored baseline lift by 0.20) is detected in the next run — `regression_flag: true` for the affected skill.
- **SC-005**: Full suite (8 covered skills — 5 standard + 3 domain — at 3 runs each, plus 16 skills producing `no_task_coverage` entries with no LLM calls) completes in under 25 minutes locally.
- **SC-006**: Running the suite twice in succession produces lift values within ±10 percentage points (LLM non-determinism tolerance).
- **SC-007**: Running with `--update-baseline` produces a human-readable diff report listing every skill whose lift changed, with old value, new value, and delta. Skills with no change are omitted from the diff.

---

## Assumptions

- Skills are identified by subdirectory name under `light-skills/skills/` — each subdirectory containing a `SKILL.md` is one skill.
- Skills tagged `load-on-demand` in their SKILL.md frontmatter, or listed with ⚡ in `light-skills/README.md`, are domain skills subject to the isolation guard.
- Benchmark tasks in `benchmark/021/tasks/` have sufficient category coverage to map to most objectscript-* skills; remaining skills get purpose-built tasks only.
- Bedrock Sonnet is the evaluation model — changing models requires re-baselining.
- 3 runs per skill is the minimum for statistical validity; more runs increase confidence but raise cost.

---

## Out of Scope

- Automatically editing or improving skills based on results.
- Running in GitHub Actions CI.
- Multi-model comparison in a single run.
- Skills outside `light-skills/skills/`.
