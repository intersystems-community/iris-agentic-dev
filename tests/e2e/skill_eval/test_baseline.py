"""Unit tests for baseline — T005."""
import json
import os
import tempfile
import pytest
from tests.e2e.skill_eval.baseline import load_baseline, save_baseline, compute_diff
from tests.e2e.skill_eval.evaluator import SkillResult


def make_result(skill, lift, fire_rate=1.0):
    return SkillResult(
        skill=skill,
        fire_rate=fire_rate,
        isolation_fire_rate=None,
        pass_rate_baseline=0.71,
        pass_rate_skill=0.71 + lift if lift else None,
        lift=lift,
        lift_delta=None,
        regression_flag=False,
        new_skill=False,
        no_task_coverage=lift is None,
        task_ids_used=["DBG-01"] if lift else [],
    )


def test_load_baseline_missing_returns_empty(tmp_path):
    result = load_baseline(str(tmp_path / "nonexistent.json"))
    assert result == {}


def test_save_and_load_roundtrip(tmp_path):
    path = str(tmp_path / "baseline.json")
    results = [make_result("objectscript-review", 0.29), make_result("objectscript-guardrails", 0.14)]
    save_baseline(results, path)
    loaded = load_baseline(path)
    assert loaded["objectscript-review"]["lift"] == pytest.approx(0.29)
    assert loaded["objectscript-guardrails"]["lift"] == pytest.approx(0.14)


def test_compute_diff_detects_regression():
    old = {"objectscript-review": {"lift": 0.29}}
    new = [make_result("objectscript-review", 0.10)]
    diff = compute_diff(old, new)
    assert len(diff) == 1
    assert diff[0]["skill"] == "objectscript-review"
    assert diff[0]["old_lift"] == pytest.approx(0.29)
    assert diff[0]["new_lift"] == pytest.approx(0.10)
    assert diff[0]["delta"] == pytest.approx(-0.19, abs=0.01)


def test_compute_diff_marks_new_skill():
    old = {}
    new = [make_result("objectscript-review", 0.29)]
    diff = compute_diff(old, new)
    assert diff[0]["new_skill"] is True


def test_compute_diff_omits_no_change():
    old = {"objectscript-review": {"lift": 0.29}}
    new = [make_result("objectscript-review", 0.29)]
    diff = compute_diff(old, new)
    assert len(diff) == 0


def test_compute_diff_sorted_by_abs_delta():
    old = {"a": {"lift": 0.5}, "b": {"lift": 0.5}}
    new = [make_result("a", 0.1), make_result("b", 0.45)]
    diff = compute_diff(old, new)
    assert diff[0]["skill"] == "a"  # bigger delta first
