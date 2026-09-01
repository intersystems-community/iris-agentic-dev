#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# Stop hook -- agent boundary.
#
# The entire quality gate is delegated to the single verify.sh entrypoint, so
# the agent, git, and CI boundaries run identical checks against identical
# policy. This is what makes the parity property STRUCTURAL rather than
# tested-for: there is exactly one gate implementation. See
# docs/how-it-works.md ("The parity property").
#
# Stop-hook contract:
#   exit 0  -> allow the agent to stop
#   exit 2  -> block the stop; stderr is returned to the agent as the reason
#
# Failure philosophy: fail OPEN when gates are not configured (runtime not
# projected, jq absent) -- an uninitialised repo must never trap the agent --
# and fail CLOSED when a configured gate reports a failure.

INPUT="$(cat 2>/dev/null || true)"

# Loop guard: if we already blocked this stop once, let the next one through.
if command -v jq >/dev/null 2>&1; then
    if [[ "$(printf '%s' "$INPUT" | jq -r '.stop_hook_active // false' 2>/dev/null)" == "true" ]]; then
        exit 0
    fi
fi

PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
VERIFY="$PROJECT_ROOT/.specify/gates/verify.sh"

# Fail open: runtime not projected -> nothing to enforce.
[[ -x "$VERIFY" ]] || exit 0

set +e
OUTPUT="$("$VERIFY" --boundary agent 2>&1)"
RC=$?
set -e

case "$RC" in
    2)
        # Gate failure -> block the stop and feed the report back to the agent.
        {
            printf '%s\n\n' "$OUTPUT"
            echo "Quality gate failed at the agent boundary. Fix the issues above before stopping."
        } >&2
        exit 2
        ;;
    0)
        printf '%s\n' "$OUTPUT"
        exit 0
        ;;
    *)
        # Internal error (missing jq/policy, etc.) -> fail open, but say why.
        printf 'gates: verify.sh could not run (exit %s); allowing stop.\n%s\n' \
            "$RC" "$OUTPUT" >&2
        exit 0
        ;;
esac
