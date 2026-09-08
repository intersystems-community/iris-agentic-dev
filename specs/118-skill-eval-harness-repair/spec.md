# Feature Specification: Repair the skill-eval regression harness

**Feature Branch**: `118-skill-eval-harness-repair`
**Created**: 2026-09-08
**Status**: Draft
**Input**: User description: "Repair the skill-eval regression harness: the nightly Skill Regression gate produced its first end-to-end sharded run on 2026-09-07 and the result showed the gate is measuring almost nothing. Fix the stale baseline, the single-skill gate, the seven all-zero task sets, and the statistical power, without raising the nightly bill."

## User Scenarios & Testing _(mandatory)_

The nightly Skill Regression workflow finally completed end to end on 2026-09-07 (run
34163867198 on `master`). The five scheduled runs before it were all cancelled at the
75-minute job timeout on the pre-shard workflow, so this is the first night I have a full
nine-skill table to read. Nine shards finished in about 22 minutes at roughly $0.62 per shard.

The table says the gate is not doing its job:

| Skill                      | Fire% | Impl% | Base | Skill | Lift | Δ vs baseline | Status     |
| -------------------------- | ----- | ----- | ---- | ----- | ---- | ------------- | ---------- |
| iris-ai-hub                | 100%  | n/a   | 30%  | 43%   | +13% | n/a           | ok         |
| objectscript-review        | 100%  | 0%    | 0%   | 0%    | +0%  | n/a           | ok         |
| objectscript-guardrails    | 100%  | 0%    | 0%   | 0%    | +0%  | n/a           | ok         |
| objectscript-list-patterns | 100%  | 0%    | 0%   | 0%    | +0%  | n/a           | ok         |
| objectscript-sql-patterns  | 100%  | 0%    | 0%   | 0%    | +0%  | n/a           | ok         |
| iris-connectivity          | 100%  | n/a   | 0%   | 0%    | +0%  | n/a           | ok         |
| ensemble-production        | 100%  | 0%    | 10%  | 10%   | +0%  | n/a           | ok         |
| objectscript-unit-test     | 100%  | 20%   | 0%   | 0%    | +0%  | n/a           | ok         |
| iris-vector-ai             | 100%  | n/a   | 30%  | 10%   | −20% | −45%          | REGRESSION |

The workflow exited 1 on the last row and passed everything above it. That single failure is
the only thing the night decided, and I do not believe it: 30% versus 10% at ten scored items
per arm is three items versus one.

Four things are wrong at once, and they interact. The stories below are ordered so that each
one is worth shipping alone, and so that the earlier ones make the later ones measurable.

### User Story 1 - A score of zero means the agent failed, not that the scorer did (Priority: P1)

Six of the nine skills report exactly 0% in both arms. Those six are exactly the six whose
lift is scored by the LLM-as-judge path; the three that report non-zero numbers are the three
scored by the pattern matcher. That correlation is perfect, and the workflow supplies no
credential the judge can use, so the judge is failing on every call and the failure handler
returns a score of zero. Zero in both arms is arithmetically a lift of +0%, which reads as
"the skill does not help" and is indistinguishable in the report from "nothing was measured".

I want a scorer that cannot silently produce a number. If it cannot score, the run says so and
stops before it spends money, and an individual scoring failure mid-run is carried as an
unscored item rather than folded into a pass rate as a failure.

**Why this priority**: two thirds of the nightly bill is currently buying zeros from a broken
scorer. Every other repair in this spec is unmeasurable until this one lands, because I cannot
tell a null task set from a null scorer while both report 0-vs-0.

**Independent Test**: run one judge-scored skill with the scoring credential deliberately
absent and confirm the run refuses to start and names what is missing; run the same skill with
the credential present and confirm both arms report scores that are not uniformly zero.

**Acceptance Scenarios**:

1. **Given** the scoring credential is absent, **When** a lift measurement is requested,
   **Then** the harness stops before the first billable agent session and names the missing
   credential.
