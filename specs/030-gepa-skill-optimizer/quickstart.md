# Quickstart: GEPA-Inspired Skill Optimization Loop

Prerequisites: a running IRIS container discoverable the same way `iris-agentic-dev
benchmark` finds one (see `specs/059-tool-telemetry-benchmark/quickstart.md`), plus an
`ANTHROPIC_API_KEY` or `OPENAI_API_KEY` in the environment (the same requirement the
benchmark harness's `--baseline` fix-generation step already has).

## Propose an optimization (no file is written)

Via any MCP client connected to `iris-agentic-dev mcp`:

```json
// Call: skill_optimize
{"name": "objectscript-review"}
```

```json
// Response (abridged)
{
  "skill_name": "objectscript-review",
  "failure_patterns": [
    {"task_id": "jira-011", "outcome": "fail", "summary": "Skill doesn't mention $LISTBUILD null-handling, so the model omitted the check the task requires."}
  ],
  "baseline_held_out_pass_rate": 0.83,
  "held_out_set_size": 12,
  "candidates": [
    {"round": 1, "held_out_pass_rate": 0.92, "held_out_set_size": 12, "passes_all_locked": true, "diff": "..."}
  ],
  "winner": 0,
  "applied": false
}
```

Nothing on disk changed. `light-skills/skills/objectscript-review/SKILL.md` is untouched.

## Apply the winning candidate

```json
// Call: skill_optimize (same process, following the propose-only call above)
{"name": "objectscript-review", "apply": true}
```

```json
{
  "skill_name": "objectscript-review",
  "applied": true,
  "written_path": "light-skills/skills/objectscript-review/SKILL.md",
  "newly_locked_task_ids": ["jira-011"],
  "held_out_pass_rate": 0.92
}
```

`jira-011` is now in `light-skills/skills/objectscript-review/.optimizer-lock.json`'s
`locked_task_ids` — every future optimization round for this skill will verify a new
candidate still passes it before it can win, regardless of that candidate's held-out score.

## Calling `apply: true` without a prior proposal

```json
{"name": "objectscript-review", "apply": true}
```

If no propose-only call happened first in this process:

```json
{"error_code": "NO_PENDING_PROPOSAL", "error": "No pending winning proposal for 'objectscript-review'. Call skill_optimize with apply omitted or false first."}
```

## A skill with nothing to fix

```json
{"name": "objectscript-navigation"}
```

```json
{"skill_name": "objectscript-navigation", "failure_patterns": [], "candidates": [], "winner": null, "note": "NOTHING_TO_OPTIMIZE — this skill has no non-passing benchmark tasks"}
```

## Independent test verification (maps to spec.md's Independent Test sections)

1. **User Story 1**: run the benchmark harness for a skill with a known-failing task,
   call `diagnose_failures` directly with that run's `TaskResult`s and telemetry —
   verify one `FailurePattern` per non-passing task, batched per `batch_size`.
2. **User Story 2**: `skill_optimize` with `apply: false` against an underperforming skill
   — verify a candidate, its held-out score, and that `SKILL.md` is unchanged on disk;
   follow with `apply: true` — verify the file is now overwritten with the winning
   candidate's text.
3. **User Story 3**: apply a winning candidate, then force a deliberately-regressing
   candidate into a later round's pool — verify it is excluded from winner selection
   despite scoring well on the held-out subset.
