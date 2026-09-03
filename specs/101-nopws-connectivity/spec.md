# Feature Specification: NoPWS Connectivity

**Feature Branch**: `101-nopws-connectivity`
**Created**: 2026-09-02
**Status**: Draft
**Input**: Merges 091-nopws-connectivity (3-layer NoPWS gap fix)

## Overview

AI-branch IRIS builds (irishealth-ai:\*, iris-ai:\*, 2026.3+) ship without an embedded
web server (`WebServer=0` in iris.cpf) — called NoPWS (No Private Web Server). iad assumes
every IRIS instance exposes Atelier REST on a web port. When it hits a NoPWS container,
the result is "connection refused" with no explanation and no fallback path.

Partial NoPWS handling already exists:

- `docker_only = true` in `.iris-agentic-dev.toml` routes execution through
  `docker exec iris session` (sentinel URL `http://127.0.0.1:1`)
- `derive_capabilities()` detects `2026.2.0AI` version strings and sets `no_pws = true`
- `iris_compile` already falls back to docker exec when `docker_only || no_pws`

This feature closes the remaining gaps at three layers:

1. **Detection and config**: a `nopws = true` toml flag (extends existing `docker_only`
   pattern), explicit NoPWS error messages, iris.cpf-based auto-detection, and new
   `ssh_host` field for remote containers.
2. **`iris_execute` fallback and `execution_path` field**: automatic docker exec fallback
   when Atelier is unreachable (mirrors the existing `iris_compile` path), plus an
   `execution_path` field in every `iris_execute` response so callers know which path ran.
3. **Skill**: a bundled `skills/nopws-setup.md` knowledge file teaching agents the full
   NoPWS setup sequence — detection, webgateway sidecar, first-boot password clearing.

---

## User Scenarios & Testing

### User Story 1 — NoPWS flag and clear error messages (Priority: P1)

An operator adds an AI-branch IRIS container to `.iris-agentic-dev.toml` with
`nopws = true`. When they connect or call `iris_test_server`, they see a plain-language
explanation — not "connection refused."

**Why this priority**: Hardest blocker. Every iad tool that uses Atelier REST silently
fails on NoPWS instances. Operators hitting AI Hub EAP containers hit this immediately.

**Independent Test**: Configure a `WorkspaceConfig` with `nopws = true` and a closed web
port; assert `iris_test_server` returns `{ nopws: true, web_available: false }` with an
explanatory message rather than a connection error.

**Acceptance Scenarios**:

1. **Given** `.iris-agentic-dev.toml` has `nopws = true` and the web port is unreachable,
   **When** `iris_test_server` is called, **Then** the response includes `nopws: true`,
   `web_available: false`, and a `message` field explaining NoPWS with remediation steps.
2. **Given** `nopws = true` and `docker_only = true`, **When** any execution tool is
   called, **Then** all calls route through `docker exec iris session` — no web port
   attempt.
3. **Given** `nopws = true` but `docker_only = false` (webgateway sidecar in use),
   **When** the web port is reachable, **Then** iad connects normally via Atelier REST —
   the `nopws` flag only suppresses "connection refused" errors, not successful connections.

### User Story 2 — iris_execute fallback + execution_path (Priority: P1)

A developer calls `iris_execute` against a local NoPWS container (no web server, container
name configured). iad falls back to `docker exec iris session` and returns the result with
`execution_path: "docker_exec_local"` so the caller knows which path ran.

**Why this priority**: `iris_compile` already has this fallback. `iris_execute` needs the
same treatment and the `execution_path` field is needed for debugging.

**Independent Test**: With `IRIS_WEB_PORT` pointed at a closed port and `IRIS_CONTAINER`
set, call `iris_execute` — assert the result returns via docker exec with `execution_path:
"docker_exec_local"`.

**Acceptance Scenarios**:

1. **Given** `docker_only = true` or NoPWS detected, **When** `iris_execute` is called
   with a simple expression, **Then** the result is returned via docker exec, not Atelier
   REST, and `execution_path` is `"docker_exec_local"` (or `"docker_exec_ssh"` if
   `ssh_host` is set).
