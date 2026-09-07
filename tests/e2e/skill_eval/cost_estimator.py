"""Dry-run cost estimator — T008."""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillEvalConfig

# Approximate costs per call (Bedrock Sonnet + Haiku)
_SONNET_COST = 0.020  # ~$0.020 per task run (500 input + 300 output tokens)
_HAIKU_COST = 0.001  # ~$0.001 per judge call

# Measured, not guessed: GHA run 34093536537 (2026-09-07) completed 185 opencode sessions in
# 4,122 s of wall clock — 22.3 s each. The old value of 15 s was set before the harness had
# ever run on a GHA runner, and it is what put the nightly estimate (67 min) under the job
# timeout (75 min) for a suite that needs about 100.
_SECONDS_PER_TASK = 22
_SECONDS_PER_JUDGE = 3


def estimate_skill(cfg: "SkillEvalConfig", runs: int = 3) -> dict:
    """Estimate one skill's cost. This is the per-shard budget in the nightly workflow."""
    # Fire-rate: N runs.
    task_runs = runs
    # Implicit fire-rate: `_run_skill` calls measure_fire_rate a second time with the prompt
    # that does not name the skill. Same cost as the explicit pass.
    if cfg.implicit_fire_rate_prompt:
        task_runs += runs
    if cfg.domain_skill and cfg.isolation_prompt:
        task_runs += runs
    # Lift: benchmark_tasks × 2 conditions × runs, each scored by the judge.
    judge_calls = len(cfg.benchmark_tasks) * 2 * runs
    task_runs += judge_calls
    total_seconds = task_runs * _SECONDS_PER_TASK + judge_calls * _SECONDS_PER_JUDGE
    return {
        "task_runs": task_runs,
        "judge_calls": judge_calls,
        "cost_usd": round(task_runs * _SONNET_COST + judge_calls * _HAIKU_COST, 2),
        "time_minutes": round(total_seconds / 60, 1),
    }


def estimate(configs: "list[SkillEvalConfig]", runs: int = 3) -> dict:
    """Estimate LLM call count and cost for the given skill configs."""
    task_runs = 0
    judge_calls = 0
    for cfg in configs:
        per_skill = estimate_skill(cfg, runs=runs)
        task_runs += per_skill["task_runs"]
        judge_calls += per_skill["judge_calls"]
    total_seconds = task_runs * _SECONDS_PER_TASK + judge_calls * _SECONDS_PER_JUDGE
    cost = task_runs * _SONNET_COST + judge_calls * _HAIKU_COST
    return {
        "task_runs": task_runs,
        "judge_calls": judge_calls,
        "cost_usd": round(cost, 2),
        "time_minutes": round(total_seconds / 60, 1),
    }


def format_dry_run(est: dict, n_covered: int, n_uncovered: int) -> str:
    lines = [
        "Skill eval dry run:",
        f"  Skills with eval.yaml: {n_covered}",
        f"  Skills without coverage: {n_uncovered}",
        f"  Task runs (Sonnet): {est['task_runs']}",
        f"  Judge calls (Haiku): {est['judge_calls']}",
        f"  Estimated cost: ~${est['cost_usd']:.2f} USD",
        f"  Estimated time: ~{est['time_minutes']} min",
        "",
        "Run with --yes to proceed, or --skill <name> for a single skill.",
    ]
    return "\n".join(lines)
