"""CLI entry point: python -m tests.e2e.skill_eval [OPTIONS] — T027."""

import argparse
import dataclasses
import datetime
import json
import os
import sys

# Ensure benchmark/021 is on path
import tests.e2e.skill_eval  # noqa: F401 (triggers sys.path shim)

from tests.e2e.skill_eval.evaluator import (
    discover_skills,
    load_eval_config,
    compare_to_baseline,
    SkillResult,
)
from tests.e2e.skill_eval.baseline import (
    compute_diff,
    format_diff_line,
    load_baseline,
    save_baseline,
)
from tests.e2e.skill_eval.cost_estimator import (
    estimate,
    format_dry_run,
    format_scorer_cost,
    merge_scorer_costs,
)
from tests.e2e.skill_eval.preflight import preflight
from tests.e2e.skill_eval.provenance import Provenance
from tests.e2e.skill_eval.reporter import EvalRun, print_summary, write_result
from tests.e2e.skill_eval.scoring import (
    EXIT_INTEGRITY,
    EXIT_MEASURED,
    EXIT_PREFLIGHT,
    baseline_write_allowed,
)
from tests.e2e.skill_eval.shard import (
    covered_skills,
    merge_shards,
    missing_shard_result,
)

_SKILLS_PACK_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "skills", "skills")
)
_TASKS_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)
_DEFAULT_RESULTS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "results")
)
_DEFAULT_BASELINE = os.path.join(_DEFAULT_RESULTS_DIR, "skill-baseline.json")
_DEFAULT_MODEL = "openai/gpt-4.1"


def _make_uncovered_result(skill_name: str) -> SkillResult:
    return SkillResult(
        skill=skill_name,
        fire_rate=None,
        implicit_fire_rate=None,
        isolation_fire_rate=None,
        pass_rate_baseline=None,
        pass_rate_skill=None,
        lift=None,
        lift_delta=None,
        regression_flag=False,
        new_skill=False,
        no_task_coverage=True,
        task_ids_used=[],
    )


def _run_skill(
    config, openai_key, model, n_runs, iris_host, iris_web_port, iris_container
) -> tuple:
    """Measure one skill. Returns `(SkillResult, lift_data)`.

    The raw `lift_data` travels with the result because it carries the scored/unscored counts
    and `run_valid`, and the caller needs those to decide whether the run may touch the
    baseline. `SkillResult` grows those fields in Phase 4.
    """
    from tests.e2e.skill_eval.fire_rate import measure_fire_rate
    from tests.e2e.skill_eval.lift import measure_lift
    from tests.e2e.skill_eval.isolation import check_isolation

    print(f"  [{config.skill}] measuring fire-rate ({n_runs} runs)...", flush=True)
    fire_rate = measure_fire_rate(
        config, n_runs=n_runs, openai_api_key=openai_key, model=model
    )
    print(f"  [{config.skill}] fire_rate={fire_rate:.2f}", flush=True)

    # Implicit fire-rate: prompt doesn't name the skill — tests autonomous triggering
    implicit_fire_rate = None
    if config.implicit_fire_rate_prompt:
        print(
            f"  [{config.skill}] measuring implicit fire-rate ({n_runs} runs)...",
            flush=True,
        )
        implicit_fire_rate = measure_fire_rate(
            config,
            n_runs=n_runs,
            openai_api_key=openai_key,
            model=model,
            prompt=config.implicit_fire_rate_prompt,
        )
        print(
            f"  [{config.skill}] implicit_fire_rate={implicit_fire_rate:.2f}",
            flush=True,
        )

    isolation_fire_rate = None
    if config.domain_skill and config.isolation_prompt:
        print(f"  [{config.skill}] checking isolation ({n_runs} runs)...", flush=True)
        isolation_fire_rate = check_isolation(
            config, n_runs=n_runs, openai_api_key=openai_key, model=model
        )
        print(
            f"  [{config.skill}] isolation_fire_rate={isolation_fire_rate:.2f}",
            flush=True,
        )

    lift_data = {
        "pass_rate_baseline": None,
        "pass_rate_skill": None,
        "lift": None,
        "task_ids_used": [],
        # A skill with no benchmark tasks measured nothing to invalidate.
        "run_valid": True,
        "items_scored": 0,
        "items_unscored": 0,
        "scorer_cost": None,
        "arms": None,
        "scoring_mode": None,
    }
    if config.benchmark_tasks:
        print(
            f"  [{config.skill}] measuring lift on {config.benchmark_tasks}...",
            flush=True,
        )
        lift_data = measure_lift(
            config,
            n_runs=n_runs,
            openai_api_key=openai_key,
            model=model,
            iris_host=iris_host,
            iris_web_port=iris_web_port,
            iris_container=iris_container,
        )
        scored = lift_data.get("items_scored", 0)
        unscored = lift_data.get("items_unscored", 0)
        print(
            f"  [{config.skill}] lift={lift_data.get('lift')} "
            f"({scored}/{scored + unscored} scored)",
            flush=True,
        )

    result = SkillResult(
        skill=config.skill,
        fire_rate=fire_rate,
        implicit_fire_rate=implicit_fire_rate,
        isolation_fire_rate=isolation_fire_rate,
        pass_rate_baseline=lift_data.get("pass_rate_baseline"),
        pass_rate_skill=lift_data.get("pass_rate_skill"),
        lift=lift_data.get("lift"),
        lift_delta=None,
        regression_flag=False,
        new_skill=False,
        no_task_coverage=False,
        task_ids_used=lift_data.get("task_ids_used", []),
        arms=lift_data.get("arms"),
    )
    return result, lift_data


