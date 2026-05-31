# Data Model: Skill Regression and Lift Suite (040)

## Entities

### SkillEvalConfig
Stored in `tests/e2e/tasks/skills/{skill-name}/eval.yaml`.

```
SkillEvalConfig
├── skill: str                    # matches directory name in light-skills/skills/
├── description: str
├── fire_rate_prompt: str         # task prompt for fire-rate measurement
├── benchmark_tasks: List[str]    # task IDs from benchmark/021/tasks/ (may be empty)
├── domain_skill: bool            # true = load-on-demand, subject to isolation guard
└── isolation_prompt: str | None  # domain skills only: general repair task (must NOT fire)
```

### SkillResult
Per-skill output in EvalRun.

```
SkillResult
├── skill: str
├── fire_rate: float | None         # null if no fire_rate_prompt
├── isolation_fire_rate: float | None  # domain skills only
├── pass_rate_baseline: float | None   # null if no benchmark_tasks
├── pass_rate_skill: float | None
├── lift: float | None               # pass_rate_skill - pass_rate_baseline
├── lift_delta: float | None         # lift - baseline_file[skill].lift (null if new)
├── regression_flag: bool
├── new_skill: bool                  # no prior baseline entry
├── no_task_coverage: bool
└── task_ids_used: List[str]
```

### EvalRun
Top-level output file `tests/e2e/results/skill-eval-{timestamp}.json`.

```
EvalRun
├── run_id: str                   # ISO timestamp
├── model: str                    # e.g. amazon-bedrock/us.anthropic...
├── judge_model: str              # Haiku model used for scoring
├── timestamp: str
├── regression_threshold: float
├── skills: List[SkillResult]
└── summary:
    ├── regressions: List[str]    # skill names with regression_flag=true
    ├── improvements: List[str]   # skills with lift_delta > 0
    ├── uncovered: List[str]      # skills with no_task_coverage=true
    └── estimated_cost_usd: float
```

### BaselineFile
`tests/e2e/results/skill-baseline.json` — dict keyed by skill name.

```
{
  "objectscript-review": {
    "fire_rate": 1.0,
    "lift": 0.29,
    "pass_rate_baseline": 0.71,
    "pass_rate_skill": 1.00,
    "run_id": "2026-05-31T...",
    "model": "amazon-bedrock/..."
  },
  ...
}
```

## State Transitions

```
SkillEvalConfig lifecycle:
  discovered (dir scan) → loaded (eval.yaml present) | uncovered (no eval.yaml)
                                                       ↓
                                              fire_rate_measured → benchmark_measured
                                                                  → result_written

BaselineFile:
  absent (first run) → created (--update-baseline after first run)
  present → compared (each run) → updated (--update-baseline only)
```
