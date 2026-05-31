# Research: Deep E2E Skills Harness (039)

**Date**: 2026-05-31
**Branch**: 039-skills-e2e

---

## Decision 1: OpenCode headless invocation

**Decision**: `opencode run "message" --format json --dangerously-skip-permissions`

**Rationale**: `opencode run` starts a full in-process server, sends the message, streams raw JSON events to stdout, and exits when idle. `--format json` gives structured machine-parseable output. `--dangerously-skip-permissions` is required for unattended CI (auto-approves tool calls). Confirmed by reading `packages/opencode/src/cli/cmd/run.ts`.

**Alternatives considered**: PTY/pexpect (complex, fragile), `opencode serve` + REST (two-process coordination), stdin pipe (also works but positional args are simpler).

---

## Decision 2: Provider credential injection

**Decision**: Inject via `OPENCODE_CONFIG_CONTENT` using `{"provider": {"openai": {"options": {"apiKey": "..."}}}}`

**Rationale**: `OPENCODE_CONFIG_CONTENT` is applied last in the merge order (highest priority), deep-merging with any existing config. The correct path is `provider.openai.options.apiKey` (confirmed in `packages/opencode/src/config/provider.ts` lines 73-112). This is fully stateless — no credential DB pre-seeding, no file writes.

**Source**: `packages/opencode/src/config/config.ts` lines 570-578 — OPENCODE_CONFIG_CONTENT merges (not replaces) with existing config, applied after all file-based config.

**Alternatives considered**: Pre-seed credentials DB (stateful, requires `opencode providers login`), env var `OPENAI_API_KEY` alone (exit code 90 — does not work without DB).

---

## Decision 3: Skills path injection

**Decision**: `OPENCODE_CONFIG_CONTENT` with `{"skills": {"paths": ["/tmp/harness-skills-XXXX"]}}`

**Rationale**: `config.skills.paths` is an array of directories OpenCode scans for `**/SKILL.md` files (confirmed in `packages/opencode/src/config/skills.ts` lines 5-12 and `packages/opencode/src/skill/index.ts` lines 174-184). Supports absolute paths, `~/` expansion, and relative-from-cwd paths. The harness writes the isolated skills dir to a temp path and injects it via OPENCODE_CONFIG_CONTENT alongside the provider key — one env var covers both.

**Alternatives considered**: Writing to `~/.config/opencode/skills/` (not isolated), using `XDG_CONFIG_HOME` override (harder to compose with existing config).

---

## Decision 4: MCP tool assertion format

**Decision**: Parse `tool_use` events from `--format json` stream; identify MCP tools by `part.tool` format `{server}:{tool}` (colon-delimited).

**Rationale**: The JSON event stream emits `{"type": "tool_use", "part": {"tool": "iris_agentic_dev:iris_compile", "state": {"status": "completed", "output": "..."}}}`. MCP tools use `{sanitized_server}:{sanitized_tool}` naming; built-in tools have no colon. Confirmed by reading `mcp/index.ts` and `run.ts`.

**Key fields**:
- `event["part"]["tool"]` — full tool name (e.g., `iris_agentic_dev:iris_compile`)
- `event["part"]["state"]["status"]` — `completed` | `error` | `running` | `pending`
- `event["part"]["state"]["output"]` — tool result text
- `event["part"]["state"]["input"]` — tool arguments

**Alternatives considered**: Read SQLite DB post-run (viable but parsing the event stream is cleaner and synchronous).

---

## Decision 5: Skill quality assertion method

**Decision**: Regex on fenced code blocks (` ```objectscript` or ` ```cls`), then scan within those blocks for `Return` followed by a newline and indented content inside a `For` loop body.

**Rationale**: LLM output for code tasks is reliably wrapped in fenced blocks. The existing benchmark harness has no code block extractor (confirmed), so we add one. A full AST parser is over-engineered; a targeted regex matching `For\s.*{[^}]*\bReturn\b` (or `For\s.*\n[^\n]*Return`) within extracted blocks is sufficient for the single anti-pattern being tested.

**Pattern**: Extract blocks matching ` ```(objectscript|cls)\n...\n``` `, then check within each block for `\bReturn\b` inside a `For` loop (defined as appearing after `For` and before the matching closing brace/dedent).

**Alternatives considered**: Full ObjectScript AST (over-engineered), whole-response match (high false positive rate), LLM-as-judge (second LLM call, non-deterministic).

---

## Decision 6: Fixture format

**Decision**: Reuse the existing benchmark fixture YAML schema from `benchmark/021/tasks/` — `id`, `category`, `description`, `expected_behavior`, `fixtures[]` (type/name/content). Add an `assertions[]` field for the new harness: `{type: "code_absent_pattern", pattern: "...", description: "..."}`.

**Rationale**: Reusing the existing schema means the same task definitions can potentially feed both harnesses. The existing `fixtures.py` uses MCP tools (`iris_doc`, `iris_compile`) to load fixtures into IRIS — the E2E harness can reuse this for the live IRIS path.

---

## Decision 7: Result JSON schema

**Decision**: Extend the existing schema from `benchmark/021/runner/result_writer.py` with E2E-specific fields.

```json
{
  "run_id": "2026-05-31T...",
  "harness": "e2e-opencode",
  "opencode_version": "...",
  "iris_agentic_dev_version": "...",
  "model": "openai/gpt-4o-mini",
  "tasks": [
    {
      "task_id": "SKILL-01",
      "scenario": "us1_skills_only",
      "condition": "baseline | objectscript-review",
      "pass": true,
      "skill_loaded": true,
      "tool_calls": ["iris_agentic_dev:iris_compile"],
      "assertion_details": {"pattern": "Return-in-For", "found": false},
      "llm_output_excerpt": "..."
    }
  ],
  "summary": {
    "pass_rate": 1.0,
    "skill_lift": 0.27,
    "tool_calls_observed": ["iris_agentic_dev:iris_compile", "iris_agentic_dev:docs_introspect"]
  }
}
```

---

## Decision 8: CI job structure

**Decision**: Two new CI jobs — `skills-e2e` (US1, no IRIS, runs on all pushes) and `skills-e2e-full` (US2+US3, with `iris-skills-e2e` container, runs on `master` only, same guard as existing `e2e-tests` job).

**Rationale**: US1 (skills quality gate) has no IRIS dependency and should run on every PR to catch README drift fast. US2+US3 require IRIS container startup (~2 min overhead) and are appropriately gated to master only.

---

## Decision 9: OPENCODE_DB isolation

**Decision**: Set `OPENCODE_DB=/tmp/opencode-harness-{run_id}.db` per run. Read it post-run for additional session introspection if the event stream is insufficient.

**Rationale**: Prevents cross-contamination of session history. The DB can be read for richer assertions (e.g., full message content) if needed beyond what the event stream exposes.