def _provenance(run_id: str, lift_data: dict, probe, runs: int) -> dict:
    """What this measurement was taken under, so a later run can say whether it compares.

    Recorded per skill rather than per run because a sharded night measures each skill in its
    own job, on its own runner, against its own container.
    """
    from runner._client import haiku_model

    return dataclasses.asdict(
        Provenance(
            run_id=run_id,
            task_ids=sorted(lift_data.get("task_ids_used") or []),
            scoring_mode=lift_data.get("scoring_mode") or "judge",
            scorer_model=probe.scorer_model,
            scorer_model_requested=haiku_model(),
            tool_surface=probe.tool_surface or "none",
            runs=runs,
        )
    )


def _resolved(results, field: str):
    """The one value every skill's provenance agrees on, or all of them when they don't."""
    values = sorted(
        {
            (r.provenance or {}).get(field)
            for r in results
            if (r.provenance or {}).get(field)
        }
    )
    if not values:
        return None
    return values[0] if len(values) == 1 else ", ".join(values)


def _merge_and_report(args) -> int:
    """Combine the per-skill shard results into one summary. Returns the process exit code.

    This is the gate for a sharded nightly run: each shard evaluates one skill and compares it
    to the baseline on its own, but nothing decides whether the *night* passed until the
    shards are back together. Exits 1 on any regression, exactly as the single-job run did.
    """
    merged = merge_shards(args.merge_results)
    results = merged.results
    if not results:
        print(f"No shard results found under {args.merge_results}", file=sys.stderr)

    # FR-010: one rule for the whole table. Two thresholds means half of it was judged under a
    # rule the other half was not, and the footer would print one of them as if it were both.
    if merged.threshold_conflict:
        print(
            "Shards disagree on the regression threshold: "
            + " and ".join(f"{t:.2f}" for t in merged.threshold_conflict)
            + ". The outcomes were not all computed under the same rule, so there is no one "
            "table to publish.",
            file=sys.stderr,
        )
        return EXIT_INTEGRITY

    reported = {r.skill for r in results}
    # A shard that timed out or failed to upload leaves no result at all. Reporting only what
    # arrived would call a clean night on a third of the suite.
    missing = [
        s
        for s in covered_skills(_SKILLS_PACK_DIR, _TASKS_SKILLS_DIR)
        if s not in reported
    ]
    for skill in missing:
        results.append(missing_shard_result(skill))

    regressions = [r.skill for r in results if r.regression_flag]
    improvements = [
        r.skill for r in results if r.lift_delta is not None and r.lift_delta > 0
    ]

    run = EvalRun(
        run_id=datetime.datetime.utcnow().strftime("%Y-%m-%dT%H%M%S"),
        model=args.model,
        # The merge job calls no scorer of its own; this is what the shards reported scoring
        # with, read back out of their provenance.
        judge_model=_resolved(results, "scorer_model"),
        timestamp=datetime.datetime.utcnow().isoformat() + "Z",
        regression_threshold=(
            merged.threshold
            if merged.threshold is not None
            else args.regression_threshold
        ),
        skills=results,
        summary={
            "regressions": regressions,
            "improvements": improvements,
            "uncovered": missing,
            "shards_merged": merged.shards_read,
        },
        scorer_model_requested=_resolved(results, "scorer_model_requested"),
        tool_surface=_resolved(results, "tool_surface"),
        run_valid=merged.run_valid,
        items_unscored_share=merged.items_unscored_share,
        reruns=merged.reruns,
    )
    print_summary(run)
    if missing:
        print(f"Missing shard results: {', '.join(missing)}")
    path = write_result(run, args.output)
    print(f"\nCombined results written to: {path}")

    if args.update_baseline:
        measured = [r for r in results if r.lift is not None]
        baseline = load_baseline(_DEFAULT_BASELINE)
        diff = compute_diff(baseline, measured)
        save_baseline(measured, _DEFAULT_BASELINE)
        print("\nBaseline updated. Changes:")
        for d in diff:
            print(format_diff_line(d))
        if not diff:
            print("  (no changes)")

    return 1 if regressions else 0


