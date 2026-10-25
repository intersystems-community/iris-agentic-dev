#!/usr/bin/env bash
set -euo pipefail

INPUT=$(cat)

EVENT=$(printf '%s' "$INPUT" | jq -r '.hook_event_name // "PostToolUse"')

if [[ "$EVENT" == "FileChanged" ]]; then
    [[ "${IRIS_COMPILE_ON_SAVE:-}" != "true" ]] && exit 0
    FILE_PATH=$(printf '%s' "$INPUT" | jq -r '.file_path // empty')
else
    FILE_PATH=$(printf '%s' "$INPUT" | jq -r '.tool_input.file_path // empty')
fi

[[ -z "$FILE_PATH" ]] && exit 0

EXT="${FILE_PATH##*.}"
case "$EXT" in
    cls|mac|inc) ;;
    *) exit 0 ;;
esac

[[ "${IRIS_AUTO_COMPILE:-}" == "false" ]] && exit 0

if [[ -z "${IRIS_HOST:-}" || -z "${IRIS_WEB_PORT:-}" ]]; then
    echo "IRIS not connected — set IRIS_HOST, IRIS_WEB_PORT, IRIS_USERNAME, IRIS_PASSWORD"
    exit 0
fi

# Compile through the binary: it uploads the local edit, then compiles it, behind the same gates as
# the MCP server (write gate, template, allow, role, compile-time gate). Never post to IRIS here.
if ! command -v iris-agentic-dev >/dev/null 2>&1; then
    echo "iris-agentic-dev is not on PATH — compile-on-save skipped"
    exit 0
fi

# The binary prints its own result and exits 1 on a refusal or a compile error. A hook reports and
# carries on, so a failed compile never fails the edit.
iris-agentic-dev compile -- "$FILE_PATH" 2>&1 || true
