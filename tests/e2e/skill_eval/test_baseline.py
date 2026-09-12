"""Unit tests for baseline — T005, extended for 118 T023 (schema 2).

The bug this file grew to cover: `save_baseline` built a fresh dict from the results it was
handed and wrote it over the file. A one-skill run therefore deleted the other eight entries,
and the nightly's `--update-baseline` leg did exactly that every time someone dispatched a
single skill. The baseline on disk today holds one entry for a nine-skill suite.
"""

import json
import os

import pytest

from tests.e2e.skill_eval import baseline as baseline_mod
from tests.e2e.skill_eval.baseline import (
    SCHEMA_VERSION,
    comparability,
    compute_diff,
    coverage_census,
    load_baseline,
    load_ungated,
    save_baseline,
    surface_change,
)
from tests.e2e.skill_eval.evaluator import SkillResult

_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
)
_TASKS_SKILLS_DIR = os.path.join(_REPO_ROOT, "tests", "e2e", "tasks", "skills")
_SHIPPED_BASELINE = os.path.join(
    _REPO_ROOT, "tests", "e2e", "results", "skill-baseline.json"
)


def provenance(task_ids=("DBG-01",), scorer_model="claude-sonnet-4-6", **over):
    prov = {
        "run_id": "2026-09-12T040211Z",
        "task_ids": sorted(task_ids),
        "scoring_mode": "judge",
        "scorer_model": scorer_model,
        "scorer_model_requested": "us.anthropic.claude-sonnet-4-6",
        "tool_surface": "1.4.1+fa0b694f8725",
        "runs": 3,
        "measured_at": "2026-09-12T04:11:07Z",
        "harness_commit": "7d82f7d",
    }
    prov.update(over)
    return prov


def arms(baseline_rate=0.33, skill_rate=0.58, total=12, unscored=0):
    def arm(rate):
        scored = total - unscored
        return {
            "items_total": total,
            "items_scored": scored,
            "items_unscored": unscored,
            "items_passed": round(rate * scored),
            "pass_rate": rate,
        }

    return {"baseline": arm(baseline_rate), "skill": arm(skill_rate)}


def make_result(skill, lift, fire_rate=1.0, prov=None, arm_data=None):
    return SkillResult(
        skill=skill,
        fire_rate=fire_rate,
        implicit_fire_rate=None,
        isolation_fire_rate=None,
        pass_rate_baseline=0.71,
        pass_rate_skill=0.71 + lift if lift else None,
        lift=lift,
        lift_delta=None,
        regression_flag=False,
        new_skill=False,
        no_task_coverage=lift is None,
        task_ids_used=["DBG-01"] if lift else [],
        provenance=prov if prov is not None else provenance(),
        arms=arm_data if arm_data is not None else arms(),
    )


@pytest.fixture
def tasks_dir(tmp_path, monkeypatch):
    """A stand-in for `tests/e2e/tasks/skills/`, so the orphan check has a tree to read."""
    root = tmp_path / "tasks-skills"
    for skill in (
        "objectscript-review",
        "objectscript-guardrails",
        "iris-connectivity",
    ):
        (root / skill).mkdir(parents=True)
        (root / skill / "eval.yaml").write_text(f"skill: {skill}\n")
    monkeypatch.setattr(baseline_mod, "_TASKS_SKILLS_DIR", str(root))
    return root


# ── diff, unchanged from T005 ────────────────────────────────────────────────


def test_load_baseline_missing_returns_empty(tmp_path):
    result = load_baseline(str(tmp_path / "nonexistent.json"))
    assert result == {}


def test_save_and_load_roundtrip(tmp_path, tasks_dir):
    path = str(tmp_path / "baseline.json")
    results = [
        make_result("objectscript-review", 0.29),
        make_result("objectscript-guardrails", 0.14),
    ]
    save_baseline(results, path)
    loaded = load_baseline(path)
    assert loaded["objectscript-review"]["lift"] == pytest.approx(0.29)
    assert loaded["objectscript-guardrails"]["lift"] == pytest.approx(0.14)


def test_compute_diff_detects_regression():
    old = {"objectscript-review": {"lift": 0.29}}
    new = [make_result("objectscript-review", 0.10)]
    diff = compute_diff(old, new)
    assert len(diff) == 1
    assert diff[0]["skill"] == "objectscript-review"
    assert diff[0]["old_lift"] == pytest.approx(0.29)
    assert diff[0]["new_lift"] == pytest.approx(0.10)
    assert diff[0]["delta"] == pytest.approx(-0.19, abs=0.01)


def test_compute_diff_marks_new_skill():
    old = {}
    new = [make_result("objectscript-review", 0.29)]
    diff = compute_diff(old, new)
    assert diff[0]["new_skill"] is True


def test_compute_diff_omits_no_change():
    old = {"objectscript-review": {"lift": 0.29}}
    new = [make_result("objectscript-review", 0.29)]
    diff = compute_diff(old, new)
    assert len(diff) == 0


def test_compute_diff_sorted_by_abs_delta():
    old = {"a": {"lift": 0.5}, "b": {"lift": 0.5}}
    new = [make_result("a", 0.1), make_result("b", 0.45)]
    diff = compute_diff(old, new)
    assert diff[0]["skill"] == "a"  # bigger delta first


