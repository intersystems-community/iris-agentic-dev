"""The nightly has to be a breakage guard, and has to stay cheap enough to run nightly.

Two histories are in this file.

From 2026-08-29 to 2026-09-07 every scheduled run finished as `cancelled` at exactly 1h15m — the
`timeout-minutes: 75` on a single sequential job. Run 34093536537 got through five of nine skills
in 68 minutes. Sharding fixed the clock. It did not fix what the job was measuring: six of nine
skills printed 0.00 against 0.00 and the job reported success, and then runs 34744344877,
34817991701 and 34939912456 failed three nights running on Δs smaller than the corpus could
resolve.

121 FR-016 settles it by changing the question. The nightly asks whether the harness runs, whether
the binary still advertises its tools, and whether a named canary set still passes — three
assertion-scored tasks, no grading call, no lift. Lift is reported by the full benchmark, at a
release and before a conference, over a corpus large enough to support it. So these tests assert
the nightly's shape and its silence about lift, and the old sharding assertions are gone with the
job they described.
"""

import os

import pytest

yaml = pytest.importorskip("yaml")

from tests.e2e.skill_eval.cost_estimator import estimate  # noqa: E402
from tests.e2e.skill_eval.evaluator import load_eval_config  # noqa: E402
from tests.e2e.skill_eval.nightly_canary import CANARY_TASKS  # noqa: E402
from tests.e2e.skill_eval.shard import covered_skills  # noqa: E402

_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
_WORKFLOW = os.path.join(_REPO_ROOT, ".github", "workflows", "skill-regression.yml")
_SKILLS_PACK_DIR = os.path.join(_REPO_ROOT, "skills", "skills")
_TASKS_SKILLS_DIR = os.path.join(_REPO_ROOT, "tests", "e2e", "tasks", "skills")

# The nightly schedule passes no `runs` input, so the workflow default applies.
_NIGHTLY_RUNS = 5

# Checkout, pip install, npm install -g opencode-ai, IRIS container start + Atelier wait, and the
# harness unit tests all run before the canary. Measured at 3m18s in run 34093536537, rounded up
# hard because a slow runner or a cold npm cache is the normal case, not the exception.
_SETUP_ALLOWANCE_MIN = 15


def _workflow() -> dict:
    with open(_WORKFLOW) as f:
        return yaml.safe_load(f)


def _steps(workflow: dict) -> str:
    return " ".join(
        step.get("run") or ""
        for job in (workflow.get("jobs") or {}).values()
        for step in (job.get("steps") or [])
    )


def _canary_job(workflow: dict) -> tuple:
    """The job that runs the canary — the one invoking `nightly_canary`."""
    for name, job in (workflow.get("jobs") or {}).items():
        for step in job.get("steps") or []:
            if "nightly_canary" in (step.get("run") or ""):
                return name, job
    pytest.fail(
        "no job in skill-regression.yml runs tests.e2e.skill_eval.nightly_canary — "
        "FR-016 makes the nightly a breakage guard, and a guard nothing invokes is not one"
    )


# ── the nightly is the canary, not the lift run ──────────────────────────────


def test_the_nightly_runs_the_canary_guard():
    name, job = _canary_job(_workflow())
    assert job.get("timeout-minutes"), (
        f"job `{name}` has no timeout-minutes — a wedged session would hold a runner for 6 h"
    )


def test_the_nightly_measures_no_lift_at_all():
    """FR-016 in the workflow: no per-skill leg, no merge, no baseline write.

    Every one of these flags starts an opencode session per benchmark task and ends in a printed
    lift. The nightly ran them for a month and published numbers its corpus could not support.
    """
    runs = _steps(_workflow())
    for forbidden in ("--skill ", "--merge-results", "--update-baseline"):
        assert forbidden not in runs, (
            f"the nightly still invokes `{forbidden.strip()}`, which measures and prints a "
            f"lift. The full benchmark owns lift now (FR-016)"
        )


def test_the_nightly_has_no_per_skill_matrix():
    """One job, three tasks. The nine-shard matrix belonged to the lift run."""
    for name, job in (_workflow().get("jobs") or {}).items():
        matrix = (job.get("strategy") or {}).get("matrix") or {}
        assert "skill" not in matrix, (
            f"job `{name}` still shards on skill — that is the lift run's shape, and it cost "
            f"~$3.70 and two hours of runner time a night"
        )


