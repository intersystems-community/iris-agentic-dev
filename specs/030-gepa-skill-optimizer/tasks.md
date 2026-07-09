---

description: "Task list for 030-gepa-skill-optimizer"

---

# Tasks: GEPA-Inspired Skill Optimization Loop

**Input**: Design documents from `/specs/030-gepa-skill-optimizer/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: MANDATORY for every user-story phase per project convention (constitution
Principle IV) — unit tests first, then a live `#[ignore]`-gated E2E test as the phase gate.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: US1 / US2 / US3 per spec.md priorities

## Path Conventions

Single Rust workspace. New module: `crates/iris-agentic-dev-core/src/optimizer/`. Tests:
`crates/iris-agentic-dev-core/tests/` (flat files, each registered as its own `[[test]]`
in `Cargo.toml`, matching the existing convention — see 059's
`test_telemetry_types`/`test_benchmark_scoring` entries).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Module skeletons shared by every user story.

- [ ] T001 [P] Create `crates/iris-agentic-dev-core/src/optimizer/mod.rs`,
      `optimizer/analyze.rs`, `optimizer/mutate.rs`, `optimizer/lock.rs` (empty `pub`
      stubs — no logic yet), and register `pub mod optimizer;` in
      `crates/iris-agentic-dev-core/src/lib.rs`.
- [ ] T002 [P] Confirm (no code change expected, verification-only) that
      `benchmark::run_suite`, `benchmark::load_embedded_tasks`,
      `benchmark::acquire_lock`/`release_lock`/`decide_lock`, and
      `telemetry::read_durable`/`filter_records` are all already `pub` (per research.md's
      "reuse as-is" decisions) — if any is not `pub`, widen its visibility as a
      same-commit, one-line change (no behavior change).

**Checkpoint**: Module compiles (empty).

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types every user story's tests depend on.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [ ] T003 [P] Define `FailurePattern` and its nested `TelemetrySummary` struct (per
      data-model.md) in `crates/iris-agentic-dev-core/src/optimizer/mod.rs`, deriving
      `Debug, Clone, Serialize, Deserialize`.
- [ ] T004 [P] Define `SkillCandidate` struct (per data-model.md) in
      `crates/iris-agentic-dev-core/src/optimizer/mod.rs`.
- [ ] T005 [P] Define `OptimizationRound` struct (per data-model.md) in
      `crates/iris-agentic-dev-core/src/optimizer/mod.rs`.
- [ ] T006 [P] Define `RegressionLockSet` and `LockHistoryEntry` structs (per
      data-model.md) in `crates/iris-agentic-dev-core/src/optimizer/lock.rs`.
