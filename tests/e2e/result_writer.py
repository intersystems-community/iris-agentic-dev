"""RunResult data classes and JSON writer."""
import dataclasses
import json
import os
from typing import Any


@dataclasses.dataclass
class AssertionResult:
    assertion_type: str
    description: str
    passed: bool
    detail: str = ""


@dataclasses.dataclass
class TaskResult:
    task_id: str
    scenario: str
    condition: str
    passed: bool
    skill_loaded: bool
    tool_calls: list[str]
    assertion_results: list[AssertionResult]
    llm_output_excerpt: str
    duration_seconds: float


@dataclasses.dataclass
class RunResult:
    run_id: str
    harness: str
    opencode_version: str
    iris_agentic_dev_version: str
    model: str
    tasks: list[TaskResult]


def compute_lift(baseline: list[TaskResult], skill: list[TaskResult]) -> float | None:
    if not baseline or not skill:
        return None
    baseline_rate = sum(1 for t in baseline if t.passed) / len(baseline)
    skill_rate = sum(1 for t in skill if t.passed) / len(skill)
    return skill_rate - baseline_rate


def _to_dict(obj: Any) -> Any:
    if dataclasses.is_dataclass(obj):
        return {k: _to_dict(v) for k, v in dataclasses.asdict(obj).items()}
    if isinstance(obj, list):
        return [_to_dict(i) for i in obj]
    return obj


def write_result(result: RunResult, output_dir: str) -> str:
    os.makedirs(output_dir, exist_ok=True)
    data = _to_dict(result)
    tasks = result.tasks
    pass_rate = sum(1 for t in tasks if t.passed) / len(tasks) if tasks else 0.0
    tool_calls_observed = sorted({tc for t in tasks for tc in t.tool_calls})
    by_scenario: dict[str, list[bool]] = {}
    for t in tasks:
        by_scenario.setdefault(t.scenario, []).append(t.passed)
    data["summary"] = {
        "pass_rate": pass_rate,
        "tool_calls_observed": tool_calls_observed,
        "by_scenario": {k: sum(v) / len(v) for k, v in by_scenario.items()},
    }
    path = os.path.join(output_dir, f"{result.run_id}.json")
    with open(path, "w") as f:
        json.dump(data, f, indent=2)
    return path
