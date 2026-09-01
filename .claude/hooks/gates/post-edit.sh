#!/bin/bash
# shellcheck shell=bash
set -uo pipefail
# Intentionally NOT using set -e or trap ERR: the dispatch lib propagates
# the formatter rc so the severity contract can map it to a hook exit code.

# PostToolUse hook for Write/Edit. Auto-formats the edited file using the
# shared formatter dispatch.
#
# INFRA-019:
#   - Sources policy.sh so the dispatch consults per-tool exclude lists
#     (and silently skips when the path is on an exclude).
#   - Reads its own severity field. severity=error -> tool failure exits 2;
#     severity=warning (default) -> failure logs a WARNING line and exits 0.
#   - ADR-006 fallback: missing or unloadable policy emits a one-line stderr
#     deprecation notice naming v0.2.0 and runs in legacy alpha.11 mode
#     (no exclude filter, errors swallowed). All fallback branches carry
#     `# REMOVE AT v0.2.0`.

if ! command -v jq >/dev/null 2>&1; then
    echo "gates: jq not found, skipping hook" \
        "(run /speckit.gates.doctor)" >&2
    exit 0
fi

INPUT=$(cat /dev/stdin)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null || echo "")

if [[ -z "$FILE_PATH" ]] || [[ ! -f "$FILE_PATH" ]]; then
    exit 0
fi

# The Claude hooks are projected to .claude/hooks/gates/, but the runtime lib
# lives in a different subtree (.specify/gates/lib/), so resolve it by the
# canonical project-relative path -- NOT relative to this script.
PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
GATES_LIB_DIR="$PROJECT_ROOT/.specify/gates/lib"
POLICY_LIB="$GATES_LIB_DIR/policy.sh"

# REMOVE AT v0.2.0
LEGACY_MODE=0
SEVERITY="warning"

if [[ ! -f "$POLICY_LIB" ]]; then
    # REMOVE AT v0.2.0
    echo "gates: policy loader not found," \
        "post-edit running in legacy mode (REMOVE AT v0.2.0)" >&2
    LEGACY_MODE=1
elif
    # shellcheck source=/dev/null disable=SC1091
    ! source "$POLICY_LIB" 2>/dev/null
then
    # REMOVE AT v0.2.0
    echo "gates: failed to source policy loader," \
        "post-edit running in legacy mode (REMOVE AT v0.2.0)" >&2
    LEGACY_MODE=1
else
    POLICY_FILE="$(gates_policy_file)"
    if [[ ! -f "$POLICY_FILE" ]]; then
        # REMOVE AT v0.2.0
        echo "gates: .specify/gates/policy.json missing," \
            "post-edit running in legacy mode (REMOVE AT v0.2.0)" >&2
        LEGACY_MODE=1
    else
        FOUND_SEV="$(gates_policy_get post-edit severity)"
        [[ -n "$FOUND_SEV" ]] && SEVERITY="$FOUND_SEV"
    fi
fi

if [[ ! -f "$GATES_LIB_DIR/formatter-dispatch.sh" ]]; then
    # Runtime not projected -> nothing to format; fail open.
    exit 0
fi
# shellcheck source=/dev/null disable=SC1091
source "$GATES_LIB_DIR/formatter-dispatch.sh"

RC=0
if [[ "$LEGACY_MODE" -eq 1 ]]; then
    # REMOVE AT v0.2.0
    format_file "$FILE_PATH" >/dev/null 2>&1 || true
    exit 0
fi

format_file "$FILE_PATH" >/dev/null || RC=$?

if [[ "$RC" -eq 0 ]]; then
    exit 0
fi

case "$SEVERITY" in
    error)
        echo "gates: post-edit: tool failure on $FILE_PATH (severity=error)" >&2
        exit 2
        ;;
    warning)
        echo "gates: post-edit: WARNING tool failure on $FILE_PATH" >&2
        exit 0
        ;;
    info | *)
        exit 0
        ;;
esac
