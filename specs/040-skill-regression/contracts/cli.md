# Contract: Skill Eval CLI

## Invocation

```bash
python -m tests.e2e.skill_eval [OPTIONS]
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--skill <name>` | all | Evaluate a single skill (no confirmation required) |
| `--dry-run` | false | Print cost estimate and exit |
| `--yes` | false | Skip confirmation for full-suite run |
| `--update-baseline` | false | Update `skill-baseline.json` + print diff after run |
| `--regression-threshold <f>` | 0.05 | Minimum lift drop to set regression_flag |
| `--runs <n>` | 3 | Fire-rate runs per skill |
| `--output <dir>` | `tests/e2e/results/` | Results output directory |
| `--model <str>` | `amazon-bedrock/us.anthropic.claude-sonnet-4-5-20250929-v1:0` | Task model |

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Run complete, no regressions |
| 1 | One or more regressions detected |
| 2 | Setup error (missing IRIS, missing key) |
| 3 | User declined confirmation |

## Dry-run output format

```
Skill eval dry run (2026-05-31):
  Skills with eval.yaml: 8 (5 standard, 3 domain)
  Skills without coverage: 16
  Task runs (Sonnet): 126
  Judge calls (Haiku): 90
  Estimated cost: ~$3.20 USD
  Estimated time: ~25 min

Run with --yes to proceed, or --skill <name> for a single skill.
```

## Stdout summary (end of run)

```
Skill Evaluation Results — 2026-05-31T14:32:00Z
================================================
Skill                      Fire%   Baseline   Skill    Lift    Δ      Regression
objectscript-review        100%    71%        100%     +29%    +0%    ✓
objectscript-guardrails    100%    71%        86%      +15%    +1%    ✓
objectscript-sql-patterns   80%    93%        100%     +7%     +0%    ✓
objectscript-list-patterns  67%    71%        91%      +20%    n/a    ✓  (new)
objectscript-unit-test      80%    71%        86%      +15%    n/a    ✓  (new)
iris-vector-ai (domain)    100%    isolation=0%  —      —      —      ✓
iris-connectivity (domain) 100%    isolation=0%  —      —      —      ✓
ensemble-production (domain) 80%   isolation=0%  —      —      —      ✓
[16 skills: no task coverage]

Regressions: 0
Improvements: 2
```
