# Feature Specification: Tool Telemetry and Benchmark Harness

**Feature Branch**: `059-tool-telemetry-benchmark`
**Created**: 2026-07-01
**Status**: Draft
**Depends on**: none (foundational — 058-iris-graph's `record_trace` action consumes this
feature's output as one of its trace-record sources)

---

## Overview

iris-agentic-dev already tracks the last 50 tool calls in memory (`agent_history`,
`agent_stats`) — enough for a live debugging session, gone the moment the process
restarts. Two unrelated efforts both need more than that snapshot can give them, and
both are currently blocked or duplicated because no durable, queryable record of "what
tool was called, with what result" exists:

1. **Skill benchmarking**: `light-skills/BENCHMARKING.md` documents a full pass-rate/
   lift/leaderboard workflow — "clone the benchmark repo, run 22 repair tasks with and
   without your skill, see the lift" — but the referenced repo is a private, internally-
   restricted GitLab URL that external contributors cannot reach, and no equivalent
   harness exists inside the public iris-agentic-dev repository itself. A prospective
   contributor who follows the documented Quick Start hits a wall at step 1.
2. **Runtime dispatch tracing**: a separate initiative (feeding the forthcoming
   `iris_graph` knowledge-graph tool) needs to know which tool/method called which other
   tool/method, how often, and when — the same shape of information the benchmark
   harness needs to know "did this tool call succeed," just aggregated differently.

Building two separate recording mechanisms for what is fundamentally the same underlying
event (a tool call happened, with these inputs, this outcome, at this time) would mean
double instrumentation cost and two sources of truth that can drift. This feature
establishes one durable telemetry layer that both consumers read from, and uses that
same layer to make the benchmark harness real, self-contained, and runnable by anyone
who can clone the public repository.

**Primary drivers:**

- **Broken promise to the community**: `BENCHMARKING.md` is a published, discoverable
  document promising a workflow that does not work end-to-end today. This is an active
  drag on external contribution (reported directly by a prospective contributor).
- **Duplicate-instrumentation risk**: without a shared layer, the dispatch-tracing effort
  and the benchmark harness would each invent their own tool-call logging, doubling
  maintenance and creating two records that can disagree about the same event.
- **Foundation for future tool-selection work**: a durable, queryable record of tool
  call outcomes is a prerequisite for any future effort to recommend or rank tools/skills
  by observed effectiveness rather than by description alone.

---

## Clarifications

### Session 2026-07-01

- Q: Should the telemetry layer persist across process restarts, or is an in-session
  durable log (longer than 50 entries, but reset on restart) sufficient for v1? → A:
  Persist across restarts — both consumers (benchmark runs, dispatch-trace export) need
  to reference data from a completed session after the process has exited, not just the
  live session.
- Q: objectscript-coder's Python MCP server already has a mature, working solution to
  almost exactly this problem — `CallRecorder` middleware: an in-memory ring buffer as
  the always-available source of truth for `agent.history`, plus a best-effort
  (never-blocking, failure-tolerant) write to a durable store (IRIS global
  `^TOOLCALLS(session_id, seq_n)`) for anything that needs to survive a restart. Should
  the new Rust telemetry layer mirror this architecture? → A: Yes — mirror the shape
  exactly: in-memory ring buffer remains the authoritative, always-available source for
  live `agent_history`/`agent_stats` (superseding the current 50-entry cap with a larger
  bound), and durable persistence is a best-effort side write that never blocks or fails
  the underlying tool call (FR-014 already requires this; this decision fixes *how*).
  Because iris-agentic-dev — unlike the Python server — routinely runs with no live IRIS
  connection at all (local-file-only mode, spec 025), the durable store MUST NOT be an
  IRIS global exclusively: when no IRIS connection is active, the best-effort write
  target is a local append-only file under the workspace/config data directory instead.
  Both targets use the same record shape; only the sink differs based on connection state.
- Q: Does the benchmark harness need to reproduce all three of objectscript-coder's task
  suites (jira repair, multi-file repair, SQL quirks), or is a smaller subset acceptable
  for v1? → A: Port only the primary repair suite (the one BENCHMARKING.md's Quick Start
  actually exercises) for v1. Multi-file and SQL-quirks suites are valuable but are
  explicitly deferred — the immediate problem is "the documented Quick Start doesn't
  work," not "every suite is available."
- Q: What triggers a tool call to be recorded — every MCP tool invocation unconditionally,
  or only invocations opted into telemetry? → A: Unconditional recording of tool name,
  success/failure, and duration for every call (matching the existing `record_call`
  behavior, just made durable and unbounded instead of a 50-entry ring buffer). Recording
  of call *parameters* is opt-in per the same `dataPolicy`/PHI-gate model already
  established (spec 051) — parameters are redacted by default unless the active policy
  allows full capture, since parameters may contain PHI-adjacent values (e.g. `iris_query`
  SQL text, `iris_global` subscripts). This deliberately diverges from
  `CallRecorder`'s HMAC-SHA256 irreversible scrubbing — iris-agentic-dev keeps the
  existing spec-051 policy model rather than adopting a second, incompatible redaction
  mechanism.
- Q: Two incompatible "lift" formulas already exist in the ecosystem —
  `objectscript_mcp/benchmark/runner.py` uses absolute difference
  (`pass_rate - baseline_pass_rate`), while `eval/agentic_eval/harness/benchmark_executor.py`
  uses relative percentage (`(rate - baseline) / baseline * 100`, used for a different
  comparison — development paths/harnesses, not skill scoring). Which does this feature's
  benchmark harness standardize on? → A: Absolute difference — matches
  `BENCHMARKING.md`'s own worked example (`+27%` computed as `91% - 64%`) and the
  already-in-production `objectscript_mcp` runner used for skill scoring specifically.
  FR-004 already reflects this; this decision confirms it explicitly rather than leaving
  the conflicting relative-percentage precedent as an implicit alternative.
- Q: FR-002 and FR-013 both assume a "session/run identifier" for scoping telemetry
  queries and detecting concurrent benchmark runs, but no such concept exists in
  iris-agentic-dev's connection state today. What does "session" mean? → A: One MCP
  server process lifetime equals one session — a session identifier is generated once
  when the server process starts and stays fixed until it exits, matching
  `CallRecorder`'s per-process `session_id` model exactly. This is sufficient because a
  benchmark run and a live dispatch-tracing session are each, in practice, a single
  process's lifetime.
- Q: `specs/021-path-aware-benchmark` already designed a fuller benchmark system
  (per-task namespace isolation, results-file layout, LLM-judge rubric) for a different
  comparison — development-path A/B testing across multiple harnesses (Claude Code,
  Copilot, etc.), not skill scoring. Should this feature reuse 021's conventions? → A:
  No — these are separate, independently-scoped mechanisms answering different questions
  ("does this skill help" vs. "does this editing path beat that one across harnesses").
  This feature MUST NOT take a dependency on 021's namespace-isolation, results-file, or
  multi-harness driver machinery; convergence is deferred until a concrete shared need
  emerges.

---

## User Scenarios & Testing

### User Story 1 — Run the Documented Benchmark Quick Start (Priority: P1)

A prospective contributor (or an existing skill author validating a change) follows
`BENCHMARKING.md`'s Quick Start section on a freshly cloned copy of the public
iris-agentic-dev repository and gets a real pass-rate/lift result for a skill, with no
access to any private repository or separate Python MCP server required.

**Why this priority**: This is the exact failure a contributor hit and reported. Until
this works, the benchmarking story is not credible, and no skill contribution can be
objectively evaluated by anyone outside the private ecosystem.

**Independent Test**: On a machine with only the public iris-agentic-dev repo cloned and
Docker available, run the benchmark harness against one of the repository's own sample
skills with `--baseline`. Verify a JSON result containing `pass_rate`, `baseline_pass_rate`,
and `lift` is produced, matching the shape BENCHMARKING.md's "Understanding Results"
section describes.

**Acceptance Scenarios**:

1. **Given** a fresh clone of iris-agentic-dev with no other setup, **When** the
   documented Quick Start steps are followed, **Then** the benchmark completes and
   produces a result file with `pass_rate`, `baseline_pass_rate`, and `lift` fields —
   no reference to an inaccessible external repository is required at any step.
2. **Given** a skill that measurably helps on at least one task, **When** the benchmark
   is run with `--baseline`, **Then** `lift` is a positive number equal to
   `pass_rate - baseline_pass_rate`, matching the existing lift definition already in
   use elsewhere in the ecosystem.
3. **Given** the benchmark run completes, **When** the result is inspected, **Then** each
   task's outcome (pass/fail, iteration count, duration) is present in the per-task detail,
   matching the existing "Understanding Results" example format.
4. **Given** no IRIS container is available or reachable, **When** the benchmark is
   invoked, **Then** a structured, actionable error is returned (not a hang or a stack
   trace) — matching the Troubleshooting section's `IRIS container not found` guidance.

---

### User Story 2 — Durable Tool-Call Record Beyond the Session (Priority: P1)

A developer wants to know what tool calls happened during a benchmark run (or any
session) after that session has ended — for debugging a failed benchmark task, auditing
what an agent actually did, or feeding a downstream analysis.

**Why this priority**: Both consumers of this feature (benchmarking, dispatch tracing)
need data that outlives the calling process. Without durability, this feature does not
actually solve the "two consumers, one source of truth" problem it exists to solve.

**Independent Test**: Run any sequence of tool calls, restart the process, and query the
telemetry record for the prior session. Verify tool name, success/failure, and duration
are present for each call, and that call count is not capped at the in-memory 50-entry
limit `agent_history` currently imposes.

**Acceptance Scenarios**:

1. **Given** more than 50 tool calls occur in a single session, **When** the telemetry
   record is queried, **Then** all calls are present — not just the most recent 50 (the
   existing in-memory limit is superseded, not extended).
2. **Given** a session ends and the process restarts, **When** the telemetry record is
   queried for the prior session, **Then** that session's tool calls are still present.
3. **Given** a tool call's parameters would be redacted under the active PHI/data policy
   for the target namespace, **When** that call is recorded, **Then** the parameters
   field is redacted in the durable record, while tool name/success/duration are still
   recorded — consistent with spec 051's existing redaction behavior for other tools.
4. **Given** the telemetry record is queried for tool calls involving a specific tool
   name, **When** the query is made, **Then** only matching entries are returned.

---

### User Story 3 — Export Tool-Call Data as Dispatch Trace Records (Priority: P2)

A downstream consumer (the forthcoming `iris_graph` runtime-tracing capability) wants to
read the same underlying tool-call data as a stream of `{from, to, via, count, ts}`
records — the exact shape the dispatch-graph feature already expects from external
tracers — without iris-agentic-dev needing a second, separate instrumentation mechanism.

**Why this priority**: This is what prevents duplicate instrumentation. It is P2 rather
than P1 because the benchmark harness (US1/US2) delivers value on its own even before any
graph-consuming feature exists; this user story is about making the *same* data reusable,
not about a second recording mechanism.

**Independent Test**: After a session with several tool calls, request the tool-call
record exported in the trace-record format. Verify each entry has the fields the
dispatch-graph feature's ingestion format requires, and that repeated identical
from/to/via combinations are aggregated into a single record with an incremented count
rather than one record per occurrence.

**Acceptance Scenarios**:

1. **Given** a recorded session with repeated calls to the same tool from the same
   calling context, **When** the trace-record export is requested, **Then** those calls
   are aggregated into one record with `count` reflecting the total occurrences, not one
   record per call.
2. **Given** a recorded session with calls to several different tools, **When** the
   trace-record export is requested, **Then** each distinct from/to/via combination
   appears as its own record.
3. **Given** the trace-record export format, **When** compared against the dispatch-graph
   feature's documented ingestion format, **Then** every required field is present with
   compatible types (no format translation needed downstream).

---

### Edge Cases

- **Telemetry storage growth**: what happens when the durable record grows very large
  over many sessions? The record MUST support a retention/pruning policy (e.g. age-based
  or size-based) so it does not grow unbounded; pruning MUST NOT occur mid-benchmark-run
  (a run's own data must survive until that run's result is finalized).
- **Benchmark task suite drift**: what happens if a ported task's expected behavior no
  longer matches the current tool/skill surface (e.g. a tool was renamed since the task
  was written)? The harness MUST report that task as an error distinct from a normal
  fail, so a stale task is not silently counted against a skill's score.
- **Concurrent benchmark runs**: what happens if two benchmark runs execute against the
  same IRIS container at the same time? The harness MUST detect and reject a concurrent
  run against a container already in use by another active run, rather than corrupting
  both runs' results.
- **Telemetry recording failure**: what happens if writing a telemetry entry fails (e.g.
  disk full, permissions)? The tool call itself MUST still complete and return its normal
  result — telemetry recording is best-effort and MUST NOT block or fail the underlying
  tool call.
- **No live IRIS connection**: what happens to durable persistence when a session runs
  entirely in local-file-only mode (no IRIS connection ever established)? Durable writes
  MUST fall back to the local append-only file sink rather than being silently dropped —
  the in-memory record remains queryable regardless, and durability is on a best-effort
  basis in both sink modes.
- **Connection state changes mid-session**: what happens if a session starts with no
  IRIS connection and later establishes one (or loses one it had)? Each record is written
  to whichever sink is active at the moment that call completes; the system MUST NOT
  attempt to migrate already-written records between sinks when connection state changes.
- **PHI-adjacent parameters in benchmark task definitions**: benchmark tasks are
  synthetic and MUST NOT contain real PHI by construction; the redaction behavior in US2
  applies to live tool-call telemetry generally, not specifically to benchmark runs.

---

## Requirements

### Functional Requirements

- **FR-001**: The system MUST record every tool invocation's tool name, success/failure
  outcome, and duration. An in-memory record remains the always-available, authoritative
  source for live queries (`agent_history`/`agent_stats`), superseding the existing
  50-entry cap with a larger bound. In addition, each record MUST be written, on a
  best-effort basis, to a store that survives process restarts: an IRIS global when a
  live IRIS connection is active, or a local append-only file under the workspace/config
  data directory when no IRIS connection is active (FR-014 governs failure handling for
  this write).
- **FR-002**: The durable telemetry record MUST be queryable by tool name, by time range,
  and by session/run identifier.
- **FR-003**: Tool-call parameters MUST be captured in the durable record only when the
  active data policy for the target connection permits full parameter capture; otherwise
  parameters MUST be redacted while tool name/outcome/duration are still recorded.
- **FR-004**: The system MUST provide a benchmark harness that: (a) loads a skill
  definition, (b) runs it against a task suite, (c) optionally runs the same suite
  without the skill as a baseline, and (d) reports `pass_rate`, `baseline_pass_rate`
  (when a baseline was run), and `lift` (defined as `pass_rate - baseline_pass_rate`).
- **FR-005**: The benchmark harness MUST be fully runnable from a clean clone of the
  public repository with no reference to, or dependency on, any private/inaccessible
  external repository.
- **FR-006**: The benchmark harness MUST use iris-agentic-dev's own tool implementations
  (compile, execute, test) to run tasks, not a separate Python MCP server process.
- **FR-007**: The system MUST port at least the primary repair task suite referenced by
  `BENCHMARKING.md`'s Quick Start section into the public repository, preserving each
  task's pass/fail evaluation criteria.
- **FR-008**: `BENCHMARKING.md` MUST be corrected so every documented step is executable
  against the public repository as published, with no broken or inaccessible references.
- **FR-009**: The system MUST provide a way to export recorded tool-call data as a stream
  of trace records, each containing `from`, `to`, `via`, `count`, and `ts` fields, with
  repeated identical from/to/via combinations aggregated into a single record with an
  incremented count rather than duplicated.
- **FR-010**: The trace-record export format MUST be directly compatible with the
  dispatch-graph feature's documented trace-ingestion format, requiring no translation
  step between this feature's output and that feature's input.
- **FR-011**: The durable telemetry record MUST support pruning by age or size without
  data-loss to any benchmark run still in progress.
- **FR-012**: A benchmark task whose expected tool/skill surface no longer matches the
  current system MUST be reported as an error outcome, distinguishable from a normal
  task failure, rather than silently counted as a fail.
- **FR-013**: The benchmark harness MUST detect and reject an attempt to start a new run
  against an IRIS container already in active use by another run.
- **FR-014**: A failure to write a telemetry entry MUST NOT cause the underlying tool
  call to fail or block.

### Key Entities

- **Session**: One MCP server process lifetime. A session identifier is generated once
  at process start and stays fixed until the process exits; every `ToolCallRecord`
  produced during that lifetime carries that identifier.
- **ToolCallRecord**: A single durable telemetry entry — tool name, outcome
  (success/failure), duration, timestamp, session identifier, and optionally parameters
  (subject to redaction policy).
- **BenchmarkTask**: A single scoring unit in the ported task suite — initial state,
  goal/expected behavior, and a pass/fail evaluation method independent of any specific
  skill.
- **BenchmarkResult**: The outcome of a benchmark run — per-task pass/fail detail,
  aggregate `pass_rate`, optional `baseline_pass_rate`, and `lift`.
- **DispatchTraceRecord**: The exported shape of aggregated tool-call data —
  `{from, to, via, count, ts}` — matching the ingestion format already expected by the
  dispatch-graph feature.

---

## Success Criteria

### Measurable Outcomes

- **SC-001**: A contributor with only the public repository cloned can go from `git clone`
  to a completed benchmark result with lift computed, following only the steps published
  in `BENCHMARKING.md`, with zero references to inaccessible resources encountered.
- **SC-002**: Tool-call history is retained and queryable after a process restart, with no
  loss of calls beyond the previous 50-entry limit.
- **SC-003**: The same underlying tool-call data can be retrieved in two forms — raw
  queryable history (for debugging/audit) and aggregated trace-record export (for
  dispatch-graph ingestion) — with zero duplicate instrumentation code paths.
- **SC-004**: 100% of the ported primary task suite's tasks execute to a pass/fail/error
  verdict (none silently skipped or hung) in a single benchmark run.
- **SC-005**: Recording telemetry adds no user-perceptible delay to normal tool call
  response time under typical single-session usage.
- **SC-006**: A benchmark run's data remains intact and correctly attributed even when a
  telemetry-record pruning cycle occurs concurrently with that run.

---

## Assumptions

1. "Benchmark harness" in this spec means the mechanics of running a task suite and
   scoring pass/fail/lift — it does not include publishing a public leaderboard website;
   the existing `BENCHMARKING.md` "Submitting to the Leaderboard" PR-based process is
   assumed to continue unchanged as the leaderboard mechanism.
2. The primary repair task suite ported for v1 is drawn from the existing, already-
   validated task definitions and evaluation logic (compile-on-buggy-code,
   fail-on-buggy-code, pass-on-fix) — task *content* is preserved; only its home
   repository changes.
3. "Durable" telemetry storage means it survives a process restart on the same machine;
   this spec does not require multi-machine replication or a centralized telemetry
   service.
4. The dispatch-graph feature's exact trace-ingestion contract (field names/types for
   `{from, to, via, count, ts}`) is treated as a fixed, already-specified external
   contract that this feature's export must match, not something this feature defines
   or is free to alter.
5. Redaction of tool-call parameters follows the same data-policy model already
   established for other PHI-adjacent tool output; this feature does not introduce a new
   redaction mechanism, only applies the existing one to telemetry capture.
6. This feature is intentionally independent of `specs/021-path-aware-benchmark`'s
   namespace-isolation, results-file, and multi-harness driver design — that system
   answers a different question (path/harness comparison) and is out of scope as a
   dependency or reuse target for this feature's benchmark harness.
7. The in-memory ring buffer's authoritative bound for live queries (superseding the
   current 50-entry cap) is an implementation-sizing detail left to the planning phase,
   not fixed by this spec — Success Criteria constrain observable behavior (durability,
   query completeness within a session), not the exact buffer size.
