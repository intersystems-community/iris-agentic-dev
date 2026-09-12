"""Unit tests for cost estimator — T007, extended for 118 T011."""

import pytest

from tests.e2e.skill_eval import cost_estimator
from tests.e2e.skill_eval.cost_estimator import (
    _SECONDS_PER_TASK,
    estimate,
    estimate_skill,
    format_dry_run,
)
from tests.e2e.skill_eval.evaluator import SkillEvalConfig


def make_config(
    benchmark_tasks, domain_skill=False, implicit=False, skill="test-skill"
):
    return SkillEvalConfig(
        skill=skill,
        description="",
        fire_rate_prompt="Fix this",
        benchmark_tasks=benchmark_tasks,
        domain_skill=domain_skill,
        isolation_prompt="Fix Return-in-loop" if domain_skill else None,
        implicit_fire_rate_prompt="Something that does not name the skill"
        if implicit
        else None,
    )


def test_estimate_single_skill_with_benchmark():
    # 1 skill, 3 benchmark tasks, 3 runs
    # fire-rate: 3 runs = 3 task calls
    # lift: 3 benchmark tasks × 2 conditions × 3 runs = 18 task calls + 18 judge calls
    # total: 21 task calls, 18 judge calls
    configs = [make_config(["DBG-01", "DBG-02", "DBG-03"])]
    est = estimate(configs, runs=3)
    assert est["task_runs"] == 21
    assert est["judge_calls"] == 18
    assert est["cost_usd"] > 0
    assert est["time_minutes"] > 0


def test_estimate_domain_skill_no_benchmark():
    # domain skill: 3 fire-rate + 3 isolation = 6 task calls, 0 judge calls
    configs = [make_config([], domain_skill=True)]
    est = estimate(configs, runs=3)
    assert est["task_runs"] == 6
    assert est["judge_calls"] == 0


def test_estimate_multiple_skills():
    configs = [make_config(["DBG-01"]), make_config(["DBG-01", "DBG-02"])]
    est = estimate(configs, runs=3)
    # skill1: 3 fire + 1*2*3=6 benchmark + 6 judge
    # skill2: 3 fire + 2*2*3=12 benchmark + 12 judge
    assert est["task_runs"] == 3 + 6 + 3 + 12  # 24
    assert est["judge_calls"] == 6 + 12  # 18


def test_estimate_counts_the_implicit_fire_rate_runs():
    """The implicit fire-rate is a third full pass over the model, and it was free.

    `_run_skill` calls `measure_fire_rate` a second time whenever the config has an
    `implicit_fire_rate_prompt`, so a skill with one costs 2×runs before lift is even
    measured. The estimator only ever counted the explicit pass, which is how the nightly
    workflow came to be budgeted at 75 minutes for a suite that needs ~100: the estimate
    read 67.2 min, under the timeout, and the run was cancelled at 1h15m every night.
    """
    without = estimate([make_config([], implicit=False)], runs=5)
    with_implicit = estimate([make_config([], implicit=True)], runs=5)
    assert without["task_runs"] == 5
    assert with_implicit["task_runs"] == 10, (
        "a skill with an implicit_fire_rate_prompt runs the fire-rate probe twice"
    )
    assert with_implicit["time_minutes"] > without["time_minutes"]


def test_seconds_per_task_matches_a_measured_run():
    """Calibrated against run 34093536537 (2026-09-07): 185 sessions in 4,122 s = 22.3 s.

    The original 15 s was a guess made before the harness ever ran on a GHA runner. Every
    session is an opencode subprocess doing real tool calls against a container in the same
    VM; it does not finish in 15 s. If this constant drops back under 20 the whole budget
    silently understates again.
    """
    assert _SECONDS_PER_TASK >= 20


