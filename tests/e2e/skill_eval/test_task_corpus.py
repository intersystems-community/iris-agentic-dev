"""The task corpus must be parseable and self-consistent before any eval runs.

A fire-rate run once rewrote tests/e2e/tasks/skills/targeted/LIST-ITERATE.yaml —
the agent ran with the repo as its cwd and edited the fixture it was about to be
scored against. The corruption only surfaced 13 minutes later as a
yaml.scanner.ScannerError from the lift stage, which reads like a harness bug.
These tests run in the unit-test step and catch it in milliseconds instead.
"""
import glob
import os

import pytest
import yaml

# Imported rather than re-derived so a move of the benchmark corpus can't leave
# this test asserting against a path nothing else uses.
from tests.e2e.skill_eval.lift import _BENCHMARK_TASKS_DIR

_TASKS_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)
_TARGETED_DIR = os.path.join(_TASKS_SKILLS_DIR, "targeted")


def _targeted_task_files() -> list[str]:
    return sorted(glob.glob(os.path.join(_TARGETED_DIR, "*.yaml")))


def _eval_config_files() -> list[str]:
    return sorted(glob.glob(os.path.join(_TASKS_SKILLS_DIR, "*", "eval.yaml")))


def test_targeted_task_dir_is_not_empty():
    assert _targeted_task_files(), f"no task YAML found under {_TARGETED_DIR}"


@pytest.mark.parametrize("path", _targeted_task_files(), ids=os.path.basename)
def test_targeted_task_parses(path):
    with open(path) as f:
        try:
            task = yaml.safe_load(f)
        except yaml.YAMLError as e:
            pytest.fail(f"{os.path.basename(path)} is not valid YAML: {e}")
    assert isinstance(task, dict), f"{path} must parse to a mapping"


@pytest.mark.parametrize("path", _targeted_task_files(), ids=os.path.basename)
def test_targeted_task_has_the_fields_lift_reads(path):
    """measure_lift reads id, description and fixtures[].{type,name,content}."""
    with open(path) as f:
        task = yaml.safe_load(f)
    stem = os.path.splitext(os.path.basename(path))[0]
    assert task.get("id") == stem, f"id must match filename ({stem})"
    assert task.get("description"), "description is the agent prompt — cannot be empty"
    for fx in task.get("fixtures", []):
        assert fx.get("type"), f"{stem}: fixture missing type"
        assert fx.get("name"), f"{stem}: fixture missing name"
        if fx["type"] == "cls":
            assert isinstance(fx.get("content"), str) and fx["content"].strip(), (
                f"{stem}: cls fixture {fx['name']} has no content — a block scalar "
                f"that lost its indentation parses as something else entirely"
            )


@pytest.mark.parametrize("path", _eval_config_files(),
                         ids=lambda p: os.path.basename(os.path.dirname(p)))
def test_eval_config_parses(path):
    with open(path) as f:
        try:
            cfg = yaml.safe_load(f)
        except yaml.YAMLError as e:
            pytest.fail(f"{path} is not valid YAML: {e}")
    assert isinstance(cfg, dict), f"{path} must parse to a mapping"


@pytest.mark.parametrize("path", _eval_config_files(),
                         ids=lambda p: os.path.basename(os.path.dirname(p)))
def test_every_referenced_benchmark_task_exists(path):
    """A benchmark_tasks entry with no YAML fails mid-eval, not at load."""
    with open(path) as f:
        cfg = yaml.safe_load(f)
    for task_id in cfg.get("benchmark_tasks") or []:
        # Same two-step resolution measure_lift does.
        targeted = os.path.join(_TARGETED_DIR, f"{task_id}.yaml")
        fallback = os.path.join(_BENCHMARK_TASKS_DIR, f"{task_id}.yaml")
        assert os.path.exists(targeted) or os.path.exists(fallback), (
            f"{cfg.get('skill')} references task {task_id}, "
            f"which exists in neither {_TARGETED_DIR} nor {_BENCHMARK_TASKS_DIR}"
        )
