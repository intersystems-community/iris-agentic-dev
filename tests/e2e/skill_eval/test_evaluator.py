"""Unit tests for SkillEvalConfig loader and discovery — T003, plus 118 T024 (outcomes)."""

import os

import pytest
import yaml

from tests.e2e.skill_eval.evaluator import (
    SkillEvalConfig,
    SkillResult,
    compare_to_baseline,
    discover_skills,
    load_eval_config,
)

SKILLS_PACK_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "skills", "skills")
)
TASKS_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)


def test_discover_skills_finds_all():
    skills = discover_skills(SKILLS_PACK_DIR)
    assert len(skills) >= 31, f"Expected at least 31 skills, found {len(skills)}"
    assert "objectscript-review" in skills
    assert "iris-vector-ai" in skills


def test_discover_skills_requires_skill_md(tmp_path):
    (tmp_path / "has-skill-md").mkdir()
    (tmp_path / "has-skill-md" / "SKILL.md").write_text("---\nname: test\n---")
    (tmp_path / "no-skill-md").mkdir()
    skills = discover_skills(str(tmp_path))
    assert skills == ["has-skill-md"]


def test_load_eval_config_returns_config(tmp_path):
    skill_dir = tmp_path / "objectscript-review"
    skill_dir.mkdir()
    config = {
        "skill": "objectscript-review",
        "description": "Reviews ObjectScript code",
        "fire_rate_prompt": "Fix this method: ...",
        "benchmark_tasks": ["DBG-01", "DBG-02"],
        "domain_skill": False,
    }
    (skill_dir / "eval.yaml").write_text(yaml.dump(config))
    result = load_eval_config("objectscript-review", str(tmp_path))
    assert result is not None
    assert result.skill == "objectscript-review"
    assert result.benchmark_tasks == ["DBG-01", "DBG-02"]
    assert result.domain_skill is False
    assert result.isolation_prompt is None


def test_load_eval_config_returns_none_for_missing(tmp_path):
    result = load_eval_config("nonexistent-skill", str(tmp_path))
    assert result is None


def test_load_eval_config_domain_skill(tmp_path):
    skill_dir = tmp_path / "iris-vector-ai"
    skill_dir.mkdir()
    config = {
        "skill": "iris-vector-ai",
        "description": "Vector search",
        "fire_rate_prompt": "Write a VECTOR_COSINE query",
        "benchmark_tasks": [],
        "domain_skill": True,
        "isolation_prompt": "Fix this Return-in-loop bug: ...",
    }
    (skill_dir / "eval.yaml").write_text(yaml.dump(config))
    result = load_eval_config("iris-vector-ai", str(tmp_path))
    assert result is not None
    assert result.domain_skill is True
    assert result.isolation_prompt is not None


def test_skill_eval_config_is_constructible_without_the_optional_fields():
    cfg = SkillEvalConfig(
        skill="s",
        description="",
        fire_rate_prompt="p",
        benchmark_tasks=[],
        domain_skill=False,
    )
    assert cfg.implicit_fire_rate_prompt is None


# ---------------------------------------------------------------------------
# 118 T024 — the outcome derivation of data-model.md § 5
# ---------------------------------------------------------------------------


def provenance(task_ids=("DBG-01",), scorer_model="claude-sonnet-4-6", **over):
    prov = {
        "run_id": "2026-09-12T040211Z",
        "task_ids": sorted(task_ids),
        "scoring_mode": "judge",
        "scorer_model": scorer_model,
        "scorer_model_requested": "us.anthropic.claude-sonnet-4-6",
        "tool_surface": "1.4.1+fa0b694f8725",
        "runs": 3,
        "measured_at": "2026-09-12T04:11:07Z",
        "harness_commit": "7d82f7d",
    }
    prov.update(over)
    return prov


def arm(pass_rate, total=12, unscored=0):
    scored = total - unscored
    return {
        "items_total": total,
        "items_scored": scored,
        "items_unscored": unscored,
        "items_passed": 0 if pass_rate is None else round(pass_rate * scored),
        "pass_rate": pass_rate,
    }


def result(lift=0.25, prov=None, arms=None, base_rate=0.33, skill_rate=0.58):
    return SkillResult(
        skill="objectscript-guardrails",
        fire_rate=1.0,
        implicit_fire_rate=None,
        isolation_fire_rate=None,
        pass_rate_baseline=base_rate,
        pass_rate_skill=skill_rate,
        lift=lift,
        lift_delta=None,
        regression_flag=False,
        new_skill=False,
        no_task_coverage=False,
        task_ids_used=["DBG-01"],
        provenance=provenance() if prov is None else prov,
        arms=arms
        if arms is not None
        else {"baseline": arm(base_rate), "skill": arm(skill_rate)},
    )


def entry(lift=0.25, prov=None):
    return {
        "fire_rate": 1.0,
        "lift": lift,
        "pass_rate_baseline": 0.33,
        "pass_rate_skill": 0.58,
        "items": {"baseline": arm(0.33), "skill": arm(0.58)},
        "provenance": provenance() if prov is None else prov,
    }


def test_a_skill_with_no_baseline_entry_is_new(capsys):
    out = compare_to_baseline(result(), {}, threshold=0.05)
    assert out.outcome == "new_skill"
    assert out.new_skill is True
    assert out.lift_delta is None
    assert out.regression_flag is False


