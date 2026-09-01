#!/bin/bash
# shellcheck shell=bash
set -uo pipefail
# Intentionally NOT using set -e or trap ERR: the dispatch lib propagates
# non-zero exit codes from formatters that ran but failed, and we want to
# count them rather than abort on the first one. severity=error then maps
# the count to exit 2 explicitly at the bottom of this hook.

# Stop hook: batch-format all changed files. Runs before verify-quality.sh.
# Checks stop_hook_active for recursion guard.
#
# INFRA-019:
#   - Sources policy.sh so the formatter dispatch can consult per-tool
#     exclude lists; without the loader, dispatch falls through unchanged.
#   - Reads the hook's own severity field. severity=error -> tool failure
#     blocks the stop with exit 2; severity=warning (default for this hook)
#     -> failure logs WARNING and exits 0.
#   - ADR-006 fallback: missing or unloadable policy emits a one-line stderr
#     deprecation notice naming v0.2.0 and runs in legacy alpha.11 mode
#     (no exclude filter, errors swallowed). Every fallback branch carries
#     the `# REMOVE AT v0.2.0` marker so the v0.2.0 cut is mechanical.

if ! command -v jq >/dev/null 2>&1; then
    echo "gates: jq not found, skipping hook" \
        "(run /speckit.gates.doctor)" >&2
    exit 0
fi

INPUT=$(cat /dev/stdin 2>/dev/null || echo "{}")

STOP_ACTIVE=$(echo "$INPUT" | jq -r '.stop_hook_active // "false"' 2>/dev/null || echo "false")
if [[ "$STOP_ACTIVE" == "true" ]]; then
    exit 0
fi

# The Claude hooks are projected to .claude/hooks/gates/, but the runtime lib
# lives in a different subtree (.specify/gates/lib/), so resolve it by the
# canonical project-relative path -- NOT relative to this script.
PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
GATES_LIB_DIR="$PROJECT_ROOT/.specify/gates/lib"
POLICY_LIB="$GATES_LIB_DIR/policy.sh"

# ADR-006 fallback decisions.
# REMOVE AT v0.2.0
LEGACY_MODE=0
SEVERITY="warning"

if [[ ! -f "$POLICY_LIB" ]]; then
    # REMOVE AT v0.2.0
    echo "gates: policy loader not found," \
        "format-changed running in legacy mode (REMOVE AT v0.2.0)" >&2
    LEGACY_MODE=1
elif
    # shellcheck source=/dev/null disable=SC1091
    ! source "$POLICY_LIB" 2>/dev/null
then
    # REMOVE AT v0.2.0
    echo "gates: failed to source policy loader," \
        "format-changed running in legacy mode (REMOVE AT v0.2.0)" >&2
    LEGACY_MODE=1
else
    POLICY_FILE="$(gates_policy_file)"
    if [[ ! -f "$POLICY_FILE" ]]; then
        # REMOVE AT v0.2.0
        echo "gates: .specify/gates/policy.json missing," \
            "format-changed running in legacy mode (REMOVE AT v0.2.0)" >&2
        LEGACY_MODE=1
    else
        FOUND_SEV="$(gates_policy_get format-changed severity)"
        [[ -n "$FOUND_SEV" ]] && SEVERITY="$FOUND_SEV"
    fi
fi

if [[ ! -f "$GATES_LIB_DIR/formatter-dispatch.sh" ]]; then
    # Runtime not projected -> nothing to format; fail open.
    exit 0
fi
# shellcheck source=/dev/null disable=SC1091
source "$GATES_LIB_DIR/formatter-dispatch.sh"

# Discover changed files (exclude deleted files). Anchor git to PROJECT_ROOT so
# the hook works regardless of the caller's cwd.
changed_files=$(git -C "$PROJECT_ROOT" diff --name-only --diff-filter=d HEAD 2>/dev/null || true)
if [[ -z "$changed_files" ]]; then
    # Also check unstaged changes
    changed_files=$(git -C "$PROJECT_ROOT" diff --name-only --diff-filter=d 2>/dev/null || true)
fi

if [[ -z "$changed_files" ]]; then
    exit 0
fi

FAILED=0
while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    full_path="$PROJECT_ROOT/$file"
    [[ -f "$full_path" ]] || continue
    if [[ "$LEGACY_MODE" -eq 1 ]]; then
        # REMOVE AT v0.2.0
        format_file "$full_path" >/dev/null 2>&1 || true
    else
        format_file "$full_path" >/dev/null || FAILED=$((FAILED + 1))
    fi
done <<<"$changed_files"

if [[ "$LEGACY_MODE" -eq 1 ]] || [[ "$FAILED" -eq 0 ]]; then
    exit 0
fi

case "$SEVERITY" in
    error)
        echo "gates: format-changed: $FAILED tool failure(s) (severity=error)" >&2
        exit 2
        ;;
    warning)
        echo "gates: format-changed: WARNING $FAILED tool failure(s)" >&2
        exit 0
        ;;
    info | *)
        exit 0
        ;;
esac
