# Implementation Plan: Harbor task export and RL environment for iris-agentic-dev

**Branch**: `claude/repo2rlenv-iris-setup-w8xij5` | **Date**: 2026-09-16 | **Research**: [research.md](./research.md)

**Nothing here is implemented yet.** This is a forward-looking proposal written before any code,
and two of its decisions are still open (see [research.md](./research.md#open-decisions)). There
is no `spec.md` or `tasks.md` because scope is not agreed.

## Summary

Export our existing task corpus to the [Harbor](https://github.com/laude-institute/harbor) task
format so the same tasks can drive evaluation, cross-agent comparison, and — if we go that way
later — RL rollouts, without rewriting the corpus for each. The reward function already exists in
`benchmark/021/runner/judge.py` and `tests/e2e/skill_eval/scoring.py`; the missing pieces are a
format, a declared agent-driver boundary, and a train/holdout split.

Deliberately trainer-agnostic. Harbor needs a scalar reward per trajectory and nothing else, so
this work is unchanged whether we end up optimizing a harness around a frontier model or
training an open-weights policy.

## Technical Context

**Language/Version**: Python 3.11 (harness side; matches `benchmark/021/runner` and
`tests/e2e/`), Rust 2021 untouched
**Primary Dependencies**: `harbor` CLI for running tasks, `repo2rlenv` for mining only. No new
runtime dependency in either Rust crate.
**Storage**: task dirs on disk; S3 later if we fan out on AWS
**Testing**: `python -m pytest`, plus `cargo test --features testing` unaffected
**Target Platform**: Linux/macOS for generation; Harbor's native Windows execution is incomplete
**Constraints**: multi-container tasks run only on Harbor's local `--env docker`; IRIS is reached
over Atelier REST, so `docker_only=true` (NoPWS) and shared-remote-IRIS are mutually exclusive
**Scale/Scope**: 5 new files, ~2 modified, no changes to the MCP server

## Slice 1 — the parts that hold regardless of the open decisions

1. **`benchmark/harbor/export.py`** — one `benchmark/021` task YAML in, one Harbor task dir out
   (`instruction.md`, `task.toml`, `environment/`, `tests/test.sh`, `solution/solve.sh`).
   Fixtures stay inline data, written into `environment/` at build time; do not bake a per-task
   image.

2. **One Tier-2 task with a real sidecar** — `environment/docker-compose.yaml` running
   `intersystemsdc/iris-community:2025.3` (ci.yml's last known-good), proving
   `harbor run --env docker` goes green locally end to end. The readiness poll and credential
   probe come from `ci.yml`'s `e2e-tests` job; do not reinvent them.

3. **One Tier-0 task from `commit_runtime`** — run
   `repo2rlenv generate --pipeline commit_runtime` over our `fix:` history and see what its
   bootstrap actually does with a Rust workspace. This is a measurement, not a commitment: the
   output may be unusable, and that is a useful result. 26 of the 58 candidates need no IRIS.

4. **Declared agent-driver interface** — prompt in; transcript, tool-call log, scalar reward,
   `scored` flag, and optional per-token behavior logprobs out. `claude_code.py`, `copilot.py`
   and `opencode_runner.py` become implementations returning `logprobs: None`. A vLLM-backed
   driver is then an addition rather than a rewrite, and it is the only way a
   FlashREINFORCE-style trainer can ever consume our rollouts.

5. **Train/holdout split as a committed file**, with a guard test asserting no task ID appears in
   both. At this corpus size memorization is the default outcome, and after the fact there is no
   way to claim otherwise. Do this before the first rollout, not after.

6. **Provenance on every rollout record** — extend `tests/e2e/skill_eval/provenance.py` to stamp
   policy version and driver kind. A trust-region filter needs to know which policy produced a
   trajectory; the field should exist before there is a trainer to read it.

7. **Unscored trajectories are dropped, not zeroed**, at the rollout boundary. `scoring.py`
   already refuses the two bad shapes (`scored=False` holding a number, `scored=True` holding
   none); the export and rollout paths must honor the same contract, because a batch-centered
   advantage lets one mis-scored item bias every other trajectory in its batch.

Then decide cloud versus on-prem with real timings in hand, and price the GPU line only if we
choose policy training.

## Test plan

Per the Test Coverage Policy, at the layer that would actually catch a silent break:

| Change             | Test                                                                                                                                                    |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `export.py`        | TOML round-trip: parse the emitted `task.toml` **string**, assert fields — not a dict literal (the #110 pattern)                                        |
| `export.py`        | Golden-file test for one task dir, so a format drift in Harbor shows up as a diff                                                                       |
| Sidecar task       | `#[ignore]`-equivalent marker; runs against a live container, skipped when Docker is absent                                                             |
| Driver interface   | A fake driver asserting the contract, including `logprobs: None` and `scored=False` paths                                                               |
| Holdout split      | Guard test: intersection of train and holdout task IDs is empty, and their union is the corpus                                                          |
| Corpus consistency | Reuse `tests/e2e/skill_eval/test_task_corpus.py`'s pattern — a referenced task file that is untracked by git is green locally and red in CI, every time |

## Out of scope

- Any change to the MCP server, its tools, or either Rust crate.
- GPU provisioning, trainer configuration, actual RL runs.
- Publishing the corpus to the Hugging Face Hub (`repo2rlenv push`) — decision 4.
- EKS. The single-node EC2 rig first; Batch fan-out once the format is stable.

## Risks

- **Harbor format drift.** It is young and moving. The golden-file test is the tripwire; keep the
  exporter the only place that knows the layout.
- **`commit_runtime` on Rust may yield nothing usable.** Its docs admit Rust parsers are
  experimental. Slice 1 step 3 is scoped to find that out cheaply rather than to depend on it.
- **IRIS image license expiry.** Community images expire ~150 days after publish, which rots any
  frozen environment image. Mirror to ECR and pin; prefer the Marketplace AMI's ~1-year license
  or a real key for anything long-lived.
- **Corpus too small for training.** ~100 tasks is an eval corpus, not a training corpus. If we
  go the training route, corpus growth is the project, and this slice is its prerequisite.
