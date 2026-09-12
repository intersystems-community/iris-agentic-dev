"""Unit tests for the scored-item invariant and pass-rate arithmetic — 118 T009.

Written before `scoring.py`. The one rule everything here exists to hold: an item the scorer
could not score has no score, and a rate computed over items that were never scored is not a
rate. The harness used to substitute `0` for both, so `0.00` in the report meant either "the
skill failed every task" or "nothing was measured" and nobody could tell which.
"""

import pytest

from tests.e2e.skill_eval import scoring


def item(
    scored=True, score=3, arm="skill", task_id="IRIS-A", run_index=0, mode="judge"
):
    return scoring.ScoredItem(
        task_id=task_id,
        arm=arm,
        run_index=run_index,
        scored=scored,
        score=None if not scored else score,
        reasoning="",
        scoring_mode=mode,
        scorer_model="claude-sonnet-4-6" if mode == "judge" else None,
    )


# ---------------------------------------------------------------------------
# The invariant
# ---------------------------------------------------------------------------


def test_an_unscored_item_cannot_carry_a_score():
    """`scored is False` ⟹ `score is None`. Constructing the old shape has to fail loudly."""
    with pytest.raises(ValueError) as excinfo:
        scoring.ScoredItem(
            task_id="IRIS-A",
            arm="skill",
            run_index=0,
            scored=False,
            score=0,
            reasoning="Judge error: no credential",
            scoring_mode="judge",
        )
    assert "score" in str(excinfo.value)


def test_a_scored_item_must_carry_a_score():
    with pytest.raises(ValueError):
        scoring.ScoredItem(
            task_id="IRIS-A",
            arm="skill",
            run_index=0,
            scored=True,
            score=None,
            reasoning="",
            scoring_mode="judge",
        )


@pytest.mark.parametrize("bad", [7, -1, 2.5, "3"])
def test_a_score_off_the_scale_is_refused(bad):
    with pytest.raises(ValueError):
        scoring.ScoredItem(
            task_id="IRIS-A",
            arm="skill",
            run_index=0,
            scored=True,
            score=bad,
            reasoning="",
            scoring_mode="judge",
        )


def test_from_verdict_carries_the_scorer_fields():
    verdict = {
        "scored": True,
        "score": 2,
        "reasoning": "Used iris_query.",
        "scoring_mode": "judge",
        "scorer_model": "claude-sonnet-4-6",
        "input_tokens": 1180,
        "output_tokens": 96,
    }
    built = scoring.ScoredItem.from_verdict(
        verdict, task_id="IRIS-A", arm="skill", run_index=1
    )
    assert built.score == 2
    assert built.scorer_model == "claude-sonnet-4-6"
    assert built.input_tokens == 1180
    assert built.run_index == 1


def test_from_verdict_refuses_a_score_without_the_scored_flag():
    """A verdict from an unpatched caller must not be read as scored by default.

    Defaulting `scored` to True would reintroduce the bug through the back door: the old
    `{"score": 0}` payload would sail through as a measured zero.
    """
    with pytest.raises(KeyError):
        scoring.ScoredItem.from_verdict(
            {"score": 0, "reasoning": "Judge error"},
            task_id="IRIS-A",
            arm="skill",
            run_index=0,
        )


def test_passing_is_two_or_better():
    assert item(score=2).passed is True
    assert item(score=1).passed is False
    assert item(scored=False).passed is False


# ---------------------------------------------------------------------------
# Pass rate
# ---------------------------------------------------------------------------


def test_pass_rate_denominates_on_scored_items_only():
    """Four items, two scorer failures, one pass out of the two that were scored."""
    items = [item(score=3), item(score=0), item(scored=False), item(scored=False)]
    assert scoring.compute_pass_rate(items) == 0.5


def test_pass_rate_of_nothing_is_none_not_zero():
    """`None` and `0.0` are different facts, and the report prints them differently."""
    assert scoring.compute_pass_rate([]) is None
    assert scoring.compute_pass_rate([item(scored=False), item(scored=False)]) is None


def test_pass_rate_of_all_failures_is_zero():
    assert scoring.compute_pass_rate([item(score=0), item(score=1)]) == 0.0


def test_pass_rate_accepts_verdict_dicts():
    """`lift.py` accumulates dicts, so the same function has to read both shapes."""
    dicts = [
        {"scored": True, "score": 3},
        {"scored": True, "score": 1},
        {"scored": False, "score": None},
    ]
    assert scoring.compute_pass_rate(dicts) == 0.5


def test_arm_result_counts_every_bucket():
    arm = scoring.ArmResult.from_items(
        [item(score=3), item(score=2), item(score=0), item(scored=False)]
    )
    assert (
        arm.items_total,
        arm.items_scored,
        arm.items_unscored,
        arm.items_passed,
    ) == (
        4,
        3,
        1,
        2,
    )
    assert arm.pass_rate == pytest.approx(2 / 3)


def test_an_arm_that_scored_nothing_has_no_pass_rate():
    arm = scoring.ArmResult.from_items([item(scored=False)] * 3)
    assert arm.items_scored == 0
    assert arm.pass_rate is None


# ---------------------------------------------------------------------------
# Run validity
# ---------------------------------------------------------------------------


def test_unscored_share_is_run_wide():
    items = [item() for _ in range(9)] + [item(scored=False)]
    assert scoring.unscored_share(items) == pytest.approx(0.1)


def test_a_run_with_no_items_has_no_unscored_share():
    assert scoring.unscored_share([]) == 0.0


def test_exactly_the_limit_is_still_valid():
    """ "Exceeds 10%" is the contract, so 10% passes. Written down because the next person
    to read the sentence will guess."""
    items = [item() for _ in range(9)] + [item(scored=False)]
    assert scoring.unscored_share(items) == pytest.approx(scoring.UNSCORED_LIMIT)
    assert scoring.run_is_valid(items) is True


def test_over_the_limit_is_invalid():
    items = [item() for _ in range(8)] + [item(scored=False), item(scored=False)]
    assert scoring.unscored_share(items) == pytest.approx(0.2)
    assert scoring.run_is_valid(items) is False


def test_a_run_that_scored_nothing_at_all_is_invalid():
    assert scoring.run_is_valid([item(scored=False)] * 5) is False


# ---------------------------------------------------------------------------
# What an invalid run is allowed to do — FR-003
# ---------------------------------------------------------------------------


def test_an_invalid_run_may_not_write_the_baseline_even_when_asked():
    """`--update-baseline` on an invalid run is the failure this whole spec exists to undo.

    The 2026-08 baseline holds nine entries of fabricated zeros because the run that wrote
    them was asked to and nothing checked whether it had measured anything.
    """
    assert scoring.baseline_write_allowed(run_valid=True, update_requested=True) is True
    assert (
        scoring.baseline_write_allowed(run_valid=False, update_requested=True) is False
    )
    assert (
        scoring.baseline_write_allowed(run_valid=True, update_requested=False) is False
    )


def test_an_invalid_run_reports_no_delta():
    assert scoring.delta_reportable(run_valid=True) is True
    assert scoring.delta_reportable(run_valid=False) is False


def test_exit_code_says_whether_money_was_spent():
    """1 and 2 are not interchangeable: 2 means nothing was spent, 1 means it was."""
    assert scoring.EXIT_MEASURED == 0
    assert scoring.EXIT_INTEGRITY == 1
    assert scoring.EXIT_PREFLIGHT == 2
