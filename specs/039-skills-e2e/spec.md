# Feature Specification: Deep E2E Skills Harness

**Feature Branch**: `039-skills-e2e`
**Created**: 2026-05-31
**Status**: Draft
**Input**: Deep E2E harness for the iris-agentic-dev skills system — simulates a total new user in a fresh isolated OpenCode installation following light-skills/README.md, with a live IRIS container, calling a real LLM, and asserting skill quality end-to-end.

## Problem Statement

The skills in `light-skills/` have benchmark scores (73% → 100% on repair tasks), but there is no automated gate that verifies:
- The README setup instructions actually work for a new user
- Skills load and influence LLM output in a real OpenCode session
- The iris-agentic-dev MCP tools work when invoked from OpenCode against a live IRIS instance

Doc drift, broken curl URLs, or a changed OpenCode config format can silently invalidate the documented setup. The existing benchmark harness tests quality in isolation; this harness tests the full stack end to end.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — New user installs skills and gets better ObjectScript output (Priority: P1)

A developer new to iris-agentic-dev follows the `light-skills/README.md` setup instructions in a clean environment. They install `objectscript-review` via the documented curl command, open an OpenCode session, ask the AI to fix a buggy ObjectScript method, and receive output that does not contain the known mistake the skill guards against.

**Why this priority**: This is the core value claim of the skills system. If a new user cannot reproduce the documented improvement, everything else is moot.

**Independent Test**: Run harness in isolated env (no pre-existing config) → curl installs `objectscript-review` → OpenCode session with real LLM → task is a method with a Return-in-For-loop bug → assert output does not contain `Return` inside a `For` loop body.

**Acceptance Scenarios**:

1. **Given** a clean environment with no OpenCode config or skills installed, **When** the harness executes the curl commands from `light-skills/README.md` Step 1 and Step 2, **Then** `AGENTS.md` exists at the expected path and `objectscript-review/SKILL.md` exists in the isolated skills directory.
2. **Given** `objectscript-review` is installed, **When** OpenCode is started in the isolated env and given a method containing a `Return` statement inside a `For` loop, **Then** the LLM output corrects the bug using `Quit` instead of `Return`.
3. **Given** the harness completes, **When** the OpenCode session SQLite log is read, **Then** the session contains evidence the skill was active (skill name appears in the session context or tool calls).
4. **Given** no skills are installed (baseline run), **When** the same buggy method is submitted, **Then** the baseline pass rate across 5 runs is below 100%, confirming the skill provides measurable lift.

### Edge Cases

- What if the curl URL for a skill returns 404 (e.g., skill was renamed or moved)? → Harness fails immediately with the specific URL and a pointer to the README line that references it.
- What if the LLM refuses to output ObjectScript (safety filter)? → Harness marks the run as inconclusive and retries once with a more neutral framing.
- What if OpenCode exits non-zero before the task completes? → Harness captures stderr, marks run as failed, retains the temp directory.

---

### User Story 2 — New user configures iris-agentic-dev MCP against live IRIS and compiles ObjectScript (Priority: P1)

The same new user follows the MCP setup instructions in the main `README.md`. They configure `iris-agentic-dev` in the isolated OpenCode config pointing at a live IRIS community container started by the harness. They ask the AI to compile a class. The AI invokes `iris_compile`, IRIS compiles the class, and the result (including any errors) is returned and visible in the session log.

**Why this priority**: The MCP tools are the other half of the value proposition. This is the "live IRIS" requirement — the harness must include a real container.

**Independent Test**: Isolated OpenCode env → `iris-agentic-dev` configured with `IRIS_HOST`, `IRIS_WEB_PORT`, `IRIS_CONTAINER` pointing at a harness-managed IRIS community container → OpenCode task: "compile `User.HarnessTestClass`" → assert session log shows `iris_compile` was called and returned a result.

**Acceptance Scenarios**:

1. **Given** a fresh IRIS community container is running and `iris-agentic-dev` is configured in the isolated OpenCode config, **When** `check_config` is called from within the OpenCode session, **Then** the result shows `connected: true` with the correct host and namespace.
2. **Given** a class `User.HarnessTestClass` with a deliberate compile error is loaded into the IRIS container, **When** the AI is asked to compile it, **Then** the AI calls `iris_compile`, receives the error with line number, and the session log records both the tool call and the result.
3. **Given** a syntactically correct class, **When** the AI is asked to compile it, **Then** `iris_compile` returns success and the AI reports it compiled cleanly.

