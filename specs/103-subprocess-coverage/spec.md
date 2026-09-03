# 075 — Subprocess profraw coverage merge

**Status: NOT IMPLEMENTED.** Spec only — no plan.md, no tasks.md, no code. Nothing described
below exists in the build. 073 and 074 sat in exactly this state while `docs/tools.md`
described them as shipped, which is how a security gate nobody had written stayed documented
for two releases (see 085). Do not cite this file as behaviour.

## Problem

Overall line coverage is stuck at ~49% despite most pure-logic files being at 94–98%.
The ceiling is `src/tools/mod.rs` (9,652 lines, 37% covered) — the MCP `call_tool`
dispatch function. Every tool invocation arm only executes when the binary receives a
JSON-RPC request over stdin; the in-process `cargo test` harness never reaches it.

The same gap affects:

| File                         | Floor | Why low                                    |
| ---------------------------- | ----- | ------------------------------------------ |
| `src/tools/mod.rs`           | 37%   | MCP dispatch arms, run only via subprocess |
| `src/tools/observability.rs` | 17%   | Tool bodies calling execute_via_generator  |
| `src/tools/coverage.rs`      | 23%   | Spawns cargo llvm-cov subprocess           |
| `src/iris/ws_session.rs`     | 21%   | WebSocket session, live connections only   |
| `src/iris/discovery.rs`      | 32%   | Docker API calls, runtime only             |

T028 (spec 071) identified the root cause: when the e2e test binary spawns
`iris-agentic-dev` as a subprocess, the binary is not instrumented and its `.profraw`
is never merged. Estimated gap: ~6,300 covered lines invisible to the counter.

Baseline at v0.9.5: **89.25%**. Today (v1.0.0 with new tools): **~49%**. The delta is
entirely the subprocess coverage gap, not regression in the covered code.

## Goal

Merge coverage from the live MCP subprocess into the llvm-cov report so the 90% gate
is achievable.

## Approach

### Phase 1 — Instrumented binary profraw capture

Build `iris-agentic-dev` with `RUSTFLAGS="-C instrument-coverage"` and set
`LLVM_PROFILE_FILE` so each invocation writes a `.profraw` to a temp directory. The
existing `test_e2e` integration tests already spawn the binary with configurable env;
adding two env vars is enough.

Steps:

1. Add a `scripts/coverage.sh` that:
   - Builds the binary with instrumentation (`cargo build --features testing`)
   - Sets `LLVM_PROFILE_FILE="$TMPDIR/iad-%p-%m.profraw"`
   - Runs the full test suite (`cargo test --features testing --include-ignored --test-threads=1`)
   - Merges all `.profraw` files with `llvm-profdata merge`
   - Generates an `lcov` report with `llvm-cov export --format=lcov`
   - Runs `scripts/check-coverage-floors.sh` against the merged report
2. Teach `check-coverage-floors.sh` to accept an `--lcov <file>` argument so it can
   parse the merged report instead of re-running `cargo llvm-cov`.

### Phase 2 — CI integration

Replace the current `cargo llvm-cov` step in `.github/workflows/ci.yml` with
`scripts/coverage.sh`. The 90% overall floor becomes enforceable.

### Phase 3 — Raise overall floor to 90%

Once merged coverage is measured, update `coverage-floors.toml`:

- Set `overall = 88` initially (2pp below first merged measurement).
- Raise per-file floors for `mod.rs`, `observability.rs`, etc. to match new reality.
- Target: raise overall floor to 90 once per-file gaps are closed.

## Out of scope

- Coverage for `ws_session.rs` (WebSocket) and `discovery.rs` (Docker API) — these
  need real network I/O and are excluded from the subprocess merge by design. Their
  floors stay low.
- New tests for already-covered files. This spec is about tooling, not test authorship.

## Success criteria

1. `scripts/coverage.sh` runs end-to-end and produces a merged lcov report.
2. `check-coverage-floors.sh --lcov <merged.lcov>` passes against updated floors.
3. Overall line coverage ≥ 88% with merged report.
4. CI runs `scripts/coverage.sh` instead of bare `cargo llvm-cov`.
5. `coverage-floors.toml` has `overall` entry ≥ 88.
