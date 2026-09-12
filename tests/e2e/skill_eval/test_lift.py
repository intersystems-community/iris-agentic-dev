"""Unit tests for lift measurement — T013, rewritten for 118 T016.

The pass-rate cases moved to the new contract: an item carries `scored` alongside `score`, and
a rate is computed over the items that were scored. The old cases here passed bare `{"score":
N}` dicts, which is the shape that let an unreachable scorer read as a failing agent all the
way through to a published number.
"""

import pytest

from tests.e2e.skill_eval.lift import (
    compute_lift_from_scores,
    compute_pass_rate,
    format_transcript,
    verdict_from_assertions,
    verdict_from_patterns,
)


def make_events(tool_calls=None, text="The fix is correct."):
    events = []
    for tool in tool_calls or []:
        events.append(
            {
                "type": "tool_use",
                "part": {
                    "tool": tool,
                    "state": {"status": "completed", "input": {}, "output": "ok"},
                },
            }
        )
    events.append({"type": "text", "part": {"text": text, "time": {"end": 1}}})
    return events


def scored(score, mode="judge"):
    return {"scored": True, "score": score, "scoring_mode": mode}


UNSCORED = {"scored": False, "score": None, "scoring_mode": "judge"}


def test_compute_pass_rate_all_pass():
    assert compute_pass_rate([scored(3), scored(2), scored(3)]) == pytest.approx(1.0)


def test_compute_pass_rate_none_pass():
    assert compute_pass_rate([scored(0), scored(1), scored(1)]) == pytest.approx(0.0)


def test_compute_pass_rate_mixed():
    assert compute_pass_rate([scored(2), scored(1), scored(3)]) == pytest.approx(2 / 3)


def test_compute_pass_rate_is_the_shared_one():
    """`lift.compute_pass_rate` is `scoring.compute_pass_rate`, not a second implementation.

    Two copies of this arithmetic is how one of them kept counting unscored items: the module
    that reported to the baseline and the module that computed the lift disagreed silently.
    """
    from tests.e2e.skill_eval import scoring

    assert compute_pass_rate is scoring.compute_pass_rate


def test_compute_lift():
    result = compute_lift_from_scores(
        baseline_scores=[scored(1), scored(1)],
        skill_scores=[scored(3), scored(3)],
    )
    assert result["pass_rate_baseline"] == pytest.approx(0.0)
    assert result["pass_rate_skill"] == pytest.approx(1.0)
    assert result["lift"] == pytest.approx(1.0)


def test_lift_counts_what_each_arm_scored():
    """The denominators travel with the rates, so a report can show `6/8 scored`."""
    result = compute_lift_from_scores(
        baseline_scores=[scored(0), scored(3), UNSCORED],
        skill_scores=[scored(3), scored(3), scored(3)],
    )
    assert result["items_scored_baseline"] == 2
    assert result["items_unscored_baseline"] == 1
    assert result["items_scored_skill"] == 3
    assert result["pass_rate_baseline"] == pytest.approx(0.5)


def test_an_arm_that_scored_nothing_has_no_lift():
    """No baseline number means no subtraction. `0.0` here would read as "no improvement"."""
    result = compute_lift_from_scores(
        baseline_scores=[UNSCORED, UNSCORED],
        skill_scores=[scored(3), scored(3)],
    )
    assert result["pass_rate_baseline"] is None
    assert result["lift"] is None
    assert result["pass_rate_skill"] == pytest.approx(1.0)


def test_lift_reports_the_run_wide_unscored_share():
    result = compute_lift_from_scores(
        baseline_scores=[scored(3)] * 4 + [UNSCORED],
        skill_scores=[scored(3)] * 5,
    )
    assert result["items_unscored"] == 1
    assert result["unscored_share"] == pytest.approx(0.1)
    assert result["run_valid"] is True


def test_a_run_over_the_unscored_limit_is_invalid():
    result = compute_lift_from_scores(
        baseline_scores=[scored(3), UNSCORED],
        skill_scores=[scored(3), UNSCORED],
    )
    assert result["run_valid"] is False


def test_lift_records_which_models_scored_it():
    """Comparability depends on the grader, so the arms' scorer models travel with the run."""
    a = {**scored(3), "scorer_model": "claude-sonnet-4-6"}
    b = {**scored(2), "scorer_model": "claude-sonnet-4-6"}
    result = compute_lift_from_scores(baseline_scores=[a], skill_scores=[b])
    assert result["scorer_models"] == ["claude-sonnet-4-6"]


# ---------------------------------------------------------------------------
# The two deterministic scoring modes
# ---------------------------------------------------------------------------


def test_tool_assertion_scoring_is_scored_with_no_model():
    """Assertion mode calls nothing, so it has no scorer model and no tokens — but it is
    scored: the assertion is a measurement."""
    verdict = verdict_from_assertions(
        passed=True, assertions=["iris_agentic_dev:iris_query"]
    )
    assert verdict["scored"] is True
    assert verdict["score"] == 3
    assert verdict["scoring_mode"] == "assertion"
    assert verdict["scorer_model"] is None
    assert verdict["input_tokens"] is None


def test_a_failed_tool_assertion_is_a_measured_zero():
    verdict = verdict_from_assertions(passed=False, assertions=["iris_query"])
    assert verdict["scored"] is True
    assert verdict["score"] == 0
    assert "iris_query" in verdict["reasoning"]


def test_pattern_scoring_declares_its_mode():
    verdict = verdict_from_patterns(2, "Compiled OK but patterns not fully met")
    assert verdict["scored"] is True
    assert verdict["score"] == 2
    assert verdict["scoring_mode"] == "pattern"
    assert verdict["scorer_model"] is None


def test_format_transcript_includes_tools():
    events = make_events(tool_calls=["iris_compile", "iris_execute"])
    turns = format_transcript(events)
    tool_names = [t.get("tool_name") for t in turns if t.get("tool_name")]
    assert "iris_compile" in tool_names
    assert "iris_execute" in tool_names
    texts = [t.get("text", "") for t in turns]
    assert any("The fix is correct" in t for t in texts)
