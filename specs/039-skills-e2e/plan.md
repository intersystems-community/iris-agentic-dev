# Implementation Plan: Deep E2E Skills Harness

**Branch**: `039-skills-e2e` | **Date**: 2026-05-31 | **Spec**: [spec.md](./spec.md)

## Summary

Build a Python test harness that simulates a new user following `light-skills/README.md` end-to-end: curls skill files from GitHub into an isolated environment, spawns a real `opencode run --format json` process with live OpenAI credentials injected via `OPENCODE_CONFIG_CONTENT`, parses the JSON event stream to assert skill quality (Return-in-loop anti-pattern absent from code blocks) and MCP tool invocation (`iris_agentic_dev:iris_compile` called against a live IRIS container). Runs as two CI jobs: `skills-e2e` (US1, no IRIS, all pushes) and `skills-e2e-full` (US2+US3, `iris-skills-e2e` container, master only).

---

## Technical Context

**Language/Version**: Python 3.11+ (consistent with existing benchmark harness)
**Primary Dependencies**: `requests` (curl URL validation + fixture loading), `subprocess` (opencode invocation), `sqlite3` (stdlib, OPENCODE_DB inspection), `pyyaml` (task fixture files), `pytest` (test runner)
**Storage**: Temp files per run (`/tmp/opencode-harness-{run_id}/`); results JSON to `tests/e2e/results/`
**Testing**: pytest — harness is itself a pytest suite; `--format json` event stream assertions inline
**Target Platform**: macOS + Linux (CI ubuntu-latest)
**Performance Goals**: US1 < 3 min; US3 < 8 min including IRIS container startup
**Constraints**: No writes to `~/.config/opencode/`; no pre-seeded credential DB; curl from live GitHub URLs only

---

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Zero-Install Binary | N/A | This is a test harness, not a tool in the binary |
| II. ObjectScript Sanity | N/A | Harness loads pre-verified fixture classes; no new ObjectScript API calls |
| III. HTTP-First Execution | N/A | Test harness; does not add Docker-required tools to the binary |
| IV. Test-First, Fixture-Driven | PASS | Fixtures committed to `tests/e2e/tasks/`; harness structure written before implementation |
| V. Output Shape Parity | N/A | New harness, no existing shape to match |
| VI. Environment Guard | PASS | `OPENAI_API_KEY` and `IRIS_CONTAINER` gated; US1 skips US2/US3 when IRIS absent |
| VII. Dependency Minimalism | PASS | All dependencies (`requests`, `pyyaml`, `pytest`) already present or stdlib |

No violations. Plan may proceed to implementation.

---

## Project Structure

```text
tests/
└── e2e/
    ├── harness.py              # Main harness entry point (CLI + pytest runner)
    ├── isolated_env.py         # IsolatedEnv: temp dir, OPENCODE_CONFIG_CONTENT assembly
    ├── opencode_runner.py      # Spawn opencode run, parse --format json event stream
    ├── assertions.py           # AssertionRunner: code block regex, tool call checks
    ├── fixtures.py             # Load HarnessTask fixtures into IRIS via iris-agentic-dev MCP
    ├── readme_validator.py     # Validate and execute README curl commands
    ├── result_writer.py        # Write RunResult JSON
    ├── tasks/                  # HarnessTask YAML fixtures
    │   ├── SKILL-01.yaml       # US1: Return-in-loop bug, objectscript-review skill
    │   ├── SKILL-01-baseline.yaml  # US1: same task, no skill (baseline)
    │   ├── MCP-01.yaml         # US2: compile task, iris_compile assertion
    │   └── FULL-01.yaml        # US3: introspect + generate + compile chain
    └── results/                # RunResult JSON output (gitignored except CI artifacts)

specs/039-skills-e2e/
├── plan.md                     # This file
├── research.md                 # Phase 0 output
├── data-model.md               # Phase 1 output
├── contracts/
│   └── harness_cli.md          # CLI and event stream contracts
└── tasks.md                    # Phase 2 output (from /speckit.tasks)
```

---

## Phase 1: Core infrastructure (no LLM calls)

**Goal**: IsolatedEnv, README validator, and event stream parser all working and unit-tested before any real OpenCode invocation.

### P1-A: IsolatedEnv + README validator

`tests/e2e/isolated_env.py`:
- Creates `mkdtemp()` skills dir
- Assembles `OPENCODE_CONFIG_CONTENT` JSON: `{"provider": {"openai": {"options": {"apiKey": "..."}}}, "skills": {"paths": [skills_dir]}}`
- Sets `OPENCODE_DB=/tmp/opencode-harness-{run_id}.db`
- Context manager: tears down on exit, retains on `--keep-on-failure`

