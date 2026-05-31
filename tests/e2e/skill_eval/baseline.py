"""Baseline read/write/diff for skill regression detection — T006."""
import json
import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillResult


def load_baseline(path: str) -> dict:
    """Load baseline JSON. Returns empty dict if file absent."""
    if not os.path.exists(path):
        return {}
    with open(path) as f:
        return json.load(f)


def save_baseline(results: "list[SkillResult]", path: str) -> None:
    """Save current run results as new baseline. Only includes skills with lift data."""
    data = {}
    for r in results:
        if r.lift is not None:
            data[r.skill] = {
                "fire_rate": r.fire_rate,
                "lift": r.lift,
                "pass_rate_baseline": r.pass_rate_baseline,
                "pass_rate_skill": r.pass_rate_skill,
            }
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w") as f:
        json.dump(data, f, indent=2)


def compute_diff(old: dict, new: "list[SkillResult]") -> list[dict]:
    """Compare new results against old baseline. Returns list of changed skills sorted by abs(delta) desc."""
    diffs = []
    for result in new:
        if result.lift is None:
            continue
        old_entry = old.get(result.skill)
        if old_entry is None:
            diffs.append({
                "skill": result.skill,
                "old_lift": None,
                "new_lift": result.lift,
                "delta": None,
                "new_skill": True,
            })
            continue
        old_lift = old_entry.get("lift")
        if old_lift is None:
            continue
        delta = result.lift - old_lift
        if abs(delta) < 0.001:
            continue  # no meaningful change
        diffs.append({
            "skill": result.skill,
            "old_lift": old_lift,
            "new_lift": result.lift,
            "delta": round(delta, 4),
            "new_skill": False,
        })
    diffs.sort(key=lambda d: abs(d["delta"] or 0), reverse=True)
    return diffs
