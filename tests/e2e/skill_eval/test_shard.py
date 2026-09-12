"""Sharding support for the nightly Skill Regression workflow.

The nightly job ran all nine covered skills in one sequential process and was cancelled at
its 75-minute timeout every night from 2026-08-29 onward. Run 34093536537 got through five
skills and part of the sixth in 68 minutes; the suite needs about 100. Nothing was hung —
`opencode_runner.collect_events` already kills a session at 300 s — the job was simply
oversubscribed.

The fix is one GitHub job per skill. That needs two things from the harness: a way to ask it
which skills are covered (so the matrix is derived, not hand-maintained and left to rot), and
a way to merge the per-shard result files back into one summary and one baseline write.
"""

import json
import os
import subprocess
import sys

import pytest


from tests.e2e.skill_eval.evaluator import SkillResult
from tests.e2e.skill_eval.shard import covered_skills, merge_results

_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
)
_SKILLS_PACK_DIR = os.path.join(_REPO_ROOT, "skills", "skills")
_TASKS_SKILLS_DIR = os.path.join(_REPO_ROOT, "tests", "e2e", "tasks", "skills")


def _result(skill, lift=0.1, regression=False):
    return SkillResult(
        skill=skill,
        fire_rate=1.0,
        implicit_fire_rate=None,
        isolation_fire_rate=None,
        pass_rate_baseline=0.5,
        pass_rate_skill=0.6,
        lift=lift,
        lift_delta=None,
        regression_flag=regression,
        new_skill=False,
        no_task_coverage=False,
        task_ids_used=["T-1"],
    )


# ── covered_skills ───────────────────────────────────────────────────────────


def test_covered_skills_is_the_intersection_not_the_skill_pack():
    """34 skills ship in the pack; 9 have an eval.yaml. Sharding the other 25 wastes a job.

    `discover_skills` answers "what skills exist", which is the wrong question for a matrix:
    a shard for a skill with no eval config does nothing but start a container and exit.
    """
    covered = covered_skills(_SKILLS_PACK_DIR, _TASKS_SKILLS_DIR)
    assert covered, "no covered skills found — the paths are wrong"
    for skill in covered:
        assert os.path.exists(os.path.join(_TASKS_SKILLS_DIR, skill, "eval.yaml")), (
            f"{skill} was listed as covered but has no eval.yaml"
        )
        assert os.path.exists(os.path.join(_SKILLS_PACK_DIR, skill, "SKILL.md")), (
            f"{skill} was listed as covered but is not in the skills pack"
        )
    assert covered == sorted(covered), "the matrix order must be stable across runs"


def test_covered_skills_ignores_a_tasks_dir_with_no_matching_skill():
    """`tests/e2e/tasks/skills/targeted/` holds task yaml, not a skill. It is not shardable."""
    covered = covered_skills(_SKILLS_PACK_DIR, _TASKS_SKILLS_DIR)
    assert "targeted" not in covered


# ── --list-skills ────────────────────────────────────────────────────────────


