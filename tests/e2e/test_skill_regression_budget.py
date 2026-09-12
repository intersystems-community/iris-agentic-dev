"""The nightly Skill Regression job has to fit inside its own timeout.

It did not. From 2026-08-29 to 2026-09-07 every scheduled run finished as `cancelled` at
exactly 1h15m — the `timeout-minutes: 75` on a single sequential job. Run 34093536537 shows
where it got to: five of nine skills plus part of the sixth in 68 minutes, still printing
progress when the runner killed it. No hang, no stuck subprocess (`collect_events` caps a
session at 300 s); the work is simply about 100 minutes long.

The 75 came from the dry-run estimate, which said 67.2 min. That estimate was wrong twice
over: it never counted the implicit fire-rate pass, and it priced a session at 15 s when the
measured cost is 22 s. So these tests check the two things that let the mistake happen —
budget honesty and job shape — rather than just asserting a bigger number.
"""

import os

import pytest

yaml = pytest.importorskip("yaml")

from tests.e2e.skill_eval.cost_estimator import estimate, estimate_skill  # noqa: E402
from tests.e2e.skill_eval.evaluator import load_eval_config  # noqa: E402
from tests.e2e.skill_eval.shard import covered_skills  # noqa: E402

_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
_WORKFLOW = os.path.join(_REPO_ROOT, ".github", "workflows", "skill-regression.yml")
_SKILLS_PACK_DIR = os.path.join(_REPO_ROOT, "skills", "skills")
_TASKS_SKILLS_DIR = os.path.join(_REPO_ROOT, "tests", "e2e", "tasks", "skills")

# The nightly schedule passes no `runs` input, so the workflow default applies.
_NIGHTLY_RUNS = 5

# Checkout, pip install, npm install -g opencode, IRIS container start + Atelier wait, and the
# harness unit tests all run before the eval step. Measured at 3m18s in run 34093536537
# (07:02:32 job start → 07:05:14 "Running skill evaluation"), rounded up hard because a slow
# GHA runner or a cold npm cache is the normal case, not the exception.
_SETUP_ALLOWANCE_MIN = 15


def _workflow() -> dict:
    with open(_WORKFLOW) as f:
        return yaml.safe_load(f)


def _configs():
    return [
        load_eval_config(s, _TASKS_SKILLS_DIR)
        for s in covered_skills(_SKILLS_PACK_DIR, _TASKS_SKILLS_DIR)
    ]


def _eval_job(workflow: dict) -> tuple[str, dict]:
    """The job that actually runs the eval — the one whose steps invoke the harness.

    Every mode of the CLI that starts no session has to be excluded by name, because they all
    invoke the same module. `--preflight-only` is the one that caught this out: it lives in
    `discover`, which runs first, so six tests here started describing the wrong job and
    failing about its missing matrix.
    """
    not_an_eval = ("--list-skills", "--merge-results", "--preflight-only", "--dry-run")
    for name, job in (workflow.get("jobs") or {}).items():
        for step in job.get("steps") or []:
            run = step.get("run") or ""
            if "tests.e2e.skill_eval" in run and not any(
                flag in run for flag in not_an_eval
            ):
                return name, job
    pytest.fail("no job in skill-regression.yml runs the skill_eval harness")


def test_the_whole_suite_no_longer_fits_in_one_job():
    """This is the measurement that the 75-minute timeout was missing.

    If the corrected estimate ever drops back under an hour the sharding is no longer earning
    its complexity — but until then, one job cannot do this.
    """
    est = estimate(_configs(), runs=_NIGHTLY_RUNS)
    assert est["time_minutes"] + _SETUP_ALLOWANCE_MIN > 75, (
        f"the suite now estimates {est['time_minutes']} min; if that is real, a single "
        f"75-minute job would work again and this file is describing a fixed problem"
    )


def test_the_eval_job_is_sharded_one_skill_at_a_time():
    name, job = _eval_job(_workflow())
    matrix = (job.get("strategy") or {}).get("matrix")
    assert matrix, (
        f"job `{name}` has no matrix — it would run all skills sequentially again"
    )
    assert "skill" in matrix, f"job `{name}` shards on {list(matrix)}, not on `skill`"
    step_runs = " ".join(step.get("run") or "" for step in job["steps"])
    assert "--skill" in step_runs, (
        f"job `{name}` shards on skill but never passes `--skill`, so every shard would "
        f"run the whole suite"
    )


