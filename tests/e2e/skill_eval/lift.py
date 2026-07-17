"""Lift measurement via OpenCode harness + benchmark judge — T014."""
import os
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillEvalConfig

_BENCHMARK_TASKS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "benchmark", "021", "tasks")
)
_SKILLS_PACK_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "skills", "skills")
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
        # Use a fresh temp workdir so previous eval artifacts don't pollute the session
        # Keep the workdir alive after collect_events so we can read written files
        import tempfile
        workdir_obj = tempfile.TemporaryDirectory(prefix="iad-eval-workdir-")
        workdir = workdir_obj.name
        try:
            events = collect_events(prompt, env.env_vars(), model=model, working_dir=workdir)
        finally:
            pass  # workdir_obj cleanup happens below

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
        written_content = _read_cls_files_from_workdir(workdir) or _extract_written_content(events)
        workdir_obj.cleanup()
        if written_content:
            expected = task_dict.get("expected_behavior", "")
            patterns_met = _check_expected_patterns(written_content, expected)
            tags = task_dict.get("tags", [])
            is_objectscript = any(t in tags for t in ["objectscript", "cls", "ens-director"]) or \
                              "Class " in written_content[:200]

            if is_objectscript:
                # ObjectScript: try to compile for verification
                compile_result = _try_compile_via_atelier(written_content, iris_host, iris_web_port)
                compiled_ok = "Compiled OK" in compile_result
                if compiled_ok and patterns_met:
                    score, reasoning = 3, "Compiled OK and meets expected behavior patterns"
                elif compiled_ok:
                    score, reasoning = 2, "Compiled OK but expected behavior patterns not fully met"
                elif patterns_met:
                    score, reasoning = 1, f"Expected patterns present but did not compile: {compile_result}"
                else:
                    score, reasoning = 0, f"Did not compile and patterns not met: {compile_result}"
            else:
                # Python / SQL / other: pattern-only scoring (no compile step)
                if patterns_met:
                    score, reasoning = 3, "All expected patterns present, no forbidden patterns found"
                else:
                    score, reasoning = 0, "Expected patterns missing or forbidden patterns present"
            return {"score": score, "reasoning": reasoning, "task_id": task_id, "condition": skill_name_or_none or "baseline"}
    try:
        workdir_obj.cleanup()
    except Exception:
        pass

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


def _check_expected_patterns(content: str, expected_behavior: str) -> bool:
    """Check required patterns present AND forbidden patterns absent in the written content."""
    import re

    # ── Required patterns: API names in backticks from expected_behavior ──
    api_names = re.findall(r'`([^`]+)`|\b(Ens\.Director\.\w+|##class\([^)]+\)|\$\$\$\w+)\b', expected_behavior)
    required = [a for pair in api_names for a in pair if a]

    # ── Forbidden patterns: anything after NOT in expected_behavior ──
    # Extract "NOT X" patterns — these MUST be absent
    forbidden_raw = re.findall(r'NOT\s+`([^`]+)`|NOT\s+([\w<>=:]+)', expected_behavior)
    forbidden = [a for pair in forbidden_raw for a in pair if a]

    # Explicit pgvector / wrong-syntax guards for vector tasks
    if "VECTOR_COSINE" in expected_behavior:
        forbidden += ["<=>", "<->", "::vector", "LIMIT "]  # LIMIT not TOP is pgvector style

    if required:
        hits = sum(1 for p in required if p in content)
        if hits < max(1, len(required) // 2):
            return False

    for bad in forbidden:
        if bad in content:
            return False

    return True


def _try_compile_via_atelier(cls_content: str, iris_host: str, iris_web_port: str) -> str:
    """Load and compile a class via Atelier REST. Returns 'compiled OK' or error string."""
    try:
        import requests, re
        # Extract class name from content
        m = re.search(r'^Class\s+([\w.]+)', cls_content, re.MULTILINE)
        if not m:
            return "Could not determine class name"
        cls_name = m.group(1)
        auth = ("_SYSTEM", "SYS")
        base = f"http://{iris_host}:{iris_web_port}/api/atelier/v1/USER"
        # Write the document
        r = requests.put(
            f"{base}/doc/{cls_name}.cls?ignoreConflict=1",
            json={"enc": False, "content": cls_content.splitlines()},
            auth=auth, timeout=30
        )
        if r.status_code not in (200, 201):
            return f"PUT failed: HTTP {r.status_code}"
        # Compile
        r2 = requests.post(f"{base}/action/compile", json=[f"{cls_name}.cls"], auth=auth, timeout=30)
        result = r2.json().get("result", {})
        errors = [s for s in result.get("status", []) if "ERROR" in str(s).upper()]
        if errors:
            return f"Compile errors: {errors[:2]}"
        return f"Compiled OK: {cls_name}"
    except Exception as e:
        return f"Compile check failed: {e}"


def _read_cls_files_from_workdir(workdir: str) -> str:
    """Read all code files written to the workdir during the eval session."""
    contents = []
    for root, _, files in os.walk(workdir):
        for f in sorted(files):
            if f.endswith((".cls", ".py", ".sql")):
                try:
                    path = os.path.join(root, f)
                    text = open(path).read()
                    if len(text) > 50:
                        contents.append(text)
                except Exception:
                    pass
    return "\n\n".join(contents)


def _extract_written_content(events: list[dict]) -> str:
    """Extract code content from write/edit tool calls — used in no-MCP light-skills mode."""
    best = ""
    for event in events:
        if event.get("type") != "tool_use":
            continue
        part = event.get("part", {})
        state = part.get("state", {})
        if state.get("status") != "completed":
            continue
        tool = part.get("tool", "")
        inp = state.get("input", {})
        if tool == "write":
            content = inp.get("content", "")
            if content and len(content) > len(best):
                best = content
        elif tool == "edit":
            # Edit gives new_string — take it if substantial
            new_str = inp.get("new_string", "")
            if new_str and len(new_str) > len(best):
                best = new_str
    if best:
        return best
    # Fall back: extract fenced code blocks from text output (objectscript/cls/sql blocks)
    import re as _re
    texts = [
        e["part"].get("text", "")
        for e in events
        if e.get("type") == "text" and e.get("part", {}).get("time", {}).get("end")
    ]
    full_text = "\n".join(texts)
    # Prefer objectscript/cls code blocks; fall back to any code block
    for lang in ["objectscript", "cls", "sql", ""]:
        pattern = rf"```{lang}\n(.*?)```" if lang else r"```\w*\n(.*?)```"
        blocks = _re.findall(pattern, full_text, _re.DOTALL)
        if blocks:
            return max(blocks, key=len)  # return the largest code block
    return full_text  # last resort: return all text


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
