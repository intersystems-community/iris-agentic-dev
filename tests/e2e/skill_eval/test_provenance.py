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


# ---------------------------------------------------------------------------
# Which harness drove it — 120 T017, FR-012
# ---------------------------------------------------------------------------
#
# `opencode` drove spec 121's pilot and `prime-agent` is the candidate, so a run recorded
# without saying which one ran cannot be compared to next month's. The rule is the one
# `tool_surface` already follows: recorded, kept out of `COMPARABILITY_FIELDS`, and a
# difference annotates the Δ instead of suppressing it. `test_baseline.py` owns that half.


class _Driver:
    """Only the two attributes `AgentDriver` declares. Nothing here reads anything else."""

    def __init__(self, name, harness_version=None):
        self.name = name
        self.harness_version = harness_version


def a_provenance(**over):
    fields = {
        "run_id": "r1",
        "task_ids": ["PILOT-01"],
        "scoring_mode": "assertion",
        "scorer_model": None,
        "scorer_model_requested": None,
        "tool_surface": "1.4.2+fa0b694f8725",
        "runs": 1,
    }
    fields.update(over)
    return provenance.Provenance(**fields)


def test_provenance_records_the_driver_and_its_version():
    p = a_provenance(driver="opencode", harness_version="0.14.3")
    assert p.driver == "opencode"
    assert p.harness_version == "0.14.3"


def test_an_unrecorded_driver_is_a_stated_fact_not_a_blank():
    """The same shape as `tool_surface: "none"`. A run nobody attributed is a run nobody can
    compare, and that belongs in the file rather than absent from it."""
    assert a_provenance().driver == provenance.DRIVER_UNRECORDED
    assert a_provenance().harness_version is None


def test_the_driver_fields_survive_the_round_trip_into_the_baseline():
    p = a_provenance(driver="prime-agent", harness_version="0.4.1")
    restored = provenance.Provenance.from_dict(json.loads(json.dumps(p.to_dict())))
    assert restored == p
    assert json.loads(json.dumps(p.to_dict()))["driver"] == "prime-agent"


def test_an_entry_written_before_these_fields_existed_still_loads():
    """Every provenance block in the committed baseline predates FR-012. Reading one must not
    raise — it must come back saying the driver was never recorded, which is true."""
    old = {
        "run_id": "2026-09-12T040211Z",
        "task_ids": ["DBG-01"],
        "scoring_mode": "judge",
        "scorer_model": "claude-sonnet-4-6",
        "scorer_model_requested": "us.anthropic.claude-sonnet-4-6",
        "tool_surface": "1.4.1+fa0b694f8725",
        "runs": 3,
    }
    restored = provenance.Provenance.from_dict(old)
    assert restored.driver == provenance.DRIVER_UNRECORDED
    assert restored.harness_version is None


def test_driver_identity_reads_the_driver_object():
    assert provenance.driver_identity(_Driver("opencode", "0.14.3")) == {
        "driver": "opencode",
        "harness_version": "0.14.3",
    }


def test_a_driver_that_cannot_say_its_version_records_none_not_a_guess():
    identity = provenance.driver_identity(_Driver("prime-agent"))
    assert identity == {"driver": "prime-agent", "harness_version": None}


def test_no_driver_at_all_is_unrecorded_rather_than_an_exception():
    """`--dry-run` and the fixture-only paths build provenance with no session behind it."""
    assert provenance.driver_identity(None) == {
        "driver": provenance.DRIVER_UNRECORDED,
        "harness_version": None,
    }


def test_the_real_drivers_answer_the_two_attributes_this_reads():
    """Against the shipped drivers, not a stub: `driver_identity` reads `name` and
    `harness_version` off the `AgentDriver` protocol, and a rename there would otherwise
    silently record every run as unrecorded."""
    from tests.e2e.skill_eval.opencode_driver import OpencodeDriver
    from tests.e2e.skill_eval.prime_agent import PrimeAgentDriver

    assert provenance.driver_identity(OpencodeDriver(harness_version="0.14.3")) == {
        "driver": "opencode",
        "harness_version": "0.14.3",
    }
    assert provenance.driver_identity(PrimeAgentDriver(harness_version="0.4.1")) == {
        "driver": "prime-agent",
        "harness_version": "0.4.1",
    }