`tests/e2e/readme_validator.py`:
- Parses `light-skills/README.md` to extract all curl commands (regex on ` ```bash` blocks containing `curl -sL ... SKILL.md`)
- For each URL: HTTP HEAD → assert 200; raises `ReadmeValidationError` with URL + README line number on failure
- Executes the curl command exactly (subprocess) into the isolated skills dir
- Unit test: mock HTTP 200/404, verify correct error on 404

### P1-B: Event stream parser

`tests/e2e/opencode_runner.py`:
- `run_opencode(prompt, env_vars, timeout=180)` → yields parsed event dicts
- Handles: `tool_use`, `text`, `error`, unknown (ignored)
- `parse_mcp_tool(tool_name)` → `(server, tool)` or `(None, tool)` for built-ins
- Unit test: feed synthetic JSON event lines, assert correct parsing of tool names and outputs

`tests/e2e/assertions.py`:
- `extract_code_blocks(text)` → list of strings (contents of ` ```objectscript` and ` ```cls` blocks)
- `check_absent_pattern(code_blocks, pattern)` → bool (True = pattern absent = PASS)
- `Return-in-For` pattern: `r'For\b[^\n]*\n(?:[^\n]*\n)*?[^\n]*\bReturn\b'` applied within each block
- `check_tool_called(events, server, tool)` → bool
- Unit test: fixture with known Return-in-loop code, assert pattern detected; fixture without, assert clean

---

## Phase 2: Task fixtures and US1 (skills quality)

**Goal**: US1 running end-to-end against real OpenCode + real LLM.

### P2-A: Task fixture files

`tests/e2e/tasks/SKILL-01.yaml`:
```yaml
id: SKILL-01
category: skills_quality
scenario: us1_skills_only
description: "objectscript-review catches Return-in-For-loop bug"
prompt: "Fix the bug in this ObjectScript method:\n\nMethod FindFirst(list As %List) As %String {\n  For i=1:1:$ListLength(list) {\n    If $List(list, i) '= \"\" { Return $List(list, i) }\n  }\n  Return \"\"\n}"
fixtures: []
skills_to_install:
  - objectscript-review
assertions:
  - type: code_absent_pattern
    pattern: 'For\b[^\n]*\n(?:[^\n]*\n)*?[^\n]*\bReturn\b'
    description: "Fixed code must not use Return inside a For loop body"
    required: true
expected_tool_calls: []
```

`tests/e2e/tasks/SKILL-01-baseline.yaml`: same but `skills_to_install: []`

### P2-B: US1 harness runner

`tests/e2e/harness.py` US1 path:
1. Validate README curl URLs (readme_validator)
2. Create IsolatedEnv
3. Curl-install skills listed in task
4. Run `opencode run {prompt} --format json --dangerously-skip-permissions` with env
5. Collect events; extract text blocks; run assertions
6. Write TaskResult; compare baseline vs skill if both ran
7. Exit 0/1 per assertion results

pytest test: `test_us1_skill_quality` — parametrized over SKILL-01 + SKILL-01-baseline

**Phase gate**: `test_us1_skill_quality` passes with skill installed, baseline < 100%.

---

## Phase 3: IRIS fixtures and US2 (MCP tools)

**Goal**: US2 running with live `iris-skills-e2e` container.

### P3-A: IRIS fixture loader

`tests/e2e/fixtures.py`:
- Reads `HarnessTask.fixtures` list
- Loads `.cls` content into IRIS via `iris_agentic_dev:iris_doc` (write) + `iris_agentic_dev:iris_compile`
- Uses the same `opencode run` invocation as the main harness (no separate MCP client)
- Alternative: direct Atelier REST call using `requests` (simpler, no LLM overhead for fixture loading)

Decision: use Atelier REST directly for fixture loading — cleaner, no LLM token cost, faster.

`tests/e2e/tasks/MCP-01.yaml`:
```yaml
id: MCP-01
category: mcp_tools
scenario: us2_mcp
description: "iris_compile is called and returns a real IRIS result"
prompt: "Compile the class User.HarnessTestClass"
fixtures:
  - type: cls
    name: User.HarnessTestClass
    content: |
      Class User.HarnessTestClass Extends %RegisteredObject
      {
      ClassMethod Hello() As %String [ CodeMode = expression ]
      {
      "Hello"
      }
      }
skills_to_install: []
assertions:
  - type: tool_called
    pattern: "iris_agentic_dev:iris_compile"
    description: "iris_compile MCP tool must be called"
    required: true
  - type: tool_output_contains
    pattern: "iris_compile"
    description: "compile result must be present"
    required: true
expected_tool_calls:
  - "iris_agentic_dev:iris_compile"
```

### P3-B: US2 harness + MCP config injection

Extend `IsolatedEnv` to also inject `iris-agentic-dev` MCP config into `OPENCODE_CONFIG_CONTENT`:
```json
{
  "provider": {"openai": {"options": {"apiKey": "..."}}},
  "skills": {"paths": ["{skills_dir}"]},
  "mcp": {
    "iris-agentic-dev": {
      "type": "local",
      "command": ["/opt/homebrew/bin/iris-agentic-dev", "mcp"],
      "enabled": true,
      "environment": {
        "IRIS_HOST": "localhost",
        "IRIS_WEB_PORT": "{iris_web_port}",
        "IRIS_CONTAINER": "{iris_container}",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "SYS",
        "IRIS_NAMESPACE": "USER"
      }
    }
  }
}
```

Guard: skip US2/US3 if `IRIS_CONTAINER` not set (same `require_iris` pattern as Rust E2E tests).

**Phase gate**: `test_us2_mcp_compile` passes — `iris_agentic_dev:iris_compile` in tool calls.

---

## Phase 4: US3 full stack + CI integration

### P4-A: Full stack task + US3

`tests/e2e/tasks/FULL-01.yaml` — introspect + generate test + compile chain:
```yaml
id: FULL-01
scenario: us3_full_stack
prompt: "Write and compile a %UnitTest for User.HarnessTestClass. Use docs_introspect first to read the class, then write the test class, then compile it."
fixtures:
  - type: cls
    name: User.HarnessTestClass
    content: "..."
skills_to_install:
  - objectscript-unit-test
assertions:
  - type: tool_called
    pattern: "iris_agentic_dev:docs_introspect"
    required: true
  - type: tool_called
    pattern: "iris_agentic_dev:iris_compile"
    required: true
```

### P4-B: CI jobs

Add to `.github/workflows/ci.yml`:

```yaml
skills-e2e:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: actions/setup-python@v5
      with: { python-version: "3.11" }
    - run: npm install -g opencode-ai
    - run: pip install requests pyyaml pytest
    - run: python -m pytest tests/e2e/ -k "us1" --scenario us1
      env:
        OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}

