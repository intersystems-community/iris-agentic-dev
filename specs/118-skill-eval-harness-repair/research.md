# Phase 0 research: skill-eval harness repair

**Date**: 2026-09-08 | **Feature**: 118-skill-eval-harness-repair

Every decision below was checked against the code or against a live call. Where I ran something,
the command and its result are recorded, because the whole point of this spec is that the harness
reported numbers nobody had checked.

## R0 — The nightly had two causes, not one

**Finding**: the six zero rows have two independent, sufficient causes.

Cause A, the one the spec names. `benchmark/021/runner/judge.py` ends its retry loop with:

```python
except Exception as e:
    if attempt == 1:
        return {"score": 0, "reasoning": f"Judge error: {e}"}
```

A scorer that cannot be reached returns a passing-scale zero, indistinguishable from a genuinely
bad answer. `benchmark/021/runner/_client.py` needs either a Bedrock signal
(`CLAUDE_CODE_USE_BEDROCK`, `AWS_BEARER_TOKEN_BEDROCK`, `AWS_ACCESS_KEY_ID`) or `ANTHROPIC_API_KEY`.
`.github/workflows/skill-regression.yml` passes `OPENAI_API_KEY` and nothing else. So every scoring
call in CI raised, twice, and returned zero.

Cause B, found during this research and not in the spec. `tests/e2e/isolated_env.py:48`:

```python
"command": ["/opt/homebrew/bin/iris-agentic-dev", "mcp"],
```

That path is a macOS Homebrew path. On `ubuntu-latest` it does not exist, and the workflow has no
step that installs the binary. The MCP server never started, so the skill arm ran with none of the
81 iad tools.

**Why the spec could not separate them**: I surveyed all nine `eval.yaml` files. Three set
`no_mcp_for_benchmark: true` (iris-ai-hub, iris-vector-ai, ensemble-production) and are scored by
compile-and-pattern matching, so they touch neither the judge nor the MCP server — and those three
are exactly the three that reported non-zero. The other six leave MCP on and carry no
`tool_assertions`, so they hit both defects. Both causes predict the same six rows. Correlation
could never distinguish them; reading the code could.

**Consequence**: fixing only the scorer would produce nine confident, provenance-stamped, useless
baseline entries — measured against an agent with no tools. Both fixes land before the rebaseline.

## R1 — Bedrock auth: bearer token only, no boto3

**Decision**: keep `AnthropicBedrock` and supply `AWS_BEARER_TOKEN_BEDROCK` + `AWS_REGION` in CI.

**Verified live** with a scratch script holding only those two variables in the environment:
`AnthropicBedrock(aws_region=...)` constructed and `messages.create` returned a completion. No
boto3 import, no SigV4 signer, no credential chain.

**Rationale**: this is one repo secret. Constitution VII (dependency minimalism) is satisfied for
free, and the failure mode we are fixing does not recur through a new dependency.

**Alternatives**: GitHub OIDC to an IAM role — correct long-term posture, but it needs a trust
policy, a role, and an AWS-side change to land before any of this spec can be tested. Rejected for
this spec, not on the merits. `ANTHROPIC_API_KEY` — a second credential to rotate for no gain, and
it would diverge from the laptop path.

**Status**: done. `AWS_BEARER_TOKEN_BEDROCK` was added to repo settings on 2026-09-08. The remaining
work is inside the branch: the eval step's `env` block still passes only `OPENAI_API_KEY`, so the
secret is present and unused.

## R2 — The scorer's identity comes from the response

**Decision**: record `msg.model` from the scoring response, never the requested constant.

**Verified live**: requesting `us.anthropic.claude-sonnet-4-6` returns `msg.model ==
"claude-sonnet-4-6"`. The request names an inference profile; the response names the model.

**Rationale**: FR-004 wants the model that scored the run. The requested constant is a wish, and
`_client.py` already carries the comment `# haiku-4-5 unavailable on this account`, which is
evidence that the constant and the reality have diverged before. `__main__.py` currently reports
`judge_model="openai/gpt-4.1"`, which is neither.

**Alternatives**: report the constant (what the code does now, and it is wrong); report both
(kept — the constant appears as `scorer_model_requested`, the response as `scorer_model`, and a
mismatch is printed once per run, not per item).

## R3 — Failure returns a verdict, it does not raise

**Decision**: `score_result` returns `{"scored": False, "score": None, "reasoning": ..., ...}`.
The caller records the item and keeps going. The aggregate check decides the run's fate.

