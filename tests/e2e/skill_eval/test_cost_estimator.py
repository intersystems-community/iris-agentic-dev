"""Unit tests for cost estimator — T007."""

from tests.e2e.skill_eval.cost_estimator import (
    _SECONDS_PER_TASK,
    estimate,
    estimate_skill,
    format_dry_run,
)
from tests.e2e.skill_eval.evaluator import SkillEvalConfig


def make_config(
    benchmark_tasks, domain_skill=False, implicit=False, skill="test-skill"
):
    return SkillEvalConfig(
        skill=skill,
        description="",
        fire_rate_prompt="Fix this",
        benchmark_tasks=benchmark_tasks,
        domain_skill=domain_skill,
        isolation_prompt="Fix Return-in-loop" if domain_skill else None,
        implicit_fire_rate_prompt="Something that does not name the skill"
        if implicit
        else None,
    )


def test_estimate_single_skill_with_benchmark():
    # 1 skill, 3 benchmark tasks, 3 runs
    # fire-rate: 3 runs = 3 task calls
    # lift: 3 benchmark tasks × 2 conditions × 3 runs = 18 task calls + 18 judge calls
    # total: 21 task calls, 18 judge calls
    configs = [make_config(["DBG-01", "DBG-02", "DBG-03"])]
    est = estimate(configs, runs=3)
    assert est["task_runs"] == 21
    assert est["judge_calls"] == 18
    assert est["cost_usd"] > 0
    assert est["time_minutes"] > 0


def test_estimate_domain_skill_no_benchmark():
    # domain skill: 3 fire-rate + 3 isolation = 6 task calls, 0 judge calls
    configs = [make_config([], domain_skill=True)]
    est = estimate(configs, runs=3)
    assert est["task_runs"] == 6
    assert est["judge_calls"] == 0


def test_estimate_multiple_skills():
    configs = [make_config(["DBG-01"]), make_config(["DBG-01", "DBG-02"])]
    est = estimate(configs, runs=3)
    # skill1: 3 fire + 1*2*3=6 benchmark + 6 judge
    # skill2: 3 fire + 2*2*3=12 benchmark + 12 judge
    assert est["task_runs"] == 3 + 6 + 3 + 12  # 24
    assert est["judge_calls"] == 6 + 12  # 18


def test_estimate_counts_the_implicit_fire_rate_runs():
    """The implicit fire-rate is a third full pass over the model, and it was free.

    `_run_skill` calls `measure_fire_rate` a second time whenever the config has an
    `implicit_fire_rate_prompt`, so a skill with one costs 2×runs before lift is even
    measured. The estimator only ever counted the explicit pass, which is how the nightly
    workflow came to be budgeted at 75 minutes for a suite that needs ~100: the estimate
    read 67.2 min, under the timeout, and the run was cancelled at 1h15m every night.
    """
    without = estimate([make_config([], implicit=False)], runs=5)
    with_implicit = estimate([make_config([], implicit=True)], runs=5)
    assert without["task_runs"] == 5
    assert with_implicit["task_runs"] == 10, (
        "a skill with an implicit_fire_rate_prompt runs the fire-rate probe twice"
    )
    assert with_implicit["time_minutes"] > without["time_minutes"]


def test_seconds_per_task_matches_a_measured_run():
    """Calibrated against run 34093536537 (2026-09-07): 185 sessions in 4,122 s = 22.3 s.

    The original 15 s was a guess made before the harness ever ran on a GHA runner. Every
    session is an opencode subprocess doing real tool calls against a container in the same
    VM; it does not finish in 15 s. If this constant drops back under 20 the whole budget
    silently understates again.
    """
    assert _SECONDS_PER_TASK >= 20


def test_estimate_skill_is_the_per_shard_budget():
    """One shard is one skill, so the per-skill estimate is what a shard timeout must cover."""
    configs = [
        make_config(["DBG-01"], skill="small"),
        make_config(["DBG-01", "DBG-02", "DBG-03"], domain_skill=True, skill="big"),
    ]
    per_skill = {c.skill: estimate_skill(c, runs=5) for c in configs}
    assert per_skill["small"]["task_runs"] == 5 + 1 * 2 * 5
    assert per_skill["big"]["task_runs"] == 5 + 5 + 3 * 2 * 5
    assert per_skill["big"]["time_minutes"] > per_skill["small"]["time_minutes"]
    # The whole-suite estimate is the sum of its shards — no double counting, no gaps.
    total = estimate(configs, runs=5)
    assert total["task_runs"] == sum(e["task_runs"] for e in per_skill.values())
    assert total["judge_calls"] == sum(e["judge_calls"] for e in per_skill.values())


def test_format_dry_run_contains_key_fields():
    configs = [make_config(["DBG-01"])]
    est = estimate(configs, runs=3)
    output = format_dry_run(est, n_covered=1, n_uncovered=23)
    assert "Estimated cost" in output
    assert "Run with --yes" in output
    assert "task runs" in output.lower() or "Task runs" in output
