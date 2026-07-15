"""benchmark/021 runner — path-aware agentic benchmark for iris-dev."""
import argparse
import os
import sys
import time


def parse_args():
    p = argparse.ArgumentParser(description="iris-dev path-aware benchmark runner")
    p.add_argument("--path", choices=["A", "B", "both"], default="both",
                   help="Which development path to benchmark (default: both)")
    p.add_argument("--categories", default=None,
                   help="Comma-separated task categories to run, e.g. GEN,MOD")
    p.add_argument("--task", default=None,
                   help="Run a single task by ID, e.g. GEN-01")
    p.add_argument("--harness", choices=["claude-code", "copilot", "both"], default="claude-code",
                   help="Which AI harness to use (default: claude-code)")
    p.add_argument("--toolset", choices=["baseline", "nostub", "merged"], default=None,
                   help="Tool set condition to run. Sets IRIS_TOOLSET env var for iris-dev. "
                        "If omitted, uses current IRIS_TOOLSET or defaults to baseline.")
    p.add_argument("--compare", action="store_true",
                   help="Generate three-way comparison report from existing condition result dirs. "
                        "Looks for the three most recent baseline/nostub/merged scores.json files.")
    p.add_argument("--report-only", metavar="SCORES_JSON",
                   help="Generate report from an existing scores.json, skip running tasks")
    p.add_argument("--dry-run", action="store_true",
                   help="Load tasks and print plan without executing")
    return p.parse_args()


def check_env():
    required = ["IRIS_HOST", "IRIS_WEB_PORT"]
    missing = [k for k in required if not os.environ.get(k)]
    if missing:
        print(f"ERROR: missing required env vars: {', '.join(missing)}", file=sys.stderr)
        sys.exit(2)
    use_bedrock = bool(
        os.environ.get("CLAUDE_CODE_USE_BEDROCK") or
        os.environ.get("AWS_BEARER_TOKEN_BEDROCK") or
        os.environ.get("AWS_ACCESS_KEY_ID")
    )
    if not use_bedrock and not os.environ.get("ANTHROPIC_API_KEY"):
        print("WARNING: no API credentials found. Set ANTHROPIC_API_KEY or AWS Bedrock env vars.",
              file=sys.stderr)


def main():
    args = parse_args()

    if args.compare:
        _run_compare()
        return

    if args.report_only:
        from .report import generate_report
        generate_report(args.report_only)
        return

    # Set IRIS_TOOLSET before any subprocess or discovery runs
    if args.toolset:
        os.environ["IRIS_TOOLSET"] = args.toolset

    check_env()

    from .task_loader import load_tasks
    tasks = load_tasks(
        path_filter=args.path,
        category_filter=args.categories.split(",") if args.categories else None,
        task_id=args.task,
    )

    if args.dry_run:
        print(f"Dry run: {len(tasks)} task(s) would run")
        for t in tasks:
            print(f"  {t['id']:10s}  path={t.get('path','both'):4s}  {t['description'][:60]}")
        return

    print(f"Running {len(tasks)} task(s) | path={args.path} | harness={args.harness}")

    active_condition = os.environ.get("IRIS_TOOLSET", "baseline")
    print(f"Condition: {active_condition}")

    # Reset namespace before the condition run (FR-001b)
    from .namespace import reset_benchmark_namespace, ensure_benchmark_namespace
    try:
        reset_benchmark_namespace()
        print(f"BENCHMARK namespace reset for condition={active_condition}")
    except Exception as e:
        # Fallback: ensure namespace exists if reset fails (e.g. first run)
        try:
            ensure_benchmark_namespace()
        except Exception:
            pass
        print(f"Note: namespace reset skipped ({e.__class__.__name__}: {e}), using ensure instead")

    from .result_writer import ResultWriter
    writer = ResultWriter()

    paths = (["A", "B"] if args.path == "both" else [args.path])
    harnesses = (["claude-code", "copilot"] if args.harness == "both" else [args.harness])

    condition_start = time.time()

    for path in paths:
        for harness in harnesses:
            if harness == "claude-code":
                from .claude_code import run_task
            else:
                from .copilot import run_task  # noqa: F811

            for task in tasks:
                if task.get("path", "both") not in ("both", path):
                    continue
                print(f"  [{path}/{harness}] {task['id']} ...", end=" ", flush=True)
                from .fixtures import apply_fixtures
                apply_fixtures(task.get("fixtures", []))
                result = run_task(task, path, condition=active_condition)
                from .judge import score_result
                scored = score_result(task, result)
                writer.record(task["id"], task["category"], path, harness, scored, result,
                               condition=active_condition)
                from .namespace import wipe_benchmark_namespace
                wipe_benchmark_namespace()
                print(f"score={scored['score']}")

    condition_wall_clock = round(time.time() - condition_start, 1)
    writer.set_condition_metadata(active_condition, condition_wall_clock)
    writer.finalize()
    print(f"\nResults: {writer.run_dir} (wall_clock={condition_wall_clock}s)")