- [ ] T007 [P] [Unit test] Write
      `crates/iris-agentic-dev-core/tests/unit/test_optimizer_types.rs` covering:
      `FailurePattern`/`SkillCandidate`/`OptimizationRound`/`RegressionLockSet` all
      serialize/deserialize round-trip via `serde_json`. Register as
      `[[test]] name = "test_optimizer_types"` in
      `crates/iris-agentic-dev-core/Cargo.toml`. Confirm FAILS (types don't exist yet)
      before T003–T006 land, then confirm PASSES after.
- [ ] T008 Implement `optimizer::lock::load(skill_dir: &Path) -> anyhow::Result<RegressionLockSet>`
      and `optimizer::lock::save(skill_dir: &Path, set: &RegressionLockSet) -> anyhow::Result<()>`
      in `crates/iris-agentic-dev-core/src/optimizer/lock.rs`, per research.md's "JSON
      sidecar, not IRIS global" decision — reading a missing
      `<skill_dir>/.optimizer-lock.json` returns an empty `RegressionLockSet`, not an
      error. Depends on T006.
- [ ] T009 [P] [Unit test] Write
      `crates/iris-agentic-dev-core/tests/unit/test_optimizer_lock.rs` covering: `load`
      on a missing file returns an empty set; `save` then `load` round-trips; `locked_task_ids`
      never contains duplicates after `record_applied_round` is called twice with an
      overlapping task id. Register in Cargo.toml. Depends on T008 (write test alongside,
      confirm fails against `todo!()` stubs first).
- [ ] T010 Implement `optimizer::lock::record_applied_round(set: &mut RegressionLockSet, newly_fixed_task_ids: Vec<String>, held_out_pass_rate: f64, applied_at: String)`
      in `crates/iris-agentic-dev-core/src/optimizer/lock.rs` per data-model.md's
      "grows only, no duplicates" validation rule. Depends on T006.

**Checkpoint**: Foundation ready — types defined, lock-set persistence works; user story
implementation can now begin.

---

## Phase 3: User Story 1 — Diagnose Failures Into Structured Patterns (Priority: P1)

**Goal**: `optimizer::analyze::diagnose_failures` turns a benchmark run's non-passing
`TaskResult`s plus their session's telemetry into `FailurePattern`s, batched.

**Independent Test**: Per quickstart.md — run the benchmark harness for a skill with a
known-failing task, call `diagnose_failures` directly with that run's data, verify one
`FailurePattern` per non-passing task.

### Tests for User Story 1 ⚠️

- [ ] T011 [P] [US1] Write
      `crates/iris-agentic-dev-core/tests/unit/test_optimizer_analyze_batching.rs`: a pure
      helper `batch_tasks(failing: &[TaskResult], batch_size: usize) -> Vec<Vec<&TaskResult>>`
      splits into chunks of at most `batch_size`, preserves order, and returns an empty
      `Vec` for an empty input (spec FR-002, Acceptance Scenario 2/3). Register in
      Cargo.toml. Confirm FAILS before T013.
- [ ] T012 [US1] [E2E — phase gate] Write
      `crates/iris-agentic-dev-core/tests/integration/test_optimizer_analyze_live.rs`,
      `#[ignore]`, following the `live_iris()` helper pattern used by
      `test_benchmark_live.rs`: run `benchmark::run_suite` for a skill known to fail at
      least one task, call `diagnose_failures` with that run's failing `TaskResult`s and
      the matching session's telemetry (via `telemetry::read_durable`/`filter_records`),
      assert one `FailurePattern` per non-passing task with a non-empty `summary`. Also
      assert a zero-failure `BenchmarkResult` produces an empty pattern list with zero
      LLM calls made (Acceptance Scenario 3 — verify via a call-count instrumentation
      point or by asserting no `ANTHROPIC_API_KEY`-consuming call occurs when the input is
      empty, e.g. by running this sub-case without an API key configured and confirming
      no `LLM_NOT_CONFIGURED`-shaped failure occurs since no call was attempted). MUST
      FAIL until T013–T014 land; MUST PASS before Phase 3 is complete.

### Implementation for User Story 1

- [ ] T013 [US1] Implement `batch_tasks` (the pure function from T011) in
      `crates/iris-agentic-dev-core/src/optimizer/analyze.rs`. Depends on T011.
- [ ] T014 [US1] Implement `optimizer::analyze::diagnose_failures(llm: &LlmClient, failing: &[TaskResult], telemetry: &[ToolCallRecord], batch_size: usize) -> anyhow::Result<Vec<FailurePattern>>`
      in `crates/iris-agentic-dev-core/src/optimizer/analyze.rs` per
      contracts/optimizer-internal-api.md: early-return `Ok(vec![])` for empty `failing`
      (no LLM call); otherwise call `batch_tasks`, and for each batch build a prompt
      (task description/goal/expected_behavior/reason plus the matching
      `TelemetrySummary` entries filtered from `telemetry` by task's session) and call
      `llm.complete(system, user)`, parsing the response into one `FailurePattern` per
      task in the batch. Depends on T003, T013.

**Checkpoint**: User Story 1 fully functional — `diagnose_failures` produces batched,
telemetry-informed failure patterns; `test_optimizer_analyze_live.rs` passes.

---

## Phase 4: User Story 2 — Propose and Validate a Skill Candidate (Priority: P1) 🎯 MVP

**Goal**: `skill_optimize` (replacing its `NOT_IMPLEMENTED` stub) runs the full
propose/score loop and, under `apply: true`, writes the winning candidate — with the
two-step gate from research.md enforced.

**Independent Test**: Per quickstart.md — call `skill_optimize` with `apply: false`
against an underperforming skill, verify a scored candidate and an unchanged skill file;
follow with `apply: true`, verify the file is overwritten.

### Tests for User Story 2 ⚠️

- [ ] T015 [P] [US2] Write
      `crates/iris-agentic-dev-core/tests/unit/test_optimizer_selection.rs`: a pure
      function `select_winner(baseline_pass_rate: f64, candidates: &[SkillCandidate], threshold: f64) -> Option<usize>`
      returns the index of the best-scoring candidate with `passes_all_locked: true` and
      `held_out_pass_rate > baseline_pass_rate + threshold`, or `None` when no candidate
      qualifies — covering: all candidates fail a locked task (→ `None`, distinct from
      "no improvement" per spec Acceptance Scenario 3/US3); a tie between two qualifying
      candidates picks the first by index (documented, deterministic tie-break). Register
      in Cargo.toml. Confirm FAILS before T019.
- [ ] T016 [US2] [E2E — phase gate] Write
      `crates/iris-agentic-dev-core/tests/integration/test_optimizer_live.rs`, `#[ignore]`,
      per `live_iris()` pattern, requiring a real LLM key: call the `skill_optimize`
      handler (via `IrisTools::call_for_test`, matching
      `test_dispatch_skill_optimize_not_implemented`'s existing pattern in
      `test_handlers_live.rs` — this test REPLACES that one, since the stub behavior it
      asserted no longer exists) with `apply: false` against a skill with a known-failing
      task; assert a non-empty `candidates` list, a `held_out_pass_rate` per candidate,
      and that the skill's `SKILL.md` file on disk is byte-identical before and after the
      call (spec SC-002). Then call again with `apply: true`; assert the file now matches
      the winning candidate's text and `newly_locked_task_ids` is non-empty. MUST FAIL
      until T017–T021 land; MUST PASS before Phase 4 is complete.
- [ ] T017 [P] [US2] Update the existing
      `test_dispatch_skill_optimize_not_implemented` test in
      `crates/iris-agentic-dev-core/tests/integration/test_handlers_live.rs` — remove it
      (superseded by T016) or repurpose it to assert the new real behavior's shape,
      per whichever this feature's implementer judges clearer; do not leave a stale
      "asserts NOT_IMPLEMENTED" test passing against a now-implemented tool.

### Implementation for User Story 2

- [ ] T018 [US2] Implement `optimizer::mutate::generate_candidates(llm: &LlmClient, current_skill_text: &str, patterns: &[FailurePattern], candidate_count: usize) -> anyhow::Result<Vec<String>>`
      in `crates/iris-agentic-dev-core/src/optimizer/mutate.rs` per
      contracts/optimizer-internal-api.md — one `llm.complete` call per candidate, fixed
      system prompt instructing preservation of passing-task guidance. Depends on T004.
- [ ] T019 [US2] Implement `optimizer::mod::score_candidate` and `optimizer::mod::run_optimize`
      in `crates/iris-agentic-dev-core/src/optimizer/mod.rs` per
      contracts/optimizer-internal-api.md: baseline `run_suite` → split failing/held-out →
      `diagnose_failures` (T014) → `generate_candidates` (T018) → `score_candidate` per
      candidate (each `run_suite` call wrapped in `acquire_lock`/`release_lock`, reusing
      059's lock per research.md) → `select_winner` (T015). Returns an `OptimizationRound`
      with `applied: false`. Depends on T014, T015, T018.
- [ ] T020 [US2] Add `apply: bool` (default `false`), `candidate_count`, `batch_size`,
      `improvement_threshold` fields to a new `SkillOptimizeParams` struct in
      `crates/iris-agentic-dev-core/src/tools/mod.rs`, replacing the current
      `SkillNameParams`-typed `skill_optimize` signature (keep `name: String` as the
      first field for input-shape continuity). Depends on T019.
- [ ] T021 [US2] Replace the `skill_optimize` handler's body in
      `crates/iris-agentic-dev-core/src/tools/mod.rs` (currently returns
      `NOT_IMPLEMENTED`): when `apply` is `false`/omitted, call `run_optimize` (T019) and
      return its `OptimizationRound` per contracts/skill-optimize-tool.md's propose-only
      shape, storing it in a new process-local `last_proposal: Mutex<HashMap<String, OptimizationRound>>`
      field on `IrisTools` (mirroring `history`'s existing per-process scope, per
      research.md). When `apply` is `true`: look up `last_proposal` for `name`; if absent
      or its `winner` is `None`, return `NO_PENDING_PROPOSAL`; otherwise write the winning
      candidate's text to the skill's `SKILL.md` (resolve `skill_dir` the same way
      `skill_community_install`/existing skill-loading code already resolves skill
      directories — no new path-resolution convention), call
      `optimizer::lock::record_applied_round` (T010) and `save` (T008), and return the
      apply-shape response. Also handle the zero-failure case (`NOTHING_TO_OPTIMIZE`,
      FR-011) and the two new error codes (`LLM_NOT_CONFIGURED` when
      `LlmClient::from_env()` is `None`, checked before any benchmark run starts).
      Depends on T020.
- [ ] T022 [US2] Remove `"skill_optimize"` from the `stub_tools`/`stubs_to_remove` list in
      `crates/iris-agentic-dev-core/src/tools/mod.rs`'s `with_registry_and_toolset`
      (around the existing `Toolset::Nostub | Toolset::Merged` stub-removal block) — this
      tool is no longer a stub and must remain registered under the default `Merged`
      toolset (per the Toolset Registration Rules requiring
      `registered_tool_names()`/removal-list sync, and per 059's own precedent of
      un-stubbing `telemetry_query`/`telemetry_export_trace` by never adding them to the
      stub list in the first place). Update
      `crates/iris-agentic-dev-core/tests/unit/test_toolset.rs`'s
      `["skill_propose", "skill_optimize", "skill_share"]` stub-removal assertion list to
      drop `"skill_optimize"`, and its comment counting stub removals ("4 stubs removed")
      to reflect 3. Depends on T021.

**Checkpoint**: User Story 2 fully functional — `skill_optimize` proposes, scores, and
(on a follow-up `apply: true` call) applies a winning candidate; `test_optimizer_live.rs`
passes; `skill_optimize` is available under the default `Merged` toolset.

---

## Phase 5: User Story 3 — Lock a Fix So It Can't Regress (Priority: P2)

**Goal**: Every candidate is checked against the full regression-lock set before it can
win, and applying a candidate grows that set.

**Independent Test**: Per quickstart.md — apply a winning candidate, force a
deliberately-regressing candidate into a later round, verify it's excluded from winner
selection despite a good held-out score.

### Tests for User Story 3 ⚠️

- [ ] T023 [P] [US3] Write
      `crates/iris-agentic-dev-core/tests/unit/test_optimizer_lock_exclusion.rs`:
      `select_winner` (T015, extended) excludes a candidate with `passes_all_locked: false`
      from winning even when its `held_out_pass_rate` is the highest among all candidates
      (spec FR-006, Acceptance Scenario 2 of US3) — this is the same function T015 tests,
      exercised here specifically against the lock-exclusion branch, kept as a separate
      file for US3 traceability per the tasks-format convention (`[Story]` labeling).
      Register in Cargo.toml. Confirm FAILS before T024 (if not already satisfied by
      T015's implementation — write this test first regardless to pin the behavior
      explicitly for US3).
- [ ] T024 [US3] [E2E — phase gate] Write
      `crates/iris-agentic-dev-core/tests/integration/test_optimizer_lock_live.rs`,
      `#[ignore]`: apply a winning candidate for a test skill (locking the task it fixed),
      then run `run_optimize` again with a candidate pool that includes one candidate
      known to regress the now-locked task (constructed via a fixture skill text that
      deliberately omits the guidance the locked task needs); assert `score_candidate`
      reports `passes_all_locked: false` for that candidate and `select_winner` never
      returns its index. MUST FAIL until T025–T026 land; MUST PASS before Phase 5 is
      complete.

### Implementation for User Story 3

- [ ] T025 [US3] Wire `optimizer::mod::score_candidate` (T019) to call `run_suite` a
      second time against the skill's current `RegressionLockSet.locked_task_ids`
      (resolved to `BenchmarkTask`s via `all_tasks`), populating
      `SkillCandidate.locked_task_results`/`passes_all_locked` per data-model.md — this
      task fills in the "kept separate from held-out scoring" detail research.md's
      `score_candidate` decision specified but T019 may have left as a placeholder.
      Depends on T019, T008.
- [ ] T026 [US3] Confirm (extend if needed) `select_winner` (T015) already excludes
      `passes_all_locked: false` candidates — if T015's initial implementation didn't yet
      cover this branch, add it now and re-run T023. Depends on T015, T023.

**Checkpoint**: All three user stories independently functional — diagnosis, propose/
apply, and regression-lock enforcement.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Repo-wide correctness and the mandatory coverage gate.

- [ ] T027 [P] Update `light-skills/BENCHMARKING.md` (or add a new short section) noting
      that `skill_optimize` now automates the "propose a skill improvement" step this doc
      otherwise describes as a manual process — one sentence, linking to
      `specs/030-gepa-skill-optimizer/quickstart.md`, not a rewrite of that doc.
- [ ] T028 [P] Run `cargo fmt --all -- --check` — no formatting diff.
- [ ] T029 [P] Run `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings.
- [ ] T030 Execute every command block in
      `specs/030-gepa-skill-optimizer/quickstart.md` verbatim against a fresh
      `iris-bench`-named container with a real LLM key configured, confirm each step
      succeeds as documented.
- [ ] T031 **Coverage gate** (Constitution VIII — NON-NEGOTIABLE): run
      `IRIS_HOST=localhost IRIS_PORT=52780 cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored`
      and confirm this feature's own new modules (`optimizer/mod.rs`, `optimizer/analyze.rs`,
      `optimizer/mutate.rs`, `optimizer/lock.rs`) individually clear 90% line coverage,
      backed by both unit tests (T007, T009, T011, T015, T023) and live integration tests
      (T012, T016, T024). If the crate-wide TOTAL is below 90% for reasons outside this
      feature's own files (per 059's precedent of documenting, not silently absorbing,
      pre-existing gaps), document the gap the same way 059's T041 did rather than
      backfilling unrelated modules.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately.
- **Foundational (Phase 2)**: Depends on Setup — BLOCKS all user stories.
- **User Story 1 (Phase 3)**: Depends on Foundational. No dependency on US2/US3.
- **User Story 2 (Phase 4)**: Depends on Foundational AND on `diagnose_failures` (T014,
  landed in US1/Phase 3) — a real dependency, since `run_optimize` calls it directly. US2
  cannot be fully implemented before Phase 3's T014 exists, even though US1 is
  independently valuable on its own.
- **User Story 3 (Phase 5)**: Depends on Foundational (lock.rs, Phase 2) AND on
  `score_candidate`/`select_winner` (T019/T015, landed in US2/Phase 4) — locking is
  additive on top of the propose/score loop US2 establishes.
- **Polish (Phase 6)**: Depends on all three user stories being complete.

### Within Each User Story

- Unit tests before implementation (written first, confirmed failing).
- E2E `#[ignore]` test is the phase gate — must pass before the phase is marked done.
- Data/pure-logic functions before the MCP tool handler that calls them.

### Parallel Opportunities

- T001, T002 (Setup) — different files/verification-only.
- T003, T004, T005, T006 (Foundational types) — different structs, same file
  (`optimizer/mod.rs` for T003–T005) — coordinate within-file, but no cross-file
  dependency; T006 is a separate file (`lock.rs`), fully parallel.
- T011 (US1 test) — parallel with any Foundational task not touching `analyze.rs`.
- T015 (US2 test) — parallel with T018 (different files: `test_optimizer_selection.rs`
  vs. `mutate.rs`).
- T023 (US3 test) — parallel with any US2 task not touching `select_winner`'s exclusion
  branch.
- T028, T029 (Polish) — independent checks, run in parallel with each other.

---

## Implementation Strategy

### MVP First (User Story 1 + User Story 2 Only)

1. Complete Phase 1: Setup.
2. Complete Phase 2: Foundational.
3. Complete Phase 3: User Story 1 — diagnosis works standalone.
4. Complete Phase 4: User Story 2 — `skill_optimize` is real, propose/apply works.
5. **STOP and VALIDATE**: run `test_optimizer_live.rs` and the quickstart.md commands
   manually against `iris-dev-iris` with a real LLM key.
6. This alone ships the headline capability — regression-locking (US3) hardens it against
   thrash but US1+US2 is a legitimate, reviewable ship point if time-constrained, exactly
   as 059's own MVP-first note observed for its own US1.

### Incremental Delivery

1. Setup + Foundational → types and lock-set persistence exist, nothing callable yet.
2. Add User Story 1 → `diagnose_failures` callable and tested standalone.
3. Add User Story 2 → `skill_optimize` fully replaces its stub — **this alone delivers
   the feature's headline promise**.
4. Add User Story 3 → repeated rounds become safe to run unattended without regressing
   prior fixes.

### Parallel Team Strategy

1. Team completes Setup + Foundational together (Phase 2 touches shared files —
   `optimizer/mod.rs` — genuinely blocking for T003–T005, not parallelizable across
   people without coordination on that one file).
2. Once Foundational is done:
   - Developer A: User Story 1 (`analyze.rs`).
   - Developer B: User Story 2 (`mutate.rs` + `tools/mod.rs` handler) — note this
     developer needs T014 from Developer A before `run_optimize` (T019) can be finished,
     so land T014 early.
   - Developer C: User Story 3 (lock-exclusion wiring) — blocked on T019/T015 from
     Developer B's work.
3. Story 1 is independent; Story 2 waits on one function (T014) from Story 1; Story 3
   waits on two functions (T019, T015) from Story 2 — documented above rather than hidden.

---

## Notes

- [P] tasks = different files, no dependencies.
- [Story] label maps task to specific user story for traceability.
- US2 has one genuine cross-story dependency (T014, `diagnose_failures`) on US1; US3 has
  two (T019, T015) on US2 — both documented above rather than hidden, matching 059's own
  precedent for its one cross-story dependency (T027 on T027-from-US2... i.e. 059's own
  US3→US2 dependency via `read_durable`).
- Verify each unit test FAILS before implementing the code it tests; verify each E2E
  `#[ignore]` test FAILS before its phase's implementation tasks, and PASSES before the
  phase is marked complete (phase gate — no exceptions).
- Commit after each task or logical group.
- Avoid: vague tasks, same-file conflicts, cross-story dependencies that break independence
  (the three exceptions above are called out explicitly rather than hidden).
