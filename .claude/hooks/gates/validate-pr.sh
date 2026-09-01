#!/bin/bash
set -euo pipefail

# PreToolUse hook for Bash commands that run `gh pr create`.
# Checks PR title/body for AI-isms, emoji, Co-Authored-By.
# Exit 0 = allow or not a PR command, Exit 2 = block (Claude Code convention).

trap 'exit 0' ERR

if ! command -v jq >/dev/null 2>&1; then
    echo "gates: jq not found, skipping hook" \
        "(run /speckit.gates.doctor)" >&2
    exit 0
fi

INPUT=$(cat /dev/stdin)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null || echo "")

# Only check gh pr create commands
if ! echo "$COMMAND" | grep -qE 'gh\s+pr\s+create'; then
    exit 0
fi

# Extract title and body from command
VALIDATOR_SCRIPT=$(mktemp)
cat > "$VALIDATOR_SCRIPT" << 'PYTHON_SCRIPT'
import re
import sys

command = sys.argv[1] if len(sys.argv) > 1 else ""

# Extract --title value
title_match = re.search(r'--title\s+["\']([^"\']*)["\']', command)
title = title_match.group(1) if title_match else ""

# Extract --body value (may be multiline via heredoc)
body_match = re.search(r'--body\s+["\']([^"\']*)["\']', command, re.DOTALL)
if not body_match:
    body_match = re.search(r'--body\s+"([^"]*)"', command, re.DOTALL)
body = body_match.group(1) if body_match else ""

text = f"{title}\n{body}"
violations = []

# AI branding (allow "Claude Code" and legitimate Claude-file references)
cleaned = re.sub(r'Claude Code', '', text)
cleaned = re.sub(r'\([^)]*\)', '', cleaned)  # Remove parenthetical scopes
cleaned = re.sub(r'[/\\]\S+', '', cleaned)   # Remove file paths
cleaned = re.sub(r'CLAUDE\.md', '', cleaned, flags=re.IGNORECASE)   # memory file
cleaned = re.sub(r'\.claude\S*', '', cleaned, flags=re.IGNORECASE)  # .claude/ paths
cleaned = re.sub(r'claude-[\w.-]+', '', cleaned, flags=re.IGNORECASE)  # kebab ids
for term in ["Anthropic", "GPT", "OpenAI", "Copilot"]:
    if re.search(rf'\b{term}\b', cleaned, re.IGNORECASE):
        violations.append(f"AI branding: {term}")
if re.search(r'\bClaude\b', cleaned, re.IGNORECASE):
    violations.append("Standalone 'Claude' (use 'Claude Code' instead)")

# Co-Authored-By
if re.search(r'Co-Authored-By:', text, re.IGNORECASE):
    violations.append("Co-Authored-By trailer")

# AI-isms
ai_isms = [
    (r'\bI have\b', "Self-reference: 'I have'"),
    (r"\bI've\b", "Self-reference: 'I've'"),
    (r'\bI updated\b', "Self-reference: 'I updated'"),
    (r'\bI fixed\b', "Self-reference: 'I fixed'"),
    (r'\bCertainly\b', "Filler: 'Certainly'"),
    (r"\bI'd be happy to\b", "Filler: 'I'd be happy to'"),
    (r'\bAs an AI\b', "Filler: 'As an AI'"),
]
for pattern, msg in ai_isms:
    if re.search(pattern, text, re.IGNORECASE):
        violations.append(msg)

# Marketing adjectives
marketing = ["seamless", "robust", "powerful", "elegant", "streamlined", "polished", "enhanced", "refined"]
for word in marketing:
    if re.search(rf'\b{word}\b', text, re.IGNORECASE):
        violations.append(f"Marketing adjective: '{word}'")

# Emoji detection
emoji_pattern = re.compile(
    "["
    "\U0001F300-\U0001F9FF"
    "\U00002600-\U000027BF"
    "\U0000FE00-\U0000FE0F"
    "\U0000200D"
    "\U00002702-\U000027B0"
    "\U0001FA00-\U0001FA6F"
    "\U0001FA70-\U0001FAFF"
    "]+",
    flags=re.UNICODE
)
if emoji_pattern.search(text):
    violations.append("Emoji detected")

if violations:
    for v in violations:
        print(v, file=sys.stderr)
    sys.exit(1)
else:
    sys.exit(0)
PYTHON_SCRIPT
if ! command -v python3 >/dev/null 2>&1; then
    echo "gates: python3 not found, skipping" \
        "PR validation" \
        "(run /speckit.gates.doctor)" >&2
    rm -f "$VALIDATOR_SCRIPT"
    exit 0
fi

VIOLATIONS=$(python3 "$VALIDATOR_SCRIPT" "$COMMAND" 2>&1) || {
    echo "PR validation failed:" >&2
    echo "$VIOLATIONS" >&2
    rm -f "$VALIDATOR_SCRIPT"
    exit 2
}
rm -f "$VALIDATOR_SCRIPT"

exit 0