def test_estimate_skill_is_the_per_shard_budget():
    """One shard is one skill, so the per-skill estimate is what a shard timeout must cover."""
    configs = [
        make_config(["DBG-01"], skill="small"),
        make_config(["DBG-01", "DBG-02", "DBG-03"], domain_skill=True, skill="big"),
    ]
    per_skill = {c.skill: estimate_skill(c, runs=5) for c in configs}
    assert per_skill["small"]["task_runs"] == 5 + 1 * 2 * 5
    assert per_skill["big"]["task_runs"] == 5 + 5 + 3 * 2 * 5
    assert per_skill["big"]["time_minutes"] > per_skill["small"]["time_minutes"]
    # The whole-suite estimate is the sum of its shards — no double counting, no gaps.
    total = estimate(configs, runs=5)
    assert total["task_runs"] == sum(e["task_runs"] for e in per_skill.values())
    assert total["judge_calls"] == sum(e["judge_calls"] for e in per_skill.values())


def test_format_dry_run_contains_key_fields():
    configs = [make_config(["DBG-01"])]
    est = estimate(configs, runs=3)
    output = format_dry_run(est, n_covered=1, n_uncovered=23)
    assert "Estimated cost" in output
    assert "Run with --yes" in output
    assert "task runs" in output.lower() or "Task runs" in output


# ---------------------------------------------------------------------------
# 118 T011 — cost from measured usage, keyed by the model that actually scored
# ---------------------------------------------------------------------------


RESOLVED = "us.anthropic.claude-sonnet-4-6"


def scored(model=RESOLVED, input_tokens=1000, output_tokens=100):
    return {
        "scored": True,
        "score": 2,
        "scoring_mode": "judge",
        "scorer_model": model,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
    }


def test_scorer_cost_keyed_by_resolved_model(monkeypatch):
    """The detector named in the constitution amendment for `unverified-model-id`.

    `_client.py` asks for one id; the response says which model answered. The old estimate
    multiplied a call count by a Haiku rate while the calls were being served by Sonnet —
    the printed cost was wrong by the ratio between two price lists, and the report labelled
    it "Haiku" either way.
    """
    monkeypatch.setattr(
        cost_estimator, "SCORER_RATES", {RESOLVED: (3.00, 15.00)}, raising=False
    )
    record = cost_estimator.scorer_cost(
        [scored(input_tokens=1_000_000, output_tokens=1_000_000)]
    )
    assert record["cost_usd"] == pytest.approx(18.00)
    assert record["models"] == [RESOLVED]


def test_scorer_cost_sums_measured_tokens(monkeypatch):
    monkeypatch.setattr(
        cost_estimator, "SCORER_RATES", {RESOLVED: (3.00, 15.00)}, raising=False
    )
    record = cost_estimator.scorer_cost([scored(), scored(), scored()])
    assert record["input_tokens"] == 3000
    assert record["output_tokens"] == 300
    assert record["cost_usd"] == pytest.approx(
        3 * (3.00 / 1e6 * 1000 + 15.00 / 1e6 * 100)
    )


def test_an_unpriced_model_yields_no_cost_rather_than_a_wrong_one(monkeypatch):
    """A model the table does not know is reported as unpriced, not billed at a guess.

    Printing a number derived from a rate nobody checked is how the old estimate got to be
    wrong quietly. `None` makes the gap visible.
    """
    monkeypatch.setattr(cost_estimator, "SCORER_RATES", {}, raising=False)
    record = cost_estimator.scorer_cost([scored(model="something-new")])
    assert record["cost_usd"] is None
    assert record["unpriced_models"] == ["something-new"]


def test_unscored_items_cost_nothing():
    """No answer, no tokens, no charge — and no silent None arithmetic either."""
    record = cost_estimator.scorer_cost(
        [
            {
                "scored": False,
                "score": None,
                "scorer_model": None,
                "input_tokens": None,
                "output_tokens": None,
            }
        ]
    )
    assert record["input_tokens"] == 0
    assert record["cost_usd"] == 0.0


def test_assertion_mode_items_cost_nothing():
    """Tool-assertion and pattern scoring call no model, so they carry no scorer cost."""
    record = cost_estimator.scorer_cost(
        [
            {
                "scored": True,
                "score": 3,
                "scoring_mode": "assertion",
                "scorer_model": None,
                "input_tokens": None,
                "output_tokens": None,
            }
        ]
    )
    assert record["cost_usd"] == 0.0
    assert record["models"] == []


