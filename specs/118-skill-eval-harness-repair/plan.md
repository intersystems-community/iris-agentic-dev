# Implementation Plan: Repair the skill-eval regression harness

**Branch**: `118-skill-eval-harness-repair` | **Date**: 2026-09-08 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/118-skill-eval-harness-repair/spec.md`

## Summary

Stories 1 and 2: make the scorer incapable of inventing a number, and make the baseline hold an
entry per skill with enough provenance to know when a Δ is meaningless.

The work is four changes to the Python harness plus one workflow change:

1. **Scoring returns an unscored verdict.** `score_result` stops returning `score: 0` when the
   scorer fails. Every item carries `scored`, the scoring mode, and the model the client actually
   resolved. Pass rates count scored items only; a run with more than a tenth unscored is invalid.
2. **A preflight that spends nothing.** Before the first opencode session, the harness makes one
   real scoring call and resolves the tool surface. Either failing stops the run and names what is
   missing.
3. **Baseline v2, merged by skill.** An entry carries provenance. Writing an entry never touches
   another skill's. A Δ is refused when task ids, scoring mode, or scorer model differ; a changed
   tool-surface revision is printed beside the Δ and does not suppress it.
4. **A report that says what it measured.** Per skill: scoring mode, resolved scorer model,
   items scored and unscored per arm, the outcome (regressed / held / new / not comparable), and
   the threshold the outcome was computed under. Nothing per-skill exits non-zero in 118.

Research turned up a second, independent cause of the six zero rows that the spec did not have
(§ "Scope added after research"), and confirmed the first one in code.

## Technical Context

**Language/Version**: Python 3.11 (the harness); no Rust changes
**Primary Dependencies**: `anthropic` (already installed in the eval job), `pyyaml`, `requests`,
`pytest`; opencode as the agent under test. No new package — see research R10.
**Storage**: JSON files under `tests/e2e/results/`; `skill-baseline.json` is the one durable file
**Testing**: `pytest tests/e2e/skill_eval/` (no IRIS, no network) plus live-scorer and live-agent
tests behind env markers
**Target Platform**: macOS laptop and `ubuntu-latest` in GitHub Actions
**Project Type**: single repo, test-harness subtree
**Performance Goals**: the preflight makes exactly one scoring call — asserted, not aspired to
(T010) — so its cost is one call and its wall clock is one round trip; no change to shard wall clock
**Constraints**: the nightly must not get more expensive; the preflight must run before any
billable session; the baseline write happens in exactly one job
**Scale/Scope**: 9 skills with an eval config, 34 skills in the pack, 12 benchmark tasks in play

## Constitution Check

_GATE: Must pass before Phase 0 research. Re-check after Phase 1 design._

| Principle                             | Status | Notes                                                                                                                                                                                                                                         |
| ------------------------------------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary                | N/A    | No change to the shipped binary. The CI job gains an install step for the binary it is _measuring_ (research R6); that is test infrastructure, not a product install.                                                                         |
| II. ObjectScript Sanity               | N/A    | No ObjectScript added or changed. The harness's existing Atelier calls are untouched.                                                                                                                                                         |
| III. HTTP-First Execution             | N/A    | No new tools.                                                                                                                                                                                                                                 |
| IV. Test-First, Fixture-Driven        | PASS   | Every behaviour change gets a pytest unit test written first (FR-022). Scorer failures are exercised with a stubbed scorer client — the no-mock rule is about IRIS, and IRIS is live in every path that touches it.                           |
| V. Output Shape Parity                | PASS   | The result JSON is a consumed contract (the merge step reads it). All three shapes are in `contracts/`, and `SkillResult` gains fields without renaming one.                                                                                  |
| VI. Environment Guard                 | N/A    | No IRIS write path added.                                                                                                                                                                                                                     |
| VII. Dependency Minimalism            | PASS   | No new package. Bedrock needs no boto3 with a bearer token — verified, research R1.                                                                                                                                                           |
| VIII. 90% Coverage Gate               | N/A    | `cargo llvm-cov` measures the Rust crates. This spec changes no Rust, so the number cannot move. The Python harness gets its own assertion instead: the new modules are covered by the unit target the nightly's `discover` job already runs. |
| IX. Tool Lift Requirement             | N/A    | No new MCP tool. This spec repairs the instrument that measures lift; it has no lift of its own.                                                                                                                                              |
| X. ObjectScript Coverage              | N/A    | Pure Python.                                                                                                                                                                                                                                  |
| XI. No Vacuous Tests                  | PASS   | Live-scorer and live-agent tests skip loudly on a missing credential and are named in the quickstart. No test asserts behind a silent `if`.                                                                                                   |
| XII. Hermetic Test Environment        | PASS   | The scorer client reads four env vars; the unit tests pin all four rather than inheriting them, which is what let the CI difference hide (research R1).                                                                                       |
| XIII. Single-Source Failure Detection | PASS   | One place decides a scoring failure (`score_result`), one place decides a run is invalid (the unscored-share check), one place decides comparability. No second copy of any of the three.                                                     |

Governance: prompt 9 (detector for the class) is satisfied by the unit tests named in the spec's
Bug Classes table plus one new scanner check, `scored-exception` (research R9). The Bug Class
Registry entries needed a constitution amendment; it is applied — `.specify/memory/constitution.md`
is at 1.5.3 with all six rows and `scored-exception` on the never-baselined list.

_No FAIL gates._

### Post-design re-check

Re-run after Phase 1. Every row above still holds. Three worth restating because the design moved
them:

- **VII (dependency minimalism)** got stronger, not weaker. R1 verified a live Bedrock call with a
  bearer token and no boto3, and R11 replaced the guessed cost constant with measured `msg.usage`
  instead of a token-counting library. Nothing new gets installed.
- **XIII (single-source failure detection)** is now checkable rather than asserted. The three
  decisions each live in exactly one named place: `score_result` (contracts/scoring.md), the
  unscored-share check (`run_valid`), and the provenance comparison (contracts/baseline-v2.md).
  Nothing else computes any of the three.
- **Governance prompt 9** is satisfied by a shipped artifact, not a staged one: the constitution is
  at 1.5.3 with six Bug Class Registry rows and `scored-exception` on the never-baselined list
  (eight classes to nine). Each row names the test that detects it, and each of those names appears
  on the task that builds it.

One thing the design surfaced that the pre-check did not name: `judge_model` keeps its key in the
shard JSON even though the value's meaning changes. That is a deliberate Principle V (output shape
parity) call — renaming it would drop the field silently in a mixed-version merge, which is the
#110 pattern. Recorded in contracts/eval-run.md rather than left to the implementer.

## Scope added after research

Research found a second cause of the six zero rows, independent of the scorer, and it changes what
the rebaseline is worth. Recording it here rather than burying it in `research.md`:

`tests/e2e/isolated_env.py:48` starts the MCP server with the hard-coded path
`/opt/homebrew/bin/iris-agentic-dev`. That path exists on the laptop and does not exist on an
`ubuntu-latest` runner, and the nightly never installs the binary. So in CI the MCP server never
starts and the agent has no iad tools at all.

The six skills that reported 0-vs-0 are exactly the six whose eval config leaves MCP on
(`iris-connectivity`, `objectscript-guardrails`, `objectscript-list-patterns`,
`objectscript-review`, `objectscript-sql-patterns`, `objectscript-unit-test`), and none of their
tasks carry `tool_assertions`, so all of them also go through the judge. The three skills that
reported non-zero numbers all set `no_mcp_for_benchmark: true` and are scored by the pattern
matcher. The two explanations predict the same six rows, which is why the spec's correlation
argument could not separate them. Both are real.

Two consequences for this spec:

- FR-007 asks the baseline to record "the revision of the tool surface the agent saw". Truthfully
  recording that requires resolving the binary, so the binary resolution is already in scope. The
  honest value in CI today is "none".
- FR-011 says every entry must be measured under the repaired scorer. An entry measured with no
  tools would satisfy that sentence and still be worthless — and it would be _stale on the next
  release_ under FR-008's advisory field. So the CI install lands in 118, before the rebaseline.

The fix is small: resolve the binary through `IAD_BINARY` → `PATH` → the homebrew path, and add an
install step to the eval job. It is Phase 2 work, T-numbered with the rest.

This is now FR-024 in spec.md, with its own row in the spec's Bug Classes table. The section stays
because the research that found it is what justifies the requirement's wording, but the requirement
is the requirement.

## Project Structure

### Documentation (this feature)

```text
specs/118-skill-eval-harness-repair/
├── spec.md
├── plan.md                     # this file
├── research.md                 # Phase 0
├── data-model.md               # Phase 1
├── quickstart.md               # Phase 1
├── contracts/
│   ├── scoring.md              # scorer return shape, exit codes
│   ├── baseline-v2.md          # baseline file schema + v1 migration
│   └── eval-run.md             # shard result and merged report JSON
├── constitution-amendment.md   # staged; constitution is edit-protected
├── checklists/requirements.md
└── tasks.md                    # /speckit.tasks
```

### Source Code (repository root)

```text
tests/e2e/skill_eval/
├── preflight.py           NEW  scorer probe + tool-surface probe, one exit path
├── provenance.py          NEW  tool-surface revision, run provenance record
├── scoring.py             NEW  ScoredItem, pass rate over scored items, unscored share
├── baseline.py            EDIT merge-by-skill, v2 schema, v1 read migration
├── evaluator.py           EDIT SkillResult gains outcome/provenance/item counts
├── lift.py                EDIT per-item scored flag, scoring mode, resolved model
├── cost_estimator.py      EDIT scorer cost re-derived from the resolved model
├── reporter.py            EDIT columns for mode, model, items, outcome
├── shard.py               EDIT merged unscored share, run_valid AND, re-run footer
├── __main__.py            EDIT preflight call site, exit-code policy, threshold recompute
├── test_preflight.py      NEW
├── test_provenance.py     NEW
├── test_scoring.py        NEW
├── test_baseline.py       EDIT
├── test_evaluator.py      EDIT
├── test_cost_estimator.py EDIT
├── test_reporter.py       EDIT
└── test_shard.py          EDIT

