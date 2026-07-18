## What's new in 0.9.2

### `iris_coverage` — ObjectScript line coverage

New tool that measures which executable lines of your ObjectScript classes were hit
during a `%UnitTest` test run, powered by `%Monitor.System.LineByLine`.

**Quick start:**

```text
iris_coverage(mode="run", classes=["MyApp.MyClass"], test_path="MyApp.Tests")
```

Returns `total_pct`, `hits`, `total`, and a per-class breakdown — plus
`meets_target` when you set `target_pct`.

- `mode=run` — all-in-one: start monitor → run tests → stop → collect results
- `mode=check` — pre-flight: verify `gmheap` is configured (must be ≥256 MB)
- `mode=start/stop/report` — manual multi-step flow
- `package="MyApp"` — auto-discovers all concrete classes; no explicit class list needed
- `cobertura_path` — optional Cobertura XML output (requires TestCoverage IPM package)

Every response includes `testcoverage_available` — whether the
[TestCoverage](https://github.com/intersystems/TestCoverage) IPM package is installed.
If not, a `testcoverage_hint` with the install command is also returned.

### `iris_test` coverage shorthand

`iris_test` gains a `coverage` flag — set it to also measure coverage inline:

```text
iris_test(pattern="MyApp.Tests", coverage=true, coverage_target_pct=80)
```

Response gains a `coverage` field with the full `iris_coverage` result embedded.

### New skill: `objectscript-coverage`

The `objectscript-coverage` skill documents the coverage workflow, all modes and
parameters, the `gmheap` requirement, error codes, and best practices. Install it
alongside `objectscript-tdd` for a complete test-and-coverage loop.

**Benchmark lift: +0.67** (merged 2.83 vs baseline 2.17 on COV tasks; spec threshold ≥ 0.20).

## Notable fixes

- **Coverage output parsing** — fixed class names being prepended with RunTest stdout
  (`All PASSEDIrisDevTest.SqlPower` → `IrisDevTest.SqlPower`). A `COVERAGE_DATA_START`
  sentinel now separates test runner output from coverage data.

- **Package auto-discovery** — fixed `%STARTSWITH` SQL using `Package.%` (literal percent)
  instead of `Package.` — no classes were being found when using `package=` param.

## Full changelog

[v0.9.1...v0.9.2](https://github.com/intersystems-community/iris-agentic-dev/compare/v0.9.1...v0.9.2)
