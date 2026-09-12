"""Unit tests for provenance and binary resolution — 118 T004.

Written before `provenance.py`. The reason this module exists at all: `isolated_env.py`
hard-coded `/opt/homebrew/bin/iris-agentic-dev`, a path that does not exist on a GitHub
runner, so every nightly session for a month started with no iad tools and the harness
reported the resulting zeros as skill measurements. Resolution has to be a function with
tests, not a string literal.

The stub binary here is a shell script, not a mock: the thing under test is "does this
shell out correctly and parse what comes back", and a mocked `subprocess` would pass
against a wrong command line.
"""

import json
import stat

import pytest

from tests.e2e.skill_eval import provenance


def _stub_binary(path, version="1.4.1", tools=None, list_exit=0):
    """A script answering `--version` and `tool --list --json` the way iad does."""
    tools = [{"name": "iris_query", "summary": "Run SQL."}] if tools is None else tools
    payload = json.dumps({"count": len(tools), "tools": tools})
    path.write_text(
        "#!/bin/sh\n"
        'if [ "$1" = "--version" ]; then\n'
        f'  echo "iris-agentic-dev {version}"\n'
        "  exit 0\n"
        "fi\n"
        'if [ "$1" = "tool" ]; then\n'
        f"  cat <<'EOF'\n{payload}\nEOF\n"
        f"  exit {list_exit}\n"
        "fi\n"
        "exit 64\n"
    )
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return str(path)


# ---------------------------------------------------------------------------
# Binary resolution
# ---------------------------------------------------------------------------


def test_iad_binary_wins(tmp_path, monkeypatch):
    explicit = _stub_binary(tmp_path / "explicit")
    on_path = tmp_path / "pathdir"
    on_path.mkdir()
    _stub_binary(on_path / "iris-agentic-dev")

    monkeypatch.setenv("IAD_BINARY", explicit)
    monkeypatch.setenv("PATH", str(on_path))
    assert provenance.resolve_binary() == explicit


def test_path_is_second(tmp_path, monkeypatch):
    on_path = tmp_path / "pathdir"
    on_path.mkdir()
    found = _stub_binary(on_path / "iris-agentic-dev")

    monkeypatch.delenv("IAD_BINARY", raising=False)
    monkeypatch.setenv("PATH", str(on_path))
    monkeypatch.setattr(provenance, "HOMEBREW_FALLBACK", str(tmp_path / "never"))
    assert provenance.resolve_binary() == found


def test_homebrew_is_last(tmp_path, monkeypatch):
    fallback = _stub_binary(tmp_path / "brewed")

    monkeypatch.delenv("IAD_BINARY", raising=False)
    monkeypatch.setenv("PATH", str(tmp_path / "empty"))
    monkeypatch.setattr(provenance, "HOMEBREW_FALLBACK", fallback)
    assert provenance.resolve_binary() == fallback


def test_no_binary_resolves_to_none(tmp_path, monkeypatch):
    monkeypatch.delenv("IAD_BINARY", raising=False)
    monkeypatch.setenv("PATH", str(tmp_path / "empty"))
    monkeypatch.setattr(provenance, "HOMEBREW_FALLBACK", str(tmp_path / "never"))
    assert provenance.resolve_binary() is None


def test_a_non_executable_iad_binary_does_not_resolve(tmp_path, monkeypatch):
    """An `IAD_BINARY` pointing at nothing is the runner's failure verbatim.

    It must not resolve, and it must not be silently replaced by a different binary either:
    a run that measured a Homebrew build while told to measure the release artifact is a
    measurement of the wrong thing.
    """
    dud = tmp_path / "not-executable"
    dud.write_text("#!/bin/sh\n")
    on_path = tmp_path / "pathdir"
    on_path.mkdir()
    _stub_binary(on_path / "iris-agentic-dev")

    monkeypatch.setenv("IAD_BINARY", str(dud))
    monkeypatch.setenv("PATH", str(on_path))
    assert provenance.resolve_binary() is None


def test_candidates_report_every_place_searched(tmp_path, monkeypatch):
    """The preflight has to name all three places, so resolution has to report all three."""
    monkeypatch.delenv("IAD_BINARY", raising=False)
    monkeypatch.setenv("PATH", str(tmp_path / "empty"))
    monkeypatch.setattr(provenance, "HOMEBREW_FALLBACK", str(tmp_path / "never"))

    sources = [c.source for c in provenance.binary_candidates()]
    assert sources == ["IAD_BINARY", "PATH", "fallback"]
    assert not any(c.usable for c in provenance.binary_candidates())


