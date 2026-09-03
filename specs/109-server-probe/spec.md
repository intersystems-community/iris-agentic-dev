# Feature Specification: Server Probe

**Feature Branch**: `098-server-probe`
**Created**: 2026-09-02
**Status**: Draft

## Overview

Two gaps in server probing share the same underlying probe logic and can be closed together:

1. **Ad-hoc probe** (`iris_test_server`): currently requires the target server to be in the
   loaded pool (`TestServerParams` has only `name: String`). If the name is not found, it
   returns `SERVER_NOT_FOUND` — there is no way to probe a host/port before adding it to
   the pool and restarting the MCP server.

2. **Fleet health check** (`iris_servers`): lists configured servers from the pool but always
   returns `reachable: null`. An agent must call `iris_test_server` once per server to get
   reachability status — N serial round trips for a fleet of N instances. The probe logic
   (GET `/api/atelier/` with timing) is inlined in `iris_test_server` only and not reusable.

This feature closes both gaps: ad-hoc connection params on `iris_test_server`, and an optional
`probe` parameter on `iris_servers` that fans out to a shared probe function in parallel.

---

## User Scenarios & Testing

### User Story 1 — Probe before adding (Priority: P1)

An agent is setting up a new server connection. Before calling `iris_add_server`, it wants to
verify the host is reachable and credentials are valid — without adding a pool entry first.

**Why this priority**: Without this, the "discover-then-add" workflow is broken. An agent
cannot verify a new server exists before committing to the config. It is also the simpler of
the two changes and unblocks the fleet probe story by extracting the shared probe logic.

**Independent Test**: Start with an empty pool. Call `iris_test_server` with
`host="localhost"`, `web_port=52780`, `username="_SYSTEM"`, `password="SYS"` against the
live iris-dev-iris container. Assert `reachable: true` with `latency_ms` present and
`iris_version` non-null.

**Acceptance Scenarios**:

1. **Given** an empty pool and a live IRIS instance at `localhost:52780`, **When**
   `iris_test_server(host="localhost", web_port=52780, username="_SYSTEM", password="SYS")`
   is called, **Then** it returns `reachable: true` with `iris_version`, `atelier_version`,
   and `latency_ms` — identical shape to a named-server probe.
2. **Given** an unreachable host, **When** called with ad-hoc params, **Then** it returns
   `reachable: false` with a structured `error` string — not a panic or `SERVER_NOT_FOUND`.
3. **Given** both `server` name and ad-hoc `host` param, **When** called, **Then** ad-hoc
   params take precedence and the pool is not consulted.
4. **Given** `iris_test_server(server="existing-server")` with no `host`, **When** called,
   **Then** existing named-server behavior is unchanged.
5. **Given** neither `server` nor `host`, **When** called, **Then** a clear error:
   `"Provide either a server name or host/web_port parameters."` — not SERVER_NOT_FOUND.

---

### User Story 2 — Fleet health check (Priority: P1)

An agent performing pre-flight checks on a multi-instance IRIS fleet needs to know which
instances are up in one call, not N serial calls.

**Why this priority**: Critical for any fleet automation. The existing workaround (N calls to
`iris_test_server`) is serial and slow. Parallel probing with a single `iris_servers(probe=true)`
call is the natural solution once probe logic is extracted from Story 1.

**Independent Test**: Configure two servers in the pool (one up, one down via a closed port).
Call `iris_servers(probe=true)`. Assert the up server has `reachable: true` with `latency_ms`,
and the down server has `reachable: false` with an `error` string. Total elapsed time < 6s.

**Acceptance Scenarios**:

1. **Given** a pool with three servers, **When** `iris_servers` is called with no `probe`
   param (or `probe=false`), **Then** the response is byte-for-byte identical to current
   behavior — `reachable: null`, no `latency_ms`, no `error` per entry.
2. **Given** a pool with three servers, **When** `iris_servers(probe=true)` is called,
   **Then** each entry includes `reachable` (bool), `latency_ms` (number or null), and
   `error` (string or null).
3. **Given** a server that times out after 5s, **When** `probe=true`, **Then** its entry has
   `reachable: false`, `latency_ms: null`, and a non-empty `error` string.
4. **Given** an empty pool, **When** `probe=true`, **Then** the response is an empty `servers`
   array — no error.
5. **Given** a 10-server fleet, **When** `probe=true`, **Then** total response time is
   bounded by the per-server timeout (5s), not 10 × 5s — probes run in parallel.

---

### Edge Cases