skills-e2e-full:
  runs-on: ubuntu-latest
  if: github.ref == 'refs/heads/master'
  steps:
    - uses: actions/checkout@v4
    - uses: actions/setup-python@v5
      with: { python-version: "3.11" }
    - run: npm install -g opencode-ai
    - run: pip install requests pyyaml pytest
    - name: Start IRIS container
      run: |
        docker run -d --name iris-skills-e2e \
          -p 52773:52773 \
          intersystemsdc/iris-community:2025.1
        # Wait for Atelier API (reuse existing wait pattern from e2e-tests job)
    - run: python -m pytest tests/e2e/ --scenario all
      env:
        OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        IRIS_CONTAINER: iris-skills-e2e
        IRIS_WEB_PORT: 52773
```

**Phase gate**: Both CI jobs green on master.

---

## Phased delivery

| Phase | Deliverable | Gate |
|-------|-------------|------|
| 1 | IsolatedEnv, README validator, event parser — all unit tested, no LLM | All unit tests pass |
| 2 | SKILL-01 fixtures, US1 harness, real OpenCode + real LLM | `test_us1_skill_quality` passes; baseline < skill |
| 3 | MCP-01 fixtures, US2 harness, live IRIS | `test_us2_mcp_compile` passes |
| 4 | FULL-01 fixtures, US3, CI jobs wired | CI green on master |

---

## Risks and mitigations

| Risk | Mitigation |
|------|-----------|
| OpenCode `--format json` schema changes | Pin OpenCode version in CI; add schema version check to event parser |
| `objectscript-review` skill doesn't always catch the bug (LLM non-determinism) | Run baseline comparison; accept that US1 measures lift, not 100% determinism |
| IRIS container startup time exceeds 8 min budget | Reuse existing `e2e-tests` wait script; parallelize fixture loading |
| GitHub raw URL for skill files returns 404 after rename | This is a feature, not a bug — the harness correctly catches it |
