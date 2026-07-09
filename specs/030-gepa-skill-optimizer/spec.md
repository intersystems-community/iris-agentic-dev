# Feature Specification: GEPA-Inspired Skill Optimization Loop

**Feature Branch**: `030-gepa-skill-optimizer`
**Created**: 2026-05-02
**Revised**: 2026-07-09
**Status**: Draft
**Depends on**: 059-tool-telemetry-benchmark (shipped — durable telemetry and the native
benchmark harness are the two foundational pieces this feature builds on)

---

## Overview

`skill_optimize` (`tools/mod.rs`) is a stub: it accepts a skill name and returns
`NOT_IMPLEMENTED — DSPy optimization not yet implemented`. `skill_propose` (pattern
mining from recorded tool calls) is likewise unimplemented. Both have been stubs since
before 059-tool-telemetry-benchmark shipped a durable, queryable record of every tool
call (`telemetry_query`, `telemetry_export_trace`) and a native benchmark harness
(`iris-agentic-dev benchmark`) that scores a skill's effect on `pass_rate`/`lift` against
the ported `jira_bugs` task suite. This feature closes the gap: it makes `skill_optimize`
real by wiring it to the trace and benchmark infrastructure that already exists.

The shape of this loop — durable trace → diagnose failures → propose a fix → verify
against a test suite → lock the fix in so it can't regress — is not novel to this
project; it is the same structure public agent-observability tooling (e.g. Opik's
trace/diagnose/fix/regression-test stack) has converged on as the necessary complement to
tracing alone. iris-agentic-dev already has the trace layer and the test-suite layer
(both from 059); this feature adds the middle two — diagnose and propose — reusing
059's infrastructure rather than adopting an external product or a second, separate
optimization mechanism.

**Scope boundary — skill text only, not tool descriptions**: the original sketch for this
feature described evolving "skill text + tool descriptions" together. This spec narrows
v1 to skill text only. Skill text (`light-skills/skills/*/SKILL.md`) is loaded at runtime
by the existing binary — a candidate can be benchmarked immediately with no rebuild. Tool
descriptions are Rust string literals in `#[tool(description = "...")]` attributes;
changing one requires editing source and recompiling, which breaks Constitution
Principle I (Zero-Install Binary) for what would otherwise be a live optimization loop.
Tool-description optimization is a legitimate follow-on (it would propose a source diff
plus a `cargo build` step instead of a runtime file write) but is a materially different
mechanism and is explicitly deferred to a future spec, not folded into this one.

**Primary drivers:**

- **`skill_optimize` is a published, discoverable stub**: it appears in `check_config`'s
  tool list and the README's Tools table implies a working optimization path exists.
  Today it does nothing.
- **The two hard prerequisites are no longer missing**: 059 shipped exactly the durable
  trace record and the native, dependency-free benchmark harness this feature's original
  sketch listed as open questions ("which GEPA endpoint," "what's the metric function").
  Both questions now have a concrete, in-repo answer.
- **A skill that regresses silently is worse than no skill**: without a held-out
  validation step and a regression-lock mechanism, a "successful" candidate could win on
  the tasks it was tuned against while quietly breaking others. This feature's
  regression-lock mechanism (User Story 3) exists specifically to prevent that.

---

## Clarifications

### Session 2026-07-09

- Q: The original sketch's Open Question #1 asked which GEPA endpoint to call — the
  hosted `optimize_anything` API at `gepa-ai.github.io` (requires an API key) or the
  `dspy-agent-skills` open-source reference implementation. Which does this feature use?
  → A: Neither. Implement a native, GEPA-inspired reflective-mutation loop in Rust,
  reusing the same `generate.rs::LlmClient` the benchmark harness's `benchmark/llm.rs`
  already uses (`LlmClient::from_env()` / `.complete(system, user)`). No new HTTP/LLM SDK
  dependency, no external optimization service, no API key beyond the
  `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` the benchmark harness already requires. This is
  consistent with Constitution Principle VII (Dependency Minimalism) — 059 already
  rejected adding an LLM SDK crate for the exact same reason — and Principle I
  (Zero-Install Binary): the optimizer must run from the same prebuilt binary as
  everything else.
