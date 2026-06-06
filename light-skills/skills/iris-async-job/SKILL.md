---
name: tdyar/iris-async-job
description: Use when running long-running IRIS operations via MCP tools — %UnitTest suites, batch jobs, queries that exceed the 30s HTTP timeout. Covers the Job+global async pattern: start a background job via iris_execute, return immediately, poll with a second call. Also covers the ISC system convention for PID+starttime tracking.
license: MIT
compatibility: objectscript, iris, docker
state: draft
iris_version: ">=2022.1"
tags: [iris, unittest, async, job, background, testing, mcp, timeout]
---

# IRIS Background Job + Poll Pattern

## The problem

`iris_execute` times out at **30s** (HTTP path). `iris_test` times out at **60s**. Long `%UnitTest` suites, batch compiles, and diagnostic collectors exceed this:

```
"error": "execution timed out after 30s"
```

The docker exec fallback (requires `container =` in `.iris-agentic-dev.toml`) has no HTTP timeout but still **blocks** the MCP call — the agent's context window is tied up for the full duration.

## The solution: two-call async pattern

**Call 1** — start the work in a background `Job`, return immediately with PID.  
**Call 2+** — poll a completion global until done.

---

## Pattern 1: Inline (no helper class)

### Start
```objectscript
kill ^zJob("done"), ^zJob("pid"), ^zJob("sc")
set ^zJob("done") = 0

// Job sets $T=1 and $zc=child PID on success; ::30 = 30s wait for a free slot
job ##class(%UnitTest.Manager).RunTest("MyPkg.MyTests", "/nodelete/noload")::30
if $T {
    set ^zJob("pid")     = $zc
    set ^zJob("started") = $h    // store $h alongside PID (PID alone is reusable after restart)
    write "started pid=", $zc, !
} else {
    write "Job failed — no process slot", !
}
```

The background job needs to set the done flag itself. Use a wrapper method for this:
```objectscript
// Helper method or label that wraps the real work:
//   do ##class(%UnitTest.Manager).RunTest("MyPkg.MyTests", "/nodelete/noload")
//   set ^zJob("done") = 1
// Job this wrapper, not the Manager directly.
```

### Poll
```objectscript
// Call this repeatedly until status is not "running"
set pid    = +$get(^zJob("pid"), 0)
set isDone = +$get(^zJob("done"), 0)
if isDone {
    write "DONE", !
} elseif pid, ##class(%SYS.ProcessQuery).%OpenId(pid) '= "" {
    write "running (pid=", pid, ")", !
} else {
    write "dead — job crashed before setting done flag", !
}
```

---

## Pattern 2: Helper class (recommended)

Write once via `iris_doc`, then call from any session without recompiling:

```objectscript
Class User.AsyncRunner Extends %RegisteredObject
{

/// Start a %UnitTest suite in a background job.
/// Returns 1=started, 0=Job command failed (no free slots).
ClassMethod StartTest(testClass As %String) As %Boolean
{
    kill ^zAsyncRun
    set ^zAsyncRun("done") = 0
    set ^zAsyncRun("class") = testClass
    job ##class(User.AsyncRunner).Background(testClass)::30
    if '$T quit 0
    set ^zAsyncRun("pid")     = $zc
    set ^zAsyncRun("started") = $h
    quit 1
}

/// Internal — runs in background, sets done flag when finished.
ClassMethod Background(testClass As %String) [ Private ]
{
    set sc = ##class(%UnitTest.Manager).RunTest(testClass, "/nodelete/noload")
    set ^zAsyncRun("sc")   = sc
    set ^zAsyncRun("done") = 1
}

/// Poll for completion.
/// Returns: "running" | "done:1" (pass) | "done:0" (fail) | "dead" (crashed)
ClassMethod Status() As %String
{
    if +$get(^zAsyncRun("done")) {
        quit "done:" _ ($$$ISOK($get(^zAsyncRun("sc"), 1)))
    }
    set pid = +$get(^zAsyncRun("pid"))
    if pid, ##class(%SYS.ProcessQuery).%OpenId(pid) '= "" quit "running"
    quit "dead"
}

}
```

**Start:**
```objectscript
write ##class(User.AsyncRunner).StartTest("ISC.sql.TestShardedVectorDiabolical"), !
// → 1
```

**Poll (call every 10–30s until not "running"):**
```objectscript
write ##class(User.AsyncRunner).Status(), !
// → "running"
// → "done:1"   tests passed
// → "done:0"   tests failed — query %UnitTest_Result tables for detail
// → "dead"     job crashed — check messages.log or ^ERRORS
```

---

## ISC system convention (`^IRIS.SystemPerformance` pattern from source)

The production ISC pattern stores **PID + start time** together and validates both before trusting liveness:

```objectscript
// Start
job doWork()::$$$JOBTIMEOUT    // $$$JOBTIMEOUT default = 10
if $T {
    set pid = $zc
    set startTime = ##class(%SYS.ProcessQuery).%OpenId(pid).StartTime
    set ^MyGlobal("run", runId, "pid")     = pid
    set ^MyGlobal("run", runId, "started") = startTime
}

// Liveness check — guards against PID reuse after IRIS restart
set process = ##class(%SYS.ProcessQuery).%OpenId(pid)
set isAlive = (process '= "") && (process.StartTime = savedStartTime)
```

Why both? After `docker compose down -v` + restart, IRIS reuses low PIDs. A PID that matches the stored number may belong to a completely different process. Storing start time and comparing it catches this.

---

## Reading %UnitTest results after completion

```sql
-- Latest test run summary
SELECT ts.Name, ts.Status, ts.Duration, tc.Name AS TestCase, tc.Status AS CaseStatus
FROM %UnitTest_Result.TestInstance ti
JOIN %UnitTest_Result.TestSuite ts ON ts.TestInstance = ti.Id
JOIN %UnitTest_Result.TestCase tc ON tc.TestSuite = ts.Id
WHERE ti.Id = (SELECT MAX(Id) FROM %UnitTest_Result.TestInstance)
ORDER BY ts.Name, tc.Name
```

```objectscript
// Or via ObjectScript after poll returns "done:*":
set last = ##class(%UnitTest.Result.TestInstance).GetLastId()
set inst = ##class(%UnitTest.Result.TestInstance).%OpenId(last)
write "Status: ", inst.Status, !   // 1=all passed, 0=failures
```

---

## CRITICAL rules

- **`$zc` is the child PID** — only valid immediately after `Job` when `$T=1`. Capture before the next statement; it gets overwritten by subsequent commands.
- **`$T` = 0 means Job failed** — no free process slots, or the 30s slot-wait timed out. Always check `$T`.
- **Done flag must be set by the background job** — not the caller. If the job crashes mid-run, the flag is never set. Detect this via PID liveness check (process gone but done=0 → "dead").
- **Never `hang` inside the poll call** — return immediately, let the agent call again after waiting. A `hang` inside `iris_execute` wastes the HTTP connection and can itself time out.
- **`/nodelete/noload` for RunTest** — prevents `%UnitTest.Manager` from looking for .udl files on disk; required when the class is already compiled in IRIS and you're not loading from a directory.
- **Store `$h` (not a timestamp string) alongside PID** — `$h` is compact and directly comparable; `##class(%SYS.ProcessQuery).%OpenId(pid).StartTime` returns `$h` format too.
