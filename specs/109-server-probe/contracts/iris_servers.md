# MCP Tool Contract: iris_servers (updated)

## Input Schema (new)

```json
{
  "probe": {
    "type": "boolean",
    "default": false,
    "description": "When true, probe all servers in parallel (5s timeout each)."
  }
}
```

## Output Schema (probe=false, default — unchanged)

```json
{
  "servers": [
    {
      "name": "myserver",
      "host": "localhost",
      "port": 52780,
      "namespace": "USER",
      "username": "_SYSTEM",
      "source": "iad-native",
      "reachable": null
    }
  ]
}
```

## Output Schema (probe=true)

```json
{
  "servers": [
    {
      "name": "myserver",
      "host": "localhost",
      "port": 52780,
      "namespace": "USER",
      "username": "_SYSTEM",
      "source": "iad-native",
      "reachable": true,
      "auth": true,
      "latency_ms": 42,
      "error": null
    },
    {
      "name": "down-server",
      "host": "192.168.1.99",
      "port": 52773,
      "namespace": "USER",
      "username": "_SYSTEM",
      "source": "fleet",
      "reachable": false,
      "auth": false,
      "latency_ms": null,
      "error": "connection refused"
    }
  ]
}
```

## Backward Compatibility

`probe` param is optional with default `false`. Callers not passing `probe` get identical behavior to today — `reachable: null`, no `latency_ms`, no per-entry `error`.