- Q: Open Question #2 asked what metric function to use, noting the benchmark score is
  expensive (a full run per candidate). What does this feature use? → A: The existing
  `benchmark::run_suite`/`BenchmarkResult.pass_rate` directly — no second, cheaper proxy
  metric. Cost is bounded by keeping the candidate count small (default 3 candidates per
  round, configurable) and scoring each candidate against a held-out subset of the task
  suite (tasks not used to derive that round's failure patterns) rather than the full 22.
  A cheaper proxy metric remains a documented possible future optimization, not a v1
  requirement.
- Q: Open Question #3 asked how many skills to optimize per run. → A: Exactly one —
  `skill_optimize`'s existing signature (`SkillNameParams { name: String }`) already takes
  a single skill name; this feature does not change that signature's shape. Optimizing
  multiple skills means calling the tool multiple times, sequentially, from the caller
  side — no batch mode in v1.
- Q: Open Question #4 asked whether this belongs in `objectscript-coder` (Python, near
  the original benchmark) or `iris-dev` (Rust, near the skill registry) — written before
  059 ported the benchmark harness itself into this repository. → A: Rust, inside
  `iris-agentic-dev-core`, as a new `optimizer/` module beside the existing `telemetry/`
  and `benchmark/` modules. 059 already settled the "which repo" half of this question by
  proving the benchmark harness belongs natively here; this feature is a direct consumer
  of that same harness plus the telemetry module, both already Rust-native in this crate.
- Q: Open Question #5 asked how "the RLM" (a recursive/reasoning-oriented trace analyzer
  that avoids flooding context) fits in. → A: As a plain Rust function
  (`optimizer::analyze::diagnose_failures`) that pages through failed `TaskResult`s in
  bounded batches (default batch size 5), and for each batch asks `LlmClient` to summarize
  a structured `FailurePattern` — no separate RLM library or service. This mirrors 059's
  own precedent of implementing a "sounds exotic" capability as a plain function reusing
  existing infrastructure rather than reaching for a new dependency.
- Q: How does a human stay in the loop before a candidate skill text overwrites the real
  file — mirroring the "nothing changes without approval" principle this kind of loop
  needs to be trustworthy? → A: Two-step propose/apply, following the existing
  `confirm`/`confirmed` gate pattern already used by `iris_query mode="write"` and other
  destructive actions in this codebase. `skill_optimize` with `apply: false` (default)
  returns the best candidate's diff and validation scores without writing anything;
  calling it again with `apply: true` and the same `name` writes the winning candidate to
  the skill file. No auto-apply path exists in v1.
- Q: What prevents a "winning" candidate from quietly regressing a task it wasn't tested
  against? → A: A regression-lock set — see User Story 3. Every task any prior accepted
  optimization run for a given skill has ever passed is added to a per-skill "must not
  regress" set, stored alongside the skill file. A new candidate's held-out score must
  clear the acceptance threshold AND the candidate must still pass every locked task,
  re-run as part of scoring — not just the held-out subset.

---

## User Scenarios & Testing

### User Story 1 — Diagnose Failures Into Structured Patterns (Priority: P1)

A skill author (or the optimizer itself, as the first stage of User Story 2) wants to
know *why* a skill's benchmark run failed the tasks it failed — not just the pass/fail
verdict already in `BenchmarkResult`, but a synthesized explanation connecting the
failure to what the skill's text did or didn't say.

**Why this priority**: Every later stage (candidate generation, scoring, locking) depends
on having a structured failure pattern to react to. Without this, "optimize this skill"
has nothing concrete to optimize against beyond a bare pass rate.

**Independent Test**: Run the benchmark harness for a skill with at least one failing
task, then call the diagnosis function directly on that run's `task_results` plus the
matching session's telemetry record. Verify a `FailurePattern` is produced for each
non-passing task, referencing that task's id and a non-empty summary.

**Acceptance Scenarios**:

1. **Given** a `BenchmarkResult` with at least one `Fail` or `Error` outcome, **When**
   diagnosis runs, **Then** exactly one `FailurePattern` is produced per non-passing task,
   each carrying the task id, the observed outcome, and a natural-language summary of the
   likely cause.
