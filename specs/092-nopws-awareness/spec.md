# Feature Specification: NoPWS Server Awareness

**Feature Branch**: `092-nopws-awareness`
**Created**: 2026-09-02
**Status**: Draft

## Overview

iad assumes every IRIS server exposes Atelier REST on a web port. IRIS builds without
a Private Web Server — `irishealth-ai:*` images, Enterprise 2026.2.0AI (DPP-1192),
and 2026.3.0AI — have no embedded Apache. When the web port is unreachable, iad
returns a generic "connection refused" error with no guidance. This spec adds a
`nopws = true` flag to `ServerEntry` so operators can declare this topology
explicitly. When set, iad skips all web-based health checks and Atelier REST calls
and returns an actionable error naming the two supported paths forward: `docker_only`
for local containers or a WebGateway sidecar for remote access.

---

## User Scenarios & Testing

### User Story 1 — Register a NoPWS AI Hub server (Priority: P0)

A developer is dogfooding AI Hub EAP on Baystate. The IRIS instance is
`irishealth-ai:2026.3.0AI` — no embedded Apache. They add the server via
`iris_add_server` with `nopws: true`. Any tool call targeting that server returns an
immediately actionable error instead of timing out. `iris_servers` shows the entry
with `nopws: true` surfaced so the agent knows the constraint at a glance.

**Acceptance Scenarios**:

1. Given a server registered with `nopws: true`, When `iris_execute` is called
   targeting it, Then the response is `{success: false, error_code: "NOPWS",
message: "This server is configured as NoPWS — no Atelier REST available. Use
docker_only=true for local containers, or deploy a WebGateway sidecar for remote
access."}` — no network timeout, no generic IO error.
2. Given a server registered with `nopws: true`, When a health-check or
   `iris_test_server` call fires, Then the web port is not probed — the response
   reports `reachable: false` with `reason: "nopws"` immediately.
3. Given `iris_servers` is called, When a server has `nopws: true`, Then its entry
   includes `"nopws": true` so the agent can reason about it without calling
   `iris_test_server`.
4. Given a server registered with `nopws: true` and `docker_only: true`, When
   `iris_execute` is called, Then the docker exec path is used (the NoPWS error is
   suppressed when `docker_only` also applies — docker_only is the working escape
   hatch).
5. Given an existing server entry in `servers.json` without a `nopws` field, When
   iad reads it, Then `nopws` defaults to `false` — no behavior change for existing
   entries.

### User Story 2 — Add nopws to workspace TOML (Priority: P1)

A developer checks `.iris-agentic-dev.toml` into the AI Hub repo. They add
`nopws = true` to the TOML. iad reads it and applies the same skip logic for all
tool calls in that workspace session.

**Acceptance Scenarios**:

1. Given `.iris-agentic-dev.toml` contains `nopws = true`, When any Atelier REST
   tool is called, Then the `NOPWS` error is returned immediately without a network
   probe.
2. Given `.iris-agentic-dev.toml` contains `nopws = true` and `docker_only = true`,
   When `iris_execute` is called, Then the docker exec path is taken — `docker_only`
   overrides the dead-end `NOPWS` error.

---

## Functional Requirements

- **FR-001**: `ServerEntry` in `servers_config.rs` gains
  `#[serde(default, skip_serializing_if = "std::ops::Not::not")] pub nopws: bool`.
  Existing entries without the field deserialize as `nopws: false`.
- **FR-002**: `WorkspaceConfig` in `workspace_config.rs` gains `pub nopws: bool`
  with `#[serde(default)]`. The field is read from `.iris-agentic-dev.toml` as
  `nopws = true/false`.
- **FR-003**: When `nopws` is true on the active connection and `docker_only` is
  false, any tool that would issue an Atelier REST call must short-circuit with
  error code `NOPWS` and the message: `"This server is configured as NoPWS — no
Atelier REST available. Use docker_only=true for local containers, or deploy a
WebGateway sidecar for remote access."` No network call is made.
- **FR-004**: The `NOPWS` short-circuit fires at the same check point as the
  existing `docker_only || no_pws` capability gate in `iris_compile` (line 3267)
  and must be applied uniformly to all tools that call Atelier REST.
- **FR-005**: `derive_capabilities` (tools/mod.rs line 2202) gains `nopws` as an
  explicit flag input alongside `docker_only`. When `nopws` is true, `atelier_rest`
  is false in the capability matrix — same outcome as the existing runtime detection,
  but without requiring a successful version probe.
- **FR-006**: `iris_test_server` skips the HTTP probe when `nopws: true` and returns
  `{reachable: false, reason: "nopws", message: "NoPWS — web port not probed"}`.
- **FR-007**: `iris_servers` output includes `"nopws": true` for servers where the
  flag is set (omitted or `false` for all others, matching the
  `skip_serializing_if` pattern).
- **FR-008**: `iris_add_server` accepts a `nopws` boolean parameter. When provided,
  it is written to the `ServerEntry`. Default is `false`.
- **FR-009**: Tool descriptions for `iris_execute`, `iris_compile`, `iris_doc`, and
  `iris_sql` gain a one-line note: `"NoPWS servers (nopws=true) require
docker_only=true or a WebGateway sidecar — Atelier REST is unavailable."`

---

## Key Entities

- **`ServerEntry`** (iris/servers_config.rs): add `pub nopws: bool` with serde
  defaults.
- **`WorkspaceConfig`** (iris/workspace_config.rs): add `pub nopws: bool` with
  `#[serde(default)]`.
- **`derive_capabilities`** (tools/mod.rs): add `nopws: bool` parameter; set
  `atelier_rest = false` when `nopws || docker_only || runtime_no_pws`.
- **`iris_add_server`** (tools/mod.rs): accept and persist `nopws` parameter.
- **`iris_test_server`** (tools/mod.rs): early return when `nopws`.

---

## Success Criteria

- A server with `nopws: true` never causes a network timeout — the `NOPWS` error
  fires synchronously on every Atelier REST tool call.
- `iris_add_server nopws=true` + `iris_servers` shows `nopws: true` on the entry.
- `nopws: true` + `docker_only: true` routes through docker exec — the combination
  is the intended working configuration for local NoPWS containers.
- TOML round-trip test: `nopws = true` in TOML → `WorkspaceConfig.nopws == true`.
- Binary-invocation test: `iris_add_server` with `nopws: true` → `iris_servers`
  shows the flag.
- Existing servers without `nopws` are unaffected — default is `false`.

---

## Out of Scope

- Auto-detecting NoPWS at connection time (the existing runtime `no_pws` detection
  from `$ZVersion` string remains as a second layer but is not the primary path
  for declared NoPWS servers).
- WebGateway sidecar configuration or provisioning.
- SSH tunnel support (separate gap).
- Changing the `docker_only` semantics — this spec adds `nopws` as a distinct,
  composable flag.

---

## Assumptions

- `docker_only = true` + `nopws = true` is the canonical configuration for a local
  NoPWS container (e.g. AI Hub EAP dev loop). The two flags are independent; their
  combination routes all I/O through `docker exec`.
- The NoPWS error message is the agent's primary guide — it must name both escape
  hatches (`docker_only` and WebGateway) so the agent can advise the user without
  additional context.
- Enterprise 2026.2.0AI (DPP-1192) and all `irishealth-ai:*` 2026.3+ images are
  NoPWS. Community builds are not affected.
