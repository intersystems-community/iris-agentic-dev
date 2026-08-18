# Plan 080: CLI Tool Dispatch Benchmark

## Architectural Decision: Extension vs Parallel Implementation

The existing harness in `benchmark/mod.rs` + `benchmark/llm.rs` + `cmd/benchmark.rs` can
be extended. A parallel implementation would duplicate the lock logic, the task loading,
the outcome aggregation, and the container setup — all of which apply unchanged to CLI
dispatch. The correct approach is extension:

- Add a `BenchmarkMode` enum to `benchmark/mod.rs`
- Add a `cli_dispatch` submodule `benchmark/cli_dispatch.rs` that implements the agentic loop
- Extend `BenchmarkResult` and `TaskResult` with new optional fields (token counts, mode)
- Extend `cmd/benchmark.rs` with the `--mode` flag and route to the new loop

The MCP path is not touched except for schema additions that are backward-compatible.

---

## New Type: `BenchmarkMode`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkMode {
    #[default]
    Mcp,
    CliDispatch,
}
```

Added to `benchmark/mod.rs`. Used in `BenchmarkResult` and as a parameter to `run_suite`.

---

## Schema Changes (backward-compatible)

### `TaskResult` — new optional fields

```rust
#[serde(default)]
pub tokens_input: Option<u32>,
#[serde(default)]
pub tokens_output: Option<u32>,
#[serde(default)]
pub tokens_total: Option<u32>,
```

MCP mode leaves these `None`. CLI dispatch populates them from API usage responses.

### `BenchmarkResult` — new optional fields

```rust
#[serde(default)]
pub mode: Option<BenchmarkMode>,   // None means pre-080 result
#[serde(default)]
pub tokens_input: Option<u64>,
#[serde(default)]
pub tokens_output: Option<u64>,
#[serde(default)]
pub tokens_total: Option<u64>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub comparison: Option<BenchmarkComparison>,
```

### New type: `BenchmarkComparison`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkComparison {
    pub other_mode: BenchmarkMode,
    pub pass_rate_delta: f64,
    pub tokens_total_delta: Option<i64>,
    pub elapsed_s_delta: f64,
}
```

---

## New Module: `benchmark/cli_dispatch.rs`

This module owns the agentic loop. It does not duplicate `container.rs` — it imports and
calls the same `write_and_compile` and `run_class_tests` functions.

### Key types

```rust
pub struct CliDispatchConfig {
    pub binary_path: PathBuf,      // resolved via iris_dev_bin() — prefers target/llvm-cov-target/debug, falls back to target/debug
    pub max_iterations: u32,
    pub max_task_tokens: u32,
    pub task_timeout_s: u64,
    pub iris_connection_args: ConnectionEnv,  // env vars for subprocess
}

pub struct TurnResult {
    pub text: String,
    pub tool_invocations: Vec<ToolInvocation>,
    pub tokens_input: Option<u32>,
    pub tokens_output: Option<u32>,
}

pub struct ToolInvocation {
    pub tool_name: String,
    pub args_json: String,
}
```

### Tool invocation format: native Anthropic tool-use API

CLI dispatch uses the Anthropic native tool-use API (not sentinel/regex parsing). The
`complete_with_usage` call passes a `tools` list defining the available iris-agentic-dev
tools as Anthropic tool definitions. The model responds with `tool_use` content blocks;
the loop extracts `(tool_name, input_json)` from each block directly — no regex needed.

This requires `claude-3+` or Sonnet 4.x, which is already the benchmark's default model.

The system prompt does not describe CLI invocation syntax. It describes the task context
and skill guidance only. The tool definitions carry all invocation shape information.

Skill content is prepended as a "# Skill guidance" section, matching MCP mode.

### Tool invocation parsing

Extract `tool_use` content blocks from the Anthropic API response. Each block has
`type: "tool_use"`, `name: <tool_name>`, `input: <json_object>`. Serialize `input` to
a JSON string and pass it to the subprocess runner as `--args '<json>'`.

A block with an input that fails subprocess execution returns the error text to the
agent as a `tool_result` content block — the agent sees the error and can retry.

### Subprocess execution

```rust
fn run_tool_subprocess(
    binary: &Path,
    invocation: &ToolInvocation,
    env_args: &ConnectionEnv,
) -> String  // returns combined stdout+stderr, always succeeds
```

Uses `std::process::Command::new(binary).arg("tool").arg(&invocation.tool_name)...`.
Never panics: all errors become the returned string, which is fed back to the agent.

### Token counting

`LlmClient::complete` today returns only `String`. CLI dispatch needs token counts.
Two options:

1. Add `complete_with_usage(&self, ...) -> Result<(String, Option<TokenUsage>)>` alongside
   the existing `complete()` method (preferred — no breaking change)
2. Change `complete()` return type to a struct (breaking — touches all callers)

Use option 1. `TokenUsage { input: u32, output: u32 }` is added to `generate.rs`.
Anthropic responses already carry `usage.input_tokens`/`usage.output_tokens`; add those
fields to `AnthropicResponse`. OpenAI responses carry `usage.prompt_tokens`/`usage.completion_tokens`;
add those to `OpenAiResponse`.

