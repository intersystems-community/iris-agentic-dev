"""Shared pytest fixtures for E2E harness."""
import os
import pytest


def pytest_configure(config):
    config.addinivalue_line("markers", "network_curl: requires live GitHub network access")
    config.addinivalue_line("markers", "requires_iris: requires a running IRIS container")
    config.addinivalue_line("markers", "us1: User Story 1 — skills quality")
    config.addinivalue_line("markers", "us2: User Story 2 — MCP tools")
    config.addinivalue_line("markers", "us3: User Story 3 — full stack")


@pytest.fixture
def openai_api_key():
    key = os.environ.get("OPENAI_API_KEY", "")
    if not key:
        pytest.skip("OPENAI_API_KEY not set")
    return key


@pytest.fixture
def iris_available():
    container = os.environ.get("IRIS_CONTAINER", "")
    port = os.environ.get("IRIS_WEB_PORT", "")
    if not container or not port:
        pytest.skip("IRIS_CONTAINER and IRIS_WEB_PORT not set")
    return {"container": container, "web_port": port}


@pytest.fixture
def iris_web_port():
    return os.environ.get("IRIS_WEB_PORT", "52773")


@pytest.fixture
def iris_container_name():
    return os.environ.get("IRIS_CONTAINER", "iris-dev-iris")