2. **Given** Atelier REST is reachable, **When** `iris_execute` is called, **Then**
   `execution_path` is `"atelier"` — no behavioral change from today.
3. **Given** docker exec fallback is used and IRIS returns `<UNDEFINED>`, **Then** the
   error is surfaced in the tool response — not swallowed.
4. **Given** docker exec fallback is needed but no container name is resolvable (no
   `IRIS_CONTAINER` env, no `container` in config), **Then** the error message says
   exactly what is missing and how to set it — not a generic failure.

### User Story 3 — Remote NoPWS container via SSH (Priority: P2)

An operator manages IRIS containers on remote hosts (Baystate, AI Hub EAP) where Docker
is not local. They set `ssh_host` in `.iris-agentic-dev.toml`; iad routes docker exec
through `ssh <host> docker exec ...`.

**Why this priority**: Needed for fleet scenarios. Without SSH support, docker exec
fallback only works on localhost.

**Independent Test**: Configure `ssh_host = "test-host"` in a WorkspaceConfig; assert the
docker exec command is prefixed with `ssh test-host`.

**Acceptance Scenarios**:

1. **Given** `ssh_host` is set and docker exec fallback is triggered, **When** a tool
   runs, **Then** the command runs as `ssh <ssh_host> docker exec <container> iris session
IRIS -U <namespace>`.
2. **Given** the SSH host is unreachable, **Then** the error names the SSH host explicitly
   — not a generic timeout.
3. **Given** `ssh_host` is not set and the container is not found locally, **Then** the
   error says "container not found locally and no ssh_host configured."

### User Story 4 — NoPWS auto-detection (Priority: P2)

`iris_test_server` is called against a server with no `nopws` flag. The web port fails.
iad auto-detects NoPWS by probing the container or superserver port and returns
`nopws_detected: true` with a ready-to-paste remediation snippet.

**Why this priority**: Most users won't know to set `nopws = true`. Auto-detection
converts a cryptic failure into a teachable moment — aligned with iad's north star of
making IRIS legible.

**Independent Test**: Against `iris-dev-iris` (has web server), `iris_test_server` returns
`nopws_detected: false`. Against a container with `WebServer=0` in iris.cpf, returns
`nopws_detected: true`.

**Acceptance Scenarios**:

1. **Given** the web port is unreachable and `docker exec` can read iris.cpf, **When**
   `iris_test_server` is called, **Then** `nopws_detected: true` is returned with
   `nopws_evidence` quoting the `WebServer=0` line.
2. **Given** auto-detection confirms NoPWS, **Then** the response includes a ready-to-
   paste `.iris-agentic-dev.toml` snippet (`nopws = true` + `docker_only = true` for
   local, or webgateway port for remote).
3. **Given** the web port is unreachable and Docker is not available locally, **Then**
   `nopws_detected: false` and `unreachable: true` — NoPWS is not assumed without
   evidence.

### User Story 5 — NoPWS setup skill (Priority: P1)

An agent hits connection refused on an AI-branch container. It loads the nopws-setup skill
and follows detection → sidecar setup → first-boot password clearing with no additional
human guidance.

**Why this priority**: Eliminates the 30-minute debugging session hitting this for the
first time (confirmed in 2026-09-02 dogfooding on Baystate).

**Independent Test**: An agent reading only the skill and given connection details for a
fresh NoPWS container can produce working iad connection config — validated by a human
reviewer reading the skill flow.

**Acceptance Scenarios**:

1. **Given** the nopws-setup skill is loaded, **When** the agent encounters connection
   refused on an AI-branch web port, **Then** it identifies probable NoPWS and follows
   the detection step before assuming the server is down.
2. **Given** NoPWS confirmed (`WebServer=0`), **When** the agent follows the skill,
   **Then** it has steps to start a webgateway sidecar and verify the Atelier API.
3. **Given** a fresh container with first-boot forced-password-change set, **When** the
   agent follows the skill, **Then** it clears the flag using documented ObjectScript.

