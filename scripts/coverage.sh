#!/usr/bin/env bash
# Subprocess-merged coverage for iris-agentic-dev.
#
# Builds the instrumented binary first, then runs the full test suite with
# IRIS_DEV_BIN set so subprocesses spawned by e2e tests (test_e2e.rs,
# test_admin_e2e.rs) emit coverage data into the same profraw directory
# that cargo-llvm-cov manages. The report is generated in one step so all
# profraw — test binary and subprocess — are merged before reporting.
#
# Usage:
#   IRIS_HOST=localhost IRIS_WEB_PORT=52780 bash scripts/coverage.sh
#
# Optional env vars:
#   COVERAGE_DIR   — output directory (default: target/coverage)
#   SKIP_FLOORS    — set to 1 to skip floor check (measure-only mode)
#
# Prerequisites:
#   ~/.cargo/bin/rustup component add llvm-tools
#   cargo install cargo-llvm-cov --locked
#
# Exit codes:
#   0  coverage collected and all floors met
#   1  floor violation or build/test failure

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO="$HOME/.cargo/bin/cargo"

[[ -x "$CARGO" ]] || { echo "ERROR: cargo not found at $CARGO"; exit 1; }

# Keep sccache out of the instrumented build, and do it with a real executable.
# cargo-llvm-cov reads build.rustc-wrapper literally, so an empty value gives it `" " + rustc`
# and nothing runs. /usr/bin/env is a passthrough here and does not exist on Windows, which is
# why .cargo/config.toml cannot carry it — see the comment there.
export CARGO_BUILD_RUSTC_WRAPPER="${CARGO_BUILD_RUSTC_WRAPPER:-/usr/bin/env}"

# ── Locate llvm-cov / llvm-profdata ───────────────────────────────────────────

RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
TOOLCHAIN_BIN=$(find "$RUSTUP_HOME/toolchains" -maxdepth 6 -name llvm-cov \
    -path "*/bin/*" 2>/dev/null | head -1)
export LLVM_COV="${LLVM_COV:-$TOOLCHAIN_BIN}"
export LLVM_PROFDATA="${LLVM_PROFDATA:-${TOOLCHAIN_BIN/llvm-cov/llvm-profdata}}"

[[ -x "$LLVM_COV" ]]      || { echo "ERROR: llvm-cov not found. Run: ~/.cargo/bin/rustup component add llvm-tools"; exit 1; }
[[ -x "$LLVM_PROFDATA" ]] || { echo "ERROR: llvm-profdata not found alongside llvm-cov"; exit 1; }

# ── Directories ───────────────────────────────────────────────────────────────

COVERAGE_DIR="${COVERAGE_DIR:-$REPO_ROOT/target/coverage}"

# Clear previous output files.
find "$COVERAGE_DIR" -maxdepth 3 -name "*.lcov" -delete 2>/dev/null || true
find "$COVERAGE_DIR" -maxdepth 3 -name "*.txt" -delete 2>/dev/null || true
mkdir -p "$COVERAGE_DIR"

IRIS_HOST="${IRIS_HOST:-localhost}"
IRIS_WEB_PORT="${IRIS_WEB_PORT:-52780}"
IRIS_USERNAME="${IRIS_USERNAME:-_SYSTEM}"
IRIS_PASSWORD="${IRIS_PASSWORD:-SYS}"
IRIS_NAMESPACE="${IRIS_NAMESPACE:-USER}"
IRIS_CONTAINER="${IRIS_CONTAINER:-iris-dev-iris}"

# ── Step -1: Drop stale instrumented objects ──────────────────────────────────
#
# llvm-cov reports on every object file it finds under llvm-cov-target, not just
# the ones this run built. A test binary left over from an earlier build carries
# its own instrumented copy of the library, and that copy never executes, so the
# same source file gets counted twice: once covered, once dark. Measured on
# 2026-09-04: data_policy_gate.rs read 50.00% with four copies of the core crate
# in the report (three stale), 98.04% after a clean. Overall read 75.64% against
# a floor of 88 and nine files looked to be below floor, all of it leftovers.
#
# So clean first. It costs a full rebuild. A coverage number that depends on what
# happens to be lying in the target directory is not a number.
#
# COVERAGE_NO_CLEAN=1 skips it. Only for iterating locally — never for the gate.

if [[ "${COVERAGE_NO_CLEAN:-}" == "1" ]]; then
    echo "=== Step -1: SKIPPED (COVERAGE_NO_CLEAN=1) — numbers may be diluted by stale objects ==="
else
    echo "=== Step -1: Clean stale instrumented objects ==="
    PATH="$HOME/.cargo/bin:$PATH" "$CARGO" llvm-cov clean --workspace
