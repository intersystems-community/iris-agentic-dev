"""Per-skill sharding for the nightly workflow.

The suite is ~100 minutes of sequential opencode sessions, so it ran past its 75-minute job
timeout and was cancelled every night. One GitHub job per skill fixes the wall clock; these
two functions are what the workflow needs from the harness to do that.

`covered_skills` builds the matrix. `merge_results` puts the shards back together, so the
combined summary and the baseline write still happen exactly once.
"""

import glob
import json
import os

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


def merge_results(results_dir: str) -> list[SkillResult]:
    """Read every shard's `skill-eval-*.json` under `results_dir` and return one result list.

    Searches recursively: `download-artifact` with no `name` unpacks each artifact into its own
    subdirectory. When two files carry the same skill — a re-run shard — the later `run_id`
    wins. A file that will not parse is skipped rather than fatal: a cancelled shard can leave
    a half-written JSON behind, and losing one skill beats losing the report.
    """
    by_skill: dict[str, tuple[str, SkillResult]] = {}
    paths = sorted(
        glob.glob(os.path.join(results_dir, "skill-eval-*.json"))
        + glob.glob(
            os.path.join(results_dir, "**", "skill-eval-*.json"), recursive=True
        )
    )
    for path in dict.fromkeys(paths):
        try:
            with open(path) as f:
                data = json.load(f)
        except (OSError, json.JSONDecodeError):
            continue
        run_id = str(data.get("run_id") or "")
        for entry in data.get("skills") or []:
            try:
                result = SkillResult(**entry)
            except TypeError:
                continue
            previous = by_skill.get(result.skill)
            if previous is None or run_id >= previous[0]:
                by_skill[result.skill] = (run_id, result)
    return [result for _, result in by_skill.values()]
