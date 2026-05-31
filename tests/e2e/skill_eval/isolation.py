"""Domain skill isolation guard — T022."""
from typing import TYPE_CHECKING
from tests.e2e.skill_eval.fire_rate import _skill_tool_fired, _install_skill_local

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillEvalConfig


def check_isolation_from_events(event_lists: list[list[dict]]) -> float:
    """Compute fire rate from isolation runs (lower is better for domain skills)."""
    if not event_lists:
        return 0.0
    hits = sum(1 for events in event_lists if _skill_tool_fired(events))
    return hits / len(event_lists)


def check_isolation(
    config: "SkillEvalConfig",
    n_runs: int,
    openai_api_key: str,
    model: str,
) -> float:
    """Run the isolation_prompt N times with the domain skill installed. Returns fire rate (expected 0.0)."""
    if not config.isolation_prompt:
        return 0.0

    from tests.e2e.isolated_env import IsolatedEnv
    from tests.e2e.readme_validator import ReadmeValidator
    from tests.e2e.opencode_runner import collect_events

    event_lists = []
    for _ in range(n_runs):
        with IsolatedEnv(openai_api_key=openai_api_key) as env:
            try:
                ReadmeValidator(skills_dir=env.skills_dir).install_skill(config.skill)
            except (ValueError, Exception):
                _install_skill_local(config.skill, env.skills_dir)
            events = collect_events(config.isolation_prompt, env.env_vars(), model=model)
        event_lists.append(events)
    return check_isolation_from_events(event_lists)
