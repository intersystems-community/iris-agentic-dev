"""Integration tests for skill eval suite — T015-T019, T033-T034."""
import json
import os
import subprocess
import sys
import tempfile
import pytest
from tests.e2e.skill_eval.evaluator import SkillEvalConfig, SkillResult, compare_to_baseline
from tests.e2e.skill_eval.baseline import save_baseline, load_baseline


MODEL = "amazon-bedrock/us.anthropic.claude-sonnet-4-5-20250929-v1:0"
TASKS_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)


@pytest.fixture
def openai_key():
    key = os.environ.get("OPENAI_API_KEY", "")
    if not key:
        pytest.skip("OPENAI_API_KEY not set")
    return key


@pytest.fixture
def iris_available():
    container = os.environ.get("IRIS_CONTAINER", "iris-dev-iris")
    port = os.environ.get("IRIS_WEB_PORT", "52780")
    return {"container": container, "web_port": port}


# ── US1: Fire-rate ─────────────────────────────────────────────────────────────

@pytest.mark.us1
def test_fire_rate_objectscript_review(openai_key):
    """objectscript-review fire-rate task must trigger the skill tool >= 50% of 3 runs."""
    from tests.e2e.skill_eval.evaluator import load_eval_config
    from tests.e2e.skill_eval.fire_rate import measure_fire_rate

    config = load_eval_config("objectscript-review", TASKS_SKILLS_DIR)
    if config is None:
        pytest.skip("eval.yaml for objectscript-review not yet written")

    rate = measure_fire_rate(config, n_runs=3, openai_api_key=openai_key, model=MODEL)
    assert rate >= 0.5, f"objectscript-review fire rate too low: {rate}"


@pytest.mark.us1
def test_lift_objectscript_review(openai_key, iris_available):
    """objectscript-review must show positive lift on DBG-01."""
    from tests.e2e.skill_eval.evaluator import load_eval_config
    from tests.e2e.skill_eval.lift import run_task_and_score, compute_lift_from_scores

    config = load_eval_config("objectscript-review", TASKS_SKILLS_DIR)
    if config is None:
        pytest.skip("eval.yaml for objectscript-review not yet written")
    if "DBG-01" not in (config.benchmark_tasks or []):
        pytest.skip("DBG-01 not in objectscript-review benchmark_tasks")

    port = iris_available["web_port"]
    container = iris_available["container"]
    baseline = run_task_and_score("DBG-01", None, openai_key, MODEL, iris_web_port=port, iris_container=container)
    skill = run_task_and_score("DBG-01", "objectscript-review", openai_key, MODEL, iris_web_port=port, iris_container=container)
    result = compute_lift_from_scores([baseline], [skill])
    assert result["lift"] >= 0, f"Lift should be non-negative on single run; got {result['lift']}"


# ── US2: Regression detection ─────────────────────────────────────────────────

@pytest.mark.us2
def test_regression_detection():
    """Injecting a fake baseline with higher lift triggers regression_flag."""
    result = SkillResult(
        skill="objectscript-review",
        fire_rate=1.0,
        isolation_fire_rate=None,
        pass_rate_baseline=0.71,
        pass_rate_skill=0.81,
        lift=0.10,
        lift_delta=None,
        regression_flag=False,
        new_skill=False,
        no_task_coverage=False,
        task_ids_used=["DBG-01"],
    )
    baseline = {"objectscript-review": {"lift": 0.99}}
    updated = compare_to_baseline(result, baseline, threshold=0.05)
    assert updated.regression_flag is True
    assert updated.lift_delta == pytest.approx(0.10 - 0.99, abs=0.01)


@pytest.mark.us2
def test_no_regression_on_improvement():
    """Higher lift than baseline does not trigger regression_flag."""
    result = SkillResult(
        skill="objectscript-review",
        fire_rate=1.0, isolation_fire_rate=None,
        pass_rate_baseline=0.71, pass_rate_skill=1.0,
        lift=0.29, lift_delta=None, regression_flag=False,
        new_skill=False, no_task_coverage=False, task_ids_used=["DBG-01"],
    )
    baseline = {"objectscript-review": {"lift": 0.10}}
    updated = compare_to_baseline(result, baseline, threshold=0.05)
    assert updated.regression_flag is False
    assert updated.lift_delta > 0


@pytest.mark.us2
def test_update_baseline_writes_diff(tmp_path):
    """save_baseline writes file; compute_diff produces diff for changed skill."""
    from tests.e2e.skill_eval.baseline import compute_diff
    baseline_path = str(tmp_path / "baseline.json")
    old = {"objectscript-review": {"lift": 0.29}}

    result = SkillResult(
        skill="objectscript-review",
        fire_rate=1.0, isolation_fire_rate=None,
        pass_rate_baseline=0.71, pass_rate_skill=0.81,
        lift=0.10, lift_delta=None, regression_flag=True,
        new_skill=False, no_task_coverage=False, task_ids_used=["DBG-01"],
    )
    save_baseline([result], baseline_path)
    assert os.path.exists(baseline_path)

    diff = compute_diff(old, [result])
    assert len(diff) == 1
    assert diff[0]["old_lift"] == pytest.approx(0.29)
    assert diff[0]["new_lift"] == pytest.approx(0.10)


# ── US3: Domain isolation ─────────────────────────────────────────────────────

@pytest.mark.us3
def test_isolation_iris_vector_ai(openai_key):
    """iris-vector-ai must NOT fire on a general ObjectScript repair task."""
    from tests.e2e.skill_eval.evaluator import load_eval_config
    from tests.e2e.skill_eval.isolation import check_isolation

    config = load_eval_config("iris-vector-ai", TASKS_SKILLS_DIR)
    if config is None:
        pytest.skip("eval.yaml for iris-vector-ai not yet written")

    rate = check_isolation(config, n_runs=3, openai_api_key=openai_key, model=MODEL)
    assert rate == 0.0, f"iris-vector-ai fired on isolation task! rate={rate}"


# ── CLI tests ─────────────────────────────────────────────────────────────────

@pytest.mark.cli
def test_dry_run_no_llm_calls():
    """--dry-run exits 0 with cost estimate and no LLM calls."""
    result = subprocess.run(
        [sys.executable, "-m", "tests.e2e.skill_eval", "--dry-run"],
        cwd=os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..")),
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, f"stderr: {result.stderr}"
    assert "Estimated cost" in result.stdout
    assert "Run with --yes" in result.stdout


@pytest.mark.cli
def test_single_skill_full_run(openai_key, iris_available, tmp_path):
    """--skill objectscript-review --yes produces a result JSON with fire_rate populated."""
    result_dir = str(tmp_path / "results")
    proc = subprocess.run(
        [
            sys.executable, "-m", "tests.e2e.skill_eval",
            "--skill", "objectscript-review",
            "--yes",
            "--output", result_dir,
        ],
        cwd=os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..")),
        capture_output=True,
        text=True,
        timeout=300,
        env={**os.environ,
             "OPENAI_API_KEY": openai_key,
             "IRIS_CONTAINER": iris_available["container"],
             "IRIS_WEB_PORT": iris_available["web_port"]},
    )
    assert proc.returncode in (0, 1), f"stderr: {proc.stderr[:500]}"
    result_files = [f for f in os.listdir(result_dir) if f.startswith("skill-eval-")]
    assert len(result_files) == 1
    with open(os.path.join(result_dir, result_files[0])) as f:
        data = json.load(f)
    skills = {s["skill"]: s for s in data["skills"]}
    assert "objectscript-review" in skills
    assert skills["objectscript-review"]["fire_rate"] is not None
