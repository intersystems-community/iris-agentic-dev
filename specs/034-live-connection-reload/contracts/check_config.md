# Contract: check_config (new tool)

## Tool Description
Returns the active IRIS connection state without making any network calls. Always succeeds — never returns IRIS_UNREACHABLE. Use to diagnose connection issues, verify hot-reload completed, or confirm which container is active.

## Request Parameters
None required.

## Response — Success (always)
```json
{
  "connected": true,
  "host": "localhost",
  "port": 52780,
  "namespace": "USER",
  "container": "iris-dev-iris",
  "config_file": "~/ws/myproject/.iris-dev.toml",
  "config_loaded_at": "2026-05-08T10:23:44Z",
  "iris_version": "IRIS for UNIX 2025.1 (Build 230.2U)",
  "write_tools_enabled": true,
  "connection_source": "config_file"
}
```

## Response — Not connected
```json
{
  "connected": false,
  "host": "localhost",
  "port": 52773,
  "namespace": "USER",
  "container": null,
  "config_file": null,
  "config_loaded_at": "2026-05-08T10:23:44Z",
  "iris_version": null,
  "write_tools_enabled": true,
  "connection_source": "env_vars"
}
```

## Response — Config parse error (connection preserved from last good load)
```json
{
  "connected": true,
  "...",
  "config_parse_error": "TOML parse error at line 3: unexpected key 'contaner'"
}
```

## connection_source values
| Value | Meaning |
|-------|---------|
| `"config_file"` | Active connection loaded from `.iris-dev.toml` |
| `"env_vars"` | Active connection from IRIS_HOST/IRIS_CONTAINER env vars |
| `"iris_select_container"` | Active connection set by a `iris_select_container` call this session |
| `"auto_discovered"` | Active connection from localhost port scan |
