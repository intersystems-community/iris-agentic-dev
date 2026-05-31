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


@dataclasses.dataclass
class SkillResult:
    skill: str
    fire_rate: Optional[float]
    isolation_fire_rate: Optional[float]
    pass_rate_baseline: Optional[float]
    pass_rate_skill: Optional[float]
    lift: Optional[float]
    lift_delta: Optional[float]
    regression_flag: bool
    new_skill: bool
    no_task_coverage: bool
    task_ids_used: list[str]


def discover_skills(light_skills_dir: str) -> list[str]:
    """Scan light-skills/skills/ and return all subdirectory names that contain SKILL.md."""
    skills = []
    for entry in sorted(os.listdir(light_skills_dir)):
        skill_md = os.path.join(light_skills_dir, entry, "SKILL.md")
        if os.path.isdir(os.path.join(light_skills_dir, entry)) and os.path.exists(skill_md):
            skills.append(entry)
    return skills


def load_eval_config(skill_name: str, tasks_skills_dir: str) -> Optional[SkillEvalConfig]:
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
    )


def compare_to_baseline(
    result: SkillResult,
    baseline: dict,
    threshold: float = 0.05,
) -> SkillResult:
    """Apply regression detection by comparing result lift against stored baseline."""
    if result.lift is None:
        return result
    entry = baseline.get(result.skill)
    if entry is None:
        result.new_skill = True
        result.lift_delta = None
        result.regression_flag = False
        return result
    old_lift = entry.get("lift")
    if old_lift is None:
        result.new_skill = False
        result.lift_delta = None
        result.regression_flag = False
        return result
    delta = result.lift - old_lift
    result.lift_delta = round(delta, 4)
    result.new_skill = False
    result.regression_flag = delta < -threshold
    return result