2. **Given** the scorer answers for some items and fails for others, **When** the run
   completes, **Then** the failed items are reported as unscored and excluded from both arms'
   pass rates.
3. **Given** more than one item in ten came back unscored, **When** the run completes,
   **Then** the run is marked invalid, is not compared to any baseline, and cannot update one.
4. **Given** a completed run, **When** I read the result record, **Then** it names which
   scoring mode and which scorer model produced each skill's numbers.

---

### User Story 2 - The gate covers every skill it claims to cover (Priority: P2)

The baseline holds exactly one entry. Eight of the nine skills therefore report Δ = n/a and
cannot trip the gate no matter what they do. The workflow presents itself as a nine-skill
regression gate and is a one-skill gate.

The one entry got there by overwrite, not by omission: a baseline update replaces the whole
file with the skills present in that run, so a single-skill run with the update flag reduces
the file to one skill. The file was last written on 2026-08-18 in an unrelated coverage commit,
which is consistent with exactly that.

The surviving entry is also old enough to be suspect. Its scoring path was last changed on
2026-06-01, and the tool surface the agent sees changed on 2026-09-07 under specs 113 and 114.
A baseline that carries no record of what it was measured against cannot be checked for that,
so I want provenance stored with each entry and a comparison that refuses to run when the
provenance no longer matches.

**Why this priority**: a gate that can only fail on one skill is worse than no gate, because
the eight green rows read as evidence. This is second only because rebaselining before the
scorer is fixed would freeze the zeros in.

**Independent Test**: update the baseline from a single-skill run and confirm the other eight
entries survive; then change a skill's task list and confirm the comparison reports "not
comparable" instead of a Δ.

**Acceptance Scenarios**:

1. **Given** a baseline with nine entries, **When** I update it from a run covering one skill,
   **Then** the other eight entries are unchanged.
2. **Given** a baseline entry recorded against a different task list than the current run
   uses, **When** the comparison runs, **Then** the skill is reported as not comparable and the
   gate treats that as a failure for a skill it is supposed to gate.
3. **Given** the aggregate step is given a regression threshold, **When** the per-skill
   results were computed under a different one, **Then** the aggregate recomputes from the
   stored numbers or fails rather than reporting under the wrong threshold.
4. **Given** a completed rebaseline, **When** I list the covered skills, **Then** every one has
   either a baseline entry or a recorded reason it is intentionally ungated.

---

### User Story 3 - No task set stays in the corpus that measures nothing (Priority: P3)

Seven task sets scored identically in both arms: the four ObjectScript pattern skills,
iris-connectivity, objectscript-unit-test at 0-vs-0, and ensemble-production at 10-vs-10. A
task set that returns the same score with and without the skill measures nothing, and each of
those shards costs about $0.41 to $0.72 a night to learn nothing.

Once the scorer is repaired I expect some of those to come back to life on their own, which is
why this story sits behind Story 1. Whatever is still flat after the repair gets one of four
verdicts, on evidence: broken scorer, too hard for both arms, too easy for both arms, or a
skill that genuinely does not help. Then it is fixed or deleted. Deleting is an acceptable
outcome; keeping a documented null task set is not.

**Why this priority**: this is where the budget for Story 4 comes from. It is third because
the verdicts are only trustworthy after the scorer works.

**Independent Test**: run the seven skills after the scorer repair, read the per-task scores
now persisted in the artifact, and confirm each task set either shows a non-zero arm difference
or carries a recorded verdict and a deletion or replacement.

**Acceptance Scenarios**:

1. **Given** a completed run, **When** any skill's two arms produced the same pass rate,
   **Then** the report names that skill as measuring nothing rather than showing it as ok.
2. **Given** a flat task set, **When** I open the run artifact, **Then** I can read the
   per-task, per-run scores that produced the flat result without re-running anything.
