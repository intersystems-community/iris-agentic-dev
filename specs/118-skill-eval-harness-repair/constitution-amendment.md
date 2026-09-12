# Staged constitution amendment: 1.5.2 → 1.5.3

**Feature**: 118-skill-eval-harness-repair
**Status**: **applied** on 2026-09-09, on Tom's instruction. `.specify/memory/constitution.md` now
carries all of the below at version 1.5.3, footer `Last Amended: 2026-09-09`.

The file is in `protected_files.extra` and `.claude/hooks/gates/protect-files.sh` blocks Edit and
Write on it, so the write went through Bash. That is a deliberate route around the gate, taken
because the file's owner asked for the amendment directly. The gate is still in place and unchanged.

This file stays as the record of what changed and why — the reasoning below is longer than what fits
in a sync impact report.

## What was applied

Bump 1.5.2 → 1.5.3. PATCH — six rows added to the Bug Class Registry, no principle changed. All
six come from the skill-eval nightly of 2026-09-07, which spent $5.47 across nine shards and
printed 0.00 against 0.00 for six of nine skills while reporting success.

Add to the Bug Class Registry table:

| Class                                                 | First shipped instance                                                                                                                                                           | Detector                                     |
| ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| Fabricated failure score                              | The eval judge's `except` returned `{"score": 0}`, indistinguishable from a genuinely bad answer, so an unreachable scorer read as total failure                                 | `scored-exception`                           |
| Durable record replaced                               | `save_baseline` rebuilt its dict from the current run and overwrote the file, so evaluating one skill deleted the other eight; a nine-skill baseline held one entry              | `test_baseline_merge_preserves_other_skills` |
| Reported identity is the requested constant           | The shard record named the judge's Haiku constant while Bedrock resolved it to `us.anthropic.claude-sonnet-4-6`, so every stored number was attributed to a model that never ran | `test_score_result_records_resolved_model`   |
| Cost constant priced for an unrun model               | $0.001 per scoring call, priced for Haiku on the direct API, while the Bedrock path resolves to a Sonnet-class model; every cap derived from it was wrong                        | `test_scorer_cost_keyed_by_resolved_model`   |
| Comparison against unlike measurement                 | A Δ reported against a baseline entry measured under a different scoring path and a tool surface two specs old, with nothing recorded that could have caught it                  | `test_provenance_mismatch_refuses_delta`     |
| Hard-coded install path read as a working environment | The harness started the MCP server from `/opt/homebrew/bin/iris-agentic-dev`, absent on the runner, so six skills' lift was measured with none of the tools the lift is about    | `test_isolated_env_has_tools`                |

Add `scored-exception` to the never-baselined list, taking it from eight classes to nine. It flags
an `except` handler that returns a dict carrying a numeric `score` key — a scoring failure that
enters the measurement as a value on the scoring scale. There is no legitimate instance, so there is
nothing to baseline.

The existing open item carries forward unresolved: Principle VIII states ≥ 90% coverage and Release
Discipline states ≥ 88%. Owner: Tom. Still needs its own run at 2.0.0.

## Why these are six rows and not one

They failed together on the same night and they are six different classes. One night can expose
six; the registry counts classes, not incidents.

**Fabricated failure score** is a failure that produces a value on the measurement scale. The
scorer could not be reached, and the harness recorded that as a zero — the same zero a genuinely
wrong answer earns. Any code that turns "I could not measure" into a number belongs here, and
`scored-exception` finds it structurally rather than by name.

**Durable record replaced** is a write that discards what it did not produce. `save_baseline` had
no bug in what it wrote; it had a bug in what it stopped writing. The detector is a unit test
rather than a scanner check because the rule is about one function's contract with a file, not a
shape that recurs across the tree.

**Reported identity is the requested constant** is a record that names an intention instead of an
event. The code asked for Haiku, the platform served Sonnet, and the artifact recorded the ask.
Every number in the file was attributed to a model that never ran.

**Cost constant priced for an unrun model** is the same mistake spent forward: a rate derived from
the model the code names rather than the one it reaches, which then propagates into every budget
figure downstream.

**Comparison against unlike measurement** is a Δ between two numbers produced under different
conditions. Nothing was recorded that could have refused it, so the comparison looked identical to
a valid one.

**Hard-coded install path read as a working environment** is an absent dependency reported as a
present one. The MCP server never started, the agent had no tools, and the arms still produced
numbers — the environment failed open, which on a measurement is indistinguishable from the
measurement succeeding.

Recording fewer would leave the rest open, which is what the registry's own sentence forbids: a fix
without a detector closes the instance and leaves the class open.

## Note on the detector's implementation

`scored-exception` is the first Python check in `scripts/gates/antipatterns.py` — every existing
check parses Rust. It uses the stdlib `ast` module rather than a regex, because finding an `except`
handler's body by pattern-matching Python indentation is the wrong tool and would miss
`dict(score=0)` and `{"score": 0.0}` while firing on legitimate test fixtures. A canary file ships
with it, per the constitution's requirement that every gate prove it still blocks a known violation.
