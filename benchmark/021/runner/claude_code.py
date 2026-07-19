"""Claude Code harness driver — MCP stdio + Anthropic API tool loop (Bedrock or direct)."""
import os
import json
import time
import subprocess
import anthropic
try:
    from ._client import make_client, sonnet_model
except ImportError:
    from _client import make_client, sonnet_model

PATH_A_SYSTEM = """You are an ObjectScript developer working in LOCAL FILES mode.

Rules:
- Write .cls files to the local filesystem (the current working directory or a temp path)
- Use iris_compile to compile them into IRIS after writing
- Use iris_execute and iris_query to verify behavior
- Do NOT use iris_doc put to write documents directly — always use local files + iris_compile
- The IRIS namespace is BENCHMARK

Complete the task efficiently. Minimize unnecessary tool calls."""

PATH_B_SYSTEM = """You are an ObjectScript developer working in ISFS (remote-only) mode.

Rules:
- Use iris_doc with mode=put to write documents directly into IRIS
- Use iris_doc with mode=get to read existing documents
- Use iris_compile to compile after writing
- Use iris_execute and iris_query to verify behavior
- Do NOT write local .cls files — all code lives in IRIS only
- The IRIS namespace is BENCHMARK

Complete the task efficiently. Minimize unnecessary tool calls."""

PYPR_SYSTEM_BASELINE = """You are a Python developer building pyprod interoperability components for InterSystems IRIS.

Rules:
- Produce the complete, correct Python source code in your response
- Do NOT call iris_compile — Python files do not need IRIS compilation
- Do NOT call iris_doc — it is for ObjectScript/COS documents only
- Answer directly from your knowledge — do not spend turns on reconnaissance

Complete the task efficiently."""

def _load_skill(name: str) -> str:
    """Load skill SKILL.md, checking light-skills/ then skills/skills/ for the file."""
    here = os.path.dirname(os.path.abspath(__file__))
    repo_root = os.path.normpath(os.path.join(here, "..", "..", ".."))
    candidates = [
        os.path.join(repo_root, "light-skills", "skills", name, "SKILL.md"),
        os.path.join(repo_root, "skills", "skills", name, "SKILL.md"),
    ]
    path = next((p for p in candidates if os.path.exists(p)), None)
    if path is None:
        return ""
    with open(path) as f:
        content = f.read()
    # Strip YAML frontmatter (--- ... ---)
    if content.startswith("---"):
        end = content.find("---", 3)
        if end != -1:
            content = content[end + 3:].lstrip()
    return content

COV_SYSTEM_BASELINE = """You are an ObjectScript developer. Use IRIS MCP tools to complete the coverage task.
The IRIS namespace is USER.
Complete the task efficiently."""

COV_SYSTEM_MERGED = """You are an ObjectScript developer measuring ObjectScript line coverage.

Key rules:
- Use iris_coverage tool — NOT iris_execute — for all coverage operations
- mode=run runs tests and measures coverage in one call (most tasks)
- mode=check verifies monitor availability (run if unsure)
- Provide either classes=["ClassName"] or package="PackageName" — never both
- test_path is a compiled class pattern (e.g. "MyApp.Tests") — /noload always used
- The IRIS namespace is USER

Complete the task in as few tool calls as possible."""

DOC_SYSTEM_BASELINE = """You are an IRIS developer. Use available MCP tools to answer documentation questions.
Complete the task efficiently."""


def _build_doc_merged_prompt() -> str:
    skill_content = _load_skill("iris-docs")
    return (
        "You are an IRIS developer answering documentation questions.\n\n"
        "The iris-docs skill is loaded below. "
        "Use iris_doc_search to find authoritative answers from docs.intersystems.com.\n\n"
        "--- SKILL: iris-docs ---\n"
        + skill_content
        + "\n--- END SKILL ---\n\n"
        "Complete the task efficiently."
    )


def _build_system_prompt(path: str, category: str = "", condition: str = "baseline") -> str:
    if category == "PYPR":
        if condition == "merged":
            skill_content = _load_skill("pyprod")
            return (
                "You are a Python developer building pyprod interoperability components "
                "for InterSystems IRIS.\n\n"
                "The following skill reference has been loaded for you. "
                "Use it to write correct code — do not call any tools to look up documentation.\n\n"
                "--- SKILL: pyprod ---\n"
                + skill_content
                + "\n--- END SKILL ---\n\n"
                "Rules:\n"
                "- Produce the complete, correct Python source code in your response\n"
                "- Do NOT call iris_compile — Python files do not need IRIS compilation\n"
                "- Do NOT call iris_doc, skill_search, kb_recall, or any other tool\n"
                "- Answer directly using the skill above — no reconnaissance needed\n\n"
                "Complete the task efficiently."
            )
        return PYPR_SYSTEM_BASELINE
    if category == "COV":
        return COV_SYSTEM_MERGED if condition == "merged" else COV_SYSTEM_BASELINE
    if category == "DOC":
        return _build_doc_merged_prompt() if condition == "merged" else DOC_SYSTEM_BASELINE
    return PATH_A_SYSTEM if path == "A" else PATH_B_SYSTEM


