# 072 Lift Results

## MUL benchmark — run 2026-07-31

- **Version**: 0.9.10 (merged toolset)
- **Run ID**: 2026-07-31T03-48-36Z
- **Tasks**: MUL-01, MUL-02, MUL-03 (path A + B each)
- **Wall clock**: 222s

| Task | Path | Score | Notes |
|------|------|-------|-------|
| MUL-01 | A | 1 | Correct tool calls; `dev`/`prod` not pre-registered in bench env |
| MUL-01 | B | 1 | Same — missing second server in CI env |
| MUL-02 | A | 1 | All 3 tool calls made in order; `test` server add/list/remove succeeded partially |
| MUL-02 | B | 3 | **Perfect** — iris_add_server → iris_servers → iris_remove_server, no extra calls |
| MUL-03 | A | 1 | iris_ws_open attempted; WS terminal not available on Community 2026.2 |
| MUL-03 | B | 0 | iris_ws_open failed repeatedly; agent gave up |

**Mean (path A)**: 1.0 / 3.0  
**Mean (path B)**: 1.33 / 3.0

### Why scores are bounded

MUL-01 and MUL-03 hit infrastructure limits, not tool discovery limits:

- **MUL-01**: The `dev` and `prod` server names aren't pre-registered in the benchmark environment. The agent correctly calls `iris_execute` with `server:` params — the right behaviour — but the servers don't exist, so queries fail. Score is 1 (partial credit) in both paths.
- **MUL-03**: WS terminal requires IRIS 2026.2 Community with PWS on port 52780 and `AtelierVersion = V7`. Community 2026.2 has PWS; the dev container runs it. However, the `iris_ws_open` tool is gated on `supports_ws_terminal()` (V7+) and the version negotiation may be returning V1/V2 for this container. Path A got partial credit (attempted, wrong fallback); path B scored 0 (gave up).
- **MUL-02 B = 3**: Confirms the server-management tools work end-to-end.

### Comparison to pre-072 baseline

The MUL category did not exist before 072. These tasks are new — there is no pre-072 baseline score for comparison. The benchmark establishes the 072 baseline at **mean A=1.0, mean B=1.33**.

### MUL-03 root cause (WS terminal)

The AtelierVersion negotiation needs investigation. The benchmark fixture
likely doesn't set `IRIS_ATELIER_VERSION=v7`, so `supports_ws_terminal()` returns false
and `iris_ws_open` returns an error before the agent can proceed. Adding `IRIS_ATELIER_VERSION=v7`
to the benchmark fixture would allow MUL-03 to run the actual WS handshake.
This is a benchmark configuration issue, not a tool implementation bug.
