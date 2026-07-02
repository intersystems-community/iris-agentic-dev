# Contract: MCP tool surface changes for telemetry

No brand-new MCP tools are added for the core recording path — recording happens as a
side effect of every existing tool call (FR-001, unconditional). Two existing/extended
surfaces change shape:

## `agent_history` (existing tool — behavior change, not schema change)

- Today: returns up to 50 most-recent `ToolCallEntry{tool, success, timestamp}` from the
  in-memory ring buffer.
- After this feature: returns up to `IRIS_TELEMETRY_BUFFER_SIZE` (default 5000) most-recent
  `ToolCallRecord{tool, success, duration_ms, timestamp, session_id, params?}` entries —
  superset of today's fields (adds `duration_ms`, `session_id`; `params` only present when
  policy allows). Existing callers reading `tool`/`success`/`timestamp` are unaffected;
  this is additive, not breaking (Output Shape Parity — Constitution V).
- New optional parameter: `session_id` (filter to a specific session; defaults to current
  session, satisfying FR-002's "queryable by session/run identifier" against the *live*
  in-memory view — durable-sink queries beyond the current session's lifetime are the new
  `telemetry_query` tool below, not `agent_history`).

## `agent_stats` (existing tool — no schema change)

- Continues to aggregate over whatever `agent_history`'s in-memory buffer holds now
  (larger bound); no new fields required by this feature's FRs.

## `telemetry_query` (NEW tool)

Queries the **durable** sink (beyond the current process's in-memory buffer), satisfying
FR-002's "queryable... by session/run identifier" requirement for the "process already
exited" case (US2's actual scenario — `agent_history` alone cannot serve this, since it's
scoped to the live process).

**Input**:
```json
{
  "tool_name": "iris_compile",       // optional filter
  "session_id": "a1b2c3d4-...",      // optional filter
  "since": "2026-07-01T00:00:00Z",   // optional, ISO8601
  "until": "2026-07-01T23:59:59Z",   // optional, ISO8601
  "limit": 500                        // optional, default 500, max 5000
}
```

**Output**: `{"records": [ToolCallRecord, ...], "truncated": false}` — `truncated: true`
when more matching records exist than `limit` allowed, so callers know to page (no
pagination token in v1 — Assumption/Scope: single-machine, non-centralized scale, per
research.md).

**Category/gate**: `ToolCategory::Query` (read-only) — no environment-guard change needed,
same tier as `agent_history`/`iris_query` explain/count modes.

## `telemetry_export_trace` (NEW tool)

Exports the aggregated `{from, to, via, count, ts}` shape for 058-iris-graph's
`record_trace` ingestion (FR-009/FR-010), satisfying US3.

**Input**:
```json
{
  "session_id": "a1b2c3d4-...",   // optional — defaults to all sessions in the durable sink
  "since": "2026-07-01T00:00:00Z" // optional
}
```

**Output**: `{"traces": [DispatchTraceRecord, ...]}` where each entry is exactly
`{"from": "...", "to": "...", "via": "mcp", "count": N, "ts": "ISO8601"}` — no wrapper
fields beyond `traces`, so a caller can feed the array directly to `iris_graph
record_trace`'s `traces` parameter (058 FR-013) with no transformation.

**Category/gate**: `ToolCategory::Query` — this tool only reads and aggregates already-
recorded data; it does not write to the graph itself (that remains `iris_graph
record_trace`'s job, on the 058 side, when that feature ships).