def _spawn_mcp() -> subprocess.Popen:
    env = os.environ.copy()
    return subprocess.Popen(
        ["iris-dev", "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        env=env,
    )


def _shutdown_mcp(proc: subprocess.Popen):
    try:
        proc.stdin.close()
        proc.wait(timeout=3)
    except Exception:
        proc.kill()


def _handshake(proc: subprocess.Popen):
    _send(proc, {"jsonrpc": "2.0", "id": 0, "method": "initialize",
                 "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                            "clientInfo": {"name": "benchmark", "version": "1"}}})
    time.sleep(0.2)
    _send(proc, {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
    time.sleep(0.2)
    # read the initialize response
    line = proc.stdout.readline()
    return json.loads(line) if line else {}


def _send(proc: subprocess.Popen, obj: dict):
    proc.stdin.write((json.dumps(obj) + "\n").encode())
    proc.stdin.flush()


def _mcp_call(proc: subprocess.Popen, tool: str, args: dict, call_id: int) -> dict:
    _send(proc, {"jsonrpc": "2.0", "id": call_id, "method": "tools/call",
                 "params": {"name": tool, "arguments": args}})
    time.sleep(1)
    # read until we get our response id
    deadline = time.time() + 10
    while time.time() < deadline:
        line = proc.stdout.readline()
        if not line:
            break
        try:
            obj = json.loads(line)
            if obj.get("id") == call_id:
                return obj
        except json.JSONDecodeError:
            pass
    return {}


def _get_tools(proc: subprocess.Popen) -> list:
    _send(proc, {"jsonrpc": "2.0", "id": 99, "method": "tools/list", "params": {}})
    time.sleep(1)
    deadline = time.time() + 5
    while time.time() < deadline:
        line = proc.stdout.readline()
        if not line:
            break
        try:
            obj = json.loads(line)
            if obj.get("id") == 99:
                tools_raw = obj.get("result", {}).get("tools", [])
                return [
                    {"name": t["name"],
                     "description": t.get("description", ""),
                     "input_schema": t.get("inputSchema", {"type": "object", "properties": {}})}
                    for t in tools_raw
                ]
        except json.JSONDecodeError:
            pass
    return []


def run_task(task: dict, path: str, condition: str = "baseline") -> dict:
    """Run one benchmark task via Claude Code (Anthropic API + iris-dev MCP)."""
    proc = _spawn_mcp()
    _handshake(proc)
    tools = _get_tools(proc)

    client = make_client()
    system = _build_system_prompt(path, category=task.get("category", ""), condition=condition)
    messages = [{"role": "user", "content": task["description"]}]
    transcript = []
    call_id = 100

    for _ in range(20):  # max 20 turns
        max_tokens = 8192 if task.get("category") == "PYPR" else 4096
        response = client.messages.create(
            model=sonnet_model(),
            max_tokens=max_tokens,
            system=system,
            tools=tools,
            messages=messages,
        )

        # collect assistant turn
        assistant_content = []
        for block in response.content:
            if block.type == "text":
                transcript.append({"role": "assistant", "text": block.text})
                assistant_content.append({"type": "text", "text": block.text})
            elif block.type == "tool_use":
                transcript.append({
                    "role": "assistant",
                    "tool_name": block.name,
                    "args": block.input,
                    "tool_use_id": block.id,
                })
                assistant_content.append({
                    "type": "tool_use",
                    "id": block.id,
                    "name": block.name,
                    "input": block.input,
                })

        messages.append({"role": "assistant", "content": assistant_content})

        if response.stop_reason == "end_turn":
            break

        # execute tool calls
        tool_results = []
        for block in response.content:
            if block.type != "tool_use":
                continue
            mcp_resp = _mcp_call(proc, block.name, block.input, call_id)
            call_id += 1
            result_content = mcp_resp.get("result", {}).get("content", [])
            result_text = result_content[0].get("text", "") if result_content else ""
            transcript.append({"role": "tool_result", "tool_result": result_text,
                                "tool_use_id": block.id})
            tool_results.append({
                "type": "tool_result",
                "tool_use_id": block.id,
                "content": result_text,
            })

        if tool_results:
            messages.append({"role": "user", "content": tool_results})

    _shutdown_mcp(proc)
    return {"path": path, "transcript": transcript, "tool_call_count": call_id - 100}
