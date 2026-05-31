"""Unit tests for ResultWriter — T013."""
import json
import os
import pytest
from tests.e2e.result_writer import write_result, compute_lift, RunResult, TaskResult


def make_task_result(task_id: str, condition: str, passed: bool) -> TaskResult:
    return TaskResult(
        task_id=task_id,
        scenario="us1_skills_only",
        condition=condition,
        passed=passed,
        skill_loaded=condition != "baseline",
        tool_calls=[],
        assertion_results=[],
        llm_output_excerpt="...",
        duration_seconds=1.0,
    )


def make_run_result(**kwargs) -> RunResult:
    defaults = dict(
        run_id="2026-05-31T000000",
        harness="e2e-opencode",
        opencode_version="0.1.0",
        iris_agentic_dev_version="0.6.11",
        model="openai/gpt-4o-mini",
        tasks=[],
    )
    defaults.update(kwargs)
    return RunResult(**defaults)


def test_write_result_creates_json(tmp_path):
    result = make_run_result(run_id="test-run-001")
    path = write_result(result, str(tmp_path))
    assert os.path.exists(path)
    with open(path) as f:
        data = json.load(f)
    assert data["run_id"] == "test-run-001"
    assert data["harness"] == "e2e-opencode"


def test_write_result_schema(tmp_path):
    result = make_run_result(
        tasks=[make_task_result("SKILL-01", "objectscript-review", True)],
    )
    path = write_result(result, str(tmp_path))
    with open(path) as f:
        data = json.load(f)
    assert "tasks" in data
    assert "summary" in data
    assert data["tasks"][0]["task_id"] == "SKILL-01"
    assert data["tasks"][0]["passed"] is True


def test_compute_lift_with_data():
    baseline = [make_task_result("SKILL-01", "baseline", False),
                make_task_result("SKILL-01", "baseline", False)]
    skill = [make_task_result("SKILL-01", "objectscript-review", True),
             make_task_result("SKILL-01", "objectscript-review", True)]
    lift = compute_lift(baseline, skill)
    assert lift == pytest.approx(1.0)


def test_compute_lift_zero():
    baseline = [make_task_result("SKILL-01", "baseline", True)]
    skill = [make_task_result("SKILL-01", "objectscript-review", True)]
    lift = compute_lift(baseline, skill)
    assert lift == pytest.approx(0.0)


def test_compute_lift_none_when_empty():
    assert compute_lift([], []) is None


def test_summary_includes_pass_rate(tmp_path):
    result = make_run_result(tasks=[
        make_task_result("SKILL-01", "objectscript-review", True),
        make_task_result("MCP-01", "baseline", False),
    ])
    path = write_result(result, str(tmp_path))
    with open(path) as f:
        data = json.load(f)
    assert data["summary"]["pass_rate"] == pytest.approx(0.5)
