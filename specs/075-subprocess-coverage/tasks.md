# 075 tasks — Subprocess profraw coverage merge

## Phase 1 — Tooling

- [X] T001 Write `scripts/coverage.sh`:
  - Builds instrumented binary via `cargo llvm-cov show-env --sh` + `cargo build`
  - Sets `LLVM_PROFILE_FILE` so spawned subprocesses emit profraw to `target/coverage/profraw/`
  - Runs full test suite via `cargo llvm-cov --no-report --include-ignored --test-threads=1`
  - Merges all profraw files with `llvm-profdata merge -sparse`
  - Generates lcov and per-file summary with `llvm-cov export/report`
  - Calls `check-coverage-floors.py --lcov` for gate check
  - Fixed 2026-08-08: `--no-run + --no-report` invalid in cargo-llvm-cov 0.8.7; use `show-env --sh` + unset wrapper vars before Step 1

- [X] T002 `check-coverage-floors.py` already supports `--lcov <file>` mode; `[floors] overall = N`
  already supported. No changes needed.

- [X] T003 Smoke-run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 bash scripts/coverage.sh`
  exited 0, produced `target/coverage/coverage.lcov` (non-empty). All 54 files passed floors.

- [X] T004 Baseline measured: **88.48% src-only line coverage** (2026-08-08, master, with ws_session
  bugfix and full subprocess profraw merge). Recorded in `coverage-floors.toml`.

- [X] T005 Updated `coverage-floors.toml`:
  - `overall = 86` (88.48% - 2pp)
  - `ws_session.rs` floor raised 21 → 68 (70.27% lcov - 2pp; WS e2e tests now run)
  - Per-file floors for dispatch-layer files (mod.rs, admin_tools.rs, etc.) remain at
    existing values — all pass with margin in merged-report mode

## Phase 2 — CI

- [ ] T006 Update `.github/workflows/ci.yml`:
  - Replace bare `cargo llvm-cov` step with `bash scripts/coverage.sh`
  - Upload `coverage.lcov` as a CI artifact
  - Cache the instrumented build to avoid rebuilding on every push

## Phase 3 — Floor raise

- [ ] T007 Raise `overall` floor toward 90 in 2pp increments per release
  until ≥ 90. Document each raise in release notes.

---

## Notes

- `llvm-profdata` and `llvm-cov` are the LLVM tools, not the cargo wrapper.
  Path: `$(rustup show home)/toolchains/$(rustup show active-toolchain | cut -d' ' -f1)/lib/rustlib/$(rustc -vV | grep host | cut -d' ' -f2)/bin/`
  Or: set `LLVM_COV` and `LLVM_PROFDATA` env vars that `cargo-llvm-cov` also respects.
- The binary must be the same build artifact used during the test run (same rustc invocation).
  `coverage.sh` Step 0 builds via show-env vars so the binary is in `target/llvm-cov-target/`.
- `ws_session.rs` coverage: 70.27% after WS e2e bugfix (was 21%). Remaining gap is
  error-path branches in async WS protocol handlers; requires network mocking to reach 90%.