- What if `host` is provided but `web_port` is omitted? Default to 52773.
- What if `username` and `password` are omitted from ad-hoc params? Default to `_SYSTEM`/`SYS`.
- What if the pool contains 0 servers and `probe=true`? Return empty array, no error.
- What if a server's credential is missing from the keychain during fleet probe? Probe returns
  `reachable: false` with `error: "credential not found"` — does not crash the batch.
- What if the IRIS instance returns HTTP 401? Report `reachable: true` (network reachable),
  `auth: false`, no `iris_version`. NOTE: This is a **bug fix** to the existing `iris_test_server`
  handler, which currently treats 401 as `reachable: false` (any non-2xx is treated as unreachable
  at mod.rs ~7644). This feature corrects that behavior.

---

## Requirements

### Functional Requirements

- **FR-001**: `iris_test_server` MUST accept optional connection params: `host` (string),
  `web_port` (integer, default 52773), `username` (string, default `_SYSTEM`), `password`
  (string, default `SYS`).
- **FR-002**: When `host` is provided, `iris_test_server` MUST skip pool lookup and probe
  the ad-hoc connection directly.
- **FR-003**: When both `server` name and `host` are provided, ad-hoc params MUST take
  precedence — pool is not consulted.
- **FR-004**: When neither `server` nor `host` is provided, `iris_test_server` MUST return
  a structured error code `MISSING_PARAMS` with message
  `"Provide either a server name or host/web_port parameters."` — not `SERVER_NOT_FOUND`.
- **FR-005**: Ad-hoc probe response shape MUST be identical to named-server probe: `reachable`,
  `auth`, `iris_version`, `namespace`, `atelier_version`, `latency_ms`, `error` (when applicable).
- **FR-006**: `iris_servers` MUST accept an optional `probe` boolean parameter (default:
  `false`). When `probe=false` or omitted, response MUST be identical to current behavior
  — no regression for existing callers.
- **FR-007**: When `probe=true`, `iris_servers` MUST probe all servers in parallel with a
  5-second per-server timeout.
- **FR-008**: When `probe=true`, each server entry MUST include `reachable` (bool),
  `latency_ms` (number or null), `error` (string or null).
- **FR-009**: Total response time for `probe=true` MUST be bounded by one 5s timeout period
  regardless of fleet size — probes run concurrently.
- **FR-010**: The probe logic MUST be extracted into a single shared function called by both
  `iris_test_server` and `iris_servers` — no duplication.

### Key Entities

- **ProbeResult**: outcome of probing one server — `{ reachable: bool, auth: bool,
atelier_version, iris_version, latency_ms: Option<u64>, error: Option<String> }`.
- **TestServerParams** (updated): adds `host`, `web_port`, `username`, `password` as optional
  fields alongside the existing `name`.
- **IrisServersProbeEntry**: a server list entry when `probe=true` — extends the existing
  `{ name, host, port, namespace, username, source }` shape with `reachable`, `latency_ms`,
  `error`.

---

## Success Criteria

### Measurable Outcomes

- **SC-001**: An agent with an empty pool can call `iris_test_server` with connection params
  and receive a valid probe result — without adding a pool entry or restarting the MCP server.
- **SC-002**: The discover-then-add workflow succeeds end-to-end: `iris_test_server` (ad-hoc)
  confirms reachable → `iris_add_server` → server in pool.
- **SC-003**: A 10-server fleet health check via `iris_servers(probe=true)` completes in under
  6 seconds (5s probe timeout + 1s overhead).
- **SC-004**: Calling `iris_servers` without `probe` (or with `probe=false`) produces a
  response with identical structure to today — no field additions, no regressions.
- **SC-005**: Existing named-server `iris_test_server` behavior is unchanged — all current
  tests pass without modification.

---

## Out of Scope

- Persisting an ad-hoc probe result to the pool (use `iris_add_server` for that).
- Continuous monitoring or polling.
- Changing pool configuration based on probe results.
- Surfacing IRIS version or namespace info in `iris_servers probe=true` (that is
  `iris_test_server`'s job per entry).
- SSH tunnel management.
- NoPWS auto-detection in probes (tracked in spec 091).
- Docker exec probe path (covered by NoPWS spec 091).

---

## Assumptions

- `IrisConnection::new(base_url, namespace, username, password, DiscoverySource)` can build
  a valid connection from ad-hoc params using the existing constructor.
- `atelier_url("/")` on the constructed connection produces the correct probe URL.
- The `reqwest::Client` is shared and does not need to be recreated per ad-hoc probe.
- Parallel probe uses `tokio::time::timeout` + `futures::future::join_all` or equivalent
  within the existing async runtime — no new dependencies needed.
- Credential defaults (`_SYSTEM`/`SYS`) match the existing `iris_test_server` behavior for
  unauthenticated probes (HTTP 401 is a valid "reachable but not authenticated" result).
