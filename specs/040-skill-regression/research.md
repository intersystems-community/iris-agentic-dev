# Research: Skill Regression and Lift Suite (040)

**Date**: 2026-05-31
**Branch**: 040-skill-regression

---

## Decision 1: Pass/fail verdict mechanism

**Decision**: Reuse `benchmark/021/runner/judge.py` — `score_result(task, result)` returns `{score: 0-3, reasoning}`. Pass = score ≥ 2.

**Rationale**: The leaderboard numbers were produced with this judge. Reusing it makes new results directly comparable. The judge uses Claude Haiku via the same `_client.py` factory that handles Bedrock/direct API transparently.

**Interface**: `from benchmark.runner.judge import score_result` — takes a task dict (from YAML) and a result dict containing `transcript` (list of tool calls + final response), returns `{score, reasoning}`.

---

## Decision 2: Task execution mechanism

**Decision**: Drive OpenCode via the 039 harness (`tests/e2e/opencode_runner.collect_events`) for skill invocation, then format the event stream as a `transcript` for the judge.

**Rationale**: The 039 harness already handles isolated OpenCode runs with `OPENCODE_CONFIG_CONTENT` + `XDG_CONFIG_HOME`, skill injection via curl/local copy, and event stream parsing. The benchmark runner (`benchmark/021/runner/claude_code.py`) uses a different mechanism (direct Anthropic API + MCP subprocess). Using 039's OpenCode path tests the actual user experience; using the direct API would bypass skill loading entirely.

**Transcript format for judge**: Convert 039 event stream to judge-compatible transcript:
- `tool_use` events → tool call entries with input/output
- `text` events → final response text
- `tool_call_count` = count of non-skill tool_use events

---

## Decision 3: eval.yaml schema

**Decision**:
```yaml
skill: objectscript-review
description: "One-sentence description of what this skill does"
fire_rate_prompt: |
  Fix the bug in this ObjectScript method: ...
benchmark_tasks:
  - DBG-01
  - DBG-02
  - MOD-03
domain_skill: false  # true for load-on-demand skills
isolation_prompt: |   # only for domain skills
  Fix this ObjectScript Return-in-loop bug: ...
```

**Rationale**: Minimal schema. `fire_rate_prompt` is the dedicated trigger task (1 task, run N times to measure fire rate). `benchmark_tasks` lists IDs from `benchmark/021/tasks/` that this skill is expected to help with. `domain_skill: true` triggers the isolation guard.

---

## Decision 4: P1 skills and their benchmark task mappings

Based on the existing leaderboard and benchmark category analysis:

| Skill | Fire-rate trigger | Benchmark tasks | Domain? |
|-------|------------------|-----------------|---------|
| objectscript-review | Return-in-For loop fix | DBG-01, DBG-02, DBG-03, MOD-01, MOD-02 | No |
| objectscript-guardrails | ObjectScript method with common mistake | DBG-01, DBG-02, MOD-01 | No |
| objectscript-sql-patterns | SQL query with IRIS-specific quirk | GEN-03, GEN-04 (SQL tasks) | No |
| objectscript-list-patterns | %List operation task | MOD-03, MOD-04 | No |
| objectscript-unit-test | Write a %UnitTest for existing class | GEN-01, GEN-02 | No |
| iris-vector-ai | VECTOR_COSINE query | (purpose-built only) | Yes |
| iris-connectivity | Python IRIS connection | (purpose-built only) | Yes |
| ensemble-production | Production start/stop | (purpose-built only) | Yes |

Note: GEN/MOD/DBG categories confirmed by reading `benchmark/021/tasks/` — GEN = generation tasks, DBG = debug/fix tasks, MOD = modification tasks, LEG = legacy code, SCM = source control.

---

## Decision 5: Cost estimation

Per run estimate (Bedrock Sonnet task + Haiku judge):
- Task run: ~$0.01–0.03 (Sonnet, ~500 input + 200 output tokens)
- Judge call: ~$0.001 (Haiku, ~300 tokens)
- Per skill (3 fire-rate runs + 5 benchmark × 2 conditions × 3 runs = 33 calls): ~$0.35
- Full suite (5 P1 skills + 3 domain): ~$2.80 full, ~$1.40 skill-only path

Dry-run output format:
```
Skill eval dry run:
  Skills: 8 (5 with tasks, 3 domain, 16 no_task_coverage)
  Task runs: 126 (Sonnet)
  Judge calls: 90 (Haiku)
  Estimated cost: ~$3.20
  Estimated time: ~25 minutes
Run with --yes to proceed.
```

---

## Decision 6: Suite entry point

**Decision**: `python -m tests.e2e.skill_eval [options]` — new module alongside existing e2e harness.

**Key flags**:
- `--skill <name>` — single skill
- `--dry-run` — cost estimate, no execution
- `--yes` — skip confirmation prompt for full-suite runs
- `--update-baseline` — update baseline + print diff report
- `--regression-threshold <float>` — default 0.05
- `--runs <n>` — fire-rate runs per skill, default 3
- `--output <dir>` — results directory, default `tests/e2e/results/`

---

## Decision 7: IRIS container for benchmark tasks

**Decision**: Reuse existing `iris-dev-iris` container locally (same as 039 harness). Benchmark tasks target `BENCHMARK` namespace — need to ensure namespace exists or create it. The benchmark runner currently uses a hardcoded `BENCHMARK` namespace; the 039 harness defaults to `USER`. The skill eval suite will use `USER` namespace (consistent with 039) and adjust fixture loading accordingly — no need for a separate BENCHMARK namespace for this evaluation path.