# --- what a published benchmark figure has to carry — 121 T038, FR-020 ----------------------------
#
# The skill-eval provenance answers "which scorer, which harness". A benchmark figure answers a
# harder question: could someone else re-measure this? That needs the container the tasks ran
# against, the corpus commit they were read from, and the item counts the interval was computed over.


@pytest.mark.requires_iris
def test_container_identity_names_the_image_and_its_digest():
    """A tag moves. `intersystemsdc/iris-community:2026.2` in six months is a different image, and a
    number measured against the old one cannot be compared to a number measured against the new one
    unless the digest is written down."""
    identity = provenance.container_identity("iris-dev-iris")
    if identity["image"] is None:
        # `requires_iris` is a label, not a skip: `ci.yml` and the nightly filter on it, but a plain
        # `pytest tests/e2e/skill_eval/` run has no filter. On a runner with no `iris-dev-iris` the
        # function answers Nones by design (see its docstring), and this test used to fail on that
        # for five nights running.
        pytest.skip("no iris-dev-iris container to read an image from")
    assert identity["container"] == "iris-dev-iris"
    assert identity["image"] == "intersystemsdc/iris-community:2026.2"
    assert identity["image_id"].startswith("sha256:")
    # RepoDigests is what someone else can pull; a locally built image has none, and then this is
    # None rather than the image id passed off as a pullable reference.
    assert identity["image_digest"] is None or identity["image_digest"].startswith(
        "intersystemsdc/iris-community@sha256:"
    )


def test_a_container_that_is_not_running_is_recorded_as_absent_not_guessed():
    identity = provenance.container_identity("no-such-container-121")
    assert identity["container"] == "no-such-container-121"
    assert identity["image"] is None
    assert identity["image_id"] is None
    assert identity["image_digest"] is None


def test_the_corpus_commit_is_the_commit_the_task_files_were_read_from():
    """`harness_commit` is HEAD. That is the same commit here, but the two are separate claims: the
    corpus can be pinned and vendored while the harness moves, and a figure re-measured later needs to
    know which corpus it was."""
    record = provenance.corpus_identity()
    assert record["corpus_commit"] is None or len(record["corpus_commit"]) == 40
    # Dirty means the files on disk are not the commit named, so the commit alone does not identify
    # the corpus. Stating it is the difference between reproducible and nearly reproducible.
    assert record["corpus_dirty"] in (True, False)
    assert record["corpus_task_count"] >= 50


def test_a_benchmark_provenance_carries_the_five_fr_020_facts():
    record = provenance.Provenance(
        run_id="ladder-1",
        task_ids=["CORPUS-01", "CORPUS-02"],
        scoring_mode="machine",
        scorer_model=None,
        scorer_model_requested=None,
        tool_surface="merged@abc1234",
        runs=1,
        agent_model="openai/gpt-4.1",
        container="iris-dev-iris",
        image="intersystemsdc/iris-community:2026.2",
        image_digest="intersystemsdc/iris-community@sha256:5ffbd9",
        image_id="sha256:f360aa",
        corpus_commit="0" * 40,
        corpus_dirty=False,
        item_counts={"tasks": 41, "arms": 3, "sessions": 123},
    ).to_dict()
    assert record["agent_model"] == "openai/gpt-4.1"
    assert record["image_digest"].endswith("5ffbd9")
    assert record["item_counts"]["sessions"] == 123
    assert record["corpus_dirty"] is False
    # And the scoring mode says no model graded anything, which is the non-negotiable this whole
    # corpus was built around.
    assert record["scoring_mode"] == "machine"


def test_an_old_provenance_block_still_loads_without_the_new_fields():
    """The committed baseline has entries written before any of this existed."""
    restored = provenance.Provenance.from_dict(
        {
            "run_id": "old",
            "task_ids": ["DBG-01"],
            "scoring_mode": "judge",
            "scorer_model": "m",
            "scorer_model_requested": "m",
            "tool_surface": "none",
            "runs": 3,
        }
    )
    assert restored.agent_model is None
    assert restored.image_digest is None
    assert restored.item_counts == {}
