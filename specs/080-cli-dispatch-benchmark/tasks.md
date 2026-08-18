# Tasks 080: CLI Tool Dispatch Benchmark

TDD order: tests first, then implementation, then integration/validation.

---

## Phase 0 — Schema and Types (no behavior yet)

These tasks add the new types and extend existing structs without changing behavior.
All existing tests must pass after each task.

- [ ] **T001** Add `BenchmarkMode` enum to `benchmark/mod.rs`
  - `Mcp` (default), `CliDispatch`; `#[serde(rename_all = "snake_case")]`
  - Unit test: `BenchmarkMode::Mcp` serializes to `"mcp"`, `CliDispatch` to `"cli_dispatch"`
  - Unit test: existing `BenchmarkResult` JSON without a `mode` field deserializes with `mode: None`

- [ ] **T002** Extend `TaskResult` with optional token fields
  - Add `tokens_input: Option<u32>`, `tokens_output: Option<u32>`, `tokens_total: Option<u32>`
  - All default to `None` via `#[serde(default)]`
  - Unit test: round-trip serialize/deserialize preserves `None` and `Some(42)` values
  - Confirm: existing `TaskResult` unit tests still pass

- [ ] **T003** Extend `BenchmarkResult` with mode, token, and comparison fields
  - Add `mode: Option<BenchmarkMode>`, `tokens_input/output/total: Option<u64>`,
    `comparison: Option<BenchmarkComparison>` (skip_serializing_if None)
  - Add `BenchmarkComparison` struct
  - Unit test: `from_task_results` still computes `pass_rate` correctly when tasks have
    `tokens_total: Some(100)` — aggregate sum is populated
  - Unit test: `BenchmarkResult` without new fields deserializes without error

- [ ] **T004** Add `TokenUsage` type and `complete_with_usage` to `LlmClient`
  - Add `pub struct TokenUsage { pub input: u32, pub output: u32 }` to `generate.rs`
  - Add `pub async fn complete_with_usage(&self, system: &str, user: &str) -> Result<(String, Option<TokenUsage>)>`
  - Extend `AnthropicResponse` to deserialize `usage.input_tokens`/`usage.output_tokens`
  - Extend `OpenAiResponse` to deserialize `usage.prompt_tokens`/`usage.completion_tokens`
  - Unit test (mock model): `complete_with_usage` returns `(text, None)` for mock model
  - Unit test: `AnthropicResponse` with a `usage` field deserializes `TokenUsage` correctly
  - Unit test: `OpenAiResponse` with a `usage` field deserializes `TokenUsage` correctly
  - Confirm: existing `complete()` callers unchanged

---

## Phase 1 — Unit Tests for CLI Dispatch Logic (tests before implementation)

Write all unit tests in this phase. They will fail until Phase 2 is complete.

- [ ] **T005** Write unit tests for `parse_tool_invocations`
  - File: `crates/iris-agentic-dev-core/src/benchmark/cli_dispatch.rs` (create stub)
  - Test: line `iris-agentic-dev tool iris_compile --args '{"doc":"Foo.cls"}'`
    → one `ToolInvocation { tool_name: "iris_compile", args_json: "{...}" }`
  - Test: prose-only response → empty `Vec`
  - Test: mixed prose + tool line → one invocation, prose ignored
  - Test: malformed JSON args → invocation is still returned (no parse error at this stage)
  - Test: multiple tool lines in one response → multiple invocations in order

- [ ] **T006** Write unit tests for sentinel detection
  - Test: response containing `===FIXED_CLASS_START===\nClass Foo {}\n===FIXED_CLASS_END===`
    → `extract_sentinel_class` returns `Some("Class Foo {}")`
  - Test: response without sentinel → `None`
  - Test: sentinel present with surrounding prose → class source extracted correctly

- [ ] **T007** Write unit test for loop limit (S-008)
  - Use a fake `AgentLoop` with an injectable `LlmCaller` trait and a fake subprocess runner
  - Fake LLM always returns a tool invocation line (never the sentinel)
  - Assert: with `max_iterations = 2`, the LLM is called exactly 2 times
  - Assert: outcome is `TaskOutcome::Fail`
  - Assert: no panic, no hang

- [ ] **T008** Write unit tests for `build_cli_dispatch_system_prompt`
  - Test: empty skill → prompt contains tool list, sentinel instructions, no "Skill guidance" section
  - Test: non-empty skill → prompt contains "# Skill guidance" before tool list
  - Test: prompt contains all required tool names from FR-002

- [ ] **T009** Write unit test for `run_tool_subprocess` error handling (S-005)
  - Use a fake binary path that does not exist → function returns an error string, does not panic
  - Use a real path to `/bin/sh -c 'exit 1'` → function returns non-empty error string

---

## Phase 2 — Implement CLI Dispatch Module

- [ ] **T010** Implement `parse_tool_invocations` in `benchmark/cli_dispatch.rs`
  - Scan response lines for `iris-agentic-dev tool <name> --args '<json>'` pattern
  - All T005 tests pass

- [ ] **T011** Implement `extract_sentinel_class` in `benchmark/cli_dispatch.rs`
  - Detect `===FIXED_CLASS_START===` / `===FIXED_CLASS_END===` sentinel
  - Fall back to `extract_fixed_classes` from `llm.rs` if sentinel absent (agent may
    output class source directly without using the sentinel)
  - All T006 tests pass

