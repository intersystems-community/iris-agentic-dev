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


# ── What a release publishes must be signed, and only from master ───────────

_PUBLISHING_JOBS = ("release", "publish-marketplace", "docker-publish", "update-homebrew-tap")


def _needs(job: dict) -> list:
    needs = job.get("needs") or []
    return [needs] if isinstance(needs, str) else needs


def _uses(job: dict, action: str) -> list:
    return [s for s in job.get("steps", []) if str(s.get("uses", "")).startswith(action + "@")]


def test_nothing_runs_before_the_tag_is_checked(workflow):
    """A tag on an unreviewed branch must not build, let alone publish.

    verify-tag refuses a tag whose commit is not on master. That only protects anything if every
    job is downstream of it; a job with no `needs` starts the moment the tag is pushed.
    """
    jobs = workflow["jobs"]

    def upstream(name, seen=None):
        seen = set() if seen is None else seen
        for dep in _needs(jobs[name]):
            if dep not in seen:
                seen.add(dep)
                upstream(dep, seen)
        return seen

    for name in jobs:
        if name == "verify-tag":
            continue
        assert "verify-tag" in upstream(name), (
            f"'{name}' does not wait for verify-tag, directly or through another job"
        )


def test_tag_check_compares_against_master(workflow):
    job = _job(workflow, "verify-tag")
    script = "\n".join(str(s.get("run", "")) for s in job["steps"])
    assert "merge-base --is-ancestor" in script and "origin/master" in script, (
        "verify-tag no longer checks that the tagged commit is on master"
    )
    checkout = _uses(job, "actions/checkout")[0]
    assert checkout.get("with", {}).get("fetch-depth") == 0, (
        "verify-tag needs full history, or origin/master is missing and the ancestry check errors"
    )


def test_every_publishing_job_waits_for_approval(workflow):
    """The release environment is where the approval rule and the publishing secrets live.

    A publishing job without `environment: release` runs unapproved, and once the secrets move out
    of the repository it also silently loses them: vsce skips with a warning, the tap push fails.
    """
    for name in _PUBLISHING_JOBS:
        assert _job(workflow, name).get("environment") == "release", (
            f"'{name}' publishes but does not run in the 'release' environment"
        )


def test_every_released_binary_has_build_provenance(workflow):
    job = _job(workflow, "release")
    attest = _uses(job, "actions/attest-build-provenance")
    assert attest, "the release job no longer attests build provenance"
    subjects = {line.strip() for line in attest[0]["with"]["subject-path"].splitlines() if line.strip()}
    built = {e["artifact"] for e in _job(workflow, "build")["strategy"]["matrix"]["include"]}
    missing = built - subjects
    assert not missing, f"published without provenance: {sorted(missing)}"
    perms = job.get("permissions", {})
    assert perms.get("id-token") == "write" and perms.get("attestations") == "write", (
        f"attesting needs id-token: write and attestations: write (has {perms})"
    )


def test_checksum_signature_is_published(workflow):
    job = _job(workflow, "release")
    script = "\n".join(str(s.get("run", "")) for s in job["steps"])
    assert "cosign sign-blob" in script and "SHA256SUMS.sigstore.json" in script, (
        "SHA256SUMS is no longer signed"
    )
    released = job["steps"][-1]["with"]["files"]
    assert "SHA256SUMS.sigstore.json" in released, "the SHA256SUMS signature is not attached"


def test_image_is_signed_by_digest(workflow):
    """A tag can be moved after signing; a digest cannot."""
    job = _job(workflow, "docker-publish")
    sign = [s for s in job["steps"] if "cosign sign " in str(s.get("run", ""))]
    assert sign, "the container image is no longer signed"
    assert "@${DIGEST}" in sign[0]["run"] and "steps.push.outputs.digest" in str(sign[0].get("env")), (
        "the image must be signed by the digest build-push-action reported, not by tag"
    )
    push = [s for s in job["steps"] if s.get("id") == "push"][0]["with"]
    assert push.get("provenance") == "mode=max" and push.get("sbom") is True


def test_release_restores_no_cache(workflow):
    """A release builds only from what it compiled itself.

    A cache entry is written by whatever earlier run had the same key; restoring one into the job
    that produces shipped binaries makes them depend on that run.
    """
    for name, job in workflow["jobs"].items():
        assert not _uses(job, "Swatinem/rust-cache"), f"'{name}' restores a Rust cache"
        for step in _uses(job, "actions/setup-node"):
            assert step.get("with", {}).get("package-manager-cache") is False, (
                f"'{name}' runs setup-node with its npm cache on"
            )
        for step in _uses(job, "mlugg/setup-zig"):
            assert step.get("with", {}).get("use-cache") is False, f"'{name}' restores a zig cache"
