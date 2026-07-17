"""Unit tests for ReadmeValidator — T011."""
import os
import textwrap
import pytest
from unittest.mock import patch, MagicMock
from tests.e2e.readme_validator import ReadmeValidator, ReadmeValidationError


SAMPLE_README = textwrap.dedent("""\
    # Skills

    ## 60-second setup

    ### Step 1: Copy AGENTS.md to your repo

    ```bash
    curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/AGENTS.md \\
      > AGENTS.md
    ```

    ### Step 2: Add the #1 skill

    ```bash
    mkdir -p ~/.claude/skills/objectscript-review
    curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/objectscript-review/SKILL.md \\
      > ~/.claude/skills/objectscript-review/SKILL.md
    ```

    Some other content here.
""")


def test_extract_skill_urls():
    v = ReadmeValidator.__new__(ReadmeValidator)
    v._readme_text = SAMPLE_README
    urls = v._extract_skill_urls()
    assert any("AGENTS.md" in u for u in urls)
    assert any("objectscript-review/SKILL.md" in u for u in urls)


def test_validate_urls_all_ok(tmp_path):
    v = ReadmeValidator.__new__(ReadmeValidator)
    v._readme_text = SAMPLE_README
    v._skills_dir = str(tmp_path)
    with patch("tests.e2e.readme_validator.requests.head") as mock_head:
        mock_head.return_value = MagicMock(status_code=200)
        v.validate_urls()  # should not raise


def test_validate_urls_404_raises(tmp_path):
    v = ReadmeValidator.__new__(ReadmeValidator)
    v._readme_text = SAMPLE_README
    v._skills_dir = str(tmp_path)
    with patch("tests.e2e.readme_validator.requests.head") as mock_head:
        mock_head.return_value = MagicMock(status_code=404)
        with pytest.raises(ReadmeValidationError) as exc_info:
            v.validate_urls()
        assert "404" in str(exc_info.value) or "raw.githubusercontent" in str(exc_info.value)


def test_readme_validation_error_contains_url():
    err = ReadmeValidationError("https://example.com/SKILL.md", 42, 404)
    assert "https://example.com/SKILL.md" in str(err)
    assert "404" in str(err)
