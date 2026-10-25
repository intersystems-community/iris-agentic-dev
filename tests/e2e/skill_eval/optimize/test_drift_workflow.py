"""The nightly drift step — 128 T031 (FR-012).

The nightly re-scores the shipped descriptions and runs no optimiser. The band it compares against
is committed, so a clean checkout can run it.
"""

from __future__ import annotations

import json
from pathlib import Path

import yaml

from tests.e2e.skill_eval.optimize import runner

WORKFLOW = Path(runner.menu.REPO) / ".github" / "workflows" / "skill-regression.yml"


def _runs() -> str:
    wf = yaml.safe_load(WORKFLOW.read_text())
    return "\n".join(
        s.get("run", "") for job in wf["jobs"].values() for s in job["steps"]
    )


def test_nightly_runs_drift_and_no_optimiser():
    runs = _runs()
    assert "python -m tests.e2e.skill_eval.optimize drift" in runs
    assert "optimize run" not in runs and "optimize apply" not in runs


def test_nightly_installs_the_pinned_requirements():
    """The nightly installs the optimiser's pins through the hashed lockfile, not on their own.

    `.github/requirements/nightly.txt` is compiled from nightly.in, which includes this directory's
    requirements.txt. A pin bumped here but not recompiled into the lockfile would never reach the
    nightly, so every exact pin must appear in the lockfile at the same version.
    """
    repo = Path(runner.menu.REPO)
    assert "--require-hashes -r .github/requirements/nightly.txt" in _runs()
    source = (repo / ".github" / "requirements" / "nightly.in").read_text()
    assert "-r ../../tests/e2e/skill_eval/optimize/requirements.txt" in source
    lock = (repo / ".github" / "requirements" / "nightly.txt").read_text().lower()
    reqs = (Path(__file__).parent / "requirements.txt").read_text().splitlines()
    for pin in (r.split("#")[0].strip() for r in reqs):
        if "==" in pin:
            assert pin.lower() in lock, f"{pin} is not in nightly.txt; recompile the lockfile"


def test_drift_band_is_committed_and_well_formed():
    band = json.loads(runner.DRIFT_FILE.read_text())
    assert 0.0 <= band["recall_lo"] <= band["recall"] <= band["recall_hi"] <= 1.0
    assert band["n"] >= 30 and band["scorer_models"]