3. **Given** a task set with a recorded verdict of too easy, too hard, or not helped, **When**
   the nightly corpus is validated, **Then** validation fails until the task set is replaced or
   removed.
4. **Given** a task whose fixtures name a namespace the harness never creates, **When** the
   corpus is validated, **Then** validation fails and names the task and the namespace.

---

### User Story 4 - The gate fires on effects, not on coin flips (Priority: P4)

At five runs over two tasks a skill contributes ten scored items per arm, so the smallest
reportable difference is ten percentage points and the standard error of a lift change is
around eighteen points. Four skills are worse than that: one task at five runs is five items,
so twenty points per item. The flagged −20% lift sits inside one standard error of zero, and
the threshold that flagged it is a 5-point floor with an escape hatch that fires on any drop
past 20 points. Both numbers were picked round, and both sit under the noise.

I want a stated minimum detectable effect, an item floor that achieves it, a threshold that is
at least the effect I can detect, and no gating at all for skills below the floor. Money is the
constraint, so power comes from retiring the null task sets and rebalancing the schedule, not
from a bigger bill.

**Why this priority**: last because it needs Story 3's verdicts to know which skills are worth
spending the runs on. It is the story that makes the gate's verdicts believable.

**Independent Test**: run one gated skill twice in a row with nothing changed between the runs
and confirm the two lift values land within the reported minimum detectable effect of each
other and that the gate does not fire.

**Acceptance Scenarios**:

1. **Given** a skill contributing fewer items per arm than the floor, **When** its lift drops
   past the threshold, **Then** the drop is reported and the gate does not fire on it.
2. **Given** a gated skill, **When** the report is printed, **Then** it shows that skill's
   items per arm and the minimum detectable effect those items buy, beside the lift.
3. **Given** the same skill measured twice with no change in between, **When** I compare the
   two lift values, **Then** they differ by less than the reported minimum detectable effect.
4. **Given** a proposed schedule change, **When** the estimated cost is computed, **Then** it
   is checked against a declared cap before any agent session starts, and exceeding the cap
   fails the run.

---

### Edge Cases

- A shard times out and uploads nothing. The merge already backfills it as a named hole; that
  hole must count as "not comparable" for a gated skill, which fails the run, rather than
  disappearing from the average.
- Two shards report the same skill because one was re-run. The later run wins today by
  string-comparing run identifiers; that stays, but the report must say a re-run was merged.
- The scorer answers with valid JSON and an out-of-range score. That is a scorer failure, not a
  zero.
- Every skill's explicit fire-rate probe reads 100%. The probe prompt names the skill outright,
  so a saturated reading is the expected reading and carries no information. A probe that has
  been saturated across consecutive runs must be reported as uninformative rather than as a
  pass, so it is not mistaken for evidence.
- A baseline entry exists for a skill whose eval config has since been deleted. The comparison
  must say so rather than silently dropping the entry on the next update.
- The task corpus is rewritten mid-run by the agent under evaluation. This has happened once
  and the corpus validation added afterwards catches it at startup; that validation must keep
  running before any billable session and must now also cover the namespace and provenance
  checks this spec adds.
- A skill is added with an eval config and no baseline. That is a new skill, not a regression,
  and must be reported distinctly from "not comparable".

## Requirements _(mandatory)_

### Functional Requirements

**Scoring integrity**

- **FR-001**: The harness MUST verify that everything its scorer needs is present and reachable
  before the first billable agent session of a run, and MUST stop and name what is missing when
  it is not.
- **FR-002**: A scoring failure MUST be recorded as an unscored item, distinct from a score of
  zero, and MUST NOT contribute to either arm's pass rate.
- **FR-003**: A run in which more than one scored item in ten came back unscored MUST be
  reported as invalid, MUST NOT be compared against a baseline, and MUST NOT update one.
- **FR-004**: Every result record MUST name, per skill, the scoring mode used and the identity
  of the model that produced the scores. The current record names one model for the whole run
  and names the wrong one.
