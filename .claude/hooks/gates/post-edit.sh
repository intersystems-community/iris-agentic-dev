#!/bin/bash
# shellcheck shell=bash
set -uo pipefail
# Intentionally NOT using set -e or trap ERR: the dispatch lib propagates
# the formatter rc so the severity contract can map it to a hook exit code.

# PostToolUse hook for Write/Edit. Auto-formats the edited file using the
# shared formatter dispatch.
#
# Policy:
#   - Sources policy.sh so the dispatch consults per-tool exclude lists
#     (and silently skips when the path is on an exclude).
#   - Reads its own severity field. severity=error -> tool failure exits 2;
#     severity=warning (default) -> failure logs a WARNING line and exits 0.
#     A formatter that is not installed is no tool failure: the file is
#     skipped with a note, as verify.sh skips the gate.
#   - Without a loadable, valid policy there are no exclude lists, and
#     formatting without them could rewrite vendored or generated
#     files, so the hook prints a one-line notice and formats nothing.
#     A project without the runtime projected is skipped silently.

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

if [[ ! -f "$GATES_LIB_DIR/formatter-dispatch.sh" ]]; then
    # Runtime not projected -> nothing to format; fail open.
    exit 0
fi

# shellcheck source=/dev/null disable=SC1091
if [[ ! -f "$POLICY_LIB" ]] || ! source "$POLICY_LIB" 2>/dev/null; then
    echo "gates: post-edit: cannot load the policy loader, not formatting" \
        "(run /speckit.gates.doctor)" >&2
    exit 0
fi
if [[ ! -f "$(gates_policy_file)" ]]; then
    echo "gates: post-edit: no .specify/gates/policy.json, not formatting" \
        "(run /speckit.gates.init)" >&2
    exit 0
fi
if ! gates_validate_policy "$(gates_policy_file)" >/dev/null 2>&1; then
    echo "gates: post-edit: the policy is invalid, not formatting" \
        "(run /speckit.gates.doctor)" >&2
    exit 0
fi
SEVERITY="$(gates_policy_get post-edit severity)"
SEVERITY="${SEVERITY:-warning}"

# shellcheck source=/dev/null disable=SC1091
source "$GATES_LIB_DIR/formatter-dispatch.sh"

RC=0
format_file "$FILE_PATH" >/dev/null || RC=$?

if [[ "$RC" -eq 0 ]]; then
    # A missing formatter is skipped, as verify.sh reports it, whatever
    # the severity (#195).
    if [[ -n "${GATES_FORMAT_SKIPPED:-}" ]]; then
        echo "gates: post-edit: $GATES_FORMAT_SKIPPED; not formatting $FILE_PATH" >&2
    fi
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