def test_the_matrix_is_derived_from_the_harness_not_hardcoded():
    """A hand-written skill list rots the moment someone adds an eval.yaml.

    `--list-skills` reads the same directories the eval reads, so the matrix cannot drift
    out of sync with what is actually covered.
    """
    workflow = _workflow()
    _, job = _eval_job(workflow)
    matrix_skill = str((job.get("strategy") or {}).get("matrix", {}).get("skill", ""))
    assert "fromJSON" in matrix_skill, (
        f"the matrix skill list is a literal ({matrix_skill!r}); derive it from "
        f"`--list-skills` instead"
    )
    generator = " ".join(
        step.get("run") or ""
        for j in workflow["jobs"].values()
        for step in (j.get("steps") or [])
    )
    assert "--list-skills" in generator, (
        "nothing in the workflow calls `--list-skills`, so `fromJSON` has no source"
    )


def test_the_shard_timeout_covers_the_slowest_skill():
    """iris-ai-hub is 6 benchmark tasks × 2 conditions × 5 runs = 60 sessions on its own."""
    name, job = _eval_job(_workflow())
    timeout = job.get("timeout-minutes")
    assert timeout, (
        f"job `{name}` has no timeout-minutes — a wedged shard would run for 6 h"
    )
    worst = max(
        estimate_skill(c, runs=_NIGHTLY_RUNS)["time_minutes"] for c in _configs()
    )
    assert timeout >= worst + _SETUP_ALLOWANCE_MIN, (
        f"job `{name}` allows {timeout} min per shard; the slowest skill needs "
        f"{worst} min of eval plus ~{_SETUP_ALLOWANCE_MIN} min of setup"
    )


def test_a_failing_shard_does_not_cancel_the_others():
    """`fail-fast` defaults to true, which would throw away eight good results for one bad."""
    name, job = _eval_job(_workflow())
    strategy = job.get("strategy") or {}
    assert strategy.get("fail-fast") is False, (
        f"job `{name}` leaves fail-fast at its default, so the first regression cancels "
        f"every other skill and the nightly report comes back mostly empty"
    )


def test_the_baseline_is_written_by_exactly_one_job():
    """Nine shards racing `git push` on one file is nine conflicts, not one baseline."""
    workflow = _workflow()
    pushers = [
        name
        for name, job in workflow["jobs"].items()
        if any(
            "git push" in (step.get("run") or "") for step in (job.get("steps") or [])
        )
    ]
    assert len(pushers) <= 1, f"more than one job pushes the baseline: {pushers}"
    for name in pushers:
        assert not (workflow["jobs"][name].get("strategy") or {}).get("matrix"), (
            f"job `{name}` pushes the baseline from inside a matrix — every shard would "
            f"commit over the others"
        )


def test_every_shard_uploads_its_own_result():
    """Aggregation reads artifacts, so a shard whose name collides overwrites its neighbour."""
    _, job = _eval_job(_workflow())
    uploads = [
        step
        for step in job["steps"]
        if str(step.get("uses", "")).startswith("actions/upload-artifact")
    ]
    assert uploads, "the sharded job uploads nothing — there is nothing to aggregate"
    for step in uploads:
        artifact = str((step.get("with") or {}).get("name", ""))
        assert "matrix.skill" in artifact, (
            f"artifact name {artifact!r} is the same for every shard; the last upload wins"
        )


def test_an_aggregate_job_merges_the_shards_and_gates_on_them():
    workflow = _workflow()
    eval_name, _ = _eval_job(workflow)
    merging = [
        (name, job)
        for name, job in workflow["jobs"].items()
        if any(
            "--merge-results" in (step.get("run") or "")
            for step in (job.get("steps") or [])
        )
    ]
    assert merging, (
        "no job merges the shard results, so a regression in one skill never fails the run"
    )
    for name, job in merging:
        needs = job.get("needs")
        needs = [needs] if isinstance(needs, str) else (needs or [])
        assert eval_name in needs, (
            f"job `{name}` merges shard results but does not need `{eval_name}` — it would "
            f"run before the shards finish"
        )