# ---------------------------------------------------------------------------
# Tool surface
# ---------------------------------------------------------------------------

TOOLS = [
    {"name": "iris_query", "summary": "Run SQL."},
    {"name": "iris_execute", "summary": "Run ObjectScript."},
    {"name": "skill", "summary": "Read a skill."},
]


def test_tool_surface_format(tmp_path):
    surface = provenance.tool_surface(_stub_binary(tmp_path / "iad", tools=TOOLS))
    version, _, digest = surface.partition("+")
    assert version == "1.4.1"
    assert len(digest) == 12, surface
    assert all(c in "0123456789abcdef" for c in digest), surface


def test_surface_hash_ignores_ordering():
    """Two runs must not look like two tool surfaces because a list came back shuffled."""
    assert provenance.surface_hash(TOOLS) == provenance.surface_hash(
        list(reversed(TOOLS))
    )


def test_surface_hash_changes_when_a_tool_name_changes():
    renamed = [
        dict(t, name="iris_sql") if t["name"] == "iris_query" else t for t in TOOLS
    ]
    assert provenance.surface_hash(TOOLS) != provenance.surface_hash(renamed)


def test_surface_hash_changes_when_a_description_changes():
    """Descriptions are the thing GEPA edits, so a summary change is a surface change.

    Nothing gates on this — a differing tool surface annotates a comparison, it never
    suppresses one — but a lift that moved because a description was rewritten and a lift
    that moved because the skill changed should not be indistinguishable in the record.
    """
    reworded = [
        dict(t, summary="Run a SQL query.") if t["name"] == "iris_query" else t
        for t in TOOLS
    ]
    assert provenance.surface_hash(TOOLS) != provenance.surface_hash(reworded)


def test_surface_hash_of_nothing_is_not_a_hash():
    assert provenance.surface_hash([]) is None


def test_tool_surface_is_none_string_without_a_binary():
    assert provenance.tool_surface(None) == "none"


def test_tool_surface_is_none_string_when_the_binary_fails(tmp_path):
    """A binary that cannot list its tools has no surface. It does not have a blank one."""
    broken = _stub_binary(tmp_path / "iad", tools=TOOLS, list_exit=1)
    assert provenance.tool_surface(broken) == "none"


def test_tool_surface_needs_no_iris(tmp_path, monkeypatch):
    """`tool --list` reads the router; a run with no container still records its surface."""
    monkeypatch.setenv("IRIS_HOST", "203.0.113.1")  # RFC 5737, guaranteed unroutable
    monkeypatch.setenv("IRIS_WEB_PORT", "1")
    surface = provenance.tool_surface(_stub_binary(tmp_path / "iad", tools=TOOLS))
    assert surface != "none"


# ---------------------------------------------------------------------------
# The dataclass
# ---------------------------------------------------------------------------


def test_provenance_records_the_task_set_not_its_size():
    """R8 comparability is per task id. A count would call two different corpora equal."""
    p = provenance.Provenance(
        run_id="r1",
        task_ids=["IRIS-B", "IRIS-A"],
        scoring_mode="judge",
        scorer_model="claude-sonnet-4-6",
        scorer_model_requested="us.anthropic.claude-sonnet-4-6",
        tool_surface="1.4.1+abc123abc123",
        runs=3,
    )
    assert p.task_ids == ["IRIS-A", "IRIS-B"], "task ids are sorted on construction"
    assert p.measured_at.endswith("Z")


def test_provenance_round_trips_through_json():
    """It is stored in the baseline file, so a dict is the shape that has to survive."""
    p = provenance.Provenance(
        run_id="r1",
        task_ids=["IRIS-A"],
        scoring_mode="judge",
        scorer_model="claude-sonnet-4-6",
        scorer_model_requested="us.anthropic.claude-sonnet-4-6",
        tool_surface="none",
        runs=1,
    )
    restored = provenance.Provenance.from_dict(json.loads(json.dumps(p.to_dict())))
    assert restored == p


def test_harness_commit_is_resolved_or_none():
    """Best-effort: a tarball checkout has no git, and that is not a failure."""
    commit = provenance.harness_commit()
    assert commit is None or (4 <= len(commit) <= 40 and commit.isalnum())


@pytest.mark.parametrize("mode", ["judge", "assertion", "pattern", "mixed"])
def test_scoring_modes_accepted(mode):
    p = provenance.Provenance(
        run_id="r",
        task_ids=[],
        scoring_mode=mode,
        scorer_model=None,
        scorer_model_requested=None,
        tool_surface="none",
        runs=1,
    )
    assert p.scoring_mode == mode
