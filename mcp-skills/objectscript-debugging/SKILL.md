---
name: objectscript-debugging
description: Captures IRIS diagnostic packets, maps .INT offsets to .CLS source lines, and correlates error logs. Use whenever an IRIS runtime error or compile failure needs to be diagnosed.
license: MIT
metadata:
  version: "1.0.0"
  author: InterSystems Developer Community
  compatibility: objectscript, iris, healthconnect
  spec: 011-debugging-analysis
---

## Purpose
Ground AI debugging in real IRIS signals — compiler errors, `messages.log` entries, stack traces, and local variable state — rather than guessing from code alone.

## When to Use
- Any `<UNDEFINED>`, `<SUBSCRIPT>`, `<PROTECT>`, or other IRIS runtime error
- Compile failures where the error message references a `.INT` offset (e.g. `+3^MyApp.Foo.1`)
- Debugging a Business Operation or Production component that is failing silently
- Correlating multiple error log entries to a single root cause

## Workflow

### Step 0 — Read the actual class source before guessing
```
# Preferred: structured method signatures + inheritance chain
docs_introspect(class_name="MyPackage.MyClass")
docs_introspect(class_name="%ASQ.Engine")   # works on system classes too

# When you need full raw source with macros (e.g. $$$DISPATCH, #define):
# Export from IRIS, then read the file:
docker exec <container> iris session IRIS -U USER \
  "set sc = \$system.OBJ.ExportUDL(\"MyClass.cls\",\"/tmp/out.cls\") halt"
docker cp <container>:/tmp/out.cls /tmp/out.cls
# Then use the Read tool on /tmp/out.cls — NEVER cat it
```

**The .INT source is not readable via $TEXT on system classes** (stored as object code). Use ExportUDL to get the .cls source instead.

### Step 1 — Capture Diagnostic Packet
Call `debug_capture_packet` to snapshot the current IRIS error state:
```
Tool: debug_capture_packet
Inputs: namespace (default USER), include_locals (default true)
Returns: error_message, stack trace, redacted local variables, timestamp
```
Always capture before attempting a fix — the packet is your ground truth.

### Step 2 — Map .INT Offset to .CLS Source
If the error references a `.INT` routine offset, resolve it to the original `.CLS` source line:
```
Tool: debug_map_int_to_cls
Inputs: error_string (e.g. "<UNDEFINED>x+3^MyApp.Foo.1") OR (routine, offset)
Returns: class name, method name, line number, source snippet
```
You can pass the raw IRIS error string directly — the tool parses it automatically.

### Step 3 — Read Error Logs
Pull recent errors from `messages.log` and `^ERRORS` global:
```
Tool: objectscript_debug_get_error_logs
Inputs: time_window_hours (default 24), max_entries (default 100)
Returns: unified log with timestamps, error codes, namespaces
```

### Step 4 — Correlate and Fix

### If IRIS tools return errors — fix the connection first
```
# Check which containers are available:
iris_list_containers()

# Connect to the right one (no restart needed):
iris_select_container(name="arno_iris_test")
iris_select_container(name="arno_iris_test", password="SYS2")  # if password changed

# Then retry the failing tool
```
Cross-reference the packet, source mapping, and logs to identify root cause. Apply fix. Recompile. Verify no new errors appear in logs.

## Common Error Patterns

| Error | Likely cause | Fix pattern |
|---|---|---|
| `<UNDEFINED>` | Variable used before SET | Check method entry path; add default |
| `<SUBSCRIPT>` | Global subscript too long | Truncate or hash the key |
| `<PROTECT>` | Privilege error on global | Check resource definitions |
| `ERROR #5002` | ObjectScript compilation error | Read compiler output line by line |
| `ERROR #5659` | Method signature mismatch | Check superclass method signature |

## Output Format

After diagnosis:
> 🔍 IRIS diagnostic packet captured: `<packet-id>`
> **Error**: `<error_message>`
> **Source**: `<ClassName>:<MethodName>` line `<N>`
> **Root cause**: [explanation]
> **Fix**: [proposed change]

## Benchmark-Observed Pattern: SQL Table Name Red Herring

**Symptom**: Embedded SQL compiles but returns wrong/empty results. Agent spends 20+ tool calls investigating SQLCODE, %Get patterns, cached queries — never finds root cause.

**Root cause**: Wrong SQL table name derived from class name.

**Fastest diagnostic step** (do this first when SQL gives unexpected results):
```objectscript
// Check the ACTUAL SQL table name for a class
Set rs = ##class(%SQL.Statement).%ExecDirect(,
  "SELECT SqlTableName FROM %Dictionary.CompiledClass WHERE Name = ?",
  "Bench.Patient")
If rs.%Next() { write rs.SqlTableName }
// Output: "Bench.Patient" — use THIS in your SQL, not "Bench_Patient"
```

```objectscript
// WRONG (common agent mistake when class has 2-level name):
&sql(SELECT COUNT(*) INTO :n FROM Bench_Patient)

// CORRECT:
&sql(SELECT COUNT(*) INTO :n FROM Bench.Patient)
```

The rule: **last dot = schema/table separator; earlier dots → underscores**.
`Bench.Patient` → `Bench.Patient`. `My.Deep.Patient` → `My_Deep.Patient`.
