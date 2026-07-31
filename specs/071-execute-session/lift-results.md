# Lift Results: iris_execute Session State (071)

## Run

- **Date**: 2026-07-30
- **Harness**: claude-code, toolset=merged
- **Tasks**: SES-01, SES-02 (both paths A and B)
- **IRIS**: 2026.2, localhost:52780
- **Results**: `benchmark/021/results/2026-07-30T23-02-04Z/`

## Scores

| Task   | Path | Score | Reasoning summary                                                          |
| ------ | ---- | ----- | -------------------------------------------------------------------------- |
| SES-01 | A    | 2/3   | Correct output, did not use `use_session`/`%ctx` — hardcoded value instead |
| SES-01 | B    | 2/3   | Same — hardcoded `549`, used `iris_query` not `iris_execute`               |
| SES-02 | A    | 1/3   | Correct output via global workaround; did not use session token mechanism  |
| SES-02 | B    | 1/3   | Correct output via global workaround; re-opened object in second call      |

Average: 1.5 / 3.0

## Analysis

The model solves the underlying problem but reaches for familiar workarounds (hardcoded
intermediates, named globals) rather than the `use_session` / `%ctx` mechanism. This is
a tool description and prompt signal problem, not a capability problem — the model can
use `use_session` correctly when told to, as the e2e tests prove.

Factors contributing to the low score:

1. The `iris_execute` tool description now documents `use_session` and `%ctx`, but the
   model has not seen enough examples of this pattern to reach for it spontaneously.
2. SES-02 requires `Ens.MessageHeader` which may not exist on all IRIS instances — the
   model may be defending against `%OpenId` failure.

## Baseline comparison

No pre-implementation baseline exists (the feature did not exist before this branch).
The 1.5/3 score is the first measurement and becomes the floor for future tool description
improvements.

## Next steps

- Improve the `iris_execute` tool description with a concrete multi-call example showing
  the `use_session` pattern end-to-end.
- Consider adding a `%ctx` usage note to the skill pack (objectscript-tdd skill or new
  session-state skill) so the model has retrieval-time guidance.
- Rerun after description changes to measure improvement.
