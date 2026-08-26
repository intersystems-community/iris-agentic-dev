"""Every file in .github/workflows/ has to parse and describe runnable jobs.

An unparseable workflow does not fail loudly. GitHub records a 0-second run with no jobs,
labels it with the file path instead of the workflow name, and says "This run likely failed
because of a workflow file issue". That is a red X indistinguishable at a glance from a
test failure, so ci.yml sat broken from 2026-08-24 (commit 4dec14f, an inline `python -c`
heredoc at column 0 inside `run: |`) through 2026-08-26 with nothing running on master.

These tests parse the workflows the way GitHub does. They cannot protect the workflow they
run under — a broken ci.yml never reaches this file — so the same failure is also caught
before a push by `crates/iris-agentic-dev-bin/tests/unit/test_workflow_files.rs`, which
runs in `cargo test`. This layer covers the other workflows and the semantic checks a plain
text scan cannot make.
"""

import glob
import os

import pytest

yaml = pytest.importorskip("yaml")

_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
_WORKFLOW_DIR = os.path.join(_REPO_ROOT, ".github", "workflows")


def _workflow_files() -> list[str]:
    files = sorted(
        glob.glob(os.path.join(_WORKFLOW_DIR, "*.yml"))
        + glob.glob(os.path.join(_WORKFLOW_DIR, "*.yaml"))
    )
    assert files, f"no workflow files under {_WORKFLOW_DIR}"
    return files


def _load(path: str) -> dict:
    with open(path) as f:
        try:
            return yaml.safe_load(f)
        except yaml.YAMLError as e:
            pytest.fail(f"{os.path.basename(path)} is not valid YAML: {e}")


@pytest.mark.parametrize("path", _workflow_files(), ids=os.path.basename)
def test_workflow_parses_and_declares_jobs(path):
    workflow = _load(path)
    assert isinstance(workflow, dict), (
        f"{os.path.basename(path)} must parse to a mapping"
    )

    # `on` is the YAML 1.1 boolean True once parsed, which is also how GitHub reads it.
    assert workflow.get("name"), f"{os.path.basename(path)} has no top-level `name`"
    assert "on" in workflow or True in workflow, (
        f"{os.path.basename(path)} has no trigger (`on:`) — it can never run"
    )

    jobs = workflow.get("jobs")
    assert isinstance(jobs, dict) and jobs, (
        f"{os.path.basename(path)} declares no jobs — this is what a clipped block scalar "
        f"looks like after parsing, and what GitHub reports as a workflow file issue"
    )


@pytest.mark.parametrize("path", _workflow_files(), ids=os.path.basename)
def test_every_step_runs_something(path):
    """A step with neither `uses` nor a non-empty `run` is a step that lost its body."""
    workflow = _load(path)
    for job_name, job in (workflow.get("jobs") or {}).items():
        if "uses" in job:  # reusable workflow call — no steps of its own
            continue
        steps = job.get("steps")
        assert steps, f"{os.path.basename(path)}: job `{job_name}` has no steps"
        for position, step in enumerate(steps, start=1):
            label = step.get("name") or step.get("uses") or f"step {position}"
            if "uses" in step:
                continue
            run = step.get("run")
            assert isinstance(run, str) and run.strip(), (
                f"{os.path.basename(path)}: job `{job_name}` step `{label}` has neither "
                f"`uses` nor a non-empty `run:`"
            )
