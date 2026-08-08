#!/usr/bin/env bash
# Subprocess-merged coverage for iris-agentic-dev.
#
# Builds the instrumented binary first, then runs the full test suite with
# LLVM_PROFILE_FILE and IRIS_DEV_BIN set so subprocesses spawned by e2e tests
# (test_e2e.rs, test_admin_e2e.rs) emit coverage data. All profraw files are
# merged into a single lcov report and checked against coverage-floors.toml.
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
PROFRAW_DIR="$COVERAGE_DIR/profraw"

# Clear previous output files.
find "$COVERAGE_DIR" -maxdepth 3 -name "*.profraw" -delete 2>/dev/null || true
find "$COVERAGE_DIR" -maxdepth 3 -name "*.profdata" -delete 2>/dev/null || true
find "$COVERAGE_DIR" -maxdepth 3 -name "*.lcov" -delete 2>/dev/null || true
find "$COVERAGE_DIR" -maxdepth 3 -name "*.txt" -delete 2>/dev/null || true
find "$COVERAGE_DIR" -maxdepth 3 -name "*.list" -delete 2>/dev/null || true
mkdir -p "$PROFRAW_DIR"

IRIS_HOST="${IRIS_HOST:-localhost}"
IRIS_WEB_PORT="${IRIS_WEB_PORT:-52780}"
IRIS_USERNAME="${IRIS_USERNAME:-_SYSTEM}"
IRIS_PASSWORD="${IRIS_PASSWORD:-SYS}"
IRIS_NAMESPACE="${IRIS_NAMESPACE:-USER}"
IRIS_CONTAINER="${IRIS_CONTAINER:-iris-dev-iris}"

# ── Step 0: Build the instrumented binary ─────────────────────────────────────

echo "=== Step 0: Build instrumented binary ==="
# cargo llvm-cov show-env prints the env vars that enable instrumented compilation.
# Eval them, then build with cargo build so we get the binary without running tests.
# This sets RUSTC_WRAPPER, LLVM_PROFILE_FILE, and related vars.
eval "$(PATH="$HOME/.cargo/bin:$PATH" "$CARGO" llvm-cov show-env --sh 2>/dev/null)"

# Override LLVM_PROFILE_FILE — we will set our own location in Step 1.
unset LLVM_PROFILE_FILE

PATH="$HOME/.cargo/bin:$PATH" \
"$CARGO" build \
    --features iris-agentic-dev-core/testing \
    -p iris-agentic-dev \
    2>&1 | tail -5

# cargo-llvm-cov builds to target/llvm-cov-target when CARGO_LLVM_COV_TARGET_DIR is set.
BIN="${CARGO_LLVM_COV_TARGET_DIR:-$REPO_ROOT/target}/llvm-cov-target/debug/iris-agentic-dev"
[[ -f "$BIN" ]] || BIN="$REPO_ROOT/target/debug/iris-agentic-dev"
[[ -f "$BIN" ]] || { echo "ERROR: instrumented binary not found. Expected at ${CARGO_LLVM_COV_TARGET_DIR:-target}/llvm-cov-target/debug/iris-agentic-dev"; exit 1; }
echo "Instrumented binary: $BIN"

# Unset the instrumentation env vars set by show-env — Step 1 re-enters cargo llvm-cov
# and those vars would double-wrap the compiler, causing "resource temporarily unavailable".
unset RUSTC_WRAPPER CARGO_LLVM_COV __CARGO_LLVM_COV_RUSTC_WRAPPER \
      __CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS __CARGO_LLVM_COV_RUSTC_WRAPPER_CRATE_NAMES \
      CARGO_LLVM_COV_SHOW_ENV CARGO_LLVM_COV_TARGET_DIR CARGO_LLVM_COV_BUILD_DIR

# ── Step 1: Full test suite with subprocess profraw capture ───────────────────

echo ""
echo "=== Step 1: Full test suite (unit + integration + e2e) ==="
echo "    IRIS_HOST=$IRIS_HOST IRIS_WEB_PORT=$IRIS_WEB_PORT"
echo ""

# LLVM_PROFILE_FILE: each spawned process writes its own profraw (unique via %p/%m).
# IRIS_DEV_BIN: subprocess-spawning tests use the instrumented binary, not target/debug.
# Both env vars are propagated by test_e2e.rs::mcp_call() and test_admin_e2e.rs::T118.
export LLVM_PROFILE_FILE="$PROFRAW_DIR/iad-%p-%m.profraw"
export IRIS_DEV_BIN="$BIN"

