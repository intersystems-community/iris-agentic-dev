# Spec 080: CLI Tool Dispatch Benchmark

## Problem

`iris-agentic-dev` has three distinct execution paths. The benchmark harness (spec 059)
measures skill effectiveness against the MCP server path only. CLI tool dispatch
(`iris-agentic-dev tool <name> <json>`) has never been benchmarked — there is no data on
whether an agent using shell commands instead of MCP tools can complete the same repair
tasks, how many tokens it costs, or how long it takes.

Keshav Iyer and other community members who wire skill repos to iris-agentic-dev need to
make an informed choice between execution modes. Today they have no data to inform that
decision.

## User Stories

### P1 — Measure CLI dispatch effectiveness vs MCP baseline

**As** a skill author or integration developer,
**I want** a benchmark result that shows CLI dispatch pass rate on the same jira task suite,
**So that** I know whether CLI dispatch is a viable agent execution path before investing in it.

Acceptance: running `iris-agentic-dev benchmark --mode cli-dispatch --skill <file>` produces a
JSON result with `pass_rate`, `tasks_passed`, `tasks_total`, `tasks_errored`, and `mode: "cli_dispatch"`.

### P2 — Token cost comparison between CLI and MCP modes

**As** a skill author or integration developer,
**I want** per-task and aggregate token counts for CLI dispatch, comparable to MCP mode,
**So that** I can evaluate the token-efficiency trade-off between the two modes before
choosing one for my integration.

Acceptance: the JSON result includes `tokens_input`, `tokens_output`, and `tokens_total`
fields at both per-task and aggregate level when the underlying API returns usage data.

### P3 — Latency comparison

**As** a skill author or integration developer,
**I want** per-task and aggregate latency for CLI dispatch shown side-by-side with MCP mode,
**So that** I understand the overhead of the subprocess-per-tool-call pattern.

Acceptance: per-task `elapsed_s` is present (already in `TaskResult`). Aggregate `elapsed_s`
is present (already in `BenchmarkResult`). A `--compare` flag loads a prior MCP result JSON
and adds a `comparison` section to the output.

---

## What CLI Dispatch Means for Benchmarking

The existing MCP benchmark gives the agent a single-shot prompt and expects it to return
corrected ObjectScript class source directly. That is not an agentic loop — it is one
`complete()` call.

CLI dispatch benchmark is categorically different: the agent runs an agentic loop where
it decides to call `iris-agentic-dev tool <name> <json>` as shell subprocess commands,
reads the output, and iterates until it believes the task is solved. The harness must
orchestrate that loop and verify the final state against the same pass/fail criteria the
MCP benchmark uses (compile success + test pass).

This means the CLI dispatch benchmark requires:

- An agentic loop that calls the LLM with tool-use (or bash execution capability)
- Per-turn token counting (new capability — `LlmClient::complete` does not return usage today)
- Subprocess invocation of `iris-agentic-dev tool` commands
- Detection of loop termination (agent declares done, or iteration/token/time limit hit)

The LLM is prompted differently: instead of MCP tools being injected by the SDK, it
receives a system prompt describing the CLI invocation pattern.

---

## Functional Requirements

### FR-001 — New benchmark mode flag

`iris-agentic-dev benchmark` gains a `--mode` flag with values `mcp` (default, current
behavior) and `cli-dispatch` (new). Omitting `--mode` preserves existing behavior exactly.

### FR-002 — CLI dispatch system prompt

In CLI dispatch mode the LLM receives a system prompt that describes:

- The task (same bug description, goal, expected behavior, initial code as today)
- How to invoke tools: `iris-agentic-dev tool <name> --args '<json>'`
- The available tools relevant to the repair task (at minimum: `iris_compile`, `iris_execute`,
  `iris_search`, `iris_symbols`)
- How to signal completion: output the corrected class source wrapped in a sentinel block

### FR-003 — Agentic loop

The harness runs an agentic loop for each task:

1. Send the task prompt to the LLM
2. Parse tool invocations from the response (by scanning for the sentinel pattern described
   in FR-002)
3. Execute each invocation as a subprocess (`std::process::Command`)
4. Feed tool output back to the LLM as the next user turn
5. Repeat until the LLM emits the completion sentinel or a limit is hit

Loop limits (all configurable via CLI flags with these defaults):

- `--max-iterations N` (default: 10) — max turns per task before recording `Fail`
- `--max-task-tokens N` (default: 50000) — stop if accumulated tokens exceed this
- `--task-timeout-s N` (existing flag, default: 30) — wall-clock timeout per task

### FR-004 — Token tracking

Each LLM API call in the agentic loop captures input and output token counts from the
API's usage field. These accumulate to per-task totals in `TaskResult` and aggregate
totals in `BenchmarkResult`. When the API does not return token counts (e.g. mock model
in tests), the fields are `null`. MCP mode populates these fields as `null` unless a
future spec adds token tracking there too — the schema is forward-compatible.

### FR-005 — Outcome determination

After the agentic loop terminates, the harness:

