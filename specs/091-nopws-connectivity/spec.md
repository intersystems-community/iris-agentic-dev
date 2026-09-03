# Feature Specification: NoPWS Connectivity

**Feature Branch**: `091-nopws-connectivity`
**Created**: 2026-09-02
**Status**: Draft
**Merges**: 091-nopws-awareness + 092-docker-exec-fallback + 094-nopws-setup-skill

## Overview

AI-branch IRIS builds (irishealth-ai:\*, iris-ai:\*, 2026.3+) ship without an embedded
web server (`WebServer=0` in iris.cpf) — called NoPWS (No Private Web Server). iad
assumes every IRIS instance exposes Atelier REST on a web port. When it hits a NoPWS
container, the result is "connection refused" with no explanation and no fallback.

This feature closes the NoPWS gap at three layers:

1. **Detection and config**: a `nopws = true` flag, auto-detection from iris.cpf, and
   actionable error messages.
2. **Execution fallback**: when Atelier REST is unavailable and a container is configured,
   route ObjectScript execution through `docker exec iris session` instead of failing.
3. **Skill**: a bundled knowledge file teaching agents the full NoPWS setup sequence —
   from detection through webgateway sidecar configuration and first-boot password clearing.

---

## User Scenarios & Testing

### User Story 1 — NoPWS config flag and clear error messages (Priority: P1)

An operator configures an AI-branch IRIS server in iad. The web port returns "connection
refused." They need to know this is a NoPWS build, not a misconfiguration, and what to do.

**Acceptance Scenarios**:

1. Given `nopws = true` in server config and web port unreachable, When any tool connects,
   Then the error says "This server is configured as NoPWS (no embedded web server). Use
   `docker_only = true` for local containers or deploy a webgateway sidecar for remote
   access." — not "connection refused."
2. Given `nopws = true`, When `iris_test_server` is called, Then the response includes
   `nopws: true` and `web_available: false` without treating the missing web port as failure.
3. Given no `nopws` flag and a failed web probe, When iad runs auto-detection, Then it
   checks iris.cpf (if container set) and the superserver port before reporting the error.
4. Given `nopws = true` and `docker_only = true`, When any tool runs, Then all calls route
   through `docker exec iris session` — no web port attempt.

### User Story 2 — Automatic docker exec fallback for local containers (Priority: P1)

A developer spins up an AI-branch IRIS container locally. They call `iris_execute`. iad
detects the web port is unreachable, falls back to `docker exec`, and returns the result.

**Acceptance Scenarios**:

1. Given `container` set in config and web port unreachable, When `iris_execute` is called,
   Then iad falls back to docker exec and returns a valid result with `execution_path:
"docker_exec_local"`.
2. Given `docker_only = true`, When any execution tool is called, Then iad skips the web
   probe and routes directly to docker exec.
3. Given web port reachable, When `iris_execute` is called, Then Atelier REST is used as
   before — docker exec is not invoked.
4. Given docker exec fallback in use and ObjectScript returns `<UNDEFINED>`, Then the error
   is surfaced in the tool response — not swallowed.

### User Story 3 — Remote NoPWS container via SSH (Priority: P2)

An operator manages a fleet of IRIS instances on remote hosts. `docker exec` must run on
the remote host via SSH.

**Acceptance Scenarios**:

1. Given `ssh_host` set in config, When docker exec fallback triggers, Then iad runs
   `ssh <ssh_host> docker exec <container> iris session IRIS -U <namespace>`.
2. Given SSH host unreachable, Then the error names the SSH host — not a generic timeout.
3. Given no `ssh_host` and container not found locally, Then the error says "container not
   found locally and no ssh_host configured."

### User Story 4 — NoPWS auto-detection (Priority: P2)

An agent encounters a new IRIS instance without knowing it is NoPWS. iad detects it
automatically and surfaces the finding with remediation guidance.

**Acceptance Scenarios**:

1. Given web port unreachable and superserver port responding, When `iris_test_server` is
   called, Then response includes `nopws_detected: true` and a remediation suggestion.
2. Given a local container where `docker exec` can read iris.cpf, When `iris_test_server`
   is called, Then iad reads `WebServer=0` and reports `nopws_detected: true` with the
   confirming config line.
3. Given auto-detection confirms NoPWS, Then the result includes a ready-to-paste toml
   snippet: `nopws = true` plus `docker_only = true` (local) or webgateway port (remote).

### User Story 5 — NoPWS setup skill (Priority: P1)

An agent hits connection refused on an AI-branch container. It loads the nopws-setup skill
and follows the detection → setup → first-boot sequence with no human intervention beyond
approving Docker commands.

**Acceptance Scenarios**:

1. Given the nopws-setup skill loaded, When the agent hits connection refused on an
   AI-branch web port, Then it identifies probable NoPWS and follows the detection step.
2. Given NoPWS confirmed (WebServer=0), When the agent follows the skill, Then it can
   start a webgateway sidecar and verify Atelier API reachable.
3. Given a fresh container with first-boot forced-password-change set, When the agent
   follows the skill, Then it clears the flag and authenticates normally.

---

## Functional Requirements

### Config

- **FR-001**: Add `nopws` boolean to server config (`[servers.name]` in
  `.iris-agentic-dev.toml`). Default: `false`.
