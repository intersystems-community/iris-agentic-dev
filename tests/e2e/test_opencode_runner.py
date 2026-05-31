"""Unit tests for opencode_runner — T007."""
import json
import os
import sqlite3
import tempfile
import pytest
from tests.e2e.opencode_runner import parse_events_from_lines, parse_mcp_tool, read_session_db


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


def test_parse_mcp_tool_with_server():
    server, tool = parse_mcp_tool("iris_agentic_dev:iris_compile")
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
