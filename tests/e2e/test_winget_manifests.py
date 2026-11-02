"""Spec 152: the winget manifests that packaging/winget/winget_manifests.py writes.

winget installs whatever the manifest names and checks only the hash the manifest gives, so a
wrong URL, version or hash ships to every Windows user who runs `winget upgrade`. The manifests
are generated, never edited by hand, and these tests pin what the generator writes: the official
1.10.0 schemas, the identifier Tom chose (D1), and the release asset the URL points at.
"""

import json
import os
import re
import subprocess
import sys

import pytest

yaml = pytest.importorskip("yaml")
jsonschema = pytest.importorskip("jsonschema")

_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
_GENERATOR = os.path.join(_REPO_ROOT, "packaging", "winget", "winget_manifests.py")
_SCHEMAS = os.path.join(_REPO_ROOT, "packaging", "winget", "schemas", "1.10.0")
_SUBMIT = os.path.join(_REPO_ROOT, "scripts", "winget-submit.sh")

IDENTIFIER = "InterSystemsCommunity.IrisAgenticDev"
VERSION = "1.7.1"
SHA = "b38f4b3e4cff5f56f7f4fe32e94de3a9f8cda9895e41672038f64ecae9b756de"
DATE = "2026-11-01"
ASSET_URL = (
    "https://github.com/intersystems-community/iris-agentic-dev/releases/download/"
    f"v{VERSION}/iris-agentic-dev-windows-x86_64.exe"
)


def _run(*args, check=True):
    return subprocess.run(
        [sys.executable, "-I", _GENERATOR, *args],
        capture_output=True,
        text=True,
        check=check,
    )


@pytest.fixture()
def manifests(tmp_path):
    _run(
        "--version",
        VERSION,
        "--sha256",
        SHA,
        "--release-date",
        DATE,
        "--out",
        str(tmp_path),
    )
    pkg_dir = (
        tmp_path
        / "manifests"
        / "i"
        / "InterSystemsCommunity"
        / "IrisAgenticDev"
        / VERSION
    )
    files = {p.name: p for p in pkg_dir.iterdir()}
    assert sorted(files) == sorted(
        [
            f"{IDENTIFIER}.yaml",
            f"{IDENTIFIER}.locale.en-US.yaml",
            f"{IDENTIFIER}.installer.yaml",
        ]
    ), f"unexpected manifest files: {sorted(files)}"
    return {
        "dir": pkg_dir,
        "version": yaml.safe_load(files[f"{IDENTIFIER}.yaml"].read_text()),
        "locale": yaml.safe_load(files[f"{IDENTIFIER}.locale.en-US.yaml"].read_text()),
        "installer": yaml.safe_load(files[f"{IDENTIFIER}.installer.yaml"].read_text()),
        "raw": {name: p.read_text() for name, p in files.items()},
    }


def _schema(kind):
    with open(os.path.join(_SCHEMAS, f"manifest.{kind}.1.10.0.json")) as f:
        return json.load(f)


# ── US3: schema-valid, and the fields winget acts on ────────────────────────


@pytest.mark.parametrize(
    "key,kind",
    [("version", "version"), ("locale", "defaultLocale"), ("installer", "installer")],
)
def test_each_manifest_validates_against_the_official_schema(manifests, key, kind):
    jsonschema.validate(manifests[key], _schema(kind))


def test_identifier_and_version_agree_across_the_three_files(manifests):
    for key in ("version", "locale", "installer"):
        assert manifests[key]["PackageIdentifier"] == IDENTIFIER
        assert manifests[key]["PackageVersion"] == VERSION
        assert manifests[key]["ManifestVersion"] == "1.10.0"
    assert manifests["version"]["DefaultLocale"] == "en-US"
    assert manifests["locale"]["PackageLocale"] == "en-US"


def test_version_is_quoted_so_yaml_never_reads_it_as_a_number(manifests):
    """`PackageVersion: 1.10` unquoted is the float 1.1 to some readers."""
    for text in manifests["raw"].values():
        assert f"PackageVersion: '{VERSION}'" in text


def test_installer_is_the_portable_release_asset(manifests):
    inst = manifests["installer"]
    assert inst["InstallerType"] == "portable"
    assert inst["Commands"] == ["iris-agentic-dev"]
    assert inst["ReleaseDate"] == DATE
    assert len(inst["Installers"]) == 1
    only = inst["Installers"][0]
    assert only["Architecture"] == "x64"
    assert only["InstallerUrl"] == ASSET_URL
    assert only["InstallerSha256"] == SHA.upper()