- **FR-005**: The harness MUST report, per skill and per arm, how many items were scored, how
  many were unscored, and the minimum detectable effect those counts buy.

**Gate coverage**

- **FR-006**: Updating the baseline MUST merge by skill. Skills absent from the current run
  MUST retain their existing entries.
- **FR-007**: A baseline entry MUST record the provenance of its measurement: the date, the run
  identifier, the task identifiers used, the items scored per arm, the scoring mode and scorer
  identity, and the revision of the tool surface the agent saw.
- **FR-008**: The harness MUST refuse to report a Δ against a baseline entry whose provenance
  no longer matches the current run, and MUST report that skill as not comparable.
- **FR-009**: The aggregate gate MUST distinguish four per-skill outcomes: regressed, held, new
  skill, and not comparable. Not comparable for a skill that is supposed to be gated MUST fail
  the run.
- **FR-010**: The regression threshold the aggregate step reports under MUST be the threshold it
  gates under. If the per-skill results were computed under a different threshold, the aggregate
  MUST recompute from the stored numbers or fail.
- **FR-011**: After the rebaseline, every skill with an eval config MUST have either a baseline
  entry or a recorded reason it is intentionally ungated.

**Task-set signal**

- **FR-012**: The harness MUST flag any skill whose two arms produced the same pass rate, and
  MUST NOT show that skill as ok.
- **FR-013**: The run artifact MUST persist per-task, per-run scores and the scorer's stated
  reason, so a flat result can be diagnosed without re-running it. Today only aggregate pass
  rates survive.
- **FR-014**: Each of the seven currently flat task sets MUST be classified, after the scorer
  repair, as one of: broken scorer, too hard for both arms, too easy for both arms, or skill
  does not help. The classification and its evidence MUST be recorded in the feature directory.
- **FR-015**: A task set classified as too easy, too hard, or not helped MUST be replaced or
  deleted. Corpus validation MUST fail while a task set carries such a classification and is
  still referenced by the nightly corpus.
- **FR-016**: Corpus validation MUST fail when a task's fixtures or expectations name a
  namespace the harness does not create. Two tasks in the current corpus name a namespace the
  harness never provisions.

**Power and budget**

- **FR-017**: The harness MUST state its minimum detectable effect: a 25 percentage point change
  in lift, at one-sided significance 0.10 and 80% power, which needs 35 scored items per arm.
- **FR-018**: A skill's lift MUST NOT gate the run unless that skill reached at least 35 scored
  items per arm.
- **FR-019**: A gated skill's task set MUST contain at least four distinct tasks, so the
  measurement samples task-to-task variance and not only run-to-run repetition of one task.
- **FR-020**: The regression threshold MUST be at least the minimum detectable effect. A lift
  drop smaller than the threshold MUST be reported and MUST NOT fail the run. The current
  compound rule, a 5-point floor with a 20-point escape hatch, MUST be replaced by one
  threshold with a stated derivation.
- **FR-021**: The estimated cost of a scheduled run MUST be checked against a declared cap
  before any agent session starts, and exceeding the cap MUST fail the run. The cap MUST be at
  or below the current envelope of $5.47 estimated per night and $38.29 estimated per week.
- **FR-022**: Every behaviour this spec changes MUST have a unit test written before the change,
  covering at minimum: the unscored-item path, the invalid-run threshold, baseline merge
  semantics, provenance mismatch, the item floor, the single regression threshold, and the
  budget cap. Tests that need a live agent or scorer MUST be marked as such and MUST be covered
  by a documented command.

### Key Entities

- **Scored item**: one task run under one arm, carrying a score, the scorer's reason, the arm,
  the task identifier, and whether it was scored at all.
- **Arm**: baseline (skill absent) or skill (skill installed). Pass rate is the share of that
  arm's scored items at or above the passing score.
- **Task set**: the ordered list of tasks a skill's lift is measured over, plus its
  discrimination verdict.