### Edge Cases

- What happens when both `nopws = true` and the web port IS reachable (webgateway
  sidecar running)? iad should use Atelier REST — `nopws` only suppresses the error,
  not the connection.
- What if docker exec returns a non-zero exit code? Surface the stderr as the error
  message, do not swallow it.
- What if `ssh_host` is set but `container` is not? Return a clear error: "ssh_host
  requires container to be set."
- What if the iris.cpf read via docker exec returns unexpected output (permission denied,
  file not found)? Treat as inconclusive — do not falsely claim NoPWS detected.

---

## Requirements

### Functional Requirements

- **FR-001**: Add `nopws: bool` field (default `false`) to `WorkspaceConfig` in
  `.iris-agentic-dev.toml`. Serde default = false.
- **FR-002**: Add `ssh_host: Option<String>` field to `WorkspaceConfig`. When set, docker
  exec runs via `ssh <ssh_host> docker exec ...` instead of local docker exec.
- **FR-003**: `iris_test_server` response gains fields: `nopws: bool` (from config),
  `web_available: bool` (from probe), `nopws_detected: bool`, `nopws_evidence: Option<String>`.
- **FR-004**: When `nopws = true` and the web port is unreachable, suppress the raw
  connection error and return a structured message explaining: NoPWS build type, local fix
  (`docker_only = true`), remote fix (webgateway sidecar), reference to docs.
- **FR-005**: Auto-detection in `iris_test_server`: when web probe fails and `nopws` is
  not set, attempt in order: (1) `docker exec <container> grep WebServer <iris_cpf>` if
  `container` is configured and Docker is local — probe `/usr/irissys/iris.cpf` first,
  then `/usr/local/etc/irissys/iris.cpf` (covers Alpine-based images); use first hit,
  (2) TCP probe of superserver port (1972). Report `nopws_detected` based on findings.
- **FR-006**: Auto-detection result includes a ready-to-paste toml snippet when NoPWS is
  confirmed.
- **FR-007**: `iris_execute` gains the same docker exec fallback as `iris_compile`: when
  `docker_only = true` or `no_pws` is detected, skip Atelier REST and route through
  `docker exec -i <container> iris session IRIS -U <namespace>` with `\r\n` line endings
  and `Halt\r\n` terminator.
- **FR-008**: Every `iris_execute` response includes `execution_path`:
  `"atelier"` | `"docker_exec_local"` | `"docker_exec_ssh"`.
- **FR-009**: When `ssh_host` is set, docker exec runs via `ssh -o StrictHostKeyChecking=no
<ssh_host> docker exec ...`. iad uses system SSH config (keys, ProxyJump) — no
  credential management. `StrictHostKeyChecking=no` is required because the MCP subprocess
  is non-interactive and cannot respond to host-key prompts.
- **FR-010**: Tools requiring Atelier by design (`iris_doc` put/get, `iris_source_control`,
  `iris_doc_search`) return a clear "NoPWS: this tool requires Atelier REST API. Set up a
  webgateway sidecar or use `docker_only = true` for supported tools." — no silent fallback.
  `iris_doc_search` has no docker exec fallback (see research.md §3); it must return
  `NOPWS_ATELIER_REQUIRED` and must not attempt an HTTP request when `docker_only || nopws`.
