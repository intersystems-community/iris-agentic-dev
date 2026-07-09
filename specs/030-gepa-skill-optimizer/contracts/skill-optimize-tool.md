# Contract: `skill_optimize` MCP tool (replaces its `NOT_IMPLEMENTED` stub)

Same tool name, same category (`ToolCategory::Skill`, unchanged). The input shape extends
today's `SkillNameParams { name: String }`; the output shape is new (the tool previously
returned a stub error unconditionally, so there is no pre-existing shape to preserve —
Constitution Principle V is satisfied trivially here, and this contract becomes the
canonical shape going forward).

## Input

```json
{
  "name": "objectscript-review",     // required — matches SkillNameParams today
  "apply": false,                     // optional, default false
  "candidate_count": 3,                // optional, default 3, max 10
  "batch_size": 5,                     // optional, default 5 (diagnosis batching)
  "improvement_threshold": 0.0         // optional, default 0.0 (any positive improvement wins)
}
```

`apply: true` additionally requires that a prior `apply: false` call for the same `name`
produced a `winner` in the current process (see `NO_PENDING_PROPOSAL` below).

## Output — propose-only (`apply: false`, the default)

```json
{
  "skill_name": "objectscript-review",
  "failure_patterns": [
    {"task_id": "jira-005", "outcome": "fail", "summary": "..."}
  ],
  "baseline_held_out_pass_rate": 0.72,
  "held_out_set_size": 12,
  "candidates": [
    {
      "round": 1,
      "held_out_pass_rate": 0.83,
      "held_out_set_size": 12,
      "passes_all_locked": true,
      "locked_task_results": [{"task_id": "jira-002", "passed": true}],
      "diff": "... unified diff of candidate text vs. current skill text ..."
    }
  ],
  "winner": 0,
  "applied": false
}
```

`winner` is `null` when no candidate both beats `baseline_held_out_pass_rate` by
`improvement_threshold` AND has `passes_all_locked: true` for every candidate (spec
Acceptance Scenario 3, User Story 3). `candidates[].diff` is a unified diff, not the full
candidate text, to keep the response compact — the full text is retrievable by re-calling
with `apply: true` (which returns it) or is available in the process-local pending
proposal for follow-up tooling.

## Output — no failures to optimize

```json
{
  "skill_name": "objectscript-review",
  "failure_patterns": [],
  "candidates": [],
  "winner": null,
  "note": "NOTHING_TO_OPTIMIZE — this skill has no non-passing benchmark tasks"
}
```

## Output — apply (`apply: true`, referencing a prior winning proposal)

```json
{
  "skill_name": "objectscript-review",
  "applied": true,
  "written_path": "light-skills/skills/objectscript-review/SKILL.md",
  "newly_locked_task_ids": ["jira-005"],
  "held_out_pass_rate": 0.83
}
```

## Error contract

| Condition | Error Code | Behavior |
|---|---|---|
| No IRIS connection discoverable | `IRIS_UNREACHABLE` | Standard code, reused as-is. |
| No LLM configured (`LlmClient::from_env()` is `None`) | `LLM_NOT_CONFIGURED` | Distinct from `IRIS_UNREACHABLE` — the caller needs to know *which* prerequisite is missing. |
| `apply: true` with no pending winning proposal for `name` in this process | `NO_PENDING_PROPOSAL` | Exit/return non-success; does NOT silently run a fresh propose round and apply it (spec FR-005, Acceptance Scenario 6). |
| Concurrent optimization run against the same container | `BENCHMARK_RUN_IN_PROGRESS` | Reused from 059's existing benchmark-run lock — same code, same semantics. |
| `candidate_count`/`batch_size` out of documented bounds | `INVALID_PARAMS` | Reused standard code. |

See `data-model.md`'s Error Code Registry for the canonical definition of each new code.