2. **Given** more failing tasks than the batch size, **When** diagnosis runs, **Then**
   tasks are processed in bounded batches (not all in a single LLM call) — no single call
   receives more than the configured batch size's worth of task detail.
3. **Given** a `BenchmarkResult` with zero non-passing tasks, **When** diagnosis runs,
   **Then** it returns an empty pattern list without making any LLM call.
4. **Given** the durable telemetry record for the benchmark run's session is available,
   **When** diagnosis runs, **Then** the tool-call sequence for a failing task (from
   `telemetry_query`, filtered to that session) is included as context alongside the
   task's own pass/fail detail.

---

### User Story 2 — Propose and Validate a Skill Candidate (Priority: P1) 🎯 MVP

A skill author calls `skill_optimize` on an underperforming skill and gets back a
concrete, validated candidate replacement for that skill's text — not a black-box
"optimized" file, but a diff plus the scores that justify it — without anything being
written until they explicitly approve it.

**Why this priority**: This is the feature. Everything else (diagnosis, locking) exists
to make this step trustworthy rather than a plausible-sounding guess.

**Independent Test**: Call `skill_optimize` with `apply: false` against a skill known to
underperform on at least one benchmark task. Verify the response contains the current
skill text, a proposed candidate, the held-out `pass_rate` for both, and does not modify
the skill file on disk.

**Acceptance Scenarios**:

1. **Given** a skill with at least one `FailurePattern` from User Story 1, **When**
   `skill_optimize` runs with `apply: false`, **Then** it generates a bounded number of
   candidate skill-text rewrites (default 3), each addressing the failure patterns while
   the mutation prompt explicitly instructs the model not to remove guidance for tasks
   that already pass.
