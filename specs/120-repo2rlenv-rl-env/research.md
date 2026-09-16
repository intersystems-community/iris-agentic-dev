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
