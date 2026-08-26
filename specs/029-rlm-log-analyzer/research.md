# RLM Log Analysis — Research Notes

**Feature**: 029-rlm-log-analyzer
**Status**: Pre-plan research
**Date**: 2026-08-05
**Sources**: opsreview agents/, rlm-iris/, aicore/python/rlm/, LOS embedded agent, this project's
session history

---

## The Pattern and Why It Matters Here

RLM's core claim: keep large data behind symbolic handles, accumulate findings in variables, only
call the LLM on bounded summaries. The rlm-iris reconciliation doc (aicore/outputs/rlm-iris-reconciliation.md)
lays out the formal requirements. What this document adds is concrete evidence from our own
tooling that the pattern works, where it hits friction, and what IRIS-specific constraints shape
the log analysis case.

---

## What We've Already Built (Our Own Evidence)

### 1. P-Buttons anomaly detection agent (`opsreview/agents/pbuttons_agent.py`)

The clearest RLM implementation in the repo. A `PButtonsSession` object holds all the loaded
time-series data (mgstat, vmstat, iostat per disk, EnsQCount). The LLM never sees the raw
data — it calls tools:

```text
inspect_data → get_stats → detect_zscore / detect_rolling / detect_trend
             → compare_columns → store_finding → finalize_report
```

`store_finding` accumulates anomalies in `session.findings: list[dict]`. `finalize_report`
synthesizes from the findings list, not from the raw data. The prompt explicitly states:
"Do NOT put raw data in findings — use statistics (mean, max, percentiles) and timestamps."

**Why this matters for 029**: `iris_get_log` with limit/offset is structurally identical to
the paginated P-Buttons table tools. The loop model is the same. The output of our log analysis
should match the shape of `PButtonsSession.findings` — compact structured records, not log
dumps.

Measured scale: single P-Buttons file → ~15-20 tables, hundreds of thousands of data points.
The agent handles this in one session without exceeding context because data stays in the
`PButtonsSession` object.

### 2. Lambda-RLM for SAM metrics (`opsreview/iris/src/OpsReview/AI/Tools/LambdaRLM.cls`)

An ObjectScript implementation of the RLM memory-retrieval pattern. `AI.Context.LambdaRLM`
is a third-party class (from the AI Hub preview image). `ComputePartition(total, budget, kStar, tauStar, depth)`
calculates the optimal partition parameters for hierarchical decomposition — how many items
to retrieve at each recursion level given a token budget.

Key property: "zero LLM calls during traversal." The decomposition is deterministic Rust/ObjectScript
math. The LLM only sees the top-k correlations that survive the scoring. This is exactly what
spec 029 calls for in `detail=false` mode.

**Friction point**: `LambdaRLM` requires `AI_Memory.Entries` to be populated. When it's not,
it returns `correlation_count: 0` with a note. This graceful degradation pattern is worth
copying for `iris_analyze_logs` — if there's nothing in the log store, return a clean empty
result rather than an error.

### 3. Offline diagnostic agent (`opsreview/agents/diagnose.py`)

A different angle: what happens when IRIS itself is unavailable. The external diagnostic
reads `messages.log` directly via `docker exec` or SSH, pattern-matches against a 1,355-entry
catalog (`specs/017-offline-diagnostic-rest/messages-log-catalog.csv`), and returns a
`DiagnosticReport` with severity-bucketed findings.

The catalog entries include: phrase match, severity, tags (subsystem), and action guidance.
The scan deduplicates by pattern ID — if the same error appears 500 times, you get one finding,
not 500.

**Why this matters for 029**: The dedup-by-pattern behavior is exactly what `top_error_codes`
in the spec sketch should do. Count occurrences, don't list them. The pattern catalog is the
right model for "structural analysis in Rust, LLM only for narrative."

The embedded agent architecture diagram in `docs/LOS_IRIS_AGENT.md` shows this as the
"resilient path" — when IRIS is frozen, the external diagnostic runs outside the process
boundary. `iris_analyze_logs` would be the "live path" equivalent: when IRIS is healthy,
analyze the log store without needing to dump everything to the LLM.

### 4. CBM structural enrichment (`diagnose.py` + `cbm_lookup.py`)

`enrich_findings()` in diagnose.py optionally adds call chain context to findings whose
subsystem tag is in the `DEFAULT_SUBSYSTEM_MAP`. It calls `codebase-memory-mcp` to find
which ObjectScript methods are in the call chain for the pattern's subsystem class.

This enrichment is optional and fails gracefully — if CBM isn't available or the class isn't
indexed, the finding is returned without the `call_chain` field.

**Implication for 029**: The `focus` parameter in the spec sketch (filter on class/package)
is where CBM enrichment would attach. If the structural analysis identifies `MyApp.Service`
as the most-affected class, and CBM is available, we could optionally annotate with "these
methods are in the call chain." That's a detail=true feature, not P0.

---

## What the rlm-iris Implementation Shows

`rlm-iris` (`/Users/tdyar/ws/rlm-iris/`) implements the pattern in ObjectScript with:

```objectscript
Set src = ##class(RLM.Source.Table).%New("Sales.Order", "Amount")
Do src.AddDimension("region", "Region")
Set eng = ##class(RLM.Engine).%New(src, ##class(RLM.LLM.REST).%New())
Write eng.Run("What drives order value, and where is it skewed?", .traceId)
```

The engine walks the source table hierarchically — `peek` gives shape, `children` gives
sub-regions, `aggregate` gives stats per slice. The LLM never sees rows.

The global archaeology use case in the README is the strongest analogy to log analysis:
`^JRNAUD` — 40 GB, 91M nodes, written by a routine nobody understands. The RLM approach
produces a 500-character description of the structure (subscript types, fanout, top values)
rather than exporting 40 GB. For `iris_get_log`, the "global" is the UUID-keyed error log
store, and the "structure description" is the error code frequency + class distribution +
temporal burst detection.

