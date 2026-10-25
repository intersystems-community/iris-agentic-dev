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
#   0  coverage collected, every declared test target ran, no test failed, all floors met
#   1  anything else. A failed test run, a target that did not run, an uninstrumented binary or an
#      empty report still produce a number where one can be measured, and then exit 1: the number
#      is printed so it can be read, and the exit code says whether to trust it.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# cargo and rustc resolve the toolchain and the workspace from the working directory.
cd "$REPO_ROOT"
CARGO="$HOME/.cargo/bin/cargo"

# Reasons the run cannot be trusted. Collected, not exited on, so a run that measured something
# still prints it; read once at the end, where the exit code is decided. A state the exit code
# cannot express is not a state.
FAILURES=()

[[ -x "$CARGO" ]] || { echo "ERROR: cargo not found at $CARGO"; exit 1; }

# Keep sccache out of the instrumented build, and do it with a real executable.
# cargo-llvm-cov reads build.rustc-wrapper literally, so an empty value gives it `" " + rustc`
# and nothing runs. /usr/bin/env is a passthrough here and does not exist on Windows, which is
# why .cargo/config.toml cannot carry it — see the comment there.
export CARGO_BUILD_RUSTC_WRAPPER="${CARGO_BUILD_RUSTC_WRAPPER:-/usr/bin/env}"

# ── Locate llvm-cov / llvm-profdata ───────────────────────────────────────────
#
# They must come from the toolchain that compiled the code: a profile written by one LLVM is not
# reliably readable by another. So ask rustc where its sysroot is and look there, with the host
# triple rustc reports. Only if that has no llvm-tools, look through RUSTUP_HOME for the same host.
#
# The test is on the OUTPUT. This used to be `find ... | head -1`, and head exits 0 on empty
# input, so "found nothing" looked like success; with no toolchains directory at all, find failed
# under pipefail and the script died with no output.

RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"

