#!/usr/bin/env bash
# SessionStart hook for the Claude Code plugin. The plugin's skills and hooks update with the
# plugin, but the MCP server is whatever `iris-agentic-dev` is on PATH, so the two can drift
# apart. Say so in one line when the binary is older or missing. Never download anything, and
# never fail the session: every path exits 0.

PLUGIN_JSON="$CLAUDE_PLUGIN_ROOT/.claude-plugin/plugin.json"
[[ -f "$PLUGIN_JSON" ]] || exit 0
PLUGIN_VERSION=$(grep -o '"version":[[:space:]]*"[0-9][0-9.]*"' "$PLUGIN_JSON" | head -1 | grep -o '[0-9][0-9.]*')
[[ -n "$PLUGIN_VERSION" ]] || exit 0

RELEASES="https://github.com/intersystems-community/iris-agentic-dev/releases/latest"

if ! command -v iris-agentic-dev >/dev/null 2>&1; then
    echo "iris-agentic-dev is not on PATH, so the iris-dev plugin's MCP server cannot start. Ask me to run the iris-agentic-dev-setup skill, or install it: brew install iris-agentic-dev (Mac, after brew tap intersystems-community/tap), or download the binary for this machine from $RELEASES."
    exit 0
fi

BINARY_VERSION=$(iris-agentic-dev --version 2>/dev/null | sed -n 's/.*[^0-9.]\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' | head -1)
[[ -n "$BINARY_VERSION" ]] || exit 0

# Is $1 older than $2? Compared field by field as numbers, so 1.10.0 is newer than 1.9.5.
older() {
    local -a a b
    IFS=. read -ra a <<<"$1"
    IFS=. read -ra b <<<"$2"
    for i in 0 1 2; do
        local x=${a[i]:-0} y=${b[i]:-0}
        ((10#$x < 10#$y)) && return 0
        ((10#$x > 10#$y)) && return 1
    done
    return 1
}

if older "$BINARY_VERSION" "$PLUGIN_VERSION"; then
    echo "iris-agentic-dev on PATH is $BINARY_VERSION but the iris-dev plugin is $PLUGIN_VERSION, so the skills may describe tools this binary does not have. Upgrade the binary: brew upgrade iris-agentic-dev on a Mac with Homebrew, otherwise download the binary for this machine from $RELEASES and replace the one on PATH. Then restart Claude Code."
fi
exit 0