- **Baseline entry**: one skill's stored lift plus the provenance needed to decide whether a new
  measurement is comparable to it.
- **Shard result**: one skill's measured numbers as uploaded by its own job, including item
  counts and unscored counts.
- **Eval run report**: the merged view across shards, with a per-skill outcome, the threshold it
  gated under, named holes for missing shards, and the run's realised cost.
- **Budget envelope**: the declared nightly and weekly caps, checked before spending.

## Assumptions

- `.specify/feature.json` pointed at `specs/113-typed-tool-schemas`, which shipped in v1.4.0 and
  is an ancestor of the current head. It is not in flight, so I replaced it with this feature
  directory.
- The scorer diagnosis in Story 1 is an inference from a perfect correlation (six judge-scored
  skills at 0-vs-0, three pattern-scored skills non-zero) plus the absence of any scorer
  credential in the workflow environment. It is not yet confirmed against a logged scorer error,
  because the run artifact does not persist scorer reasons. FR-013 is what makes the next
  occurrence self-evidencing; confirming this one is the first task of Story 1.
- Specs 113 and 114 changed the tool surface on 2026-09-07, after the 2026-08-18 baseline write.
  That invalidates the baseline for the skills whose lift is measured with the tool surface
  present. It does not by itself invalidate iris-vector-ai's entry, because that skill's lift is
  measured with no tools available at all. iris-vector-ai's baseline is stale for a different
  reason: its scoring path last changed on 2026-06-01 and the entry carries no record of which
  scoring path produced it.
- The cost figures in this spec are the harness's own dry-run estimates at five runs per skill,
  not billed amounts. They price an agent session at $0.020 and a scorer call at $0.001. Those
  constants were last corrected on 2026-09-07 against a measured session duration, so I treat
  them as the best available estimate and the caps in FR-021 are stated in the same units.
- The two-tier schedule in the cost table below is the cheapest arrangement I found that
  reaches the item floor. The plan phase may find a better one; FR-021's cap is the binding
  constraint, not the schedule.
- Passing score stays at 2 of 3. Nothing in the observed data suggests the threshold is the
  problem, and changing it would invalidate every historical number at once.

## Cost

Estimated, at five runs per skill, from the harness's own estimator.

**Before.** Nine shards every night: $5.47 per night, $38.29 per week.

| Bucket                                                               | Nightly | Share |
| -------------------------------------------------------------------- | ------: | ----: |
| Judge-scored shards returning 0-vs-0                                 |   $2.67 |   49% |
| ensemble-production returning 10-vs-10                               |   $0.72 |   13% |
| Signal-bearing but with no baseline to compare against (iris-ai-hub) |   $1.46 |   27% |
| Signal-bearing and able to move the gate (iris-vector-ai)            |   $0.62 |   11% |

So 62% of the nightly spend returns identical numbers in both arms, and only 11% is capable of
failing the run.

**After.** Two tiers, funded by retiring the null task sets and by not measuring every skill's
lift every night.

| Tier                                                                                   |       Cost | Cadence         |
| -------------------------------------------------------------------------------------- | ---------: | --------------- |
| Trigger probes for all kept skills at three runs (fire-rate, implicit, isolation only) |      $1.14 | nightly         |
| Full-power lift for one skill per night, rotating, each at 35+ items per arm           |     ~$2.50 | nightly average |
| **Total**                                                                              | **~$3.64** | **nightly**     |

Weekly that is about $25.48 against today's $38.29, a 33% cut, while every lift measurement
that runs is powered to the stated effect instead of being noise. A skill at four tasks and nine
runs, or six tasks and six runs, costs $1.87 to $2.05 for 36 items per arm; nine such skills is
about $17.50 a week. Each task set retired in Story 3 takes roughly $1.90 a week off that.

