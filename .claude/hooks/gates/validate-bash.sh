#!/bin/bash
set -euo pipefail

# PreToolUse hook for Bash commands.
# Reads JSON from stdin, parses the command field, blocks destructive patterns.
# Exit 0 = allow, Exit 1 = block.

trap 'exit 0' ERR

if ! command -v jq >/dev/null 2>&1; then
    echo "gates: jq not found, skipping hook" \
        "(run /speckit.gates.doctor)" >&2
    exit 0
fi

INPUT=$(cat /dev/stdin)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null || echo "")

if [[ -z "$COMMAND" ]]; then
    exit 0
fi

BLOCKED=""

# Destructive filesystem operations (matching literal $HOME in user commands).
# No trailing \b: a word boundary after "/" never matches at end-of-string on
# GNU grep (Linux/CI), so `rm -rf /` slipped through there while matching on
# BSD grep (macOS). The dangerous-target alternation is anchor enough.
# shellcheck disable=SC2016
if echo "$COMMAND" | grep -qE 'rm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+)?(-[a-zA-Z]*r[a-zA-Z]*\s+)?(\/|\/\*|~|\$HOME)'; then
    BLOCKED="Destructive rm command targeting root, home, or wildcard"
fi

# Force push
if echo "$COMMAND" | grep -qE 'git\s+push\s+(.*\s)?(-f|--force)(\s|$)'; then
    BLOCKED="git push --force"
fi

# Hard reset
if echo "$COMMAND" | grep -qE 'git\s+reset\s+--hard'; then
    BLOCKED="git reset --hard"
fi
if echo "$COMMAND" | grep -qE 'git\s+clean\s+-[a-zA-Z]*f'; then
    BLOCKED="git clean -f"
fi
if echo "$COMMAND" | grep -qE 'git\s+checkout\s+\.$'; then
    BLOCKED="git checkout . (discards all changes)"
fi
if echo "$COMMAND" | grep -qE 'git\s+restore\s+\.$'; then
    BLOCKED="git restore . (discards all changes)"
fi

# Dangerous permissions
if echo "$COMMAND" | grep -qE 'chmod\s+(-R\s+)?777'; then
    BLOCKED="chmod 777"
fi

# Disk destruction
if echo "$COMMAND" | grep -qE '>\s*/dev/sd'; then
    BLOCKED="Write to raw disk device"
fi
if echo "$COMMAND" | grep -qE 'mkfs\.'; then
    BLOCKED="Format filesystem"
fi
if echo "$COMMAND" | grep -qE 'dd\s+if=/dev/(zero|random)'; then
    BLOCKED="dd from zero/random device"
fi

# Fork bomb
if echo "$COMMAND" | grep -qF ':(){ :|:& };:'; then
    BLOCKED="Fork bomb"
fi

# Environment destruction
if echo "$COMMAND" | grep -qE '(unset\s+PATH|PATH=\s*$)'; then
    BLOCKED="PATH destruction"
fi

# Pipe to shell
if echo "$COMMAND" | grep -qE '(curl|wget)\s.*\|\s*(sh|bash)'; then
    BLOCKED="Pipe remote content to shell"
fi

if [[ -n "$BLOCKED" ]]; then
    echo "BLOCKED: $BLOCKED" >&2
    echo "Command: $COMMAND" >&2
    exit 2
fi

exit 0
