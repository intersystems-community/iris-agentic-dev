# Tasks: Deep E2E Skills Harness (039)

## Feature
Build a Python test harness that simulates a new user following `light-skills/README.md` end-to-end: isolated OpenCode environment, real LLM, GitHub curl installs, skill quality assertions, and live IRIS MCP tool verification.

## Files
- `tests/e2e/harness.py`
- `tests/e2e/isolated_env.py`
- `tests/e2e/opencode_runner.py`
- `tests/e2e/assertions.py`
- `tests/e2e/fixtures.py`
- `tests/e2e/readme_validator.py`
- `tests/e2e/result_writer.py`
- `tests/e2e/tasks/SKILL-01.yaml`
- `tests/e2e/tasks/SKILL-01-baseline.yaml`
- `tests/e2e/tasks/MCP-01.yaml`
- `tests/e2e/tasks/FULL-01.yaml`
- `.github/workflows/ci.yml` (modified)

---

## Phase 1: Setup

No new project initialization needed — `tests/` directory and pytest are already present in the repo. This phase creates the directory structure and shared utilities.

- [x] T001 Create `tests/e2e/` directory with `__init__.py`, `conftest.py` (shared pytest fixtures: `openai_api_key`, `iris_available`, `iris_web_port`, `iris_container_name`)
- [x] T002 Add `requests` and `pyyaml` to `Cargo.toml` dev-dependencies or `requirements-test.txt` (whichever is appropriate for the existing test infra — check how `benchmark/021/` manages deps)
- [x] T003 Create `tests/e2e/results/` directory with `.gitignore` (ignore `*.json` result files, keep dir)
- [x] T004 Create `tests/e2e/tasks/` directory with `__init__.py`

---

## Phase 2: Foundational (Blocking — all US phases depend on these)

**Purpose**: Core infrastructure modules used by all three user story phases. Must be complete and unit-tested before any US phase begins.

