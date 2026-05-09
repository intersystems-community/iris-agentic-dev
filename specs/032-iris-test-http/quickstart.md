# Quickstart: HTTP-Native Unit Test Runner

## Running Tests Without Docker

Set `IRIS_HOST` and `IRIS_WEB_PORT` but NOT `IRIS_CONTAINER`:

```bash
IRIS_HOST=localhost IRIS_WEB_PORT=52773 iris-dev mcp
```

Then in your MCP session:

```
iris_test(pattern="MyApp.Tests", namespace="USER")
```

Response (success case):
```json
{
  "success": true,
  "total": 15,
  "passed": 15,
  "failed": 0,
  "errors": 0,
  "duration_ms": 843.2,
  "path": "http",
  "log_id": "iris-1746196800000-a3f2c1b4",
  "test_suites": [
    {"name": "MyApp.Tests.OrderTest", "tests": 8, "failures": 0, "errors": 0, "status": "passed"},
    {"name": "MyApp.Tests.CustomerTest", "tests": 7, "failures": 0, "errors": 0, "status": "passed"}
  ]
}
```

To get per-method detail:
```
iris_get_log(id="iris-1746196800000-a3f2c1b4")
```

## With Docker (Unchanged Behavior)

```bash
IRIS_CONTAINER=my-iris iris-dev mcp
```

`iris_test` uses docker exec path automatically. If docker fails, falls back to HTTP.

## Path Routing

| `IRIS_CONTAINER` | Docker accessible | Path used |
|------------------|-------------------|-----------|
| Not set | — | `http` |
| Set | Yes | `docker` |
| Set | No | `http_fallback` |

## Diagnosing Failures

When `success: false`, check `test_suites` for which suite failed, then:

```
iris_get_log(id="<log_id>")
```

Look at `test_cases` within the failing suite for `failure_message` on each failed test.
