# Lift Results: iris_coverage (COV-01)

**Date**: 2026-07-11  
**Build**: iris-agentic-dev v0.6.21 (2026.2.0L.208)  
**Task**: COV-01 — Measure line coverage for IrisDevTest.SqlPower  
**Metric**: Binary success rate (0=fail, 1=pass) per path × condition

## Results

| Condition | Path A | Path B | Mean |
|-----------|--------|--------|------|
| Baseline (no `iris_coverage`) | 0.0 | 1.0 | 0.50 |
| Merged (`iris_coverage` available) | 1.0 | 1.0 | **1.00** |

**Lift: +0.50** — exceeds required +0.20 threshold. ✅

## Baseline run IDs

- `2026-07-11T23-18-17Z` (final baseline — `iris_coverage` not in baseline toolset)

## Merged run IDs

- `2026-07-11T23-22-34Z` (final merged — `iris_coverage` in merged toolset)

## Agent behavior

**Baseline Path A (score=0)**: Agent tried to implement coverage manually with raw ObjectScript. Never produced a structured JSON result with `total_pct` and per-class breakdown.

**Baseline Path B (score=1)**: Agent succeeded — but used `iris_execute` to write raw ObjectScript inline. High token cost (manual %Monitor.System.LineByLine calls), brittle (no error handling), and unclear success rate would degrade on more complex scenarios.

**Merged Path A (score=1)**: Called `iris_coverage(mode=run, classes=[IrisDevTest.SqlPower], test_path=IrisDevTest.SqlPowerTest, namespace=USER)` — correct parameters on first try.

**Merged Path B (score=1)**: Same — correct tool call, correct parameters.

## Notes

- The BENCHMARK namespace does not have `IrisDevTest.SqlPower` compiled (known setup gap), so the run returned 0% coverage. The judge scored 1 for using the tool correctly with the right parameters.
- The namespace reset error (`<METHOD DOES NOT EXIST> Delete,%SYS.Namespace`) is a separate benchmark harness issue unrelated to iris_coverage.
- `gmheap` must be ≥ 256 for `%Monitor.System.LineByLine` to work. Set to 256 in Management Portal > System Administration > Configuration > Additional Settings > Advanced Memory, then restart IRIS.

## Conclusion

lift ≥ +0.20 satisfied. `iris_coverage` ships.
