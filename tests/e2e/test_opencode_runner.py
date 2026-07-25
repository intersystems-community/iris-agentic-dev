"""Unit tests for opencode_runner — T007."""
import json
import os
import sqlite3
import subprocess
import tempfile
import pytest
from tests.e2e import opencode_runner
from tests.e2e.opencode_runner import (
    collect_events,
    parse_events_from_lines,
    parse_mcp_tool,
    read_session_db,
)


TOOL_USE_COMPLETED = json.dumps({
    "type": "tool_use",
    "timestamp": 1000,
    "sessionID": "s1",
    "part": {
        "id": "p1",
        "sessionID": "s1",
        "type": "tool",
        "tool": "iris_agentic_dev:iris_compile",
        "state": {
            "status": "completed",
            "input": {"cls_name": "User.Foo"},
            "output": "Compiled OK",
            "title": "iris_compile",
        }
    }
})

TOOL_USE_BUILTIN = json.dumps({
    "type": "tool_use",
    "timestamp": 1001,
    "sessionID": "s1",
    "part": {
        "id": "p2",
        "sessionID": "s1",
        "type": "tool",
        "tool": "bash",
        "state": {"status": "completed", "input": {"command": "ls"}, "output": "file.txt"},
    }
})

TEXT_EVENT = json.dumps({
    "type": "text",
    "timestamp": 1002,
    "sessionID": "s1",
    "part": {"type": "text", "text": "The class compiled successfully.", "time": {"end": 1}}
})

ERROR_EVENT = json.dumps({
    "type": "error",
    "timestamp": 1003,
    "sessionID": "s1",
    "error": {"name": "CompileError", "data": {"message": "Syntax error at line 5"}}
})

UNKNOWN_EVENT = json.dumps({"type": "some_future_event", "data": {}})


def test_parse_mcp_tool_with_colon():
    server, tool = parse_mcp_tool("iris_agentic_dev:iris_compile")
    assert server == "iris_agentic_dev"
    assert tool == "iris_compile"


def test_parse_mcp_tool_hyphen_server():
    # OpenCode keeps hyphens in server name: iris-agentic-dev_iris_compile
    # parse_mcp_tool returns underscore-sanitized server name for consistent matching
    server, tool = parse_mcp_tool("iris-agentic-dev_iris_compile")
    assert server == "iris_agentic_dev"
    assert tool == "iris_compile"


def test_parse_mcp_tool_builtin():
    server, tool = parse_mcp_tool("bash")
    assert server is None
    assert tool == "bash"


def test_parse_mcp_tool_multi_colon():
    server, tool = parse_mcp_tool("my_server:some:tool")
    assert server == "my_server"
    assert tool == "some:tool"


def test_tool_use_event_parsed():
    events = list(parse_events_from_lines([TOOL_USE_COMPLETED]))
    assert len(events) == 1
    e = events[0]
    assert e["type"] == "tool_use"
    assert e["part"]["tool"] == "iris_agentic_dev:iris_compile"
    assert e["part"]["state"]["status"] == "completed"
    assert e["part"]["state"]["output"] == "Compiled OK"


def test_builtin_tool_event_parsed():
    events = list(parse_events_from_lines([TOOL_USE_BUILTIN]))
    assert events[0]["part"]["tool"] == "bash"


def test_text_event_parsed():
    events = list(parse_events_from_lines([TEXT_EVENT]))
    assert events[0]["type"] == "text"
    assert "compiled successfully" in events[0]["part"]["text"]


def test_error_event_parsed():
    events = list(parse_events_from_lines([ERROR_EVENT]))
    assert events[0]["type"] == "error"


def test_unknown_event_silently_ignored():
    events = list(parse_events_from_lines([UNKNOWN_EVENT]))
    assert events[0]["type"] == "some_future_event"