- **FR-011**: Unit test: `toml::from_str` round-trip for `nopws = true` and
  `ssh_host = "baystate"` in WorkspaceConfig — confirms serde wiring (prevents silent-drop
  regression like issue #110).
- **FR-012**: Binary invocation test (#[ignore], IAD_BINARY): spawn binary, call
  `iris_test_server`, assert `nopws` field present in response.
- **FR-013**: Live IRIS integration test (#[ignore], iris-dev-iris, --test-threads=1):
  `iris_test_server` against community container returns `nopws_detected: false`; docker
  exec fallback test with IRIS_WEB_PORT pointing at a closed port.
- **FR-014**: Add `skills/nopws-setup.md` (under 300 lines) covering: detection commands,
  plain-language NoPWS explanation, Option A (webgateway sidecar: pull, network, CSP.conf,
  verify), Option B (`docker_only = true`), first-boot password clearing
  (`Do $System.Security.ChangePassword("_SYSTEM","SYS","SYS")`), error recognition table.
- **FR-015**: Skill description keywords: "NoPWS", "No Private Web Server", "AI branch",
  "connection refused", "webgateway sidecar", "irishealth-ai" — ensures correct skill
  selection.
- **FR-016**: `iris_compile` responses from the docker exec path MUST include
  `execution_path: "docker_exec_local"` or `"docker_exec_ssh"` for parity with
  `iris_execute`. The `method: "docker_exec"` field already exists in `iris_compile`
  responses; `execution_path` is the finer-grained counterpart using the same three-value
  vocabulary as `iris_execute` (`"atelier"` / `"docker_exec_local"` / `"docker_exec_ssh"`).

### Key Entities

- **WorkspaceConfig** (`.iris-agentic-dev.toml`): gains `nopws: bool` and
  `ssh_host: Option<String>`. `docker_only: bool` already exists — unchanged.
- **Routing mode**: `"atelier"` (default), `"docker_exec_local"`, `"docker_exec_ssh"`.
  Returned as `execution_path` in `iris_execute` responses.
- **NoPWS detection result**: `{ nopws_detected: bool, nopws_evidence: Option<String>,
suggestion: Option<String> }` — embedded in `iris_test_server` response.

---

## Success Criteria

### Measurable Outcomes

- **SC-001**: An operator hitting a NoPWS container sees a plain-language explanation with
  at least one concrete remediation step — zero occurrences of raw "connection refused"
  when `nopws = true` is configured.
- **SC-002**: `iris_execute` against a local NoPWS container (web port closed, container
  configured) returns a valid result via docker exec with no manual intervention.
- **SC-003**: An agent with only the nopws-setup skill and container connection details
  can establish a working iad connection without asking anyone for help (measured by a
  reviewer reading the skill and confirming the steps are complete and correct).
- **SC-004**: All existing Atelier-path tests pass unchanged — NoPWS changes are opt-in
  via `nopws = true` config or triggered only on actual connection failure.
- **SC-005**: Unit round-trip test for `nopws` and `ssh_host` fields passes — prevents
  silent-drop regression.

---

## Out of Scope

- SSH tunnel management (establishing, monitoring, reconnecting tunnels).
- Automatically deploying or starting a webgateway sidecar — iad advises, not provisions.
- Multi-line `{}` block support in iris session terminal mode (spec 096).
- Windows NoPWS containers (docker exec path is Linux/macOS only for this spec).
- IIS integration on Windows NoPWS builds.
- NoPWS on Kubernetes or bare metal.
- Windows named-pipe / superserver native protocol (port 1972).
- Modifying `ServerEntry` (servers.json / iris_add_server) — NoPWS config lives in
  `.iris-agentic-dev.toml` (WorkspaceConfig), not the iad-native servers.json.

---

## Assumptions

- `docker_only = true` already works in WorkspaceConfig — this spec extends it with
  `nopws` flag and `ssh_host`, and adds the missing `iris_execute` fallback that
  `iris_compile` already has.
- NoPWS auto-detection via `2026.2.0AI` version string already exists in
  `derive_capabilities()` — this spec broadens detection to include iris.cpf checks and
  superserver port probing.
- The webgateway sidecar pattern (`webgateway:*` container fronting NoPWS IRIS) is the
  ISC-documented approach for remote NoPWS access.
- SSH invocation uses system SSH config — iad does not manage keys or passphrases.
- `iris.cpf` path varies by base OS. Detection probes `/usr/irissys/iris.cpf` (Ubuntu)
  then `/usr/local/etc/irissys/iris.cpf` (Alpine) — first hit wins (FR-005).
- Docker exec fallback has a 10-second timeout; a hung container is surfaced as a timeout
  error, not an indefinite block.
