"""Shared pytest fixtures for E2E harness."""

import os
import pytest

from tests.e2e import billing


def pytest_configure(config):
    config.addinivalue_line(
        "markers", "network_curl: requires live GitHub network access"
    )
    config.addinivalue_line(
        "markers", "requires_iris: requires a running IRIS container"
    )
    config.addinivalue_line("markers", "us1: User Story 1 — skills quality")
    config.addinivalue_line("markers", "us2: User Story 2 — MCP tools")
    config.addinivalue_line("markers", "us3: User Story 3 — full stack")
    config.addinivalue_line(
        "markers",
        f"billable: spawns a real agent session and costs money; skipped unless "
        f"{billing.BILLABLE_ENV}=1",
    )
    config.addinivalue_line("markers", "cli: drives a command-line entry point")
    config.addinivalue_line(
        "markers",
        "live: starts a throwaway Docker container and follows shipped setup steps on it; "
        "select with -m live, and run under the shared lock",
    )
    config.addinivalue_line(
        "markers",
        "requires_binary: needs a real iris-agentic-dev build; skips without one, so CI has "
        "to select it by this marker with IAD_BINARY set or it reports green over nothing",
    )


def pytest_collection_modifyitems(config, items):
    """Skip the tests that spend money unless something asked for them by name.

    The gate in `billing.py` already stops the spawn, so nothing here protects the wallet — it stops a
    directory sweep from turning red over tests that were never meant to run in it. Skipped and not
    deselected, so the count still says out loud that they exist.
    """
    if billing.allowed():
        return
    skip = pytest.mark.skip(
        reason=f"spawns a billable agent session; set {billing.BILLABLE_ENV}=1 to run it"
    )
    for item in items:
        if item.get_closest_marker("billable") is not None:
            item.add_marker(skip)


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