def test_multiple_events():
    lines = [TOOL_USE_COMPLETED, TEXT_EVENT, ERROR_EVENT]
    events = list(parse_events_from_lines(lines))
    assert len(events) == 3
    assert [e["type"] for e in events] == ["tool_use", "text", "error"]


def test_empty_lines_skipped():
    events = list(parse_events_from_lines(["", "  ", TOOL_USE_COMPLETED, ""]))
    assert len(events) == 1


def test_read_session_db_with_fixture():
    with tempfile.NamedTemporaryFile(suffix=".db", delete=False) as f:
        db_path = f.name
    try:
        conn = sqlite3.connect(db_path)
        conn.execute("CREATE TABLE session (id TEXT, title TEXT)")
        conn.execute("INSERT INTO session VALUES ('s1', 'Test session')")
        conn.commit()
        conn.close()
        rows = read_session_db(db_path)
        assert isinstance(rows, dict)
        assert "session" in rows
        assert rows["session"][0] == ("s1", "Test session")
    finally:
        os.unlink(db_path)


def test_read_session_db_missing_file():
    rows = read_session_db("/nonexistent/path.db")
    assert rows == {}


# --- Working-directory sandboxing -------------------------------------------
#
# The agent under test runs with --dangerously-skip-permissions, so whatever
# cwd it gets is a directory it can freely overwrite. Defaulting that to the
# repo checkout let a fire-rate run rewrite the benchmark fixture it was
# about to be scored against (tests/e2e/tasks/skills/targeted/LIST-ITERATE.yaml),
# corrupting the YAML and crashing the lift stage minutes later.


class _FakeProc:
    """Stand-in for Popen that records the cwd it was given."""

    def __init__(self):
        self.stdout = iter([])
        self.returncode = 0

    def kill(self):
        pass

    def wait(self, timeout=None):
        return 0


@pytest.fixture
def popen_spy(monkeypatch):
    seen = {}

    def fake_popen(cmd, **kwargs):
        cwd = kwargs.get("cwd")
        seen["cwd"] = cwd
        seen["cmd"] = cmd
        # Sampled while the process is "running" — a scratch dir is cleaned up
        # by the time collect_events returns, so it can't be checked after.
        seen["cwd_existed"] = cwd is not None and os.path.isdir(cwd)
        seen["cwd_contents"] = sorted(os.listdir(cwd)) if seen["cwd_existed"] else None
        return _FakeProc()

    monkeypatch.setattr(subprocess, "Popen", fake_popen)
    return seen


def test_omitted_working_dir_does_not_run_in_the_repo(popen_spy):
    """No working_dir must not mean "run in the repo checkout"."""
    collect_events("do something", {})
    cwd = popen_spy["cwd"]
    repo_root = os.path.abspath(
        os.path.join(os.path.dirname(__file__), "..", "..")
    )
    assert cwd is not None, "cwd must be set explicitly, never inherited"
    assert os.path.commonpath([os.path.abspath(cwd), repo_root]) != repo_root, (
        f"agent would run inside the repo checkout: {cwd}"
    )


def test_omitted_working_dir_gets_a_real_empty_directory(popen_spy):
    collect_events("do something", {})
    assert popen_spy["cwd_existed"], f"cwd must exist: {popen_spy['cwd']}"
    assert popen_spy["cwd_contents"] == [], "scratch workdir must start empty"


def test_scratch_workdir_is_cleaned_up(popen_spy):
    collect_events("do something", {})
    assert not os.path.exists(popen_spy["cwd"]), "scratch workdir must not be left behind"


def test_explicit_working_dir_is_respected(popen_spy):
    with tempfile.TemporaryDirectory() as d:
        collect_events("do something", {}, working_dir=d)
        assert popen_spy["cwd"] == d


def test_each_omitted_run_gets_its_own_directory(popen_spy):
    collect_events("first", {})
    first = popen_spy["cwd"]
    collect_events("second", {})
    second = popen_spy["cwd"]
    assert first != second, "runs must not share a scratch workdir"
