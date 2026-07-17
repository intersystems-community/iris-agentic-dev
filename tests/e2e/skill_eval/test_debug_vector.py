"""Debug iris-vector-ai cold-start scoring."""
import os, sys, shutil, tempfile
import pytest

@pytest.mark.skipif(not os.environ.get("OPENAI_API_KEY"), reason="needs key")
def test_debug_vector_cold():
    sys.path.insert(0, 'benchmark/021')
    from tests.e2e.isolated_env import IsolatedEnv
    from tests.e2e.opencode_runner import collect_events
    from tests.e2e.skill_eval.lift import _read_cls_files_from_workdir, _extract_written_content, _check_expected_patterns
    import yaml

    key = os.environ["OPENAI_API_KEY"]
    model = "amazon-bedrock/us.anthropic.claude-sonnet-4-5-20250929-v1:0"
    task = yaml.safe_load(open("tests/e2e/tasks/skills/targeted/VECTOR-SYNTAX-COLD.yaml"))
    expected = task["expected_behavior"]

    for label, install_skill in [("BASELINE", False), ("WITH_SKILL", True)]:
        workdir = tempfile.mkdtemp(prefix=f"vec-debug-{label}-")
        try:
            with IsolatedEnv(openai_api_key=key) as env:
                if install_skill:
                    dest = os.path.join(env.skills_dir, "iris-vector-ai")
                    os.makedirs(dest, exist_ok=True)
                    shutil.copy2("skills/skills/iris-vector-ai/SKILL.md", os.path.join(dest, "SKILL.md"))
                events = collect_events(task["description"], env.env_vars(), model=model, working_dir=workdir)

            content = _read_cls_files_from_workdir(workdir) or _extract_written_content(events)
            patterns_met = _check_expected_patterns(content, expected)
            print(f"\n=== {label} ===")
            print(f"Content length: {len(content)}")
            print(f"Pattern check: {patterns_met}")
            # Show key snippets
            for kw in ["VECTOR_COSINE", "TO_VECTOR", "<=>", "::vector", "LIMIT", "TOP"]:
                if kw.lower() in content.lower():
                    print(f"  FOUND: {kw}")
            print(f"First 600:\n{content[:600]}")
        finally:
            shutil.rmtree(workdir, ignore_errors=True)
