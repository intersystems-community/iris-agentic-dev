# MCP Tool Contract: iris_test_server (updated for NoPWS)

## New fields in response

```json
{
  "name": "myserver",
  "reachable": false,
  "nopws": true,
  "web_available": false,
  "nopws_detected": true,
  "nopws_evidence": "WebServer=0",
  "suggestion": "Add to .iris-agentic-dev.toml:\n  nopws = true\n  docker_only = true",
  "error": "NoPWS: this IRIS build has no embedded web server..."
}
```

Fields added (all additive):

- `nopws: bool` — from WorkspaceConfig.nopws config
- `web_available: bool` — true if HTTP probe succeeded (200/401 = available)
- `nopws_detected: bool` — true if auto-detection confirmed WebServer=0
- `nopws_evidence: string | null` — quoted line from iris.cpf if detected
- `suggestion: string | null` — ready-to-paste toml snippet

## Backward Compatibility

All existing fields unchanged. New fields are additive. `reachable` semantics unchanged.