def test_a_matching_provenance_holds():
    out = compare_to_baseline(
        result(lift=0.27), {"objectscript-guardrails": entry(0.25)}, threshold=0.05
    )
    assert out.outcome == "held"
    assert out.outcome_reason is None
    assert out.lift_delta == pytest.approx(0.02)
    assert out.regression_flag is False


def test_a_drop_past_the_threshold_regresses():
    out = compare_to_baseline(
        result(lift=0.10), {"objectscript-guardrails": entry(0.25)}, threshold=0.05
    )
    assert out.outcome == "regressed"
    assert out.lift_delta == pytest.approx(-0.15)
    assert out.threshold_applied == pytest.approx(0.05)


def test_a_drop_inside_the_threshold_holds():
    out = compare_to_baseline(
        result(lift=0.22), {"objectscript-guardrails": entry(0.25)}, threshold=0.05
    )
    assert out.outcome == "held"


def test_the_outcome_is_one_comparison_against_the_threshold():
    """FR-020's predecessor rule required lift ≤ 0 *or* a 20-point drop as well.

    A skill going from +0.40 to +0.10 was reported as holding, because it was still net
    positive and the drop was under 20 points. Nobody could derive either constant. One
    threshold, one comparison, and the number is in `threshold_applied` for the reader.
    """
    out = compare_to_baseline(
        result(lift=0.10), {"objectscript-guardrails": entry(0.40)}, threshold=0.05
    )
    assert out.outcome == "regressed"


def test_provenance_mismatch_refuses_delta():
    """The detector for `incomparable-delta` in the staged constitution amendment.

    A Δ between a number graded by one model on one task set and a number graded by another
    on another is arithmetic, not measurement. The old code subtracted them anyway.
    """
    out = compare_to_baseline(
        result(),
        {
            "objectscript-guardrails": entry(
                prov=provenance(task_ids=["DBG-01", "DBG-99"])
            )
        },
        threshold=0.05,
    )
    assert out.outcome == "not_comparable"
    assert out.lift_delta is None
    assert out.regression_flag is False
    assert "task_ids" in out.outcome_reason


@pytest.mark.parametrize(
    "field,value",
    [
        ("task_ids", ["DBG-02"]),
        ("scoring_mode", "assertion"),
        ("scorer_model", "claude-haiku-4-5"),
    ],
)
def test_the_refusal_names_the_field_that_differed(field, value):
    out = compare_to_baseline(
        result(),
        {"objectscript-guardrails": entry(prov=provenance(**{field: value}))},
        threshold=0.05,
    )
    assert out.outcome == "not_comparable"
    assert field in out.outcome_reason


def test_a_v1_entry_is_not_comparable_and_says_why():
    out = compare_to_baseline(
        result(), {"objectscript-guardrails": entry(prov=None) | {"provenance": None}}
    )
    assert out.outcome == "not_comparable"
    assert out.outcome_reason == "no provenance recorded"


@pytest.mark.parametrize("empty_arm", ["baseline", "skill"])
def test_an_arm_that_scored_nothing_is_not_comparable(empty_arm):
    """`pass_rate is None` is not `0.0`, and the difference is the whole spec.

    An arm with no scored items has no rate. Subtracting from it produced the plausible
    `0.00` lift that the nightly published for six of nine skills for a month.
    """
    arms = {"baseline": arm(0.33), "skill": arm(0.58)}
    arms[empty_arm] = arm(None, total=12, unscored=12)
    out = compare_to_baseline(
        result(lift=None, arms=arms), {"objectscript-guardrails": entry()}
    )
    assert out.outcome == "not_comparable"
    assert out.outcome_reason == "no scored items"
    assert out.lift_delta is None


def test_a_changed_tool_surface_annotates_without_suppressing():
    """The release that changes the tool surface is the release the gate is for."""
    out = compare_to_baseline(
        result(lift=0.27),
        {
            "objectscript-guardrails": entry(
                0.25, prov=provenance(tool_surface="1.3.0+000000000000")
            )
        },
        threshold=0.05,
    )
    assert out.outcome == "held"
    assert out.lift_delta == pytest.approx(0.02)
    assert out.surface_note and "1.3.0+000000000000" in out.surface_note


def test_regression_flag_is_read_from_the_outcome_and_not_computed_twice():
    """Two fields, one decision. `regression_flag` survives only for shard-merge compatibility."""
    for lift, expected in ((0.10, True), (0.25, False), (0.40, False)):
        out = compare_to_baseline(
            result(lift=lift), {"objectscript-guardrails": entry(0.25)}, threshold=0.05
        )
        assert out.regression_flag is expected
        assert out.regression_flag == (out.outcome == "regressed")


def test_the_threshold_used_is_recorded_on_the_result():
    """FR-010: the printed threshold must be the one the outcome was computed under."""
    out = compare_to_baseline(
        result(lift=0.10), {"objectscript-guardrails": entry(0.25)}, threshold=0.20
    )
    assert out.threshold_applied == pytest.approx(0.20)
    assert out.outcome == "held", "a 15-point drop is inside a 20-point threshold"


def test_the_derivation_order_puts_new_skill_before_comparability():
    """§5 order, first match wins: no entry at all is `new_skill`, not `not_comparable`."""
    out = compare_to_baseline(result(prov=None), {}, threshold=0.05)
    assert out.outcome == "new_skill"
