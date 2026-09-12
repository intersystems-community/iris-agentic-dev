"""Dry-run cost estimator — T008."""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillEvalConfig

# Approximate costs per call (Bedrock Sonnet + Haiku)
_SONNET_COST = 0.020  # ~$0.020 per task run (500 input + 300 output tokens)
_HAIKU_COST = 0.001  # ~$0.001 per judge call

# Scorer rates in USD per million tokens, as (input, output).
#
# Confirmed 2026-09-12 against https://platform.claude.com/docs/en/about-claude/pricing
# § Model pricing: Claude Sonnet 4.6 and 4.5 are $3 / MTok input and $15 / MTok output;
# Claude Haiku 4.5 is $1 and $5. Bedrock's own price list
# (https://aws.amazon.com/bedrock/pricing/) renders its 4.x table client-side and does not
# come back in a plain fetch, so these are the first-party rates the Bedrock list matches;
# the same page notes that Bedrock *regional* endpoints carry a 10% premium over global, so
# a run pinned to a region costs about 1.1× what this table says.
#
# Keyed by the resolved model, matched by prefix, because Bedrock reports concrete ids like
# `us.anthropic.claude-sonnet-4-6-20260514-v1:0`. Nothing here is written from memory: a
# price nobody checked is how the old `_HAIKU_COST` came to bill a Sonnet-class scorer at a
# Haiku rate.
SCORER_RATES = {
    "claude-sonnet-4-6": (3.00, 15.00),
    "claude-sonnet-4-5": (3.00, 15.00),
    "claude-haiku-4-5": (1.00, 5.00),
    "us.anthropic.claude-sonnet-4-6": (3.00, 15.00),
    "us.anthropic.claude-sonnet-4-5": (3.00, 15.00),
    "us.anthropic.claude-haiku-4-5": (1.00, 5.00),
}

# Measured, not guessed: GHA run 34093536537 (2026-09-07) completed 185 opencode sessions in
# 4,122 s of wall clock — 22.3 s each. The old value of 15 s was set before the harness had
# ever run on a GHA runner, and it is what put the nightly estimate (67 min) under the job
# timeout (75 min) for a suite that needs about 100.
_SECONDS_PER_TASK = 22
_SECONDS_PER_JUDGE = 3


def estimate_skill(cfg: "SkillEvalConfig", runs: int = 3) -> dict:
    """Estimate one skill's cost. This is the per-shard budget in the nightly workflow."""
    # Fire-rate: N runs.
    task_runs = runs
    # Implicit fire-rate: `_run_skill` calls measure_fire_rate a second time with the prompt
    # that does not name the skill. Same cost as the explicit pass.
    if cfg.implicit_fire_rate_prompt:
        task_runs += runs
    if cfg.domain_skill and cfg.isolation_prompt:
        task_runs += runs
    # Lift: benchmark_tasks × 2 conditions × runs, each scored by the judge.
    judge_calls = len(cfg.benchmark_tasks) * 2 * runs
    task_runs += judge_calls
    total_seconds = task_runs * _SECONDS_PER_TASK + judge_calls * _SECONDS_PER_JUDGE
    return {
        "task_runs": task_runs,
        "judge_calls": judge_calls,
        "cost_usd": round(task_runs * _SONNET_COST + judge_calls * _HAIKU_COST, 2),
        "time_minutes": round(total_seconds / 60, 1),
    }


def estimate(configs: "list[SkillEvalConfig]", runs: int = 3) -> dict:
    """Estimate LLM call count and cost for the given skill configs."""
    task_runs = 0
    judge_calls = 0
    for cfg in configs:
        per_skill = estimate_skill(cfg, runs=runs)
        task_runs += per_skill["task_runs"]
        judge_calls += per_skill["judge_calls"]
    total_seconds = task_runs * _SECONDS_PER_TASK + judge_calls * _SECONDS_PER_JUDGE
    cost = task_runs * _SONNET_COST + judge_calls * _HAIKU_COST
    return {
        "task_runs": task_runs,
        "judge_calls": judge_calls,
        "cost_usd": round(cost, 2),
        "time_minutes": round(total_seconds / 60, 1),
    }


