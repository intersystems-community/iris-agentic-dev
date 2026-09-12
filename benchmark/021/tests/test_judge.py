"""T007 — Unit tests for judge.py using a mock Anthropic client.

Extended for 118 T008. The scorer's failure path used to return `{"score": 0}`, so a run
that could not reach the model was indistinguishable from an agent that failed every task,
and the nightly published the difference as a real 0% pass rate. Every test below that
touches the failure path is asserting the *absence* of a number.

Mocking the client is right here and mocking IRIS never is: the client is the boundary this
module owns, and what these tests check is what `score_result` does with what comes back.
The live half is `tests/e2e/skill_eval/test_preflight.py`, gated on `IAD_EVAL_LIVE_SCORER`.
"""

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
        {
            "role": "assistant",
            "tool_name": "iris_compile",
            "args": {"target": "Bench.Greeter.cls"},
        },
        {"role": "tool_result", "tool_result": '{"success": true}'},
        {"role": "assistant", "text": "Class created and compiled successfully."},
    ],
}

RESOLVED_MODEL = "us.anthropic.claude-sonnet-4-6-20260514-v1:0"


def _mock_anthropic(
    score,
    reasoning="",
    model=RESOLVED_MODEL,
    input_tokens=1180,
    output_tokens=96,
    raw=None,
):
    """A client whose response carries what the real one carries: text, model, usage."""
    mock_msg = MagicMock()
    text = (
        raw if raw is not None else json.dumps({"score": score, "reasoning": reasoning})
    )
    mock_msg.content = [MagicMock(text=text)]
    mock_msg.model = model
    mock_msg.usage = MagicMock(input_tokens=input_tokens, output_tokens=output_tokens)
    mock_client = MagicMock()
    mock_client.messages.create.return_value = mock_msg
    return mock_client


def _score(client):
    with patch("judge.make_client") as mock_make:
        mock_make.return_value = client
        from judge import score_result

        return score_result(SAMPLE_TASK, SAMPLE_RESULT)


def test_score_result_returns_valid_schema():
    result = _score(_mock_anthropic(3, "Correct and efficient"))
    assert result["scored"] is True
    assert result["score"] in (0, 1, 2, 3)
    assert isinstance(result["reasoning"], str)
    assert result["scoring_mode"] == "judge"


def test_score_result_returns_score_3():
    assert _score(_mock_anthropic(3, "Perfect"))["score"] == 3


def test_score_result_scores_a_real_zero():
    """A zero the model actually assigned still comes back as a zero.

    The fix is not "no more zeros" — it is "no zeros nobody measured". A transcript the
    scorer read and rejected is a scored failure and belongs in the denominator.
    """
    result = _score(_mock_anthropic(0, "Never called a tool"))
    assert result["scored"] is True
    assert result["score"] == 0


def test_score_result_handles_api_error():
    """The inverted test. This asserted `score == 0` for a scorer that never answered."""
    with patch("judge.make_client") as mock_make:
        mock_client = MagicMock()
        mock_client.messages.create.side_effect = Exception("API error")
        mock_make.return_value = mock_client
        from judge import score_result

        result = score_result(SAMPLE_TASK, SAMPLE_RESULT)
    assert result["scored"] is False
    assert result["score"] is None, "an unreachable scorer has not measured a zero"
    assert "API error" in result["reasoning"]
    assert result["scorer_model"] is None
    assert result["input_tokens"] is None and result["output_tokens"] is None


def test_score_result_records_resolved_model():
    """The detector named in the constitution amendment for `unverified-model-id`.

    `_client.py` asks for one id and Bedrock may serve another; the report used to print a
    hardcoded `openai/gpt-4.1` regardless. What a number was scored by has to come from the
    response.
    """
    result = _score(_mock_anthropic(2, "Fine", model=RESOLVED_MODEL))
    assert result["scorer_model"] == RESOLVED_MODEL


def test_score_result_records_token_counts():
    """Cost is re-derived from measured usage (R11), so usage has to survive the call."""
    result = _score(_mock_anthropic(2, "Fine", input_tokens=1234, output_tokens=56))
    assert result["input_tokens"] == 1234
    assert result["output_tokens"] == 56


def test_a_response_missing_usage_is_still_scored():
    """An SDK that stops reporting usage costs the cost estimate, not the measurement."""
    client = _mock_anthropic(2, "Fine")
    client.messages.create.return_value.usage = None
    result = _score(client)
    assert result["scored"] is True and result["score"] == 2
    assert result["input_tokens"] is None


def test_out_of_range_score_is_unscored():
    """`7` parses as JSON and is not a verdict on a 0–3 scale.

    Clamping it would put an unmeasured number on the scale, which is the same lie as the
    fabricated zero — just harder to spot, because it lands in the middle of the range.
    """
    for bad in (7, -1, "good", 2.5):
        result = _score(_mock_anthropic(bad))
        assert result["scored"] is False, bad
        assert result["score"] is None, bad
        assert str(bad) in result["reasoning"], result["reasoning"]


def test_unparseable_response_is_unscored():
    result = _score(_mock_anthropic(None, raw="I would rather not say."))
    assert result["scored"] is False
    assert result["score"] is None


def test_missing_score_key_is_unscored():
    result = _score(_mock_anthropic(None, raw=json.dumps({"reasoning": "no score"})))
    assert result["scored"] is False
    assert result["score"] is None


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
            {
                "role": "assistant",
                "tool_name": "iris_doc_search",
                "args": {"query": "SQL execution methods"},
            },
            {"role": "tool_result", "tool_result": '{"hits": []}'},
            {"role": "assistant", "text": "%SQL.Statement and embedded SQL."},
        ],
    }
    with patch("judge.make_client") as mock_make:
        mock_make.return_value = _mock_anthropic(2, "Mostly correct")
        from judge import DOC_CATEGORY_NOTE, score_result

        score_result(doc_task, doc_result)
        call_args = mock_make.return_value.messages.create.call_args
        prompt_text = call_args[1]["messages"][0]["content"]
    assert DOC_CATEGORY_NOTE in prompt_text
