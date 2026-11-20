# Research: Repo2RLEnv / Harbor RL environment for iris-agentic-dev

**Branch**: `claude/repo2rlenv-iris-setup-w8xij5` | **Date**: 2026-09-16 | **Status**: research only

No code was written for this. Every number below was measured against this repo at commit
`HEAD` of master on the date above, or read out of the upstream docs cited at the end. Read
[plan.md](./plan.md) for the proposed first slice.

## Question

Can we set up [Repo2RLEnv](https://github.com/huggingface/Repo2RLEnv) for this repo, what are
the cloud and on-prem sandbox options, and what changes if we train with a method like
FlashREINFORCE?

## Verdict

Repo2RLEnv fits as a **packaging and quality layer**, not as a push-button generator.

- Its strongest pipelines are the wrong shape here. `pr_runtime` and `pr_diff` are **Python only
  in v0.3** ("JS/Go/Rust/Java in v0.4+"; Rust log parsers exist but "aren't production-ready"),
  and we only have **22 merged PRs** — too thin to mine either way.
- `commit_runtime` is the one that fits: it inherits a language-agnostic bootstrap and its own
  docs say it works best on "direct-commit repos (Go, single-maintainer crates, internal
  repos)". That is this repo.
- Its warning is aimed straight at us: "If the suite needs network, GPUs, or flaky services, it
  won't run green in a slim container and yield collapses toward 0." A live IRIS container is
  that flaky service — `ci.yml` allows 90 s of init plus up to 7 minutes of Atelier polling.

The task format (Harbor) is worth adopting regardless, because it is trainer-agnostic and
because Harbor exists to run arbitrary agents — Claude Code, Codex CLI, OpenHands — against the
same tasks.

## Repo2RLEnv

Python 3.12+, Git, Docker, a GitHub token, and LLM provider keys. `pip install repo2rlenv`,
extras `tasksmith,daytona,harbor,modal,mutation`.

```bash
repo2rlenv generate --repo pallets/click --pipeline pr_diff \
  --pipeline-opt limit=3 --out ./workspace/click-tasks
repo2rlenv validate ./workspace/click-tasks --deep
repo2rlenv pipelines list
repo2rlenv push ./workspace/click-tasks <org>/<dataset>
```

Emitted task (Harbor layout):

```text
<task-id>/
├── instruction.md          # the learner's assignment
├── task.toml               # runtime, resources, provenance, labels
├── environment/            # Dockerfile (or docker-compose.yaml), source snapshot, fixtures
├── solution/solve.sh       # private reference implementation
└── tests/test.sh           # trusted verifier entrypoint
```

| Pipeline            | Purpose                                                       | State        |
| ------------------- | ------------------------------------------------------------- | ------------ |
| `pr_diff`           | Reproduce PR changes, scored by diff similarity and LLM judge | stable       |
| `pr_runtime`        | Fix PR regressions; failing tests pass, existing stay green   | stable       |
| `commit_runtime`    | Test-based tasks from commit history                          | stable       |
| `code_instruct`     | LLM-authored problems grounded in repo APIs                   | experimental |
| `equivalence_tests` | Implement functions matching private references               | experimental |
| `cve_patches`       | Repair vulnerabilities from CVE and fix-commit records        | experimental |

Plus 14 research recipes (`swe_smith`, `r2e_gym`, `repo_mutate`, `terminal_synth`, …), all
code-owned and experimental.

`commit_runtime` gates, from its docs: `require_fail_to_pass` (default true), `min_fail_to_pass`,
`require_new_test_funcs`, `max_source_files_per_commit` (10), `skip_merge_commits`,
`clone_depth` (200), `validation_timeout_sec` (600). It filters on conventional-commit and
bugfix signals — `fix:` prefixes, `Closes #N` trailers, keywords like bug/crash/broken.

Most verifiers return deterministic 0/1 rewards; native pipelines support graded test and
diff-similarity rewards.

## Harbor execution backends

This is the fact that decides the architecture:

> Most environments expect a single Dockerfile which is insufficient for multi-container tasks.
> The `--env docker` environment supports multi-container tasks by preferring an
> `environment/docker-compose.yaml` file if present. `DockerEnvironment` is currently the only
> environment that supports multi-container tasks.

Cloud backends — Daytona, Modal, LangSmith, Blaxel, Novita Sandbox, Tensorlake, Runta, Vercel
Sandbox — are single-container. **An IRIS sidecar rules all of them out.**

```bash
harbor run --dataset terminal-bench@2.0 --agent claude-code --model <m> --n-concurrent 4
harbor run ... --n-concurrent 100 --env daytona
harbor datasets list
```

## The architectural fact that unlocks the cloud path

`iris-agentic-dev` reaches IRIS over **Atelier REST (52773/52780)**. It is a network client, so
IRIS does not have to live in the rollout sandbox. One shared IRIS, one namespace per rollout,
and every single-container backend works.

We already have the isolation primitive:
`benchmark/021/runner/namespace.py:reset_benchmark_namespace()` drops and recreates `BENCHMARK`
to kill carry-over between conditions. Namespace reset is seconds; IRIS boot is minutes.
**Never boot IRIS per rollout.**

### The one conflict: NoPWS plus `docker_only`

Enterprise 2026.2.0AI has no private web server (DPP-1192), and our workaround is
`docker_only=true`, which execs into the container. That needs a local Docker socket, so IRIS
must be co-located with the worker — which kills the shared-IRIS design. A network-reachable
shared IRIS requires Atelier REST: either a Community image with PWS, or an Enterprise instance
fronted by a Web Gateway exposing `/api/atelier`. This decision sets whether the fleet needs one
IRIS node or one IRIS container per worker.

## What this repo already has

The reward function mostly exists. This is the main reason not to start from Repo2RLEnv's
generators.