### Edge Cases

- What if the IRIS container takes longer than expected to start? → Harness polls the Atelier endpoint with a timeout (same pattern as CI E2E job), fails with clear message if exceeded.
- What if `iris-agentic-dev` binary is not installed on the CI runner? → Harness downloads the latest release binary as part of setup, verifying the documented installation instructions work.

---

### User Story 3 — Full stack: skills + MCP + live IRIS together (Priority: P2)

The harness combines US1 and US2: skills installed, MCP configured, live IRIS running. The AI is given a task that requires both — read a class from IRIS via `docs_introspect`, generate a `%UnitTest` for it using the `objectscript-unit-test` skill, and compile the generated test class. The session log shows the full chain: introspect → skill-guided generation → compile result.

**Why this priority**: This is the "whole enchilada" scenario that validates the integrated stack. P2 because US1 and US2 independently cover the critical paths; this validates they compose correctly.

**Independent Test**: All components up → AI given: "write and compile a unit test for `User.HarnessTestClass`" → session log shows `docs_introspect` call, generated test class content, `iris_compile` call with result.

**Acceptance Scenarios**:

1. **Given** skills and MCP are both configured, **When** the AI is asked to write a unit test for an existing IRIS class, **Then** the session log shows `docs_introspect` was called before the test was written.
2. **Given** the test class is generated, **When** the AI compiles it via `iris_compile`, **Then** the compile succeeds or returns actionable errors.
3. **Given** the harness completes the full-stack scenario, **When** results are collected, **Then** the run produces a structured report: skill-load confirmed, MCP tool calls recorded, compile result captured.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The harness MUST create a fully isolated environment using `OPENCODE_CONFIG_CONTENT` (to inject config without touching `~/.config/opencode/`) and `OPENCODE_DB` (to isolate the session SQLite DB) — with no reads from or writes to `~/.config/opencode/` or `~/.local/share/opencode/`. A temporary directory is created per run for the skills dir and DB file.
- **FR-002**: The skill installation commands executed by the harness MUST be exact reproductions of the curl commands documented in `light-skills/README.md` — not simplified equivalents.
- **FR-003**: The harness MUST validate that all curl URLs from the README return HTTP 200 before executing them, failing immediately with the specific URL on any non-200 response.
- **FR-004**: The harness MUST spawn a real OpenCode process (not a mock) with provider credentials injected via `OPENCODE_CONFIG_CONTENT` (embedding `{"provider": {"openai": {"options": {"apiKey": "..."}}}}` — note the `options` nesting, confirmed by OpenCode source at `packages/opencode/src/config/provider.ts`) — no pre-seeded credential DB, no file writes to `~/.config/opencode/`.
- **FR-005**: LLM task inputs MUST be fixed, versioned ObjectScript snippets stored in the harness — not generated dynamically — so results are reproducible across runs.
- **FR-006**: The harness MUST collect tool calls and LLM output from the `--format json` event stream during the run. Optionally, it MAY also read the `OPENCODE_DB` SQLite file post-run for richer session introspection (e.g., full message history). The event stream is the primary assertion surface; SQLite is a secondary debugging aid.
- **FR-007**: The harness MUST start a fresh IRIS community container named `iris-skills-e2e` (same image tag as the existing CI E2E job) in its own independent CI job, load required test classes before the OpenCode session begins, and tear it down after. It MUST NOT share or depend on the `iris-e2e` container used by the existing E2E suite.
- **FR-008**: For skill quality assertions, the harness MUST extract fenced code blocks (` ```objectscript` or ` ```cls`) from the LLM text output and check within those blocks for `Return` inside a `For` loop body using regex — not a full-response string match.
- **FR-009**: For MCP assertions, the harness MUST verify that specific tool names appear in the session log tool calls with non-empty results.
- **FR-010**: US1 (skills only) MUST be runnable without a live IRIS container. US2 and US3 MUST be skipped when no IRIS container is available.
- **FR-011**: Each run MUST produce a structured JSON result: scenario name, pass/fail, LLM output excerpt, tool calls observed, assertion details.
- **FR-012**: A baseline run (no skills installed) MUST execute alongside each skill run to produce a per-run lift measurement.
- **FR-013**: The harness MUST tear down the temporary directory and IRIS container after each run; on failure, retain both for debugging (configurable via flag).

### Key Entities