**Rationale**: a raise on the first failed scoring call throws away the rest of that skill's
measurement, including the arm that might have succeeded. FR-003's gate is an aggregate — more
than one item in ten unscored invalidates the run — so it needs the whole run's counts to
evaluate, which means the loop has to finish. This also keeps the transient-blip case (one 429)
from destroying a $5 run.

**Alternatives**: raise immediately (loses data, and cannot express "9 of 10 scored fine");
retry forever (unbounded spend); keep returning zero (the bug).

## R4 — Pass rate over scored items only

**Decision**: `passed / items_scored`, with `items_scored` and `items_unscored` recorded per arm.
An arm with zero scored items has no pass rate — `None`, not `0.0`.

**Rationale**: `compute_pass_rate` currently divides by `len(scores)`, so an unscored item is a
failure. That is the arithmetic that turned a credential problem into a 0.00 pass rate. Dividing by
the scored count makes an unscored item absent rather than failed, and the count makes its absence
visible.

**Alternatives**: impute the arm's mean (invents data); count unscored as failures (status quo).

## R5 — Preflight makes one real call

**Decision**: a new `preflight.py` runs after argument parsing and before the first agent session.
It makes one real scoring call against a fixed tiny prompt and resolves the binary and tool surface.
Either failing exits non-zero with the missing variable named. Skipped for `--list-skills`,
`--dry-run`, and `--merge-results`, which spend nothing.

**Rationale**: the failure was a credential that was present in name and unusable in fact. Only a
real call distinguishes those. Cost is one small completion, well under a cent, against the $5.47
the last broken nightly spent.

**Alternatives**: check env vars are non-empty (effectively what existed; `OPENAI_API_KEY` was
present and irrelevant); score one item and check it (the arms cost dollars, the preflight should
cost cents).

## R6 — Binary resolution and the tool-surface revision

**Decision**: resolve the MCP command as `IAD_BINARY` → `shutil.which("iris-agentic-dev")` →
`/opt/homebrew/bin/iris-agentic-dev`, and derive

```text
tool_surface = f"{version}+{sha256(",".join(sorted(names)))[:12]}"
```

from `iris-agentic-dev tool --list --json`.

**Verified live** against the installed 1.4.0 binary: `tool --list --json` returns
`{"count": 81, "tools": [...]}` and needs no IRIS connection — that is what spec 114 built it for.
Version comes from `--version`.

**Rationale**: FR-007 wants the tool surface the agent saw recorded in the baseline. A version
string alone cannot see a tool added on a branch; the name set can. The hash is short because it is
an identity, not a payload. `tool --list` costs one process spawn and no IRIS.

**Alternatives**: version string alone (blind to unreleased changes); the full name list in the
baseline file (81 strings per entry, times nine, for something only ever compared for equality);
hashing the full schemas (spec 113 changes them constantly, so it would flag on every schema tweak
and mean nothing).

## R7 — Baseline v2: envelope plus merge-by-skill

**Decision**:

```json
{ "schema": 2, "skills": { "<name>": { ... , "provenance": { ... } } } }
```

`save_baseline` reads the file, replaces only the skills present in this run, and writes back. A v1
file (a bare map of skill names) is migrated in memory on read; entries from it are marked
comparable-unknown, because they carry no provenance.

**Rationale**: `save_baseline` currently builds `data = {}` from the current run and overwrites the
file, so evaluating one skill deletes the other eight — which is why a nine-skill baseline file has
one entry in it. Provenance needs a home, and a reader needs to know which shape it holds; a
version field is the cheapest way to say so.

On the v1 entries: FR-011 rebaselines all nine anyway, so the migration path is exercised once and
then never matters. It exists so the first post-fix run does not crash on the file already on disk.

**Alternatives**: add provenance keys to the flat map (no way to distinguish a v1 entry from a v2
entry with fields missing — exactly the question "is this comparable" asks); a separate provenance
file (two files to keep in sync, and nothing forces it); rewrite v1 on disk at read time (a read
that mutates the durable record, for a file about to be replaced).

## R8 — Comparability: refuse the Δ, or annotate it

**Decision**: three fields make a Δ meaningless and suppress it — the task id set, the scoring
mode, and the resolved scorer model. A changed tool-surface revision prints beside the Δ and does
not suppress it.

**Rationale**: change the task set and you have measured a different thing. Change the scoring mode
and the scale is not the same scale. Change the scorer and the calibration moved. But a changed
tool surface is the thing the harness exists to detect — suppressing on it would silence the gate
on precisely the releases that need it, which is FR-008's "advisory".

**Alternatives**: suppress on any provenance difference (silences the gate on every release);
suppress on none (the 0.00-vs-0.58 comparison this spec exists to prevent).

## R9 — A detector for the class, in Python

