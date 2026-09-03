# Feature Specification: docker exec Fallback Transport for NoPWS Containers

**Feature Branch**: `093-docker-exec-fallback`
**Created**: 2026-09-02
**Status**: Draft

## Overview

When the web port is unreachable — NoPWS build, container starting, Apache crashed —
all iad tools fail immediately. For locally-running containers this is unnecessary:
`docker exec <container> iris session iris` is a fully capable transport for
ObjectScript execution, document reads, and basic queries. This spec routes
`iris_execute`, `iris_compile`, and `iris_doc` (get/list) through `docker exec`
when `docker_only = true` and a `container_name` is set in the server config. The
tool output shape is identical; only the transport changes. Auto-fallback on
connection failure is explicitly excluded — opt-in only via `docker_only = true`.

---

## User Scenarios & Testing

### User Story 1 — Execute ObjectScript on a NoPWS local container (Priority: P1)

A developer is running an AI Hub EAP image locally: `irishealth-ai:2026.3.0AI`, no
Apache. They set `docker_only = true` and `container_name = "aihub-iris"` in their
`.iris-agentic-dev.toml`. `iris_execute` sends code through
`docker exec aihub-iris iris session iris` and returns output. The tool response
looks identical to an Atelier REST response.

**Acceptance Scenarios**:

1. Given `docker_only = true` and `container_name` is set, When `iris_execute` is
   called with `code = "write $ZVERSION,!"`, Then the response is
   `{success: true, output: "<version string>"}` — routed through docker exec.
2. Given `docker_only = true` and `container_name` is set, When `iris_execute` is
   called with invalid ObjectScript, Then the response is
   `{success: false, error: "<IRIS error text>"}` — parse and runtime errors
   surfaced, not swallowed.
3. Given `docker_only = true` and `container_name` is set, When `iris_doc` is called
   with `mode = "get"` and a valid class name, Then the class source is returned.
4. Given `docker_only = true` and `container_name` is set, When `iris_doc` is called
   with `mode = "list"` and a pattern, Then the matching document names are returned.
5. Given `docker_only = true` and `container_name` is set, When `iris_compile` is
   called, Then the compile result (success/errors) is returned from docker exec.

### User Story 2 — docker exec not used without explicit opt-in (Priority: P1)

A server is configured without `docker_only = true`. Its web port goes down. The
tool returns the existing error — no silent fallback to docker exec.

**Acceptance Scenarios**:

1. Given `docker_only = false` (default), When the web port is unreachable, Then
   the existing `IRIS_UNREACHABLE` error is returned — docker exec is never tried.
2. Given `docker_only = true` but `container_name` is absent, When any tool is
   called, Then the response is `{success: false, error_code: "DOCKER_EXEC_NO_CONTAINER",
message: "docker_only=true requires container_name to be set."}`.
3. Given `docker_only = true` and `container_name` is set, When `docker exec`
   returns a non-zero exit code because the container is stopped, Then the response
   is `{success: false, error_code: "DOCKER_EXEC_FAILED", message: "<docker error>"}`.

### User Story 3 — iris_sql via docker exec (Priority: P2)

An agent needs to run a read-only SQL query against a NoPWS container. `iris_sql`
with `mode = "read"` runs `iris session iris` and uses embedded `&sql(...)` — or the
`%SQL.Statement` translation path already present in `iris_execute`. The query result
returns as a JSON rows array.

**Acceptance Scenarios**:

1. Given `docker_only = true` and `container_name` set, When `iris_sql` is called
   with `mode = "read"` and a SELECT, Then the result includes `{rows: [...]}`.
2. Given `docker_only = true`, When `iris_sql` is called with `mode = "write"`,
   Then the Execute gate applies as normal — blocked on `mcpTemplate = live`.

---

## Functional Requirements

- **FR-001**: The docker exec transport is opt-in exclusively via
  `docker_only = true`. Auto-fallback on connection error is not implemented.
  This is a deliberate safety choice: silent transport switching masks config
  problems and could silently target the wrong container.
- **FR-002**: `ServerEntry` gains
  `#[serde(skip_serializing_if = "Option::is_none")] pub container_name: Option<String>`.
  `WorkspaceConfig` already has `container` (the env-var path); the new field
  co-exists without collision.