- **FR-002**: Add `ssh_host` optional string to server config. When set, docker exec
  runs via `ssh <ssh_host> docker exec ...` instead of locally.
- **FR-003**: `iris_add_server` accepts `nopws`, `docker_only`, and `ssh_host` params
  and writes them to the toml entry.

### Error messages

- **FR-004**: When `nopws = true` and web port unreachable, replace "connection refused"
  with a NoPWS-specific message: build type, local fix (`docker_only = true`), remote fix
  (webgateway sidecar), and a reference to docs/nopws.md.
- **FR-005**: `iris_test_server` response includes `nopws: bool` (from config),
  `web_available: bool`, `nopws_detected: bool`, and `nopws_evidence: string`.

### Auto-detection

- **FR-006**: When web probe fails on a server not marked `nopws = true`, attempt
  detection in order:
  1. If `container` set and Docker available locally: `docker exec <container> grep WebServer
/usr/irissys/iris.cpf` — check for `WebServer=0`.
  2. If superserver port responds but web port does not: flag as probable NoPWS.
  3. Otherwise: unreachable (not NoPWS).
- **FR-007**: Detection result includes a ready-to-paste toml snippet.

### Execution fallback

- **FR-008**: When `docker_only = true`, skip web probe and route all ObjectScript
  execution through `docker exec` immediately.
- **FR-009**: When `docker_only` is not set but `container` is set and web port is
  unreachable after one probe, fall back to docker exec automatically.
- **FR-010**: Docker exec command: `docker exec -i <container> iris session IRIS -U
<namespace>` with stdin `\r\n` line endings and `Halt\r\n` terminator.
- **FR-011**: When `ssh_host` is set, run `ssh <ssh_host> docker exec ...` instead of
  local docker exec. iad uses system SSH config (keys, ProxyJump) — no credential
  management.
- **FR-012**: Every `iris_execute` response includes `execution_path`: `"atelier"`,
  `"docker_exec_local"`, or `"docker_exec_ssh"`.
- **FR-013**: `iris_compile` fallback: `Do $System.OBJ.Compile(classname,"ck")` via
  docker exec when Atelier is unavailable.
- **FR-014**: Tools requiring Atelier by design (`iris_doc` put/get, `iris_source_control`)
  return a clear "NoPWS: this tool requires the Atelier REST API" error — no fallback.

### Skill

- **FR-015**: Add `skills/nopws-setup.md` covering:
  - Detection: `docker exec <container> grep WebServer /usr/irissys/iris.cpf`
  - Plain-language NoPWS explanation
  - Option A: webgateway sidecar (step-by-step: pull image, network config, CSP.conf,
    expose port, verify with `curl /api/atelier/`)
  - Option B: `docker_only = true` (local containers only)
  - First-boot password flag: `Do $System.Security.ChangePassword("_SYSTEM","SYS","SYS")`
  - Minimum CSP.conf content
  - Error recognition table (symptom → cause → skill section)
- **FR-016**: Skill description includes keywords: "NoPWS", "No Private Web Server",
  "AI branch", "connection refused", "webgateway sidecar", "irishealth-ai" — so agents
  load it on the right signals.
- **FR-017**: Skill is under 300 lines.

---

## Key Entities

- **NoPWS flag** (`nopws: bool`): explicit declaration that this server has no embedded
  web server.
- **`ssh_host`** (`string`): remote host to run docker exec on via SSH.
- **Routing mode**: `atelier` (default), `docker_exec_local`, `docker_exec_ssh`.
- **NoPWS detection result**: `{ nopws_detected, nopws_evidence, suggestions }`.

---

## Success Criteria

- Connecting to a NoPWS container with `nopws = true` produces a plain-language
  explanation — no raw "connection refused" reaching the user or agent.
- `iris_execute` on a local NoPWS container (container configured, web port closed)
  returns a valid result via docker exec with no user intervention.
- An agent with only the nopws-setup skill and connection details for a fresh NoPWS
  container can establish a working iad connection without additional human guidance
  beyond approving Docker commands.
- All existing Atelier-path tests pass unchanged. NoPWS behavior is opt-in via config.
- A developer new to AI-branch IRIS who hits this error can fix it without asking
  anyone, using only the information in the error message or skill.

---

## Out of Scope

- SSH tunnel management (establishing, monitoring, reconnecting).
- Automatically deploying a webgateway sidecar — iad advises, not provisions.
- Multi-line `{}` block support in terminal mode (spec 096).
- Windows NoPWS containers (docker exec path is Linux/macOS only).
- IIS integration on Windows NoPWS.
- NoPWS on Kubernetes or bare metal.
- Windows named pipe / superserver protocol (port 1972 native).

---

## Assumptions

- NoPWS is identifiable by `WebServer=0` in iris.cpf (local) or web port unreachable
  while superserver responds (remote).
- The webgateway sidecar pattern (`webgateway:*` container fronting NoPWS IRIS) is the
  documented ISC approach for remote NoPWS access.
- `docker_only = true` already exists and routes through docker exec; this spec extends
  its interaction with `nopws` but does not redesign it.
- SSH invocation uses system SSH config — no credential management in iad.
