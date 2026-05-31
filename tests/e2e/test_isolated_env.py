"""Unit tests for IsolatedEnv — T005."""
import json
import os
import pytest
from unittest.mock import patch
from tests.e2e.isolated_env import IsolatedEnv


def test_temp_dir_created_and_torn_down():
    with IsolatedEnv(openai_api_key="sk-test") as env:
        assert os.path.isdir(env.skills_dir)
        assert os.path.isdir(os.path.dirname(env.db_path))
        skills_dir = env.skills_dir
    assert not os.path.isdir(skills_dir)


def test_retain_on_failure(tmp_path):
    env = IsolatedEnv(openai_api_key="sk-test", keep_on_failure=True)
    env.__enter__()
    skills_dir = env.skills_dir
    env.__exit__(ValueError, ValueError("test error"), None)
    assert os.path.isdir(skills_dir), "Should be retained on failure"
    import shutil
    shutil.rmtree(skills_dir, ignore_errors=True)


def test_teardown_on_clean_exit():
    env = IsolatedEnv(openai_api_key="sk-test", keep_on_failure=True)
    env.__enter__()
    skills_dir = env.skills_dir
    env.__exit__(None, None, None)
    assert not os.path.isdir(skills_dir), "Should be torn down on clean exit even with keep_on_failure"


def test_config_content_has_options_apikey():
    with IsolatedEnv(openai_api_key="sk-test-key") as env:
        cfg = json.loads(env.config_content)
        # Must use options.apiKey, not direct apiKey (I1 fix)
        assert cfg["provider"]["openai"]["options"]["apiKey"] == "sk-test-key"
        assert "apiKey" not in cfg["provider"]["openai"], "apiKey must be nested under options"


def test_config_content_has_skills_path():
    with IsolatedEnv(openai_api_key="sk-test") as env:
        cfg = json.loads(env.config_content)
        assert env.skills_dir in cfg["skills"]["paths"]


def test_config_content_no_mcp_by_default():
    """No mcp key by default — global isolation handled via XDG_CONFIG_HOME."""
    with IsolatedEnv(openai_api_key="sk-test") as env:
        cfg = json.loads(env.config_content)
        assert "mcp" not in cfg


def test_with_mcp_adds_iris_agentic_dev():
    with IsolatedEnv(openai_api_key="sk-test") as env:
        env.with_mcp(iris_host="localhost", iris_web_port="52780", iris_container="iris-dev-iris")
        cfg = json.loads(env.config_content)
        assert "mcp" in cfg
        assert "iris-agentic-dev" in cfg["mcp"]
        mcp_env = cfg["mcp"]["iris-agentic-dev"]["environment"]
        assert mcp_env["IRIS_HOST"] == "localhost"
        assert mcp_env["IRIS_WEB_PORT"] == "52780"
        assert mcp_env["IRIS_CONTAINER"] == "iris-dev-iris"


def test_opencode_db_path_is_isolated():
    with IsolatedEnv(openai_api_key="sk-test") as env1:
        with IsolatedEnv(openai_api_key="sk-test") as env2:
            assert env1.db_path != env2.db_path


def test_env_vars_dict():
    with IsolatedEnv(openai_api_key="sk-test") as env:
        ev = env.env_vars()
        assert ev["OPENCODE_CONFIG_CONTENT"] == env.config_content
        assert ev["OPENCODE_DB"] == env.db_path
        assert ev["XDG_CONFIG_HOME"] == env.xdg_config
        assert "XDG_DATA_HOME" not in ev  # intentionally NOT overridden — see isolated_env.py
        assert os.path.isdir(env.xdg_config)
