"""The dependency-vulnerability scan has to exist, run on pull requests, and run on a schedule.

TPRM-6601 asked whether the project tests for security problems before release. A scan that only runs
when someone pushes misses an advisory published against a crate that has not changed, so it also runs
weekly. These tests keep the workflow from being renamed away, reduced to a no-op, or left without a
schedule.
"""

import os

import pytest

yaml = pytest.importorskip("yaml")

_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
_AUDIT = os.path.join(_ROOT, ".github", "workflows", "audit.yml")


@pytest.fixture(scope="module")
def workflow():
    assert os.path.isfile(_AUDIT), ".github/workflows/audit.yml is missing"
    with open(_AUDIT) as handle:
        return yaml.safe_load(handle)


def _steps(job):
    return [step.get("run", "") + " " + step.get("uses", "") for step in job["steps"]]


def _triggers(workflow):
    # `on` parses to the boolean True, which is also how GitHub reads it.
    return workflow.get("on") or workflow.get(True)


def test_it_runs_on_pull_requests_and_on_master(workflow):
    triggers = _triggers(workflow)
    assert "pull_request" in triggers
    assert "master" in triggers["push"]["branches"]


def test_it_runs_weekly_so_new_advisories_are_caught_without_a_code_change(workflow):
    schedule = _triggers(workflow)["schedule"]
    assert schedule and schedule[0]["cron"].split()[4] != "*", "a weekly cron names a weekday"


def test_the_rust_job_runs_cargo_audit_against_the_lockfile(workflow):
    steps = " ".join(" ".join(_steps(job)) for job in workflow["jobs"].values())
    assert "cargo audit" in steps
    assert "--locked" in steps, "cargo-audit itself is installed from its lockfile"


def test_the_extension_job_audits_its_whole_tree(workflow):
    # Dev dependencies included: vsce and the rest of the dev tooling run in the release job with
    # VSCE_PAT in the environment, so an advisory against them matters as much as a shipped one.
    steps = _steps(workflow["jobs"]["extension"])
    audits = [s for s in steps if "npm audit" in s]
    assert audits, "the extension job runs npm audit"
    for step in audits:
        assert "vscode-iris-agentic-dev" in step
        assert "--omit=dev" not in step, "the audit leaves out the tools that run beside VSCE_PAT"
        assert "--audit-level=high" in step


def test_the_root_lint_tooling_is_audited_too(workflow):
    steps = " ".join(_steps(workflow["jobs"]["lint-tooling"]))
    assert "npm ci" in steps and "npm audit --audit-level=high" in steps


def test_cargo_deny_runs_pinned_against_the_lockfile(workflow):
    # cargo audit only prints yanked, unsound and unmaintained crates. cargo-deny is what fails on
    # them, so it has to be there, at an exact version, reading deny.toml.
    steps = " ".join(_steps(workflow["jobs"]["cargo-deny"]))
    assert "cargo install cargo-deny --locked --version " in steps
    assert "cargo deny --locked check" in steps
    assert os.path.isfile(os.path.join(_ROOT, "deny.toml")), "deny.toml is missing"


def test_a_failing_scan_fails_the_job(workflow):
    for job in workflow["jobs"].values():
        for step in job["steps"]:
            assert step.get("continue-on-error") is not True, "a scan that cannot fail proves nothing"
