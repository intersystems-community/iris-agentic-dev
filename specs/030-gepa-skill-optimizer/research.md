# Research: GEPA-Inspired Skill Optimization Loop

## Reflective-mutation loop instead of an external GEPA dependency

**Decision**: implement candidate generation as a direct `LlmClient::complete(system, user)`
call (the same client `benchmark/llm.rs` already uses for the benchmark harness's
fix-proposal step), with a system prompt instructing the model to rewrite a skill's text
given (a) the current text, (b) the `FailurePattern`s diagnosed from this round's failures,
and (c) an explicit instruction to preserve guidance for tasks that already pass. This is
"GEPA" in the sense the term is used in the wider literature — reflective prompt/text
evolution driven by structured natural-language feedback rather than gradient descent —
implemented natively rather than by calling the `gepa-ai` hosted API or vendoring the
`dspy-agent-skills` reference implementation.

**Alternatives considered**:
- Hosted `optimize_anything` API (`gepa-ai.github.io`) — rejected. Requires a separate API
  key beyond the `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` the benchmark harness already needs,
  adds an external network dependency to a core loop, and its input/output contract is not
  under this project's control. Violates the spirit of Constitution Principle I
  (Zero-Install Binary — the optimizer should run from the same prebuilt binary as
  everything else) for no benefit this project's scale needs.
- Vendoring `dspy-agent-skills` — rejected. Would add a Python (or DSPy-Rust-bridge)
  dependency where none exists today, violating Principle VII (Dependency Minimalism) —
  the same reasoning 059 used to reject adding an LLM SDK crate and instead reuse
  `generate.rs`'s raw `reqwest`-based request builders.
- Full GEPA Pareto-frontier search (many candidates, multi-objective selection) —
  rejected for v1. A bounded, single-objective (`pass_rate` on held-out + locked tasks)
  loop with a small candidate count (default 3) is sufficient to validate the mechanism
  end-to-end; a richer search strategy is a natural v2 extension once the basic
  propose/score/apply/lock loop is proven, not a v1 requirement.

## Diagnosis batching instead of a dedicated RLM library

**Decision**: `optimizer::analyze::diagnose_failures` pages through non-passing
`TaskResult`s in fixed-size batches (default 5, configurable), and for each batch makes
one `LlmClient::complete` call whose prompt includes: each task's `description`/`goal`/
`expected_behavior`, its `TaskResult.reason`, and the matching tool-call sequence for that
task's session pulled from `telemetry::read_durable` + `filter_records` (059). The model's
response is parsed into one `FailurePattern` per task in the batch. No context-flooding
risk exists because batch size is fixed regardless of total failure count (spec FR-002,
SC-003).

**Alternatives considered**: a true recursive/hierarchical RLM implementation (per the
Lespérance/HALO references in the original sketch) — rejected for v1. The "avoid flooding
context" goal those techniques solve is achieved here by the much simpler mechanism of a
fixed batch size, since this project's failure counts are bounded by a 22-task suite, not
the much larger trace volumes those papers address. Revisiting a true RLM implementation
is reasonable if/when the task suite grows by an order of magnitude (Assumption 4-adjacent
scaling concern), not before.

## Held-out scoring reuses `benchmark::run_suite` directly

**Decision**: no new scoring code. `optimizer::mod::score_candidate` calls the existing
`benchmark::run_suite(iris, client, namespace, tasks, skill_content, iris_version)` with
`tasks` set to the held-out subset (this round's failing tasks' complement within the
22-task suite) unioned with the skill's persisted `RegressionLockSet` tasks, and
`skill_content` set to the candidate text. The returned `BenchmarkResult.pass_rate` is the
candidate's score; `tasks_total`/`tasks_passed` on the locked-task subset specifically
(computed by filtering `task_results` to locked ids) is used for the lock-check gate
(spec FR-006), kept separate from the held-out-set pass rate reported to the caller
(spec FR-012).

**Alternatives considered**: a cheaper proxy metric (e.g. static text similarity to a
"known good" pattern, or a smaller single-task smoke check) — rejected for v1 per the
Clarifications session; the full `run_suite` cost is accepted and bounded by keeping the
candidate count small (default 3) rather than by cheapening the metric itself. A follow-up
could introduce a fast pre-filter (e.g. compile-only check before running tests) to cut
candidates before the full suite run, but that optimization is deferred.

## Regression-lock storage: JSON sidecar next to the skill file, not an IRIS global

**Decision**: `<skill_dir>/.optimizer-lock.json` — a small JSON object
`{"locked_task_ids": ["jira-005", ...], "history": [{...}]}` — colocated with each skill's
`SKILL.md`. Read/written via plain `std::fs`, no IRIS interaction.

**Alternatives considered**: reusing telemetry's `^IRISDEV("telemetry", ...)` IRIS-global
sink — rejected. The regression-lock set is small (bounded by a 22-task suite, Assumption
4), skill-local rather than session-scoped, and needs to persist independent of which IRIS
instance a given optimization round happened to run against (a skill is shared across
projects/instances; its lock history should not be tied to one IRIS container's global
namespace). A local file matches the precedent `.iris-agentic-dev.toml` and 059's
local-file telemetry sink already establish for exactly this kind of "small, durable,
doesn't need a database" state.

## Propose/apply gate mirrors the existing `confirm`/`confirmed` pattern

**Decision**: `skill_optimize`'s params gain an `apply: bool` field (default `false`,
matching the existing codebase convention of defaulting destructive-write gates to off —
see `iris_query mode="write"`'s `confirm` field and `iris_doc`'s `confirmed` field for
precedent). When `apply` is omitted or `false`, the response is propose-only: candidate
diff, scores, held-out set size, no file write. When `apply: true`, the call must
reference the winning candidate from a prior propose-only call for the same skill name
(tracked via a short-lived, process-local `last_proposal: HashMap<String, OptimizationRound>`
on `IrisTools`, mirroring the in-memory `history` ring buffer's existing per-process
lifetime scope — not persisted beyond the current MCP server process, so an `apply: true`
call after a process restart with no fresh propose-only call in the new process correctly
returns the FR-005 error).

**Alternatives considered**: a single-call propose-and-apply-if-better mode — rejected;
this is precisely the "nothing changes without explicit approval" property the spec
requires (Clarifications), matching the same reasoning Opik's Ollie layer uses ("propose a
diff; nothing changes without your explicit approval").

## Concurrent-run protection reuses 059's existing lock, not a new one

**Decision**: `optimizer::mod::run_optimize` calls the existing
`benchmark::acquire_lock`/`release_lock`/`decide_lock` around every `run_suite` invocation
it makes (one per candidate, plus the initial diagnostic run), exactly as the `benchmark`
CLI command already does. No new lock key, no new lock file format.

**Alternatives considered**: a dedicated `skill_optimize`-specific lock — rejected; the
underlying resource being protected (one IRIS container, not corrupted by two concurrent
benchmark-shaped runs) is identical to what 059's lock already protects, and multiple
optimizer-issued `run_suite` calls within the same round are naturally serialized by
reusing that same lock, with no additional mechanism required.
