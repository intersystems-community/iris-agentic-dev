# Feature Specification: iris_servers Connection Status

**Feature Branch**: `098-iris-servers-status`
**Created**: 2026-09-02
**Status**: Draft

## Overview

`iris_servers` lists configured servers from the pool but does not probe them. An agent
managing a fleet of IRIS instances cannot tell at a glance which servers are reachable
versus unreachable without calling `iris_test_server` once per server — a separate round
trip for each. This feature adds an optional `probe` parameter that pings all servers in
parallel and returns reachability status inline.

---

## User Scenarios & Testing

### User Story 1 — Fleet health check (Priority: P1)

An agent performing pre-flight checks on a multi-instance IRIS fleet needs to know which
instances are up before taking any action. Today it must call `iris_test_server` once per
server and aggregate the results manually.

**Why this priority**: Single call, parallel probes, immediate answer. This is the
"is anything down?" check that any fleet automation needs at the start of every session.

**Independent Test**: Configure two servers; bring one down; call `iris_servers` with
`probe=true`; assert one entry has `reachable: true` and one has `reachable: false`.

**Acceptance Scenarios**:

1. Given a pool with three configured servers, When `iris_servers` is called with
   `probe=false` (or no probe parameter), Then the response lists all servers with no
   `reachable` field — identical to current behavior.
2. Given a pool with three configured servers, When `iris_servers` is called with
   `probe=true`, Then each server entry includes `reachable` (bool) and `latency_ms`
   (number, null if unreachable).
3. Given a server that times out, When `probe=true`, Then its entry has
   `reachable: false`, `latency_ms: null`, and an `error` string describing the failure.
4. Given an empty pool, When `probe=true`, Then the response is an empty array — no
   error.

---

## Functional Requirements

- **FR-001**: `iris_servers` gains an optional `probe` boolean parameter (default:
  `false`). Existing callers with no `probe` argument see no change in behavior or
  response shape.
- **FR-002**: When `probe=true`, all servers are probed in parallel with a per-server
  timeout of 5 seconds (same as `iris_test_server`).
- **FR-003**: Each server entry in the `probe=true` response includes:
  - `reachable`: bool
  - `latency_ms`: number (round-trip time) or null if unreachable
  - `error`: string or null (failure reason if unreachable)
- **FR-004**: The probe mechanism reuses the same check as `iris_test_server` — Atelier
  REST ping for web-port servers, docker exec ping for `docker_only=true` servers.
- **FR-005**: Total response time is bounded by the single per-server timeout (5s), not
  N × 5s, because probes run in parallel.
- **FR-006**: NoPWS servers (no web port) return `reachable: false` with
  `error: "no web port configured"` unless `docker_only=true` is set, in which case the
  docker exec path is used.

---

## Success Criteria

- A 10-server fleet health check completes in under 6 seconds (5s probe timeout + 1s
  overhead).
- `probe=false` response shape is byte-for-byte identical to the current `iris_servers`
  response — no regression for existing callers.
- A server that is unreachable returns a clear `error` string, not a generic failure.

---

## Out of Scope

- Continuous monitoring / polling (use `iris_test_server` in a loop for that).
- Changing the pool configuration based on probe results.
- Surfacing IRIS version or namespace info in the probe response (that's
  `iris_test_server`'s job).
