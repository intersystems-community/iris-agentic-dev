# Contract: scoring and exit codes

**Feature**: 118-skill-eval-harness-repair

Two consumed interfaces: what a scoring call returns, and what the process's exit code means.

## Scoring call

`score_result(task, result) -> dict`

The dict is the Scored-item payload from `data-model.md`, minus the fields the caller fills in
(`arm`, `run_index`):

```json
{
  "scored": true,
  "score": 2,
  "reasoning": "Used iris_query with an explicit namespace; missed the schema qualifier.",
  "scoring_mode": "judge",
  "scorer_model": "claude-sonnet-4-6",
  "input_tokens": 1180,
  "output_tokens": 96
}
```

An unscorable item:

```json
{
  "scored": false,
  "score": null,
  "reasoning": "scorer unreachable: no Bedrock credential and no API key in environment",
  "scoring_mode": "judge",
  "scorer_model": null,
  "input_tokens": null,
  "output_tokens": null
}
```

Guarantees:

- `score` is `null` whenever `scored` is `false`. Never `0`.
- `score` is one of `0, 1, 2, 3` whenever `scored` is `true`. A response that parses as JSON and
  carries anything else — `7`, `-1`, `"good"`, a float — is a scorer failure and returns the
  unscored shape. An out-of-range number is the same lie as a fabricated zero: it lands on the
  scoring scale without having been measured.
- The function does not raise for a scorer failure. It raises only for a programming error —
  a malformed task, a missing fixture — which is a bug in the caller, not a measurement outcome.
- `reasoning` on a failure names what was missing or what the scorer said, not `str(e)` alone.
- `scorer_model` comes from the scoring response, never from the requested constant.

Assertion and pattern modes return the same shape with `scorer_model: null` and no token counts;
they are deterministic and always `scored: true`.

## Pass rate

`compute_pass_rate(items) -> float | None`

- Denominator is the count of items with `scored is true`.
- Returns `null` when that count is zero. Not `0.0`.

## Run validity

A run is invalid when the share of unscored items across all skills and arms exceeds **10%**.
Invalid runs still write their shard file — the counts are the evidence — and still print the
table, with every affected row marked. They exit non-zero.

## Exit codes

| Code | Meaning                                                                          |
| ---- | -------------------------------------------------------------------------------- |
| 0    | The run measured what it set out to measure. Regressions may be reported         |
| 1    | The run's integrity failed: unscored share over the limit, or threshold mismatch |
| 2    | Preflight failed: the scorer is unusable, or a required argument is missing      |

Code 2 means nothing was spent. Code 1 means money was spent and the numbers cannot be trusted;
that distinction is the point of having two codes.

A reported regression does not set the exit code in this spec (research R12). The signal has never
fired, so its false-positive rate is unmeasured; it becomes blocking after a night of real numbers,
in a separate change.

## Preflight

`preflight(args) -> PreflightResult`

Runs after argument parsing, before the first agent session. Skipped for `--list-skills`,
`--dry-run`, and `--merge-results`.

Checks, in order:

1. **Corpus valid.** The existing task-corpus validation, moved inside the preflight rather than
   left beside it. It is the cheapest check and the only one that costs nothing, so it runs first.
   On failure: exit 2, naming the task and the file. Extending it to namespaces a task names and
   the harness never creates is FR-016, deferred.
2. **Scorer reachable.** Exactly one real scoring call against a fixed two-line prompt — one, not
   a retry loop, so a broken credential costs one call and not nine. Must return a parseable
   verdict. On failure: exit 2, message naming which credentials were looked for and which were
   present.
3. **Binary resolved.** `IAD_BINARY` → `PATH` → `/opt/homebrew/bin/iris-agentic-dev`. On failure:
   exit 2, naming the three places searched.
4. **Tool surface resolved.** `iris-agentic-dev tool --list --json` and `--version`. Needs no IRIS.

Returns the resolved scorer model, the resolved binary path, and the tool-surface revision. All
three go into the run's provenance.

The order is cheapest-first and is part of the contract: a run with both a broken corpus and a
broken credential must report the corpus, because fixing the credential would not have helped.

Failure output names the fix, not just the symptom:

```text
preflight: scorer unreachable
  looked for: AWS_BEARER_TOKEN_BEDROCK (absent), CLAUDE_CODE_USE_BEDROCK (absent),
              AWS_ACCESS_KEY_ID (absent), ANTHROPIC_API_KEY (absent)
  present but unused: OPENAI_API_KEY
  the scorer runs on Bedrock; set AWS_BEARER_TOKEN_BEDROCK and AWS_REGION
  nothing was spent
```