- [x] T005 Write unit tests for `IsolatedEnv` in `tests/e2e/test_isolated_env.py`: test temp dir creation/teardown, `OPENCODE_CONFIG_CONTENT` JSON assembly — assert `provider.openai.options.apiKey` path (not `provider.openai.apiKey`), skills path present, MCP block present when `with_mcp()` called; `OPENCODE_DB` path isolation; retain-on-failure flag
- [x] T006 Implement `tests/e2e/isolated_env.py`: `IsolatedEnv` context manager — creates `mkdtemp()` skills dir, assembles `OPENCODE_CONFIG_CONTENT` with `{"provider":{"openai":{"options":{"apiKey":"..."}}}, "skills":{"paths":[...]}}` (note `options` nesting), sets `OPENCODE_DB=/tmp/opencode-harness-{run_id}.db`, tears down on exit (retains on `--keep-on-failure`)
- [x] T007 Write unit tests for event stream parser in `tests/e2e/test_opencode_runner.py`: feed synthetic JSON lines, assert correct parsing of `tool_use` events, MCP tool name splitting (`iris_agentic_dev:iris_compile` → server + tool), text event accumulation, error event handling, unknown event types silently ignored; test `read_session_db` with a minimal SQLite fixture returns rows without error
- [x] T008 Implement `tests/e2e/opencode_runner.py`: `run_opencode(prompt, env_vars, timeout=180)` → yields parsed event dicts from `opencode run "{prompt}" --format json --dangerously-skip-permissions` subprocess; `parse_mcp_tool(name)` → `(server, tool)` tuple; `read_session_db(db_path)` → list of raw session rows for post-run debugging (FR-006 secondary path — event stream is primary)
- [x] T009 Write unit tests for assertion helpers in `tests/e2e/test_assertions.py`: `extract_code_blocks` on text with/without fenced blocks, `check_absent_pattern` with known Return-in-loop fixture (assert detected) and clean fixture (assert clean), `check_tool_called` with synthetic event list
- [x] T010 Implement `tests/e2e/assertions.py`: `extract_code_blocks(text)` extracts ` ```objectscript` and ` ```cls` block contents; `check_absent_pattern(blocks, pattern)` → bool (True = absent = PASS); `check_tool_called(events, server, tool)` → bool; Return-in-For pattern: `r'For\b[^\n]*\n(?:[^\n]*\n)*?[^\n]*\bReturn\b'`
- [x] T011 Write unit tests for `ReadmeValidator` in `tests/e2e/test_readme_validator.py`: mock HTTP HEAD 200 → no error; mock 404 → `ReadmeValidationError` with URL and README line number; verify extracted curl commands match README content
- [x] T012 Implement `tests/e2e/readme_validator.py`: parse `light-skills/README.md` for curl commands in ` ```bash` blocks, HTTP HEAD each URL, raise `ReadmeValidationError(url, line_number)` on non-200, execute curl into provided skills dir
- [x] T013 Write unit tests for `ResultWriter` in `tests/e2e/test_result_writer.py`: assert output JSON schema matches `RunResult` data model (run_id, harness, tasks[], summary), assert skill_lift computed correctly when baseline+skill both present
- [x] T014 Implement `tests/e2e/result_writer.py`: `write_result(run_result, output_dir)` → `{output_dir}/{run_id}.json`; `compute_lift(baseline_results, skill_results)` → float

**Checkpoint**: All T005–T014 unit tests pass (`pytest tests/e2e/test_*.py`). Foundation ready. Note: `read_session_db` in T008 is a secondary debugging aid (FR-006); primary assertions use the event stream.

---

## Phase 3: US1 — Skills quality gate (Priority: P1) 🎯 MVP

### Goal
Validate that a new user installing `objectscript-review` via the README curl commands gets measurably better ObjectScript output from OpenCode — and that broken README URLs are caught immediately.

### Independent test criteria
`test_us1_skill_quality` passes: README URLs validate, skill installs, OpenCode LLM output for the Return-in-loop task does NOT contain `Return` inside a `For` loop when skill is active.

### Tasks

- [x] T015 [US1] Write E2E test `test_us1_readme_urls_valid` in `tests/e2e/test_us1.py`: instantiate `ReadmeValidator`, call `validate_urls()`, assert all return HTTP 200 (requires network; mark `@pytest.mark.network`)
- [x] T016 [US1] Write E2E test `test_us1_skill_quality` in `tests/e2e/test_us1.py`: create `IsolatedEnv`, install `objectscript-review` via `ReadmeValidator`, run OpenCode with `SKILL-01` task prompt, parse events, extract code blocks from text output, assert `check_absent_pattern(blocks, RETURN_IN_FOR_PATTERN)` is True
- [x] T017 [US1] Write E2E test `test_us1_baseline` in `tests/e2e/test_us1.py`: same as T016 but `skills_to_install=[]`; assert the run completes (result recorded); this establishes the baseline — it is NOT required to fail, just to run
- [x] T018 [P] [US1] Create task fixture `tests/e2e/tasks/SKILL-01.yaml`: id=SKILL-01, scenario=us1_skills_only, prompt contains the buggy `FindFirst` method with `Return` inside `For`, skills_to_install=[objectscript-review], assertions=[code_absent_pattern with Return-in-For regex]
- [x] T019 [P] [US1] Create task fixture `tests/e2e/tasks/SKILL-01-baseline.yaml`: same as SKILL-01 but `skills_to_install: []`
- [x] T020 [US1] Implement `tests/e2e/harness.py` US1 runner: `run_scenario(task_yaml, isolated_env)` — installs skills, runs opencode, collects events, runs assertions, writes `TaskResult`; `main()` CLI entry point with `--scenario`, `--model`, `--baseline`, `--output` flags

**Phase gate**: `pytest tests/e2e/test_us1.py -m "not network or network"` passes with `OPENAI_API_KEY` set. `test_us1_skill_quality` passes. `test_us1_baseline` completes (recorded). Skill lift observable in results JSON.

---

## Phase 4: US2 — MCP tools against live IRIS (Priority: P1)

### Goal
Validate that a new user who configures `iris-agentic-dev` in the isolated OpenCode config can compile a class — the AI calls `iris_compile`, IRIS executes it, and the result is captured in the session log.

### Independent test criteria
`test_us2_mcp_compile` passes: `iris_agentic_dev:iris_compile` appears in tool_use events with `status: completed` and non-empty output.

### Tasks

- [x] T021 [US2] Write unit test `test_iris_fixture_loader` in `tests/e2e/test_fixtures.py`: mock Atelier REST `PUT /doc` and `POST /action/compile` calls, assert `load_fixture(cls_fixture, host, port)` makes correct requests with correct body
- [x] T022 [US2] Implement `tests/e2e/fixtures.py`: `load_fixture(fixture, iris_host, iris_web_port, iris_username, iris_password, iris_namespace)` — direct Atelier REST calls (`PUT /api/atelier/v1/{ns}/doc/{name}.cls` + `POST /api/atelier/v1/{ns}/action/compile`); no LLM invocation for fixture loading
- [x] T023 [US2] Write E2E test `test_us2_mcp_compile` in `tests/e2e/test_us2.py`: skip if `IRIS_CONTAINER` not set (`pytest.mark.skipif`); create `IsolatedEnv` with MCP config block; load `MCP-01` fixture into IRIS via `fixtures.py`; run OpenCode with compile prompt; assert `check_tool_called(events, "iris_agentic_dev", "iris_compile")` is True and tool output is non-empty
- [x] T024 [US2] Write E2E test `test_us2_check_config` in `tests/e2e/test_us2.py`: skip if no IRIS; run OpenCode with prompt "call check_config"; assert `iris_agentic_dev:check_config` in events with output containing `"connected": true`
- [x] T025 [P] [US2] Create task fixture `tests/e2e/tasks/MCP-01.yaml`: id=MCP-01, scenario=us2_mcp, prompt="Compile the class User.HarnessTestClass", fixture `User.HarnessTestClass` (simple class with one ClassMethod), assertions=[tool_called iris_agentic_dev:iris_compile]
- [x] T026 [US2] Extend `tests/e2e/isolated_env.py` with `with_mcp(iris_host, iris_web_port, iris_container)` method — adds `iris-agentic-dev` MCP server block to the `OPENCODE_CONFIG_CONTENT` JSON alongside the provider and skills keys

**Phase gate**: `pytest tests/e2e/test_us2.py` passes with `IRIS_CONTAINER` and `IRIS_WEB_PORT` set. `test_us2_mcp_compile` shows `iris_agentic_dev:iris_compile` in tool calls.

---

## Phase 5: US3 — Full stack (Priority: P2)

### Goal
Validate that skills + MCP compose correctly: introspect a class, generate a unit test guided by `objectscript-unit-test` skill, compile the generated test.

### Independent test criteria
`test_us3_full_stack` passes: session log shows `docs_introspect` called before `iris_compile`, and compile result is present.

### Tasks

- [x] T027 [US3] Write E2E test `test_us3_full_stack` in `tests/e2e/test_us3.py`: skip if no IRIS; create `IsolatedEnv` with MCP config + `objectscript-unit-test` skill; load `FULL-01` fixture; run OpenCode with "write and compile a unit test for User.HarnessTestClass" prompt; assert both `iris_agentic_dev:docs_introspect` and `iris_agentic_dev:iris_compile` appear in events in that order
- [x] T028 [P] [US3] Create task fixture `tests/e2e/tasks/FULL-01.yaml`: id=FULL-01, scenario=us3_full_stack, skills_to_install=[objectscript-unit-test], fixture=User.HarnessTestClass, assertions=[tool_called docs_introspect (required), tool_called iris_compile (required)]
- [x] T029 [US3] Implement ordered tool assertion `check_tools_in_order(events, tools_list)` in `tests/e2e/assertions.py` — asserts each tool in `tools_list` appears in events and in the specified order; add unit test in `test_assertions.py`

**Phase gate**: `pytest tests/e2e/test_us3.py` passes with IRIS available. Both tool calls confirmed in order.

---

## Phase 6: CI integration and polish

- [x] T030 Add `skills-e2e` CI job to `.github/workflows/ci.yml`: ubuntu-latest, install opencode-ai via npm, install Python deps, run `pytest tests/e2e/ -m "us1 and not network_curl" -k "not us2 and not us3"` with `OPENAI_API_KEY` secret; runs on all pushes
- [x] T031 Add `skills-e2e-full` CI job to `.github/workflows/ci.yml`: ubuntu-latest, `if: github.ref == 'refs/heads/master'`, start `iris-skills-e2e` container (same image + wait pattern as existing `e2e-tests` job), run full pytest suite with `OPENAI_API_KEY` + `IRIS_CONTAINER=iris-skills-e2e` + `IRIS_WEB_PORT=52773` secrets
- [x] T032 Add `@pytest.mark.network_curl` to `test_us1_readme_urls_valid` and ensure it runs in the `skills-e2e` CI job (not skipped) — this is the README drift detection gate
- [x] T033 Add `--keep-on-failure` flag to CI `skills-e2e-full` job via `pytest --keep-on-failure` and upload `tests/e2e/results/` + `/tmp/opencode-harness-*/` as CI artifacts on failure
- [x] T034 Update `light-skills/README.md` `60-second setup` section to add verification step: "run `pytest tests/e2e/test_us1.py` to verify your setup end-to-end"
- [x] T035 Run full test suite locally: `pytest tests/e2e/` with IRIS available — assert all phases pass, result JSON written, lift measurable in summary

---

## Dependency Graph

```
T001-T004 (setup)
    ↓
T005-T014 (foundational — all unit tests + core modules)
    ↓
T015-T020 (US1 — no IRIS needed) ──────────────────────────────────────────────┐
    ↓                                                                           │
T021-T026 (US2 — requires IRIS)                                                │
    ↓                                                                           │
T027-T029 (US3 — requires IRIS)                                                │
    ↓                                                                           │
T030-T035 (CI + polish — requires all phases green) ←──────────────────────────┘
```

Parallelizable within each phase: T015/T016/T017 can run in parallel (different test functions + fixtures); T018/T019 in parallel (different YAML files); T021/T025 in parallel (different files); T027/T028 in parallel.

## MVP Scope

T001–T020 (Phases 1–3) deliver the P1 skill quality gate and README drift detection with no IRIS dependency. This alone validates the core value claim and can be merged independently before US2/US3.

## Total: 35 tasks across 6 phases
- Phase 1 (Setup): 4 tasks
- Phase 2 (Foundational): 10 tasks
- Phase 3 (US1): 6 tasks
- Phase 4 (US2): 6 tasks
- Phase 5 (US3): 3 tasks
- Phase 6 (Polish + CI): 6 tasks
