# Contract: `optimizer` module internal API (no CLI surface — MCP tool only)

Unlike 059's benchmark harness, this feature does not add a new CLI subcommand — it is
reachable only via the `skill_optimize` MCP tool (contracts/skill-optimize-tool.md).
This contract documents the internal function boundaries between the new `optimizer/`
module and the existing `benchmark`/`telemetry` modules it consumes, so implementation
tasks (tasks.md) have a stable signature to write tests against.

## `optimizer::analyze::diagnose_failures`

```rust
pub async fn diagnose_failures(
    llm: &LlmClient,
    failing: &[TaskResult],
    telemetry: &[ToolCallRecord],   // pre-filtered to the triggering session, by caller
    batch_size: usize,
) -> anyhow::Result<Vec<FailurePattern>>
```

Pages `failing` into chunks of `batch_size`; one `llm.complete(...)` call per chunk.
Returns one `FailurePattern` per input `TaskResult`, in the same order. Empty `failing`
slice returns `Ok(vec![])` with zero LLM calls (spec Acceptance Scenario 3, User Story 1).

## `optimizer::mutate::generate_candidates`

```rust
pub async fn generate_candidates(
    llm: &LlmClient,
    current_skill_text: &str,
    patterns: &[FailurePattern],
    candidate_count: usize,
) -> anyhow::Result<Vec<String>>   // raw candidate texts, not yet scored
```

One `llm.complete(...)` call per candidate (not batched — each candidate is an
independent proposal, sampled separately so candidates differ). System prompt fixed
constant (mirrors `benchmark::llm::SYSTEM_PROMPT`'s pattern) instructing the model to
preserve guidance for currently-passing tasks.

## `optimizer::lock`

```rust
pub struct RegressionLockSet { pub locked_task_ids: Vec<String>, pub history: Vec<LockHistoryEntry> }

pub fn load(skill_dir: &Path) -> anyhow::Result<RegressionLockSet>   // missing file => empty set, not an error
pub fn save(skill_dir: &Path, set: &RegressionLockSet) -> anyhow::Result<()>
pub fn record_applied_round(set: &mut RegressionLockSet, newly_fixed_task_ids: Vec<String>, held_out_pass_rate: f64, applied_at: String)
```

## `optimizer::mod::score_candidate`

```rust
pub async fn score_candidate(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    candidate_text: &str,
    held_out_tasks: &[BenchmarkTask],
    locked_tasks: &[BenchmarkTask],
    iris_version: &str,
) -> ScoredCandidate
```

Internally calls `benchmark::run_suite` twice — once with `held_out_tasks`, once with
`locked_tasks` (kept separate per data-model.md's note that held-out pass rate and
locked-task pass/fail are reported distinctly, spec FR-012 vs. FR-006). Does not call
`run_suite` with a combined task list, to keep the two reported numbers unambiguous.

## `optimizer::mod::run_optimize`

```rust
pub async fn run_optimize(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    llm: &LlmClient,
    skill_dir: &Path,
    all_tasks: &[BenchmarkTask],
    telemetry_records: &[ToolCallRecord],
    opts: OptimizeOptions,   // candidate_count, batch_size, improvement_threshold
) -> anyhow::Result<OptimizationRound>
```

Orchestrates: baseline `run_suite` → split failing/held-out → `diagnose_failures` →
`generate_candidates` → `score_candidate` per candidate (wrapped in
`acquire_lock`/`release_lock` per research.md) → winner selection (best
`held_out_pass_rate` among candidates with `passes_all_locked == true` that beats
`baseline_held_out_pass_rate` by `opts.improvement_threshold`). Does not write the skill
file or update the lock set itself — that is the caller's (`tools/mod.rs`'s
`skill_optimize` handler's) job when `apply: true`, keeping `run_optimize` a pure
propose-and-score function reusable by both the propose-only and apply paths.
