# Phase 1 data model: skill-eval harness repair

**Date**: 2026-09-08 | **Feature**: 118-skill-eval-harness-repair

Six entities. Two are new (Scored item, Provenance), four are existing dataclasses that gain
fields. Nothing is renamed, so the merge step keeps reading old shard files during the transition.

## 1. Scored item

The result of one task run in one arm, and whether it got a score at all. New — today a score is a
bare `int` in a list, which is exactly why an unscorable item could not be told from a zero.

| Field           | Type                                      | Notes                                                       |
| --------------- | ----------------------------------------- | ----------------------------------------------------------- |
| `task_id`       | str                                       | From the skill's `eval.yaml`, e.g. `IRIS-PYTHON-CONNECT`    |
| `arm`           | `"baseline"` \| `"skill"`                 | Which arm produced it                                       |
| `run_index`     | int                                       | 0-based, up to `--runs`                                     |
| `scored`        | bool                                      | False means the scorer could not produce a verdict          |
| `score`         | int \| None                               | 0–3 when `scored`; **None** when not. Never 0 as a stand-in |
| `reasoning`     | str                                       | The scorer's text, or the failure description               |
| `scoring_mode`  | `"judge"` \| `"assertion"` \| `"pattern"` | Which of the three paths produced it                        |
| `scorer_model`  | str \| None                               | From the scoring response; None for assertion/pattern modes |
| `input_tokens`  | int \| None                               | Measured, for the cost record (R11)                         |
| `output_tokens` | int \| None                               | Measured                                                    |

Rules:

- `scored is False` ⟹ `score is None`. The invariant that the whole spec turns on.
- `scored is True` ⟹ `score in (0, 1, 2, 3)`.
- `scoring_mode == "judge"` ⟹ `scorer_model` is set whenever `scored` is True.
- A pass is `score >= 2`, unchanged from today.

## 2. Arm result

The aggregate of one arm's scored items for one skill. New as a named thing; today it is a
bare float.

| Field            | Type          | Notes                                                    |
| ---------------- | ------------- | -------------------------------------------------------- |
| `items_total`    | int           | Tasks × runs                                             |
| `items_scored`   | int           | Count with `scored is True`                              |
| `items_unscored` | int           | `items_total - items_scored`                             |
| `items_passed`   | int           | Count with `score >= 2`                                  |
| `pass_rate`      | float \| None | `items_passed / items_scored`; **None** when scored is 0 |

Rules:

- `pass_rate` is over scored items only (R4).
- `items_scored == 0` ⟹ `pass_rate is None`, and every downstream Δ involving it is refused.

## 3. Provenance

What a measurement was taken under. New. The record that makes an old number comparable or not.

| Field                    | Type        | Notes                                                                |
| ------------------------ | ----------- | -------------------------------------------------------------------- |
| `run_id`                 | str         | The run that produced the measurement (FR-007)                       |
| `task_ids`               | list[str]   | Sorted. The task set, not just its size                              |
| `scoring_mode`           | str         | The skill's mode; mixed modes within a skill are recorded as `mixed` |
| `scorer_model`           | str \| None | Resolved from the response (R2)                                      |
| `scorer_model_requested` | str \| None | The constant the code asked for; kept so a divergence is visible     |
| `tool_surface`           | str         | `"<version>+<12-hex>"`, or `"none"` when no binary resolved (R6)     |
| `runs`                   | int         | `--runs` for that measurement                                        |
| `measured_at`            | str         | ISO 8601 UTC                                                         |
| `harness_commit`         | str \| None | Short SHA when resolvable                                            |

Comparability, per R8: `task_ids`, `scoring_mode`, and `scorer_model` must match for a Δ to be
computed. `tool_surface` difference is annotated, never suppressing. `run_id`, `runs`,
`measured_at`, and `harness_commit` are informational.

## 4. Baseline entry (schema 2)

One skill's stored reference measurement. Extends today's four-float entry.

| Field                | Type          | Notes                                  |
| -------------------- | ------------- | -------------------------------------- |
| `pass_rate_baseline` | float \| None | Existing field, now nullable           |
| `pass_rate_skill`    | float \| None | Existing field, now nullable           |
| `lift`               | float \| None | Existing                               |
| `fire_rate`          | float         | Existing                               |
| `items`              | object        | The two Arm results, keyed by arm name |
| `provenance`         | Provenance    | New; required in v2                    |

The file that holds them:

| Field            | Type   | Notes                                                               |
| ---------------- | ------ | ------------------------------------------------------------------- |
| `schema`         | int    | `2`. Absent means v1                                                |
| `skills`         | object | Skill name → Baseline entry                                         |
| `ungated_skills` | object | Skill name → reason string. FR-011's other branch; empty by default |

