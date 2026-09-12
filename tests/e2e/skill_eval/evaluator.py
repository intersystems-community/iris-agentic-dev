"""SkillEvalConfig loader and skill discovery — T004."""

import os
import dataclasses
from typing import Optional
import yaml


@dataclasses.dataclass
class SkillEvalConfig:
    skill: str
    description: str
    fire_rate_prompt: str
    benchmark_tasks: list[str]
    domain_skill: bool
    isolation_prompt: Optional[str] = None
    targeted_tasks_dir: Optional[str] = None  # if set, look for tasks here first
    implicit_fire_rate_prompt: Optional[str] = None  # unprompted trigger test
    no_mcp_for_benchmark: bool = (
        False  # run benchmark tasks without MCP tools (light-skills scenario)
    )


@dataclasses.dataclass
class SkillResult:
    skill: str
    fire_rate: Optional[float]
    implicit_fire_rate: Optional[float]  # unprompted trigger rate
    isolation_fire_rate: Optional[float]
    pass_rate_baseline: Optional[float]
    pass_rate_skill: Optional[float]
    lift: Optional[float]
    lift_delta: Optional[float]
    regression_flag: bool
    new_skill: bool
    no_task_coverage: bool
    task_ids_used: list[str]
    # 118: the outcome enum is what decides; `regression_flag` above is derived from it and
    # keeps its name only because `shard.py` merges shard files by key, and a rename would drop
    # the field silently in a mixed-version merge (the #110 pattern).
    outcome: str = "not_comparable"
    outcome_reason: Optional[str] = None
    arms: Optional[dict] = None  # {"baseline": ArmResult.to_dict(), "skill": ...}
    provenance: Optional[dict] = None
    threshold_applied: Optional[float] = None
    # Printed beside the Δ when the binary changed, never suppressing it.
    surface_note: Optional[str] = None


def discover_skills(skills_pack_dir: str) -> list[str]:
    """Scan skills/skills/ and return all subdirectory names that contain SKILL.md."""
    skills = []
    for entry in sorted(os.listdir(skills_pack_dir)):
        skill_md = os.path.join(skills_pack_dir, entry, "SKILL.md")
        if os.path.isdir(os.path.join(skills_pack_dir, entry)) and os.path.exists(
            skill_md
        ):
            skills.append(entry)
    return skills


def load_eval_config(
    skill_name: str, tasks_skills_dir: str
) -> Optional[SkillEvalConfig]:
    """Load eval.yaml for a skill. Returns None if no eval.yaml exists."""
    path = os.path.join(tasks_skills_dir, skill_name, "eval.yaml")
    if not os.path.exists(path):
        return None
    with open(path) as f:
        data = yaml.safe_load(f)
    return SkillEvalConfig(
        skill=data["skill"],
        description=data.get("description", ""),
        fire_rate_prompt=data["fire_rate_prompt"],
        benchmark_tasks=data.get("benchmark_tasks", []),
        domain_skill=data.get("domain_skill", False),
        isolation_prompt=data.get("isolation_prompt"),
        targeted_tasks_dir=data.get("targeted_tasks_dir"),
        implicit_fire_rate_prompt=data.get("implicit_fire_rate_prompt"),
        no_mcp_for_benchmark=data.get("no_mcp_for_benchmark", False),
    )


def _an_arm_scored_nothing(result: SkillResult) -> bool:
    """True when either arm has no rate. `None` and `0.0` are different facts."""
    arms = result.arms or {}
    if arms:
        return any(
            (arms.get(arm) or {}).get("pass_rate") is None
            for arm in ("baseline", "skill")
        )
    return result.pass_rate_baseline is None or result.pass_rate_skill is None


def compare_to_baseline(
    result: SkillResult,
    baseline: dict,
    threshold: float = 0.05,
) -> SkillResult:
    """Derive the outcome for one skill against its stored entry.

    The order is data-model.md § 5 and the first match wins: no entry → `new_skill`;
    incomparable provenance → `not_comparable` naming the field; an arm with no scored items →
    `not_comparable`; a drop past the threshold → `regressed`; otherwise `held`.

    `regression_flag` is set from `outcome` at the bottom and computed nowhere else. The rule it
    replaces was a 5-point floor with an escape hatch at 20 points, and nobody could derive
    either constant — a skill going from +0.40 to +0.10 was reported as holding.
    """
    from tests.e2e.skill_eval.baseline import comparability, surface_change

    result.threshold_applied = threshold
    entry = baseline.get(result.skill)

    if entry is None:
        result.outcome = "new_skill"
        result.outcome_reason = None
        result.new_skill = True
        result.lift_delta = None
        result.regression_flag = False
        return result

    result.new_skill = False
    result.surface_note = surface_change(entry, result.provenance)

    reason = comparability(entry, result.provenance)
    if reason is None and _an_arm_scored_nothing(result):
        reason = "no scored items"
    if reason is None and (entry.get("lift") is None or result.lift is None):
        reason = "no lift recorded"

    if reason is not None:
        result.outcome = "not_comparable"
        result.outcome_reason = reason
        result.lift_delta = None
        result.regression_flag = False
        return result

    delta = result.lift - entry["lift"]
    result.lift_delta = round(delta, 4)
    result.outcome = "regressed" if delta < -threshold else "held"
    result.outcome_reason = None
    result.regression_flag = result.outcome == "regressed"
    return result
