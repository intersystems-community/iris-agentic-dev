# Benchmarking ObjectScript AI Skills

Run the repair benchmark yourself, measure your skills, and submit results to the leaderboard.

**Time required**: ~10 minutes for setup, ~5 minutes per skill run.

---

## Prerequisites

1. **Docker** — IRIS runs in a container
2. **Rust toolchain** (`cargo build --release`) — the harness is a native subcommand of
   `iris-agentic-dev`, not a separate tool
3. **An LLM API key** — `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`
4. **The public repository** (no private repo, no separate Python MCP server needed)

```bash
git clone https://github.com/intersystems-community/iris-agentic-dev.git
cd iris-agentic-dev
cargo build --release
```

---

## Quick Start — Run One Skill in 10 Minutes

```bash
# 1. Start the IRIS benchmark container
docker run -d --name iris-bench \
  -p 1972:1972 -p 52773:52773 \
  intersystemsdc/iris-community:latest
# Wait ~30 seconds for IRIS to start

# 2. Run the benchmark with the top-ranked skill
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52773
export IRIS_GENERATE_CLASS_MODEL=claude-sonnet-4-6   # or any model generate.rs supports
export ANTHROPIC_API_KEY=sk-ant-...                   # or OPENAI_API_KEY for gpt-* models

./target/release/iris-agentic-dev benchmark \
  --skill light-skills/skills/objectscript-review/SKILL.md \
  --baseline \
  --output results.json

# 3. See your results
cat results.json | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(f\"Pass rate: {d['pass_rate']:.0%} ({d['tasks_passed']}/{d['tasks_total']})\")
print(f\"Baseline: {d.get('baseline_pass_rate',0):.0%}\")
print(f\"Lift:     {d.get('lift',0):+.0%}\")
"
```

No `pip install`, no second repository, no separate MCP server process — the harness is
a subcommand of the same binary you already have.

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
    {"task_id": "jira-001", "outcome": "pass", "iterations": 1, "elapsed_s": 8.2, "reason": ""},
    {"task_id": "jira-002", "outcome": "fail", "iterations": 1, "elapsed_s": 3.9, "reason": ""}
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

---

## Writing a Skill That Will Score Well

The data is clear: **shorter hard-gate checklists beat long reference documents**.

| Design | Example | Score |
|--------|---------|-------|
| 205-word hard gate checklist | `objectscript-review` | **100%** |
| 268-word all-in-one | `iris-light-slim` | 86% |
| 472-word pattern reference | `objectscript-list-patterns` | 91% |
| 5,170-word comprehensive reference | `iris-light` | 21% |

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

1. Your skill file at `light-skills/skills/yourgithub/your-skill/SKILL.md`
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

**No IRIS connection found**
```bash
docker run -d --name iris-bench -p 1972:1972 -p 52773:52773 \
  intersystemsdc/iris-community:latest
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52773
```

**LLM authentication failed**
```bash
# Anthropic:
export ANTHROPIC_API_KEY=sk-ant-...

# OpenAI:
export OPENAI_API_KEY=sk-...
```
Also confirm `IRIS_GENERATE_CLASS_MODEL` is set to a model name your key has access to.

**`SUITE_NOT_AVAILABLE`**
- Only `--suite jira` (the default) is available in v1 — `mf`/`sql` are not yet ported.

**`BENCHMARK_RUN_IN_PROGRESS`**
- Another benchmark run is already active against the same IRIS host. Wait for it to
  finish; a run older than `--max-time-s` (default 600) is treated as abandoned and
  automatically overridden on the next attempt.

**Tasks time out**
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
    "files": [{"path": "src/X.cls", "content": "...buggy code..."}]
  },
  "test_code": {"path": "tests/TestX.cls", "content": "...test that fails on bug..."},
  "hints": [],
  "expected_behavior": "...",
  "success_criteria": {"compile_success": true, "tests_pass": true, "max_patch_lines": 30, "requires_symbol_preservation": true}
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
