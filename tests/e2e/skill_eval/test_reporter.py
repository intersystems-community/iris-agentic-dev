"""Unit tests for reporter — T009."""
import json
import os
import tempfile
import pytest
from tests.e2e.skill_eval.reporter import print_summary, write_result, EvalRun
from tests.e2e.skill_eval.evaluator import SkillResult


def make_result(skill, lift=None, fire_rate=1.0, regression=False, no_coverage=False):
    return SkillResult(
        skill=skill,
        fire_rate=fire_rate if not no_coverage else None,
        isolation_fire_rate=None,
        pass_rate_baseline=0.71 if lift else None,
        pass_rate_skill=(0.71 + lift) if lift else None,
        lift=lift,
        lift_delta=None,
        regression_flag=regression,
        new_skill=False,
        no_task_coverage=no_coverage,
        task_ids_used=["DBG-01"] if lift else [],
    )


def make_eval_run(skills):
    return EvalRun(
        run_id="2026-05-31T000000",
        model="amazon-bedrock/test",
        judge_model="amazon-bedrock/haiku",
        timestamp="2026-05-31T00:00:00Z",
        regression_threshold=0.05,
        skills=skills,
        summary={
            "regressions": [],
            "improvements": [],
            "uncovered": ["iris-docs"],
            "estimated_cost_usd": 1.50,
        },
    )


def test_print_summary_contains_skill_name(capsys):
    run = make_eval_run([make_result("objectscript-review", lift=0.29)])
    print_summary(run)
    captured = capsys.readouterr()
    assert "objectscript-review" in captured.out
    assert "0.29" in captured.out or "29" in captured.out


def test_print_summary_shows_regression_flag(capsys):
    run = make_eval_run([make_result("objectscript-review", lift=0.10, regression=True)])
    print_summary(run)
    captured = capsys.readouterr()
    assert "REGRESSION" in captured.out or "✗" in captured.out or "regression" in captured.out.lower()


def test_write_result_creates_json(tmp_path):
    run = make_eval_run([make_result("objectscript-review", lift=0.29)])
    path = write_result(run, str(tmp_path))
    assert os.path.exists(path)
    with open(path) as f:
        data = json.load(f)
    assert data["run_id"] == "2026-05-31T000000"
    assert len(data["skills"]) == 1
    assert data["skills"][0]["skill"] == "objectscript-review"
    assert data["summary"]["estimated_cost_usd"] == pytest.approx(1.50)
