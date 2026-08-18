# Benchmarking ObjectScript AI Skills

Run the repair benchmark yourself, measure your skills, and submit results to the leaderboard.

**Time required**: ~10 minutes for setup, ~5 minutes per skill run.

---

## Prerequisites

1. **The `iris-agentic-dev` binary**, a version that includes the `benchmark` subcommand —
   run `iris-agentic-dev benchmark --help` to confirm; if it errors with "unrecognized
   subcommand", grab the latest build from
   [releases](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
2. **Docker** — IRIS runs in a container
3. **An LLM API key** — `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`

No `git clone`, no Rust toolchain, no `pip install`, no separate Python MCP server — just
the prebuilt binary you already use for compile/execute/test:

```bash
# Mac (Homebrew)
brew tap intersystems-community/iris-agentic-dev
brew install iris-agentic-dev

# Mac direct download (Apple Silicon)
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-macos-arm64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
xattr -d com.apple.quarantine /usr/local/bin/iris-agentic-dev 2>/dev/null

# Linux x86_64
curl -fsSL https://github.com/intersystems-community/iris-agentic-dev/releases/latest/download/iris-agentic-dev-linux-x86_64 \
  -o /usr/local/bin/iris-agentic-dev && chmod +x /usr/local/bin/iris-agentic-dev
```

**Windows**: download `iris-agentic-dev-windows-x86_64.exe` from the
[releases page](https://github.com/intersystems-community/iris-agentic-dev/releases/latest)
and place it on your PATH.

Already have the binary installed? Check it's new enough:

```bash
iris-agentic-dev benchmark --help
```

If that errors with "unrecognized subcommand," reinstall using the commands above to pick
up the latest release.

---

## Quick Start — Run One Skill in 10 Minutes

```bash
# 1. Start the IRIS benchmark container
docker run -d --name iris-bench \
  -p 1972:1972 -p 52773:52773 \
  intersystemsdc/iris-community:latest
sleep 30   # wait for IRIS to finish starting before the next step

# 2. Get the SKILL.md files (only needed if you don't already have a local clone —
#    the harness itself needs no repository, only the skill file you want to test)
curl -sL https://raw.githubusercontent.com/intersystems-community/iris-agentic-dev/master/skills/skills/objectscript-review/SKILL.md \
  -o objectscript-review-SKILL.md

# 3. Run the benchmark with the top-ranked skill
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52773
export IRIS_GENERATE_CLASS_MODEL=claude-sonnet-4-6   # or any model generate.rs supports
export ANTHROPIC_API_KEY=sk-ant-...                   # or OPENAI_API_KEY for gpt-* models

iris-agentic-dev benchmark \
  --skill objectscript-review-SKILL.md \
  --baseline \
  --output results.json

# 4. See your results
cat results.json | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(f\"Pass rate: {d['pass_rate']:.0%} ({d['tasks_passed']}/{d['tasks_total']})\")
print(f\"Baseline: {d.get('baseline_pass_rate',0):.0%}\")
print(f\"Lift:     {d.get('lift',0):+.0%}\")
"
```

---

## CLI Dispatch Mode

In addition to the standard single-shot MCP mode, the benchmark supports a second
execution path: **CLI dispatch**. In this mode an agentic loop runs where the model
calls `iris-agentic-dev tool <name> --args '<json>'` as shell subprocesses, reads the
output, and iterates until it believes the task is solved. This mirrors how a developer
might wire `iris-agentic-dev` into a shell-based agent.

### When to use CLI dispatch

Use CLI dispatch benchmarking when:

- You are building an agent that shells out to `iris-agentic-dev` rather than connecting
  to it via MCP
- You want to compare subprocess-per-tool-call overhead against the MCP path
- You are measuring token cost difference between the two execution patterns

### Running CLI dispatch

```bash
iris-agentic-dev benchmark \
  --mode cli-dispatch \
  --skill path/to/your/SKILL.md \
  --output cli-dispatch-results.json
```

Additional flags:

| Flag                  | Default | Description                                             |
| --------------------- | ------- | ------------------------------------------------------- |
| `--max-iterations N`  | 10      | Maximum turns per task before recording `fail`          |
| `--max-task-tokens N` | 50000   | Stop if accumulated tokens per task exceed this         |
| `--compare path`      | —       | Load a prior result JSON and add a `comparison` section |

### CLI dispatch output

The result JSON includes the same `pass_rate`, `tasks_passed`, `tasks_total` fields
plus token counts and a mode discriminator:

```json
{
  "mode": "cli_dispatch",
  "pass_rate": 0.68,
  "tasks_passed": 15,
  "tasks_total": 22,
  "tasks_errored": 0,
  "tokens_input": 142000,
  "tokens_output": 38000,
  "tokens_total": 180000,
  "elapsed_s": 312.4,
  "task_results": [
    {
      "task_id": "jira-001",
      "outcome": "pass",
      "iterations": 3,
      "elapsed_s": 14.2,
      "tokens_input": 6200,
      "tokens_output": 1800,
      "tokens_total": 8000,
      "reason": ""
    }
  ]
}
```

`iterations` counts turns in the agentic loop (each turn is one LLM call). Token
fields are `null` when the model does not return usage data.

### Comparing modes

```bash
# 1. Run MCP mode first (this is the default)
iris-agentic-dev benchmark \
  --skill path/to/SKILL.md \
  --output mcp-results.json

# 2. Run CLI dispatch and compare
iris-agentic-dev benchmark \
  --mode cli-dispatch \
  --skill path/to/SKILL.md \
  --compare mcp-results.json \
  --output cli-results.json
```

The `--compare` flag appends a `comparison` section:

```json
{
  "comparison": {
    "other_mode": "mcp",
    "pass_rate_delta": -0.045,
    "tokens_total_delta": 158000,
    "elapsed_s_delta": 124.8
  }
}
```

### Interpreting CLI dispatch results

**Token cost**: CLI dispatch typically uses 5–15× more tokens than MCP single-shot mode
because each turn includes the full conversation history and tool results. The trade-off
is that the agent can gather information iteratively rather than working from static
context.

**Latency**: subprocess-per-tool-call adds process-spawn overhead per invocation (10–50ms
on a local machine). For tasks requiring 5–10 tool calls, expect 2–5 seconds of overhead
per task versus the network round-trip cost of MCP.

**Pass rate gap**: if CLI dispatch pass rate is significantly lower than MCP single-shot,
the most common causes are: (1) the skill is too short to guide multi-turn reasoning,
(2) the model is spending early turns on exploration rather than the fix, or (3) the
default `--max-iterations 10` is too low for complex tasks.

---

## Detailed Setup

### Step 1: Configure IRIS

The benchmark needs any reachable IRIS instance — Community Edition in Docker is the
easiest path. The harness talks to it entirely over Atelier REST (HTTP), the same
mechanism every other `iris-agentic-dev` tool uses — there is no Docker-exec dependency
for running tasks.

```bash
docker ps --filter "name=iris" --format "{{.Names}} {{.Ports}}"
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52773   # the container's mapped Atelier/web port
```

### Step 2: Configure LLM access

The harness reuses the same `LlmClient` used by `iris_generate_class`/`iris_generate_test`
— it supports Anthropic and OpenAI models directly (no AWS Bedrock support today):

```bash
# Anthropic
export IRIS_GENERATE_CLASS_MODEL=claude-sonnet-4-6
export ANTHROPIC_API_KEY=sk-ant-...

# OpenAI
export IRIS_GENERATE_CLASS_MODEL=gpt-4.1
export OPENAI_API_KEY=sk-...
```

### Step 3: Run the benchmark

```bash
# Basic run — with your skill, no baseline comparison
iris-agentic-dev benchmark --skill path/to/your/SKILL.md

# With baseline (runs twice — with skill AND without — shows lift)
iris-agentic-dev benchmark --skill path/to/your/SKILL.md --baseline

# Against a specific benchmark suite
iris-agentic-dev benchmark --skill path/to/your/SKILL.md --suite jira   # default, 22-task repair
# --suite mf and --suite sql are NOT YET PORTED — only jira is available in v1

# Save results to file
iris-agentic-dev benchmark --skill path/to/your/SKILL.md --baseline --output my_skill_results.json
```

---

## Understanding Results

```json
{
  "pass_rate": 0.7727272727272727,
  "baseline_pass_rate": 0.8636363636363636,
  "lift": -0.09090909090909094,
  "tasks_passed": 17,
  "tasks_total": 22,
  "tasks_errored": 0,
  "iris_version": "2026.1",
  "elapsed_s": 187.4,
  "task_results": [
    { "task_id": "jira-001", "outcome": "pass", "iterations": 1, "elapsed_s": 8.2, "reason": "" },
    { "task_id": "jira-002", "outcome": "fail", "iterations": 1, "elapsed_s": 3.9, "reason": "" }
  ]
}
```

`lift = pass_rate - baseline_pass_rate` (absolute difference — negative means the skill
underperformed the baseline on this run). `outcome` is `"pass"`, `"fail"`, or `"error"` —
an errored task (e.g. a tool-level failure unrelated to the fix itself) is excluded from
`pass_rate`'s denominator and reported separately in `tasks_errored`, so a stale task
never silently counts against your score.

**Interpreting lift:**

- `+15%` or higher → genuinely useful, submit to leaderboard
- `+5% to +15%` → useful for its specific domain, label as domain-specific
- `0% to +5%` → marginal, probably too broad or too narrow
- **Negative lift** → the skill is hurting on tasks where it isn't relevant; load on demand only, not globally

**Noise floor**: with only 22 tasks, each task is worth ~4.5 points of pass_rate. A single
LLM call per task is stochastic — the same skill run twice can land a few points apart with
no real change. Treat lift below ~±9% (roughly 2 tasks' worth) as noise, not signal, unless
you've run the suite multiple times and it holds up. If you're deciding between two skills
and the gap is inside that band, run `--baseline` a second time before trusting the number.

---

## Limitations

This is a small, homegrown suite, not a rigorous capability benchmark — read results
accordingly:

- **Contamination**: the 22 tasks are public, in this repo, on GitHub. If a model has seen
  this repo (or a fork/mirror of it) during training, it can pass tasks by recalling the
  fix rather than reasoning about the bug, inflating `pass_rate` for reasons that have
  nothing to do with your skill. This is the same failure mode documented for HumanEval
  ([Riddell et al., 2024](https://arxiv.org/html/2412.01526v1)) once its tasks became
  widely circulated. There's no mitigation for this today — a future version may hold back
  a private subset for spot-checking.
- **Single-run, no variance estimate**: each `pass_rate` here is one pass through the
  suite with one model call per task, not a mean over repeated runs. Model output is
  stochastic, so a single number overstates precision — see the noise-floor note above.
- **Single-model validation**: this harness has only been run end-to-end against one model
  family. A skill's lift on model A says nothing about its lift on model B — per
  [Anthropic's framing](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents),
  you're always evaluating harness + model together, never the model (or skill) in
  isolation.
- **Task selection is not systematic**: all 22 tasks came from one task-generation pass,
  not a curated, diversity-checked sample the way
  [SWE-bench Verified](https://www.swebench.com/verified.html) is human-filtered for
  representativeness. Expect gaps in bug-type coverage.

None of this means the numbers are useless — a `+20%` lift that survives a couple of
reruns is real signal. It means don't cite a single run's `pass_rate` as a general claim
about a skill's quality, and don't be surprised if results don't fully replicate on a
different model or a larger, less contamination-prone suite.

---

## Writing a Skill That Will Score Well

The data is clear: **shorter hard-gate checklists beat long reference documents**.

| Design                             | Example                      | Score    |
| ---------------------------------- | ---------------------------- | -------- |
| 205-word hard gate checklist       | `objectscript-review`        | **100%** |
| 268-word all-in-one                | `iris-light-slim`            | 86%      |
| 472-word pattern reference         | `objectscript-list-patterns` | 91%      |
| 5,170-word comprehensive reference | `iris-light`                 | 21%      |

### The RED-GREEN methodology

**RED**: Run with `--baseline` first and inspect `task_results` for tasks where
`outcome` is `"fail"` — those are the gaps your skill needs to close.

```bash
iris-agentic-dev benchmark --skill /dev/null --baseline --output baseline.json
```

**GREEN**: Write a skill that addresses the specific failure patterns you observed.

**REFACTOR**: Run benchmark again. If pass rate dropped on some tasks, your skill is too broad — narrow it.

### Skill format

```yaml
---
name: "yourgithub/your-skill-name"
description: "Use when [narrow trigger conditions]"
iris_version: ">=2024.1"
tags: [objectscript]
author: yourgithub
state: draft                    # set to "reviewed" automatically when >= 80%
---

# Your Skill Title

## HARD GATE

Do not show code until this passes.

- [ ] Rule 1
- [ ] Rule 2
...

## Output Format

If violations: > ⚠️ [N] issues found: ...
If clean: > ✅ Passed.
```

### Rules that make skills work

1. **Description = "Use when..." only** — if you summarize the workflow, the model follows the description and skips the body
2. **Hard gate = checkboxes, not prose** — `- [ ] Check X` is read; a paragraph is skimmed
3. **< 300 words for general skills** — models skim long context; your checklist gets ignored
4. **One pattern per skill** — a skill for `$Order` loops is better than one for "all loop patterns"

---

## Submitting to the Leaderboard

### What we accept

- Skills with measured benchmark results (pass rate + baseline + lift)
- Skills that improve on at least one suite
- Skills with a narrow, specific trigger description
- Skills that are self-contained (no external references required)

### What we note but still accept

- Skills with negative lift on the repair suite — labeled "domain-specific, load on demand"
- Skills that score well on SQL/MF but not repair — different suites, different value

### PR format

Open a PR to [intersystems-community/iris-agentic-dev](https://github.com/intersystems-community/iris-agentic-dev) with:

1. Your skill file at `skills/skills/yourgithub/your-skill/SKILL.md`
2. PR description including:

```markdown
## Skill: yourgithub/your-skill-name

**Suite**: jira
**Pass rate**: XX%
**Baseline**: XX%
**Lift**: +XX%
**IRIS version**: 2026.1
**Model**: claude-sonnet-4-6
**Words**: NNN

### What this catches that other skills don't

[One paragraph]

### Benchmark output

[Paste your results.json]
```

---

## Additional Suites (Not Yet Ported)

The `mf` (multi-file repair) and `sql` (IRIS SQL quirks) suites from the original
research are explicitly deferred — v1 ports only the primary `jira` repair suite that
this Quick Start exercises. `--suite mf` / `--suite sql` will error with
`SUITE_NOT_AVAILABLE` until a future contribution ports them.

---

## Troubleshooting

### No IRIS connection found

```bash
docker run -d --name iris-bench -p 1972:1972 -p 52773:52773 \
  intersystemsdc/iris-community:latest
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52773
```

### LLM authentication failed

```bash
# Anthropic:
export ANTHROPIC_API_KEY=sk-ant-...

# OpenAI:
export OPENAI_API_KEY=sk-...
```

Also confirm `IRIS_GENERATE_CLASS_MODEL` is set to a model name your key has access to.

### `SUITE_NOT_AVAILABLE`

- Only `--suite jira` (the default) is available in v1 — `mf`/`sql` are not yet ported.

### `BENCHMARK_RUN_IN_PROGRESS`

- Another benchmark run is already active against the same IRIS host. Wait for it to
  finish; a run older than `--max-time-s` (default 600) is treated as abandoned and
  automatically overridden on the next attempt.

### Tasks time out

- The LLM call is slow — raise `--task-timeout-s` (default 30) or `--max-time-s`
  (default 600), or switch to a faster model.

---

## Benchmark Task Format

Each task is a JSON file under
`crates/iris-agentic-dev-core/src/benchmark/tasks/jira_bugs/`:

```json
{
  "task_id": "jira-001",
  "category": "jira_bugs",
  "difficulty": "easy",
  "description": "Fix null pointer error when processing empty patient records",
  "goal": "Add $IsObject check before accessing object properties",
  "initial_code": {
    "files": [{ "path": "src/X.cls", "content": "...buggy code..." }]
  },
  "test_code": { "path": "tests/TestX.cls", "content": "...test that fails on bug..." },
  "hints": [],
  "expected_behavior": "...",
  "success_criteria": {
    "compile_success": true,
    "tests_pass": true,
    "max_patch_lines": 30,
    "requires_symbol_preservation": true
  }
}
```

`test_code` classes extend `%RegisteredObject` with `ClassMethod`/`Method`s named
`TestXxx` (not `%UnitTest.TestCase`) — the harness invokes each `Test*` method directly
and treats an uncaught exception as a failure, matching this schema's existing
convention.

**Adding new tasks**: tasks must:

1. Compile on buggy code (syntax errors are a different skill test)
2. Fail the test on buggy code
3. Pass the test on the correct fix
4. Be self-contained (no external class dependencies)

Current suite:

- `crates/iris-agentic-dev-core/src/benchmark/tasks/jira_bugs/` — 22 single-function
  repair tasks (the only suite ported in v1)

---

## Questions?

File issues at [intersystems-community/iris-agentic-dev](https://github.com/intersystems-community/iris-agentic-dev/issues).