def test_list_skills_prints_json_the_matrix_can_consume():
    """`fromJSON` in the workflow needs a bare JSON array on stdout and nothing else.

    Invoked as a subprocess because that is how the workflow calls it: a stray print or a
    logging line ahead of the payload breaks the matrix with an unhelpful GitHub error.
    """
    env = dict(os.environ, PYTHONPATH=_REPO_ROOT)
    proc = subprocess.run(
        [sys.executable, "-m", "tests.e2e.skill_eval", "--list-skills"],
        cwd=_REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert proc.returncode == 0, f"--list-skills failed: {proc.stderr}"
    listed = json.loads(proc.stdout)
    assert isinstance(listed, list) and listed
    assert listed == covered_skills(_SKILLS_PACK_DIR, _TASKS_SKILLS_DIR)


def test_list_skills_does_not_need_an_api_key():
    """The matrix is computed in a job with no secrets. Requiring OPENAI_API_KEY blocks it."""
    env = {k: v for k, v in os.environ.items() if k != "OPENAI_API_KEY"}
    env["PYTHONPATH"] = _REPO_ROOT
    proc = subprocess.run(
        [sys.executable, "-m", "tests.e2e.skill_eval", "--list-skills"],
        cwd=_REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert proc.returncode == 0, (
        f"--list-skills needs a key it should not need: {proc.stderr}"
    )
    assert json.loads(proc.stdout)


# ── merge_results ────────────────────────────────────────────────────────────


def test_merge_results_reads_every_shard_file(tmp_path):
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    for i, skill in enumerate(["alpha", "beta", "gamma"]):
        write_result(
            EvalRun(
                run_id=f"2026-09-07T00000{i}",
                model="openai/gpt-4.1",
                judge_model="openai/gpt-4.1",
                timestamp="2026-09-07T00:00:00Z",
                regression_threshold=0.05,
                skills=[_result(skill)],
                summary={},
            ),
            str(tmp_path),
        )
    merged = merge_results(str(tmp_path))
    assert sorted(r.skill for r in merged) == ["alpha", "beta", "gamma"]
    assert all(isinstance(r, SkillResult) for r in merged)


def test_merge_results_keeps_the_regression_flag(tmp_path):
    """The aggregate job's exit code is the gate. A dropped flag turns a red night green."""
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    write_result(
        EvalRun(
            run_id="2026-09-07T000000",
            model="openai/gpt-4.1",
            judge_model="openai/gpt-4.1",
            timestamp="2026-09-07T00:00:00Z",
            regression_threshold=0.05,
            skills=[_result("ok"), _result("broken", lift=-0.4, regression=True)],
            summary={},
        ),
        str(tmp_path),
    )
    merged = merge_results(str(tmp_path))
    flagged = [r.skill for r in merged if r.regression_flag]
    assert flagged == ["broken"]


def test_merge_results_dedupes_a_rerun_shard(tmp_path):
    """Re-running one shard leaves two files for that skill. The newer run wins."""
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    for run_id, lift in [("2026-09-07T000000", 0.1), ("2026-09-07T010000", 0.5)]:
        write_result(
            EvalRun(
                run_id=run_id,
                model="openai/gpt-4.1",
                judge_model="openai/gpt-4.1",
                timestamp="2026-09-07T00:00:00Z",
                regression_threshold=0.05,
                skills=[_result("alpha", lift=lift)],
                summary={},
            ),
            str(tmp_path),
        )
    merged = merge_results(str(tmp_path))
    assert len(merged) == 1
    assert merged[0].lift == 0.5


def test_merge_results_on_an_empty_directory_is_empty_not_an_error(tmp_path):
    """Every shard failing to upload is a real outcome; it must not crash the aggregate job."""
    assert merge_results(str(tmp_path)) == []


def test_merge_results_finds_files_in_per_shard_subdirectories(tmp_path):
    """`download-artifact` with no name unpacks each artifact into its own subdirectory."""
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    for skill in ["alpha", "beta"]:
        shard_dir = tmp_path / f"skill-eval-results-{skill}"
        write_result(
            EvalRun(
                run_id=f"2026-09-07T00000-{skill}",
                model="openai/gpt-4.1",
                judge_model="openai/gpt-4.1",
                timestamp="2026-09-07T00:00:00Z",
                regression_threshold=0.05,
                skills=[_result(skill)],
                summary={},
            ),
            str(shard_dir),
        )
    merged = merge_results(str(tmp_path))
    assert sorted(r.skill for r in merged) == ["alpha", "beta"]


def test_merge_results_skips_a_truncated_shard_file(tmp_path):
    """A cancelled shard can leave a half-written JSON. One bad file must not lose the rest."""
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    write_result(
        EvalRun(
            run_id="2026-09-07T000000",
            model="openai/gpt-4.1",
            judge_model="openai/gpt-4.1",
            timestamp="2026-09-07T00:00:00Z",
            regression_threshold=0.05,
            skills=[_result("good")],
            summary={},
        ),
        str(tmp_path),
    )
    (tmp_path / "skill-eval-truncated.json").write_text('{"skills": [{"skill": "bad"')
    merged = merge_results(str(tmp_path))
    assert [r.skill for r in merged] == ["good"]


# ── --merge-results ──────────────────────────────────────────────────────────


def test_merge_results_cli_exits_nonzero_on_a_regression(tmp_path):
    """The aggregate job is the gate, so its exit code has to carry the verdict."""
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    write_result(
        EvalRun(
            run_id="2026-09-07T000000",
            model="openai/gpt-4.1",
            judge_model="openai/gpt-4.1",
            timestamp="2026-09-07T00:00:00Z",
            regression_threshold=0.05,
            skills=[_result("broken", lift=-0.4, regression=True)],
            summary={},
        ),
        str(tmp_path),
    )
    env = dict(os.environ, PYTHONPATH=_REPO_ROOT)
    env.pop("OPENAI_API_KEY", None)
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "tests.e2e.skill_eval",
            "--merge-results",
            str(tmp_path),
            "--output",
            str(tmp_path / "out"),
        ],
        cwd=_REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert proc.returncode == 1, (
        f"a regression must fail the job:\n{proc.stdout}\n{proc.stderr}"
    )
    assert "broken" in proc.stdout


def test_merge_results_cli_exits_zero_when_clean(tmp_path):
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    write_result(
        EvalRun(
            run_id="2026-09-07T000000",
            model="openai/gpt-4.1",
            judge_model="openai/gpt-4.1",
            timestamp="2026-09-07T00:00:00Z",
            regression_threshold=0.05,
            skills=[_result("ok")],
            summary={},
        ),
        str(tmp_path),
    )
    env = dict(os.environ, PYTHONPATH=_REPO_ROOT)
    env.pop("OPENAI_API_KEY", None)
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "tests.e2e.skill_eval",
            "--merge-results",
            str(tmp_path),
            "--output",
            str(tmp_path / "out"),
        ],
        cwd=_REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert proc.returncode == 0, f"{proc.stdout}\n{proc.stderr}"