- **IsolatedEnv**: Temporary directory tree providing `XDG_CONFIG_HOME`/`XDG_DATA_HOME` for a single harness run. Contains skills dir, OpenCode config, and session DB.
- **HarnessTask**: A fixed versioned ObjectScript snippet + expected assertion (anti-pattern to check, tool calls expected). Stored in `tests/e2e/tasks/`.
- **RunResult**: Structured JSON capturing scenario name, pass/fail, skill-load evidence, tool calls, LLM output excerpt, baseline comparison.
- **HarnessIRISContainer**: The IRIS community container started and managed by the harness. Separate from any developer's own containers.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The skill quality gate catches the `Return`-in-loop anti-pattern in 100% of runs when `objectscript-review` is installed, and in fewer than 100% of baseline runs — confirming measurable lift.
- **SC-002**: The harness detects README documentation drift (broken URL, changed config key, removed skill file) within one CI run of it occurring, with a clear error message identifying the specific line.
- **SC-003**: The MCP tool gate confirms `iris_compile` is invoked and returns a real IRIS result in 100% of runs where a live IRIS container is available.
- **SC-004**: The skills-only path (US1) completes in under 3 minutes on the CI runner.
- **SC-005**: The full-stack path (US3) completes in under 8 minutes on the CI runner, including IRIS container startup.
- **SC-006**: The full-stack scenario (US3) runs on every push to `master` in CI with no manual intervention.

---

## Clarifications

### Session 2026-05-31

- Q: Does OpenCode support non-interactive/headless mode? → A: Yes. `opencode run "message" --format json` starts an in-process server, sends the message, streams raw JSON events (tool calls, text, errors) to stdout, and exits when idle. Fully non-interactive.
- Q: How should the harness inject provider credentials into the isolated OpenCode run? → A: Via `OPENCODE_CONFIG_CONTENT` env var — embed `{"provider": {"openai": {"options": {"apiKey": "$OPENAI_API_KEY"}}}}` in the config JSON (note `options` nesting — confirmed by spike reading `packages/opencode/src/config/provider.ts`). Zero setup, fully stateless per run.
- Q: Should the harness curl skill files from GitHub or copy from local checkout? → A: Always curl from GitHub — true new-user simulation, validates URLs are live, catches renamed/moved files. A broken URL is a documentation bug that must fail CI.
- Q: How strictly should the harness parse LLM output to assert the Return-in-loop anti-pattern is absent? → A: Regex on fenced code blocks — extract ```objectscript/```cls blocks, then check for `Return` inside a `For` body within those blocks.
- Q: Should the harness share the existing E2E job's IRIS container or spin its own? → A: Spin an independent container (`iris-skills-e2e`) in its own CI job — fully parallel with the existing E2E suite, no ordering dependency or interference.

## OpenCode Headless Interface (spike findings, 2026-05-31)

Confirmed by reading `packages/opencode/src/cli/cmd/run.ts` and live spike:

- **`opencode run "message" --format json`** — the harness invocation. Streams JSON events and exits.
- **`--dangerously-skip-permissions`** — auto-approves all tool permission prompts. Required for unattended CI.
- **`OPENCODE_DB`** env var — overrides SQLite DB path. Harness points this at a known temp file to isolate and read session data after the run.
- **`OPENCODE_CONFIG_CONTENT`** env var — injects the full config JSON as a string, bypassing `~/.config/opencode/config.json`. Harness uses this to inject skills path and MCP server config without touching `~/.config/opencode/`.
- **Provider credentials** — stored in OpenCode's own SQLite DB via `opencode providers login`. Exit code 90 = no credentials. Harness must inject provider options (including API key) via `OPENCODE_CONFIG_CONTENT`.

## Assumptions

- OpenCode is installed on the CI runner or the harness installs it as part of setup.
- An OpenAI API key is available as a CI secret (`OPENAI_API_KEY`).
- The IRIS community image is publicly pullable without authentication on the CI runner.
- OpenCode's `--format json` event stream schema is stable across the pinned OpenCode version; the harness pins the version it tests against.
- The harness is written in Python, consistent with the existing benchmark harness in `light-skills/`.
- The `objectscript-review` skill's Return-in-loop guard is stable enough to serve as the primary quality assertion.

---

## Out of Scope

- Testing OpenCode itself — the harness assumes OpenCode works and tests only iris-agentic-dev's integration with it.
- Benchmarking across multiple models or providers — one fixed model per run (OpenAI, configurable).
- Windows or WSL2 harness paths — Mac/Linux only.
- CI performance optimization beyond the stated time targets.
