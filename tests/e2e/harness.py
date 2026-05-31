"""E2E harness runner — executes a HarnessTask via a real OpenCode session."""
import os
import time
from typing import Any

from tests.e2e.isolated_env import IsolatedEnv
from tests.e2e.opencode_runner import collect_events, parse_mcp_tool
from tests.e2e.assertions import (
    extract_code_blocks,
    check_absent_pattern,
    check_tool_called,
    check_tools_in_order,
)
from tests.e2e.readme_validator import ReadmeValidator
from tests.e2e.result_writer import (
    AssertionResult,
    TaskResult,
    RunResult,
    write_result,
)
from tests.e2e.task_loader import HarnessTask, load_task, TASKS_DIR


def run_task(
    task: HarnessTask,
    openai_api_key: str,
    model: str = "openai/gpt-4o-mini",
    iris_host: str | None = None,
    iris_web_port: str | None = None,
    iris_container: str | None = None,
    keep_on_failure: bool = False,
    readme_path: str | None = None,
) -> TaskResult:
    start = time.time()
    with IsolatedEnv(openai_api_key=openai_api_key, keep_on_failure=keep_on_failure) as env:
        if iris_host and iris_web_port and iris_container:
            env.with_mcp(iris_host=iris_host, iris_web_port=iris_web_port, iris_container=iris_container)

        # Install skills from README curl commands
        if task.skills_to_install:
            validator = ReadmeValidator(skills_dir=env.skills_dir, readme_path=readme_path)
            for skill_name in task.skills_to_install:
                validator.install_skill(skill_name)

        # Run OpenCode — task.model overrides the caller's default
        effective_model = task.model or model
        events = collect_events(
            prompt=task.prompt,
            env_vars=env.env_vars(),
            model=effective_model,
        )

        # Collect text for code block extraction
        text_output = " ".join(
            e["part"].get("text", "")
            for e in events
            if e.get("type") == "text" and e.get("part", {}).get("time", {}).get("end")
        )
        code_blocks = extract_code_blocks(text_output)

        # Collect tool calls
        tool_calls_seen = []
        for e in events:
            if e.get("type") == "tool_use" and e.get("part", {}).get("state", {}).get("status") == "completed":
                tool_calls_seen.append(e["part"]["tool"])

        # Check skill_loaded: skill tool was invoked or skill name appears in text
        skill_loaded = any(
            "skill" in tc.lower() and any(s.replace("-", "_") in tc for s in task.skills_to_install)
            for tc in tool_calls_seen
        ) or any(
            skill in text_output
            for skill in task.skills_to_install
        )

        # Run assertions
        assertion_results = []
        all_required_passed = True
        for a in task.assertions:
            if a.type == "code_absent_pattern":
                passed = check_absent_pattern(code_blocks, a.pattern)
            elif a.type == "tool_called":
                server, tool = parse_mcp_tool(a.pattern)
                passed = check_tool_called(events, server, tool)
            elif a.type == "tool_output_contains":
                server, tool_name = parse_mcp_tool(a.pattern.split("|")[0])
                substring = a.pattern.split("|")[1] if "|" in a.pattern else ""
                passed = any(
                    e.get("part", {}).get("tool") == a.pattern.split("|")[0]
                    and substring in e.get("part", {}).get("state", {}).get("output", "")
                    for e in events if e.get("type") == "tool_use"
                )
            else:
                passed = False
            if a.required and not passed:
                all_required_passed = False
            assertion_results.append(AssertionResult(
                assertion_type=a.type,
                description=a.description,
                passed=passed,
            ))

        return TaskResult(
            task_id=task.id,
            scenario=task.scenario,
            condition=task.skills_to_install[0] if task.skills_to_install else "baseline",
            passed=all_required_passed,
            skill_loaded=skill_loaded,
            tool_calls=tool_calls_seen,
            assertion_results=assertion_results,
            llm_output_excerpt=text_output[:500],
            duration_seconds=time.time() - start,
        )