def test_merge_results_cli_writes_a_combined_result_file(tmp_path):
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    for skill in ["alpha", "beta"]:
        write_result(
            EvalRun(
                run_id=f"2026-09-07T00000-{skill}",
                model="openai/gpt-4.1",
                judge_model="openai/gpt-4.1",
                timestamp="2026-09-07T00:00:00Z",
                regression_threshold=0.05,
                skills=[_result(skill)],
                summary={},
            ),
            str(tmp_path / f"shard-{skill}"),
        )
    out = tmp_path / "out"
    env = dict(os.environ, PYTHONPATH=_REPO_ROOT)
    env.pop("OPENAI_API_KEY", None)
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "tests.e2e.skill_eval",
            "--merge-results",
            str(tmp_path),
            "--output",
            str(out),
        ],
        cwd=_REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert proc.returncode == 0, f"{proc.stdout}\n{proc.stderr}"
    written = [p for p in os.listdir(out) if p.startswith("skill-eval-")]
    assert written, f"no combined result written to {out}"
    with open(os.path.join(out, written[0])) as f:
        combined = json.load(f)
    measured = sorted(s["skill"] for s in combined["skills"] if s["lift"] is not None)
    assert measured == ["alpha", "beta"]
    # The real covered skills had no shard here, so they come back as holes, not as passes.
    # A hole is `not_comparable` with a reason, not `no_task_coverage`: these skills have tasks,
    # and saying otherwise would blame the corpus for a shard that failed to upload.
    holes = {
        s["skill"]
        for s in combined["skills"]
        if s["outcome_reason"] == "shard produced no result"
    }
    assert holes == set(covered_skills(_SKILLS_PACK_DIR, _TASKS_SKILLS_DIR))


