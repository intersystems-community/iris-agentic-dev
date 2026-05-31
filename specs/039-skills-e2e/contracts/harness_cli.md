# Contract: E2E Harness CLI

## Invocation

```bash
python -m tests.e2e.harness [OPTIONS] [TASK_IDS...]
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--scenario` | `all` | `us1` \| `us2` \| `us3` \| `all` |
| `--model` | `openai/gpt-4o-mini` | OpenCode model string |
| `--keep-on-failure` | false | Retain temp dirs on assertion failure |
| `--baseline` | false | Run baseline (no skills) alongside skill run |
| `--output` | `tests/e2e/results/` | Directory for RunResult JSON |
| `--iris-container` | env `IRIS_CONTAINER` | Container name for US2/US3 |
| `--iris-web-port` | env `IRIS_WEB_PORT` | Web port for US2/US3 |

## Required environment variables

| Variable | Description |
|----------|-------------|
| `OPENAI_API_KEY` | Injected into OpenCode via OPENCODE_CONFIG_CONTENT |
| `IRIS_CONTAINER` | Required for US2/US3; US1 skips if absent |
| `IRIS_WEB_PORT` | Required for US2/US3 |

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | All assertions passed |
| 1 | One or more assertions failed |
| 2 | Harness setup error (missing env var, OpenCode not found, IRIS unreachable) |
| 3 | OpenCode process error (non-zero exit, timeout) |

## OpenCode invocation contract

The harness MUST invoke OpenCode as:

```bash
opencode run "{prompt}" \
  --format json \
  --model {model} \
  --dangerously-skip-permissions
```

With environment:
```
OPENCODE_DB=/tmp/opencode-harness-{run_id}.db
OPENCODE_CONFIG_CONTENT={"provider":{"openai":{"options":{"apiKey":"{key}"}}},"skills":{"paths":["{skills_dir}"]}}
```

## README curl contract

The harness MUST validate README curl URLs before installing:
1. HTTP HEAD request to each URL
2. Assert HTTP 200; fail with exit code 2 and URL on non-200
3. Execute the curl command exactly as documented in `light-skills/README.md`

## Event stream contract

The harness parses `--format json` stdout line by line. Each line is a JSON object with `type` field. The harness MUST handle:

- `tool_use` — extract `part.tool`, `part.state.status`, `part.state.output`
- `text` — accumulate `part.text` for code block extraction
- `error` — record and mark task failed
- Unknown event types — silently ignore (forward compatibility)

MCP tools identified by colon in `part.tool` field: `{server}:{tool}`.
Built-in tools have no colon.
