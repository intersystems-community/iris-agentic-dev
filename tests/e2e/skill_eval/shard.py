"""Per-skill sharding for the nightly workflow.

The suite is ~100 minutes of sequential opencode sessions, so it ran past its 75-minute job
timeout and was cancelled every night. One GitHub job per skill fixes the wall clock; these
two functions are what the workflow needs from the harness to do that.

`covered_skills` builds the matrix. `merge_results` puts the shards back together, so the
combined summary and the baseline write still happen exactly once.
"""

import dataclasses
import glob
import json
import os
from typing import Optional

from tests.e2e.skill_eval.evaluator import (
    SkillResult,
    discover_skills,
    load_eval_config,
)


def covered_skills(skills_pack_dir: str, tasks_skills_dir: str) -> list[str]:
    """Skills that ship in the pack *and* have an eval.yaml, sorted.

    Sharding on `discover_skills` alone would spend a job, an IRIS container and four minutes
    of setup on each of the 25 skills that have nothing to measure.
    """
    return [
        skill
        for skill in discover_skills(skills_pack_dir)
        if load_eval_config(skill, tasks_skills_dir) is not None
    ]


@dataclasses.dataclass
class MergedShards:
    """One night, put back together — and what the merge itself has to say about it.

    The counts are recomputed over all items rather than averaged over shards: averaging shares
    would let a two-item shard outweigh a forty-item one, and the 10% unscored gate reads the
    night, not the mean of its parts.
    """

    results: list
    run_valid: bool
    items_scored: int
    items_unscored: int
    items_total: int
    items_unscored_share: float
    threshold: Optional[float]
    # Every distinct threshold found, when the shards disagreed. FR-010: half a table judged
    # under a different rule is not one table.
    threshold_conflict: list
    # skill → the `run_id` a later shard displaced. A silently deduplicated table cannot be
    # told from one that only ever had one result per skill.
    reruns: dict
    shards_read: int


def missing_shard_result(skill: str) -> SkillResult:
    """A skill whose shard uploaded nothing: a named hole, not a failure and not a zero.

    Nothing per-skill fails the run in 118 (FR-009). What must not happen is the skill quietly
    dropping out of the table, which is how a report covering a third of the suite read as a
    clean night.
    """
    return SkillResult(
        skill=skill,
        fire_rate=None,
        implicit_fire_rate=None,
        isolation_fire_rate=None,
        pass_rate_baseline=None,
        pass_rate_skill=None,
        lift=None,
        lift_delta=None,
        regression_flag=False,
        new_skill=False,
        # It has an eval.yaml — coverage is not what went missing.
        no_task_coverage=False,
        task_ids_used=[],
        outcome="not_comparable",
        outcome_reason="shard produced no result",
    )


def _shard_paths(results_dir: str) -> list[str]:
    """Recursive: `download-artifact` with no `name` unpacks each artifact into a subdirectory."""
    paths = sorted(
        glob.glob(os.path.join(results_dir, "skill-eval-*.json"))
        + glob.glob(
            os.path.join(results_dir, "**", "skill-eval-*.json"), recursive=True
        )
    )
    return list(dict.fromkeys(paths))


def merge_shards(results_dir: str) -> MergedShards:
    """Read every shard's `skill-eval-*.json` under `results_dir` and combine them.

    When two files carry the same skill — a re-run shard — the later `run_id` wins and the
    displaced one is recorded. A file that will not parse is skipped rather than fatal: a
    cancelled shard can leave a half-written JSON behind, and losing one skill beats losing the
    report.
    """
    by_skill: dict[str, tuple[str, SkillResult]] = {}
    reruns: dict[str, str] = {}
    thresholds: list[float] = []
    run_valid = True
    shards_read = 0

    for path in _shard_paths(results_dir):
        try:
            with open(path) as f:
                data = json.load(f)
        except (OSError, json.JSONDecodeError):
            print(f"WARNING: skipping unreadable shard file {path}")
            continue
        shards_read += 1
        run_id = str(data.get("run_id") or "")
        threshold = data.get("regression_threshold")
        if threshold is not None and threshold not in thresholds:
            thresholds.append(threshold)
        # A shard that declares nothing is treated as valid; only an explicit False invalidates.
        if (
            data.get("run_valid") is False
            or data.get("summary", {}).get("run_valid") is False
        ):
            run_valid = False
        for entry in data.get("skills") or []:
            try:
                result = SkillResult(**entry)
            except TypeError:
                continue
            previous = by_skill.get(result.skill)
            if previous is None:
                by_skill[result.skill] = (run_id, result)
            elif run_id >= previous[0]:
                reruns[result.skill] = previous[0]
                by_skill[result.skill] = (run_id, result)
            else:
                reruns[result.skill] = run_id

    results = [result for _, result in by_skill.values()]
    scored = total = 0
    for result in results:
        for name in ("baseline", "skill"):
            arm = (result.arms or {}).get(name) or {}
            scored += arm.get("items_scored") or 0
            total += arm.get("items_total") or 0
    unscored = total - scored

    return MergedShards(
        results=results,
        run_valid=run_valid,
        items_scored=scored,
        items_unscored=unscored,
        items_total=total,
        items_unscored_share=round(unscored / total, 4) if total else 0.0,
        threshold=thresholds[0] if len(thresholds) == 1 else None,
        threshold_conflict=sorted(thresholds) if len(thresholds) > 1 else [],
        reruns=reruns,
        shards_read=shards_read,
    )


def merge_results(results_dir: str) -> list[SkillResult]:
    """Just the result list, for callers that need nothing else about the merge."""
    return merge_shards(results_dir).results
