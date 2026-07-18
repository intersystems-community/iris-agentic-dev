---
name: objectscript-coverage
description: Measure ObjectScript line coverage using iris_coverage. Use when a user asks about test coverage, coverage reports, or wants to know how well their test suite exercises their code.
license: MIT
metadata:
  version: "1.0.0"
  author: InterSystems Developer Community
  compatibility: objectscript, iris
---

## Purpose

Measure line coverage for ObjectScript classes using `iris_coverage`. Coverage tells
you which executable lines were hit during a test run — essential for knowing whether
your test suite exercises production code.

## Quick Start

Most tasks need `mode=run` — it starts the monitor, runs tests, stops, and returns results:

```text
iris_coverage(
  mode="run",
  classes=["MyApp.MyClass", "MyApp.OtherClass"],
  test_path="MyApp.Tests",
  namespace="USER"
)
```

Or use `package` to auto-discover all concrete classes:

```text
iris_coverage(
  mode="run",
  package="MyApp",
  test_path="MyApp.Tests"
)
```

## Pre-flight Check

Before running coverage, verify the monitor is available:

```text
iris_coverage(mode="check", namespace="USER")
```

Expected response: `{ok: true, bbsiz_state: "ready"}`

If you get `BBSIZ_NOT_CONFIGURED`:

1. Open Management Portal → System Administration → Configuration → Additional Settings → Advanced Memory
2. Set `gmheap` to `256` (or higher)
3. Restart IRIS — gmheap takes effect at startup only

## Modes

| Mode     | What it does                                                                |
| -------- | --------------------------------------------------------------------------- |
| `run`    | Start + RunTest + Stop + Report in one call. **Use this for most tasks.**   |
| `check`  | Pre-flight: verify monitor is available. Run before first coverage attempt. |
| `start`  | Start monitoring (manual flow).                                             |
| `stop`   | Stop monitoring (manual flow).                                              |
| `report` | Collect results after manual stop.                                          |

## Parameters

| Parameter        | Required for     | Description                                                                                                                            |
| ---------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `mode`           | Always           | `run`, `check`, `start`, `stop`, or `report`                                                                                           |
| `classes`        | start/run/report | Explicit class list (e.g. `["MyApp.MyClass"]`). Mutually exclusive with `package`.                                                     |
| `package`        | start/run/report | Auto-discover concrete classes (e.g. `"MyApp"`). Expands via `%Dictionary.ClassDefinition`.                                            |
| `test_path`      | run              | Compiled class pattern for `%UnitTest.Manager.RunTest` (e.g. `"MyApp.Tests"`). `/noload` always used — classes must be compiled first. |
| `target_pct`     | optional         | Coverage target percentage. Response includes `meets_target: true/false`.                                                              |
| `namespace`      | optional         | IRIS namespace. Defaults to connection default.                                                                                        |
| `cobertura_path` | optional         | Path for Cobertura XML output. Requires TestCoverage IPM package (`zpm "install testcoverage"`).                                       |

## Response

```json
{
  "success": true,
  "total_pct": 73.4,
  "hits": 45,
  "total": 61,
  "meets_target": false,
  "target_pct": 90.0,
  "testcoverage_available": false,
  "testcoverage_hint": "Install with: zpm \"install testcoverage\"",
  "classes": [
    { "class": "MyApp.MyClass", "routine": "MyApp.MyClass.1", "hit": 45, "total": 61, "pct": 73.8 }
  ]
}
```

## Coverage via iris_test Shorthand

Set `coverage=true` on `iris_test` to run tests and measure coverage in one call:

```text
iris_test(
  pattern="MyApp.Tests",
  namespace="USER",
  coverage=true,
  coverage_target_pct=80.0
)
```

Response includes a `coverage` field with the same structure as `iris_coverage` output.

## TestCoverage IPM Package

`iris_coverage` checks for the [TestCoverage](https://github.com/intersystems/TestCoverage)
IPM package (`testcoverage`). If installed, it unlocks:

- Cobertura XML output (CI/CD integration)
- Per-method coverage detail
- Python-embedded coverage

Install: `zpm "install testcoverage"` (requires IPM)

The `testcoverage_available` field in every response tells you whether it's present.
When it's `false`, a `testcoverage_hint` with install instructions is also returned.

## Best Practices

- Always run `mode=check` on a new instance before attempting coverage
- Use `mode=run` for automated workflows — it handles the full lifecycle atomically
- Compile test classes before calling `iris_coverage` — use `iris_compile` first
- Use `target_pct` to enforce a coverage gate in CI: check `meets_target` in the response
- Coverage measures **line** execution, not branch or path coverage

## Error Codes

| Code                   | Meaning                         | Fix                                                                         |
| ---------------------- | ------------------------------- | --------------------------------------------------------------------------- |
| `BBSIZ_NOT_CONFIGURED` | `gmheap` too small for monitor  | Increase `gmheap` to 256+ in Management Portal, restart IRIS                |
| `MONITOR_IN_USE`       | Another process has the monitor | Call `mode=stop` first                                                      |
| `NO_CLASSES`           | Package expansion found nothing | Check package name; verify classes are compiled                             |
| `MISSING_PARAM`        | Required param absent           | Add `test_path` for `mode=run`; add `classes` or `package` for start/report |
| `INVALID_ACTION`       | Unknown mode string             | Use: `run`, `check`, `start`, `stop`, `report`                              |
