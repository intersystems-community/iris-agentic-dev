# Quickstart: running the repaired skill-eval harness

**Feature**: 118-skill-eval-harness-repair

For whoever runs this next, on a laptop or in CI. Written against the post-repair behaviour.

## What you need

| Variable                   | Why                                                                                |
| -------------------------- | ---------------------------------------------------------------------------------- |
| `AWS_BEARER_TOKEN_BEDROCK` | The scorer runs on Bedrock. Without it, preflight exits 2 and spends nothing       |
| `AWS_REGION`               | Bedrock region                                                                     |
| `IAD_BINARY`               | Optional. Path to `iris-agentic-dev`. Falls back to `PATH`, then the Homebrew path |
| `PYTHONPATH`               | Repo root, so `tests.e2e.skill_eval` imports                                       |

`OPENAI_API_KEY` is not used by the scorer. It was the only key the nightly supplied, and that is
why the nightly printed zeros.

Check what you have before spending anything:

```bash
python -m tests.e2e.skill_eval --list-skills     # no preflight, no spend
python -m tests.e2e.skill_eval --skill iris-connectivity --dry-run   # cost estimate, no spend
```

## One skill

```bash
export PYTHONPATH=.
python -m tests.e2e.skill_eval --skill iris-connectivity --runs 3
```

The preflight runs first: one small scoring call, then binary and tool-surface resolution. It costs
well under a cent and takes a few seconds. If it fails, nothing else runs and the message names the
variable to set.

Then the two arms run, and the table prints with the footer described in
`contracts/eval-run.md` — scorer model, tool surface, threshold, unscored share.

## Reading the output

- `scored 24/24` — every item got a verdict. This is the normal case.
- `scored 22/24` — two items could not be scored. Under the 10% limit, so the run is still valid,
  but the two are excluded from the pass rate rather than counted as failures.
- `run valid: no` — more than a tenth of items went unscored. Exit code 1. The numbers in the table
  are real for the items that scored, and the run is not fit to compare or to baseline.
- `Δ —` with `not comparable` — no comparison was made. The following line says which provenance
  field differs. This is the expected output for every skill on the first run after the rebaseline,
  because the stored numbers predate the repair.
- `tool surface A → B` beside a Δ — the comparison was made, and the tool surface moved. That is
  information, not a reason to ignore the Δ.

## Rebaselining

Only after a run comes back valid, with the binary resolved and the arms non-zero.

```bash
python -m tests.e2e.skill_eval --category objectscript --update-baseline --yes
```

The write merges by skill: entries for skills not in this run keep their values. Verify that, since
it is the bug this spec fixed:

```bash
git diff --stat tests/e2e/results/skill-baseline.json   # only the skills you ran should change
```

## Merging shards

The CI aggregate job does this; locally it is how you combine several partial runs.

```bash
python -m tests.e2e.skill_eval --merge-results 'tests/e2e/results/shard-*.json'
```

The threshold must be identical across every shard. A mismatch exits 1 rather than printing a table
where half the rows were judged under a different rule.

## Tests

```bash
pytest tests/e2e/skill_eval/ benchmark/021/tests/          # no network, no IRIS, seconds
```

Two live tests skip loudly rather than silently:

```bash
IAD_EVAL_LIVE_SCORER=1 pytest tests/e2e/skill_eval/test_preflight.py   # one real scoring call
IAD_EVAL_LIVE_AGENT=1 pytest tests/e2e/skill_eval/test_lift.py -k one_task   # one real session
```

Without those variables set they report `SKIPPED (set IAD_EVAL_LIVE_SCORER=1)`, which is a fact you
can read in the output. Constitution XI: a test that quietly passes without asserting anything is
worse than one that says it did not run.

## In CI

`.github/workflows/skill-regression.yml`, the `evaluate` job. Three things it needs that it did not
have:

1. `AWS_BEARER_TOKEN_BEDROCK` and `AWS_REGION` in the eval step's `env`. The secret exists in repo
   settings as of 2026-09-08; what is missing is the `env:` block that passes it through. `AWS_REGION`
   is a literal, not a secret. Today the step supplies `OPENAI_API_KEY` and nothing else, which is
   the credential the scorer does not use.
2. A step that downloads the released linux binary and exports `IAD_BINARY`. Without it the MCP
   server never starts and the skill arm runs with none of the 81 tools.
3. Nothing else. `anthropic` is already in the `pip install` line.

The `aggregate` job is the only place the baseline is written. Keeping it that way is what makes
merge-by-skill sufficient rather than needing a lock.
