# Feature Specification: NoPWS Setup Bundled Skill

**Feature Branch**: `099-nopws-setup-skill`
**Created**: 2026-09-02
**Status**: Draft

## Overview

Connecting iad to a NoPWS IRIS container (Enterprise 2026.2.0AI and later builds without
the Private Web Server) is a multi-step process involving detection, a local-vs-remote
decision, docker exec configuration, optional WebGateway sidecar deployment, first-boot
password remediation, and final validation. No agent can complete this reliably without
a written SOP. Today that knowledge is scattered across docs and support tickets. This
spec adds a bundled skill `nopws-setup` that encodes the full SOP as a concise
instruction file installable via `iris-agentic-dev skill install nopws-setup`.

---

## User Scenarios & Testing

### User Story 1 — Agent-guided NoPWS setup for a local container (Priority: P1)

A developer starts an Enterprise 2026.2.0AI container for local development and tries to
connect iad. `iris_test_server` returns `connection refused` on the web port. They have
no idea why. They load the `nopws-setup` skill. The agent now knows to call
`iris_test_server` to confirm the failure mode, identify the container as NoPWS, instruct
the developer to add the server with `docker_only=true`, call `iris_admin
action=fresh_container_setup` if needed, and validate with `iris_execute`. The developer
completes the setup in one conversation turn with no doc-diving.

**Acceptance Scenarios**:

1. Given the `nopws-setup` skill is installed and active, When the agent encounters a
   `connection refused` error on the web port, Then the agent follows the detection →
   decision → configuration → validation sequence without asking the user to find
   documentation.
2. Given a local container and `docker_only=true` chosen, When the agent calls
   `iris_add_server` with those flags, Then the server is registered and subsequent
   `iris_test_server` succeeds.
3. Given a remote NoPWS instance (no docker exec available), When the agent reaches the
   WebGateway decision branch, Then the skill provides the exact `docker run` command for
   the `webgateway:2026.2-amd64` sidecar and the minimal CSP.conf snippet, rather than
   an error.
4. Given the skill is installed via `iris-agentic-dev skill install nopws-setup`, When
   `iris-agentic-dev skill list` is run, Then `nopws-setup` appears in the output with
   `managed_by: "iris-agentic-dev"`.
5. Given a fresh container that also has `ChangePassword=1`, When the skill SOP runs,
   Then the agent calls `iris_admin action=fresh_container_setup` before the final
   `iris_test_server` validation.

### User Story 2 — Skill prevents wrong-tool mistakes (Priority: P2)

Without the skill, an agent trying to connect to NoPWS often attempts Atelier REST on
the web port, gets `connection refused`, and either loops or asks the user for help.
With the skill loaded, the agent recognizes the NoPWS pattern in the first error and
pivots to the docker exec path immediately.

**Acceptance Scenarios**:

1. Given the skill is loaded, When the agent sees `connection refused` on port 52780 (or
   similar web port), Then the agent does not retry the HTTP path — it checks for
   `nopws` indicators and branches to the docker exec path.
2. Given the agent has determined the container is NoPWS, When it calls `iris_add_server`,
   Then it includes `docker_only=true` and `nopws=true` parameters — not the default HTTP
   path.

---

## Functional Requirements

- **FR-001**: Add `skills/skills/nopws-setup/SKILL.md` to the bundled skill pack. The
  skill YAML front-matter must include `name`, `description`, `tags`, `author`,
  `state: reviewed`, `managed_by: "iris-agentic-dev"`, and `iris_version: ">=2026.2"`.
- **FR-002**: The skill body encodes the following SOP in order:
  1. **Detect NoPWS** — call `iris_test_server`; `connection refused` on web port is the
     primary signal. Secondary: check `docker inspect <container>` for absence of PWS
     service or `DPP-1192` marker.
  2. **Decision gate** — local container? → docker exec path. Remote host? →
     WebGateway sidecar path.
  3. **Docker exec path** — call `iris_add_server` with
     `docker_only=true, container_name=<name>, nopws=true`.
  4. **WebGateway path** — provide the `docker run` command for
     `containers.intersystems.com/intersystems/webgateway:2026.2-amd64` with the
     minimal `CSP.conf` stanza; then `iris_add_server` with `nopws=false` (web available
     via gateway).
  5. **First-boot check** — if `iris_test_server` returns `Unexpected error: 1`, call
     `iris_admin action=fresh_container_setup`.
  6. **Validate** — `iris_test_server` must return success; smoke-test with
     `iris_execute` `Write $ZV`.
- **FR-003**: The skill must fit the existing SKILL.md format used by other bundled skills
  (YAML front-matter + Markdown body). It must be installable to Claude Code, OpenCode,
  and VS Code Copilot via the existing `skill install` mechanism without code changes.
- **FR-004**: The skill must not duplicate content from `iris-connectivity` or
  `iris-devtester` — reference those skills by name where overlap exists.
- **FR-005**: Add `nopws-setup` to the skill inventory table in `docs/skills.md`.
- **FR-006**: E2E test: `iris-agentic-dev skill install nopws-setup --dry-run` exits 0
  and prints the install path without writing files. `skill list` after a real install
  shows the skill. These are binary-invocation tests (no live IRIS required).

---

## Key Entities

- **`skills/skills/nopws-setup/SKILL.md`**: new bundled skill file — the deliverable.
- **Skill loader** (skill.rs or equivalent): no code changes required if the skill
  follows the existing SKILL.md format; the loader already handles new files in
  `skills/skills/`.
- **`docs/skills.md`**: add one row to the skill inventory table.

---

## Success Criteria

- `iris-agentic-dev skill install nopws-setup` installs the skill to all supported agents.
- An agent with the skill loaded can complete NoPWS setup (local container path) without
  any user providing documentation or flags not already in the conversation.
- The skill does not regress existing ObjectScript repair benchmark scores when loaded
  for non-NoPWS tasks (verify with `iris-agentic-dev benchmark` — expected: 0% lift, no
  regression).
- The skill file passes `markdownlint-cli2` and `prettier` with no changes required.

---

## Out of Scope

- Automating the WebGateway sidecar `docker run` invocation (the skill documents the
  command; the agent may run it, but iad does not orchestrate docker compose or sidecar
  lifecycle).
- NoPWS setup for IRIS HealthShare or non-community edition differences beyond what the
  skill body covers.
- Modifying the `iris_add_server` tool interface (that is spec 092/093).

---

## Assumptions

- `docker_only=true` and `nopws=true` flags on `iris_add_server` are implemented (spec
  092/093) and merged before this skill ships — the skill references them by name.
- `iris_admin action=fresh_container_setup` is implemented (spec 097) and merged.
- The WebGateway image tag `2026.2-amd64` is correct for the EAP AI build fleet; verify
  against the IRIS 2026.2 release notes before publishing the skill.
- NoPWS is specific to Enterprise 2026.2+ AI builds (DPP-1192); community containers
  always have PWS and do not need this skill.
