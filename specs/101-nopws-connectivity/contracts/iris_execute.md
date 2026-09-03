# MCP Tool Contract: iris_execute (updated)

## New output field: execution_path

All `iris_execute` response branches gain `execution_path`:

```json
{
  "success": true,
  "output": "...",
  "execution_path": "atelier",
  "method": "http"
}
```

`execution_path` values:
- `"atelier"` — HTTP execution via Atelier REST (execute_via_generator)
- `"docker_exec_local"` — docker exec on local machine
- `"docker_exec_ssh"` — docker exec via SSH (`ssh_host` configured)

`"method"` kept for backward compatibility (Constitution V). `execution_path` is the new canonical field.

## Backward Compatibility

`execution_path` is additive. All existing fields unchanged. Existing callers not using `execution_path` are unaffected.
