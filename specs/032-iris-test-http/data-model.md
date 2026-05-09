# Data Model: HTTP-Native Unit Test Runner

## Entities

### TestRun (top-level response)

| Field | Type | Description |
|-------|------|-------------|
| `success` | bool | True if all test methods passed |
| `total` | int | Total test method count across all suites |
| `passed` | int | Count of passed test methods |
| `failed` | int | Count of failed test methods (assertion failures) |
| `errors` | int | Count of errored test methods (unexpected errors) |
| `skipped` | int | Count of skipped test methods (0 if not supported) |
| `duration_ms` | float | Total run duration in milliseconds |
| `path` | string | Execution path: `"http"` \| `"docker"` \| `"http_fallback"` |
| `source` | string or null | Data source: `null` (normal) \| `"globals_fallback"` (degraded) |
| `log_id` | string or null | UUID for full detail retrieval via `iris_get_log` |
| `test_suites` | TestSuite[] | Suite-level summaries (inline, no per-method detail) |

### TestSuite (inline summary — no per-method detail inline)

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Test class name (e.g. `"MyApp.Tests.OrderTest"`) |
| `tests` | int | Total test method count in this suite |
| `failures` | int | Failed test method count |
| `errors` | int | Errored test method count |
| `duration_ms` | float | Suite duration |
| `status` | string | `"passed"` \| `"failed"` \| `"error"` |

### TestSuiteDetail (stored in log store, retrieved via iris_get_log)

Everything in TestSuite plus:

| Field | Type | Description |
|-------|------|-------------|
| `test_cases` | TestCase[] | Full per-method detail |

### TestCase (in log store only)

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Test method name (e.g. `"TestCreateOrder"`) |
| `class_name` | string | Parent class name |
| `status` | string | `"passed"` \| `"failed"` \| `"error"` \| `"skipped"` |
| `duration_ms` | float | Method duration (null in globals_fallback) |
| `failure_message` | string or null | Assertion text for failed/error; null for passed |

## Status Mapping

Maps `%UnitTest.Result` `Status: %Integer` to string:
- `1` → `"passed"`
- `0` → `"failed"` (assertion failure)
- Any other / error during execution → `"error"`

## New Error Codes

| Code | When |
|------|------|
| `NO_TESTS_FOUND` | Pattern matched zero compiled test classes |
| `NAMESPACE_NOT_FOUND` | Specified namespace does not exist |
| `TEST_EXECUTION_ERROR` | `RunTest()` itself failed before tests could run |

## Progressive Disclosure Integration

`iris_test` uses `apply_truncation()` on the `test_cases` array within each TestSuiteDetail stored in the log store. The inline response always returns suite-level summaries only. Full per-case detail is always stored in log store (no threshold — always store for test runs).

Threshold env var: `IRIS_INLINE_TESTS` (default: 0 — always use log store for detail).
