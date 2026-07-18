---
name: iris-coverage-run
description: Measure ObjectScript line coverage using %Monitor.System.LineByLine. Wraps a %UnitTest.Manager.RunTest() call, collects per-class hit/total counts, prints a coverage table. Prerequisite: iris-coverage-setup (bbsiz=4096, IRIS restarted).
---

# /iris-coverage-run — Measure ObjectScript Line Coverage

Run a test suite wrapped in `%Monitor.System.LineByLine` and report per-class coverage.
Prerequisite: `/iris-coverage-setup` must have been run (bbsiz set, IRIS restarted, `$zu(84)` verified).

## Invoke

```text
/iris-coverage-run <test-path> [classes...]
```

Example — fhir-017 full suite:

```text
/iris-coverage-run Test.HS.FHIRServer.VectorSearch \
  HS.FHIRServer.VectorSearchParameter \
  HS.FHIRServer.Storage.Json.SearchColumn \
  HS.FHIRServer.Storage.SearchTableBuilder \
  HS.FHIRServer.Storage.JsonAdvSQL.Indexer \
  HS.FHIRServer.Storage.JsonAdvSQL.Search \
  HS.FHIRServer.API.Data.QueryParameters \
  HS.FHIRServer.Tools.CapabilityStatementBuilder
```

## What this does

1. Stops any leftover monitor session (`Stop()` is idempotent)
2. Builds `%List` of INT routine names: each class name + `.1` suffix
3. `%Monitor.System.LineByLine.Start(routines, "", "")`
4. `%UnitTest.Manager.RunTest(testPath, "/nodelete")`
5. `%Monitor.System.LineByLine.Stop()`
6. For each routine: `%ResultSet("%Monitor.System.LineByLine:Result").Execute(rtn)`
   - Each row: `%List(lineNum, execCount, clockTime, totalTime)`
   - `execCount = -1` → non-executable line (skip — not in denominator)
   - `execCount = 0` → executable but not hit (miss)
   - `execCount > 0` → hit
7. Print table: routine name, hit, total, pct
8. Print PASS/FAIL vs 90% target

## CoverageRunner reference implementation

The `Test.HS.FHIRServer.VectorSearch.CoverageRunner` class in fhir-017 is the canonical implementation.
Run it directly for fhir-017:

```objectscript
// Full suite
do ##class(Test.HS.FHIRServer.VectorSearch.CoverageRunner).Run()

// Unit tests only
do ##class(Test.HS.FHIRServer.VectorSearch.CoverageRunner).Run("/unit")

// Integration tests only (requires FixAuth + seeded data)
do ##class(Test.HS.FHIRServer.VectorSearch.CoverageRunner).Run("/integ")
```

## Template for any project

```objectscript
// 1. Stop any leftover monitor
do ##class(%Monitor.System.LineByLine).Stop()

// 2. Build routine list — INT names (ClassName.1)
set routines = $lb(
    "MyApp.ClassA.1",
    "MyApp.ClassB.1"
)

// 3. Start
set sc = ##class(%Monitor.System.LineByLine).Start(routines, "", "")
if $$$ISERR(sc) {
    write "ERROR: ", $system.Status.GetErrorText(sc), !
    quit
}

// 4. Run tests — use compiled class pattern with /noload
// Tests must already be compiled. /noload skips disk load; /nodelete keeps results.
do ##class(%UnitTest.Manager).RunTest("My.Test.Package", "/noload/nodelete")

// 5. Stop
do ##class(%Monitor.System.LineByLine).Stop()

// 6. Collect results
set totalAll = 0, hitAll = 0
set ptr = 0
while $listnext(routines, ptr, rtn) {
    set rset = ##class(%ResultSet).%New("%Monitor.System.LineByLine:Result")
    do rset.Execute(rtn)
    set total = 0, hit = 0
    while rset.Next() {
        set data = rset.GetData(1)
        set execCount = $listget(data, 2)
        if execCount < 0 { continue }   // non-executable
        set total = total + 1
        if execCount > 0 { set hit = hit + 1 }
    }
    set totalAll = totalAll + total
    set hitAll = hitAll + hit
    write rtn, ": ", hit, "/", total
    if total > 0 { write " = ", $fnumber(hit/total*100,"",1), "%" }
    write !
}
write "TOTAL: ", hitAll, "/", totalAll
if totalAll > 0 { write " = ", $fnumber(hitAll/totalAll*100,"",1), "%" }
write !
```

## Key caveats

**Same-process requirement**: The result query MUST run in the same IRIS process that
called `Stop()`. In practice this means one `iris session` or `iris_execute` call that
does start → test → stop → query. Don't split across calls.

**Monitor is global**: Only one process can hold the monitor. If another process has it,
`Start()` returns `ERROR #6060`. Fix: `do ##class(%Monitor.System.LineByLine).Stop()`.

**TBLP 127 incompatibility**: `$zu(84,0,1,1,1,1,1,1)` throws `<FUNCTION>` → monitor
not available. Run on dpgenai1 (sqlt146 image) or wait for AI build. See `/iris-coverage-setup`.

**UnitTest output noise is expected**: `%UnitTest.Manager` writes pass/fail output during
the monitor window. This doesn't affect coverage counts.

**Non-executable lines are NOT counted**: Labels, comments, blank lines, class/method
declarations all return `execCount = -1`. Only executable ObjectScript statements count.
Coverage = hit executable lines / total executable lines.

## Expected output format

```text
=== COVERAGE REPORT ===
                                              Routine   Hit  Total     Pct
-------------------------------------------------------------------------
                HS.FHIRServer.VectorSearchParameter    12     15   80.0%
           HS.FHIRServer.Storage.Json.SearchColumn     18     20   90.0%
         HS.FHIRServer.Storage.SearchTableBuilder       8      9   88.9%
         HS.FHIRServer.Storage.JsonAdvSQL.Indexer      22     25   88.0%
           HS.FHIRServer.Storage.JsonAdvSQL.Search     34     38   89.5%
              HS.FHIRServer.API.Data.QueryParameters   41     45   91.1%
      HS.FHIRServer.Tools.CapabilityStatementBuilder   14     15   93.3%
-------------------------------------------------------------------------
                                                TOTAL  149    167   89.2%

BELOW TARGET — 89.2% (need 0.8% more)
```
