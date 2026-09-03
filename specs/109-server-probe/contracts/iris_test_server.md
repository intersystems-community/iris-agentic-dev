# MCP Tool Contract: iris_test_server (updated)

## Input Schema

```json
{
  "name": { "type": "string", "description": "Named server from pool. Omit when using ad-hoc host." },
  "host": { "type": "string", "description": "Ad-hoc hostname/IP. When provided, skips pool lookup." },
  "web_port": { "type": "integer", "default": 52773 },
  "username": { "type": "string", "default": "_SYSTEM" },
  "password": { "type": "string", "default": "SYS" }
}
```

All fields optional. Validation:
- `host` present → ad-hoc probe
- `name` present, no `host` → named-server probe (existing)
- neither → error `MISSING_PARAMS`

## Output Schema (success)

```json
{
  "reachable": true,
  "auth": true,
  "iris_version": "IRIS for UNIX (Ubuntu Server LTS) 2026.2 ...",
  "atelier_version": "1.5.0",
  "namespace": "USER",
  "latency_ms": 42,
  "error": null
}
```

## Output Schema (unreachable)

```json
{
  "reachable": false,
  "auth": false,
  "iris_version": null,
  "latency_ms": null,
  "error": "connection refused"
}
```

## Output Schema (reachable, auth failed)

```json
{
  "reachable": true,
  "auth": false,
  "iris_version": null,
  "latency_ms": 8,
  "error": "HTTP 401 Unauthorized"
}
```

## Backward Compatibility

Named-server path (`name` only) produces identical output to today. No breaking change.