def main():
    parser = argparse.ArgumentParser(
        description="Skill regression and lift measurement suite"
    )
    parser.add_argument("--skill", help="Evaluate a single skill by name")
    parser.add_argument(
        "--category",
        help="Filter skills by name prefix (e.g. objectscript, iris, domain)",
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="Print cost estimate and exit"
    )
    parser.add_argument(
        "--yes", action="store_true", help="Skip confirmation for full-suite run"
    )
    parser.add_argument(
        "--update-baseline",
        action="store_true",
        help="Update skill-baseline.json after run",
    )
    parser.add_argument("--regression-threshold", type=float, default=0.05)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--output", default=_DEFAULT_RESULTS_DIR)
    parser.add_argument("--model", default=_DEFAULT_MODEL)
    parser.add_argument(
        "--list-skills",
        action="store_true",
        help="Print covered skill names as a JSON array and exit (builds the CI matrix)",
    )
    parser.add_argument(
        "--preflight-only",
        action="store_true",
        help=(
            "Run the preflight checks and exit — 0 ready, 2 not. Costs one small scoring "
            "call. Runs once in CI so a missing credential is reported before the matrix "
            "starts nine containers to discover it nine times."
        ),
    )
    parser.add_argument(
        "--merge-results",
        metavar="DIR",
        help="Merge per-shard skill-eval-*.json under DIR into one summary and exit",
    )
    args = parser.parse_args()

    # Before anything spends: corpus, scorer, binary, tool surface. Skipped for the three
    # modes that start no session. Exit 2 means nothing was spent — the distinction from 1
    # is the point of having both codes.
    probe = preflight(args)
    if not probe.ok:
        print(f"preflight: {probe.failure}", file=sys.stderr)
        sys.exit(EXIT_PREFLIGHT)
    if not probe.skipped:
        print(
            f"preflight ok — scorer {probe.scorer_model} via {probe.auth_source}, "
            f"tools {probe.tool_surface} at {probe.binary}"
        )
    if args.preflight_only:
        sys.exit(EXIT_MEASURED)

    # Both shard-support modes are pure file IO. They run in CI jobs that hold no secrets and
    # start no container.
    if args.list_skills:
        print(json.dumps(covered_skills(_SKILLS_PACK_DIR, _TASKS_SKILLS_DIR)))
        sys.exit(0)

    if args.merge_results:
        sys.exit(_merge_and_report(args))

    # The agent key. Not checked here: `preflight()` above owns every reason a run must not
    # start, and a second exit-2 site outside it drifts from the contract in scoring.md.
    openai_key = os.environ.get("OPENAI_API_KEY", "")

    iris_container = os.environ.get("IRIS_CONTAINER", "iris-dev-iris")
    iris_web_port = os.environ.get("IRIS_WEB_PORT", "52780")
    iris_host = "localhost"

    # Discover all skills
    all_skills = discover_skills(_SKILLS_PACK_DIR)

    # Apply filters
    if args.skill:
        target_skills = [args.skill] if args.skill in all_skills else []
        if not target_skills:
            print(
                f"ERROR: skill '{args.skill}' not found in skills/skills/",
                file=sys.stderr,
            )
            sys.exit(2)
    elif args.category:
        if args.category == "domain":
            # domain = skills with domain_skill: true in eval.yaml
            target_skills = [
                s
                for s in all_skills
                if load_eval_config(s, _TASKS_SKILLS_DIR)
                and load_eval_config(s, _TASKS_SKILLS_DIR).domain_skill
            ]
        else:
            target_skills = [s for s in all_skills if s.startswith(args.category)]
    else:
        target_skills = all_skills

    # Load configs
    configs = []
    uncovered = []
    for skill in target_skills:
        cfg = load_eval_config(skill, _TASKS_SKILLS_DIR)
        if cfg:
            configs.append(cfg)
        else:
            uncovered.append(skill)

    # Cost estimate
    est = estimate(configs, runs=args.runs)

    if args.dry_run:
        print(format_dry_run(est, n_covered=len(configs), n_uncovered=len(uncovered)))
        sys.exit(0)

    # Confirmation for full-suite (not single-skill)
    if not args.skill and not args.yes:
        print(format_dry_run(est, n_covered=len(configs), n_uncovered=len(uncovered)))
        answer = input("\nProceed? [y/N] ").strip().lower()
        if answer not in ("y", "yes"):
            print("Aborted.")
            sys.exit(3)

    print(
        f"\nRunning skill evaluation ({len(configs)} covered, {len(uncovered)} uncovered)...\n"
    )

    # Load baseline for regression comparison
    baseline = load_baseline(_DEFAULT_BASELINE)

    # Run evaluations
    run_id = datetime.datetime.utcnow().strftime("%Y-%m-%dT%H%M%S")
    results: list[SkillResult] = []
    invalid_skills: list[str] = []
    cost_records: list[dict] = []
    for cfg in configs:
        result, lift_data = _run_skill(
            cfg,
            openai_key,
            args.model,
            args.runs,
            iris_host,
            iris_web_port,
            iris_container,
        )
        # Recorded before the comparison, because the comparison reads it: a Δ is only computed
        # when this run's task set, scale, and grader match the ones the entry was measured with.
        result.provenance = _provenance(run_id, lift_data, probe, args.runs)
        if lift_data.get("scorer_cost"):
            cost_records.append(lift_data["scorer_cost"])
        if lift_data.get("run_valid", True):
            result = compare_to_baseline(
                result, baseline, threshold=args.regression_threshold
            )
        else:
            # Too much of this skill went unscored to compare it to anything. A Δ against a
            # rate computed over three of twenty-four items is noise wearing a number.
            invalid_skills.append(cfg.skill)
            print(
                f"  [{cfg.skill}] {lift_data.get('items_unscored')} of "
                f"{lift_data.get('items_total')} items unscored "
                f"({lift_data.get('unscored_share')}) — no comparison, no baseline write",
                flush=True,
            )
        results.append(result)

    # Add uncovered skills
    for skill in uncovered:
        results.append(_make_uncovered_result(skill))

    # Build summary
    regressions = [r.skill for r in results if r.regression_flag]
    improvements = [
        r.skill for r in results if r.lift_delta is not None and r.lift_delta > 0
    ]
    uncovered_names = [r.skill for r in results if r.no_task_coverage]

    run_valid = not invalid_skills
    actual_cost = merge_scorer_costs(cost_records)
    run = EvalRun(
        run_id=run_id,
        model=args.model,
        judge_model=probe.scorer_model,
        timestamp=datetime.datetime.utcnow().isoformat() + "Z",
        regression_threshold=args.regression_threshold,
        skills=results,
        summary={
            "regressions": regressions,
            "improvements": improvements,
            "uncovered": uncovered_names,
            "estimated_cost_usd": est["cost_usd"],
            "scorer_cost": actual_cost,
            "run_valid": run_valid,
            "invalid_skills": invalid_skills,
            "tool_surface": probe.tool_surface,
        },
        scorer_model_requested=_resolved(results, "scorer_model_requested"),
        tool_surface=probe.tool_surface,
        run_valid=run_valid,
        reruns=None,
    )

    print_summary(run)
    print(format_scorer_cost(actual_cost))
    path = write_result(run, args.output)
    print(f"\nResults written to: {path}")

    # An invalid run may not reach the durable file, first write or not. The 2026-08 baseline
    # holds nine entries of fabricated zeros because a run was asked to write them and nothing
    # checked whether it had measured anything.
    if not run_valid:
        print(
            f"\nBaseline not written: {', '.join(invalid_skills)} scored too little to be a "
            "reference measurement."
        )
    elif not os.path.exists(_DEFAULT_BASELINE):
        save_baseline(results, _DEFAULT_BASELINE)
        print("No baseline found — created from this run.")
    elif baseline_write_allowed(run_valid, args.update_baseline):
        diff = compute_diff(baseline, results)
        save_baseline(results, _DEFAULT_BASELINE)
        print("\nBaseline updated. Changes:")
        if diff:
            for d in diff:
                print(format_diff_line(d))
        else:
            print("  (no changes)")

    # 1 covers both an integrity failure and a regression; only the message distinguishes
    # them. A reported regression is not yet blocking on its own (research R12) — it becomes
    # so after a night of real numbers — but an invalid run always is.
    if not run_valid:
        sys.exit(EXIT_INTEGRITY)
    sys.exit(EXIT_INTEGRITY if regressions else EXIT_MEASURED)


if __name__ == "__main__":
    main()