def test_the_cost_line_names_the_model_not_a_family(monkeypatch):
    """ "Judge calls (Haiku)" was a label, not a fact. The report has to name what scored."""
    monkeypatch.setattr(
        cost_estimator, "SCORER_RATES", {RESOLVED: (3.00, 15.00)}, raising=False
    )
    line = cost_estimator.format_scorer_cost(cost_estimator.scorer_cost([scored()]))
    assert RESOLVED in line
    assert "Haiku" not in line


def test_the_cost_line_says_so_when_it_cannot_price_the_run(monkeypatch):
    monkeypatch.setattr(cost_estimator, "SCORER_RATES", {}, raising=False)
    line = cost_estimator.format_scorer_cost(
        cost_estimator.scorer_cost([scored(model="mystery")])
    )
    assert "mystery" in line
    assert "no rate" in line.lower() or "unpriced" in line.lower()


def test_the_shipped_rate_table_prices_the_model_the_scorer_asks_for():
    """The requested model must be priceable, or every run prints an unpriced cost.

    Keyed on `_client.haiku_model()` rather than a literal so a change there fails here
    instead of silently landing in the unpriced bucket. Asserted through `_rate_for` and not
    as dict membership: `haiku_model()` is Bedrock's id on a machine with a Bedrock credential
    and the dated direct id (`claude-haiku-4-5-20251001`) on one without, and both must price.
    Dict membership passed on my laptop and failed on the runner for exactly that reason.
    """
    import tests.e2e.skill_eval  # noqa: F401  — sys.path shim for `runner`
    from runner._client import haiku_model

    assert cost_estimator._rate_for(haiku_model()) is not None, (
        f"{haiku_model()} has no declared rate, so the run's cost cannot be derived"
    )


@pytest.mark.parametrize(
    "model",
    [
        "us.anthropic.claude-sonnet-4-6",
        "us.anthropic.claude-sonnet-4-6-20260514-v1:0",
        "claude-sonnet-4-6",
        "claude-haiku-4-5-20251001",
        "claude-haiku-4-5",
    ],
    ids=["bedrock-crx", "bedrock-concrete", "direct", "direct-dated", "direct-bare"],
)
def test_both_id_shapes_price(model):
    """Bedrock ids and dated direct ids are the same models under two naming schemes."""
    assert cost_estimator._rate_for(model) is not None


def test_merging_per_skill_costs_adds_up_to_the_run():
    """A sharded run bills per skill; the footer has to report the night, not the last shard."""
    a = cost_estimator.scorer_cost([scored(input_tokens=1000, output_tokens=100)])
    b = cost_estimator.scorer_cost([scored(input_tokens=3000, output_tokens=200)])
    merged = cost_estimator.merge_scorer_costs([a, b])
    assert merged["input_tokens"] == 4000
    assert merged["output_tokens"] == 300
    assert merged["cost_usd"] == pytest.approx(a["cost_usd"] + b["cost_usd"])
    assert merged["models"] == [RESOLVED]


def test_one_unpriced_shard_makes_the_whole_run_unpriced():
    """Half a bill is worse than no bill: it reads as the total."""
    priced = cost_estimator.scorer_cost([scored(input_tokens=1000, output_tokens=100)])
    unknown = cost_estimator.scorer_cost(
        [scored(model="something-new", input_tokens=1000, output_tokens=100)]
    )
    merged = cost_estimator.merge_scorer_costs([priced, unknown])
    assert merged["cost_usd"] is None
    assert merged["unpriced_models"] == ["something-new"]


def test_merging_nothing_costs_nothing():
    merged = cost_estimator.merge_scorer_costs([])
    assert merged["cost_usd"] == 0
    assert merged["models"] == []


def test_every_declared_rate_carries_a_citation():
    """A price written from memory is the thing this test exists to prevent."""
    source = __import__("pathlib").Path(cost_estimator.__file__).read_text()
    table_start = source.index("SCORER_RATES")
    preamble = source[max(0, table_start - 1200) : table_start]
    assert "https://" in preamble, "the rate table needs a source URL above it"
