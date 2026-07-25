"""OpenCode subprocess runner and event stream parser."""
import contextlib
import json
import os
import signal
import sqlite3
import subprocess
import tempfile
import threading
from typing import Generator


def parse_mcp_tool(tool_name: str) -> tuple[str | None, str]:
    """Split MCP tool name into (server, tool).

    OpenCode emits tool names as '{server}_{tool}' where server keeps its
    original name (hyphens preserved). E.g. 'iris-agentic-dev' + 'iris_compile'
    → 'iris-agentic-dev_iris_compile'.

    Comparison is done with both the original name and the underscore-sanitized
    version so callers can use either form.

    Falls back to colon-split for forward compatibility.
    """
    if ":" in tool_name:
        server, _, rest = tool_name.partition(":")
        return server, rest
    # Known server names (original form with hyphens)
    _KNOWN_SERVERS = [
        "iris-agentic-dev",
        "objectscript-plaza",
        "objectscript",
    ]
    for server in _KNOWN_SERVERS:
        prefix = server + "_"
        if tool_name.startswith(prefix):
            # Return sanitized server name (hyphens→underscores) for consistent matching
            return server.replace("-", "_"), tool_name[len(prefix):]
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


@contextlib.contextmanager
def _sandbox_dir(working_dir: str | None) -> Generator[str, None, None]:
    """Yield the cwd to run the agent in, never the caller's cwd.

    The agent runs with --dangerously-skip-permissions, so its cwd is a
    directory it can rewrite at will. Inheriting os.getcwd() pointed that at
    the repo checkout, which let a fire-rate run edit the very benchmark
    fixture it was about to be scored on. Callers that need to read files the
    agent wrote pass an explicit working_dir; everyone else gets a throwaway.
    """
    if working_dir is not None:
        yield working_dir
        return
    with tempfile.TemporaryDirectory(prefix="opencode-scratch-") as scratch:
        yield scratch


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
    collected: list[dict] = []
    with _sandbox_dir(working_dir) as cwd:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            env=env,
            cwd=cwd,
        )

        # Kill the process after timeout regardless
        timer = threading.Timer(timeout, lambda: proc.kill())
        timer.start()

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
