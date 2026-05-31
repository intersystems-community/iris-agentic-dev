"""Unit tests for lift measurement — T013."""
import pytest
from unittest.mock import patch, MagicMock
from tests.e2e.skill_eval.lift import compute_pass_rate, compute_lift_from_scores, format_transcript


def make_events(tool_calls=None, text="The fix is correct."):
    events = []
    for tool in (tool_calls or []):
        events.append({
            "type": "tool_use",
            "part": {"tool": tool, "state": {"status": "completed", "input": {}, "output": "ok"}},
        })
    events.append({"type": "text", "part": {"text": text, "time": {"end": 1}}})
    return events


def test_compute_pass_rate_all_pass():
    scores = [{"score": 3}, {"score": 2}, {"score": 3}]
    assert compute_pass_rate(scores) == pytest.approx(1.0)


def test_compute_pass_rate_none_pass():
    scores = [{"score": 0}, {"score": 1}, {"score": 1}]
    assert compute_pass_rate(scores) == pytest.approx(0.0)


def test_compute_pass_rate_mixed():
    scores = [{"score": 2}, {"score": 1}, {"score": 3}]
    assert compute_pass_rate(scores) == pytest.approx(2 / 3)


def test_compute_lift():
    result = compute_lift_from_scores(
        baseline_scores=[{"score": 1}, {"score": 1}],
        skill_scores=[{"score": 3}, {"score": 3}],
    )
    assert result["pass_rate_baseline"] == pytest.approx(0.0)
    assert result["pass_rate_skill"] == pytest.approx(1.0)
    assert result["lift"] == pytest.approx(1.0)


def test_format_transcript_includes_tools():
    events = make_events(tool_calls=["iris_compile", "iris_execute"])
    transcript = format_transcript(events)
    assert "iris_compile" in transcript
    assert "iris_execute" in transcript
    assert "The fix is correct" in transcript