| Asset                 | Where                                                          | What it gives us                                                                          |
| --------------------- | -------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| 39 curated tasks      | `benchmark/021/tasks/*.yaml`                                   | id, category, path A/B, description, `expected_behavior`, **fixtures inline as data**     |
| Graded judge          | `benchmark/021/runner/judge.py`                                | 0–3 rubric with per-category notes, incl. anti-hallucination rules for DOC tasks          |
| Pass semantics        | `tests/e2e/skill_eval/scoring.py`                              | `PASS_THRESHOLD = 2`, `UNSCORED_LIMIT = 0.10`, `score: None` never `0`                    |
| Tool-use metrics      | `benchmark/021/runner/toolset_tracker.py`                      | `wrong_tool_count`, `total_tool_calls` against the live `tools/list`                      |
| Measurement rigor     | `tests/e2e/skill_eval/`                                        | `baseline.py`, `lift.py`, `fire_rate.py`, `preflight.py`, `provenance.py`, `isolation.py` |
| Agent drivers         | `claude_code.py`, `copilot.py`, `tests/e2e/opencode_runner.py` | three implementations of roughly one interface, not yet declared                          |
| Bedrock scoring       | `benchmark/021/runner/_client.py`                              | `AnthropicBedrock` when `AWS_BEARER_TOKEN_BEDROCK` / `AWS_ACCESS_KEY_ID` is set           |
| Environment blueprint | `.github/workflows/ci.yml` `e2e-tests`                         | the exact IRIS bring-up, readiness poll and credential probe a task image needs           |

Test targets that matter for tiering: `unit` (100 files, no IRIS), `binary` (14, spawns the
binary over stdio, no IRIS), `integration` (54, live IRIS, serial).

## Mining yield, measured

```text
commits on master                                             883
fix:/perf: commits                                            282
  touching both crates/**/*.rs and a test file (F2P candidates) 73
    with <= 3 source files changed                              58
      tests are unit-only, no IRIS needed                       26
merged PRs                                                     22
specs/ feature directories                                      79
```

Reproduce with:

```bash
git fetch --unshallow   # a shallow clone reports 50 commits and lies about all of this
python3 - <<'EOF'
import subprocess, re
log = subprocess.run(["git","log","--pretty=format:@@%H|%s","--name-only"],
                     capture_output=True, text=True).stdout
commits, cur = [], None
for line in log.splitlines():
    if line.startswith("@@"):
        cur = {"sha": line[2:].split("|")[0], "subj": line.split("|",1)[1], "files": []}
        commits.append(cur)
    elif line.strip() and cur:
        cur["files"].append(line.strip())
src = lambda f: f.startswith("crates/") and f.endswith(".rs") and "/tests/" not in f
tst = lambda f: (".rs" in f and "/tests/" in f) or f.startswith("tests/")
fix = [c for c in commits if re.match(r'^(fix|perf)(\(|:)', c["subj"])]
cand = [c for c in fix if any(map(src, c["files"])) and any(map(tst, c["files"]))]
small = [c for c in cand if len([f for f in c["files"] if src(f)]) <= 3]
unit = [c for c in small if all("integration" not in f for f in c["files"] if tst(f))]
print(len(commits), len(fix), len(cand), len(small), len(unit))
EOF
```

The 79 `specs/*/tasks.md` are a second seed source with acceptance criteria already written, and
`repo_mutate` / `swe_smith` are built for synthesizing defects into source we own. Corpus size
is the binding constraint (see FlashREINFORCE below), so both matter.

## Task tiers

**Tier 0 — no IRIS.** The 26 unit-only fix commits, the `unit` and `binary` targets, clippy/fmt
gates, the `tool --list` / `--schema` surface. Single container: Rust toolchain plus repo
snapshot. Runs 100-wide on any backend. This is where `commit_runtime` can genuinely
auto-generate.

**Tier 1 — remote IRIS.** The 39 `benchmark/021` tasks and `tests/e2e/tasks`: agent-uses-the-MCP-
tools work, which is the actual product surface. Container holds agent plus `iris-agentic-dev`,
`IRIS_HOST` points at a shared server, namespace per rollout. Works on cloud backends.

**Tier 2 — IRIS sidecar.** The `integration` target, productions, mirroring, globals.
`environment/docker-compose.yaml` with `intersystemsdc/iris-community:2025.3` (ci.yml's last
known-good). Local Docker only, and slow.

## FlashREINFORCE

Critic-free, single-rollout, asynchronous RL (NVIDIA, September 2026). Three parts: One-Batch
REINFORCE (advantage = reward minus the rollout-batch mean, no group, no whitening), Sequence
Trust Region (token importance ratios against stored behavior probabilities, then trajectory-level
admission via a Bernoulli KL proxy at threshold δ), and Sample-Mean Optimization (average loss
within a trajectory before aggregating across the batch). No critic, no ratio clipping, no
reference-model forward pass. Tolerates ~4–8 steps of policy staleness.

Landing as configuration in trainers that already exist, so there is no bespoke trainer to write:

- OpenRLHF PR #1339: `--algo.advantage.estimator flash_reinforce`,
  `--algo.advantage.is_correction_level {off,token,seq}`, `--is_correction_mode {mask,clip}`,
  `--is_correction_gating {ratio,binary_kl,tv}`, `--is_correction_threshold [LOW] HIGH`,
  `--actor.loss_agg_mode seq-mean-token-mean`, plus an `is_filter_ratio` rejection metric.
- NVIDIA-NeMo labs-molt PR #116 for the async path.

### What it means for this repo

1. **It breaks the agent driver, and only the agent driver.** Workers must submit "the
   behavior-policy probabilities that actually generated its tokens" — vLLM behavior logprobs.
   No closed API exposes those, so `claude_code.py`, `copilot.py` and `opencode_runner.py` are
   eval drivers permanently. Training needs an open-weights policy served by vLLM in an agent
   loop that speaks MCP to `iris-agentic-dev`. Tasks, fixtures and judge survive unchanged.

