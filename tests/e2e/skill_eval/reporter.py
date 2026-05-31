"""Stdout summary table and JSON result writer — T010."""
import dataclasses
import json
import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillResult


@dataclasses.dataclass
class EvalRun:
    run_id: str
    model: str
    judge_model: str
    timestamp: str
    regression_threshold: float
    skills: "list[SkillResult]"
    summary: dict


def print_summary(run: EvalRun) -> None:
    """Print human-readable summary table to stdout."""
    header = f"\nSkill Evaluation Results — {run.timestamp}"
    print(header)
    print("=" * len(header.strip()))
    print(f"{'Skill':<35} {'Fire%':>6} {'Base':>6} {'Skill':>6} {'Lift':>7} {'Δ':>7}  {'Status'}")
    print("-" * 80)

    def fmt_pct(v):
        return f"{v*100:.0f}%" if v is not None else "  n/a"

    def fmt_lift(v):
        if v is None:
            return "   n/a"
        sign = "+" if v >= 0 else ""
        return f"{sign}{v*100:.0f}%"

    for r in sorted(run.skills, key=lambda x: (x.lift or 0), reverse=True):
        status = "✓"
        if r.no_task_coverage:
            status = "(no coverage)"
        elif r.regression_flag:
            status = "✗ REGRESSION"
        delta = fmt_lift(r.lift_delta) if r.lift_delta is not None else "   n/a"
        print(
            f"{r.skill:<35} {fmt_pct(r.fire_rate):>6} {fmt_pct(r.pass_rate_baseline):>6} "
            f"{fmt_pct(r.pass_rate_skill):>6} {fmt_lift(r.lift):>7} {delta:>7}  {status}"
        )

    print()
    regressions = run.summary.get("regressions", [])
    improvements = run.summary.get("improvements", [])
    uncovered = run.summary.get("uncovered", [])
    print(f"Regressions: {len(regressions)}", end="")
    if regressions:
        print(f"  [{', '.join(regressions)}]")
    else:
        print()
    print(f"Improvements: {len(improvements)}")
    if uncovered:
        print(f"No task coverage: {len(uncovered)} skills (add eval.yaml to cover them)")
    cost = run.summary.get("estimated_cost_usd")
    if cost:
        print(f"Estimated cost: ~${cost:.2f} USD")


def write_result(run: EvalRun, output_dir: str) -> str:
    """Write EvalRun to JSON. Returns path."""
    os.makedirs(output_dir, exist_ok=True)
    path = os.path.join(output_dir, f"skill-eval-{run.run_id}.json")
    data = dataclasses.asdict(run)
    with open(path, "w") as f:
        json.dump(data, f, indent=2)
    return path
