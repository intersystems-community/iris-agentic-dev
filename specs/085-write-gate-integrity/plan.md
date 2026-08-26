# Implementation Plan: Write-Gate Integrity

**Branch**: `085-write-gate-integrity` | **Date**: 2026-08-25 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/085-write-gate-integrity/spec.md`

## Summary

Make the write and destructive gates enforce what the configuration declares, and make the gap
impossible to reopen. Five changes, in dependency order:

1. **Resolve the gate as data.** A pure `resolve_gates()` returning a value plus the source that
   decided it, replacing the write-once `IRIS_WRITE_TOOLS_ENABLED` env var that no second config
   load can change. This is the #110 stale-value defect and the reason no test could reach it.
2. **Classify every tool once, declaratively**, per tool and per action, cross-checked against the
   MCP annotations already in the router so a contributor has to lie twice.
3. **Enforce at one dispatch point** — `ServerHandler::call_tool`, before the router runs, before
   IRIS is touched — deleting the six per-handler guards and the two router removals.
4. **Fail closed on the contradictory config**, with a non-zero exit, replacing the `return None`
   that currently drops through to the permissive namespace inference.
5. **Close the surfaces that let prose ship ahead of code**: a docs/skills integrity test with
   four extractors, and `--locked` on every build so lockfile drift fails loudly instead of
   dirtying the version string.

Phase 0 is complete: [research.md](./research.md) carries the current architecture, six decisions
with rejected alternatives, the API verification table, and an explicit account of what the docs
test cannot catch.

## Technical Context

**Language/Version**: Rust, workspace edition 2021
**Primary Dependencies**: `rmcp` (MCP server + tool router), `tokio`, `serde` / `serde_json` /
`toml`, `tracing`. No new crates — see research.md, Dependencies.
**Storage**: `.iris-agentic-dev.toml` (workspace config); no database
**Testing**: `cargo test` for unit and binary-invocation layers; `cargo test -- --include-ignored
--test-threads=1` against live `iris-dev-iris` (localhost:52780) for the enforcement matrix;
`cargo llvm-cov --features testing` for the coverage gate
**Target Platform**: macOS arm64/x86_64, Linux x86_64/arm64, Windows x86_64 (release matrix).
The gate is platform-independent; the lockfile and version-string work touches the release
workflow for all five targets.
**Project Type**: Single Rust workspace, two crates (`iris-agentic-dev-core`,
`iris-agentic-dev-bin`)
**Performance Goals**: The classification lookup runs on every tool call. Target: no measurable
added latency on the read-only path — a lookup over a sorted static slice of ~75 entries, no
allocation, no IRIS round trip, no lock.
**Constraints**: Gate enforcement must not depend on IRIS connectivity (FR-012) and must take
effect inside an established session (FR-011), which rules out anything that re-shapes the tool
list. Refusals must keep the existing `err_json` response shape (Principle V) so the reporter's
published probes keep working.
**Scale/Scope**: ~75 registered tools across three tiers (Baseline, Nostub, Merged); 6 existing
guard sites removed; 2 router removals removed; ~130 existing `[[test]]` targets in core.

## Constitution Check

_GATE: Must pass before Phase 0 research. Re-check after Phase 1 design._

| Principle                      | Status                   | Notes                                                                                                                                                                                                                                                                                                                                                                                             |
| ------------------------------ | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary         | PASS                     | No new install step, no new runtime requirement. Enforcement is in-process.                                                                                                                                                                                                                                                                                                                       |
| II. ObjectScript Sanity        | PASS                     | No new ObjectScript. research.md carries the verification table for every existing API this feature relies on; enforcement happens before any IRIS call.                                                                                                                                                                                                                                          |
| III. HTTP-First Execution      | PASS                     | No new tools, so no new Docker-required tool in the Merged tier. The gate itself needs no transport.                                                                                                                                                                                                                                                                                              |
| IV. Test-First, Fixture-Driven | PASS                     | FR-022 through FR-030 specify the tests as requirements. The pure resolver and classification table are testable with no IRIS; the enforcement matrix needs live IRIS by design.                                                                                                                                                                                                                  |
| V. Output Shape Parity         | PASS                     | Refusals reuse the `err_json` envelope the four current guards already emit — same `error_code` field, same shape. Deliberately not an `McpError`.                                                                                                                                                                                                                                                |
| VI. Environment Guard          | **PASS — remediation**   | **The codebase violates this principle today.** "Any new tool that can modify IRIS data MUST be classified as write-capable and subject to this gate" — at least five tools are not. This feature is the repair. See Constitution Remediation below.                                                                                                                                              |
| VII. Dependency Minimalism     | PASS                     | Zero new crates. `const` data, a pure function, and `regex` which is already a dev-dependency.                                                                                                                                                                                                                                                                                                    |
| VIII. 90% Coverage Gate        | PASS — with an exception | Polish carries the constitution's canonical `cargo llvm-cov --summary-only -p iris-agentic-dev-core --features testing -- --include-ignored` task. `write_gate.rs` must land ≥ 90% on its own. The crate TOTAL is 85.00% before this feature starts, so this feature cannot reach the 90% gate by itself — recorded as an exception in Complexity Tracking rather than restated as a weaker gate. |
| IX. Tool Lift Requirement      | PASS                     | No new tool, so no lift measurement is owed. But T023 changes the advertised tool list when writes are off, so a no-regression benchmark run is required — and Principle IX says the phase carrying that evidence MUST precede Polish and cannot be deferred. `tasks.md` Phase 10 is that gate; results go in `lift-results.md`.                                                                  |
| X. ObjectScript Coverage       | N/A                      | Pure Rust feature. No `.cls`, `.mac`, or `.int` added or changed, so no `TestCoverage` run or `coverage-results.md` is owed.                                                                                                                                                                                                                                                                      |

_A plan with any FAIL gate MUST NOT proceed to implementation._

### Constitution Remediation (Principle VI)

Principle VI has been in force since before the defect: constitution v1.3.2, ratified 2026-05-01,
last amended 2026-08-19, and the classification MUST predates the 2026-08-01 commits. So this is
not a plan that needs a waiver — it is a plan whose entire purpose is to bring the codebase back
into compliance with a principle it has been violating for five releases.

Two consequences for how this plan is judged:

- The Constitution Check cannot be satisfied by the plan alone. It is satisfied when the
  enforcement matrix (FR-026) is green against live IRIS, because that test is the only artifact
  that can demonstrate compliance for the whole tool set rather than for the tools someone
  remembered.
- Principle VI's own wording — "Any **new** tool that can modify IRIS data" — is why three rounds
  of fixes each covered only the tools in front of them. The principle says what to do about new
  tools and says nothing about auditing the existing set. That is a constitution gap, not a plan
  gap, and it is out of scope here (see spec.md, Out of Scope: repository process controls).

## Project Structure

### Documentation (this feature)

```text
specs/085-write-gate-integrity/
├── spec.md              # Complete, marker-free
├── plan.md              # This file
├── research.md          # Phase 0 — six decisions, API verification, docs-test blind spots
├── data-model.md        # Phase 1 — gate value, source enum, classification, error registry
├── quickstart.md        # Phase 1 — how to verify each defect is closed
├── contracts/
│   └── check_config.md  # Phase 1 — response shape addition (gate value + deciding source)
├── checklists/
│   └── requirements.md  # Green
└── tasks.md             # Phase 2 — /speckit.tasks, not created here
```

### Source Code (repository root)

```text
crates/iris-agentic-dev-core/
├── src/
│   ├── iris/
│   │   ├── connection.rs          # is_write_allowed() → delegates to the resolver; inference kept
│   │   └── workspace_config.rs    # loses the env exports and the fail-open `return None`;
│   │                              #   gains validate_gate_config()
│   └── tools/
│       ├── mod.rs                 # call_tool() gains the single gate check;
│       │                          #   6 handler guards and 2 router removals deleted;
│       │                          #   ConnectionState carries GateResolution
│       ├── write_gate.rs          # NEW — GateResolution, GateSource, resolve_gates(),
│       │                          #   WriteClass, CLASSIFICATION table
│       └── admin_tools.rs         # 2 handler guards deleted; ERR_WRITE_GATE stays
└── tests/
    ├── unit/
    │   ├── test_gate_resolution.rs    # NEW — pure resolver, toml strings, env-already-set branch
    │   ├── test_gate_classification.rs # NEW — completeness both ways + annotation cross-check
    │   ├── test_docs_contract.rs       # NEW — four extractors over docs/ and skills/
    │   └── test_lockfile_sync.rs       # NEW — cargo metadata --locked
    └── integration/
        └── test_gate_enforcement_live.rs # NEW — matrix vs live IRIS, asserts absent side effects

