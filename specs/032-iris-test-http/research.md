# Research: HTTP-Native Unit Test Runner

**Feature**: 032-iris-test-http
**Date**: 2026-05-07
**Status**: Complete

## Decisions

| Decision | Choice | Rationale | Alternatives Considered |
|----------|--------|-----------|------------------------|
| How to run tests over HTTP | `execute_via_generator` with `RunTest()` + `/noload/run` qualifier | Already the established pattern for HTTP ObjectScript execution; works on all IRIS versions | docker exec (requires IRIS_CONTAINER); Atelier REST has no unit-test endpoint |
| How to read results | Query `%UnitTest.Result.*` SQL tables after run | Direct SQL query over HTTP via `iris.query()` — reliable, structured, no temp files needed for results | jUnit XML file write + read (fragile, requires `/junit` qualifier support); parse stdout (only gives counts) |
| jUnit `/junit` qualifier | **NOT USED** — not reliably available | `RunTest()` signature is `(&testspec, qspec, &userparam)` where `qspec` is a string of flags. `/junit` is not a documented standard qualifier in the available IRIS version. Querying `%UnitTest.Result` tables is more reliable and portable. | `/junit=path` qualifier (undocumented, version-dependent) |
| Temp file for test run ID | `/tmp/irisd_{uuid}_test.txt` pattern (existing convention) | Matches codebase convention from `execute_via_generator`. UUID ensures no concurrent collisions. | `%File.TempFilename()` — not used anywhere in codebase; manual pattern preferred |
| Progressive disclosure integration | `apply_truncation()` on `test_cases` array | Same pattern as iris_compile/iris_search. Store full per-test detail in log store; return suite summaries inline. | Always return full detail (large test suites would flood context) |
| Fallback when results not queryable | `^UnitTestRoot` is a PATH string, not a results store | **CRITICAL**: `^UnitTestRoot` is `Set ^UnitTestRoot = "/path/to/tests"` — it is not a results global. Fallback is: re-query `%UnitTest.Result` with the TestInstance ID. If that fails, return partial results from stdout parsing. | No fallback (bad UX for transient SQL errors) |
| Toolset tier | Baseline (all tiers) | Spec clarification Q3: HTTP path enhances existing `iris_test` which is in Baseline. All users benefit. | Merged only |

## API Verification (Constitution Principle II)

All verified against `iris-dev-iris` (IRIS Community 2025.1):

### `%UnitTest.Manager.RunTest(testspec, qspec, userparam)`
- **Verified**: Method exists, signature confirmed
- `testspec` — test class pattern (e.g. `"MyApp.Tests"`)  
- `qspec` — qualifier string: `/noload/run` (run already-compiled tests without loading from filesystem)
- Returns: status code

### `%UnitTest.Result.*` Schema (verified by SQL query)

**`%UnitTest.Result.TestSuite`**:
- `Name: %String`, `Status: %Integer`, `Duration: %Numeric`
- `ErrorAction: %String`, `ErrorDescription: %String`
- `TestCases → %UnitTest.Result.TestCase` (collection)
- `TestInstance → %UnitTest.Result.TestInstance`

**`%UnitTest.Result.TestCase`**:
- `Name: %String`, `Status: %Integer`, `Duration: %Numeric`
- `ErrorAction: %String`, `ErrorDescription: %String`
- `TestMethods → %UnitTest.Result.TestMethod` (collection)

**`%UnitTest.Result.TestMethod`**:
- `Name: %String`, `Status: %Integer`, `Duration: %Numeric`
- `ErrorAction: %String`, `ErrorDescription: %String`
- `TestAsserts → %UnitTest.Result.TestAssert` (collection)

**`%UnitTest.Result.TestAssert`**:
- `Action: %String`, `Counter: %Integer`, `Description: %String`
- `Location: %String`, `Status: %Integer`

**`%UnitTest.Result.TestInstance`**:
- Contains metadata for the overall test run

### Status Integer Mapping (verified by inspection)
- `1` = Passed
- `0` = Failed  
- Negative or error codes = Error/unexpected failure

### `apply_truncation()` signature (verified in log_store.rs)
```rust
pub fn apply_truncation(
    result: &mut Value,
    items_key: &str,   // key of array to truncate
    threshold: usize,
    inline: bool,
    store: &Arc<Mutex<LogStore>>,
    tool: &str,
)
```
Mutates result in-place; adds `truncated`, `log_id`, `inline_count`, `total_count`.

### `execute_via_generator()` (verified in connection.rs)
```rust
pub async fn execute_via_generator(
    &self, code: &str, namespace: &str, client: &reqwest::Client
) -> anyhow::Result<String>
```
Returns captured `Write` output as String.

## Implementation Approach

### HTTP Path Flow
1. Call `RunTest(pattern, "/noload/run")` via `execute_via_generator`
2. After run completes, query `%UnitTest.Result.TestSuite` + children via SQL to get structured results
3. Parse into TestRun → TestSuite → TestCase → TestMethod hierarchy
4. Apply progressive disclosure: store full `test_cases` in log store, return suite summaries inline

### SQL Query for Results
```sql
-- Get the most recent TestInstance for this namespace
SELECT TOP 1 ID FROM %UnitTest_Result.TestInstance ORDER BY %ID DESC

-- Get suites for that instance  
SELECT ID, Name, Status, Duration, ErrorDescription 
FROM %UnitTest_Result.TestSuite 
WHERE TestInstance = ?

-- Get test methods for a suite
SELECT ID, Name, Status, Duration, ErrorDescription
FROM %UnitTest_Result.TestMethod
WHERE TestCase IN (
  SELECT ID FROM %UnitTest_Result.TestCase WHERE TestSuite = ?
)
```

Note: SQL table names use underscore (`%UnitTest_Result.TestSuite`) not dot notation.

### New Error Codes
- `NO_TESTS_FOUND` — pattern matched zero test classes
- `NAMESPACE_NOT_FOUND` — namespace does not exist
- `TEST_EXECUTION_ERROR` — RunTest itself failed (not assertion failures)

## Known Limitations
- `^UnitTestRoot` is NOT a results global — it stores the filesystem path for test loading. The fallback approach (re-query `%UnitTest.Result`) is superior.
- The `/junit` qualifier for XML output is not reliably available across IRIS versions. Direct SQL query is more portable and doesn't require file I/O.
- Very large test suites (1000+ test methods) — progressive disclosure handles context flooding via log store.
