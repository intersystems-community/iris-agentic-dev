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

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO="$HOME/.cargo/bin/cargo"

[[ -x "$CARGO" ]] || { echo "ERROR: cargo not found at $CARGO"; exit 1; }

echo "=== Running unit coverage (no IRIS required) ==="
PATH="$HOME/.cargo/bin:$PATH" \
  "$CARGO" llvm-cov \
    --features testing \
    --no-fail-fast \
    -p iris-agentic-dev-core \
    --lib --test '*' \
    --summary-only \
    -- --test-threads=1 2>&1 | tee "$REPO_ROOT/target/coverage-raw.txt"

echo ""
echo "=== Checking floors ==="
python3 "$REPO_ROOT/scripts/check-coverage-floors.py" \
  --floors "$REPO_ROOT/coverage-floors.toml" \
  --coverage "$REPO_ROOT/target/coverage-raw.txt" \
  --src "$REPO_ROOT/crates/iris-agentic-dev-core/src"