def test_merge_results_cli_reports_skills_no_shard_covered(tmp_path):
    """A shard that never uploaded is a silent hole. Name it, do not average it away."""
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    write_result(
        EvalRun(
            run_id="2026-09-07T000000",
            model="openai/gpt-4.1",
            judge_model="openai/gpt-4.1",
            timestamp="2026-09-07T00:00:00Z",
            regression_threshold=0.05,
            skills=[_result("alpha")],
            summary={},
        ),
        str(tmp_path),
    )
    env = dict(os.environ, PYTHONPATH=_REPO_ROOT)
    env.pop("OPENAI_API_KEY", None)
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "tests.e2e.skill_eval",
            "--merge-results",
            str(tmp_path),
            "--output",
            str(tmp_path / "out"),
        ],
        cwd=_REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    covered = covered_skills(_SKILLS_PACK_DIR, _TASKS_SKILLS_DIR)
    assert "Missing shard" in proc.stdout, proc.stdout
    for skill in covered:
        assert skill in proc.stdout, f"{skill} has no shard result and was not reported"


def test_merge_results_cli_needs_no_iris_and_no_api_key(tmp_path):
    """Aggregation is pure file IO. It must not start a container or need a secret."""
    env = {
        k: v
        for k, v in os.environ.items()
        if k not in ("OPENAI_API_KEY", "IRIS_HOST", "IRIS_WEB_PORT", "IRIS_CONTAINER")
    }
    env["PYTHONPATH"] = _REPO_ROOT
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "tests.e2e.skill_eval",
            "--merge-results",
            str(tmp_path),
            "--output",
            str(tmp_path / "out"),
        ],
        cwd=_REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert proc.returncode == 0, f"{proc.stdout}\n{proc.stderr}"


# ── 118 T026: what the merge has to say about the night ──────────────────────


def _arm(pass_rate, total=12, unscored=0):
    scored = total - unscored
    return {
        "items_total": total,
        "items_scored": scored,
        "items_unscored": unscored,
        "items_passed": 0 if pass_rate is None else round(pass_rate * scored),
        "pass_rate": pass_rate,
    }


def _measured(skill, lift=0.1, unscored=0, total=12, outcome="held"):
    result = _result(skill, lift=lift)
    result.outcome = outcome
    result.arms = {
        "baseline": _arm(0.5, total=total, unscored=unscored),
        "skill": _arm(0.6, total=total),
    }
    result.threshold_applied = 0.05
    return result


def _write_shard(directory, run_id, results, threshold=0.05, run_valid=True):
    from tests.e2e.skill_eval.reporter import EvalRun, write_result

    return write_result(
        EvalRun(
            run_id=run_id,
            model="openai/gpt-4.1",
            judge_model="claude-sonnet-4-6",
            timestamp="2026-09-12T00:00:00Z",
            regression_threshold=threshold,
            skills=results,
            summary={"run_valid": run_valid},
            run_valid=run_valid,
        ),
        str(directory),
    )


def test_the_merged_unscored_share_is_recomputed_over_all_items(tmp_path):
    """Averaging shares would let a two-item shard outweigh a forty-item one.

    Shard A: 2 unscored of 24. Shard B: 0 of 24. The night is 2/48 = 4.2%, not the mean of
    8.3% and 0% — and the 10% gate has to read the night.
    """
    from tests.e2e.skill_eval.shard import merge_shards

    _write_shard(tmp_path, "2026-09-12T000001", [_measured("alpha", unscored=2)])
    _write_shard(tmp_path, "2026-09-12T000002", [_measured("beta")])
    merged = merge_shards(str(tmp_path))
    assert merged.items_total == 48
    assert merged.items_unscored == 2
    assert merged.items_unscored_share == pytest.approx(2 / 48, abs=1e-4)


def test_merged_run_valid_is_the_and_of_the_shards(tmp_path):
    """One shard that measured nothing makes the night's numbers partial, not fine."""
    from tests.e2e.skill_eval.shard import merge_shards

    _write_shard(tmp_path, "2026-09-12T000001", [_measured("alpha")], run_valid=True)
    _write_shard(tmp_path, "2026-09-12T000002", [_measured("beta")], run_valid=False)
    assert merge_shards(str(tmp_path)).run_valid is False


