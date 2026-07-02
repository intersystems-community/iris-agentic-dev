# Contract: `iris-agentic-dev benchmark` CLI subcommand

New `Commands::Benchmark` variant in `crates/iris-agentic-dev-bin/src/main.rs`, backed by
`crates/iris-agentic-dev-bin/src/cmd/benchmark.rs` (new file, following the exact
`CompileCommand` pattern in `cmd/compile.rs`: `#[derive(Args)]` struct with
`#[arg(long, env = "...")]` fields, `IrisConnection` discovery via
`discover_iris(apply_workspace_config(explicit, ...))`).

## Invocation

```bash
iris-agentic-dev benchmark \
  --skill light-skills/skills/objectscript-review/SKILL.md \
  --baseline \
  --output results.json
```

## Arguments

| Flag | Env | Default | Notes |
|---|---|---|---|
| `--skill <path>` | — | required | Path to a `SKILL.md`. |
| `--baseline` | — | `false` | Run a second pass with no skill content, compute `lift`. |
| `--suite <name>` | — | `jira` | v1 only supports `jira` (the ported primary suite) — any other value errors with a message pointing at the deferred-suite Assumption. |
| `--output <path>` | — | stdout | Where to write the JSON `BenchmarkResult`. |
| `--model <name>` | `IRIS_GENERATE_CLASS_MODEL` (reused, not a new env var) | none — required via flag or env | Passed through to `generate.rs`'s existing `LlmClient::from_env`-equivalent construction. |
| `--task-timeout-s <n>` | — | `30` | Per-task timeout. |
| `--max-time-s <n>` | — | `600` | Overall run timeout. |
| `--host`/`--web-port`/`--namespace`/`--username`/`--password` | `IRIS_HOST`/etc. (existing) | same discovery chain as `compile`/other subcommands | Reused verbatim from `CompileCommand`'s connection-discovery block — no new discovery mechanism. |

**No Docker requirement**: the benchmark harness is fully HTTP — compiling task/fix
source and running each task's test class (via a direct `$ClassMethod`/`$Method`
introspection mechanism mirroring objectscript-coder's own `IrisLight.BenchRunner`, since
the ported task suite's test classes extend `%RegisteredObject`, not
`%UnitTest.TestCase`) both go through the plain Atelier-REST/`execute_via_generator`
path already used by every other tool. See research.md for the live verification.

## Output (JSON, written to `--output` or stdout)

```json
{
  "pass_rate": 0.91,
  "baseline_pass_rate": 0.64,
  "lift": 0.27,
  "tasks_passed": 20,
  "tasks_total": 22,
  "tasks_errored": 0,
  "iris_version": "2026.1",
  "elapsed_s": 187.4,
  "task_results": [
    {"task_id": "jira-001", "outcome": "pass", "iterations": 1, "elapsed_s": 8.2, "reason": ""},
    {"task_id": "jira-020", "outcome": "fail", "iterations": 3, "elapsed_s": 30.0, "reason": "tests_pass=false"}
  ]
}
```

`outcome` is one of `"pass"` / `"fail"` / `"error"` — matches the `TaskResult.outcome`
enum in data-model.md. This exact shape (field names, nesting) matches
`BENCHMARKING.md`'s existing "Understanding Results" example and `objectscript_mcp`'s
Python `BenchmarkResult` dataclass field names, satisfying Constitution Principle V
(Output Shape Parity) against the pre-existing documented/implemented shape rather than
inventing a new one.

## Error contract

| Condition | Error Code | Behavior |
|---|---|---|
| No IRIS connection discoverable | `IRIS_UNREACHABLE` | Exit non-zero, structured message matching `BENCHMARKING.md`'s Troubleshooting "IRIS container not found" guidance — same message shape `compile`/other subcommands already use for `IrisDiscovery::NotFound`. |
| `--suite` value other than `jira` | `SUITE_NOT_AVAILABLE` | Exit non-zero before any container interaction, message naming the deferred suites (`mf`, `sql`). |
| Concurrent run detected against the same container (FR-013) | `BENCHMARK_RUN_IN_PROGRESS` | Exit non-zero with a distinct message (not the same as "container not found") — detection mechanism: a lock record written to the durable telemetry sink at run start (`^IRISDEV("telemetry","benchmark_lock",container_name)`), checked and cleared at run end; a stale lock older than `--max-time-s` is treated as abandoned and overridden with a warning, not a hard block. |
| A task's tool/skill surface no longer matches current tools (FR-012) | — (not an error code; a per-task `TaskResult.outcome`) | That task's `outcome` is `"error"`, not `"fail"`; run continues, overall exit code is still 0 if no other failure occurred (an errored task is not itself a run failure). |

See `data-model.md`'s Error Code Registry section for the canonical definition of each new code.
