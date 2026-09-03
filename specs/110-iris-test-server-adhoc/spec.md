# Feature Specification: iris_test_server Ad-hoc Probe

**Feature Branch**: `098-iris-test-server-adhoc`
**Created**: 2026-09-02
**Status**: Draft

## Overview

`iris_test_server` only tests servers already registered in the pool by name. An agent
that wants to validate a new server's connection details before adding it must use a
painful workaround: call `iris_add_server`, call `iris_test_server`, then call
`iris_remove_server` if it failed. On a NoPWS fleet this is especially disruptive —
adding and then removing a bad entry pollutes the config.

Adding optional `host`, `port`, `username`, `password`, and `namespace` parameters lets
callers probe any target on demand. When these parameters are provided (and no
`server_name` is given), `iris_test_server` performs an ad-hoc connection attempt,
authenticates, and verifies namespace access — then returns the same diagnostic output as
the named-server path. The server is never written to config. NoPWS detection is
included: if the probe fails with connection refused, the error suggests setting
`nopws = true` if the target is known to be an AI-branch container.

---

## User Scenarios & Testing

### User Story 1 — Validate connection details before adding (Priority: P1)

An agent is configuring iad for a customer's AI Hub EAP environment. The customer
provides `host`, `port`, `username`, and `password`. The agent wants to verify these
details work before committing them to the server pool.

Today: the agent must add the server, test it, then remove it on failure.
After this fix: one `iris_test_server(host=..., port=..., username=..., password=...)`
call probes the target and returns a pass/fail with latency — no config change.

**Acceptance Scenarios**:

1. Given valid credentials for a live IRIS instance, When `iris_test_server` is called
   with `host`, `port`, `username`, `password`, and no `server_name`, Then the response
   has `reachable: true`, a non-null `latency_ms`, and `atelier_version`.
2. Given an incorrect password, When `iris_test_server` is called ad-hoc, Then the
   response has `reachable: false` and `error` describing authentication failure.
3. Given a host that is unreachable (wrong IP, firewall), When `iris_test_server` is
   called ad-hoc, Then the response has `reachable: false` and `error` describing
   the connection failure.
4. After calling `iris_test_server` ad-hoc, When `iris_servers` is called, Then the
   probed host is not in the server list — no side effects.

### User Story 2 — NoPWS detection on ad-hoc probe (Priority: P1)

An agent probing a fresh AI Hub EAP container gets "connection refused" on port 52773.
The response should suggest `nopws = true` rather than leaving the agent to guess.

**Acceptance Scenarios**:

1. Given a NoPWS host (connection refused on web port), When `iris_test_server` is
   called ad-hoc, Then the error message includes a suggestion to set `nopws = true`
   and a reference to the `nopws-setup` skill.
2. Given a host that is genuinely down (not NoPWS — times out rather than refusing),
   When `iris_test_server` is called ad-hoc, Then the NoPWS suggestion does not appear
   — only connection-refused errors trigger it.

### User Story 3 — Namespace access verification (Priority: P2)

The agent wants to confirm not just that the server responds but that the specified
namespace is accessible with the given credentials.

**Acceptance Scenarios**:

1. Given a valid host/port/credentials but a namespace the user has no access to, When
   `iris_test_server` is called ad-hoc with that namespace, Then `reachable: true` but
   `namespace_accessible: false` with a descriptive error.
2. Given valid credentials and an accessible namespace, When `iris_test_server` is
   called ad-hoc, Then `namespace_accessible: true` appears in the response.
3. Given no `namespace` parameter in the ad-hoc call, Then namespace verification is
   skipped — `namespace_accessible` is absent from the response.

---

## Functional Requirements

- **FR-001**: `iris_test_server` gains optional parameters: `host` (string), `port`
  (u16), `username` (string), `password` (string), `namespace` (string). All five are
  optional and independent of `server_name`.
- **FR-002**: Dispatch rule:
  - `server_name` provided, no ad-hoc params → existing named-server path (no change).
  - Ad-hoc params provided, no `server_name` → ad-hoc probe path.
  - Both provided → error: `AMBIGUOUS_PARAMS` — caller must use one form or the other.
  - Neither provided → error: `MISSING_PARAMS` — at least one identifying form required.
- **FR-003**: The ad-hoc probe performs a `GET /api/atelier/` with the supplied
  credentials and measures latency, identical to the named-server probe.
- **FR-004**: If the connection fails with "connection refused" (TCP RST or immediate
  refusal, not timeout), the error message includes:
  `"Connection refused on port <port>. If this is an AI-branch IRIS container (irishealth-ai, iris-ai), it may be a NoPWS build — set nopws=true in the server config. See skill: nopws-setup."`
- **FR-005**: When `namespace` is provided and the Atelier probe succeeds, perform a
  namespace verification step: `GET /api/atelier/v1/<NAMESPACE>/docnames/CLS/` or
  equivalent lightweight call. Return `namespace_accessible: true/false` and an error
  string on failure.
- **FR-006**: The ad-hoc response shape mirrors the named-server response:
  `{reachable, latency_ms, atelier_version, iris_version, namespace_accessible?,
error?}`. The `name` field is absent (no pool name for an ad-hoc target) or set to
  `"<adhoc>"`.
- **FR-007**: No server entry is written to `servers.json` or the pool at any point
  during or after an ad-hoc probe. The probe is fully read-only with respect to config.
- **FR-008**: Ad-hoc probe respects the same 5-second timeout as the named-server path.

---

## Key Entities

- **`iris_test_server`** (tools/mod.rs, line ~7621): extend `TestServerParams` with
  optional ad-hoc fields; add dispatch logic; add NoPWS error hint.
- **`TestServerParams`** (server_tools, tools/mod.rs): add `host`, `port`, `username`,
  `password`, `namespace` optional fields.
- **`server_tools` helpers**: extract the Atelier ping into a shared function usable
  from both named-server and ad-hoc paths (also used by spec 094 probe-all logic).

---

## Success Criteria

- An agent can validate arbitrary IRIS connection details with one tool call, with no
  config side effects.
- Connection-refused on a NoPWS host produces an actionable error message, not a
  generic failure.
- Namespace access is verified when a namespace is supplied — a server that is up but
  denies namespace access is reported correctly.
- The named-server path (`server_name` only) is unaffected — no regression.

---

## Out of Scope

- Auto-adding the server to config if the probe succeeds (that is `iris_add_server`'s
  job; the agent decides whether to add).
- Testing docker-only (`docker_only=true`) servers via ad-hoc probe — docker-only
  requires a container name, not just host/port.
- LDAP or Kerberos authentication in the ad-hoc path (basic auth only).
- Bulk ad-hoc probing of multiple targets in one call (use `iris_servers(probe=true)`
  for pool members).

---

## Assumptions

- The caller supplies a password in plaintext in the tool call. This is acceptable for a
  probe-only operation; no credential is persisted.
- Port defaults to `52773` if omitted from the ad-hoc params (standard IRIS web port).
- Namespace defaults are not applied — if no namespace is given, namespace verification
  is skipped entirely rather than guessing `USER`.
