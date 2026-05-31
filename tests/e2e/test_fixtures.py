"""Unit tests for IRIS fixture loader — T021."""
import pytest
from unittest.mock import patch, MagicMock
from tests.e2e.fixtures import load_fixture
from tests.e2e.task_loader import HarnessFixture


CLS_FIXTURE = HarnessFixture(
    type="cls",
    name="User.HarnessTestClass",
    content="Class User.HarnessTestClass Extends %RegisteredObject {}",
)


def make_mock_response(status_code: int, json_data: dict | None = None) -> MagicMock:
    r = MagicMock()
    r.status_code = status_code
    r.json.return_value = json_data or {}
    r.raise_for_status = MagicMock()
    return r


def test_load_fixture_puts_cls_document():
    with patch("tests.e2e.fixtures.requests.put") as mock_put, \
         patch("tests.e2e.fixtures.requests.post") as mock_post:
        mock_put.return_value = make_mock_response(200)
        mock_post.return_value = make_mock_response(200, {"result": {"status": []}})

        load_fixture(CLS_FIXTURE, iris_host="localhost", iris_web_port="52773")

        assert mock_put.called
        put_url = mock_put.call_args[0][0]
        assert "User.HarnessTestClass.cls" in put_url
        assert "atelier" in put_url


def test_load_fixture_compiles_after_put():
    with patch("tests.e2e.fixtures.requests.put") as mock_put, \
         patch("tests.e2e.fixtures.requests.post") as mock_post:
        mock_put.return_value = make_mock_response(200)
        mock_post.return_value = make_mock_response(200, {"result": {"status": []}})

        load_fixture(CLS_FIXTURE, iris_host="localhost", iris_web_port="52773")

        assert mock_post.called
        post_url = mock_post.call_args[0][0]
        assert "compile" in post_url.lower() or "action" in post_url.lower()


def test_load_fixture_raises_on_put_error():
    with patch("tests.e2e.fixtures.requests.put") as mock_put:
        mock_put.return_value = make_mock_response(403)
        mock_put.return_value.raise_for_status.side_effect = Exception("403 Forbidden")
        with pytest.raises(Exception, match="403"):
            load_fixture(CLS_FIXTURE, iris_host="localhost", iris_web_port="52773")