def test_license_matches_cargo_toml(manifests):
    with open(os.path.join(_REPO_ROOT, "Cargo.toml")) as f:
        cargo_license = re.search(r'^license\s*=\s*"([^"]+)"', f.read(), re.M).group(1)
    assert manifests["locale"]["License"] == cargo_license


def test_locale_urls_point_at_this_repository(manifests):
    loc = manifests["locale"]
    repo = "https://github.com/intersystems-community/iris-agentic-dev"
    assert loc["PackageUrl"] == repo
    assert loc["LicenseUrl"].startswith(repo + "/blob/")
    assert loc["ReleaseNotesUrl"] == f"{repo}/releases/tag/v{VERSION}"
    assert loc["Moniker"] == "iris-agentic-dev"


def test_every_file_names_its_schema_for_editors(manifests):
    for name, text in manifests["raw"].items():
        assert (
            "# yaml-language-server: $schema=https://aka.ms/winget-manifest." in text
        ), name


# ── US3.3: refuse input that would publish a broken package ─────────────────


@pytest.mark.parametrize("bad", ["1.7", "v1.7.1", "1.7.1-rc1", "1.7.1\n", ""])
def test_refuses_a_version_that_is_not_x_y_z(tmp_path, bad):
    r = _run(
        "--version",
        bad,
        "--sha256",
        SHA,
        "--release-date",
        DATE,
        "--out",
        str(tmp_path),
        check=False,
    )
    assert r.returncode != 0
    assert "version" in r.stderr.lower()
    assert not (tmp_path / "manifests").exists()


@pytest.mark.parametrize("bad", ["", "abc", SHA[:-1], SHA + "0", "z" * 64])
def test_refuses_a_hash_that_is_not_64_hex(tmp_path, bad):
    r = _run(
        "--version",
        VERSION,
        "--sha256",
        bad,
        "--release-date",
        DATE,
        "--out",
        str(tmp_path),
        check=False,
    )
    assert r.returncode != 0
    assert "sha256" in r.stderr.lower()
    assert not (tmp_path / "manifests").exists()


def test_refuses_a_date_that_is_not_iso(tmp_path):
    r = _run(
        "--version",
        VERSION,
        "--sha256",
        SHA,
        "--release-date",
        "11/01/2026",
        "--out",
        str(tmp_path),
        check=False,
    )
    assert r.returncode != 0
    assert "date" in r.stderr.lower()


# ── US2: the submit script never writes anywhere without --submit ───────────


def _script():
    with open(_SUBMIT) as f:
        return f.read()


def test_submit_script_takes_the_hash_from_the_release_not_a_constant():
    text = _script()
    assert "SHA256SUMS" in text
    assert "winget_manifests.py" in text
    # The downloaded asset is checked against SHA256SUMS before anything is generated.
    assert re.search(r"shasum -a 256 -c|sha256sum -c", text), (
        "the asset is never verified"
    )


def test_submit_script_pushes_and_opens_a_pr_only_under_submit():
    """FR-006 and US2.3: nothing leaves the machine unless --submit was given."""
    text = _script()
    lines = [
        ln
        for ln in text.splitlines()
        if re.search(r"\bgit push\b|\bgh pr create\b|\bgh repo fork\b", ln)
    ]
    assert lines, "the script never submits at all"
    body = text.split('if [ "$SUBMIT" = 1 ]', 1)
    assert len(body) == 2, 'submission is not behind an `if [ "$SUBMIT" = 1 ]` block'
    before = body[0]
    for ln in lines:
        assert ln not in before, f"runs before the --submit check: {ln.strip()}"


def test_submit_script_reads_no_token_variable():
    """D2: no winget token exists; the script uses the gh login on the maintainer's machine."""
    text = _script()
    assert not re.search(r"WINGET_TOKEN|GITHUB_TOKEN|GH_TOKEN=|_PAT\b", text)


def test_submit_script_is_strict_bash():
    assert _script().splitlines()[1].strip() == "set -euo pipefail"


# ── FR-004: CI installs the package with winget on Windows ──────────────────


def test_ci_has_a_windows_job_that_installs_from_the_manifests():
    with open(os.path.join(_REPO_ROOT, ".github", "workflows", "ci.yml")) as f:
        ci = yaml.safe_load(f)
    job = ci["jobs"].get("winget-install")
    assert job, "ci.yml has no winget-install job"
    assert str(job["runs-on"]).startswith("windows")
    script = "\n".join(str(s.get("run", "")) for s in job["steps"])
    assert "winget_manifests.py" in script
    assert "winget validate" in script
    assert "winget install --manifest" in script
    assert "--version" in script
    assert "secrets." not in json.dumps(job), "the install test needs no secret"
