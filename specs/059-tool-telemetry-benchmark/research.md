# Research: Tool Telemetry and Benchmark Harness

Verified against `iris-dev-iris` (port 52780/HTTP, USER namespace, via
`execute_via_generator`/direct global access — no compile step required for global
reads/writes, confirmed live).

## Durable IRIS-global sink: `^IRISDEV("telemetry", session_id, seq_n)`

**Decision**: use `^IRISDEV("telemetry", session_id, seq_n)` as the durable-sink global,
not objectscript-coder's `^TOOLCALLS(session_id, seq_n)` name. `^IRISDEV` is unused by any
existing iris-agentic-dev Rust code (`grep` confirms no `^TOOLCALLS`/`^IRISDEV` references)
and is consistent with `058-iris-graph/research.md`'s own open item #1, which already
proposed an `IRISDEV`-namespaced global prefix for that feature's config. Using a shared
top-level `^IRISDEV` global (subscripted by feature name) avoids polluting the target
namespace's global list with one top-level global per iris-agentic-dev feature.

**Verified live** — no compile/class step needed, a direct `$INCREMENT`/`set`/`kill`
against a global works via the existing HTTP-only `execute_via_generator` path:
```
set id=$INCREMENT(^IRISDEV("telemetry","test_session"))
set ^IRISDEV("telemetry","test_session",id)=$LISTBUILD("iris_compile",1,123,$ZDATETIME($HOROLOG,3))
→ wrote 1
write $LISTTOSTRING(^IRISDEV("telemetry","test_session",1),"|")
→ iris_compile|1|123|2026-07-01 20:31:28
```
Record encoding: `$LISTBUILD(tool, success_flag, duration_ms, iso_timestamp)` at
`^IRISDEV("telemetry", session_id, seq_n)`, with `seq_n` from
`$INCREMENT(^IRISDEV("telemetry",session_id))` (the 0-subscript node doubles as both the
per-session sequence counter and the "does this session have any durable records" existence
check). Parameters (when policy allows capture) go in a 5th list element; when redacted,
that element is simply omitted rather than replaced with a placeholder, keeping the common
(no-params) case's record small.

**Alternatives considered**: SQL table (`Ens.Rule`-style persistent class) — rejected,
because it requires a compile step (a class must exist and be compiled before any row can
be written), reintroducing exactly the "IRIS class-cache/compile fragility" risk this
project has repeatedly hit (Bug S in 058, stale-fixture issues in 055/056). A bare global
write needs no compiled class and matches the existing `execute_via_generator` HTTP-only
execution model with zero extra moving parts.

## Local file sink: JSONL under a `telemetry/` subdir of the config data directory

**Decision**: when no live IRIS connection exists (spec 025 local-file mode), append one
JSON object per line to `<config_dir>/telemetry/<session_id>.jsonl`, where `<config_dir>`
follows the same resolution the existing `.iris-agentic-dev.toml` workspace-config lookup
uses (workspace root, falling back to a user config dir) — this avoids inventing a second
path-resolution convention. Each line is the same record shape as the IRIS-global sink
(tool, success, duration_ms, timestamp, session_id, optional redacted params), just
JSON-encoded instead of `$LISTBUILD`-encoded, so the aggregation/export code in
`trace_export.rs` operates on one in-memory `ToolCallRecord` type regardless of which sink
produced it — sink choice only affects the write path, never the read/query/export logic.

