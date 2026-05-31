"""US3 E2E tests — full stack: skills + MCP + live IRIS."""
import os
import pytest
from tests.e2e.harness import run_task
from tests.e2e.task_loader import load_task, TASKS_DIR
from tests.e2e.fixtures import load_all_fixtures
from tests.e2e.assertions import check_tools_in_order
from tests.e2e.opencode_runner import collect_events
from tests.e2e.isolated_env import IsolatedEnv


@pytest.mark.us3
def test_us3_full_stack(openai_api_key, iris_available):
    """docs_introspect → generate → iris_compile chain must complete in order."""
    container = iris_available["container"]
    web_port = iris_available["web_port"]
    task = load_task(os.path.join(TASKS_DIR, "FULL-01.yaml"))

    load_all_fixtures(task.fixtures, iris_host="localhost", iris_web_port=web_port)

    result = run_task(
        task=task,
        openai_api_key=openai_api_key,
        iris_host="localhost",
        iris_web_port=web_port,
        iris_container=container,
    )

    assert "iris_agentic_dev:docs_introspect" in result.tool_calls, \
        f"docs_introspect not called. Tool calls: {result.tool_calls}"
    assert "iris_agentic_dev:iris_compile" in result.tool_calls, \
        f"iris_compile not called. Tool calls: {result.tool_calls}"
    assert result.passed, \
        f"Full-stack assertion failed: {[(a.description, a.passed) for a in result.assertion_results]}"
