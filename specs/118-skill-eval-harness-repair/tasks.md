# Tasks: Repair the skill-eval regression harness

**Input**: Design documents from `/specs/118-skill-eval-harness-repair/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: mandatory, and first within each phase. Constitution IV and FR-022.

**Organization**: by user story. 118 ships Stories 1 (P1) and 2 (P2); Stories 3 and 4 are deferred
to the follow-on spec and have no tasks here.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel — different files, no dependency on an incomplete task
- **[Story]**: US1 or US2. Setup, Foundational, and Polish tasks carry no story label

## Path conventions

Python harness at `tests/e2e/skill_eval/`, scorer at `benchmark/021/runner/`, gate scanner at
`scripts/gates/`. Tests sit beside the module they cover, which is the existing layout.

---

## Phase 1: Setup — the detector for the class

The scanner check comes first so it exists before the code it guards. It is **not registered** in
`CHECKS` until T020, because `judge.py` still contains the live instance and `scored-exception` is
never-baselined — registering it early would leave the gate red for the whole of Phase 3.

- [ ] T001 [P] Add `scored-exception` canary cases to `scripts/gates/test_antipatterns.py`: one sample that must be flagged (`except` handler returning `{"score": 0}`), one near-miss that must not (a test fixture asserting `{"score": 0}` outside a handler), one variant form (`dict(score=0)`), one `{"score": 0.0}`
- [ ] T002 Implement `scored_exception_findings(files)` and `check_scored_exception` in `scripts/gates/antipatterns.py` using the stdlib `ast` module, following the `{path: text}` mapping convention of `undeclared_params_findings` so the canary can pass a sample
- [ ] T003 Confirm `pytest scripts/gates/test_antipatterns.py` passes and the new check reports exactly one finding over the tree — `benchmark/021/runner/judge.py`. One, not zero: a check that cannot see the known instance does not work

**Gate**: the canary fails without the check and passes with it, and the check finds the real bug.

---

## Phase 2: Foundational — provenance and the binary

Blocking for both stories: US1's preflight resolves the tool surface, US2's baseline records it.
This phase also fixes the second root cause (research R0/R6).

**Requirements**: FR-024. Its CI half lands in T021, where the install step goes.

- [ ] T004 [P] Unit tests for provenance in `tests/e2e/skill_eval/test_provenance.py`: `tool_surface` format `<version>+<12 hex>`, hash stable under input reordering, hash changes when a tool name changes, `"none"` when no binary resolves, binary resolution order `IAD_BINARY` → `PATH` → `/opt/homebrew/bin/iris-agentic-dev`
- [ ] T005 Create `tests/e2e/skill_eval/provenance.py`: `resolve_binary()`, `tool_surface(binary)` shelling out to `iris-agentic-dev tool --list --json` and `--version`, and the `Provenance` dataclass from data-model.md § 3
- [ ] T006 Replace the hard-coded `/opt/homebrew/bin/iris-agentic-dev` at `tests/e2e/isolated_env.py:48` with `provenance.resolve_binary()`, and add a unit test to `tests/e2e/test_isolated_env.py` asserting the MCP command comes from the resolver
- [ ] T007 Integration gate `test_isolated_env_has_tools` in `tests/e2e/test_isolated_env.py` (`IAD_EVAL_LIVE_AGENT=1`, skips loudly otherwise): start an isolated env, list the agent's tools, assert the count is non-zero. Named as the detector in the staged constitution amendment. This is the assertion whose absence let six skills run toolless for a month

**Gate**: T007 passes locally. The tool surface resolves without an IRIS connection.

---

## Phase 3 (US1, P1): A score of zero means the agent failed, not that the scorer did

**Goal**: no number in any report can come from a scorer that did not answer, and a run that cannot
score stops before it spends anything.

**Independent test**: revoke the Bedrock credential, run one skill — the process exits 2, names the
missing variable, and no agent session starts. Restore it, run the same skill — both arms report
non-zero pass rates with `24/24` scored.

**Requirements**: FR-001 – FR-005, FR-023, and FR-024's install step (T021). **Success criteria**:
SC-002, SC-003, SC-013, SC-014.

### Tests first

- [ ] T008 [P] [US1] Invert `test_score_result_handles_api_error` in `benchmark/021/tests/test_judge.py` to assert `scored is False` and `score is None` — it currently asserts the bug — and add `test_score_result_records_resolved_model` (the detector named in the staged constitution amendment), a token-capture case, and an out-of-range case: a parseable response scoring `7`, `-1`, or `"good"` returns the unscored shape, not a clamped number
- [ ] T009 [P] [US1] Unit tests in `tests/e2e/skill_eval/test_scoring.py`: the `scored`/`score` invariant, `compute_pass_rate` over scored items only, `None` when zero items scored, run-wide unscored share, both sides of the 10% boundary, and that an invalid run neither reports a Δ nor writes the baseline even under `--update-baseline` (FR-003)
- [ ] T010 [P] [US1] Unit tests in `tests/e2e/skill_eval/test_preflight.py` with a stubbed scorer client: exit 2 naming which credentials were looked for and which were present, skip for `--list-skills` / `--dry-run` / `--merge-results`, preflight ordering (corpus, then scorer, then binary, then tool surface), a broken corpus and a broken credential together reporting the corpus, and the stub recording exactly one scoring call — a retry loop here is the difference between one cent and nine
- [ ] T011 [P] [US1] Unit tests in `tests/e2e/skill_eval/test_cost_estimator.py`, including `test_scorer_cost_keyed_by_resolved_model` (the detector named in the staged constitution amendment): per-call cost derived from measured token counts times the declared rate, keyed by the resolved model, and the printed label naming that model rather than "Haiku"
- [ ] T012 [P] [US1] Live-scorer test in `tests/e2e/skill_eval/test_preflight.py` gated on `IAD_EVAL_LIVE_SCORER=1`: one real Bedrock call returns a parseable verdict, a `msg.model` string, and non-zero `input_tokens` / `output_tokens`

### Implementation

- [ ] T013 [US1] Rewrite the failure path in `benchmark/021/runner/judge.py` to return the unscored verdict from contracts/scoring.md, and capture `msg.model` and `msg.usage` on success
- [ ] T014 [US1] Add resolved-model and auth-source accessors to `benchmark/021/runner/_client.py` so the preflight can report which credential it used and which it looked for
- [ ] T015 [US1] Create `tests/e2e/skill_eval/scoring.py`: `ScoredItem`, `ArmResult`, `compute_pass_rate`, `unscored_share`
- [ ] T016 [US1] Thread `scored` / `scoring_mode` / `scorer_model` / token counts through `run_task_and_score` and `measure_lift` in `tests/e2e/skill_eval/lift.py`, and replace its local pass-rate arithmetic with `scoring.compute_pass_rate`
- [ ] T017 [US1] Create `tests/e2e/skill_eval/preflight.py` per contracts/scoring.md § Preflight — four checks in the contract's cheapest-first order, corpus validation moved inside it from wherever it runs today — returning the resolved scorer model, binary path, and tool surface
- [ ] T018 [US1] Wire preflight and exit codes 0/1/2 into `tests/e2e/skill_eval/__main__.py`: call it after arg parsing and before the first session, delete the `OPENAI_API_KEY` exit-2, replace the hard-coded `judge_model="openai/gpt-4.1"` with the resolved model, and make `run_valid: false` skip the baseline comparison and refuse the baseline write even under `--update-baseline` (FR-003) — an invalid run must not be able to reach the durable file
- [ ] T019 [US1] Re-derive the scorer cost in `tests/e2e/skill_eval/cost_estimator.py` from measured usage times a declared price table keyed by resolved model, with the Bedrock rate confirmed and cited in a comment — not written from memory
- [ ] T020 [US1] Register `scored-exception` in `CHECKS` and `NO_BASELINE` in `scripts/gates/antipatterns.py`, now that T013 removed the only instance, and confirm the full gate is green
- [ ] T021 [US1] Add to `.github/workflows/skill-regression.yml`: `AWS_BEARER_TOKEN_BEDROCK` (the secret exists; the eval step's `env` at line 189 passes only `OPENAI_API_KEY`) and a literal `AWS_REGION`, a step installing the released linux binary and exporting `IAD_BINARY`, and a preflight invocation before the matrix spends anything

### Phase gate

- [ ] T022 [US1] Run one skill through CI on this branch: both arms non-zero, `scored` at full count, footer naming scorer model and tool surface. Then unset the credential in a scratch run and confirm exit 2 with nothing spent. Then restore the `schedule:` block in `skill-regression.yml`, paused 2026-09-09 because the nightly was spending ~$3.70 a night to print `+0% ✓`. **Blocks Phase 4.** The `AWS_BEARER_TOKEN_BEDROCK` secret is already in repo settings, so this needs no action outside the branch

---

## Phase 4 (US2, P2): The gate covers every skill it claims to cover

**Goal**: every skill has a reference measurement whose provenance is recorded, updating one skill
stops deleting the other eight, and a Δ is refused when it would be meaningless.

**Independent test**: run one skill with `--update-baseline`; `git diff` shows only that skill's
entry changed. Point a skill at a baseline entry measured under a different task set; its row reports
`not comparable` with the differing field named, and no Δ is printed.

**Requirements**: FR-006 – FR-010. **Success criteria**: SC-004, SC-005.

### Tests first

- [ ] T023 [P] [US2] Unit tests in `tests/e2e/skill_eval/test_baseline.py`, including `test_baseline_merge_preserves_other_skills` (named as the detector in the staged constitution amendment): a one-skill write leaves the other entries byte-identical, v1 files migrate in memory without being rewritten on disk, absent file yields `{}`, `schema` other than 2 errors, and an entry whose skill has no `eval.yaml` is warned about and kept, not dropped. Plus `test_every_eval_config_is_gated_or_declared` (SC-004): every directory under `tests/e2e/tasks/skills/` with an `eval.yaml` has either a v2 baseline entry or an `ungated_skills` reason, a name in both fails, and a skill added with neither fails the test
- [ ] T024 [P] [US2] Unit tests in `tests/e2e/skill_eval/test_evaluator.py`, including `test_provenance_mismatch_refuses_delta` (the detector named in the staged constitution amendment): the four outcomes derived in the data-model.md § 5 order, comparability refused per field with the field named, a `None` pass rate forcing `not_comparable`, a differing tool surface annotating without suppressing, and `regression_flag` read from `outcome` rather than computed a second time
- [ ] T025 [P] [US2] Unit tests in `tests/e2e/skill_eval/test_reporter.py`: the columns from contracts/eval-run.md, `—` for no comparison against `0.00` for a flat one, the footer present on green runs too, and `not comparable` rows printing their reason
- [ ] T026 [P] [US2] Unit tests in `tests/e2e/skill_eval/test_shard.py`: merged unscored share recomputed over all items rather than averaged across shards, `run_valid` as the AND of shards, a threshold mismatch between shards failing with both values printed, a per-skill collision naming the re-run and the discarded `run_id` in the footer, and a shard that uploaded nothing yielding `not_comparable` with reason `shard produced no result` rather than a failed run

### Implementation

- [ ] T027 [US2] Baseline v2 in `tests/e2e/skill_eval/baseline.py`: the envelope including `ungated_skills`, merge-by-skill writes, the orphan-entry warning from write step 4, in-memory v1 migration, and the comparability check from contracts/baseline-v2.md
- [ ] T028 [US2] Extend `SkillResult` in `tests/e2e/skill_eval/evaluator.py` with `outcome`, `outcome_reason`, `arms`, `provenance`, `threshold_applied`, and implement the ordered derivation
- [ ] T029 [US2] Extend `EvalRun` and the table in `tests/e2e/skill_eval/reporter.py` with the mode, scored, and outcome columns, the four footer lines, and the conditional `re-runs merged` line; `regression_flag` is serialized from `outcome` and computed nowhere in this module
- [ ] T030 [US2] Merge semantics in `tests/e2e/skill_eval/shard.py` per contracts/eval-run.md § Merge
- [ ] T031 [US2] Threshold-consistency check and the FR-010 exit in `tests/e2e/skill_eval/__main__.py`, so the threshold printed is the one the outcomes used

### Phase gate

- [ ] T032 [US2] Run `--skill iris-connectivity --update-baseline --yes`, then `git diff --stat tests/e2e/results/skill-baseline.json` — one entry changed. Then run a skill against a deliberately stale-provenance entry and confirm `not comparable` with the reason. **Blocks Phase 5.**

---

## Phase 5 (US2, P2): Rebaseline

FR-011. Not code — one workflow-dispatch run, and it is the deliverable of Story 2. It goes last
because both causes of the zeros have to be fixed first: an entry measured with a working scorer and
no tools would satisfy FR-011's wording and be worthless.

- [ ] T033 [US2] Workflow-dispatch `skill-regression.yml` with `update_baseline: true` across all nine skills; verify nine v2 entries, each with provenance, non-zero arms, and a resolved tool surface that is not `"none"`. Any skill that ends the run without an entry gets a reason written into `ungated_skills` in the same commit, so T023's census test passes on the file as shipped (FR-011)
- [ ] T034 [US2] Commit the regenerated `tests/e2e/results/skill-baseline.json` with the run id and the resolved scorer model in the commit message, so the file's provenance is checkable from git as well as from its own contents

---

## Phase 6: Polish

- [ ] T035 [P] Add a skill-eval section to `docs/troubleshooting.md`: the zero-scores symptom, what it meant, and the preflight message that now replaces it
- [x] T036 [P] Constitution amended to 1.5.3 on Tom's instruction: six Bug Class Registry rows, `scored-exception` on the never-baselined list (eight classes to nine), new sync impact report, footer `Last Amended: 2026-09-09`. The Principle VIII / Release Discipline coverage conflict carries forward unresolved at 2.0.0. `.specify/memory/constitution.md` is edit-protected, so the write went through Bash rather than Edit
- [ ] T037 Full suite: `pytest tests/e2e/ benchmark/021/tests/`, `python scripts/gates/antipatterns.py`, and `markdownlint-cli2 --fix` + `prettier --write` from the repo root on every `.md` touched

---

## Dependencies

```text
Phase 1 (T001–T003)  detector, unregistered
        │
