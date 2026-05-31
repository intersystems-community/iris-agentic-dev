"""Unit tests for assertions — T009."""
import pytest
from tests.e2e.assertions import (
    extract_code_blocks,
    check_absent_pattern,
    check_tool_called,
    check_tools_in_order,
    RETURN_IN_FOR_PATTERN,
)


CLEAN_TEXT = """
Here is the fixed method:

```objectscript
Method FindFirst(list As %List) As %String {
  For i=1:1:$ListLength(list) {
    If $List(list, i) '= "" { Quit $List(list, i) }
  }
  Quit ""
}
```

This uses Quit instead of Return inside the loop.
"""

BUGGY_TEXT = """
Here is the method:

```cls
Method FindFirst(list As %List) As %String {
  For i=1:1:$ListLength(list) {
    If $List(list, i) '= "" { Return $List(list, i) }
  }
  Return ""
}
```
"""

NO_CODE_BLOCK = "This response has no code blocks. Just plain text."

MIXED_LANG_TEXT = """
```python
def foo():
    return 1
```

```objectscript
Method Bar() { Quit 1 }
```
"""


def test_extract_code_blocks_objectscript():
    blocks = extract_code_blocks(CLEAN_TEXT)
    assert len(blocks) == 1
    assert "FindFirst" in blocks[0]


def test_extract_code_blocks_cls():
    blocks = extract_code_blocks(BUGGY_TEXT)
    assert len(blocks) == 1
    assert "Return" in blocks[0]


def test_extract_code_blocks_none():
    blocks = extract_code_blocks(NO_CODE_BLOCK)
    assert blocks == []


def test_extract_code_blocks_only_objectscript_and_cls():
    blocks = extract_code_blocks(MIXED_LANG_TEXT)
    assert len(blocks) == 1
    assert "Bar" in blocks[0]
    assert "def foo" not in blocks[0]


def test_check_absent_pattern_clean():
    blocks = extract_code_blocks(CLEAN_TEXT)
    assert check_absent_pattern(blocks, RETURN_IN_FOR_PATTERN) is True


def test_check_absent_pattern_buggy():
    blocks = extract_code_blocks(BUGGY_TEXT)
    assert check_absent_pattern(blocks, RETURN_IN_FOR_PATTERN) is False


def test_check_absent_pattern_empty_blocks():
    assert check_absent_pattern([], RETURN_IN_FOR_PATTERN) is True


def make_tool_event(tool_name: str, status: str = "completed") -> dict:
    return {
        "type": "tool_use",
        "part": {
            "tool": tool_name,
            "state": {"status": status, "output": "result"},
        }
    }


def test_check_tool_called_found():
    events = [
        make_tool_event("bash"),
        make_tool_event("iris_agentic_dev:iris_compile"),
        make_tool_event("iris_agentic_dev:docs_introspect"),
    ]
    assert check_tool_called(events, "iris_agentic_dev", "iris_compile") is True


def test_check_tool_called_builtin():
    events = [make_tool_event("bash"), make_tool_event("read")]
    assert check_tool_called(events, None, "bash") is True


def test_check_tool_called_not_found():
    events = [make_tool_event("bash")]
    assert check_tool_called(events, "iris_agentic_dev", "iris_compile") is False


def test_check_tool_called_wrong_server():
    events = [make_tool_event("other_server:iris_compile")]
    assert check_tool_called(events, "iris_agentic_dev", "iris_compile") is False


def test_check_tool_called_only_completed():
    events = [make_tool_event("iris_agentic_dev:iris_compile", status="error")]
    assert check_tool_called(events, "iris_agentic_dev", "iris_compile") is False


def test_check_tools_in_order_correct():
    events = [
        make_tool_event("iris_agentic_dev:docs_introspect"),
        make_tool_event("bash"),
        make_tool_event("iris_agentic_dev:iris_compile"),
    ]
    tools = [("iris_agentic_dev", "docs_introspect"), ("iris_agentic_dev", "iris_compile")]
    assert check_tools_in_order(events, tools) is True


def test_check_tools_in_order_wrong_order():
    events = [
        make_tool_event("iris_agentic_dev:iris_compile"),
        make_tool_event("iris_agentic_dev:docs_introspect"),
    ]
    tools = [("iris_agentic_dev", "docs_introspect"), ("iris_agentic_dev", "iris_compile")]
    assert check_tools_in_order(events, tools) is False


def test_check_tools_in_order_missing_tool():
    events = [make_tool_event("iris_agentic_dev:docs_introspect")]
    tools = [("iris_agentic_dev", "docs_introspect"), ("iris_agentic_dev", "iris_compile")]
    assert check_tools_in_order(events, tools) is False


def test_check_tools_in_order_empty():
    events = [make_tool_event("bash")]
    assert check_tools_in_order(events, []) is True
