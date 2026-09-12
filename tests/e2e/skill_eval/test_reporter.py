"""Unit tests for reporter — T009, extended for 118 T025.

Every column and footer line asserted below exists because its absence hid something in the
runs of 2026-08/09. That report printed a skill name, a fire rate, and a lift, and nothing
else: no scorer, no denominator, no threshold. `objectscript-review  +0% ✓` was indistinguishable
from a skill that had been measured and held.
"""

import json
import os

import pytest

from tests.e2e.skill_eval.evaluator import SkillResult
from tests.e2e.skill_eval.reporter import EvalRun, print_summary, write_result


def arm(pass_rate, total=12, unscored=0):
    scored = total - unscored
    return {
        "items_total": total,
        "items_scored": scored,
        "items_unscored": unscored,
        "items_passed": 0 if pass_rate is None else round(pass_rate * scored),
        "pass_rate": pass_rate,
    }


def provenance(**over):
    prov = {
        "run_id": "2026-09-12T040211Z",
        "task_ids": ["DBG-01"],
        "scoring_mode": "judge",
        "scorer_model": "claude-sonnet-4-6",
        "scorer_model_requested": "us.anthropic.claude-sonnet-4-6",
        "tool_surface": "1.4.1+fa0b694f8725",
        "runs": 3,
        "measured_at": "2026-09-12T04:11:07Z",
        "harness_commit": "7d82f7d",
    }
    prov.update(over)
    return prov


def make_result(
    skill,
    lift=None,
    fire_rate=1.0,
    regression=False,
    no_coverage=False,
    outcome=None,
    outcome_reason=None,
    delta=None,
    arms=None,
    prov=None,
    surface_note=None,
):
    if outcome is None:
        outcome = (
            "regressed"
            if regression
            else ("held" if lift is not None else "not_comparable")
        )
    return SkillResult(
        skill=skill,
        fire_rate=fire_rate if not no_coverage else None,
        implicit_fire_rate=None,
        isolation_fire_rate=None,
        pass_rate_baseline=0.71 if lift else None,
        pass_rate_skill=(0.71 + lift) if lift else None,
        lift=lift,
        lift_delta=delta,
        regression_flag=outcome == "regressed",
        new_skill=outcome == "new_skill",
        no_task_coverage=no_coverage,
        task_ids_used=["DBG-01"] if lift else [],
        outcome=outcome,
        outcome_reason=outcome_reason,
        arms=arms
        if arms is not None
        else (
            None
            if no_coverage
            else {"baseline": arm(0.71), "skill": arm(0.71 + (lift or 0))}
        ),
        provenance=prov
        if prov is not None
        else (None if no_coverage else provenance()),
        threshold_applied=0.05,
        surface_note=surface_note,
    )


def make_eval_run(skills, **over):
    fields = {
        "run_id": "2026-05-31T000000",
        "model": "amazon-bedrock/test",
        "judge_model": "claude-sonnet-4-6",
        "timestamp": "2026-05-31T00:00:00Z",
        "regression_threshold": 0.05,
        "skills": skills,
        "summary": {
            "regressions": [],
            "improvements": [],
            "uncovered": ["iris-docs"],
            "estimated_cost_usd": 1.50,
        },
        "scorer_model_requested": "us.anthropic.claude-sonnet-4-6",
        "tool_surface": "1.4.1+fa0b694f8725",
        "run_valid": True,
    }
    fields.update(over)
    return EvalRun(**fields)


def test_print_summary_contains_skill_name(capsys):
    run = make_eval_run([make_result("objectscript-review", lift=0.29)])
    print_summary(run)
    captured = capsys.readouterr()
    assert "objectscript-review" in captured.out
    assert "0.29" in captured.out or "29" in captured.out


def test_print_summary_shows_a_regressed_row(capsys):
    run = make_eval_run(
        [make_result("objectscript-review", lift=0.10, regression=True, delta=-0.19)]
    )
    print_summary(run)
    assert "regress" in capsys.readouterr().out.lower()


def test_write_result_creates_json(tmp_path):
    run = make_eval_run([make_result("objectscript-review", lift=0.29)])
    path = write_result(run, str(tmp_path))
    assert os.path.exists(path)
    with open(path) as f:
        data = json.load(f)
    assert data["run_id"] == "2026-05-31T000000"
    assert len(data["skills"]) == 1
    assert data["skills"][0]["skill"] == "objectscript-review"
    assert data["summary"]["estimated_cost_usd"] == pytest.approx(1.50)


def test_write_result_carries_the_outcome_and_its_provenance(tmp_path):
    """The shard file is what the aggregate job reads; a dropped field is a dropped fact."""
    run = make_eval_run([make_result("objectscript-review", lift=0.29, delta=0.02)])
    data = json.loads(open(write_result(run, str(tmp_path))).read())
    entry = data["skills"][0]
    assert entry["outcome"] == "held"
    assert entry["threshold_applied"] == pytest.approx(0.05)
    assert entry["provenance"]["scorer_model"] == "claude-sonnet-4-6"
    assert entry["arms"]["skill"]["items_scored"] == 12
    assert entry["regression_flag"] == (entry["outcome"] == "regressed")


# ---------------------------------------------------------------------------
# 118 T025 — the columns and footer of contracts/eval-run.md
# ---------------------------------------------------------------------------


