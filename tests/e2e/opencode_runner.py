"""OpenCode subprocess runner and event stream parser."""
import json
import os
import signal
import sqlite3
import subprocess
import threading
from typing import Generator


def parse_mcp_tool(tool_name: str) -> tuple[str | None, str]:
    """Split 'server:tool' → (server, tool). Built-ins return (None, tool)."""
    if ":" in tool_name:
        server, _, rest = tool_name.partition(":")
        return server, rest
    return None, tool_name


def parse_events_from_lines(lines: list[str]) -> Generator[dict, None, None]:
    """Parse JSON event lines from opencode run --format json output."""
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            event = json.loads(line)
            yield event
        except json.JSONDecodeError:
            continue


def run_opencode(
    prompt: str,
    env_vars: dict,
    model: str = "openai/gpt-4o-mini",
    timeout: int = 300,
    working_dir: str | None = None,
) -> Generator[dict, None, None]:
    """Spawn opencode run and yield parsed JSON events from stdout.

    Uses Popen + readline so we can kill the process as soon as the
    session goes idle rather than waiting for opencode to exit on its own
    (opencode run can hang on teardown after the LLM response completes).
    """
    env = {**os.environ, **env_vars}
    cmd = [
        "opencode", "run", prompt,
        "--format", "json",
        "--model", model,
        "--dangerously-skip-permissions",
    ]
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        env=env,
        cwd=working_dir or os.getcwd(),
    )

    # Kill the process after timeout regardless
    timer = threading.Timer(timeout, lambda: proc.kill())
    timer.start()

    collected: list[dict] = []
    try:
        for line in proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
                collected.append(event)
                # Stop reading once the session reports idle — opencode has finished
                if (event.get("type") == "session.status"
                        and event.get("properties", {}).get("status", {}).get("type") == "idle"):
                    break
                # Also stop on step_finish with no more steps pending (heuristic)
            except json.JSONDecodeError:
                continue
    finally:
        timer.cancel()
        try:
            proc.kill()
            proc.wait(timeout=5)
        except Exception:
            pass

    yield from collected


def collect_events(
    prompt: str,
    env_vars: dict,
    model: str = "openai/gpt-4o-mini",
    timeout: int = 300,
    working_dir: str | None = None,
) -> list[dict]:
    """Run opencode and return all events as a list."""
    return list(run_opencode(prompt, env_vars, model=model, timeout=timeout, working_dir=working_dir))


def read_session_db(db_path: str) -> dict:
    """Read all tables from the OpenCode session SQLite DB. Returns {} if missing."""
    if not os.path.exists(db_path):
        return {}
    try:
        conn = sqlite3.connect(db_path)
        tables = [r[0] for r in conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table'"
        ).fetchall()]
        result = {}
        for table in tables:
            result[table] = conn.execute(f"SELECT * FROM {table}").fetchall()
        conn.close()
        return result
    except Exception:
        return {}