def _run_compare():
    """Find the most recent scores.json for each condition and generate comparison report."""
    import json
    import glob
    results_base = os.path.join(os.path.dirname(__file__), "..", "results")
    results_base = os.path.normpath(results_base)

    def find_latest(condition: str):
        pattern = os.path.join(results_base, "*", "scores.json")
        candidates = []
        for path in glob.glob(pattern):
            try:
                with open(path) as f:
                    data = json.load(f)
                if data.get("condition") == condition:
                    candidates.append((os.path.getmtime(path), path, data))
            except Exception:
                pass
        if not candidates:
            return None, None
        candidates.sort(key=lambda x: x[0], reverse=True)
        _, path, data = candidates[0]
        return os.path.dirname(path), data

    baseline_dir, baseline_data = find_latest("baseline")
    nostub_dir, nostub_data = find_latest("nostub")
    merged_dir, merged_data = find_latest("merged")

    missing = [c for c, d in [("baseline", baseline_data), ("nostub", nostub_data), ("merged", merged_data)] if d is None]
    if missing:
        print(f"ERROR: missing condition result(s): {', '.join(missing)}", file=sys.stderr)
        print("Run each condition first: --toolset baseline, --toolset nostub, --toolset merged", file=sys.stderr)
        sys.exit(1)

    from .comparison import generate_comparison
    report = generate_comparison(baseline_data, nostub_data, merged_data)

    # Write comparison.json next to the merged results
    out_dir = merged_dir
    comparison_path = os.path.join(out_dir, "comparison.json")
    with open(comparison_path, "w") as f:
        json.dump(report, f, indent=2)
    print(f"comparison.json → {comparison_path}")

    # Write HTML report
    html_path = os.path.join(out_dir, "comparison_report.html")
    _write_comparison_html(report, html_path)
    print(f"comparison_report.html → {html_path}")

    verdict = report["hypothesis_result"].upper()
    diff = report.get("score_diff_merged_vs_baseline", 0)
    print(f"\nHypothesis: {verdict} (merged vs baseline: {diff:+.3f})")
    if report["regressions"]:
        print(f"Regressions: {len(report['regressions'])} task(s)")
    for note in report.get("notes", []):
        print(f"Note: {note}")


def _write_comparison_html(report: dict, path: str):
    """Write a simple HTML comparison report (T040)."""
    conds = report.get("conditions", {})

    def row(label, key, fmt="{:.3f}"):
        cells = "".join(
            f"<td>{fmt.format(conds.get(c, {}).get(key, 0)) if isinstance(conds.get(c, {}).get(key, 0), float) else conds.get(c, {}).get(key, 0)}</td>"
            for c in ("baseline", "nostub", "merged")
        )
        return f"<tr><td><b>{label}</b></td>{cells}</tr>"

    verdict = report["hypothesis_result"]
    badge_color = "#22c55e" if verdict == "confirmed" else "#ef4444"
    regressions_html = "".join(
        f"<li>{r['task_id']}: baseline={r['baseline_score']} → merged={r['merged_score']}</li>"
        for r in report.get("regressions", [])
    ) or "<li>None</li>"
    notes_html = "".join(f"<li>{n}</li>" for n in report.get("notes", []))

    html = f"""<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8">
<title>iris-dev Ablation Study — Comparison Report</title>
<style>
body{{font-family:'IBM Plex Mono',monospace;background:#0d1117;color:#e2eaf6;padding:32px;}}
h1{{font-size:18px;margin-bottom:4px;}}p.sub{{color:#7a90a8;font-size:12px;margin-bottom:24px;}}
table{{border-collapse:collapse;width:100%;margin-bottom:20px;}}
th,td{{padding:10px 14px;border:1px solid rgba(255,255,255,0.08);font-size:13px;text-align:left;}}
thead{{background:#1a2332;}}
.badge{{display:inline-block;padding:4px 12px;border-radius:6px;font-weight:600;font-size:13px;background:{badge_color};color:#fff;}}
ul{{padding-left:20px;line-height:1.8;font-size:12px;color:#7a90a8;}}
</style></head><body>
<h1>iris-dev Tool Ablation Study — Comparison Report</h1>
<p class="sub">3-condition study: baseline (34 tools) vs nostub (29 tools) vs merged (23 tools)</p>
<p>Hypothesis: <span class="badge">{verdict.upper()}</span>
&nbsp; Score diff (merged − baseline): <b>{report.get('score_diff_merged_vs_baseline', 0):+.3f}</b></p>
<table>
<thead><tr><th>Metric</th><th>Baseline (34)</th><th>Nostub (29)</th><th>Merged (23)</th></tr></thead>
<tbody>
{row("Mean score (0–3)", "mean_score")}
{row("Mean tool calls / task", "tool_call_count_mean")}
{row("Total stub errors", "total_stub_errors", "{}")}
{row("Total wrong tool calls", "total_wrong_tool_calls", "{}")}
{row("Wall clock (s)", "wall_clock_seconds", "{:.1f}")}
</tbody></table>
<h2 style="font-size:14px">Regressions (baseline=3 → merged&lt;2)</h2><ul>{regressions_html}</ul>
<h2 style="font-size:14px">Notes</h2><ul>{notes_html}</ul>
</body></html>"""

    with open(path, "w") as f:
        f.write(html)


if __name__ == "__main__":
    main()
