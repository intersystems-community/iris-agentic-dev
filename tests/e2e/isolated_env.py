"""IsolatedEnv — per-run isolation via OPENCODE_CONFIG_CONTENT + OPENCODE_DB."""
import json
import os
import shutil
import tempfile
import time


class IsolatedEnv:
    def __init__(self, openai_api_key: str, keep_on_failure: bool = False):
        self._openai_api_key = openai_api_key
        self._keep_on_failure = keep_on_failure
        self._tmpdir: str | None = None
        self._failed = False
        self._mcp_config: dict | None = None
        self.skills_dir: str = ""
        self.db_path: str = ""
        self.run_id: str = ""

    def __enter__(self) -> "IsolatedEnv":
        self.run_id = str(int(time.time() * 1000))
        self._tmpdir = tempfile.mkdtemp(prefix=f"opencode-harness-{self.run_id}-")
        self.skills_dir = os.path.join(self._tmpdir, "skills")
        # XDG_CONFIG_HOME override blocks ~/.config/opencode/config.json from loading,
        # preventing global MCP servers (objectscript-plaza etc.) from bleeding in.
        # XDG_DATA_HOME is intentionally NOT overridden — opencode hangs on DB migration
        # when the data dir is empty. The existing auth DB must be accessible.
        self.xdg_config = os.path.join(self._tmpdir, "xdg_config")
        os.makedirs(self.skills_dir, exist_ok=True)
        os.makedirs(self.xdg_config, exist_ok=True)
        self.db_path = os.path.join(self._tmpdir, "opencode.db")
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        failed = exc_type is not None
        if failed and self._keep_on_failure:
            return False  # retain, re-raise
        if self._tmpdir and os.path.isdir(self._tmpdir):
            shutil.rmtree(self._tmpdir, ignore_errors=True)
        return False

    def with_mcp(self, iris_host: str, iris_web_port: str, iris_container: str,
                 iris_namespace: str = "USER", iris_username: str = "_SYSTEM",
                 iris_password: str = "SYS") -> "IsolatedEnv":
        self._mcp_config = {
            "iris-agentic-dev": {
                "type": "local",
                "command": ["/opt/homebrew/bin/iris-agentic-dev", "mcp"],
                "enabled": True,
                "environment": {
                    "IRIS_HOST": iris_host,
                    "IRIS_WEB_PORT": iris_web_port,
                    "IRIS_CONTAINER": iris_container,
                    "IRIS_NAMESPACE": iris_namespace,
                    "IRIS_USERNAME": iris_username,
                    "IRIS_PASSWORD": iris_password,
                },
            }
        }
        return self

    @property
    def config_content(self) -> str:
        cfg: dict = {
            "provider": {
                "openai": {
                    # options.apiKey is the correct path per OpenCode source
                    # packages/opencode/src/config/provider.ts
                    "options": {"apiKey": self._openai_api_key}
                }
            },
            "skills": {"paths": [self.skills_dir]},
            # MCP: only present when with_mcp() was called.
            # Global MCP isolation is handled via XDG_CONFIG_HOME override in env_vars().
            **({} if not self._mcp_config else {"mcp": self._mcp_config}),
        }
        return json.dumps(cfg)

    def env_vars(self) -> dict:
        return {
            "OPENCODE_CONFIG_CONTENT": self.config_content,
            "OPENCODE_DB": self.db_path,
            # XDG_CONFIG_HOME blocks ~/.config/opencode/config.json (global MCP servers).
            # XDG_DATA_HOME intentionally not overridden — opencode hangs with empty data dir.
            "XDG_CONFIG_HOME": self.xdg_config,
        }