**Decision**: add a `scored-exception` check to `scripts/gates/antipatterns.py`, using the stdlib
`ast` module: flag any `except` handler whose body returns a dict literal containing a `score` key
with a numeric value. Ship a canary file that the check must flag.

**Rationale**: constitution clarification prompt 9 requires a detector per shipped bug class, and
the never-baselined list means this one cannot be silenced with a baseline entry. Every check in
`CHECKS` today parses Rust; this is the first Python one, so it uses `ast` rather than a regex —
regexes over Python indentation to find an `except` body would be the wrong tool.

The Bug Class Registry row lives in `.specify/memory/constitution.md`, which
`.claude/hooks/gates/protect-files.sh` blocks. Same situation as spec 113 and 117: the amendment is
staged as `constitution-amendment.md` in this directory for Tom to apply via
`/speckit.constitution`. I am not bypassing the hook.

**Alternatives**: a grep for `"score": 0` (misses `{"score": 0.0}`, `dict(score=0)`, and a variable
holding zero, and fires on legitimate test fixtures); a unit test only (catches this instance,
not the next tool that copies the pattern).

## R10 — No new dependency

**Decision**: nothing new gets installed.

`anthropic` is already in the eval job's `pip install` line. `ast`, `hashlib`, `shutil`,
`subprocess`, and `json` are stdlib. boto3 is unnecessary per R1.

**Alternatives**: boto3 (R1 shows it is not needed); `tiktoken` for token estimates (R11 measures
actual usage instead, which is better and free); `pydantic` for the baseline schema (the harness
uses dataclasses throughout; one module in a different idiom is worse than a hand-written check).

## R11 — Cost from measured usage, not a guess

**Decision**: capture `msg.usage.input_tokens` / `output_tokens` on each scoring call, and derive
the per-call cost from a declared price table keyed by the resolved model.

**Verified live**: `msg.usage` returns both fields on the Bedrock path.

**Rationale**: `cost_estimator.py` carries `_HAIKU_COST = 0.001  # ~$0.001 per judge call` and the
scorer is not Haiku — `_client.py` says haiku-4-5 is unavailable on this account and requests a
Sonnet-class model instead. A Sonnet-class call is roughly an order of magnitude more, so the
dry-run estimate is low by about that much, and the label "Judge calls (Haiku)" is wrong on its
face. Measured tokens times a declared rate is checkable; a constant with a `~` in its comment is
not. FR-023 asks for exactly this.

The Bedrock per-token rate has to be confirmed at implementation time and cited in the table — I am
not writing a price into the repo from memory.

**Alternatives**: bump the constant to a bigger guess (same class of bug, new number); drop the
dry-run estimate (it is the only thing standing between a `--runs 5` typo and a large bill).

## R12 — Exit codes: the run's integrity, not the skills' scores

**Decision**: in 118 the process exits non-zero for exactly three reasons — preflight failed
(FR-001), too large a share of items went unscored (FR-003), or the threshold the report printed
does not match the threshold the outcomes were computed under (FR-010). `regression_flag` is
reported truthfully and does not by itself set the exit code.

**Rationale**: the gate has never fired on a regression, so nobody knows its false-positive rate,
and the compound rule in `evaluator.py`

```python
result.regression_flag = delta < -threshold and (result.lift <= 0 or delta < -0.20)
```

is not the rule the spec describes. Turning an untrusted signal into a red build on the first night
of a repaired harness would train everyone to ignore it. Story 2 makes the signal honest and
visible; making it blocking is a decision to take once there is a night of real numbers to look at.

The three that do fail the build are all statements about whether the run measured anything, which
is the failure the last one got wrong.

**Alternatives**: fail on any regression (the untrusted-signal problem above); fail on nothing (a
broken preflight would print a warning and score zeros again).

## R13 — Where the new code goes

**Decision**: three new modules — `scoring.py`, `preflight.py`, `provenance.py` — rather than
growing `lift.py`.

**Rationale**: `lift.py` is 364 lines and already holds transcript formatting, fixture loading, two
scoring paths, and the run loop. Each of the three new concerns has more than one caller:
`scoring.py` is used by `lift.py` and `evaluator.py`; `preflight.py` by `__main__.py` and its own
test; `provenance.py` by `lift.py`, `baseline.py`, and `reporter.py`. Twenty-five flat modules with
tests beside them is the existing shape of this directory, and this keeps it.

**Alternatives**: one `integrity.py` holding all three (couples the preflight's subprocess handling
to the pass-rate arithmetic for no reason); inline in `lift.py` (a 550-line module and a circular
import the moment `baseline.py` needs the provenance record).