`ungated_skills` is where a skill that has an eval config and deliberately no entry records why.
Keeping it in this file rather than in prose means the reader that answers "is every skill
covered?" is the reader that already has the file open. A skill name MUST NOT appear in both maps.

Writes are merge-by-skill: read, replace the skills in this run, write back (R7). A v1 file is
migrated in memory on read; its entries get `provenance: None`, which makes every one of them
`not_comparable` with reason `no provenance recorded` — not "unknown, try anyway".

## 5. Skill result

One skill's outcome for this run. Existing `SkillResult`, extended.

| Field               | Type          | Notes                                                                  |
| ------------------- | ------------- | ---------------------------------------------------------------------- |
| `skill_name`        | str           | Existing                                                               |
| `lift`              | float \| None | Existing, now nullable                                                 |
| `fire_rate`         | float         | Existing                                                               |
| `baseline_delta`    | float \| None | Existing; None when not comparable                                     |
| `regression_flag`   | bool          | Existing. **Derived**: `outcome == "regressed"`, computed nowhere else |
| `outcome`           | enum          | New: `regressed` \| `held` \| `new_skill` \| `not_comparable`          |
| `outcome_reason`    | str \| None   | New; why, when `not_comparable`                                        |
| `arms`              | object        | New: the two Arm results                                               |
| `provenance`        | Provenance    | New                                                                    |
| `threshold_applied` | float         | New; the threshold the outcome was computed under (FR-010)             |

State transitions — the outcome is derived, in this order, and the first match wins:

1. No baseline entry for the skill → `new_skill`
2. Baseline entry has no provenance, or provenance differs on task set / scoring mode / scorer
   model → `not_comparable`, with `outcome_reason` naming the field
3. Either arm's `pass_rate` is None → `not_comparable`, reason `no scored items`
4. `baseline_delta < -threshold_applied` → `regressed`
5. Otherwise → `held`

`regression_flag` is then `outcome == "regressed"` and is computed in that one place. It survives
only because `shard.py` merges shard files by key and a mid-transition merge would drop a renamed
or deleted field silently — the same reason `judge_model` keeps its name. Two fields, one decision,
and the enum is the one that decides.

Rule 4 is a single comparison, which is not the rule the harness gates on today: that is a
5-point floor with an escape hatch at 20 points. Replacing the compound rule with one threshold
and a stated derivation is FR-020, deferred. The single comparison is safe here because 118 gates
on nothing per-skill (R12) — `regressed` is a printed word, not an exit code — and shipping the
compound rule into a new derivation would carry a threshold nobody can derive into the one place
the follow-on has to rewrite anyway.

## 6. Eval run report

The merged, printed artifact. Existing `EvalRun`, extended.

| Field                    | Type              | Notes                                                                                                   |
| ------------------------ | ----------------- | ------------------------------------------------------------------------------------------------------- |
| `run_id`                 | str               | Existing                                                                                                |
| `results`                | list[SkillResult] | Existing                                                                                                |
| `judge_model`            | str               | Existing name, kept for shard-file compatibility; now the resolved scorer model, not a hardcoded string |
| `scorer_model_requested` | str \| None       | New                                                                                                     |
| `threshold`              | float             | New; the one the outcomes used                                                                          |
| `items_unscored_share`   | float             | New; run-wide, drives the FR-003 gate                                                                   |
| `run_valid`              | bool              | New; False when the unscored share exceeds the limit                                                    |
| `estimated_cost_usd`     | float \| None     | New; from measured usage × the declared rate (R11)                                                      |

`judge_model` keeps its name deliberately: `shard.py` merges shard files by key, and renaming it
would make a mid-transition merge drop the field silently — the #110 pattern this project has
already been bitten by. It stops being a lie (it was `"openai/gpt-4.1"`) without becoming a
compatibility break.

## Relationships

```text
Eval run report
 └── Skill result            (one per skill evaluated)
      ├── Arm result × 2     (baseline, skill)
      │    └── Scored item × (tasks × runs)
      └── Provenance         (also stored in the Baseline entry)

Baseline file (schema 2)
 └── Baseline entry          (one per skill, merged not replaced)
      ├── Arm result × 2
      └── Provenance
```

## Glossary

Three pairs of names look like drift and are not. Written down so nobody normalises them.

| In prose       | In code          | Why they differ                                                                         |
| -------------- | ---------------- | --------------------------------------------------------------------------------------- |
| not comparable | `not_comparable` | Enum value. Prose reads it as English, the JSON carries the identifier                  |
| the judge      | `judge_model`    | The field name predates Bedrock; it now holds the resolved scorer model (§6)            |
| the scorer     | `scorer_model`   | The general term, covering judge, assertion, and pattern modes; `None` for the last two |
