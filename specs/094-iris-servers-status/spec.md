# Feature Specification: iris_servers Probe Parameter

**Feature Branch**: `094-iris-servers-status`
**Created**: 2026-09-02
**Status**: Draft

## Overview

`iris_servers` lists pool configuration but does not probe. A misconfigured, unreachable,
or NoPWS server looks identical to a healthy one in the response — all entries show
`reachable: null`. An agent managing a NoPWS fleet like Baystate has no way to distinguish
a connected server from a broken one without calling `iris_test_server` once per entry and
aggregating the results manually.

Adding an optional `probe = true` parameter fires a lightweight health check per server in
parallel and returns `reachable`, `latency_ms`, and `error` inline. NoPWS servers with
`nopws = true` set skip the web-port probe and surface `reachable: null` with a reason.
Default `probe = false` preserves the existing response shape exactly.

---

## User Scenarios & Testing

### User Story 1 — Fleet health check at session start (Priority: P1)

An agent connecting to a multi-instance IRIS fleet wants to know which servers are up
before taking any action. Today it calls `iris_test_server` once per server and merges
the results by hand — N round trips, N tool calls.

After this fix: one `iris_servers(probe=true)` call returns all servers with inline
reachability. The agent can immediately identify the broken entry and report it.

**Acceptance Scenarios**:

1. Given a pool with three servers, When `iris_servers` is called with no `probe` param,
   Then all entries have `reachable: null` — response shape is byte-for-byte identical
   to current behavior.
2. Given a pool with three servers where one is unreachable, When `iris_servers` is
   called with `probe=true`, Then the reachable entries have `reachable: true` and
   a `latency_ms` number; the unreachable entry has `reachable: false` and a non-null
   `error` string.
3. Given a server with `nopws = true` in its config, When `probe=true`, Then its entry
   has `reachable: null` and `probe_skipped: "nopws"` — the probe is not attempted.
4. Given an empty pool, When `probe=true`, Then the response is `{servers: [], count: 0}`
   with no error.
5. Given `probe=true` and a server that times out, Then the entry has
   `reachable: false`, `latency_ms: null`, and an `error` describing the timeout.

### User Story 2 — Surface plaintext-password servers (Priority: P2)

After spec 095 ships, some servers will have passwords stored in config (plaintext
fallback). An agent auditing the fleet wants to know which servers are using fallback
storage so it can recommend migration to Server Manager.

**Acceptance Scenarios**:

1. Given a server added with plaintext fallback (spec 095), When `iris_servers` is
   called, Then that entry includes `has_plaintext_password: true`.
2. Given a server using keychain storage, When `iris_servers` is called, Then
   `has_plaintext_password` is either `false` or absent.

---

## Functional Requirements

- **FR-001**: `iris_servers` gains an optional `probe` boolean parameter (default:
  `false`). Callers that pass no `probe` argument see no change in response shape or
  field names.
- **FR-002**: When `probe=true`, all server probes run concurrently with a per-server
  timeout of 5 seconds. Total wall-clock time is bounded by one timeout, not N times
  the timeout.
- **FR-003**: Each probed server entry in the response includes:
  - `reachable`: `true` or `false`
  - `latency_ms`: integer (round-trip ms) or `null` if unreachable
  - `error`: string or `null` (failure reason when `reachable: false`)
- **FR-004**: Servers with `nopws = true` in their config entry skip the web-port probe.
  Their entry returns `reachable: null` and `probe_skipped: "nopws"`. They are not
  counted as unreachable.
- **FR-005**: The probe reuses the same Atelier REST ping path as `iris_test_server`
  — `GET /api/atelier/` with credentials. No new HTTP client logic.
- **FR-006**: `iris_servers` adds a `has_plaintext_password: bool` field per entry
  (true when the `ServerEntry.password` field is populated, per spec 095). The password
  value itself is never returned.

---

## Key Entities

- **`iris_servers`** (tools/mod.rs, line ~7458): add `probe` param, parallelize probe
  logic, add `has_plaintext_password` field.
- **`ServerEntry`** (iris/servers_config.rs): `nopws` field consulted to skip probe;
  `password` field consulted for `has_plaintext_password`.
- **`iris_test_server`** (tools/mod.rs, line ~7621): probe logic extracted into a shared
  helper so both tools use the same Atelier ping path.

---

## Success Criteria

- A 10-server fleet health check with `probe=true` completes in under 6 seconds.
- `probe=false` (default) response is byte-for-byte identical to current `iris_servers`
  output — zero regression for existing callers.
- NoPWS servers never block the probe timeout; they return immediately with
  `probe_skipped: "nopws"`.
- `has_plaintext_password: true` appears on any server whose `ServerEntry` has a
  populated `password` field.

---

## Out of Scope

- Continuous monitoring or polling (call `iris_test_server` in a loop).
- Changing pool state based on probe results.
- Returning IRIS version or namespace info in the probe (that is `iris_test_server`'s
  job).
- Auto-removing unreachable servers from the pool.

---

## Assumptions

- Spec 095 ships before or with this spec — `ServerEntry.password` must exist for
  `has_plaintext_password` to be meaningful. If 095 is delayed, FR-006 can ship
  independently as a no-op field (`false` always) until 095 lands.
- The `nopws` field on `ServerEntry` is introduced in spec 091. This spec depends on
  it being present and readable from the pool.
