"""CLI entry point: python -m tests.e2e.skill_eval [OPTIONS] — T027."""
import argparse
import dataclasses
import datetime
import os
import sys

# Ensure benchmark/021 is on path
import tests.e2e.skill_eval  # noqa: F401 (triggers sys.path shim)

from tests.e2e.skill_eval.evaluator import (
    discover_skills, load_eval_config, compare_to_baseline,
    SkillResult,
)
from tests.e2e.skill_eval.baseline import load_baseline, save_baseline, compute_diff
from tests.e2e.skill_eval.cost_estimator import estimate, format_dry_run
from tests.e2e.skill_eval.reporter import EvalRun, print_summary, write_result

_LIGHT_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "light-skills", "skills")
)
_TASKS_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)
_DEFAULT_RESULTS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "results")
)
_DEFAULT_BASELINE = os.path.join(_DEFAULT_RESULTS_DIR, "skill-baseline.json")
_DEFAULT_MODEL = "amazon-bedrock/us.anthropic.claude-sonnet-4-5-20250929-v1:0"


def _make_uncovered_result(skill_name: str) -> SkillResult:
    return SkillResult(
        skill=skill_name,
        fire_rate=None, isolation_fire_rate=None,
        pass_rate_baseline=None, pass_rate_skill=None,
        lift=None, lift_delta=None,
        regression_flag=False, new_skill=False,
        no_task_coverage=True, task_ids_used=[],
    )


def _run_skill(config, openai_key, model, n_runs, iris_host, iris_web_port, iris_container) -> SkillResult:
    from tests.e2e.skill_eval.fire_rate import measure_fire_rate
    from tests.e2e.skill_eval.lift import measure_lift
    from tests.e2e.skill_eval.isolation import check_isolation

    print(f"  [{config.skill}] measuring fire-rate ({n_runs} runs)...", flush=True)
    fire_rate = measure_fire_rate(config, n_runs=n_runs, openai_api_key=openai_key, model=model)
    print(f"  [{config.skill}] fire_rate={fire_rate:.2f}", flush=True)

    isolation_fire_rate = None
    if config.domain_skill and config.isolation_prompt:
        print(f"  [{config.skill}] checking isolation ({n_runs} runs)...", flush=True)
        isolation_fire_rate = check_isolation(config, n_runs=n_runs, openai_api_key=openai_key, model=model)
        print(f"  [{config.skill}] isolation_fire_rate={isolation_fire_rate:.2f}", flush=True)

    lift_data = {"pass_rate_baseline": None, "pass_rate_skill": None, "lift": None, "task_ids_used": []}
    if config.benchmark_tasks:
        print(f"  [{config.skill}] measuring lift on {config.benchmark_tasks}...", flush=True)
        lift_data = measure_lift(
            config, n_runs=n_runs, openai_api_key=openai_key, model=model,
            iris_host=iris_host, iris_web_port=iris_web_port, iris_container=iris_container,
        )
        print(f"  [{config.skill}] lift={lift_data.get('lift')}", flush=True)

    return SkillResult(
        skill=config.skill,
        fire_rate=fire_rate,
        isolation_fire_rate=isolation_fire_rate,
        pass_rate_baseline=lift_data.get("pass_rate_baseline"),
        pass_rate_skill=lift_data.get("pass_rate_skill"),
        lift=lift_data.get("lift"),
        lift_delta=None,
        regression_flag=False,
        new_skill=False,
        no_task_coverage=False,
        task_ids_used=lift_data.get("task_ids_used", []),
    )


