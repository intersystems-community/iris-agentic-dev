"""US2 E2E tests — MCP tools against live IRIS. Requires IRIS_CONTAINER."""
import os
import pytest
from tests.e2e.harness import run_task
from tests.e2e.task_loader import load_task, TASKS_DIR
from tests.e2e.fixtures import load_all_fixtures
from tests.e2e.opencode_runner import collect_events
from tests.e2e.assertions import check_tool_called
from tests.e2e.isolated_env import IsolatedEnv


@pytest.mark.us2
def test_us2_check_config(openai_api_key, iris_available):
    """check_config tool must return connected=true."""
    container = iris_available["container"]
    web_port = iris_available["web_port"]
    with IsolatedEnv(openai_api_key=openai_api_key) as env:
        env.with_mcp(iris_host="localhost", iris_web_port=web_port, iris_container=container)
        events = collect_events(
            prompt="Call the check_config tool and tell me if IRIS is connected.",
            env_vars=env.env_vars(),
        )
    assert check_tool_called(events, "iris_agentic_dev", "check_config"), \
        "check_config must be called"
    outputs = [
        e["part"]["state"].get("output", "")
        for e in events
        if e.get("type") == "tool_use"
        and e["part"].get("tool") == "iris_agentic_dev:check_config"
        and e["part"]["state"].get("status") == "completed"
    ]
    assert any("connected" in o.lower() for o in outputs), \
        f"check_config output should mention connected. Got: {outputs}"


@pytest.mark.us2
def test_us2_mcp_compile(openai_api_key, iris_available):
    """iris_compile must be called and return a real IRIS result."""
    container = iris_available["container"]
    web_port = iris_available["web_port"]
    task = load_task(os.path.join(TASKS_DIR, "MCP-01.yaml"))

    # Pre-load fixtures into IRIS
    load_all_fixtures(
        task.fixtures,
        iris_host="localhost",
        iris_web_port=web_port,
    )

    result = run_task(
        task=task,
        openai_api_key=openai_api_key,
        iris_host="localhost",
        iris_web_port=web_port,
        iris_container=container,
    )
    assert "iris_agentic_dev:iris_compile" in result.tool_calls, \
        f"iris_compile not called. Tool calls: {result.tool_calls}"
    assert result.passed, \
        f"Assertion failed. Details: {[(a.description, a.passed) for a in result.assertion_results]}"
