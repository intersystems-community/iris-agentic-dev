# Lift run for spec 173 (T065): does the skill help an agent use `propose`?

Run on 2026-11-27 (ET) on the laptop, against `iris-dev-iris` (IRIS Community 2026.2, namespace `BENCHMARK`).

## Result

On the one task, the agent with the `iris-storage-mapping` skill passed 5 of 5 sessions. The agent with tools only
passed 3 of 5. The ladder reports this as **underpowered** (b=1, c=0, n=1 pair), so it is not evidence of a lift. It
is a pass rate on one task.

| Arm                          | Sessions scored | Passed | Rate | 95% interval (Wilson) |
| ---------------------------- | --------------- | ------ | ---- | --------------------- |
| `tools`                      | 5               | 3      | 0.60 | 0.23 to 0.88          |
| `tools+iris-storage-mapping` | 5               | 5      | 1.00 | 0.57 to 1.00          |

One task makes one pair: the ladder reduces the five repeats of an arm to a single verdict per task, so the
paired comparison has n=1. A real lift claim needs many tasks. This run only shows that the skill arm did not fail on
SKILL-74, and that it finished faster.

## The two tools-arm failures are not the same kind

- **Repeat 0 (genuine).** 132 calls and the 300 s clock. Every iad call answered. The agent found
  `iris_storage_map`, ran `read` and `check`, then spent most of its calls searching documentation and never left a
  working map. In the earlier one-repeat diagnostic run the tools arm failed the same way (102 calls, 300 s).
- **Repeat 4 (harness stall).** 5 calls and the 300 s clock. `check_config`, `iris_doc` and the other iad calls all
  returned `MCP error -32001: Request timed out`, and later calls `-32000: Connection closed`. The agent never had a
  working tool server. The ladder flagged the same fault on two skill-arm attempts and re-ran them, but it did not
  flag this one, so it counts as a failure. With this session left out the tools arm is 3 of 4.

## Time and cost per session (passing sessions)

| Arm                          | Calls (each passing session) | Seconds            |
| ---------------------------- | ---------------------------- | ------------------ |
| `tools`                      | 31, 27, 44                   | 114, 88, 142       |
| `tools+iris-storage-mapping` | 20, 15, 16, 23, 16           | 56, 71, 66, 64, 69 |

The skill arm used 15 to 23 calls and about a minute. The tools arm used 27 to 44 calls and 88 to 142 seconds when it
passed. Measured spend for the ten scored sessions is about $0.21. The stalled and diagnostic attempts below add about
$0.12, so the whole exercise cost about $0.33 against the $3 cap.

## What went wrong along the way

1. **First attempt, with `nice -n 15`.** Both sessions timed out after 1 and 4 calls, with every iad call after the
   first returning `Request timed out`. The laptop load average was 38 (XDR, Defender, OrbStack and another session's
   `rustc`). I stopped the run and dropped `nice` for the ladder. `caffeinate` kept the machine awake.
2. **Stalls without `nice` too.** Two skill-arm attempts (12 and 4 calls) and one tools attempt (5 calls) stalled the
   same way. IRIS answered Atelier requests in about 11 ms, the binary starts in about 1.1 s, and a direct stdio call
   of `check_config` under the same empty `HOME` worked. I did not find the cause. The load on the laptop is the
   likely one, but I did not show it.
3. **Diagnostic run (1 repeat).** Tools failed (102 calls, 300 s), skill passed (19 calls, 52 s). Kept as
   `tests/e2e/results/lift-173-luna-diagnostic-r1.json`.

## What this does and does not show

- The skill arm loaded its skill in all five sessions, which the ladder checks.
- The model is `bedrock-oai/global.openai.gpt-6-luna`, effort high. Earlier studies found this model loads skills when
  they are offered, and that it passes most tasks without a skill, so there is little room for a skill to add. Here the
  tools arm still failed once on a task where the agent has to make a legacy index global usable by SQL.
- One task, one model, one run of five. It says nothing about other tasks or other models.
- The tools arm's genuine failure came from the agent not finding the way to a working map, not from a wrong answer
  from `propose`. The skill's value on this task is the route it names.
- SKILL-74 asks the agent to map a legacy index global (`^IadSm173PersonX`) so lookups by name use it. It ran
  on the merged tree at `36f6dc19` with the debug binary (`tool_surface 1.7.1+40d9f72492c5`), after the iFind fixes.
- The run was not a timing benchmark, so Community on a loaded laptop is acceptable. The stalls make absolute times
  unreliable.

## Files

- `/Users/tdyar/ws/iris-agentic-dev/.iad-local/wt-172/tests/e2e/results/lift-173-luna.json` (report)
- `/Users/tdyar/ws/iris-agentic-dev/.iad-local/wt-172/tests/e2e/results/ladder-20261127T113713.runs.jsonl` and
  `.transcripts/` (the ten scored sessions)
- `/Users/tdyar/ws/iris-agentic-dev/.iad-local/wt-172/tests/e2e/results/lift-173-luna-diagnostic-r1.json` (the one-repeat
  diagnostic)
- No `needs_fix` flag was raised for the skill.