2. **The lag tolerance is why this method suits an IRIS environment.** Our rollout latency is
   long-tailed and ugly. Synchronous GRPO stalls a whole batch on the slowest trajectory, which
   with IRIS in the loop is the common case. Here a slow rollout lands stale and is still
   admitted. Optimize throughput and independence, not tail latency.

3. **One rollout per prompt moves the scarce resource from IRIS concurrency to task breadth.**
   GRPO at k=8 amortizes one environment setup across eight rollouts of the same fixture;
   batch-centering over independent prompts gives that up. At ~39 curated plus ~58 mineable
   tasks we would replay the whole corpus every batch or two. Hence: corpus size is priority
   one, hold out a test split **before the first rollout**, and keep fixtures as data (our task
   YAMLs already inline `.cls` content) so one prebaked image serves every task.

4. **Batch centering turns the unscored-item rule into trainer correctness.** An unscorable
   rollout admitted as `0` shifts the baseline for every other trajectory in the batch — under
   GRPO the same bug corrupts one group of eight. `scoring.py`'s `score: None` rule and the 10%
   `UNSCORED_LIMIT` must be enforced at the rollout boundary: drop the trajectory, never zero it.
   The spec-118 bug shipped a month of misleading nightlies as an eval defect; under this
   trainer it is a silently biased gradient.

5. **No whitening means judge variance lands directly in advantage scale.** Deterministic
   `tests/test.sh` is the primary reward; the 0–3 rubric is for diagnostics and for DOC-style
   tasks nothing deterministic reaches. Do not mix tiers in one batch — Tier 0's high pass rate
   contaminates the baseline and centers out hard Tier-1 successes. An all-pass or all-fail batch
   yields no signal, so bin tasks by the pass rates `skill_eval` baselines already measure.

6. **One repo-specific hacking surface.** `wrong_tool_count` is a good metric and a bad reward
   term: the cheapest way to zero it is to call no tools and answer from memory, which is the
   exact failure the DOC rubric exists to catch. If we shape on it, apply the penalty only to
   trajectories that already passed the verifier.

## AWS

Harbor has no first-class AWS backend, which does not matter: on EC2 we _are_ the local Docker
host, so we get the only multi-container-capable backend at cloud scale. The scoring path is
already AWS-native (`_client.py`, and `skill-regression.yml` runs the judge on Bedrock in
`us-east-1`).

| Layer                    | Service                                    | Notes                                                                                                                                                    |
| ------------------------ | ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Corpus + rollout records | S3                                         | FlashREINFORCE adds per-token behavior probabilities per trajectory. Compress them; they dwarf the transcripts.                                          |
| Images                   | ECR                                        | Prebake the Rust toolchain plus a warm `target/` and cargo registry. Mirror the IRIS image: no Docker Hub pull limits, and a pin against license rot.    |
| IRIS                     | EC2 m6i/r6i, private subnet                | Marketplace Community AMI or our own container. Namespace per rollout. 52773 never public.                                                               |
| Rollout workers          | AWS Batch array jobs or ECS, c7i **Spot**  | Spot is safe _because_ of the 4–8 step staleness tolerance: a reclaimed worker loses one trajectory, not a batch.                                        |
| Learner + policy server  | g6e (L40S) for a 7–8B policy, p5 if larger | Critic-free, clip-free, reference-free removes three memory line items, which is what makes single-node g6e realistic. vLLM emits the behavior logprobs. |
| Judge                    | Bedrock                                    | Already wired. In-account, no external key.                                                                                                              |
| Secrets                  | Secrets Manager / SSM                      | `iris.key`, Bedrock token. SSM Session Manager, not SSH.                                                                                                 |
| Queueing                 | SQS → Batch                                | One rollout per prompt makes this the natural shape.                                                                                                     |