1. Extracts the corrected class source from the agent's final message using the same
   `extract_fixed_classes` logic from `benchmark/llm.rs`
2. Compiles and tests it against IRIS using the same `container::write_and_compile` and
   `container::run_class_tests` calls as the existing MCP path

Outcome is `Pass`, `Fail`, or `Error` with the same semantics as FR-012 in spec 059.
Specifically: loop exhaustion without a valid class block → `Fail`; a tool-level failure
before the fix was exercised → `Error`.

### FR-006 — Comparison output

`--compare <prior-result.json>` loads a prior result (MCP or CLI dispatch) and appends a
`comparison` object to the output:

```json
{
  "comparison": {
    "other_mode": "mcp",
    "pass_rate_delta": 0.05,
    "tokens_total_delta": 1234,
    "elapsed_s_delta": 12.3
  }
}
```

`tokens_total_delta` is `null` when either result lacks token data.

### FR-007 — Mode field in output

`BenchmarkResult` gains a `mode` field: `"mcp"` or `"cli_dispatch"`. Results written
before this field existed deserialize with `mode: null` (backward compatible via `#[serde(default)]`).

### FR-008 — Lock compatibility

The run lock from spec 059 (FR-013) applies to CLI dispatch runs using the same global
lock key. Only one benchmark run of either mode may be active per IRIS container at a time.

### FR-009 — Skill content injection

Skill content (the `--skill` file) is injected into the CLI dispatch system prompt as a
"Skill guidance" section, identical to the MCP mode injection. An empty skill (`/dev/null`
or `""`) runs as the baseline pass.

### FR-010 — Binary self-reference

Before starting a CLI dispatch run, the harness resolves `std::env::current_exe()` and
uses that path for all subprocess calls. If resolution fails, it errors immediately with
`CLI_DISPATCH_BINARY_NOT_FOUND` before any LLM calls are made.

---

## Acceptance Scenarios

Each scenario maps to one independent test.

### S-001 — CLI dispatch run produces valid result JSON (P1)

Given a live IRIS container and a valid LLM API key,
when `iris-agentic-dev benchmark --mode cli-dispatch --skill /dev/null` runs,
then the output is valid JSON with `mode: "cli_dispatch"`, `pass_rate` in [0.0, 1.0],
`tasks_total >= 1`, `tasks_errored >= 0`, and each `task_results[*].outcome` in
`["pass", "fail", "error"]`.

### S-002 — Token counts appear in CLI dispatch result (P2)

Given a live run against a model that returns usage data (Anthropic or OpenAI),
when the run completes,
then `tokens_input > 0`, `tokens_output > 0`, and `task_results[0].tokens_total > 0`.

### S-003 — --compare produces delta section (P3)

Given a saved MCP result JSON and a new CLI dispatch result,
when `--compare <mcp-result.json>` is passed,
then the output contains `comparison.other_mode: "mcp"`, `comparison.pass_rate_delta`,
`comparison.tokens_total_delta`, and `comparison.elapsed_s_delta`.

### S-004 — Agent that never uses CLI tools still terminates cleanly (edge case)

Given an agent that responds with only prose and no tool invocations,
when the run reaches `--max-iterations` turns without a tool call,
then the task records `outcome: "fail"` (not `"error"`), and the harness proceeds to
the next task without hanging.

### S-005 — Malformed JSON in a tool invocation is fed back to the agent (edge case)

Given an agent response containing
`iris-agentic-dev tool iris_compile --args '{bad json'`,
when the harness attempts that invocation,
then the subprocess error is captured, fed back as the tool result in the next turn, and
the loop continues rather than panicking the harness.

### S-006 — IRIS unreachable mid-benchmark records Error not crash (edge case)

Given a run where IRIS becomes unreachable after two tasks complete,
when subsequent tasks attempt to compile via the CLI tool,
then each affected task records `outcome: "error"` with `reason` containing
`IRIS_UNREACHABLE`, and the run completes all remaining tasks and writes a full result.

### S-007 — --mode mcp explicit is identical to default (regression)

Given the same inputs and a live IRIS container,
when `--mode mcp` is passed explicitly vs omitted,
then pass rate, per-task outcomes, and the JSON schema are identical.

### S-008 — max-iterations limit stops the loop (unit, no live IRIS required)

Given `--max-iterations 2` and a mock agent that always returns a tool invocation,
when the harness runs a single task,
then the LLM is called exactly 2 times, and the task records `outcome: "fail"`.

---

## Success Criteria

1. S-001 through S-008 all pass.
2. Token fields populate for Anthropic and OpenAI models; `null` for mock model only.
3. MCP mode regression: all existing benchmark tests pass unchanged.
4. `skills/BENCHMARKING.md` updated with CLI dispatch usage example, output schema,
   and an interpretation note on what token-cost differences mean in practice.

---

## Out of Scope

- CLI dispatch benchmarking for the `mf` or `sql` suites (not yet ported).
- Automatic model-selection guidance based on benchmark results.
- Parallel task execution in CLI dispatch mode.
- An agent that autonomously writes new skill files from failure patterns.