IRIS_HOST="$IRIS_HOST" \
IRIS_WEB_PORT="$IRIS_WEB_PORT" \
IRIS_USERNAME="$IRIS_USERNAME" \
IRIS_PASSWORD="$IRIS_PASSWORD" \
IRIS_NAMESPACE="$IRIS_NAMESPACE" \
IRIS_CONTAINER="$IRIS_CONTAINER" \
IRIS_DEV_BIN="$IRIS_DEV_BIN" \
LLVM_PROFILE_FILE="$LLVM_PROFILE_FILE" \
PATH="$HOME/.cargo/bin:$PATH" \
"$CARGO" llvm-cov \
    --features testing \
    --no-report \
    --no-fail-fast \
    -- \
    --include-ignored \
    --test-threads=1 \
    2>&1 | tee "$COVERAGE_DIR/step1.log" || {
    echo ""
    echo "WARNING: some tests failed — coverage will be partial (see $COVERAGE_DIR/step1.log)"
    echo ""
}

# ── Step 2: Merge all profraw files ───────────────────────────────────────────

echo ""
echo "=== Step 2: Merging profraw files ==="

# Collect from cargo-llvm-cov's target dir AND our subprocess profraw dir.
PROFRAW_LIST="$COVERAGE_DIR/profraw.list"
{
    find "$REPO_ROOT/target" -name "*.profraw" -not -path "*/coverage/profraw/*"
    find "$PROFRAW_DIR" -name "*.profraw"
} | sort -u > "$PROFRAW_LIST"

PROFRAW_COUNT=$(wc -l < "$PROFRAW_LIST" | tr -d ' ')
echo "Found $PROFRAW_COUNT profraw file(s)"

if [[ "$PROFRAW_COUNT" -eq 0 ]]; then
    echo "ERROR: no profraw files found — instrumented build may have failed"
    exit 1
fi

PROFDATA="$COVERAGE_DIR/merged.profdata"
# Use xargs to handle large numbers of profraw files without hitting ARG_MAX.
tr '\n' '\0' < "$PROFRAW_LIST" | \
    xargs -0 "$LLVM_PROFDATA" merge -sparse -o "$PROFDATA"
echo "Merged profdata: $PROFDATA"

# ── Step 3: Generate lcov and summary ─────────────────────────────────────────

echo ""
echo "=== Step 3: Generating lcov report ==="

LCOV="$COVERAGE_DIR/coverage.lcov"
SUMMARY="$COVERAGE_DIR/summary.txt"

# Collect all instrumented object files: main binary + test binaries.
# llvm-cov needs each object to map counter IDs back to source locations.
OBJECTS_ARGS=("$BIN")
while IFS= read -r obj; do
    name=$(basename "$obj")
    if [[ "$name" != *"."* ]] && [[ -x "$obj" ]]; then
        OBJECTS_ARGS+=("-object=$obj")
    fi
done < <(find "$REPO_ROOT/target/llvm-cov-target/debug/deps" \
    -maxdepth 1 -type f -newer "$REPO_ROOT/Cargo.toml" 2>/dev/null)

echo "Using ${#OBJECTS_ARGS[@]} object file(s)"

"$LLVM_COV" export \
    "${OBJECTS_ARGS[0]}" \
    "${OBJECTS_ARGS[@]:1}" \
    -instr-profile="$PROFDATA" \
    -format=lcov \
    -ignore-filename-regex='(/.cargo/registry|/rustc/|rust-toolchain|/tmp/rust-)' \
    2>/dev/null \
    > "$LCOV"

echo "lcov report: $LCOV ($(wc -l < "$LCOV") lines)"

echo ""
echo "=== Step 3b: Summary ==="
"$LLVM_COV" report \
    "${OBJECTS_ARGS[0]}" \
    "${OBJECTS_ARGS[@]:1}" \
    -instr-profile="$PROFDATA" \
    -ignore-filename-regex='(/.cargo/registry|/rustc/|rust-toolchain|/tmp/rust-)' \
    2>/dev/null | tee "$SUMMARY" | grep -E "^TOTAL|iris-agentic-dev-core/src"

# ── Step 4: Floor check ───────────────────────────────────────────────────────

if [[ "${SKIP_FLOORS:-}" == "1" ]]; then
    echo ""
    echo "Floor check skipped (SKIP_FLOORS=1)"
    echo "lcov: $LCOV"
    echo "summary: $SUMMARY"
    exit 0
fi

echo ""
echo "=== Step 4: Coverage floor check ==="
python3 "$REPO_ROOT/scripts/check-coverage-floors.py" \
    --floors "$REPO_ROOT/coverage-floors.toml" \
    --lcov "$LCOV" \
    --src "$REPO_ROOT/crates/iris-agentic-dev-core/src"
