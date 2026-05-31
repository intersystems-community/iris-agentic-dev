"""Unit tests for isolation — T021."""
import pytest
from tests.e2e.skill_eval.isolation import check_isolation_from_events
from tests.e2e.skill_eval.evaluator import SkillEvalConfig


def make_domain_config(with_isolation_prompt=True):
    return SkillEvalConfig(
        skill="iris-vector-ai",
        description="Vector search",
        fire_rate_prompt="Write a VECTOR_COSINE query",
        benchmark_tasks=[],
        domain_skill=True,
        isolation_prompt="Fix this Return-in-loop bug: ..." if with_isolation_prompt else None,
    )


def make_skill_event():
    return {"type": "tool_use", "part": {"tool": "skill", "state": {"status": "completed"}}}


def make_text_event():
    return {"type": "text", "part": {"text": "Fixed", "time": {"end": 1}}}


def test_isolation_no_skill_calls():
    event_lists = [[make_text_event()], [make_text_event()], [make_text_event()]]
    rate = check_isolation_from_events(event_lists)
    assert rate == 0.0


def test_isolation_unexpected_skill_call():
    event_lists = [[make_skill_event()], [make_text_event()], [make_text_event()]]
    rate = check_isolation_from_events(event_lists)
    assert rate == pytest.approx(1 / 3)


def test_isolation_all_fire():
    event_lists = [[make_skill_event()], [make_skill_event()], [make_skill_event()]]
    rate = check_isolation_from_events(event_lists)
    assert rate == pytest.approx(1.0)
