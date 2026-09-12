"""Baseline read/write/diff for skill regression detection — T006, schema 2 in 118 T027.

The baseline is the one durable artifact in the harness: committed, read by every run, and
until now silently truncated by every run. `save_baseline` used to build a fresh dict from the
results it was handed and write it over the file, so a one-skill `--update-baseline` deleted the
other eight entries. The file on disk holds one entry for a nine-skill suite because of that.

Schema 2 adds two things the old four floats could not carry: `items`, so a rate has a visible
denominator, and `provenance`, so a later run can say whether its number is comparable to this
one rather than assuming it. Writes are merge-by-skill. See contracts/baseline-v2.md.
"""

import json
import os
from typing import TYPE_CHECKING, Optional

if TYPE_CHECKING:
    from tests.e2e.skill_eval.evaluator import SkillResult

SCHEMA_VERSION = 2

# The three fields that have to match before a Δ means anything. `tool_surface` is deliberately
# not among them: suppressing the comparison when the binary changed would silence the gate on
# exactly the releases it exists to check.
COMPARABILITY_FIELDS = ("task_ids", "scoring_mode", "scorer_model")

_TASKS_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)


def _empty() -> dict:
    return {"schema": SCHEMA_VERSION, "skills": {}, "ungated_skills": {}}


def _migrate_v1(raw: dict) -> dict:
    """A v1 file is skill names at the top level. Migrated in memory, never rewritten on read.

    The migrated entries get `provenance: None`, which makes every one of them
    `not_comparable` — not "unknown, try anyway". The numbers in the file on disk were graded
    by a scorer that returned zeros for everything it could not reach.
    """
    skills = {}
    for name, entry in raw.items():
        if not isinstance(entry, dict):
            continue
        skills[name] = {**entry, "items": entry.get("items"), "provenance": None}
    return {"schema": SCHEMA_VERSION, "skills": skills, "ungated_skills": {}}


def load_file(path: str) -> dict:
    """Read the whole envelope, migrating a v1 file in memory. Absent → an empty v2 envelope.

    Raises `ValueError` on a schema this harness does not know. An older harness quietly
    rewriting a newer file is how a format change loses a night's provenance.
    """
    if not os.path.exists(path):
        return _empty()
    try:
        with open(path) as f:
            raw = json.load(f)
    except (OSError, json.JSONDecodeError):
        return _empty()
    if not isinstance(raw, dict) or not raw:
        return _empty()
    if "schema" not in raw:
        return _migrate_v1(raw)
    if raw["schema"] != SCHEMA_VERSION:
        raise ValueError(
            f"{path}: schema {raw['schema']!r}, but this harness writes schema "
            f"{SCHEMA_VERSION}. Refusing to read it rather than downgrade the file."
        )
    return {
        "schema": SCHEMA_VERSION,
        "skills": raw.get("skills") or {},
        "ungated_skills": raw.get("ungated_skills") or {},
    }


def load_baseline(path: str) -> dict:
    """The stored entries, keyed by skill. `{}` when the file is absent — every skill is new."""
    if not os.path.exists(path):
        return {}
    return load_file(path)["skills"]


def load_ungated(path: str) -> dict:
    """Skill → the reason it has an eval config and deliberately no entry (FR-011)."""
    if not os.path.exists(path):
        return {}
    return load_file(path)["ungated_skills"]


def entry_from_result(result: "SkillResult") -> dict:
    """One skill's stored reference measurement, including what it was measured under."""
    return {
        "fire_rate": result.fire_rate,
        "items": getattr(result, "arms", None),
        "lift": result.lift,
        "pass_rate_baseline": result.pass_rate_baseline,
        "pass_rate_skill": result.pass_rate_skill,
        "provenance": getattr(result, "provenance", None),
    }


def _configured_skills(tasks_skills_dir: Optional[str] = None) -> set:
    root = tasks_skills_dir or _TASKS_SKILLS_DIR
    if not os.path.isdir(root):
        return set()
    return {
        name
        for name in os.listdir(root)
        if os.path.exists(os.path.join(root, name, "eval.yaml"))
    }


