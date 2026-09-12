# Contract: baseline file v2

**Feature**: 118-skill-eval-harness-repair
**File**: `tests/e2e/results/skill-baseline.json`

The one durable artifact in the harness. Committed, read by every run, and until now silently
truncated by every run.

## Shape

```json
{
  "schema": 2,
  "skills": {
    "objectscript-guardrails": {
      "pass_rate_baseline": 0.33,
      "pass_rate_skill": 0.58,
      "lift": 0.25,
      "fire_rate": 1.0,
      "items": {
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
      "provenance": {
        "run_id": "2026-09-12T04-02-11Z-shard-3",
        "task_ids": [
          "OBJECTSCRIPT-ERROR-HANDLING",
          "OBJECTSCRIPT-STORAGE-BLOCK"
        ],
        "scoring_mode": "judge",
        "scorer_model": "claude-sonnet-4-6",
        "scorer_model_requested": "us.anthropic.claude-sonnet-4-6",
        "tool_surface": "1.4.0+9f2c1ab77e04",
        "runs": 3,
        "measured_at": "2026-09-12T04:11:07Z",
        "harness_commit": "9bd88c5"
      }
    }
  },
  "ungated_skills": {}
}
```

`ungated_skills` maps a skill name to the reason it has an eval config and deliberately no entry.
It is FR-011's second branch, and it lives here so that "is every skill covered?" is answerable
from this file alone. A name in both `skills` and `ungated_skills` is an error — the two together
are the coverage census, and a skill cannot be gated and excused at once.

## Write semantics

`save_baseline(results, path)`:

1. Read the existing file. Absent or unparseable → start from
   `{"schema": 2, "skills": {}, "ungated_skills": {}}`.
2. A v1 file (no `schema` key, skill names at the top level) is migrated in memory: each entry
   keeps its four floats, gains `items: null` and `provenance: null`.
3. Replace only the entries for skills present in `results`.
4. Print a warning naming any entry whose skill has no `eval.yaml` under
   `tests/e2e/tasks/skills/`. Keep the entry — a deleted config may come back, and a write is not
   the place to decide otherwise — but never let it leave silently.
5. Write the whole file back, keys sorted, two-space indent, trailing newline.

An entry for a skill not in this run is preserved byte-for-byte in content — same values, same
field order after the sort. This is the fix for the bug that left a nine-skill baseline holding one
entry.

Step 4 exists because the opposite of the bug in step 3 is also a bug: a file that quietly keeps
entries for skills that no longer exist reports coverage it does not have, and the census in
`ungated_skills` would be counting a skill nobody can run.

## Read semantics

`load_baseline(path) -> dict[str, BaselineEntry]`

- Absent file → `{}`. Not an error; every skill is then `new_skill`.
- `schema` absent → v1, migrate in memory, do not write back.
- `schema` present and not `2` → error. A newer file must not be silently downgraded by an older
  harness.
- `ungated_skills` absent → `{}`. Every skill is then expected to have an entry.

## Comparability

A Δ against an entry is computed only when all three match:

| Field          | Mismatch means                                                      |
| -------------- | ------------------------------------------------------------------- |
| `task_ids`     | A different set of tasks was measured. Not the same experiment      |
| `scoring_mode` | A different scale. `judge` 0–3 and `assertion` 3/0 are not the same |
| `scorer_model` | Recalibrated. The old numbers were graded by a different grader     |

`provenance` being `null` (a v1 entry) also blocks the Δ — reason `no provenance recorded`.

`tool_surface` mismatch **does not** block the Δ. It prints beside it:

```text
objectscript-guardrails   +0.02  held      (tool surface 1.4.0+9f2c1ab77e04 → 1.5.0+3ba0e1d44c19)
```

Suppressing on the tool surface would silence the gate on exactly the releases it exists to check.

## Migration path in practice

FR-011 rebaselines all nine skills, so every entry becomes v2 in one workflow-dispatch run. The v1
read path exists so the first post-repair run does not crash on the file currently on disk — the
one holding a single `iris-vector-ai` entry — and so that entry reports `not_comparable` instead of
being compared against a number measured by a scorer that returned zeros.