The trade is detection latency: a regression in a given skill is caught within a week rather
than within a night. I am taking that trade because today's nightly detection is unavailable for
eight of nine skills and noise-limited for the ninth, so the latency I am giving up is
notional.

## Out of Scope

- Editing any skill's own content to move a score. Rebaselining is in scope; rewriting a skill
  so a number goes up is measuring my own homework.
- GEPA tool-description optimisation. It shares the harness and it is a separate effort.
- Adding new skills. Adding or replacing benchmark tasks for existing skills is in scope under
  FR-015.
- Changing the agent model or the passing score.
- Reworking the shard and merge topology. It worked on its first full night and the wall clock
  is not the problem.

## Dependencies

No blocking specs. Specs 113 and 114 are already merged and are the reason the baseline is
stale, not a prerequisite. This spec touches only the Python harness under `tests/e2e/` and
`benchmark/021/`, the nightly workflow, and the task corpus; it changes no Rust and no tool.

## Bug Classes and Detectors

Constitution governance requires a detector per bug class, not just a fix per instance.

| Class                                 | Instance                                                                                          | Detector                                                                                                              |
| ------------------------------------- | ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Scorer failure scored as zero         | Six skills reported 0-vs-0 for at least one night because the scorer could not authenticate       | Preflight reachability check (FR-001) plus a unit test asserting a scorer failure yields an unscored item, not a zero |
| Whole-file baseline replace           | The baseline shrank to one entry in an unrelated commit                                           | Unit test: a single-skill update preserves every other entry (FR-006)                                                 |
| Comparison against unlike measurement | A Δ reported against a baseline measured under a different scoring path and tool surface          | Provenance stored and checked; "not comparable" is an outcome (FR-007, FR-008)                                        |
| Null task set kept in the corpus      | Seven task sets returned identical arms and stayed in the nightly schedule                        | Flat-arm flag in the report plus corpus validation failing on a recorded null verdict (FR-012, FR-015)                |
| Gate threshold under the noise floor  | A 5-point threshold with a 20-point escape hatch on a measurement with an 18-point standard error | Threshold derived from the item count and asserted against it; item floor gates gating (FR-018, FR-020)               |
| Budget checked after spending         | The dry-run estimate existed but nothing enforced it                                              | Declared cap asserted before the first session (FR-021)                                                               |

## Success Criteria _(mandatory)_

### Measurable Outcomes

- **SC-001**: No skill in a valid run reports the same pass rate in both arms without a recorded
  verdict explaining why. The nine-skill table has zero unexplained flat rows.
- **SC-002**: Zero items in any gating run carry a score that came from a scorer that failed to
  answer.
- **SC-003**: A run with the scoring credential removed stops before the first agent session and
  names the missing credential, spending nothing.
- **SC-004**: Every skill with an eval config has either a baseline entry or a recorded reason it
  is ungated. Skills able to fail the gate rise from one of nine to at least six of nine.
- **SC-005**: Updating the baseline from a one-skill run leaves every other entry byte-identical.
- **SC-006**: Every gated skill reports at least 35 scored items per arm and at least four
  distinct tasks.
- **SC-007**: For every gated skill, the threshold it is gated on is greater than or equal to the
  minimum detectable effect printed beside its lift.
- **SC-008**: A skill measured twice back to back with nothing changed produces two lift values
  differing by less than the reported minimum detectable effect, and the gate does not fire on
  the pair.
- **SC-009**: Estimated nightly cost is at or below $5.47 and estimated weekly cost is at or
  below $38.29, checked before any session runs; the target arrangement lands near $3.64 nightly
  and $25.48 weekly.
- **SC-010**: The share of nightly spend on measurements capable of failing the gate rises from
  11% to at least 80%.
- **SC-011**: A flat result can be diagnosed from the stored run artifact alone, without
  re-running the skill: per-task scores and scorer reasons are present for every item.
- **SC-012**: A shard that uploads nothing causes the run to fail for the skills it was meant to
  gate, rather than being averaged away.