def test_merged_run_valid_is_true_when_every_shard_was(tmp_path):
    from tests.e2e.skill_eval.shard import merge_shards

    _write_shard(tmp_path, "2026-09-12T000001", [_measured("alpha")])
    _write_shard(tmp_path, "2026-09-12T000002", [_measured("beta")])
    assert merge_shards(str(tmp_path)).run_valid is True


def test_a_threshold_mismatch_between_shards_is_reported_with_both_values(tmp_path):
    """FR-010. Half a table judged under a different rule is not one table."""
    from tests.e2e.skill_eval.shard import merge_shards

    _write_shard(tmp_path, "2026-09-12T000001", [_measured("alpha")], threshold=0.05)
    _write_shard(tmp_path, "2026-09-12T000002", [_measured("beta")], threshold=0.20)
    merged = merge_shards(str(tmp_path))
    assert merged.threshold_conflict == [0.05, 0.2]
    assert merged.threshold is None, "there is no one threshold to print"


def test_one_threshold_across_shards_is_the_runs_threshold(tmp_path):
    from tests.e2e.skill_eval.shard import merge_shards

    _write_shard(tmp_path, "2026-09-12T000001", [_measured("alpha")], threshold=0.05)
    _write_shard(tmp_path, "2026-09-12T000002", [_measured("beta")], threshold=0.05)
    merged = merge_shards(str(tmp_path))
    assert merged.threshold == pytest.approx(0.05)
    assert merged.threshold_conflict == []


def test_the_merge_names_the_rerun_and_the_run_id_it_discarded(tmp_path):
    """A silently deduplicated table cannot be told from one that never had a duplicate."""
    from tests.e2e.skill_eval.shard import merge_shards

    _write_shard(tmp_path, "2026-09-12T034002", [_measured("alpha", lift=0.1)])
    _write_shard(tmp_path, "2026-09-12T040211", [_measured("alpha", lift=0.5)])
    merged = merge_shards(str(tmp_path))
    assert [r.lift for r in merged.results] == [0.5]
    assert merged.reruns == {"alpha": "2026-09-12T034002"}


def test_no_rerun_means_no_rerun_record(tmp_path):
    from tests.e2e.skill_eval.shard import merge_shards

    _write_shard(tmp_path, "2026-09-12T040211", [_measured("alpha")])
    assert merge_shards(str(tmp_path)).reruns == {}


def test_a_shard_that_uploaded_nothing_is_not_comparable_not_a_failure(tmp_path):
    """FR-009: nothing per-skill fails the run in 118. A hole is a named hole."""
    from tests.e2e.skill_eval.shard import missing_shard_result

    result = missing_shard_result("iris-ai-hub")
    assert result.skill == "iris-ai-hub"
    assert result.outcome == "not_comparable"
    assert result.outcome_reason == "shard produced no result"
    assert result.regression_flag is False
    assert result.lift is None
    assert result.no_task_coverage is False, (
        "the skill has tasks; the shard is what went missing"
    )


def test_merge_shards_counts_the_shards_it_read(tmp_path):
    from tests.e2e.skill_eval.shard import merge_shards

    _write_shard(tmp_path, "2026-09-12T000001", [_measured("alpha")])
    _write_shard(tmp_path, "2026-09-12T000002", [_measured("beta")])
    assert merge_shards(str(tmp_path)).shards_read == 2


def test_merge_results_is_still_the_result_list(tmp_path):
    """`merge_results` keeps its shape: the CLI and the older tests read it as a list."""
    from tests.e2e.skill_eval.shard import merge_shards

    _write_shard(tmp_path, "2026-09-12T000001", [_measured("alpha")])
    assert [r.skill for r in merge_results(str(tmp_path))] == [
        r.skill for r in merge_shards(str(tmp_path)).results
    ]