Licensing improves on AWS: the Marketplace IRIS Community AMI ships a built-in license valid
roughly a year from the product version's release date, with a production-enabled USER
namespace — better than the container rot documented in `ci.yml` ("expire ~150 days after image
publish"). From an InterSystems account we can mount a real key and drop the core and connection
caps, which is what sets rollout concurrency once IRIS is shared.

Two gotchas: the NoPWS/`docker_only` conflict above, and Bedrock model access —
`_client.py` sets `_BEDROCK_HAIKU = "us.anthropic.claude-sonnet-4-6"` with the comment "haiku-4-5
unavailable on this account", so at RL scale we would arbitrate every rollout at Sonnet prices.
Either enable Haiku in the account or keep the deterministic verifier primary and sample the
judge.

Cost shape: the GPU learner dominates and everything else is noise. Tier 0 needs no IRIS at all,
so the cheapest useful run is also the first one on the list. GPU quota is more likely to block
us than budget.

## prime-agent probe

T006 and T009. Two live headless runs against `prime-agent` 0.8.1 on 2026-09-18, $0.083 total, plus
a read of the shipped `docs/skills.md`. Decision 5's three verified-from-docs facts held; its fourth
claim, that skills are importable Python packages, was too narrow. The event stream is
machine-readable and the MCP path works headless — but the tool log names only `ipython`, and that
one fact is what spec 121's per-tool work has to absorb.

### Does a headless run honour a pinned stdio MCP server? Yes.

```bash
prime-agent mcp add iris-agentic-dev --cwd /path/to/repo \
  --env IRIS_HOST=IRIS_HOST --env IRIS_WEB_PORT=IRIS_WEB_PORT \
  --env IRIS_USERNAME=IRIS_USERNAME --env IRIS_PASSWORD=IRIS_PASSWORD \
  -- ./target/debug/iris-agentic-dev mcp

PATH=/opt/homebrew/opt/node@22/bin:$PATH TMPDIR=$(mktemp -d) \
  prime-agent --mode json --no-session --provider openai --model gpt-4.1 \
  -p 'Call the iris-agentic-dev MCP tool iris_info once and report the IRIS version.'
```

That reached the live container and came back with `2026.2.0L (Build 208U)` off a real `iris_info`
call. `mcp add` writes `~/.prime/agent/settings.json`:

```json
{
  "mcpServers": {
    "iris-agentic-dev": {
      "type": "stdio",
      "command": "./target/debug/iris-agentic-dev",
      "args": ["mcp"],
      "cwd": "/path/to/repo",
      "env": { "IRIS_HOST": { "env": "IRIS_HOST" } }
    }
  }
}
```

Three things cost an hour each and are worth writing down:

- **`env` values are indirection references, not values.** A literal `"IRIS_HOST": "localhost"`
  raises `ValueError: MCP stdio env values must use {"env": "NAME"} references`. The parent process
  exports the real value; the settings file only names it. Good for the harness — no credential ever
  lands in a config file an arm writes — but it means `arms.configure` has to export as well as write.
- **Node ≥ 22.8.0.** The CLI refuses to start otherwise, and Homebrew's default `node` is 20.
- **A sandboxed `HOME` needs a fresh `TMPDIR`.** The daemon derives its worker socket identity partly
  from `TMPDIR`, so reusing the outer one against a new `HOME` gives
  `Timed out after 30000ms waiting for … "create"` and an `ENOENT` on a socket that was never
  created. `TMPDIR=$(mktemp -d)` per session fixes it, and the pilot already runs one temp dir per
  session.

### Is the tool log machine-readable? Yes, and it names one tool.

`--mode json` writes newline-delimited JSON: `session`, `agent_start`, `turn_start`,
`message_start`, `message_update`, `message_end`, `tool_execution_start`, `tool_execution_update`,
`tool_execution_end`, `turn_end`, `agent_end`. One captured run is at
`tests/e2e/skill_eval/fixtures/prime_agent_session.jsonl` (deltas dropped, long strings truncated —
see the README beside it).

A completed call is `tool_execution_end` with `result.details.status == "ok"`, carrying `durationMs`
and correlated to its start by `toolCallId`. A failed cell is the same event with
`status == "error"` and `isError: true`, which the first probe produced when the settings file held
literal env values. That is a cleaner completion tell than opencode's, which has no reliable
`session.status` idle event at all — `hit_the_clock` reads the wall clock precisely because opencode
would not say.

**The finding that matters: `prime-agent` exposes exactly one model-visible tool, `ipython`.** MCP
servers are reachable only from inside its persistent kernel, through `rlm.mcp`:

```python
await mcp.list_tools("iris-agentic-dev")
await mcp.call_tool("iris-agentic-dev", "iris_info", {"what": "metadata"})
```

So `tool_execution_start.args.code` holds Python source, and the tool name is inside a string
literal in it. Three consequences:

1. **FR-013's per-tool log needs a parser, not a field read.** Under opencode a tool call is an
   event with a name; here it is a `mcp.call_tool(...)` call site in a code string, possibly several
   per cell, possibly in a loop or behind a variable. The driver can recognise the literal form
   reliably and must report anything else as an unattributed call rather than guessing — an
   undercount is a smaller number, an invented attribution is a wrong one. This lands directly on
   spec 121 T039–T041, which assume a per-tool name per call.
2. **One `ipython` cell can hold several tool calls**, so "completed calls" and "completed cells" are
   different numbers. The boundary's `ToolCall` log holds the former; `tool_execution_end` counts the
   latter.
3. **A tools-arm session can reach IRIS without any MCP call at all** — the kernel has Python, and
   the container is reachable over HTTP. That is true of the bare arm's `bash` too, so it changes no
   arm's definition, but a reach figure computed from this harness measures MCP use, not IRIS use.

### Token and cost accounting comes free, and it is not cheap here

Every assistant `message_end` carries `usage.input/output/cacheRead/cacheWrite/totalTokens` and
`usage.cost.{input,output,cacheRead,cacheWrite,total}`. Sum over assistant `message_end` events —
**not** `turn_end`, which repeats the last message's usage rather than totalling the turn. This is
the per-item cost the pilot runner currently cannot report under opencode.

The number it reported is a warning: the run that called `mcp.list_tools` first cost $0.0624 against
$0.021 for the run that did not, because the whole 81-tool listing landed in the kernel output and
then in context (20,887 input tokens on the following turn). Under a per-tool benchmark this is
paid on every rollout. `IRIS_TOOLSET=merged` already narrows the surface; the arm should also expect
the model to list before it calls, and the cost estimator (Phase 4) should assume it does.

### The third arm ports — Decision 5's premise was too narrow (T009)

`prime-agent` implements the [Agent Skills standard](https://agentskills.io/specification):
directories holding a `SKILL.md`, discovered recursively, with names and descriptions in the system
prompt and the body loaded on demand. Python-backed skills are a documented superset (a
`pyproject.toml` plus `src/<import_name>/__init__.py`, installed editable into the kernel venv), not
the format. So iad's 34 `SKILL.md` packs are first-class, and the `tools+skills` arm is buildable on
this harness. `docs/skills.md` goes as far as documenting `"skills": ["~/.claude/skills"]` for
exactly this case.

Discovery locations, which is the list the driver's absence facts have to name:

| Where                                                                      | Precedence |
| -------------------------------------------------------------------------- | ---------- |
| `--skill <path>` (repeatable; loads even under `--no-skills`)              | highest    |
| `skills` array in settings (files or directories, `-name` to force-remove) |            |
| Package `skills/` directories or `pi.skills` in `package.json`             |            |
| Project `.prime/agent/skills/`, `.agents/skills/` (cwd and ancestors)      |            |
| Global `~/.prime/agent/skills/`, `~/.agents/skills/`                       |            |
| Built-in pack shipped inside the `prime-agent` install                     | lowest     |

**The bare and tools arms are contaminated by default.** `prime-agent` ships `prime-intellect`,
`skill-creator` and `websearch` and loads them unless told not to, so the two arms that claim no
skills need `enableBuiltinSkills: false` in settings (or `--no-skills`, which also excludes the
built-ins), and `assert_absent` has to check every row above — including the install directory,
which is not under the sandboxed `HOME`. A driver that checked only `~/.prime/agent/skills/` would
report a clean bare arm that was reading `websearch`'s description on every turn. This is FR-009's
vacuity in its second form, and it is why T004 moved the check behind the driver.

The packs load. The Phase 2 gate ran the `tools+skills` arm with all 34 installed under a sandboxed
`HOME`, and that arm is the one that solved the task.

### Phase 2 gate: PILOT-01 across three arms, under prime-agent (T009)

`openai/gpt-4.1`, 300-second sessions, `tests/e2e/results/pilot-120-phase2-gate.json`:

| Arm            | Verdict | Cells | Session |
| -------------- | ------- | ----: | ------: |
| `bare`         | FAIL    |     0 |   5.7 s |
| `tools`        | FAIL    |     2 |  27.2 s |
| `tools+skills` | PASS    |     8 |  56.3 s |

One task, one session per arm. It says the harness swap works end to end — arm configuration,
sandboxed `HOME`, MCP registration, skill install, event parsing, grading against live IRIS — and it
says nothing about what the tools are worth.

The two failures are the model's, not the harness's, and the transcripts say so plainly. Both ended
their turn by asking Tom which approach he would prefer: "Is it acceptable to connect using the iris
terminal…?" (`bare`), "Should I continue trying different web API routes, or do you want me to use a
shell/terminal approach…?" (`tools`). `-p` is one-shot, so a question ends the session. The `tools`
arm had `iris-agentic-dev` registered and was told about it — `formatGenericMcpGuidance` in
`dist/core/system-prompt.js` names every enabled server and shows `mcp.call_tool`, independent of
skills — and it still went to Atelier REST through `httpx`, failed on the route, and asked. The
`tools+skills` arm called `mcp.list_tools` after guessing a tool name wrong, then `iris_doc` get,
`iris_compile`, `iris_doc` put with `compile: true`, and finished. That is the third consequence
above showing up in a live arm on its first outing, and it is a reason the pilot is eight tasks
across three arms rather than one.

Three harness rules came out of the gate's four runs, each now covered by a unit test in
`tests/e2e/skill_eval/test_prime_agent.py`:

- **A session `TMPDIR` must be short.** macOS caps a unix socket path at 104 bytes (`sun_path`), the
  daemon binds `$TMPDIR/prime-agent-<10 digits>/worker-<12 hex>-<12 hex>.sock`, and the default
  49-character `/var/folders/...` `TMPDIR` puts that at about 127. The bind truncates, the supervisor
  `lstat`s the name it asked for, and the session dies `ENOENT` after the 30-second daemon timeout.
  Gate run 1 reported all three arms as "FAIL, 0 tool calls, 31 s" — indistinguishable from three
  agents that ignored their tools. `session_tmpdir_root()` prefers `/tmp` and refuses any root that
  does not leave room for the socket.
- **The daemon outlives the CLI.** `prime-agent` starts a detached supervisor plus a worker per
  session; killing the CLI leaves both running, and gate run 3 accumulated eight strays across four
  timed-out sessions, after which every new session hung. Both name the session's `TMPDIR` in their
  command line, so `pkill -9 -f <tmpdir>` reaps precisely what this session started and leaves a
  developer's own daemon alone. It runs in a `finally`, timeout or not.
- **Cleanup cannot fail a run.** The reaper kills the daemon while node is still writing its compile
  cache, so a file lands in `TMPDIR` after the listing the cleanup walks and the final `rmdir` fails
  `ENOTEMPTY`. `tempfile.TemporaryDirectory` raises that at the caller, and the caller reads any
  exception from `collect_events` as a session that never ran — gate run 4 reported a finished `bare`
  arm as unscored with `[Errno 66] Directory not empty`. `mkdtemp` plus
  `shutil.rmtree(ignore_errors=True)` keeps the housekeeping out of the verdict.

Two honesty rules ride with them. A session that emitted no events at all raises `SessionNeverRan`
with the tail of stderr attached, and `pilot.run_one` turns any spawn exception into `passed=None` —
a dead supervisor is a hole in the arm, never a FAIL that describes the daemon. And a session killed
on the clock keeps the calls it already made: `Popen` plus `communicate(timeout=…)` holds the partial
stdout on `TimeoutExpired`, which `subprocess.run` would discard.

### Phase 3 gate: the exported task, driven end to end (T013, T014)

`benchmark/harbor/export.py` turns one `GradedTask` into one directory per arm. The gate drove the
exported `GOLDEN-01`/`tools` directory against `iris-dev-iris` with the local debug binary:

| Step                      | Expected | Got                                       |
| ------------------------- | -------- | ----------------------------------------- |
| `environment/seed.sh`     | exit 0   | exit 0, fixture applied, compile failed   |
| `environment/seed.sh` × 2 | no-op    | exit 0, nothing written (the marker held) |
| `tests/test.sh`           | `0.0`    | `reward.txt` = `0.0`                      |
| `solution/solve.sh`       | exit 0   | exit 0, reference compiled                |
| `tests/test.sh`           | `1.0`    | `reward.txt` = `1.0`                      |

The reward was read from the file both times, never from an exit code, which is the whole point of
FR-004: `tests/test.sh` writes `1.0` for PASS, `0.0` for FAIL, and **nothing** when the check printed
neither word, both words, or could not run. Harbor reads a missing `reward.txt` as a trial with no
reward, which is the honest reading; a `0` there would file a harness fault in the arm's column, the
same mistake as scoring a killed session zero.

**What is skipped, and why it is named here rather than left out:** `harbor run --env docker` did not
run. The harbor CLI is not installed on this machine (`which harbor` → not found); Docker is up. So
everything inside the directory is verified and Harbor's orchestration of it is not. The value most
likely to be wrong when it does run is the route out: `network_mode = "allowlist"` with
`allowed_hosts = ["${IRIS_HOST}"]`, which is what Harbor's docs describe and what spec 121's T002
recorded, but whether `${VAR}` templating reaches `allowed_hosts` as well as `[environment.env]` is
unverified. It is worth naming because a task container that cannot reach IRIS fails every arm for the
same reason — a broken corpus that reads as a broken tool. There is a unit test asserting the host is
allowlisted, so the value cannot drift silently; only Harbor can say whether it resolves.

Four things the format forced, each with a plausible alternative that grades nothing:

- **The fixture is applied by the healthcheck, with a marker file.** A single-step Harbor task has no
  `setup.sh`, and `[environment.healthcheck] command` is the only hook that runs before the agent.
  But a healthcheck fires on an interval, so the second firing would put the broken fixture back over
  whatever the agent had fixed — the run would grade the fixture, every arm, every task. `seed.sh`
  writes `/tmp/.iad-seeded-<task-id>` last, after the fixture is in place, and returns immediately if
  it is there.
- **Fixtures travel inline, in a heredoc, applied through `doc put -f`.** Not a per-task image: 40
  tasks is 40 directories against one pinned image rather than 40 builds. Not `doc put -` either —
  the CLI takes the `-` as the document name (`ERROR #16006`), which is a defect worth its own issue.
  And not JSON through `tool iris_doc`, because that puts an escaping layer between the corpus's
  ObjectScript and what lands on the server.
- **The fixture's compile is allowed to fail; the reference's is not.** A class that does not compile
  _is_ the task in most of this corpus. `seed.sh` ends `|| true` on the compile and `solve.sh` does
  not, so a reference that cannot compile fails the gate instead of quietly grading `0.0`.
- **`[[environment.mcp_servers]]` has `command` as a string and `args` as a list.** `arms.py` had the
  whole argv in `command` plus a per-server `env` holding the toolset, and schema 1.3 has neither:
  Harbor would have looked for an executable named `iris-agentic-dev mcp` and dropped the `env`, so
  the tools arm would have registered nothing and reported the result as the tools'. The toolset moved
  to `[environment.env]`, where `IRIS_TOOLSET` is read anyway.

The exporter refuses before it creates anything. `validate_shape` and then live `validate_live` run
first, so a task with no reference, a check with only one verdict word, or a check that already passes
against its own untouched fixture leaves no directory on disk for a later run to find and grade. That
last one is the 118 failure class exactly: every arm clears the task and the report reads 0.00 lift
with the tools working perfectly.

`benchmark/harbor/golden/golden-01-tools/` is the committed golden copy. Harbor's format is young, so
a drift upstream — or a well-meant edit to a generated script — is a diff against six files rather
than a corpus that runs and grades nothing. It pins the image tag explicitly; the version pin itself
is a separate assertion, so a release bump is not a golden-file failure that says nothing about the
format. `IAD_UPDATE_GOLDEN=1 pytest benchmark/harbor/test_golden.py` adopts a deliberate change.

### Phase 4 gate: the split guard, and provenance that names the harness (T015–T018)

The guard was driven in both directions by editing the committed
`tests/e2e/tasks/benchmark/pilot/split.toml` and running `test_split.py`: `PILOT-04` on both sides is
red (`PILOT-04 is in both sides. The publish guard reads holdout and would pass it`), `PILOT-08` on
neither is red (`PILOT-08 is in neither side`), the restored file is green. Both messages name the ID.
A message that said "coverage mismatch" would hand the next person two lists to diff by hand, and the
split is exactly the kind of file where the wrong ID is one character off.

The train side is three tasks and the reason is written into the file: PILOT-01 is train because every
phase gate in this spec ran it and I read the transcripts to work out why the bare and tools arms
failed. That is tuning, whatever it was called at the time. PILOT-02 and PILOT-03 join it so the
optimizer has more than one task to fit against. The other five stay unread, and a task moves to train
the moment anyone tunes anything while looking at it — as a commit, with a reason.

FR-012 landed with it. `Provenance` grew `driver` and `harness_version`, and three things about how:

- **The identity is read off the driver object, not passed in as a string.** `provenance.driver_identity`
  takes an `AgentDriver` and reads `name` and `harness_version`. A call site that knows the name well
  enough to hard-code it has already broken the boundary the driver exists to hold — and the swap to
  `prime-agent` is the change this field is for. A test asserts the two shipped drivers answer those
  two attributes, so a rename there fails loudly instead of recording every run as `unrecorded`.
- **`unrecorded` is a value, not a gap.** Every provenance block in the committed baseline predates the
  field. Reading one back gives `driver: "unrecorded"`, which is what happened, and is the same shape
  as `tool_surface: "none"` for a session that ran with no tools.
- **Neither field is in `COMPARABILITY_FIELDS`.** `baseline.driver_change` annotates the Δ instead —
  the same rule `tool_surface` follows. The release that replaces `opencode` with `prime-agent` is the
  one a comparison most needs to survive; suppressing it there would blind the gate on the change it
  was built to catch. `driver_change` also stays quiet when either side never recorded a driver:
  "unrecorded → opencode" is a schema change, and printing it on every row of the first run after this
  landed would teach people to skip the line that matters.

### Phase 5 gate: namespace per rollout, and what the namespace tools do not do (T019–T020)

Decision 1 closed on shared IRIS over Atelier REST and named one part of it unproven:
namespace-per-rollout. The pilot ran serial in one `BENCHMARK` namespace, so nothing asserted that two
rollouts cannot see each other's classes. `tests/e2e/skill_eval/rollout_namespace.py` is the primitive,
32 unit tests and 4 live ones cover it, and the live half took 14.8 s of container time and no model
tokens.

**The name is derived, not assigned.** `namespace_for(task_id, index)` is a pure function, so the seed
step and the grade step arrive at the same namespace without talking to each other. A registry that
hands out names has to be consulted, and anything that has to be consulted can be skipped. The digest
runs over the task id as written rather than the legible label, because `PILOT-01` and `PILOT_01` both
flatten to `PILOT01` and a name built from the label alone would hand them one namespace.

**Grading refuses a namespace this manager did not create**, which closes spec 121's finding C4 — its
Constitution Check claimed that refusal and no task implemented it. `BENCHMARK` gets its own message:
it is where the serial pilot ran, so it holds the last run's answers, and it is the value a call site
reaches for out of habit. The lease set is per manager rather than global, because two managers are two
workers and one must not vouch for the other's namespace.

**The concurrency test only means something with a barrier.** Two rollouts writing a class of the same
name with different bodies can each finish their write-then-read before the other starts, and a single
shared namespace would then pass twice. Both write, both wait, then both read. Verified by running the
broken arrangement on purpose — one namespace, two rollouts — which returns
`['PASS', 'FAIL:ROLLOUT-ZERO']`: one rollout grading the other's document, named in the failure text.

**Three defects in the namespace tool surface, all found by running it rather than reading it.** T020
said "on top of the existing tools", and the existing tools cannot do it:

- `iris_namespace_create` requires the database to exist already — `ERROR #420` — and nothing in the
  81-tool surface creates a database. `iris_database_list` and `iris_database_stats` read; nothing
  writes.
- `Config.Namespaces.CreateOne` edits the CPF, which leaves the namespace on disk and absent from the
  running instance. The tool returns `{"created": true}` regardless. `iris_namespace_list` never lists
  it, `$namespace=` gives `<NAMESPACE>`, and a rollout that trusts the report grades nowhere.
- There is no `iris_namespace_delete`. Deletion is three objects — namespace, database, directory — and
  all three are reachable only through `iris_execute`.

So the database work and the activation go through `iris_execute`, and the create is believed only when
`iris_namespace_list` confirms it. Three tools worth adding: `iris_database_create`,
`iris_namespace_delete`, and an `iris_namespace_create` that activates what it created instead of
reporting a success the running instance does not have.

**`drop` keeps the namespace, `purge` takes it away.** Reuse by index costs one `iris_namespace_list`
call where a create costs four round trips and a CPF edit, so the run loop reuses and the clear on drop
is what makes reuse safe. `delete_namespace` refuses any name it did not derive — that path removes a
database file, so `%SYS`, `USER` and `BENCHMARK` are unreachable from it by construction.

### What `commit_runtime` actually does with this repo: 50 candidates, 0 tasks (T022)

Run for real rather than assumed twice, `repo2rlenv` 0.9.1 from PyPI into a throwaway venv:

```bash
repo2rlenv --no-ui generate --repo intersystems-community/iris-agentic-dev \
  --pipeline commit_runtime --out /tmp/r2r-out --llm openai/gpt-5-mini --max-spend-usd 1.0
```

```text
candidates  50
emitted      0
skipped     50  non_bugfix_type 31 · no_new_test_funcs 8 · no_fail_to_pass 3
                problem_statement_too_short 3 · ci_only_patch 3
                empty_source_patch 1 · no_test_patch 1
```

**The expected negative arrived, and not for the expected reason.** "Rust parsers experimental" was
the predicted blocker and it never came up: the bootstrap agent read `Cargo.toml`, installed rustup
and the build deps, got `cargo test --no-run` to compile, and saved a 5.4 GB image in 8 iterations
and 479 s for **$0.012** of LLM spend against the $1 cap. Rust was not the problem.

Three things are, in increasing order of how hard they are to fix:

- **`--llm` is mandatory even though `commit_runtime` classifies deterministically.** The refusal is
  `pipeline 'commit_runtime' requires --llm (bootstrap needs an LLM to build the sandbox image)`.
  Nothing in the pipeline docs says the bootstrap is the LLM consumer.
- **The commit-type classifier rejects 31 of 50 as `non_bugfix_type`**, and the walk is
  `depth=200`, so it never sees most of the history. The 26 unit-only fix commits in _Mining yield_
  above were counted over all 883 commits after `--unshallow`; that is not the population this tool
  sampled.
- **`f2p=0` on every commit it got far enough to validate.** Two of the three reached test parsing
  and reported `pre=2373 post=2373 … f2p=0 p2p=2026` and `pre=2357 post=2357 … f2p=0 p2p=2025`. No
  test flipped across the fix. That is the structural answer: the tests that discriminate in this
  repo are `#[ignore]`d and need live IRIS, and the sandbox image has neither IRIS nor
  `--features testing` — its smoke test ran a bare `cargo test --workspace`, exited 101, and the
  image was cached but flagged. A commit whose only witness is a test that cannot run is not a
  fail-to-pass pair.

So `commit_runtime` yields nothing here until the sandbox image is the Tier 1 environment — IRIS
reachable, feature flag on — which is the image the Phase 3 exporter already builds by hand. Mining
is not a shortcut past that work; it is downstream of it. The corpus stays hand-written, spec 121's
T031 grows it, and this measurement is recorded once so it is not re-litigated. Total cost: $0.012
and about eight minutes. The bootstrap image was deleted afterwards.

## Upstream status check — 2026-11-20

Everything above was written against `repo2rlenv` 0.9.1 and Harbor as it stood in mid-September.
Both moved. Versions and dates read off the two release pages on 2026-11-20.

| Project        | Then (2026-09) | Now             | What changed that touches this file                                                                                                                                                                           |
| -------------- | -------------- | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `repo2rlenv`   | 0.9.1 (15 Sep) | 0.9.6 (10 Nov)  | Rust and `cargo-nextest` log parsing (0.9.2, 0.9.4); Harbor 0.24 then 0.25 adapters (0.9.4, 0.9.5); isolated verifiers and contradictory-assertion checks that catch forged passes (0.9.6); three new recipes |
| Harbor         | 0.22/0.23-era  | 0.26.0 (13 Nov) | Compose on cloud sandboxes (Tensorlake Docker-in-Docker and network policy 0.23, Modal CPU Compose on VMs 0.26); `harbor lint` (0.25); an HCP v0 rollout server and inference relay (0.26)                    |
| FlashREINFORCE | PR open        | merged          | OpenRLHF #1339 merged to `main` on 2026-09-14. No release tag names it; it is a configuration, as predicted.                                                                                                  |

Three claims above are now wrong or weaker, and one is confirmed:

- **"An IRIS sidecar rules all cloud backends out" no longer holds.** Compose tasks run on
  Tensorlake and Modal now. _Harbor execution backends_ and the AWS table both assume otherwise.
  This reopens Tier 2 on a cloud backend and makes open decision 1 a real choice rather than a
  forced one.
- **"Rust parsers aren't production-ready" was never our blocker and is now partly fixed anyway.**
  The T022 probe already found Rust was not the problem. 0.9.2 and 0.9.4 add the parsers regardless.
- **Regenerating with 0.9.6 would not change the T022 result.** `f2p=0` came from a sandbox image
  with no IRIS and no `--features testing`, not from a parser. The finding stands until the image
  is the Tier 1 environment; nothing in 0.9.2–0.9.6 builds that for us.
- **Trajectory generation (0.9.6) is new and is the one thing worth a second probe.** It converts
  Claude Code, Codex and Pi sessions into repair, build, extend and QA Harbor tasks. Corpus breadth
  is this programme's binding constraint, and we generate agent sessions as a by-product of every
  benchmark run. Experimental, and its own notes say generated tasks are unverified until evaluated.

### Where the self-improvement loop actually is

There is one, and it is not RL. Spec 128's `tests/e2e/skill_eval/optimize/` is a reflective
optimiser over skill descriptions and tool hints: propose, score on train, gate on a frozen
holdout, write only on SHIP.

- **Split** `sha256(id) mod 100 < 60`, recorded in `routing-split.toml`, with `load_split` raising
  when file and rule disagree — the committed split this plan's Slice 1 step 5 asked for.
- **Four gates** (`optimize/gate.py`): paired bootstrap 95% lower bound above 0, false-hint rate not
  up, exact-name Recall@1 not down, live ladder within 0.05. All four or HOLD.
- **A priced ledger** with a hard cap, and `apply` that refuses anything but SHIP.
- **Status: HOLD.** The 2026-09-26 run moved holdout Recall@1 from 0.242 to 0.606 (paired diff
  +0.364, 95% [+0.182, +0.545]) and halved the false-hint rate, then failed on exact-name (one miss
  of seven) and on a missing ladder run. Nothing has been written to `skills/`.

So the mechanism exists end to end and has produced a candidate worth shipping on two of four
gates. What it has not produced is a shipped change, and the two things standing in the way are a
ladder run and a corpus relabel — see `specs/128-skill-routing-loop/results.md` § Next.

**Half of the nightly is dark.** `optimize drift` runs nightly at a $2 cap and has been passing.
The nightly canary behind it has not run since 2026-11-13: `skill-regression.yml` still starts
`intersystemsdc/iris-community:2025.3`, whose community license expired on exactly that date, so
"Wait for Atelier API" fails and the canary step is skipped. Eight consecutive red runs, 159
through 166. `ci.yml` was moved to 2026.1 with the `--entrypoint /tini -- /iris-main` workaround in
5381d17; the nightly never got the same treatment. A drift guard over a corpus nothing is measuring
is the 118 shape again, one layer up.

### What is still missing for RL proper

The pieces that only matter for policy training are the ones still absent: a vLLM-backed driver
(`DriverRun.logprobs` is `None` from every driver that exists), a rollout server, and a corpus an
order of magnitude larger. Harbor 0.26's HCP rollout server is new since this file was written and
is the first upstream piece that would plug into the exporter we already have.

## Open decisions

1. **Shared remote IRIS or compose sidecar** as the primary shape (drives the NoPWS question,
   the AWS topology, and whether cloud backends are usable at all). Recommendation: shared
   remote IRIS for throughput, sidecar retained for eval fidelity.
2. **Harness optimization or policy training** as the goal. Optimizing prompts, skills and the
   81-tool surface against a frontier model needs no GPUs and reuses `skill_eval` wholesale.
   Training a policy means open weights, GPUs, and a few hundred tasks before the gradient means
   anything. Building Tier 0/1 in Harbor format serves the first and leaves the second open.
3. **Terraform or CDK** for the single-node rig.
4. Whether to **publish the corpus** to the Hub (`repo2rlenv push`) or keep it in-repo.

## Sources

- [Repo2RLEnv](https://github.com/huggingface/Repo2RLEnv) ·
  [commit_runtime](https://raw.githubusercontent.com/huggingface/Repo2RLEnv/main/docs/pipelines/commit_runtime.md) ·
  [pr_runtime](https://raw.githubusercontent.com/huggingface/Repo2RLEnv/main/docs/pipelines/pr_runtime.md)
- [Harbor](https://github.com/laude-institute/harbor) ·
  [task structure](https://www.harborframework.com/docs/tasks) ·
  [MCP-server task tutorial](https://www.harborframework.com/docs/tutorials/mcp-server-task)
- [FlashREINFORCE](https://github.com/yifanzhang-pro/FlashREINFORCE) ·
  [paper PDF](https://yifanzhang-pro.github.io/FlashREINFORCE/FlashREINFORCE.pdf) ·
  [OpenRLHF #1339](https://github.com/OpenRLHF/OpenRLHF/pull/1339) ·
  [NeMo labs-molt #116](https://github.com/NVIDIA-NeMo/labs-molt/pull/116)
- [verifiers v1](https://www.primeintellect.ai/blog/verifiers-v1) ·
  [Scaling Agentic RL](https://www.primeintellect.ai/blog/scaling-agentic-rl) ·
  [Environments Hub](https://www.primeintellect.ai/blog/environments)
- [IRIS Community Edition on AWS Marketplace](https://aws.amazon.com/marketplace/pp/prodview-tdzm2pjb7opqs) ·
  [Deploy IRIS Community Edition in the cloud](https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=ACLOUD)