def save_baseline(
    results: "list[SkillResult]",
    path: str,
    ungated: Optional[dict] = None,
) -> None:
    """Merge this run's measurements into the file. Entries not in `results` are untouched.

    Write steps are contracts/baseline-v2.md § Write semantics. Step 4 — the orphan warning —
    is here because the opposite of the truncation bug is also a bug: a file that quietly keeps
    entries for skills nobody can run reports coverage it does not have.
    """
    data = load_file(path)
    skills = data["skills"]
    ungated_skills = dict(data["ungated_skills"])

    for result in results:
        if result.lift is None:
            # Nothing was measured, so there is nothing to be a reference. A no-coverage skill
            # belongs in `ungated_skills` with a reason, not in `skills` with nulls.
            continue
        skills[result.skill] = entry_from_result(result)
        # Measured, so it is gated. A name cannot be in both maps.
        ungated_skills.pop(result.skill, None)

    for name, reason in (ungated or {}).items():
        if name not in skills:
            ungated_skills[name] = reason

    configured = _configured_skills()
    orphans = sorted(name for name in skills if configured and name not in configured)
    if orphans:
        print(
            f"WARNING: baseline entries with no eval.yaml under {_TASKS_SKILLS_DIR}: "
            f"{', '.join(orphans)}. Kept — a deleted config may come back — but they are "
            "not measurable and do not count as coverage."
        )

    data = {
        "schema": SCHEMA_VERSION,
        "skills": skills,
        "ungated_skills": ungated_skills,
    }
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w") as f:
        json.dump(data, f, indent=2, sort_keys=True)
        f.write("\n")


def coverage_census(tasks_skills_dir: str, path: str) -> list[str]:
    """SC-004: every skill with an eval config is either gated or excused, in writing.

    Returns one line per problem, empty when the file accounts for every configured skill. The
    third state — tasks, no entry, no explanation — is how six of nine skills came to run every
    night against nothing and report it as success.
    """
    envelope = load_file(path)
    skills = envelope["skills"]
    ungated = envelope["ungated_skills"]
    problems = []
    for name in sorted(_configured_skills(tasks_skills_dir)):
        gated = name in skills
        excused = name in ungated
        if gated and excused:
            problems.append(
                f"{name}: in both skills and ungated_skills — a skill cannot be gated and "
                "excused at once"
            )
        elif not gated and not excused:
            problems.append(
                f"{name}: has an eval.yaml but no baseline entry and no ungated_skills reason"
            )
    return problems


def comparability(entry: dict, provenance: Optional[dict]) -> Optional[str]:
    """`None` when a Δ against this entry is meaningful, else the reason it is not.

    Named fields, not a boolean: "not comparable" with no reason sends someone to re-run a
    skill when what changed was the grader.
    """
    old = (entry or {}).get("provenance")
    if not old:
        return "no provenance recorded"
    if not provenance:
        return "no provenance recorded for this run"
    for field in COMPARABILITY_FIELDS:
        was, now = old.get(field), provenance.get(field)
        if field == "task_ids":
            was, now = sorted(was or []), sorted(now or [])
        if was != now:
            return f"{field} differs: {was!r} → {now!r}"
    return None


def surface_change(entry: dict, provenance: Optional[dict]) -> Optional[str]:
    """The tool-surface annotation that prints beside a Δ, or `None` when it did not change."""
    old = (entry or {}).get("provenance") or {}
    was = old.get("tool_surface")
    now = (provenance or {}).get("tool_surface")
    if not was or not now or was == now:
        return None
    return f"tool surface {was} → {now}"


def compute_diff(old: dict, new: "list[SkillResult]") -> list[dict]:
    """Compare new results against old baseline. Returns changed skills, largest Δ first."""
    diffs = []
    for result in new:
        if result.lift is None:
            continue
        old_entry = old.get(result.skill)
        if old_entry is None:
            diffs.append(
                {
                    "skill": result.skill,
                    "old_lift": None,
                    "new_lift": result.lift,
                    "delta": None,
                    "new_skill": True,
                }
            )
            continue
        old_lift = old_entry.get("lift")
        if old_lift is None:
            continue
        delta = result.lift - old_lift
        if abs(delta) < 0.001:
            continue  # no meaningful change
        diffs.append(
            {
                "skill": result.skill,
                "old_lift": old_lift,
                "new_lift": result.lift,
                "delta": round(delta, 4),
                "new_skill": False,
            }
        )
    diffs.sort(key=lambda d: abs(d["delta"] or 0), reverse=True)
    return diffs
