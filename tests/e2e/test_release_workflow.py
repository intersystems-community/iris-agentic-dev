"""Guards on .github/workflows/release.yml ordering and coverage.

A broken release workflow is only discoverable by cutting a tag, and a tag is
the one thing that cannot be taken back once the Marketplace has accepted a
publish. These tests read the workflow as data so the ordering mistakes are
caught by `cargo test`-adjacent CI instead of by users.
"""

import os

import pytest

yaml = pytest.importorskip("yaml")

_REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
_RELEASE_YML = os.path.join(_REPO_ROOT, ".github", "workflows", "release.yml")


@pytest.fixture(scope="module")
def workflow() -> dict:
    with open(_RELEASE_YML) as f:
        return yaml.safe_load(f)


def _job(workflow: dict, name: str) -> dict:
    jobs = workflow["jobs"]
    assert name in jobs, f"release.yml has no '{name}' job (jobs: {sorted(jobs)})"
    return jobs[name]


def _steps_named(job: dict, needle: str) -> list:
    return [s for s in job["steps"] if needle.lower() in str(s.get("name", "")).lower()]


# ── The Marketplace publish must not outrun the release it points at ─────────


def test_marketplace_publish_waits_for_the_github_release():
    """The published extension downloads binaries from the release being cut.

    vsce publish is irreversible: once 0.4.x is live on the Marketplace, users
    auto-update to it within hours and the version can never be re-pushed. If
    that happens before the `release` job has attached the binaries, every
    auto-install 404s for the length of the gap, and any user who installs
    during it caches a failure. The publish therefore has to depend on
    `release`, not race it.
    """
    with open(_RELEASE_YML) as f:
        workflow = yaml.safe_load(f)

    publishers = [
        (name, job)
        for name, job in workflow["jobs"].items()
        if any(
            "vsce publish" in str(step.get("run", ""))
            for step in job.get("steps", [])
        )
    ]
    assert publishers, "no job runs 'vsce publish' — did the publish step move?"

    for name, job in publishers:
        needs = job.get("needs") or []
        needs = [needs] if isinstance(needs, str) else needs
        assert "release" in needs, (
            f"job '{name}' publishes to the Marketplace but does not need "
            f"'release' (needs: {needs}). The extension it publishes downloads "
            f"binaries from the GitHub release, so publishing first ships an "
            f"extension that 404s until the release job finishes."
        )


def test_release_job_does_not_depend_on_the_publisher():
    """Guards against fixing the ordering by creating a dependency cycle."""
    with open(_RELEASE_YML) as f:
        workflow = yaml.safe_load(f)

    release_needs = _job(workflow, "release").get("needs") or []
    release_needs = [release_needs] if isinstance(release_needs, str) else release_needs

    for dep in release_needs:
        dep_job = workflow["jobs"][dep]
        assert not any(
            "vsce publish" in str(step.get("run", ""))
            for step in dep_job.get("steps", [])
        ), (
            f"'release' needs '{dep}', which publishes to the Marketplace. "
            f"That is a cycle: the publish must wait for the release."
        )


# ── The version guard must run before anything irreversible ─────────────────


def test_version_guard_runs_before_the_vsix_is_built(workflow):
    """A mismatched serverVersion has to fail before any artifact is produced."""
    job = _job(workflow, "build-vsix")
    steps = job["steps"]

    guard_idx = next(
        (i for i, s in enumerate(steps) if "serverVersion" in str(s.get("run", ""))),
        None,
    )
    assert guard_idx is not None, "build-vsix lost the serverVersion guard"

    package_idx = next(
        (i for i, s in enumerate(steps) if "run package" in str(s.get("run", ""))),
        None,
    )
    assert package_idx is not None, "build-vsix no longer packages the extension"
    assert guard_idx < package_idx, (
        "the serverVersion guard runs after the .vsix is built. Check the "
        "version before spending the build, so a mismatch fails fast."
    )


def test_version_guard_can_fail_the_build(workflow):
    """continue-on-error on the guard would make it decorative."""
    job = _job(workflow, "build-vsix")
    for step in job["steps"]:
        if "serverVersion" in str(step.get("run", "")):
            assert step.get("continue-on-error") is not True, (
                "the serverVersion guard is continue-on-error, so a mismatched "
                "version would warn and ship anyway"
            )


# ── Assets the downstream consumers expect must actually be published ───────


def test_every_platform_the_extension_downloads_is_built(workflow):
    """platform.ts maps platform+arch to an asset name; all four must exist.

    getBinaryName returning a name the release never uploads is a 404 the
    extension cannot recover from.
    """
    expected = {
        "iris-agentic-dev-macos-arm64",
        "iris-agentic-dev-macos-x86_64",
        "iris-agentic-dev-linux-x86_64",
        "iris-agentic-dev-windows-x86_64.exe",
    }
    built = {
        entry["artifact"]
        for entry in _job(workflow, "build")["strategy"]["matrix"]["include"]
    }
    missing = expected - built
    assert not missing, f"the extension downloads these but the build never makes them: {missing}"

    released = _job(workflow, "release")["steps"][-1]["with"]["files"]
    released_names = {line.strip() for line in released.splitlines() if line.strip()}
    not_attached = expected - released_names
    assert not not_attached, f"built but never attached to the release: {not_attached}"


def test_homebrew_tap_update_waits_for_the_release(workflow):
    """The formula's url points into the release; publishing it early 404s brew."""
    needs = _job(workflow, "update-homebrew-tap").get("needs") or []
    needs = [needs] if isinstance(needs, str) else needs
    assert "release" in needs, (
        f"update-homebrew-tap does not need 'release' (needs: {needs}); the "
        f"formula would point at assets that are not uploaded yet"
    )


# ── The trigger must not fire on extension-only tags ────────────────────────


def test_trigger_excludes_vscode_tags(workflow):
    """`v*` also matched `vscode-v0.4.24` and cut a bogus binary release."""
    import fnmatch

    # PyYAML parses the bare `on:` key as the boolean True.
    triggers = workflow.get("on") or workflow[True]
    patterns = triggers["push"]["tags"]

    assert any(fnmatch.fnmatch("v0.9.6", p) for p in patterns), (
        f"a normal binary tag no longer triggers the release: {patterns}"
    )
    assert any(fnmatch.fnmatch("v0.10.0", p) for p in patterns), (
        f"a two-digit minor version no longer triggers the release: {patterns}"
    )
    assert not any(fnmatch.fnmatch("vscode-v0.4.26", p) for p in patterns), (
        f"an extension-only tag still triggers a full binary release: {patterns}"
    )