crates/iris-agentic-dev-bin/
├── src/cmd/mcp.rs                  # calls validate_gate_config(); exit(2) on rejection
└── tests/integration/
    └── test_mcp_binary_config.rs    # gains the rewrite-config-twice-in-one-process test

docs/tools.md                        # allowlist section deleted; destructive section corrected;
                                     #   072 leftovers and the stale count fixed
skills/skills/iris-agentic-dev/SKILL.md  # phantom Tier 3 and two phantom error codes removed
.github/workflows/{ci,release}.yml   # --locked on every build/test step
```

**Structure Decision**: Existing two-crate workspace, unchanged. One new module
(`tools/write_gate.rs`) holds the resolver and the classification table together, because the
completeness test needs both and splitting them invites the table drifting from the enum. Five new
test targets, each registered as a `[[test]]` in `crates/iris-agentic-dev-core/Cargo.toml` (or the
bin crate) following the existing convention; the live one takes `required-features = ["testing"]`.

## Complexity Tracking

The Constitution Check has no FAIL gates. Two design choices, one gate exception, and one
documented-command deviation are recorded here — the design choices because a reviewer will
otherwise flag them as redundant, the exception because a MUST that a feature cannot meet should be
named rather than quietly restated as something weaker, and the deviation because running a
command different from the one the constitution prints needs a reason on the record.

| Choice                                                                                           | Why needed                                                                                                                                                                                                                                                                                                                                                                                                                             | Simpler alternative rejected because                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A `CLASSIFICATION` table that restates what `read_only_hint` / `destructive_hint` already assert | Two independent declarations that a test cross-checks. A contributor adding an ungated write tool has to get both wrong in the same commit for it to ship.                                                                                                                                                                                                                                                                             | Deriving the gate from the annotations alone. The annotations were provably wrong: `c641d79` (2026-08-18, #94) had to remove `read_only_hint` from six mutating tools that had shipped advertising themselves as read-only. Deriving enforcement from them would have made that mistake a security hole instead of a hint bug.                                                                                                                                                                               |
| Per-action classification rather than per-tool                                                   | `iris_doc` writes on 4 of ~8 modes, `iris_query` only on `mode="write"`, `iris_lookup_manage` has documented read actions, and the seventh destructive item is `skill(action="forget")` — an action, not a tool. Per-tool granularity would either gate reads or leak writes.                                                                                                                                                          | Splitting the multi-action tools into separate read and write tools. That is a breaking change to the advertised tool surface, affects every agent prompt and bundled skill, and is a much larger blast radius than the defect warrants.                                                                                                                                                                                                                                                                     |
| Principle VIII's 90% crate coverage gate is still unmet when this feature lands                  | The crate TOTAL is 85.00% at `21a1bfb`, before any of this work. The gap is pre-existing and architectural; closing it means covering modules this feature never touches. What this feature does owe is two numbers T073 checks: `write_gate.rs` ≥ 90% on its own, and the crate TOTAL not below the T003 baseline.                                                                                                                    | Writing coverage to 90% inside this branch — that turns a security fix into a coverage project and delays the enforcement matrix. Rejected as scope, not as principle. Recorded here rather than folded silently into T073, because "TOTAL not below baseline" is a weaker claim than the constitution makes. The constitution also contradicts itself on the number: Principle VIII says ≥ 90%, Release Discipline says `scripts/coverage.sh` ≥ 88%. Reconciling those needs an amendment, not a plan edit. |
| The coverage command run is the constitution's, with `IRIS_PORT` corrected to `IRIS_WEB_PORT`    | No code reads `IRIS_PORT` — `git grep '"IRIS_PORT"' crates/*/src` returns nothing; the variable the discovery cascade and `scripts/coverage.sh` both use is `IRIS_WEB_PORT`. With the constitution's literal text, `discovery_tests::discover_iris_returns_none_when_nothing_found` fails on any machine running an IRIS container, because its skip guard tests `IRIS_WEB_PORT`, so the coverage run aborts before producing a TOTAL. | Running the command verbatim and reporting the abort. That produces no coverage number at all, which is worse than one measured with a working env var. Correcting `.specify/memory/constitution.md:192,202` is a constitution amendment and out of scope for this feature.                                                                                                                                                                                                                                  |