def test_the_table_has_the_mode_scored_and_outcome_columns(capsys):
    run = make_eval_run([make_result("iris-connectivity", lift=0.29, delta=0.04)])
    print_summary(run)
    out = capsys.readouterr().out
    header = next(
        line for line in out.splitlines() if "skill" in line and "mode" in line
    )
    for column in ("mode", "scored", "lift", "outcome"):
        assert column in header, f"{column} missing from the header: {header!r}"
    row = next(
        line for line in out.splitlines() if line.startswith("iris-connectivity")
    )
    assert "judge" in row
    assert "24/24" in row, f"scored is both arms summed: {row!r}"
    assert "held" in row


def test_a_partly_unscored_skill_shows_its_denominator(capsys):
    """`22/24` is a visible fact in the table, not a footnote nobody reads."""
    arms = {"baseline": arm(0.38), "skill": arm(0.50, unscored=2)}
    run = make_eval_run([make_result("objectscript-review", lift=0.12, arms=arms)])
    print_summary(run)
    row = next(
        line
        for line in capsys.readouterr().out.splitlines()
        if line.startswith("objectscript-review")
    )
    assert "22/24" in row


def test_no_comparison_prints_a_dash_and_a_flat_one_prints_a_number(capsys):
    run = make_eval_run(
        [
            make_result(
                "objectscript-review",
                lift=0.12,
                outcome="not_comparable",
                outcome_reason="task_ids differ",
                delta=None,
            ),
            make_result("iris-connectivity", lift=0.29, outcome="held", delta=0.0),
        ]
    )
    print_summary(run)
    lines = capsys.readouterr().out.splitlines()
    refused = next(line for line in lines if line.startswith("objectscript-review"))
    flat = next(line for line in lines if line.startswith("iris-connectivity"))
    assert "—" in refused, f"no comparison must not print a number: {refused!r}"
    assert "0.00" in flat, f"a flat comparison is a measurement: {flat!r}"


def test_a_not_comparable_row_prints_its_reason(capsys):
    run = make_eval_run(
        [
            make_result(
                "objectscript-review",
                lift=0.12,
                outcome="not_comparable",
                outcome_reason="scorer_model differs: claude-haiku-4-5 → claude-sonnet-4-6",
            )
        ]
    )
    print_summary(run)
    out = capsys.readouterr().out
    assert "not comparable" in out, "the enum is read as English in the table"
    assert "scorer_model differs" in out


def test_a_changed_tool_surface_prints_beside_the_delta(capsys):
    run = make_eval_run(
        [
            make_result(
                "objectscript-review",
                lift=0.12,
                delta=0.02,
                surface_note="tool surface 1.3.0+000000000000 → 1.4.1+fa0b694f8725",
            )
        ]
    )
    print_summary(run)
    out = capsys.readouterr().out
    assert "1.3.0+000000000000 → 1.4.1+fa0b694f8725" in out
    assert "held" in out, "the annotation must not suppress the comparison"


def test_the_footer_prints_on_a_green_run_too(capsys):
    """The broken run printed none of these four lines, which is why it looked normal."""
    run = make_eval_run([make_result("iris-connectivity", lift=0.29, delta=0.01)])
    print_summary(run)
    out = capsys.readouterr().out
    assert "scorer: claude-sonnet-4-6" in out
    assert "requested us.anthropic.claude-sonnet-4-6" in out
    assert "tool surface: 1.4.1+fa0b694f8725" in out
    assert "threshold: 0.05" in out
    assert "unscored: 0/24" in out
    assert "run valid: yes" in out


def test_the_footer_reports_the_unscored_share_over_all_items(capsys):
    run = make_eval_run(
        [
            make_result(
                "objectscript-review",
                lift=0.12,
                arms={"baseline": arm(0.38), "skill": arm(0.50, unscored=2)},
            ),
            make_result("iris-connectivity", lift=0.29),
        ]
    )
    print_summary(run)
    out = capsys.readouterr().out
    assert "unscored: 2/48" in out
    assert "4.2%" in out


def test_an_invalid_run_says_so_in_the_footer(capsys):
    run = make_eval_run(
        [make_result("objectscript-review", lift=None, outcome="not_comparable")],
        run_valid=False,
    )
    print_summary(run)
    assert "run valid: no" in capsys.readouterr().out


def test_the_reruns_line_prints_only_when_there_was_a_rerun(capsys):
    clean = make_eval_run([make_result("iris-connectivity", lift=0.29)])
    print_summary(clean)
    assert "re-runs merged" not in capsys.readouterr().out

    merged = make_eval_run(
        [make_result("iris-connectivity", lift=0.29)],
        reruns={"iris-connectivity": "2026-09-12T034002Z"},
    )
    print_summary(merged)
    out = capsys.readouterr().out
    assert "re-runs merged: iris-connectivity" in out
    assert "2026-09-12T034002Z" in out, "the discarded run_id is the point of the line"


def test_a_skill_with_no_coverage_still_gets_a_row(capsys):
    run = make_eval_run([make_result("iris-docs", no_coverage=True)])
    print_summary(run)
    out = capsys.readouterr().out
    assert "iris-docs" in out
    assert "no coverage" in out
