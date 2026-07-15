"""T012 — Unit tests for claude_code.py driver (mocked MCP + Anthropic)."""
import json
from unittest.mock import MagicMock, patch, call

SAMPLE_TASK = {
    "id": "GEN-01",
    "description": "Write a class Bench.Greeter with ClassMethod Hello() returning 'Hello World'",
    "expected_behavior": "##class(Bench.Greeter).Hello() returns 'Hello World'",
    "path": "A",
}


def _make_mock_mcp(tool_results: list):
    """Return a mock _mcp_call that returns tool_results in sequence."""
    mock = MagicMock(side_effect=tool_results)
    return mock


def _make_mock_anthropic(tool_calls: list, final_text: str):
    """Return a mock Anthropic client that yields tool_calls then final_text."""
    responses = []
    for tc in tool_calls:
        msg = MagicMock()
        msg.stop_reason = "tool_use"
        block = MagicMock()
        block.type = "tool_use"
        block.name = tc["name"]
        block.input = tc["input"]
        block.id = f"tool_{tc['name']}"
        msg.content = [block]
        responses.append(msg)

    final = MagicMock()
    final.stop_reason = "end_turn"
    block = MagicMock()
    block.type = "text"
    block.text = final_text
    final.content = [block]
    responses.append(final)

    client = MagicMock()
    client.messages.create.side_effect = responses
    return client


def test_run_task_returns_transcript():
    mock_tool_result = {"result": {"content": [{"type": "text", "text": '{"success":true}'}]}}

    with patch("claude_code.make_client") as MockAnth, \
         patch("claude_code._mcp_call") as mock_mcp, \
         patch("claude_code._spawn_mcp") as mock_spawn, \
         patch("claude_code._shutdown_mcp"), \
         patch("claude_code._handshake"), \
         patch("claude_code._get_tools") as mock_tools:
        MockAnth.return_value = _make_mock_anthropic(
            [{"name": "iris_compile", "input": {"target": "Bench.Greeter.cls"}}],
            "Class created successfully."
        )
        mock_mcp.return_value = mock_tool_result
        mock_spawn.return_value = MagicMock()
        mock_tools.return_value = []

        from claude_code import run_task
        result = run_task(SAMPLE_TASK, "A")

    assert "transcript" in result
    assert "path" in result
    assert result["path"] == "A"
    assert len(result["transcript"]) > 0


def test_run_task_path_a_prompt_mentions_local_files():
    from claude_code import _build_system_prompt
    prompt = _build_system_prompt("A")
    assert "local" in prompt.lower() or "file" in prompt.lower()
    # Path A tells agent NOT to use iris_doc put — instruction must be present
    assert "do not use iris_doc put" in prompt.lower() or "not use iris_doc" in prompt.lower()


def test_run_task_path_b_prompt_mentions_iris_doc():
    from claude_code import _build_system_prompt
    prompt = _build_system_prompt("B")
    assert "iris_doc" in prompt.lower()
    assert "isfs" in prompt.lower() or "remote" in prompt.lower()


def test_run_task_pypr_baseline_prompt_is_direct():
    from claude_code import _build_system_prompt
    prompt = _build_system_prompt("A", category="PYPR", condition="baseline")
    assert "pyprod" in prompt.lower() or "python" in prompt.lower()
    assert "iris_compile" in prompt.lower() and "not" in prompt.lower()
    assert ".cls" not in prompt.lower()
    # baseline should NOT instruct agent to call the skill tool
    assert "skill(" not in prompt


def test_run_task_pypr_merged_prompt_calls_skill():
    from claude_code import _build_system_prompt
    prompt = _build_system_prompt("A", category="PYPR", condition="merged")
    assert "skill(" in prompt or "skill tool" in prompt.lower()
    assert "iris_compile" in prompt.lower() and "not" in prompt.lower()
    assert ".cls" not in prompt.lower()