2. **Given** the generated candidates, **When** each is scored, **Then** scoring runs the
   existing `benchmark::run_suite` against a held-out subset of tasks (tasks not used to
   derive this round's failure patterns) and reports each candidate's `pass_rate`.
3. **Given** the scored candidates, **When** the best one is selected, **Then** it is only
   reported as a "win" if its held-out `pass_rate` exceeds the current skill's held-out
   `pass_rate` by at least a configurable threshold (default: any positive improvement,
   `> 0.0`); otherwise the response reports no winning candidate and the current skill
   text is left untouched.
4. **Given** `apply: false` (the default), **When** `skill_optimize` returns a winning
   candidate, **Then** the skill file on disk is unchanged — the response contains the
   diff and scores only.
5. **Given** `apply: true` and a `name` matching a prior `apply: false` call's winning
   candidate, **When** `skill_optimize` runs, **Then** the skill file is overwritten with
   that candidate's text and the response confirms the write.
6. **Given** `apply: true` with no prior winning candidate available for that skill,
   **When** `skill_optimize` runs, **Then** it returns an error rather than silently
   running a fresh (unreviewed) optimization pass and applying it in the same call.

---

### User Story 3 — Lock a Fix So It Can't Regress (Priority: P2)

Once a candidate skill text is applied, the specific tasks that candidate fixed must keep
passing in every future optimization round for that skill — mirroring the "growing
regression suite" half of the trace/diagnose/fix/lock loop, so each accepted round makes
the skill strictly harder to regress, not just momentarily better.

**Why this priority**: Without this, User Story 2 alone can thrash — an optimization
round could "fix" one task while silently breaking a previously-fixed one, and nothing
would catch it. This is P2 because User Story 2 delivers standalone value (a
human-reviewed candidate) even before locking exists; locking is what makes repeated
rounds safe to run unattended.

**Independent Test**: Apply a winning candidate for a skill (User Story 2), then run
`skill_optimize` again for the same skill with a deliberately worse candidate forced into
the pool. Verify the regression-locked task from the first round is checked, and a
candidate that fails any locked task is excluded from being reported as a winner
regardless of its held-out score.

**Acceptance Scenarios**:

1. **Given** an applied candidate that fixed task `jira-005`, **When** the optimization
   round completes, **Then** `jira-005` is added to that skill's regression-lock set,
   persisted alongside the skill file.
2. **Given** a skill with a non-empty regression-lock set, **When** a later optimization
   round scores new candidates, **Then** every candidate is also run against every locked
   task, and any candidate that fails a locked task is excluded from winner selection even
   if its held-out `pass_rate` would otherwise be the best.
3. **Given** all candidates in a round fail at least one locked task, **When** the round
   completes, **Then** the response reports no winning candidate, distinct from "no
   candidate improved on the baseline" (Acceptance Scenario 3 of User Story 2) — the
   caller can tell "nothing beat the current skill" apart from "something beat it but
   broke a locked task."

---

### Edge Cases

- **No IRIS connection**: diagnosis (User Story 1) needs telemetry context and the
  benchmark harness needs a live IRIS connection for both diagnosis and scoring;
  `skill_optimize` MUST fail fast with `IRIS_UNREACHABLE` (the existing standard code)
  rather than attempting a partial run.
- **No LLM configured**: candidate generation (User Story 2) and diagnosis (User Story 1)
  both call `LlmClient`; when `LlmClient::from_env()` returns `None` (no API key), the
  tool MUST return a structured error rather than silently skipping generation and
  reporting an empty candidate list as if none existed.
- **Held-out set too small**: v1's task suite has 22 tasks; if the failure-pattern batch
  consumes most of them, the held-out set could shrink to a handful of tasks, making
  `pass_rate` noisy. The system MUST report the held-out set's size alongside the score so
  a caller can judge confidence — it MUST NOT silently proceed with, e.g., a 1-task
  held-out set without surfacing that size.
- **Concurrent optimization runs for the same skill**: two simultaneous `skill_optimize`
  calls for the same skill against the same IRIS container must not corrupt each other's
  benchmark runs. This reuses 059's existing `acquire_lock`/`release_lock`/`decide_lock`
  concurrent-run guard (already keyed by container name) rather than inventing a second
  lock mechanism.
- **A skill with zero failing tasks**: `skill_optimize` on an already-perfect skill MUST
  report "no failure patterns, nothing to optimize" rather than fabricating a candidate
  or erroring.
- **Regression-lock set grows unbounded across many rounds**: out of scope for v1 —
  22 tasks total bounds the lock set's maximum size to the suite size; pruning is
  unnecessary until the task suite itself grows substantially (Assumption 4).

---

## Requirements

### Functional Requirements

- **FR-001**: The system MUST provide a diagnosis function that, given a `BenchmarkResult`
  and its session's durable telemetry record, produces one `FailurePattern` per
  non-passing (`Fail`/`Error`) task, each containing the task id, observed outcome, and an
  LLM-synthesized natural-language summary of the likely cause.
- **FR-002**: Diagnosis MUST process failing tasks in bounded batches (configurable, default
  5) rather than in a single unbounded LLM call, so context size does not grow linearly
  with the number of failures.
- **FR-003**: The system MUST implement `skill_optimize` (superseding its current
  `NOT_IMPLEMENTED` stub) to: (a) run the benchmark suite for the named skill, (b)
  diagnose any failures via FR-001, (c) generate a bounded number of candidate skill-text
  rewrites (default 3, configurable) via an LLM, explicitly instructed to preserve
  guidance for currently-passing tasks, (d) score each candidate's `pass_rate` against a
  held-out task subset via the existing benchmark harness, and (e) report the best
  candidate if it exceeds the current skill's held-out `pass_rate` by a configurable
  threshold (default: any positive improvement).
- **FR-004**: `skill_optimize` MUST default to a propose-only mode (no write to the skill
  file) and MUST require an explicit `apply: true` parameter, referencing a specific
  prior winning candidate, before overwriting the skill file.
- **FR-005**: `skill_optimize` called with `apply: true` when no prior winning candidate
  exists for that skill MUST return an error rather than silently running and applying a
  fresh optimization pass in the same call.
- **FR-006**: The system MUST maintain a per-skill regression-lock set of task ids that any
  previously-accepted candidate for that skill has passed. Every optimization round MUST
  score each candidate against the full locked set in addition to the held-out subset, and
  MUST exclude any candidate that fails a locked task from winner selection regardless of
  its held-out score.
- **FR-007**: When a candidate is applied (FR-004), the tasks it newly fixed (tasks the
  prior skill text failed that the applied candidate passes) MUST be added to that skill's
  regression-lock set.
- **FR-008**: Candidate generation and diagnosis MUST reuse the existing
  `generate.rs::LlmClient` (`from_env()`/`complete()`) — no new HTTP client, LLM SDK, or
  external optimization service dependency is introduced.
- **FR-009**: Held-out scoring MUST reuse the existing `benchmark::run_suite` function
  in-process — no second scoring/evaluation mechanism.
- **FR-010**: Concurrent `skill_optimize` runs against the same IRIS container MUST be
  detected and rejected using the existing benchmark-run lock
  (`acquire_lock`/`release_lock`/`decide_lock`), not a new lock mechanism.
- **FR-011**: A `skill_optimize` call on a skill with zero non-passing tasks MUST report
  that no optimization is needed rather than fabricating a candidate.
- **FR-012**: The response MUST report the held-out task set's size alongside every
  reported `pass_rate`, so a caller can judge score confidence.
- **FR-013**: `skill_optimize` MUST fail with `IRIS_UNREACHABLE` when no IRIS connection is
  discoverable, and with a distinct structured error when no LLM is configured
  (`LlmClient::from_env()` returns `None`) — neither condition silently degrades to a
  partial or fabricated result.

### Key Entities

- **FailurePattern**: One diagnosed non-passing task — task id, observed outcome
  (`Fail`/`Error`), a natural-language cause summary, and the tool-call sequence (from
  telemetry) that produced it.
- **SkillCandidate**: One proposed replacement skill text — the full candidate text, the
  round it was generated in, and (once scored) its held-out `pass_rate` and locked-task
  pass/fail detail.
- **OptimizationRound**: One `skill_optimize` invocation's result — the skill name, the
  `FailurePattern`s diagnosed, the `SkillCandidate`s generated and scored, the winner (if
  any), and whether `apply` was requested.
- **RegressionLockSet**: Per-skill, persisted set of task ids that must keep passing in
  every future round for that skill.

---

## Success Criteria

### Measurable Outcomes

- **SC-001**: `skill_optimize` on a skill with at least one benchmark failure returns a
  candidate with a held-out `pass_rate` at least as good as the current skill's, or
  explicitly reports no winning candidate — never a candidate that silently regresses a
  previously-locked task.
- **SC-002**: No skill file is modified by any `skill_optimize` call made with `apply`
  omitted or `false`.
- **SC-003**: Diagnosis (FR-001) never issues more than one LLM call per configured batch
  of failing tasks, regardless of total failure count.
- **SC-004**: A candidate accepted and applied in round N does not, in round N+1 or later,
  regress the specific task(s) it fixed in round N — verified by the regression-lock check
  before any later round's winner is selected.
- **SC-005**: `skill_optimize`'s response always states the held-out set size used for its
  reported `pass_rate`.

---

## Assumptions

1. This feature reuses 059-tool-telemetry-benchmark's `benchmark::run_suite`,
   `benchmark::load_embedded_tasks`, `acquire_lock`/`release_lock`/`decide_lock`, and
   `telemetry::read_durable`/`filter_records` as-is; it does not modify their behavior or
   signatures.
2. "GEPA" in this feature's name refers to the reflective-mutation *technique* (propose a
   textual candidate informed by structured failure feedback, score it, keep it if it
   wins) — not the external `gepa-ai` library or hosted API, which this feature does not
   call or depend on.
3. Tool-description optimization (the other half of the original sketch's scope) is
   explicitly out of scope for this spec and deferred to a future feature, per the Scope
   Boundary note in Overview.
4. The regression-lock set's maximum size is bounded by the task suite's size (22 tasks in
   v1); no pruning mechanism is needed until the suite itself grows substantially.
5. The Lespérance "RLM ⊕ GEPA" recursive pattern (using this optimizer to improve its own
   diagnosis/mutation prompts) referenced in the original sketch's Key References is
   explicitly out of scope for v1 — this feature optimizes skill text only, not its own
   prompts.
6. One skill is optimized per `skill_optimize` call; no batch-all-skills mode exists in
   v1 (Clarifications).