fi
echo ""

# ── Step 0: Build the instrumented binary ─────────────────────────────────────
#
# We build the iris-agentic-dev binary under cargo-llvm-cov's instrumentation
# wrapper so e2e tests can spawn it as IRIS_DEV_BIN and get subprocess coverage.
# show-env exports RUSTC_WRAPPER and related vars; we then cargo build while
# those are active, then unset them before Step 1 so the second cargo llvm-cov
# invocation doesn't double-wrap the compiler (os error 35).

echo "=== Step 0: Build instrumented binary ==="
eval "$(PATH="$HOME/.cargo/bin:$PATH" "$CARGO" llvm-cov show-env --sh 2>/dev/null)"

unset LLVM_PROFILE_FILE

PATH="$HOME/.cargo/bin:$PATH" \
"$CARGO" build \
    --features iris-agentic-dev-core/testing \
    -p iris-agentic-dev \
    2>&1 | tail -5

BIN="${CARGO_LLVM_COV_TARGET_DIR:-$REPO_ROOT/target}/llvm-cov-target/debug/iris-agentic-dev"
[[ -f "$BIN" ]] || BIN="$REPO_ROOT/target/debug/iris-agentic-dev"
[[ -f "$BIN" ]] || { echo "ERROR: instrumented binary not found"; exit 1; }
echo "Instrumented binary: $BIN"

unset RUSTC_WRAPPER CARGO_LLVM_COV __CARGO_LLVM_COV_RUSTC_WRAPPER \
      __CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS __CARGO_LLVM_COV_RUSTC_WRAPPER_CRATE_NAMES \
      CARGO_LLVM_COV_SHOW_ENV CARGO_LLVM_COV_TARGET_DIR CARGO_LLVM_COV_BUILD_DIR

# ── Step 1: Full test suite + lcov report in one pass ─────────────────────────
#
# IRIS_DEV_BIN points at the instrumented binary so e2e tests spawn it.
# cargo-llvm-cov sets LLVM_PROFILE_FILE internally; the test harness propagates
# it to each spawned subprocess (see test_e2e.rs::mcp_call), so subprocess
# profraw lands in the same directory cargo-llvm-cov manages. When cargo
# llvm-cov generates the lcov report it merges all profraw in its target
# directory — including the subprocess ones — producing unified coverage.

LCOV="$COVERAGE_DIR/coverage.lcov"

echo ""
echo "=== Step 1: Full test suite (unit + integration + e2e) ==="
echo "    IRIS_HOST=$IRIS_HOST IRIS_WEB_PORT=$IRIS_WEB_PORT"
echo ""

IRIS_HOST="$IRIS_HOST" \
IRIS_WEB_PORT="$IRIS_WEB_PORT" \
IRIS_USERNAME="$IRIS_USERNAME" \
IRIS_PASSWORD="$IRIS_PASSWORD" \
IRIS_NAMESPACE="$IRIS_NAMESPACE" \
IRIS_CONTAINER="$IRIS_CONTAINER" \
IRIS_DEV_BIN="$BIN" \
PATH="$HOME/.cargo/bin:$PATH" \
"$CARGO" llvm-cov \
    --features testing \
    --no-fail-fast \
    --no-report \
    -- \
    --include-ignored \
    --test-threads=1 \
    2>&1 | tee "$COVERAGE_DIR/step1.log" || {
    echo ""
    echo "WARNING: some tests failed — coverage will be partial (see $COVERAGE_DIR/step1.log)"
    echo ""
}

# Generate the lcov report from profraw data collected above.
# Runs even when some tests failed (--no-fail-fast + --no-report above).
PATH="$HOME/.cargo/bin:$PATH" \
"$CARGO" llvm-cov report \
    --lcov \
    --output-path "$LCOV" \
    2>&1 | tee -a "$COVERAGE_DIR/step1.log" || true

[[ -f "$LCOV" ]] || { echo "ERROR: lcov report not generated (see $COVERAGE_DIR/step1.log)"; exit 1; }
echo "lcov report: $LCOV ($(wc -l < "$LCOV") lines)"

# ── Step 2: Floor check ───────────────────────────────────────────────────────

if [[ "${SKIP_FLOORS:-}" == "1" ]]; then
    echo ""
    echo "Floor check skipped (SKIP_FLOORS=1)"
    echo "lcov: $LCOV"
    exit 0
fi

echo ""
echo "=== Step 2: Coverage floor check ==="
python3 "$REPO_ROOT/scripts/check-coverage-floors.py" \
    --floors "$REPO_ROOT/coverage-floors.toml" \
    --lcov "$LCOV" \
    --src "$REPO_ROOT/crates/iris-agentic-dev-core/src"