def _rate_for(model: str):
    """The (input, output) rate for a resolved model id, or `None` if the table has none.

    Longest prefix first, so `us.anthropic.claude-sonnet-4-6-20260514-v1:0` matches the
    cross-region key rather than the bare one.
    """
    if not model:
        return None
    for key in sorted(SCORER_RATES, key=len, reverse=True):
        if model.startswith(key) or key in model:
            return SCORER_RATES[key]
    return None


def scorer_cost(items) -> dict:
    """What the scoring calls in a run actually cost, from measured usage.

    Not a call count times a guessed per-call rate — that is what printed $0.001 per call for
    a scorer that was serving Sonnet. Items scored by assertion or pattern carry no tokens and
    cost nothing. A model with no declared rate leaves `cost_usd` as `None` rather than a
    number derived from a price nobody checked.
    """
    input_tokens = 0
    output_tokens = 0
    cost = 0.0
    models: set = set()
    unpriced: set = set()
    for item in items:
        model = item.get("scorer_model")
        if not model:
            continue
        models.add(model)
        item_in = item.get("input_tokens") or 0
        item_out = item.get("output_tokens") or 0
        input_tokens += item_in
        output_tokens += item_out
        rate = _rate_for(model)
        if rate is None:
            unpriced.add(model)
            continue
        cost += item_in * rate[0] / 1e6 + item_out * rate[1] / 1e6
    return {
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cost_usd": None if unpriced else round(cost, 6),
        "models": sorted(models),
        "unpriced_models": sorted(unpriced),
    }


def merge_scorer_costs(records) -> dict:
    """One bill from several per-skill ones. Same shape, so `format_scorer_cost` reads both.

    A single unpriced model unprices the whole run: a partial total printed as *the* total is
    the more expensive mistake.
    """
    input_tokens = 0
    output_tokens = 0
    cost = 0.0
    models: set = set()
    unpriced: set = set()
    for record in records:
        input_tokens += record.get("input_tokens") or 0
        output_tokens += record.get("output_tokens") or 0
        models.update(record.get("models") or [])
        unpriced.update(record.get("unpriced_models") or [])
        cost += record.get("cost_usd") or 0.0
    return {
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cost_usd": None if unpriced else round(cost, 6),
        "models": sorted(models),
        "unpriced_models": sorted(unpriced),
    }


def format_scorer_cost(record: dict) -> str:
    """One line naming the model that scored the run and what it charged."""
    models = ", ".join(record["models"]) or "no scorer called"
    tokens = f"{record['input_tokens']:,} in / {record['output_tokens']:,} out"
    if record["cost_usd"] is None:
        unpriced = ", ".join(record["unpriced_models"])
        return (
            f"  Scoring ({models}): {tokens} — no rate declared for {unpriced}, so this run "
            "cannot be priced"
        )
    return f"  Scoring ({models}): {tokens} = ${record['cost_usd']:.4f} USD"


def format_dry_run(est: dict, n_covered: int, n_uncovered: int) -> str:
    # Names the model the scorer will ask for. "Judge calls (Haiku)" was a label, not a fact:
    # the Bedrock path resolves that constant to a Sonnet-class model.
    from runner._client import haiku_model

    lines = [
        "Skill eval dry run:",
        f"  Skills with eval.yaml: {n_covered}",
        f"  Skills without coverage: {n_uncovered}",
        f"  Task runs (Sonnet): {est['task_runs']}",
        f"  Judge calls ({haiku_model()}): {est['judge_calls']}",
        f"  Estimated cost: ~${est['cost_usd']:.2f} USD",
        f"  Estimated time: ~{est['time_minutes']} min",
        "",
        "Run with --yes to proceed, or --skill <name> for a single skill.",
    ]
    return "\n".join(lines)