tests/e2e/isolated_env.py       EDIT binary path resolved, not hard-coded
tests/e2e/test_isolated_env.py  EDIT resolver assertion + non-zero tool count gate
benchmark/021/runner/judge.py     EDIT unscored verdict, resolved model, usage capture
benchmark/021/runner/_client.py   EDIT resolved-model and auth-source accessors
benchmark/021/tests/test_judge.py EDIT the api-error test currently asserts the bug
scripts/gates/antipatterns.py     EDIT new `scored-exception` check + canary
.github/workflows/skill-regression.yml EDIT Bedrock env, binary install, preflight step
```

**Structure Decision**: the harness keeps its flat module layout — one module per concern, tests
beside them, which is what the existing 25 files do. Three new modules rather than growing
`lift.py` past 400 lines: the scorer contract, the preflight, and the provenance record are each
consumed by more than one caller (`lift.py`, `__main__.py`, `baseline.py`), and `lift.py` already
mixes transcript formatting, fixture loading, and two scoring paths.

## Phasing

Ordered so each phase leaves the harness in a state worth having, and so the rebaseline happens
last — after both causes of the zeros are fixed.

| Phase | What                                                                      | Gate                                                                                              |
| ----- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| 1     | Setup: the `scored-exception` check and its canary, unregistered          | Canary fails without the check; the check finds the one real instance in `judge.py`               |
| 2     | Foundational: provenance record, tool surface, binary resolution (FR-024) | An isolated env starts with a non-zero tool count                                                 |
| 3     | US1 — scoring integrity + preflight + CI reachability (FR-001..005, 023)  | One skill runs green in CI with non-zero arms; a revoked credential stops the run before spending |
| 4     | US2 — baseline v2, merge, comparability, report (FR-006..010)             | A one-skill update leaves the other entries byte-identical                                        |
| 5     | Rebaseline all nine skills (FR-011)                                       | Nine entries, each with provenance, each measured post-repair                                     |
| 6     | Polish: docs, amendment handoff, full suite                               | `pytest tests/e2e/ benchmark/021/tests/` green; scanner green                                     |

Phase 5 is a workflow-dispatch run with `update_baseline: true`, not code. It costs about one
night's spend and it is the deliverable of Story 2.

Task count: 37, inside the 40 cap (constitution clarification prompt 3). See `tasks.md`.

## Complexity Tracking

No constitution violations to justify. One judgement call worth naming:

| Decision                                            | Why                                                                                  | Simpler alternative rejected because                                                                                                                        |
| --------------------------------------------------- | ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Baseline schema v2 with an envelope, not a flat map | Provenance has to live somewhere, and a reader has to know which shape it is holding | Adding provenance keys to the flat map leaves no way to tell a v1 entry from a v2 entry with fields missing, which is exactly the "not comparable" question |
| Preflight makes a real scoring call                 | The failure mode was a present-but-unusable credential; presence tests pass on that  | An env-var presence check is what the harness effectively had, and it is why nine shards spent $5.47 to print zeros                                         |