# ── T023: merge-by-skill writes ──────────────────────────────────────────────


def test_baseline_merge_preserves_other_skills(tmp_path, tasks_dir):
    """The detector for the `overwrite-on-partial-write` class in the constitution amendment.

    A one-skill write must leave every other entry exactly as it found it. The old writer
    rebuilt the file from the results it was handed, so `--skill iris-connectivity
    --update-baseline` published a baseline in which the other eight skills had never been
    measured — and the next night's run reported all eight as `new_skill`.
    """
    path = str(tmp_path / "baseline.json")
    save_baseline(
        [
            make_result("objectscript-review", 0.29),
            make_result("objectscript-guardrails", 0.14),
            make_result("iris-connectivity", 0.67),
        ],
        path,
    )
    before = json.loads(open(path).read())

    save_baseline([make_result("iris-connectivity", 0.40)], path)
    after = json.loads(open(path).read())

    assert set(after["skills"]) == set(before["skills"])
    for untouched in ("objectscript-review", "objectscript-guardrails"):
        assert after["skills"][untouched] == before["skills"][untouched], (
            f"{untouched} changed on a write that did not measure it"
        )
    assert after["skills"]["iris-connectivity"]["lift"] == pytest.approx(0.40)


def test_a_written_skill_is_no_longer_declared_ungated(tmp_path, tasks_dir):
    """A skill cannot be gated and excused at once, so a measurement retires its excuse."""
    path = str(tmp_path / "baseline.json")
    save_baseline([], path, ungated={"iris-connectivity": "awaiting rebaseline"})
    assert load_ungated(path) == {"iris-connectivity": "awaiting rebaseline"}

    save_baseline([make_result("iris-connectivity", 0.67)], path)
    assert load_ungated(path) == {}
    assert "iris-connectivity" in load_baseline(path)


def test_the_written_file_declares_its_schema(tmp_path, tasks_dir):
    path = str(tmp_path / "baseline.json")
    save_baseline([make_result("iris-connectivity", 0.67)], path)
    data = json.loads(open(path).read())
    assert data["schema"] == SCHEMA_VERSION
    assert set(data) == {"schema", "skills", "ungated_skills"}
    assert open(path).read().endswith("\n"), (
        "no trailing newline — every commit re-diffs it"
    )


def test_an_entry_records_what_it_was_measured_under(tmp_path, tasks_dir):
    path = str(tmp_path / "baseline.json")
    save_baseline([make_result("iris-connectivity", 0.67)], path)
    entry = load_baseline(path)["iris-connectivity"]
    assert entry["provenance"]["scorer_model"] == "claude-sonnet-4-6"
    assert entry["provenance"]["task_ids"] == ["DBG-01"]
    assert entry["items"]["baseline"]["items_scored"] == 12


# ── T023: v1 migration ───────────────────────────────────────────────────────


def _v1_file(tmp_path):
    path = tmp_path / "baseline.json"
    path.write_text(
        json.dumps(
            {
                "iris-vector-ai": {
                    "fire_rate": 1.0,
                    "lift": 0.25,
                    "pass_rate_baseline": 0.33,
                    "pass_rate_skill": 0.58,
                }
            },
            indent=2,
        )
    )
    return path


def test_a_v1_file_migrates_in_memory(tmp_path):
    """The file on disk today is v1. Reading it must not crash and must not rewrite it."""
    path = _v1_file(tmp_path)
    original = path.read_text()

    loaded = load_baseline(str(path))
    assert loaded["iris-vector-ai"]["lift"] == pytest.approx(0.25)
    assert loaded["iris-vector-ai"]["provenance"] is None
    assert loaded["iris-vector-ai"]["items"] is None
    assert path.read_text() == original, "a read rewrote the file"


def test_a_migrated_entry_is_not_comparable(tmp_path):
    """No provenance means nobody knows what graded it. That is not a reference measurement.

    The single entry on disk was measured by a scorer that returned zeros for everything it
    could not reach, so comparing a new number against it would report a lift regression that
    is really a credential that has since been fixed.
    """
    entry = load_baseline(str(_v1_file(tmp_path)))["iris-vector-ai"]
    assert comparability(entry, provenance()) == "no provenance recorded"


def test_a_v1_file_can_be_migrated_by_a_write(tmp_path, tasks_dir):
    """Writing one skill onto a v1 file keeps the old entries, in migrated shape."""
    path = _v1_file(tmp_path)
    save_baseline([make_result("objectscript-review", 0.29)], str(path))
    data = json.loads(path.read_text())
    assert data["schema"] == SCHEMA_VERSION
    assert data["skills"]["iris-vector-ai"]["lift"] == pytest.approx(0.25)
    assert data["skills"]["iris-vector-ai"]["provenance"] is None
    assert data["skills"]["objectscript-review"]["provenance"] is not None