if [[ -z "${LLVM_COV:-}" ]]; then
    RUSTC_PATH="$HOME/.cargo/bin:$PATH"
    HOST_TRIPLE="$(PATH="$RUSTC_PATH" rustc -vV 2>/dev/null | sed -n 's/^host: //p')"
    SYSROOT="$(PATH="$RUSTC_PATH" rustc --print sysroot 2>/dev/null || true)"
    TOOLCHAIN_BIN=""
    if [[ -n "$HOST_TRIPLE" && -n "$SYSROOT" ]]; then
        CANDIDATE="$SYSROOT/lib/rustlib/$HOST_TRIPLE/bin/llvm-cov"
        if [[ -x "$CANDIDATE" ]]; then
            TOOLCHAIN_BIN="$CANDIDATE"
        fi
    fi
    if [[ -z "$TOOLCHAIN_BIN" && -n "$HOST_TRIPLE" ]]; then
        TOOLCHAIN_BIN="$(find "$RUSTUP_HOME/toolchains" -maxdepth 6 -name llvm-cov \
            -path "*/$HOST_TRIPLE/bin/*" 2>/dev/null | sort | head -1 || true)"
    fi
    LLVM_COV="$TOOLCHAIN_BIN"
fi
export LLVM_COV
export LLVM_PROFDATA="${LLVM_PROFDATA:-$(dirname "${LLVM_COV:-/nonexistent}")/llvm-profdata}"

[[ -n "$LLVM_COV" && -x "$LLVM_COV" ]] || {
    echo "ERROR: llvm-cov not found for the active toolchain. Run: ~/.cargo/bin/rustup component add llvm-tools" >&2
    exit 1
}
[[ -x "$LLVM_PROFDATA" ]] || { echo "ERROR: llvm-profdata not found alongside llvm-cov ($LLVM_PROFDATA)" >&2; exit 1; }

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
# Tests that spawn iris-agentic-dev (test_e2e.rs, test_admin_e2e.rs, the binary and bin
# integration targets) find it through IRIS_DEV_BIN / IAD_BINARY, or, under CARGO_LLVM_COV, at
# `<target>/llvm-cov-target/debug/iris-agentic-dev`. cargo-llvm-cov relocates its target
# directory there; a plain `cargo build` under show-env does not, it writes `target/debug/`. So
# build with CARGO_TARGET_DIR pointed at the relocated directory: the binary the tests look for
# is then the instrumented one, built once, and the later `cargo llvm-cov` run reuses it.
#
# show-env exports RUSTC_WRAPPER and related vars; we build while those are active, then unset
# them before Step 1 so the second cargo llvm-cov invocation does not double-wrap the compiler
# (os error 35).

echo "=== Step 0: Build instrumented binary ==="
eval "$(PATH="$HOME/.cargo/bin:$PATH" "$CARGO" llvm-cov show-env --sh 2>/dev/null)"

# show-env prints nothing when it cannot run. Building anyway gives an ordinary binary that
# writes no profile, and the merged report would read as "the subprocesses covered nothing".
if [[ -z "${CARGO_LLVM_COV:-}" ]]; then
    echo "ERROR: cargo llvm-cov show-env did not set up instrumentation; the binary would not be instrumented" >&2
    exit 1
fi

unset LLVM_PROFILE_FILE

LLVM_COV_TARGET="${CARGO_LLVM_COV_TARGET_DIR:-$REPO_ROOT/target}/llvm-cov-target"
BIN="$LLVM_COV_TARGET/debug/iris-agentic-dev"

BUILD_LOG="$COVERAGE_DIR/step0.log"
if ! CARGO_TARGET_DIR="$LLVM_COV_TARGET" \
    PATH="$HOME/.cargo/bin:$PATH" \
    "$CARGO" build \
        --features iris-agentic-dev-core/testing \
        -p iris-agentic-dev \
        >"$BUILD_LOG" 2>&1; then
    tail -20 "$BUILD_LOG" >&2
    echo "ERROR: instrumented build failed (full output: $BUILD_LOG)" >&2
    exit 1
fi
tail -5 "$BUILD_LOG"

[[ -f "$BIN" ]] || { echo "ERROR: instrumented binary not found at $BIN" >&2; exit 1; }
echo "Instrumented binary: $BIN"

unset RUSTC_WRAPPER CARGO_LLVM_COV __CARGO_LLVM_COV_RUSTC_WRAPPER \
      __CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS __CARGO_LLVM_COV_RUSTC_WRAPPER_CRATE_NAMES \
      CARGO_LLVM_COV_SHOW_ENV CARGO_LLVM_COV_TARGET_DIR CARGO_LLVM_COV_BUILD_DIR

# ── Step 1: Full test suite + lcov report in one pass ─────────────────────────
#
# IRIS_DEV_BIN and IAD_BINARY both name the instrumented binary. IAD_BINARY wins over
# IRIS_DEV_BIN in testing::iad_binary_path, so one exported by the caller (CI exports a relative
# ./target/debug one) would send every spawned-binary test to an uninstrumented binary and the
# subprocess half of the number would read zero.
# cargo-llvm-cov sets LLVM_PROFILE_FILE internally; the test harness propagates
# it to each spawned subprocess (see test_e2e.rs::mcp_call), so subprocess
# profraw lands in the same directory cargo-llvm-cov manages. When cargo
# llvm-cov generates the lcov report it merges all profraw in its target
# directory — including the subprocess ones — producing unified coverage.
#
# `--include-ignored`: the live-IRIS tests are #[ignore]d, and without the flag they are listed
# and skipped, and the TOTAL leaves out what they exercise.
#
# The status is captured from PIPESTATUS with errexit off. `cmd | tee` under pipefail and set -e
# would abort here on the first failing test, and the report, the target check and the floors
# below would never run.

LCOV="$COVERAGE_DIR/coverage.lcov"

echo ""
echo "=== Step 1: Full test suite (unit + integration + e2e) ==="
echo "    IRIS_HOST=$IRIS_HOST IRIS_WEB_PORT=$IRIS_WEB_PORT"
echo ""

set +e
IRIS_HOST="$IRIS_HOST" \
IRIS_WEB_PORT="$IRIS_WEB_PORT" \
IRIS_USERNAME="$IRIS_USERNAME" \
IRIS_PASSWORD="$IRIS_PASSWORD" \
IRIS_NAMESPACE="$IRIS_NAMESPACE" \
IRIS_CONTAINER="$IRIS_CONTAINER" \
IRIS_DEV_BIN="$BIN" \
IAD_BINARY="$BIN" \
PATH="$HOME/.cargo/bin:$PATH" \
"$CARGO" llvm-cov \
    --features testing \
    --no-fail-fast \
    --no-report \
    -- \
    --include-ignored \
    --test-threads=1 \
    2>&1 | tee "$COVERAGE_DIR/step1.log"
TEST_RC=${PIPESTATUS[0]}
set -e

if [[ "$TEST_RC" -ne 0 ]]; then
    echo ""
    echo "WARNING: the test run exited $TEST_RC; coverage is from a partial run (see $COVERAGE_DIR/step1.log)"
    FAILURES+=("the test run exited $TEST_RC (see $COVERAGE_DIR/step1.log)")
fi

# ── Did the run execute the suite the workspace declares? ────────────────────
#
# Derived from `cargo metadata`, not remembered: a run that executed fewer targets than the
# workspace declares prints a smaller TOTAL that looks exactly like a complete one. Counts the
# lib, bin and test targets cargo runs as tests; examples and build scripts are listed by
# metadata and never run. `--tests` is what `cargo llvm-cov` selects with no target flags.

DECLARED=$("$CARGO" metadata --no-deps --format-version 1 | python3 -c '
import json, sys
d = json.load(sys.stdin)
print(sum(1 for p in d["packages"] for t in p["targets"]
          if t.get("test") and set(t["kind"]) & {"lib", "bin", "test"}))
') || DECLARED=0
RAN=$(grep -cE '^[[:space:]]+Running ' "$COVERAGE_DIR/step1.log" || true)
echo "Test targets declared: $DECLARED, executed: $RAN"
if [[ "$DECLARED" -eq 0 ]]; then
    FAILURES+=("cargo metadata reported no test targets, so the run cannot be checked against the workspace")
elif [[ "$RAN" -lt "$DECLARED" ]]; then
    FAILURES+=("only $RAN of $DECLARED declared test targets executed; the TOTAL leaves out the rest")
fi

# Generate the lcov report from profraw data collected above.
# Runs even when some tests failed (--no-fail-fast + --no-report above).
set +e
PATH="$HOME/.cargo/bin:$PATH" \
"$CARGO" llvm-cov report \
    --lcov \
    --output-path "$LCOV" \
    2>&1 | tee -a "$COVERAGE_DIR/step1.log"
REPORT_RC=${PIPESTATUS[0]}
set -e

if [[ "$REPORT_RC" -ne 0 || ! -f "$LCOV" ]]; then
    echo "ERROR: lcov report not generated (exit $REPORT_RC; see $COVERAGE_DIR/step1.log)" >&2
    exit 1
fi
# A report with no source files measured nothing. Existence is not enough.
if ! grep -q '^SF:' "$LCOV"; then
    echo "ERROR: $LCOV has no source files; nothing was measured (see $COVERAGE_DIR/step1.log)" >&2
    exit 1
fi
echo "lcov report: $LCOV ($(wc -l < "$LCOV") lines)"

# ── Step 2: Floor check ───────────────────────────────────────────────────────

if [[ "${SKIP_FLOORS:-}" == "1" ]]; then
    echo ""
    echo "Floor check skipped (SKIP_FLOORS=1)"
    echo "lcov: $LCOV"
else
    echo ""
    echo "=== Step 2: Coverage floor check ==="
    set +e
    python3 "$REPO_ROOT/scripts/check-coverage-floors.py" \
        --floors "$REPO_ROOT/coverage-floors.toml" \
        --lcov "$LCOV" \
        --src "$REPO_ROOT/crates/iris-agentic-dev-core/src"
    FLOORS_RC=$?
    set -e
    if [[ "$FLOORS_RC" -ne 0 ]]; then
        FAILURES+=("the coverage floor check failed (exit $FLOORS_RC)")
    fi
fi

# ── Verdict ───────────────────────────────────────────────────────────────────
# `if`, not `[[ ... ]] && ...`: an AND-list as the last statement makes a false test the exit
# status of a complete, correct run.
if [[ ${#FAILURES[@]} -gt 0 ]]; then
    echo "" >&2
    echo "FAILED: the coverage run cannot be trusted as a pass:" >&2
    for f in "${FAILURES[@]}"; do
        echo "  - $f" >&2
    done
    exit 1
fi
exit 0
