# Contract: iris_test (enhanced)

**Tool name**: `iris_test`  
**Toolset tier**: Baseline (all tiers)  
**Breaking change**: No — additive. Existing docker path unchanged. New HTTP path activates when `IRIS_CONTAINER` is not set.

---

## Parameters (unchanged + new)

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `pattern` | string | Yes | — | Test class pattern e.g. `"MyApp.Tests"` or `"MyApp.Tests.OrderTest"` |
| `namespace` | string | No | `"USER"` | IRIS namespace containing compiled test classes |
| `timeout` | integer | No | `60` | Max seconds to wait for test run to complete |

---

## Response (HTTP path)

```json
{
  "success": false,
  "total": 15,
  "passed": 12,
  "failed": 2,
  "errors": 1,
  "skipped": 0,
  "duration_ms": 843.2,
  "path": "http",
  "log_id": "iris-1746196800000-a3f2c1b4",
  "test_suites": [
    {
      "name": "MyApp.Tests.OrderTest",
      "tests": 8,
      "failures": 1,
      "errors": 0,
      "duration_ms": 412.1,
      "status": "failed"
    },
    {
      "name": "MyApp.Tests.CustomerTest",
      "tests": 7,
      "failures": 1,
      "errors": 1,
      "duration_ms": 431.1,
      "status": "error"
    }
  ]
}
```

Full per-method detail retrieved via:
```
iris_get_log(id="iris-1746196800000-a3f2c1b4")
```

Returns `test_suites` with `test_cases` arrays:
```json
{
  "success": true,
  "log_id": "iris-1746196800000-a3f2c1b4",
  "result": {
    "test_suites": [
      {
        "name": "MyApp.Tests.OrderTest",
        "test_cases": [
          {
            "name": "TestCreateOrder",
            "class_name": "MyApp.Tests.OrderTest",
            "status": "passed",
            "duration_ms": 12.3,
            "failure_message": null
          },
          {
            "name": "TestDeleteOrder",
            "class_name": "MyApp.Tests.OrderTest",
            "status": "failed",
            "duration_ms": 8.1,
            "failure_message": "AssertEquals failed: expected 0, got 1"
          }
        ]
      }
    ]
  }
}
```

---

## Error Responses

**NO_TESTS_FOUND**:
```json
{
  "success": false,
  "error_code": "NO_TESTS_FOUND",
  "error": "No compiled test classes found matching pattern 'MyApp.Tests' in namespace USER"
}
```

**NAMESPACE_NOT_FOUND**:
```json
{
  "success": false,
  "error_code": "NAMESPACE_NOT_FOUND",
  "error": "Namespace 'MYAPP' does not exist on this IRIS instance"
}
```

**TEST_EXECUTION_ERROR**:
```json
{
  "success": false,
  "error_code": "TEST_EXECUTION_ERROR",
  "error": "RunTest failed: <error text from IRIS>"
}
```

**Degraded mode** (source = globals_fallback — should not occur with SQL approach, kept for resilience):
```json
{
  "success": false,
  "total": 15,
  "passed": 12,
  "failed": 3,
  "errors": 0,
  "skipped": 0,
  "duration_ms": null,
  "path": "http",
  "source": "globals_fallback",
  "log_id": "iris-1746196800000-b4c5d6e7",
  "test_suites": [...]
}
```

---

## Path Routing Logic

```
IRIS_CONTAINER set? 
  → YES: docker exec path (existing behavior)
       → docker exec fails? → HTTP fallback (path: "http_fallback")
  → NO: HTTP path (path: "http")
       → SQL query fails? → partial result with available data
```
