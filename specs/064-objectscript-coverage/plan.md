# Plan: 064 — ObjectScript Line Coverage Tool

## Tech stack

- Rust (same as all other tools in `iris-agentic-dev-core`)
- New file: `crates/iris-agentic-dev-core/src/tools/coverage.rs`
- Wired into `tools/mod.rs` via the same `dispatch!` + `registered_tool_names` pattern as `iris_global`
- Exposed as `iris_coverage` in the bin crate's `tool` subcommand (automatic — no extra changes needed)

## Architecture

Single tool: `iris_coverage` with a `mode` field.

```text
iris_coverage(
    mode:       "start" | "stop" | "report" | "run" | "check",
    classes:    ["MyApp.MyClass", ...],   // required for start/run/report
    test_path:  "My/Tests",               // required for mode=run
    target_pct: 90.0,                     // optional, default 90.0
    namespace:  "MYNS"                    // optional
)
```

### Execution path

All ObjectScript execution goes through `execute_via_generator` (HTTP). The entire
start→test→stop→collect sequence runs in a single generated class method so results
are always queried in the same process that called `Stop()`.

For `mode=run`, the generated code:

1. `Stop()` any leftover session (idempotent)
2. `Start(routineList, "", "")`
3. `RunTest(testPath, "/nodelete")`
4. `Stop()`
5. Loop over each routine → `%ResultSet.Execute()` → collect hit/total
6. Output JSON via `write $$$JSON(...)`

### Output format

```json
{
  "success": true,
  "total_pct": 73.4,
  "hits": 149,
  "total": 167,
  "meets_target": false,
  "target_pct": 90.0,
  "classes": [
    { "class": "MyApp.MyClass", "routine": "MyApp.MyClass.1", "hit": 45, "total": 61, "pct": 73.8 }
  ]
}
```

Error codes:

- `BBSIZ_NOT_CONFIGURED` — `$zu(84)` threw `<FUNCTION>` or returned unexpected value
- `MONITOR_IN_USE` — `Start()` returned error #6060
- `IRIS_EXECUTE_ERROR` — ObjectScript code threw an error
- `MISSING_PARAM` — required param not provided for mode

### mode=check

Pre-flight only — verifies `$zu(84,0,1,1,1,1,1,1)` returns 1 and no monitor is already running.
Returns `{ok: true, bbsiz_state: "ready"}` or actionable error with CPF fix instructions.

### mode=start / mode=stop

Thin wrappers for manual workflows (start monitor → run arbitrary code → stop → report).

### mode=report

Queries results for the given class list. Must be called in the same session as stop —
practical use is limited; mainly useful for inspection. Documents the same-process caveat.

## Files changed

| File                                                 | Change                                                                    |
| ---------------------------------------------------- | ------------------------------------------------------------------------- |
| `crates/iris-agentic-dev-core/src/tools/coverage.rs` | NEW — all logic                                                           |
| `crates/iris-agentic-dev-core/src/tools/mod.rs`      | Add `mod coverage`, `IrisCoverageParams`, dispatch, registered_tool_names |
| `crates/iris-agentic-dev-core/Cargo.toml`            | Add test binary entries                                                   |
| `README.md`                                          | Add `iris_coverage` to Tools table                                        |

## Toolset placement

`iris_coverage` goes in the **merged** toolset (same as all non-debug tools). No changes to
toolset selection logic needed — `registered_tool_names` already includes merged tools.

## ObjectScript code generation

The critical ObjectScript block for `mode=run`:

```objectscript
new $namespace
set $namespace = "MYNS"
try {
  do ##class(%Monitor.System.LineByLine).Stop()
  set routines = $lb("MyApp.ClassA.1","MyApp.ClassB.1")
  set sc = ##class(%Monitor.System.LineByLine).Start(routines,"","")
  if $$$ISERR(sc) {
    write "{""error_code"":""MONITOR_IN_USE"",""message"":""",
    write $system.Status.GetErrorText(sc),""}",!
    quit
  }
  do ##class(%UnitTest.Manager).RunTest("My/Tests","/nodelete")
  do ##class(%Monitor.System.LineByLine).Stop()
  // collect results
  set results = "["
  set first = 1
  set ptr = 0
  while $listnext(routines,ptr,rtn) {
    set rset = ##class(%ResultSet).%New("%Monitor.System.LineByLine:Result")
    do rset.Execute(rtn)
    set hit = 0, total = 0
    while rset.Next() {
      set data = rset.GetData(1)
      set ec = $listget(data,2)
      if ec < 0 { continue }
      set total = total + 1
      if ec > 0 { set hit = hit + 1 }
    }
    if 'first { set results = results_"," }
    set first = 0
    set cls = $piece(rtn,".",1,$length(rtn,".")-1)
    set pct = $select(total>0:$fnumber(hit/total*100,"",2),1:0)
    set results = results_"{""class"":"""_cls_""",""hit"":"_hit_",""total"":"_total_",""pct"":"_pct_"}"
  }
  set results = results_"]"
  write results, !
} catch ex {
  write "{""error_code"":""IRIS_EXECUTE_ERROR"",""message"":""",
  write ex.DisplayString(),""}",!
}
```

The JSON output is written to stdout and parsed by the Rust handler.

## Caveats documented in tool description

- Requires `bbsiz=4096` in `iris.cpf [config]` and IRIS restart — use `mode=check` first
- TBLP 127 (2026.3.0TBLP) does not support `$zu(84)` — returns `BBSIZ_NOT_CONFIGURED`
- Only one monitor session at a time — `mode=run` always stops any prior session first
- Classes arg takes product class names (no `.1` suffix) — tool adds `.1` internally
- Coverage is for executable lines only; labels/comments/blanks excluded from denominator

## Test strategy

### Unit tests (no IRIS)

- `IrisCoverageParams` deserialization for each mode
- `build_routine_name()`: `"MyApp.MyClass"` → `"MyApp.MyClass.1"`
- `build_coverage_run_code()`: spot-check generated ObjectScript contains `Start` and `Stop`
- `parse_coverage_output()`: valid JSON → structured result; error JSON → error code; empty → error
- `mode=check` with `$zu(84)` returning `<FUNCTION>` error text → `BBSIZ_NOT_CONFIGURED`
- `MISSING_PARAM` for mode=run without classes, mode=run without test_path

### Integration tests (live IRIS, `#[ignore]`)

- `live_coverage_check_returns_ok_or_bbsiz_error` — check pre-flight on real IRIS
- `live_coverage_run_with_known_class` — run against `%Library.RegisteredObject` or similar tiny class
