"""Assertion helpers for the E2E skills harness."""
import re

# Detects Return inside a For loop body in ObjectScript code blocks.
# Matches: For ... { ... Return ... } or For ... \n ... Return
RETURN_IN_FOR_PATTERN = r"(?s)For\b[^\n{]*\{[^}]*\bReturn\b"

_CODE_BLOCK_RE = re.compile(
    r"```(?:objectscript|cls)\n(.*?)```",
    re.IGNORECASE | re.DOTALL,
)


def extract_code_blocks(text: str) -> list[str]:
    """Extract contents of ```objectscript and ```cls fenced blocks."""
    return [m.group(1) for m in _CODE_BLOCK_RE.finditer(text)]


def check_absent_pattern(blocks: list[str], pattern: str) -> bool:
    """Return True (PASS) if pattern does NOT appear in any code block."""
    for block in blocks:
        if re.search(pattern, block):
            return False
    return True


def check_tool_called(events: list[dict], server: str | None, tool: str) -> bool:
    """Return True if a completed tool_use event matches the given server+tool."""
    from tests.e2e.opencode_runner import parse_mcp_tool
    for event in events:
        if event.get("type") != "tool_use":
            continue
        part = event.get("part", {})
        state = part.get("state", {})
        if state.get("status") != "completed":
            continue
        ev_server, ev_tool = parse_mcp_tool(part.get("tool", ""))
        if ev_tool == tool and ev_server == server:
            return True
    return False


def check_tools_in_order(events: list[dict], tools: list[tuple[str | None, str]]) -> bool:
    """Return True if each (server, tool) in tools appears in events in order."""
    if not tools:
        return True
    from tests.e2e.opencode_runner import parse_mcp_tool
    tool_idx = 0
    for event in events:
        if event.get("type") != "tool_use":
            continue
        part = event.get("part", {})
        state = part.get("state", {})
        if state.get("status") != "completed":
            continue
        ev_server, ev_tool = parse_mcp_tool(part.get("tool", ""))
        expected_server, expected_tool = tools[tool_idx]
        if ev_tool == expected_tool and ev_server == expected_server:
            tool_idx += 1
            if tool_idx == len(tools):
                return True
    return False