- [ ] **T012** Implement `build_cli_dispatch_system_prompt`
  - Builds the system prompt per FR-002
  - All T008 tests pass

- [ ] **T013** Implement `run_tool_subprocess`
  - `std::process::Command` using `binary_path`, `arg("tool")`, `arg(name)`, `arg("--args")`, `arg(args_json)`
  - Set IRIS connection env vars from `CliDispatchConfig::connection_env`
  - Capture stdout+stderr, return combined string; never panic
  - All T009 tests pass

- [ ] **T014** Implement the agentic loop in `run_cli_dispatch_task`
  - Uses `complete_with_usage` for each turn
  - Accumulates token counts
  - Stops on sentinel, `max_iterations`, `max_task_tokens`, or timeout
  - After sentinel: calls `container::write_and_compile` + `container::run_class_tests`
  - All T007 tests pass

- [ ] **T015** Implement `run_cli_dispatch_suite`
  - Sequential task loop (matching `run_suite` in `mod.rs`)
  - Aggregates token totals into `BenchmarkResult`
  - Sets `mode: Some(BenchmarkMode::CliDispatch)`

---

## Phase 3 — CLI Wiring

- [ ] **T016** Extend `cmd/benchmark.rs` with new flags
  - Add `--mode`, `--max-iterations`, `--max-task-tokens`, `--compare`
  - Route `"mcp"` → existing path (unchanged), `"cli-dispatch"` → new path
  - Unknown mode → exit 1 with `UNKNOWN_MODE`
  - Unit test: `--mode foo` exits 1

- [ ] **T017** Implement `--compare` logic
  - Load prior result JSON, compute deltas, attach `BenchmarkComparison`
  - Unit test: two `BenchmarkResult` values → `BenchmarkComparison` computes correct deltas
  - Unit test: prior result has no token data → `tokens_total_delta: null`

---

## Phase 4 — Integration Tests (live IRIS, mock LLM)

All tests in this phase run against the live `iris-dev-iris` container. Mark
`#[ignore]` per project convention. Run with `--include-ignored --test-threads=1`.

- [ ] **T018** Integration test: S-001 — valid result JSON (mock LLM, single task)
  - Override to run only 1 task to keep CI time bounded
  - Assert: JSON valid, `mode: "cli_dispatch"`, `pass_rate` in [0.0, 1.0]

- [ ] **T019** Integration test: S-004 — prose-only agent terminates cleanly
  - Fake LLM returns only prose, no tool invocations, no sentinel
  - Assert: outcome is `"fail"`, run completes in bounded time

- [ ] **T020** Integration test: S-005 — malformed JSON in tool call fed back to agent
  - Fake LLM returns one malformed tool line, then on second turn returns sentinel
  - Assert: loop proceeds, no panic, final outcome is either pass or fail

- [ ] **T021** Integration test: S-006 — IRIS unreachable mid-benchmark
  - Run with an IRIS connection that returns errors after a configurable number of calls
  - Assert: errored tasks have `outcome: "error"` and `reason` containing `IRIS_UNREACHABLE`
  - Assert: run completes all tasks

- [ ] **T022** Integration test: S-007 — `--mode mcp` explicit equals default (regression)
  - Run once with no `--mode`, once with `--mode mcp`
  - Assert: `pass_rate` and per-task `outcome` values are identical

---

## Phase 5 — Live LLM Integration Tests (require API key)

Mark `#[ignore]` — run manually or in nightly CI with key available.

- [ ] **T023** Live integration test: S-002 — token counts appear in result
  - Real LLM call (Anthropic or OpenAI)
  - Assert: `tokens_input > 0`, `tokens_output > 0`, `task_results[0].tokens_total > 0`

- [ ] **T024** Live integration test: end-to-end CLI dispatch on full jira suite
  - Full 22-task run with a real model
  - Assert: `pass_rate >= 0.3` (sanity floor, not a quality gate)
  - Assert: `tokens_total` is present and reasonable (> 0, < 10,000,000)
  - Record result to `tests/e2e/results/cli-dispatch-baseline.json` for future comparison

---

## Phase 6 — Documentation and Cleanup

- [ ] **T025** Update `skills/BENCHMARKING.md`
  - Add "CLI Dispatch Mode" section after the existing Quick Start
  - Document `--mode cli-dispatch`, `--max-iterations`, `--max-task-tokens`, `--compare`
  - Add example output with token fields
  - Add interpretation note on token-cost differences
  - Run `markdownlint-cli2 --fix skills/BENCHMARKING.md && prettier --write skills/BENCHMARKING.md`

- [ ] **T026** Confirm coverage floor still passes
  - Run `cargo test --features testing` after all implementation tasks
  - If coverage drops below current floor (88%), identify which new code is uncovered and
    add targeted unit tests

---

## Dependencies

```text
T001 → T002 → T003 → T004  (schema/types, in order)
T005, T006, T007, T008, T009  (can run in parallel after T004)
T010 → T005 (T010 makes T005 pass)
T011 → T006 (T011 makes T006 pass)
T012 → T008
T013 → T009
T014 → T007, T010, T011, T012, T013
T015 → T014
T016 → T015
T017 → T016
T018..T022 → T016 (integration tests need wired CLI)
T023, T024 → T022
T025 → T024
T026 → T025
```
