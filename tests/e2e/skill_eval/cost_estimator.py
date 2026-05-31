"""Dry-run cost estimator — T008."""
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillEvalConfig

# Approximate costs per call (Bedrock Sonnet + Haiku)
_SONNET_COST = 0.020   # ~$0.020 per task run (500 input + 300 output tokens)
_HAIKU_COST = 0.001    # ~$0.001 per judge call
_SECONDS_PER_TASK = 15
_SECONDS_PER_JUDGE = 3


def estimate(configs: "list[SkillEvalConfig]", runs: int = 3) -> dict:
    """Estimate LLM call count and cost for the given skill configs."""
    task_runs = 0
    judge_calls = 0
    for cfg in configs:
        # Fire-rate: N runs per skill
        task_runs += runs
        if cfg.domain_skill and cfg.isolation_prompt:
            # Isolation: N additional runs
            task_runs += runs
        # Lift: benchmark_tasks × 2 conditions × runs
        n_benchmark = len(cfg.benchmark_tasks)
        if n_benchmark > 0:
            task_runs += n_benchmark * 2 * runs
            judge_calls += n_benchmark * 2 * runs
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
