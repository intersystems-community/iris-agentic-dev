#!/usr/bin/env bash
# Check per-file coverage floors defined in coverage-floors.toml.
#
# Every src/ file must have an entry in coverage-floors.toml. Adding new code
# without registering a floor is a CI failure — this is the drift guard.
#
# Usage:
#   ./scripts/check-coverage-floors.sh           # unit tests only, no IRIS needed
#
# Exit codes:
#   0  all files at or above their floor, no unregistered files
#   1  one or more violations

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO="$HOME/.cargo/bin/cargo"

[[ -x "$CARGO" ]] || { echo "ERROR: cargo not found at $CARGO"; exit 1; }

# Same passthrough scripts/coverage.sh sets, for the same reason: cargo-llvm-cov reads
# build.rustc-wrapper literally and an empty value leaves it unable to run rustc at all.
# .cargo/config.toml cannot hold this — /usr/bin/env does not exist on Windows.
export CARGO_BUILD_RUSTC_WRAPPER="${CARGO_BUILD_RUSTC_WRAPPER:-/usr/bin/env}"

# Stale objects under llvm-cov-target get reported alongside the ones this run
# builds. A leftover test binary holds its own instrumented copy of the library
# that never executes, so a source file is counted twice — once covered, once
# dark — and every file reads low. See the Step -1 comment in coverage.sh for the
# measurement. Clean first; a diluted floor check is worse than none.
if [[ "${COVERAGE_NO_CLEAN:-}" == "1" ]]; then
  echo "=== SKIPPING clean (COVERAGE_NO_CLEAN=1) — numbers may be diluted ==="
else
  echo "=== Clean stale instrumented objects ==="
  PATH="$HOME/.cargo/bin:$PATH" "$CARGO" llvm-cov clean --workspace
fi

echo "=== Running unit coverage (no IRIS required) ==="
COVERAGE_OUT="$REPO_ROOT/target/coverage-raw.txt"
mkdir -p "$REPO_ROOT/target"

# Capture output regardless of cargo exit code so the floor check always sees
# fresh data. mcp_handshake tests need a live MCP server and fail in offline CI;
# that's pre-existing and must not suppress the coverage summary.
PATH="$HOME/.cargo/bin:$PATH" \
  "$CARGO" llvm-cov \
    --features testing \
    --ignore-run-fail \
    -p iris-agentic-dev-core \
    --lib --test '*' \
    --summary-only \
    -- --test-threads=1 2>&1 | tee "$COVERAGE_OUT"

# Fail hard if cargo produced no coverage output at all (e.g. compile error).
if ! grep -q "Filename\|TOTAL\|iris_agentic" "$COVERAGE_OUT" 2>/dev/null; then
  echo "ERROR: coverage-raw.txt has no coverage data — cargo may have failed to compile"
  exit 1
fi

echo ""
echo "=== Checking floors ==="
python3 "$REPO_ROOT/scripts/check-coverage-floors.py" \
  --floors "$REPO_ROOT/coverage-floors.toml" \
  --coverage "$COVERAGE_OUT" \
  --src "$REPO_ROOT/crates/iris-agentic-dev-core/src"
