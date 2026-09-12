# Contract: shard result and merged report

**Feature**: 118-skill-eval-harness-repair

Two artifacts. A shard result is written by each matrix leg and read by the aggregate job; the
merged report is what a human reads.

## Shard result JSON

Written to `--output`. One per matrix leg.

```json
{
  "run_id": "2026-09-12T04-02-11Z-shard-3",
  "judge_model": "claude-sonnet-4-6",
  "scorer_model_requested": "us.anthropic.claude-sonnet-4-6",
  "threshold": 0.05,
  "items_unscored_share": 0.0,
  "run_valid": true,
  "estimated_cost_usd": 0.61,
  "results": [
    {
      "skill_name": "objectscript-guardrails",
      "lift": 0.25,
      "fire_rate": 1.0,
      "baseline_delta": 0.02,
      "regression_flag": false,
      "outcome": "held",
      "outcome_reason": null,
      "threshold_applied": 0.05,
      "arms": {
        "baseline": {
          "items_total": 12,
          "items_scored": 12,
          "items_unscored": 0,
          "items_passed": 4,
          "pass_rate": 0.33
        },
        "skill": {
          "items_total": 12,
          "items_scored": 12,
          "items_unscored": 0,
          "items_passed": 7,
          "pass_rate": 0.58
        }
      },
      "provenance": { "...": "as in baseline-v2.md" }
    }
  ]
}
```

`judge_model` keeps its key name on purpose. `shard.py` merges by key, and a rename would drop the
field silently in any mixed-version merge — the failure mode this repo has already shipped once
(issue #110). What changes is that it now holds the resolved model instead of the hardcoded
`"openai/gpt-4.1"`.

## Merge

`merge_results(paths) -> EvalRun`

- Later `run_id` wins on a per-skill collision, unchanged from today. What is new is that the
  merge says so: a footer line names each skill whose result came from a re-run and the `run_id`
  it discarded. A silently-deduplicated table cannot be told from one that only ever had one
  result per skill, and the two mean different things about the night.
- A shard that uploaded nothing stays a named hole, and the skills it was meant to cover report
  `not_comparable` with reason `shard produced no result`. It does not fail the run — nothing
  per-skill does in 118 (FR-009). Making it fail is SC-012, deferred.
- Unparseable files are skipped with a printed warning, unchanged.
- `run_valid` for the merged run is the AND of the shards'.
- `items_unscored_share` for the merged run is recomputed over all items, not averaged over shards.
  Averaging shares would let one tiny shard of two items outweigh one of forty.
- `threshold` must be identical across shards. A mismatch is FR-010's failure: exit 1, printing
  both values. It means half the table was judged under a different rule than the other half.

## Printed table

Every column exists because its absence hid something in the run of 2026-09-07.

```text
skill                          mode      scored  base   skill  lift   Δ base  outcome
------------------------------ --------- ------- ------ ------ ------ ------- ---------------
iris-connectivity              judge     24/24    0.42   0.71  +0.29   +0.04  held
objectscript-guardrails        judge     24/24    0.33   0.58  +0.25   +0.02  held
objectscript-review            judge     22/24    0.38   0.50  +0.12      —   not comparable
iris-vector-ai                 pattern   18/18    0.33   0.58  +0.25      —   not comparable
ensemble-production            pattern   18/18    0.50   0.50   0.00   -0.08  regressed

scorer: claude-sonnet-4-6 (requested us.anthropic.claude-sonnet-4-6)
tool surface: 1.4.0+9f2c1ab77e04
threshold: 0.05   unscored: 2/108 (1.9%)   estimated cost: $5.12
run valid: yes
re-runs merged: objectscript-review (discarded 2026-09-12T03-40-02Z-shard-3)
```

Rules:

- `scored` is `items_scored/items_total` summed over both arms. `22/24` is a visible fact, not a
  footnote.
- A `—` in the Δ column means no comparison was made. A `0.00` means a comparison was made and came
  out flat. Today both print as a number.
- `not comparable` rows print their reason on a following indented line.
- The footer names the scorer, the tool surface, the threshold, and the unscored share on every run,
  including the green ones. The broken run printed none of these, which is why it looked normal.
- `re-runs merged` prints only when there was one. An absent line means one result per skill.
- `regression_flag` in the JSON is `outcome == "regressed"` and nothing computes it separately. It
  keeps its name for the same reason `judge_model` does.

## Non-goals for this contract

Per-item detail stays out of the shard JSON. Reasoning strings for 108 items across nine shards
would be the bulk of the artifact, and the transcript files already hold them. The counts are what
the gate needs.
