"""Unit tests for fire_rate — T011."""
import pytest
from unittest.mock import patch, MagicMock
from tests.e2e.skill_eval.evaluator import SkillEvalConfig
from tests.e2e.skill_eval.fire_rate import measure_fire_rate_from_events, _skill_tool_fired


def make_config():
    return SkillEvalConfig(
        skill="objectscript-review",
        description="",
        fire_rate_prompt="Fix this method",
        benchmark_tasks=["DBG-01"],
        domain_skill=False,
    )


def make_skill_event():
    return {"type": "tool_use", "part": {"tool": "skill", "state": {"status": "completed"}}}


def make_text_event():
    return {"type": "text", "part": {"text": "Here is the fix", "time": {"end": 1}}}


def make_bash_event():
    return {"type": "tool_use", "part": {"tool": "bash", "state": {"status": "completed"}}}


def test_skill_fired_detects_skill_tool():
    events = [make_text_event(), make_skill_event()]
    assert _skill_tool_fired(events) is True


def test_skill_fired_false_without_skill_tool():
    events = [make_text_event(), make_bash_event()]
    assert _skill_tool_fired(events) is False


def test_skill_fired_exact_match_not_suffix():
    events = [{"type": "tool_use", "part": {"tool": "iris-agentic-dev_skill", "state": {"status": "completed"}}}]
    assert _skill_tool_fired(events) is False


def test_measure_fire_rate_all_hit():
    event_lists = [[make_skill_event()], [make_skill_event()], [make_skill_event()]]
    rate = measure_fire_rate_from_events(event_lists)
    assert rate == pytest.approx(1.0)


def test_measure_fire_rate_none_hit():
    event_lists = [[make_text_event()], [make_text_event()], [make_bash_event()]]
    rate = measure_fire_rate_from_events(event_lists)
    assert rate == pytest.approx(0.0)


def test_measure_fire_rate_partial():
    event_lists = [[make_skill_event()], [make_text_event()], [make_skill_event()]]
    rate = measure_fire_rate_from_events(event_lists)
    assert rate == pytest.approx(2 / 3)
