# Contract: iris_reload_pool

## Tool: `iris_reload_pool`

### Input

No parameters required.

```json
{}
```

### Output — success

```json
{
  "success": true,
  "servers_loaded": 2,
  "servers": ["dev-iris", "prod-iris"],
  "note": "To see new servers in the model's tool list, restart Claude Desktop (or re-run initialize)."
}
```

### Output — parse error (fail-safe)

```json
{
  "success": false,
  "error_code": "TOML_PARSE_ERROR",
  "error": "TOML parse error at line 12: ...",
  "note": "Existing pool preserved — no servers were removed."
}
```

### Output — no config file

```json
{
  "success": true,
  "servers_loaded": 0,
  "servers": [],
  "note": "No config file found. Pool is empty but valid. To see new servers in the model's tool list, restart Claude Desktop (or re-run initialize)."
}
```

## Invariants

1. On parse error, the existing pool is NEVER replaced — `iris_servers` returns the same list before and after.
2. `servers` array lists names from the newly loaded pool (or old pool if reload failed).
3. `note` field always present — explains the MCP protocol limitation on tool-list refresh.
4. The pool swap is atomic — no tool call sees a partially-built pool.

## Background reload (ConfigWatcher path)

When `check_reload` detects a mtime change and swaps the pool:

- No response is produced (background, not in response to a tool call)
- Pool swap uses same fail-safe: old pool preserved on parse error
- Log message emitted on swap (info level)