- **FR-003**: `iris_execute` docker exec path: serialize the code string to a
  temp file inside the container via
  `docker exec -i <container> iris session iris -U <namespace>`,
  send the code on stdin, capture stdout and stderr. Output is returned as-is.
  The existing SQL translation (`translate_sql`) applies before the code is sent
  — the transport is below the translation layer.
- **FR-004**: `iris_compile` docker exec path: issue the compile directive as
  ObjectScript code via the same transport. Error output is parsed using the
  existing compile-error regex.
- **FR-005**: `iris_doc` docker exec path for `mode = "get"`: read the document
  source using `$system.OBJ.ExportToStream`. For `mode = "list"`: query
  `%Library.RoutineMgr_StudioOpenDialog`. Other modes (`put`, `delete`,
  `insert`, `delete_lines`) require Atelier REST and must return
  `error_code: "NOPWS"` when `docker_only = true`.
- **FR-006**: `iris_sql` mode `read`: run the query via `%SQL.Statement` in a
  `docker exec` session, serialize result rows as JSON. Write-mode SQL is
  Execute-gated as normal; the transport does not bypass the gate.
- **FR-007**: Tool descriptions for `iris_execute`, `iris_compile`, `iris_doc`,
  and `iris_sql` note: `"Runs via docker exec when docker_only=true and
container_name is set — no web port required."` (append to existing
  description, do not replace).
- **FR-008**: The `docker exec` command is assembled with no shell interpolation:
  `Command::new("docker").args(["exec", "-i", container, "iris", "session", "iris",
"-U", namespace])`. Arguments are never passed through a shell.
- **FR-009**: Timeout for docker exec calls mirrors the existing HTTP timeout
  (30 s default, configurable via `request_timeout_secs` in TOML).

---

## Key Entities

- **`ServerEntry`** (iris/servers_config.rs): add `pub container_name: Option<String>`.
- **`WorkspaceConfig`** (iris/workspace_config.rs): `docker_only` already exists.
  Add `container_name` as an optional TOML key that overrides `IRIS_CONTAINER` env
  when set.
- **`iris_execute`** (tools/mod.rs): add docker exec branch when
  `docker_only && container_name.is_some()`.
- **`iris_compile`** (tools/mod.rs): same branch.
- **`iris_doc`** (tools/mod.rs): docker exec branch for `get` and `list` modes;
  `NOPWS` error for write modes.
- **`iris_sql`** (tools/mod.rs): docker exec branch for `read` mode.

---

## Success Criteria

- `iris_execute` returns correct output from a NoPWS local container with
  `docker_only = true` and `container_name` set.
- `iris_compile` returns structured errors from the docker exec path.
- `iris_doc get` returns class source; `iris_doc list` returns names.
- No tool uses docker exec unless `docker_only = true` is explicitly declared.
- Missing `container_name` with `docker_only = true` returns an actionable error,
  not a panic or timeout.
- Live IRIS integration test: all three tools succeed against `iris-dev-iris` with
  `docker_only = true` and `container_name = "iris-dev-iris"`.
- Binary-invocation test: `initialize` + `tools/call iris_execute` with
  `docker_only`-configured toml returns a structured response (no crash).

---

## Out of Scope

- Auto-fallback when web port goes down (explicit opt-in only — see FR-001).
- `iris_doc put`, `delete`, `insert`, `delete_lines` via docker exec — these
  require Atelier REST for atomicity guarantees.
- `iris_search` and `iris_global` via docker exec (separate work items if needed).
- SSH tunneling or remote docker contexts — only the local `docker` CLI is used.
- Windows named pipe docker transport (docker CLI on Windows is the same
  `Command::new("docker")` call; the transport is CLI-agnostic).

---

## Assumptions

- `docker` is on `PATH` in the agent's execution environment. If absent,
  `DOCKER_NOT_FOUND` error is returned with an install hint.
- The container is already running when the tool is called. Container lifecycle
  management (start/stop) is out of scope.
- `iris session iris` is available in all IRIS 2022+ images. The `-U namespace`
  flag sets the namespace at session start.
- The docker exec path is not the performance-sensitive path. Session-per-call
  overhead (~ 200–500 ms) is acceptable for a NoPWS development workflow.
- 092-nopws-awareness lands first, establishing `nopws: true` semantics. This spec
  builds on it: `docker_only + container_name` is the working escape hatch that
  093 delivers for local NoPWS containers.