def test_the_nightly_pushes_nothing():
    """A baseline commit is a claim about measured lift. The nightly no longer makes one."""
    pushers = [
        name
        for name, job in (_workflow().get("jobs") or {}).items()
        if any(
            "git push" in (step.get("run") or "") for step in (job.get("steps") or [])
        )
    ]
    assert pushers == [], f"the nightly still pushes from {pushers}"


def test_the_canary_set_is_named_in_the_workflow_or_read_from_the_module():
    """The set is declared once, in `nightly_canary.CANARY_TASKS`.

    A second hand-written list in YAML is a list that rots — the workflow invokes the module and
    the module names the tasks.
    """
    runs = _steps(_workflow())
    hardcoded = [task for task in CANARY_TASKS if task in runs]
    assert not hardcoded, (
        f"the workflow names canary tasks itself ({hardcoded}); let CANARY_TASKS be the one "
        f"source, so adding a canary does not need a YAML edit"
    )


def test_the_nightly_still_installs_the_binary_it_checks():
    """The tool-surface probe reads `IAD_BINARY`, and without it the check is unanswerable.

    The nightly ran for weeks with no iad tools in any session: `isolated_env.py` held a literal
    Homebrew path that no runner has. Every transcript was a bare model and the judge scored it
    as the skill failing.
    """
    runs = _steps(_workflow())
    assert "IAD_BINARY" in runs, (
        "nothing exports IAD_BINARY, so the tool surface reads as None"
    )


def test_the_nightly_runs_the_harness_unit_tests():
    """The cheapest breakage signal there is, and it costs no session at all."""
    assert "pytest" in _steps(_workflow())


# ── the budget ───────────────────────────────────────────────────────────────


def _configs():
    return [
        load_eval_config(s, _TASKS_SKILLS_DIR)
        for s in covered_skills(_SKILLS_PACK_DIR, _TASKS_SKILLS_DIR)
    ]


def test_the_canary_is_a_fraction_of_the_suite_it_replaces():
    """Three sessions against ~450. If that ratio ever closes, the nightly is a benchmark again."""
    suite = estimate(_configs(), runs=_NIGHTLY_RUNS)
    # 22 s a session measured, 3 canary tasks, no repeats — the canary is a yes/no, not a rate.
    canary_minutes = len(CANARY_TASKS) * 22 / 60
    assert canary_minutes * 10 < suite["time_minutes"], (
        f"the canary estimates {canary_minutes:.1f} min against the suite's "
        f"{suite['time_minutes']} min; it is meant to be an order of magnitude cheaper"
    )


def test_the_nightly_timeout_covers_the_canary_with_room_for_setup():
    name, job = _canary_job(_workflow())
    timeout = job.get("timeout-minutes")
    canary_minutes = len(CANARY_TASKS) * 22 / 60
    assert timeout >= canary_minutes + _SETUP_ALLOWANCE_MIN, (
        f"job `{name}` allows {timeout} min; the canary needs ~{canary_minutes:.1f} min of "
        f"sessions plus ~{_SETUP_ALLOWANCE_MIN} min of setup"
    )
    assert timeout <= 45, (
        f"job `{name}` allows {timeout} min for three assertion-scored tasks. A generous "
        f"timeout on a cheap job is how a two-hour lift run gets added back without anyone "
        f"noticing the clock"
    )


def test_no_eval_result_file_is_tracked_in_git():
    """A tracked `skill-eval-*.json` rides along in every artifact upload.

    Each shard used to upload `tests/e2e/results/skill-eval-*.json`, which is a glob over the
    whole directory — so 35 result files from May and June, force-added past that directory's own
    `.gitignore`, arrived in all nine artifacts. Run 34707534110's merge read 324 files to find
    nine, and its footer reported seven skills as re-runs that discarded a 2026-05-31 result.
    """
    import subprocess

    tracked = subprocess.run(
        ["git", "ls-files", "tests/e2e/results/"],
        cwd=_REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split()
    stale = [p for p in tracked if os.path.basename(p).startswith("skill-eval-")]
    assert not stale, (
        f"{len(stale)} eval result file(s) are tracked: {stale[:3]}. They are ignored by "
        f"tests/e2e/results/.gitignore for a reason — every artifact carries all of "
        f"them. `git rm` them; only skill-baseline.json belongs in the repo."
    )
