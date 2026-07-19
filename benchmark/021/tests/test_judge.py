"""T007 — Unit tests for judge.py using a mock Anthropic client."""
import json
from unittest.mock import MagicMock, patch

SAMPLE_TASK = {
    "id": "GEN-01",
    "description": "Write a class Bench.Greeter with ClassMethod Hello() returning 'Hello World'",
    "expected_behavior": "##class(Bench.Greeter).Hello() returns 'Hello World'",
    "path": "A",
}

SAMPLE_RESULT = {
    "path": "A",
    "transcript": [
        {"role": "assistant", "tool_name": "iris_compile", "args": {"target": "Bench.Greeter.cls"}},
        {"role": "tool_result", "tool_result": '{"success": true}'},
        {"role": "assistant", "text": "Class created and compiled successfully."},
    ],
}


def _mock_anthropic(score: int, reasoning: str):
    mock_msg = MagicMock()
    mock_msg.content = [MagicMock(text=json.dumps({"score": score, "reasoning": reasoning}))]
    mock_client = MagicMock()
    mock_client.messages.create.return_value = mock_msg
    return mock_client


def test_score_result_returns_valid_schema():
    with patch("judge.make_client") as mock_make:
        mock_make.return_value = _mock_anthropic(3, "Correct and efficient")
        from judge import score_result
        result = score_result(SAMPLE_TASK, SAMPLE_RESULT)
    assert "score" in result
    assert "reasoning" in result
    assert result["score"] in (0, 1, 2, 3)
    assert isinstance(result["reasoning"], str)


def test_score_result_returns_score_3():
    with patch("judge.make_client") as mock_make:
        mock_make.return_value = _mock_anthropic(3, "Perfect")
        from judge import score_result
        result = score_result(SAMPLE_TASK, SAMPLE_RESULT)
    assert result["score"] == 3


def test_score_result_handles_api_error():
    with patch("judge.make_client") as mock_make:
        mock_client = MagicMock()
        mock_client.messages.create.side_effect = Exception("API error")
        mock_make.return_value = mock_client
        from judge import score_result
        result = score_result(SAMPLE_TASK, SAMPLE_RESULT)
    assert result["score"] == 0
    assert "Judge error" in result["reasoning"]


def test_doc_category_uses_doc_note():
    """DOC tasks use DOC_CATEGORY_NOTE not PYPR_CATEGORY_NOTE."""
    doc_task = {
        "id": "DOC-01",
        "description": "What are the SQL execution methods in IRIS?",
        "expected_behavior": "Names %SQL.Statement, embedded SQL, ResultSet",
        "path": "A",
        "category": "DOC",
    }
    doc_result = {
        "path": "A",
        "transcript": [
            {"role": "assistant", "tool_name": "iris_doc_search",
             "args": {"query": "SQL execution methods"}},
            {"role": "tool_result", "tool_result": '{"hits": []}'},
            {"role": "assistant", "text": "%SQL.Statement and embedded SQL."},
        ],
    }
    with patch("judge.make_client") as mock_make:
        mock_make.return_value = _mock_anthropic(2, "Mostly correct")
        from judge import score_result, DOC_CATEGORY_NOTE
        score_result(doc_task, doc_result)
        call_args = mock_make.return_value.messages.create.call_args
        prompt_text = call_args[1]["messages"][0]["content"]
    assert DOC_CATEGORY_NOTE in prompt_text
