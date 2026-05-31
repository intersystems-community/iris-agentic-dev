"""Unit tests for cost estimator — T007."""
import pytest
from tests.e2e.skill_eval.cost_estimator import estimate, format_dry_run
from tests.e2e.skill_eval.evaluator import SkillEvalConfig


def make_config(benchmark_tasks, domain_skill=False):
    return SkillEvalConfig(
        skill="test-skill",
        description="",
        fire_rate_prompt="Fix this",
        benchmark_tasks=benchmark_tasks,
        domain_skill=domain_skill,
        isolation_prompt="Fix Return-in-loop" if domain_skill else None,
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


def test_format_dry_run_contains_key_fields():
    configs = [make_config(["DBG-01"])]
    est = estimate(configs, runs=3)
    output = format_dry_run(est, n_covered=1, n_uncovered=23)
    assert "Estimated cost" in output
    assert "Run with --yes" in output
    assert "task runs" in output.lower() or "Task runs" in output