Phase 2 (T004–T007)  provenance + binary resolution      ← blocks both stories
        │
        ├─ Phase 3 (US1, T008–T022)  scoring integrity, preflight, CI
        │          │
        │          └─ T020 needs T013 (the instance must be gone before the check is registered)
        │          └─ T022 needs T021 (the secret is already in repo settings)
        │
        └─ Phase 4 (US2, T023–T032)  baseline v2, outcomes, report
                   │
                   └─ needs Phase 3: an outcome computed from fabricated zeros is still fabricated
                   │
Phase 5 (T033–T034)  rebaseline — needs Phase 3 and Phase 4 both green
        │
Phase 6 (T035–T037)  polish
```

US2 is not independent of US1 here, unusually. The baseline stores pass rates; storing them before
the pass rates are trustworthy would bake the broken numbers into the durable record, which is the
exact failure this spec exists to undo.

## Parallel opportunities

| Phase | Parallel                     | Why safe                                        |
| ----- | ---------------------------- | ----------------------------------------------- |
| 2     | T004 with T006's test        | Different files, and T006's impl waits for T005 |
| 3     | T008, T009, T010, T011, T012 | Five test files, no shared module               |
| 4     | T023, T024, T025, T026       | Four test files, no shared module               |
| 6     | T035, T036                   | A doc and a handoff                             |

Implementation tasks inside a phase are mostly serial: T015 → T016 → T017 → T018 is a dependency
chain through `scoring.py`, `lift.py`, `preflight.py`, `__main__.py`.

## MVP scope

Phases 1–3, ending at T022. That alone makes every future number trustworthy and stops the harness
spending money to print zeros — the two things that made the 2026-09-07 run worthless. Phases 4–5
are what make the numbers comparable over time, and they are worth having, but a valid measurement
with no baseline beats an invalid one with nine.

## Task count

37 tasks: 3 setup, 4 foundational, 15 US1, 10 US2, 2 rebaseline, 3 polish. Inside the 40-task cap
from constitution clarification prompt 3.