### Loop implementation

```rust
pub async fn run_cli_dispatch_task(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    task: &BenchmarkTask,
    skill_content: &str,
    config: &CliDispatchConfig,
) -> TaskResult
```

This is the parallel of `run_task` in `mod.rs`. It:

1. Verifies the binary path exists (FR-010)
2. Builds the initial prompt (system + task context + skill)
3. Runs the agentic loop (FR-003)
4. On sentinel detection, extracts class source and calls `container::write_and_compile`
   - `container::run_class_tests`
5. Accumulates token counts into `TaskResult`

### Suite runner

```rust
pub async fn run_cli_dispatch_suite(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    tasks: &[BenchmarkTask],
    skill_content: &str,
    iris_version: &str,
    config: &CliDispatchConfig,
) -> BenchmarkResult
```

Parallel to `run_suite` in `mod.rs`, returns a `BenchmarkResult` with
`mode: Some(BenchmarkMode::CliDispatch)` and aggregated token counts.

---

## Changes to `cmd/benchmark.rs`

```rust
#[arg(long, default_value = "mcp")]
pub mode: String,  // "mcp" | "cli-dispatch"

#[arg(long, default_value = "10")]
pub max_iterations: u32,

#[arg(long, default_value = "50000")]
pub max_task_tokens: u32,

#[arg(long)]
pub compare: Option<String>,  // path to prior result JSON
```

The `run_inner` method splits on `mode`:

- `"mcp"` → existing `run_suite` call (unchanged behavior)
- `"cli-dispatch"` → `run_cli_dispatch_suite` call
- anything else → immediate error with `UNKNOWN_MODE`

The `--compare` logic loads the prior result, computes deltas, and sets
`result.comparison` before serializing.

---

## File Changes

| File                                                         | Change                                                                                  |
| ------------------------------------------------------------ | --------------------------------------------------------------------------------------- |
| `crates/iris-agentic-dev-core/src/benchmark/mod.rs`          | Add `BenchmarkMode`, extend `TaskResult` + `BenchmarkResult`, add `BenchmarkComparison` |
| `crates/iris-agentic-dev-core/src/benchmark/cli_dispatch.rs` | New file — agentic loop, system prompt, subprocess runner, suite runner                 |
| `crates/iris-agentic-dev-core/src/benchmark/llm.rs`          | Add `complete_with_usage`, `TokenUsage`; extend `AnthropicResponse` + `OpenAiResponse`  |
| `crates/iris-agentic-dev-core/src/generate.rs`               | Add `TokenUsage` type (or import from `benchmark/llm.rs`)                               |
| `crates/iris-agentic-dev-bin/src/cmd/benchmark.rs`           | Add `--mode`, `--max-iterations`, `--max-task-tokens`, `--compare` flags                |
| `skills/BENCHMARKING.md`                                     | Document CLI dispatch usage, output schema, comparison flag                             |
| `crates/iris-agentic-dev-core/tests/`                        | New integration test file for CLI dispatch scenarios                                    |

---

## Test Strategy

Tests follow the project's live-IRIS rule: anything touching compilation or tool output
uses a live container. The exception is the loop-limit unit test (S-008), which can use a
mock LLM and a mock subprocess runner injected via a trait.

### Unit tests (no IRIS, no live LLM)

- `decide_lock` — already tested, unchanged
- `extract_fixed_classes` from sentinel — new case alongside existing tests
- `parse_tool_invocations` — tests for well-formed lines, malformed JSON lines, prose-only
  responses, and responses mixing prose with tool calls
- `run_cli_dispatch_task` loop limit — inject a mock LLM that always returns a tool call,
  verify loop stops at `max_iterations` and records `Fail`

### Integration tests (live IRIS, mock LLM)

- S-001, S-004, S-005, S-006, S-007 can use a mock LLM (model = "mock") so they run in
  CI without an API key
- S-002 requires a live LLM API key; mark `#[ignore]` per existing convention
- S-003 requires two result files; can be constructed from mock runs

---

## Resolved Design Decisions

1. **Tool invocation format** — Native Anthropic tool-use API. The loop sends tool
   definitions to the API; responses carry `tool_use` content blocks with structured
   `(name, input)` — no regex or sentinel parsing needed. Requires `claude-3+` (already
   the benchmark default).

2. **Token counting for OpenAI** — Non-streaming requests; `generate.rs` does not set
   `stream: true` today, so `usage` is present on all responses. No change needed to
   request construction.

3. **Binary path for subprocess** — Use the `iris_dev_bin()` pattern from
   `progressive_disclosure_integration.rs`: prefers
   `target/llvm-cov-target/debug/iris-agentic-dev`, falls back to
   `target/debug/iris-agentic-dev`. Do not use `current_exe()`. Integration tests
   require the binary to be pre-built; the subprocess runner is abstracted behind a trait
   so unit tests can inject a fake runner without a built binary.

4. **Baseline comparison across modes** — CLI dispatch baseline = same agentic loop,
   empty skill. MCP single-shot is not used as the reference. This isolates the skill
   variable cleanly within CLI dispatch mode.
