"""Unit tests for SkillEvalConfig loader and discovery — T003."""
import os
import tempfile
import pytest
import yaml
from tests.e2e.skill_eval.evaluator import discover_skills, load_eval_config, SkillEvalConfig

LIGHT_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "light-skills", "skills")
)
TASKS_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)


def test_discover_skills_finds_all():
    skills = discover_skills(LIGHT_SKILLS_DIR)
    assert len(skills) == 24
    assert "objectscript-review" in skills
    assert "iris-vector-ai" in skills


def test_discover_skills_requires_skill_md(tmp_path):
    (tmp_path / "has-skill-md").mkdir()
    (tmp_path / "has-skill-md" / "SKILL.md").write_text("---\nname: test\n---")
    (tmp_path / "no-skill-md").mkdir()
    skills = discover_skills(str(tmp_path))
    assert skills == ["has-skill-md"]


def test_load_eval_config_returns_config(tmp_path):
    skill_dir = tmp_path / "objectscript-review"
    skill_dir.mkdir()
    config = {
        "skill": "objectscript-review",
        "description": "Reviews ObjectScript code",
        "fire_rate_prompt": "Fix this method: ...",
        "benchmark_tasks": ["DBG-01", "DBG-02"],
        "domain_skill": False,
    }
    (skill_dir / "eval.yaml").write_text(yaml.dump(config))
    result = load_eval_config("objectscript-review", str(tmp_path))
    assert result is not None
    assert result.skill == "objectscript-review"
    assert result.benchmark_tasks == ["DBG-01", "DBG-02"]
    assert result.domain_skill is False
    assert result.isolation_prompt is None


def test_load_eval_config_returns_none_for_missing(tmp_path):
    result = load_eval_config("nonexistent-skill", str(tmp_path))
    assert result is None


def test_load_eval_config_domain_skill(tmp_path):
    skill_dir = tmp_path / "iris-vector-ai"
    skill_dir.mkdir()
    config = {
        "skill": "iris-vector-ai",
        "description": "Vector search",
        "fire_rate_prompt": "Write a VECTOR_COSINE query",
        "benchmark_tasks": [],
        "domain_skill": True,
        "isolation_prompt": "Fix this Return-in-loop bug: ...",
    }
    (skill_dir / "eval.yaml").write_text(yaml.dump(config))
    result = load_eval_config("iris-vector-ai", str(tmp_path))
    assert result is not None
    assert result.domain_skill is True
    assert result.isolation_prompt is not None