---

## Concrete Use Cases (Observed from Our Sessions)

### Case 1: Silent overnight freeze detection

Tom's `los-iris` froze at 1:52am on 2026-05-17 (`WDSTOP set to freeze system`). Six hours of
downtime, nobody noticed. The watchdog + external diagnostic was built specifically to catch
this. The log pattern (`WDSTOP`) is in the catalog with severity=Fatal.

**Relevance**: An `iris_analyze_logs` call at 8am would have surfaced this immediately: one
fatal pattern, onset time of 1:52am, zero healthy samples after that. The burst detection
(all entries after 1:52am are the same error code) makes this unambiguous. No LLM call needed
for `detail=false` — the structural summary is the answer.

### Case 2: P-Buttons → log correlation

The P-Buttons agent finds a WDQsz spike at 14:07 on a specific date. The next question is
"what was IRIS logging at that time?" This requires a `journal_search` or `iris_analyze_logs`
call focused on the same 15-minute window. Today you'd have to do that manually. With
`iris_analyze_logs(time_range={"from": "14:00", "to": "14:30"})`, the temporal burst
detection would confirm whether the spike coincided with a specific error cluster.

**Relevance**: The `burst_window` field in the spec sketch exists specifically for this use
case. It's the bridge between the infrastructure-level P-Buttons signal and the application-level
IRIS log signal.

### Case 3: SQL workload anomaly investigation

`opsreview/agents/sql_workload_agent.py` finds high-cost query groups. The next investigation
step — which is currently manual — is "were there errors logged around these queries?" An
`iris_analyze_logs(focus="User.SQL", time_range=...)` would surface `<MAXSTRING>` or
`<UNDEFINED>` patterns in SQL-related classes without dumping raw log entries.

### Case 4: HealthShare/Ensemble production health

The embedded agent architecture shows SAM queue metrics (HL7 queue depth, FHIR throughput)
fed into the RLM decomposition. When a queue is backing up, the natural next step is to check
the application error log for the relevant Ensemble job. Today this requires navigating to
Management Portal manually. `iris_analyze_logs(namespace="HSPIERRE", focus="Ens")` would give
you error code distribution in Ensemble classes in that namespace in seconds.

---

## Open Questions — Resolved Against the Evidence

The spec left three questions open. Here's what the evidence says:

### Q1: Server-side vs client-side iteration?

**Answer: server-side (Rust), no question.**

Every implementation in the repo — P-Buttons session, Lambda-RLM, diagnose.py, rlm-iris engine —
runs the loop inside the tool, not in the agent. The agent calls one tool and gets back a
structured summary. Making the agent iterate via `iris_get_log` pagination would require the
agent to write loop logic, track state across calls, and know about offset/limit semantics.
That's the wrong abstraction for a tool API.

The spec note about "client-side is more RLM-pure" misreads what rlm-iris actually does:
the `RLM.Engine` IS the server-side loop. The LLM calls `eng.Run()` and gets a report.
`iris_analyze_logs` should be `eng.Run()`, not a set of primitives that the LLM assembles.

### Q2: Burst detection threshold?

**Answer: adaptive, derived from the data.**

The P-Buttons `detect_rolling` tool uses a configurable rolling window (default 5-minute).
The `detect_zscore` tool flags values > 2σ or > 3σ as medium/high. For log burst detection,
the right approach is: compute mean entries-per-minute over the full time range, then flag
windows where the rate exceeds 3σ as a burst. This is adaptive — a quiet instance that logs
10 entries/hour will surface a burst of 50 entries in a 5-minute window; a busy instance
that logs 1000 entries/hour would not.

Fixed window (e.g., "more than 100 entries in 5 minutes") would false-positive on busy
instances and miss bursts on quiet ones.

### Q3: Write analysis result back to log store?

**Answer: no, not in P0.**

The diagnose.py external diagnostic returns a `DiagnosticReport` JSON and does not persist it.
The P-Buttons agent writes a markdown report to disk, not back to IRIS. Persisting to the log
store adds scope (what key? what TTL? who reads it back?) with no immediate benefit. The agent
can store the structured result itself if it wants to reference it later. Skip for P0.

---

## What This Means for the Spec

The spec's "Sketch" status reflects genuine uncertainty about implementation approach. The
evidence above resolves most of that uncertainty:

1. **Server-side Rust loop** over `iris_get_log` pages — same as rlm-iris engine
2. **Structural analysis** (counts, burst detection, class distribution) in Rust — zero LLM
3. **detail=false** returns the structured summary — this is the default and covers most cases
4. **detail=true** calls `iris_execute_method` on an LLM via MCP sampling IF available,
   passing only the structured summary (not raw entries) as input
5. **Burst detection** is adaptive (rolling z-score), not fixed-threshold
6. **No persistence** of analysis results in P0
7. **CBM enrichment** of top classes is a post-P0 option, not a P0 feature

The spec is ready to move to `/speckit.plan`.

---

## Related Reading

- `aicore/outputs/rlm-iris-reconciliation.md` — formal RLM requirements and architecture
- `opsreview/agents/pbuttons_agent.py` — the most complete RLM-pattern implementation in the
  repo; the session/tools/findings structure is the template for this feature
- `opsreview/agents/diagnose.py` — the pattern catalog approach to log dedup and severity
- `opsreview/iris/src/OpsReview/AI/Tools/LambdaRLM.cls` — Lambda-RLM MCP tool; shows the
  graceful degradation pattern for missing data
- `rlm-iris/README.md` — global archaeology use case; closest analogy to log store analysis
- `specs/027-*` — the UUID log store that provides the paginated access primitive
