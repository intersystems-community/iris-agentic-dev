"""Debug: run run_task_and_score on VECTOR-PYTHON-SEARCH and show what it sees."""
import os
import pytest

@pytest.mark.skipif(not os.environ.get("OPENAI_API_KEY"), reason="needs key")
def test_score_vector_python():
    from tests.e2e.skill_eval.lift import run_task_and_score, _check_expected_patterns
    import yaml

    key = os.environ["OPENAI_API_KEY"]
    model = "amazon-bedrock/us.anthropic.claude-sonnet-4-5-20250929-v1:0"
    task = yaml.safe_load(open("tests/e2e/tasks/skills/targeted/VECTOR-PYTHON-SEARCH.yaml"))

    from tests.e2e.skill_eval.lift import _read_cls_files_from_workdir, _extract_written_content, _check_expected_patterns
    import tempfile, shutil
    from tests.e2e.isolated_env import IsolatedEnv
    from tests.e2e.opencode_runner import collect_events
    import yaml

    task = yaml.safe_load(open("tests/e2e/tasks/skills/targeted/VECTOR-PYTHON-SEARCH.yaml"))

    for label, skill in [("BASELINE", None), ("WITH_SKILL", "iris-vector-ai")]:
        workdir_obj = tempfile.TemporaryDirectory(prefix=f"probe-{label}-")
        workdir = workdir_obj.name
        with IsolatedEnv(openai_api_key=key) as env:
            if skill:
                dest = os.path.join(env.skills_dir, skill)
                os.makedirs(dest, exist_ok=True)
                shutil.copy2(f"skills/skills/{skill}/SKILL.md", os.path.join(dest, "SKILL.md"))
            events = collect_events(task["description"], env.env_vars(), model=model, working_dir=workdir)

        disk = _read_cls_files_from_workdir(workdir)
        stream = _extract_written_content(events)
        content = disk or stream
        patterns = _check_expected_patterns(content, task["expected_behavior"])
        print(f"\n=== {label} ===")
        print(f"disk files: {len(disk)} chars | stream: {len(stream)} chars")
        for kw in ["TO_VECTOR(?, DOUBLE", "TO_VECTOR(?)", "<=>", "LIMIT", "VECTOR_COSINE", "TOP "]:
            if kw.lower() in content.lower():
                print(f"  FOUND: {repr(kw)}")
        print(f"patterns_met: {patterns}")
        print(f"Content preview:\n{content[:600]}")
        workdir_obj.cleanup()

    for label, skill in [("BASELINE", None), ("WITH_SKILL", "iris-vector-ai")]:
        r = run_task_and_score("VECTOR-PYTHON-SEARCH", skill, key, model,
                               iris_host="localhost", iris_web_port="52780",
                               iris_container="iris-dev-iris", no_mcp=True)
        print(f"\n{label}: score={r['score']} — {r['reasoning']}")