def main():
    parser = argparse.ArgumentParser(description="Skill regression and lift measurement suite")
    parser.add_argument("--skill", help="Evaluate a single skill by name")
    parser.add_argument("--category", help="Filter skills by name prefix (e.g. objectscript, iris, domain)")
    parser.add_argument("--dry-run", action="store_true", help="Print cost estimate and exit")
    parser.add_argument("--yes", action="store_true", help="Skip confirmation for full-suite run")
    parser.add_argument("--update-baseline", action="store_true", help="Update skill-baseline.json after run")
    parser.add_argument("--regression-threshold", type=float, default=0.05)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--output", default=_DEFAULT_RESULTS_DIR)
    parser.add_argument("--model", default=_DEFAULT_MODEL)
    args = parser.parse_args()

    openai_key = os.environ.get("OPENAI_API_KEY", "")
    if not openai_key:
        print("ERROR: OPENAI_API_KEY not set", file=sys.stderr)
        sys.exit(2)

    iris_container = os.environ.get("IRIS_CONTAINER", "iris-dev-iris")
    iris_web_port = os.environ.get("IRIS_WEB_PORT", "52780")
    iris_host = "localhost"

    # Discover all skills
    all_skills = discover_skills(_LIGHT_SKILLS_DIR)

    # Apply filters
    if args.skill:
        target_skills = [args.skill] if args.skill in all_skills else []
        if not target_skills:
            print(f"ERROR: skill '{args.skill}' not found in light-skills/skills/", file=sys.stderr)
            sys.exit(2)
    elif args.category:
        if args.category == "domain":
            # domain = skills with domain_skill: true in eval.yaml
            target_skills = [
                s for s in all_skills
                if load_eval_config(s, _TASKS_SKILLS_DIR) and
                   load_eval_config(s, _TASKS_SKILLS_DIR).domain_skill
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

    print(f"\nRunning skill evaluation ({len(configs)} covered, {len(uncovered)} uncovered)...\n")

    # Load baseline for regression comparison
    baseline = load_baseline(_DEFAULT_BASELINE)

    # Run evaluations
    results: list[SkillResult] = []
    for cfg in configs:
        result = _run_skill(cfg, openai_key, args.model, args.runs,
                            iris_host, iris_web_port, iris_container)
        result = compare_to_baseline(result, baseline, threshold=args.regression_threshold)
        results.append(result)

    # Add uncovered skills
    for skill in uncovered:
        results.append(_make_uncovered_result(skill))

    # Build summary
    regressions = [r.skill for r in results if r.regression_flag]
    improvements = [r.skill for r in results if r.lift_delta is not None and r.lift_delta > 0]
    uncovered_names = [r.skill for r in results if r.no_task_coverage]

    run = EvalRun(
        run_id=datetime.datetime.utcnow().strftime("%Y-%m-%dT%H%M%S"),
        model=args.model,
        judge_model="amazon-bedrock/us.anthropic.claude-sonnet-4-5-20250929-v1:0",
        timestamp=datetime.datetime.utcnow().isoformat() + "Z",
        regression_threshold=args.regression_threshold,
        skills=results,
        summary={
            "regressions": regressions,
            "improvements": improvements,
            "uncovered": uncovered_names,
            "estimated_cost_usd": est["cost_usd"],
        },
    )

    print_summary(run)
    path = write_result(run, args.output)
    print(f"\nResults written to: {path}")

    # First-run baseline creation (FR-006)
    if not os.path.exists(_DEFAULT_BASELINE):
        save_baseline(results, _DEFAULT_BASELINE)
        print("No baseline found — created from this run.")
    elif args.update_baseline:
        diff = compute_diff(baseline, results)
        save_baseline(results, _DEFAULT_BASELINE)
        print("\nBaseline updated. Changes:")
        if diff:
            for d in diff:
                sign = "+" if (d["delta"] or 0) > 0 else ""
                new_skill = " (new)" if d["new_skill"] else ""
                old = f"{d['old_lift']:.2f}" if d["old_lift"] is not None else "n/a"
                print(f"  {d['skill']}: {old} → {d['new_lift']:.2f} ({sign}{d['delta']:.2f}){new_skill}")
        else:
            print("  (no changes)")

    sys.exit(1 if regressions else 0)


if __name__ == "__main__":
    main()
