"""US1 E2E tests — skills quality gate. Requires OPENAI_API_KEY."""
import os
import pytest
from tests.e2e.harness import run_task
from tests.e2e.task_loader import load_task, TASKS_DIR
from tests.e2e.readme_validator import ReadmeValidator


@pytest.mark.us1
@pytest.mark.network_curl
def test_us1_readme_urls_valid(tmp_path):
    """All curl URLs in skills/README.md must return HTTP 200."""
    v = ReadmeValidator(skills_dir=str(tmp_path))
    urls = v.validate_urls()
    assert len(urls) > 0, "README must contain at least one curl URL"


@pytest.mark.us1
def test_us1_skill_quality(openai_api_key, tmp_path):
    """objectscript-review skill must suppress Return-in-For bug."""
    task = load_task(os.path.join(TASKS_DIR, "SKILL-01.yaml"))
    result = run_task(
        task=task,
        openai_api_key=openai_api_key,
        keep_on_failure=True,
    )
    assert result.passed, (
        f"Skill quality gate failed.\n"
        f"LLM output excerpt: {result.llm_output_excerpt[:300]}\n"
        f"Assertions: {[(a.description, a.passed) for a in result.assertion_results]}"
    )


@pytest.mark.us1
def test_us1_baseline(openai_api_key, tmp_path):
    """Baseline (no skill) run — result recorded; not required to pass."""
    task = load_task(os.path.join(TASKS_DIR, "SKILL-01-baseline.yaml"))
    result = run_task(
        task=task,
        openai_api_key=openai_api_key,
    )
    # Baseline just needs to complete and produce a result — not pass
    assert result.task_id == "SKILL-01-baseline"
    assert result.llm_output_excerpt is not None
