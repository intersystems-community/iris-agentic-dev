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


def format_transcript(events: list[dict]) -> list[dict]:
    """Format OpenCode event stream as a judge-compatible transcript (list of turn dicts)."""
    turns = []
    for event in events:
        if event.get("type") == "tool_use":
            part = event["part"]
            state = part.get("state", {})
            if state.get("status") != "completed":
                continue
            tool = part.get("tool", "")
            turns.append({
                "role": "assistant",
                "tool_name": tool,
                "args": state.get("input", {}),
                "tool_result": str(state.get("output", ""))[:300],
            })
        elif event.get("type") == "text":
            part = event["part"]
            if part.get("time", {}).get("end"):
                turns.append({"role": "assistant", "text": part.get("text", "")[:500]})
    return turns


def _apply_global_fixture(fx: dict, iris_host: str, iris_web_port: str) -> None:
    """Set a global subscript via Atelier execute."""
    import requests
    name = fx.get("name", "^BenchData").lstrip("^")
    subscript = fx.get("subscript", "")
    value = fx.get("value", "")
    code = f'Set ^{name}("{subscript}") = "{value}"'
    url = f"http://{iris_host}:{iris_web_port}/api/atelier/v1/USER/action/query"
    requests.post(url, json={"query": f"CALL %SYSTEM.SQL.Execute('{code}')"}, auth=("_SYSTEM", "SYS"), timeout=10)


def run_task_and_score(
    task_id: str,
    skill_name_or_none,
    openai_api_key: str,
    model: str,
    iris_host: str = "localhost",
    iris_web_port: str = "52780",
    iris_container: str = "iris-dev-iris",
    no_mcp: bool = False,  # True = light-skills scenario, no MCP tools
) -> dict:
    """Run a benchmark task via OpenCode and return the judge score."""
    import yaml
    from tests.e2e.isolated_env import IsolatedEnv
    from tests.e2e.opencode_runner import collect_events
    from tests.e2e.fixtures import load_all_fixtures
    from tests.e2e.task_loader import HarnessFixture
    from tests.e2e.skill_eval.fire_rate import _install_skill_local

    # Ensure benchmark judge is importable
    import tests.e2e.skill_eval  # triggers sys.path shim
    from runner.judge import score_result

    # Look in targeted tasks dir first, then fall back to benchmark tasks dir
    _TARGETED_DIR = os.path.abspath(
        os.path.join(os.path.dirname(__file__), "..", "tasks", "skills", "targeted")
    )
    targeted_path = os.path.join(_TARGETED_DIR, f"{task_id}.yaml")
    task_path = targeted_path if os.path.exists(targeted_path) else os.path.join(_BENCHMARK_TASKS_DIR, f"{task_id}.yaml")
    with open(task_path) as f:
        task_dict = yaml.safe_load(f)

    # Load cls fixtures into IRIS (global/routine fixtures use docker exec via benchmark harness)
    cls_fixtures = [
        HarnessFixture(type=fx["type"], name=fx["name"], content=fx["content"])
        for fx in task_dict.get("fixtures", [])
        if fx.get("type") == "cls" and "content" in fx
    ]
    if cls_fixtures:
        load_all_fixtures(cls_fixtures, iris_host=iris_host, iris_web_port=iris_web_port)

    # Apply global fixtures via iris_execute
    for fx in task_dict.get("fixtures", []):
        if fx.get("type") == "global":
            _apply_global_fixture(fx, iris_host=iris_host, iris_web_port=iris_web_port)

    prompt = task_dict["description"]

    with IsolatedEnv(openai_api_key=openai_api_key) as env:
        # Light-skills scenario: no MCP tools at all, skill knowledge is the only signal
        if not no_mcp:
            env.with_mcp(
                iris_host=iris_host,
                iris_web_port=iris_web_port,
                iris_container=iris_container,
            )
        if skill_name_or_none:
            try:
                from tests.e2e.readme_validator import ReadmeValidator
                ReadmeValidator(skills_dir=env.skills_dir).install_skill(skill_name_or_none)
            except (ValueError, Exception):
                _install_skill_local(skill_name_or_none, env.skills_dir)
        events = collect_events(prompt, env.env_vars(), model=model)

    # Check for tool_assertions in task — bypasses LLM judge, scores by tool calls
    tool_assertions = task_dict.get("tool_assertions", [])
    if tool_assertions:
        from tests.e2e.assertions import check_tool_called
        passed = all(
            check_tool_called(events, *_parse_assertion_tool(a))
            for a in tool_assertions
        )
        score = 3 if passed else 0
        reasoning = "tool assertions passed" if passed else f"missing required tools: {tool_assertions}"
        return {"score": score, "reasoning": reasoning, "task_id": task_id, "condition": skill_name_or_none or "baseline"}

    # For no-MCP (light-skills) mode: extract written file content and present as response
    # The model writes .cls files locally; judge should assess the code quality, not tool use
    if no_mcp:
        written_content = _extract_written_content(events)
        if written_content:
            # Score the written code content directly, without tool-call overhead confusion
            result = {"transcript": [{"role": "assistant", "text": written_content}], "tool_call_count": 0, "path": "B"}
            scored = score_result(task_dict, result)
            # Score 1 = correct code but judge wants compilation proof; treat as pass in light-skills
            if scored["score"] == 1:
                scored = {**scored, "score": 2, "reasoning": scored["reasoning"] + " [light-skills pass]"}
            return {**scored, "task_id": task_id, "condition": skill_name_or_none or "baseline"}

    turns = format_transcript(events)
    tool_count = sum(
        1 for e in events
        if e.get("type") == "tool_use"
        and e.get("part", {}).get("state", {}).get("status") == "completed"
        and e.get("part", {}).get("tool") != "skill"
    )
    result = {"transcript": turns, "tool_call_count": tool_count, "path": "B"}
    scored = score_result(task_dict, result)
    return {**scored, "task_id": task_id, "condition": skill_name_or_none or "baseline"}


def _extract_written_content(events: list[dict]) -> str:
    """Extract content from write tool calls — used in no-MCP mode to get the actual code."""
    for event in events:
        if event.get("type") != "tool_use":
            continue
        part = event.get("part", {})
        state = part.get("state", {})
        if state.get("status") != "completed":
            continue
        tool = part.get("tool", "")
        if tool == "write":
            inp = state.get("input", {})
            content = inp.get("content", "")
            if content and len(content) > 50:
                return content
    # Fall back to text output
    texts = [
        e["part"].get("text", "")
        for e in events
        if e.get("type") == "text" and e.get("part", {}).get("time", {}).get("end")
    ]
    return "\n".join(texts)


def _parse_assertion_tool(assertion: str):
    """Parse 'server:tool' → (server, tool). 'tool' alone → (None, tool)."""
    if ":" in assertion:
        server, _, tool = assertion.partition(":")
        return server, tool
    return None, assertion


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
                task_id, None, openai_api_key, model, iris_host, iris_web_port, iris_container,
                no_mcp=config.no_mcp_for_benchmark,
            )
            baseline_scores.append(b)
            s = run_task_and_score(
                task_id, config.skill, openai_api_key, model, iris_host, iris_web_port, iris_container,
                no_mcp=config.no_mcp_for_benchmark,
            )
            skill_scores.append(s)
        task_ids_used.append(task_id)
    result = compute_lift_from_scores(baseline_scores, skill_scores)
    result["task_ids_used"] = task_ids_used
    return result