def test_a_future_schema_is_an_error_not_a_downgrade(tmp_path):
    """An older harness must not quietly rewrite a file it does not understand."""
    path = tmp_path / "baseline.json"
    path.write_text(json.dumps({"schema": 3, "skills": {}}))
    with pytest.raises(ValueError) as exc:
        load_baseline(str(path))
    assert "3" in str(exc.value) and str(path) in str(exc.value)


def test_an_unparseable_file_reads_as_empty(tmp_path):
    path = tmp_path / "baseline.json"
    path.write_text("{ this is not json")
    assert load_baseline(str(path)) == {}


# ── T023: orphan entries ─────────────────────────────────────────────────────


def test_an_orphan_entry_is_warned_about_and_kept(tmp_path, tasks_dir, capsys):
    """A deleted eval config may come back; a write is not the place to decide it won't.

    Silence is the failure mode: a file that keeps entries for skills nobody can run reports
    coverage it does not have, and the census would count a skill with no tasks as gated.
    """
    path = str(tmp_path / "baseline.json")
    save_baseline([make_result("skill-that-was-deleted", 0.29)], path)
    capsys.readouterr()

    save_baseline([make_result("objectscript-review", 0.29)], path)
    out = capsys.readouterr().out
    assert "skill-that-was-deleted" in out
    assert "skill-that-was-deleted" in load_baseline(path), "the orphan was dropped"


# ── T023 / SC-004: the coverage census ───────────────────────────────────────


def test_every_eval_config_is_gated_or_declared():
    """SC-004, against the file as shipped. Nine skills have eval configs; each is accounted for.

    Either it has a baseline entry, or `ungated_skills` says in writing why it does not. The
    third state — a skill with tasks, no entry, and no explanation — is how six of nine skills
    came to run every night against nothing.
    """
    problems = coverage_census(_TASKS_SKILLS_DIR, _SHIPPED_BASELINE)
    assert problems == [], "\n".join(problems)


def test_the_census_reports_a_skill_with_neither(tmp_path, tasks_dir):
    path = str(tmp_path / "baseline.json")
    save_baseline([make_result("objectscript-review", 0.29)], path)
    problems = coverage_census(str(tasks_dir), path)
    assert any("iris-connectivity" in p for p in problems)
    assert any("objectscript-guardrails" in p for p in problems)
    assert not any("objectscript-review" in p for p in problems)


def test_the_census_accepts_a_declared_reason(tmp_path, tasks_dir):
    path = str(tmp_path / "baseline.json")
    save_baseline(
        [make_result("objectscript-review", 0.29)],
        path,
        ungated={
            "objectscript-guardrails": "no post-repair measurement yet",
            "iris-connectivity": "no post-repair measurement yet",
        },
    )
    assert coverage_census(str(tasks_dir), path) == []


def test_the_census_rejects_a_name_in_both_maps(tmp_path, tasks_dir):
    """Gated and excused at once is not a state; the two maps together are the census."""
    path = tmp_path / "baseline.json"
    path.write_text(
        json.dumps(
            {
                "schema": 2,
                "skills": {"objectscript-review": {"lift": 0.29}},
                "ungated_skills": {
                    "objectscript-review": "still catching up",
                    "objectscript-guardrails": "no measurement yet",
                    "iris-connectivity": "no measurement yet",
                },
            }
        )
    )
    problems = coverage_census(str(tasks_dir), str(path))
    assert any("objectscript-review" in p and "both" in p for p in problems), problems


# ── T023: comparability ──────────────────────────────────────────────────────


def entry(prov=None, lift=0.25):
    return {
        "fire_rate": 1.0,
        "lift": lift,
        "pass_rate_baseline": 0.33,
        "pass_rate_skill": 0.58,
        "items": arms(),
        "provenance": provenance() if prov is None else prov,
    }


def test_matching_provenance_is_comparable():
    assert comparability(entry(), provenance()) is None


@pytest.mark.parametrize(
    "field,value",
    [
        ("task_ids", ["DBG-01", "DBG-02"]),
        ("scoring_mode", "assertion"),
        ("scorer_model", "claude-haiku-4-5"),
    ],
)
def test_each_comparability_field_refuses_and_names_itself(field, value):
    """A Δ across a changed task set, scale, or grader is not a measurement of anything."""
    reason = comparability(entry(), provenance(**{field: value}))
    assert reason and field in reason


def test_task_ids_compare_as_a_set_not_an_order():
    reason = comparability(
        entry(prov=provenance(task_ids=["DBG-01", "DBG-02"])),
        provenance(task_ids=["DBG-02", "DBG-01"]),
    )
    assert reason is None


def test_a_run_with_no_provenance_cannot_claim_comparability():
    reason = comparability(entry(), None)
    assert reason and "provenance" in reason


def test_a_differing_tool_surface_does_not_block_the_delta():
    """Suppressing here would silence the gate on exactly the releases it exists to check."""
    prov = provenance(tool_surface="1.5.0+3ba0e1d44c19")
    assert comparability(entry(), prov) is None
    note = surface_change(entry(), prov)
    assert note and "1.4.1+fa0b694f8725" in note and "1.5.0+3ba0e1d44c19" in note


def test_an_unchanged_tool_surface_has_nothing_to_annotate():
    assert surface_change(entry(), provenance()) is None
