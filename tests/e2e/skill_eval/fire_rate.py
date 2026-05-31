"""Fire-rate measurement via OpenCode harness — T012."""
import os
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillEvalConfig

_LIGHT_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "light-skills", "skills")
)
_TASKS_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)


def _skill_tool_fired(events: list[dict]) -> bool:
    """Return True if the OpenCode 'skill' built-in tool was called (exact match)."""
    for event in events:
        if event.get("type") != "tool_use":
            continue
        part = event.get("part", {})
        state = part.get("state", {})
        if state.get("status") != "completed":
            continue
        if part.get("tool") == "skill":
            return True
    return False


def measure_fire_rate_from_events(event_lists: list[list[dict]]) -> float:
    """Compute fire rate from a list of event lists (one per run)."""
    if not event_lists:
        return 0.0
    hits = sum(1 for events in event_lists if _skill_tool_fired(events))
    return hits / len(event_lists)


def measure_fire_rate(
    config: "SkillEvalConfig",
    n_runs: int,
    openai_api_key: str,
    model: str,
    prompt: str | None = None,
) -> float:
    """Run the fire-rate task N times and return hit rate."""
    from tests.e2e.isolated_env import IsolatedEnv
    from tests.e2e.readme_validator import ReadmeValidator
    from tests.e2e.opencode_runner import collect_events

    task_prompt = prompt or config.fire_rate_prompt
    event_lists = []
    for _ in range(n_runs):
        with IsolatedEnv(openai_api_key=openai_api_key) as env:
            # Install skill from README curl or local fallback
            try:
                validator = ReadmeValidator(skills_dir=env.skills_dir)
                validator.install_skill(config.skill)
            except ValueError:
                _install_skill_local(config.skill, env.skills_dir)
            events = collect_events(task_prompt, env.env_vars(), model=model)
        event_lists.append(events)
    return measure_fire_rate_from_events(event_lists)


def _install_skill_local(skill_name: str, skills_dir: str) -> None:
    """Copy skill from local light-skills/ directory."""
    import shutil
    src = os.path.join(_LIGHT_SKILLS_DIR, skill_name, "SKILL.md")
    if not os.path.exists(src):
        raise FileNotFoundError(f"Skill '{skill_name}' not found in README or light-skills/")
    dest_dir = os.path.join(skills_dir, skill_name)
    os.makedirs(dest_dir, exist_ok=True)
    shutil.copy2(src, os.path.join(dest_dir, "SKILL.md"))
