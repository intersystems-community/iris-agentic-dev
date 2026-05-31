"""Lift measurement via OpenCode harness + benchmark judge — T014."""
import os
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillEvalConfig

_BENCHMARK_TASKS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "benchmark", "021", "tasks")
)
_LIGHT_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "light-skills", "skills")
)


def compute_pass_rate(scores: list[dict]) -> float:
    """Pass = score >= 2."""
    if not scores:
        return 0.0
    passed = sum(1 for s in scores if s.get("score", 0) >= 2)
    return passed / len(scores)


def compute_lift_from_scores(baseline_scores: list[dict], skill_scores: list[dict]) -> dict:
    pr_baseline = compute_pass_rate(baseline_scores)
    pr_skill = compute_pass_rate(skill_scores)
    return {
        "pass_rate_baseline": round(pr_baseline, 4),
        "pass_rate_skill": round(pr_skill, 4),
        "lift": round(pr_skill - pr_baseline, 4),
    }


def format_transcript(events: list[dict]) -> str:
    """Format OpenCode event stream as a judge-compatible transcript string."""
    lines = []
    for event in events:
        if event.get("type") == "tool_use":
            part = event["part"]
            tool = part.get("tool", "")
            state = part.get("state", {})
            inp = str(state.get("input", {}))[:200]
            out = str(state.get("output", ""))[:300]
            lines.append(f"[tool_call: {tool}]\nInput: {inp}\nOutput: {out}")
        elif event.get("type") == "text":
            part = event["part"]
            if part.get("time", {}).get("end"):
                lines.append(f"[response]\n{part.get('text', '')}")
    tool_count = sum(
        1 for e in events
        if e.get("type") == "tool_use"
        and e["part"].get("state", {}).get("status") == "completed"
        and e["part"].get("tool") != "skill"
    )
    lines.append(f"\n[tool_call_count: {tool_count}]")
    return "\n\n".join(lines)


def run_task_and_score(
    task_id: str,
    skill_name_or_none,
    openai_api_key: str,
    model: str,
    iris_host: str = "localhost",
    iris_web_port: str = "52780",
    iris_container: str = "iris-dev-iris",
) -> dict:
    """Run a benchmark task via OpenCode and return the judge score."""
    import yaml
    from tests.e2e.isolated_env import IsolatedEnv
    from tests.e2e.opencode_runner import collect_events
    from tests.e2e.fixtures import load_all_fixtures
    from tests.e2e.task_loader import HarnessFixture
    from fire_rate import _install_skill_local

    # Ensure benchmark judge is importable
    import tests.e2e.skill_eval  # triggers sys.path shim
    from runner.judge import score_result

    task_path = os.path.join(_BENCHMARK_TASKS_DIR, f"{task_id}.yaml")
    with open(task_path) as f:
        task_dict = yaml.safe_load(f)

    # Load fixtures into IRIS
    fixtures = [
        HarnessFixture(type=fx["type"], name=fx["name"], content=fx["content"])
        for fx in task_dict.get("fixtures", [])
    ]
    if fixtures:
        load_all_fixtures(fixtures, iris_host=iris_host, iris_web_port=iris_web_port)

    prompt = task_dict["description"]

    with IsolatedEnv(openai_api_key=openai_api_key) as env:
        if skill_name_or_none:
            env_with_mcp = env.with_mcp(
                iris_host=iris_host,
                iris_web_port=iris_web_port,
                iris_container=iris_container,
            )
            try:
                from tests.e2e.readme_validator import ReadmeValidator
                ReadmeValidator(skills_dir=env.skills_dir).install_skill(skill_name_or_none)
            except (ValueError, Exception):
                _install_skill_local(skill_name_or_none, env.skills_dir)
        else:
            env.with_mcp(
                iris_host=iris_host,
                iris_web_port=iris_web_port,
                iris_container=iris_container,
            )
        events = collect_events(prompt, env.env_vars(), model=model)

    transcript = format_transcript(events)
    result = {"transcript": transcript, "tool_call_count": sum(
        1 for e in events
        if e.get("type") == "tool_use"
        and e["part"].get("state", {}).get("status") == "completed"
        and e["part"].get("tool") != "skill"
    ), "path": "B"}
    scored = score_result(task_dict, result)
    return {**scored, "task_id": task_id, "condition": skill_name_or_none or "baseline"}


def measure_lift(
    config: "SkillEvalConfig",
    n_runs: int,
    openai_api_key: str,
    model: str,
    iris_host: str = "localhost",
    iris_web_port: str = "52780",
    iris_container: str = "iris-dev-iris",
) -> dict:
    """Run all benchmark tasks baseline + skill and compute lift."""
    baseline_scores = []
    skill_scores = []
    task_ids_used = []
    for task_id in config.benchmark_tasks:
        for _ in range(n_runs):
            b = run_task_and_score(
                task_id, None, openai_api_key, model, iris_host, iris_web_port, iris_container
            )
            baseline_scores.append(b)
            s = run_task_and_score(
                task_id, config.skill, openai_api_key, model, iris_host, iris_web_port, iris_container
            )
            skill_scores.append(s)
        task_ids_used.append(task_id)
    result = compute_lift_from_scores(baseline_scores, skill_scores)
    result["task_ids_used"] = task_ids_used
    return result