**Alternatives considered**: SQLite — rejected per Principle VII (a new dependency for a
capability an append-only JSONL file already provides at v1's scale; Assumption 3 caps
scope at single-machine, non-centralized durability, so no query-by-SQL requirement exists
that JSONL can't satisfy via simple line scanning).

## Session identifier: `uuid` v4, generated once at `IrisTools::new`

**Decision**: add the `uuid` crate (`v4` feature) as a new workspace dependency, generate
one UUID when `IrisTools::new`/`new_with_toolset` runs, store it alongside the existing
`history` field. This matches the Clarifications session's "one MCP server process
lifetime = one session" decision exactly and mirrors objectscript-coder's `CallRecorder`
per-process `session_id` model.

**Alternatives considered**: PID + start-timestamp — rejected; PIDs recycle across fast
process restarts (a realistic scenario for benchmark-loop invocations), which risks two
distinct sessions colliding under the same durable-sink subscript, corrupting the
session-scoped queries FR-002 requires. A single small, widely-used crate (`uuid`) is a
better tradeoff than a hand-rolled uniqueness scheme that itself needs dedicated testing.

## In-memory ring buffer capacity: configurable, default 5,000

**Decision**: replace the hardcoded `VecDeque::with_capacity(50)` with a capacity read from
an env var (`IRIS_TELEMETRY_BUFFER_SIZE`, default `5000`), following the exact precedent
already set by `IRIS_LOG_STORE_MAX`/`IRIS_LOG_TTL_MINUTES` in `IrisTools::new`. 5,000
entries at the existing `ToolCallEntry` struct's size (~48 bytes plus a `String` tool name)
is well under 1MB resident — trivial memory cost — while comfortably covering a full
benchmark run (26 tasks × a handful of tool calls each) without truncation. This resolves
Assumption 7's explicitly-deferred sizing question.

**Alternatives considered**: unbounded `Vec` — rejected; Edge Case "Telemetry storage
growth" explicitly requires a retention policy, and an unbounded in-memory buffer would
defeat that for long-running MCP server processes (interactive sessions, not just
benchmark runs) — the durable sink, not the in-memory buffer, is the right place for
unbounded history; the in-memory buffer only needs to serve *live* `agent_history` queries.

## Pruning policy: age-based, applied to the durable sink only, checkpointed against active runs

**Decision**: pruning targets the durable sink (IRIS global / JSONL file), never the
in-memory ring buffer (which is already self-pruning via its fixed capacity). Age-based
default (`IRIS_TELEMETRY_RETENTION_DAYS`, default 30) — simpler to reason about than
size-based for a per-session-subscripted global, since pruning is "delete session
subscripts older than N days," a single pass over `$ORDER(^IRISDEV("telemetry",session))`
subscripts compared against each session's first-record timestamp. To satisfy FR-011/
SC-006 ("pruning MUST NOT occur mid-benchmark-run"), the benchmark harness registers its
own `session_id` in a small in-memory "active runs" set (already implicit — a running
`IrisTools` process's own session is always active) and pruning explicitly skips the
current process's session_id; since pruning only ever runs against *other*, already-exited
sessions' data (a session can only be pruned after that session's process has exited), a
run in progress can never be pruned regardless of timing, making the "concurrent with a
live run" edge case satisfied by construction.

## Benchmark harness IRIS interaction: fully HTTP, mirroring objectscript-coder's own `IrisLight.BenchRunner` mechanism — not `%UnitTest.Manager`

**Decision (supersedes two earlier drafts of this section)**: the ported `jira_bugs`
task suite's `test_code` classes extend `%RegisteredObject` with `ClassMethod`/`Method`s
named `TestXxx` — verified by reading all 22 ported task files' `test_code.content` first
lines; NONE extend `%UnitTest.TestCase`. This is objectscript-coder's own
`IrisLight.BenchRunner.RunAll` mechanism (`objectscript_mcp/benchmark/runner.py`):
introspect the compiled class's `%Dictionary.ClassDefinition.Methods`, invoke each
`Test*`-named method directly via `$ClassMethod`/`$Method`, catch exceptions to detect
failure. This needs no `%UnitTest.Manager`, `^UnitTestRoot`, or Docker at all — it runs
correctly and deterministically over the plain `execute_via_generator` HTTP path,
verified live end-to-end (real bug → fail, real fix → pass, wrong fix → fail).

**Two earlier, incorrect approaches were tried and rejected before landing on this**:

1. *Compiled-class-name pattern via `%UnitTest.Manager.RunTest`* — rejected.
   `RunTest`'s `testspec` argument is ALWAYS a filesystem directory path under
   `^UnitTestRoot` (confirmed against IRIS's own class documentation: "Loads and
   compiles any test files in the directory specified by the first argument, testspec").
   A pattern like `"BenchSmoke.TestAdder"` with `/noload` does not run an
   already-compiled class — it silently finds zero test files in a directory and
   reports `"All PASSED"` vacuously (confirmed via a side-effect global write inside the
   test method that never fired).
2. *Directory-file-based `%UnitTest.Manager.RunTest`, correctly targeting a written
   `.cls` file under `^UnitTestRoot/<FullDottedClassPath>/`* — this IS the correct way
   to invoke `RunTest`, and worked perfectly when invoked via `docker exec`'s real
   terminal session (verified: correct per-method "TestFoo passed"/"TestFoo FAILED --
   msg" output, deterministic across repeated runs). But run via the HTTP-only
   `execute_via_generator` path, it is **deterministically broken**: it reports "All
   PASSED" even for a class with a single deliberately-failing assertion, every time,
   with a fresh class each run (confirmed via the same side-effect-global technique, and
   via direct `^UnitTest.Result` inspection showing a fabricated pass record). Root cause
   is believed to be `$Principal` being `/dev/null` inside `execute_via_generator`'s
   device-redirected execution context (confirmed via `write $Principal`), which
   `%UnitTest.Manager`'s own internal device/terminal handling appears to depend on to
   actually invoke test methods — not merely to produce visible output. This approach
   would have required Docker exec (the same documented exception Constitution
   Principle III already grants `iris_test`) purely to work around this limitation, but
   is unnecessary since the task suite doesn't use `%UnitTest.TestCase` at all.

**The `IrisLight.BenchRunner`-equivalent mechanism has none of these problems** because
it never invokes `%UnitTest.Manager` — it's a direct `$ClassMethod`/`$Method` call inside
a `try`/`catch`, which works identically whether invoked via `execute_via_generator` or
`docker exec`. One real quirk to account for: Atelier PUT auto-prefixes a bare
(no-package) class name's document with `User.` (e.g. `Class TestFoo {...}` compiles to
`User.TestFoo`, not `TestFoo` — verified live via `/docnames/CLS` listing). All 22 ported
tasks' `test_code` classes are bare-named, so `benchmark::container::resolve_class_name`
predicts this prefix before referencing the compiled class.

**Decision**: `benchmark::container::run_class_tests(iris, client, namespace, class_name)`
implements the `RunAll`-equivalent introspection/invocation directly in generated
ObjectScript (via `execute_via_generator`), returning `(all_passed, detail)` where
`detail` lists failing methods semicolon-separated. `write_and_compile` (Atelier
PUT+compile, same pattern `iris_compile`'s local-file branch already uses) handles both
the task's initial buggy source and the LLM-proposed fix. No Docker exec, no
`IRIS_CONTAINER` requirement, matching FR-005/FR-006's "no private-repo/Python-MCP
dependency" intent as cleanly as possible — this is materially simpler than either
`%UnitTest.Manager` approach and fully satisfies Constitution Principle III's HTTP-first
default with no exception needed.

**Alternatives considered**: `%UnitTest.Manager.RunTest` (see the two rejected attempts
above) — rejected for the reasons given; keep the Python harness's `docker exec`/`iris
session IRIS` subprocess approach in Rust via `std::process::Command` — rejected as
unnecessary once the correct (`RunAll`-style) mechanism was identified, and would have
duplicated `IrisConnection::execute`'s existing Docker-exec implementation had it been
needed at all.

## Task suite port: `bench/eval_tasks/jira_bugs/*.json` (26 files), format preserved as-is

**Decision**: the 26 JSON task files in objectscript-coder's `bench/eval_tasks/jira_bugs/`
(verified: `001-null-pointer-check.json` through `021-...`, plus 5 `MF-*.json` multi-file
variants) use a self-contained schema — `task_id`, `initial_code.files[]`,
`test_code`, `expected_behavior`, `hints[]`, `success_criteria` (`compile_success`,
`tests_pass`, `max_patch_lines`, `requires_symbol_preservation`) — with no code
dependency on the Python runner beyond JSON parsing. Per the Clarifications session's
decision to port only the primary repair suite, only the base `NNN-*.json` files (the
"jira" suite BENCHMARKING.md's Quick Start references, `--suite jira` / 22 tasks per the
doc's own count) are ported for v1; the 5 `MF-*.json` (multi-file) files are left in
objectscript-coder, matching the explicit multi-file-suite deferral already recorded in
the spec's Clarifications.

**Alternatives considered**: regenerate the task suite from scratch in Rust-native form —
rejected; Assumption 2 explicitly requires task *content* preservation, only the home
repository changes, and the existing JSON schema needs no translation to be consumed by a
Rust `serde_json`-based loader.

## Trace-record export format: verified against 058-iris-graph's `record_trace` contract

**Decision**: confirmed via `git show 058-iris-graph:specs/058-iris-graph/spec.md` that the
exact expected shape is
`{"from":"Package.Class:Method","to":"Package.Class:Method","via":"direct|dispatch|workmgr","count":N,"ts":"ISO8601"}`.
This feature's `trace_export.rs` must produce exactly this shape with no translation layer,
per FR-010. `from`/`to` in the exported records are populated as `tool_name` (this
feature has no method-level call-graph data — only tool-level), and `via` is a fixed
literal (e.g. `"mcp"`) distinguishing tool-telemetry-sourced edges from Pierre's own
dispatch-tracer-sourced edges once both feed the same `record_trace` sink. This is an
intentionally coarser edge than 058's own method-to-method dispatch tracing — it answers
"which tool was called, how often" at the MCP-tool granularity, which is what this
feature's data actually captures; finer-grained method dispatch tracing remains
Pierre's own instrumentation's job, not something this feature backfills.

**Alternatives considered**: none — the ingestion contract is a fixed external interface
per Assumption 4; this feature has no freedom to alter field names/types.

## Redaction: reuse spec-051's `dataPolicy` model directly, no new mechanism

**Decision**: confirmed via reading `crates/iris-agentic-dev-core/src/policy/` that
`DataPolicy` (`Block`/`Allow`/`Redact`) and `check_bulk_phi_gate` already exist for exactly
this purpose. Telemetry parameter capture reuses the same `DataPolicy` enum and the active
workspace config's policy value — when `DataPolicy::Allow`, capture parameters verbatim;
otherwise, redact (omit) them, matching FR-003/US2-Scenario-3. No `Scrubber`/HMAC-SHA256
mechanism (objectscript-coder's `CallRecorder` approach) is introduced, per the
Clarifications session's explicit decision to diverge from that irreversible-scrubbing
design in favor of the existing spec-051 policy model.
