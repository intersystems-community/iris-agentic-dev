#!/bin/bash
set -euo pipefail

# PreToolUse hook for Write/Edit.
# Reads JSON from stdin, parses file_path, blocks modification of sensitive files.
# Exit 0 = allow, Exit 1 = block.

trap 'exit 0' ERR

if ! command -v jq >/dev/null 2>&1; then
    echo "gates: jq not found, skipping hook" \
        "(run /speckit.gates.doctor)" >&2
    exit 0
fi

INPUT=$(cat /dev/stdin)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null || echo "")

if [[ -z "$FILE_PATH" ]]; then
    exit 0
fi

BASENAME=$(basename "$FILE_PATH")

BLOCKED=""

# Allowlist: .example / .sample / .template files are safe to edit even when
# the base name looks sensitive (e.g. .env.example). Mirrors the git
# pre-commit forbidden-file allowlist so the two boundaries agree.
if [[ "$BASENAME" == *.example ]] || [[ "$BASENAME" == *.sample ]] \
    || [[ "$BASENAME" == *.template ]]; then
    exit 0
fi

# Environment files
if [[ "$BASENAME" == ".env" ]] || [[ "$BASENAME" == .env.* ]]; then
    BLOCKED="Environment file"
fi

# SSH keys
case "$BASENAME" in
    id_rsa*|id_ed25519*|id_ecdsa*|authorized_keys|known_hosts)
        BLOCKED="SSH key/config file"
        ;;
esac

# Certificates
case "$BASENAME" in
    *.pem|*.key|*.crt|*.p12|*.pfx)
        BLOCKED="Certificate/key file"
        ;;
esac

# Credentials
if echo "$BASENAME" | grep -qiE '(credentials|secret|password|token|keystore)'; then
    BLOCKED="Credentials file"
fi

# Cloud configs
if echo "$BASENAME" | grep -qE '^(gcloud-.*\.json|service-account.*\.json|aws-credentials)$'; then
    BLOCKED="Cloud credentials file"
fi

# Lock files
case "$BASENAME" in
    package-lock.json|yarn.lock|pnpm-lock.yaml|Cargo.lock|poetry.lock)
        BLOCKED="Lock file (auto-generated)"
        ;;
esac

# Sensitive directories
if echo "$FILE_PATH" | grep -qE '/(\.ssh|\.gnupg|\.aws|\.gcloud)/'; then
    BLOCKED="File in sensitive directory"
fi

# Policy-declared extra protected paths (protected_files.extra). Matched against
# both the project-relative path and the basename so exact entries and globs
# (e.g. ".specify/memory/constitution.md", "docs/**") both work.
if [[ -z "$BLOCKED" ]]; then
    PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
    POLICY_LIB="$PROJECT_ROOT/.specify/gates/lib/policy.sh"
    # shellcheck source=/dev/null disable=SC1091
    [[ -f "$POLICY_LIB" ]] && source "$POLICY_LIB"
    if command -v gates_policy_section_list >/dev/null 2>&1; then
        REL="$FILE_PATH"
        [[ "$FILE_PATH" == "$PROJECT_ROOT/"* ]] && REL="${FILE_PATH#"$PROJECT_ROOT"/}"
        while IFS= read -r entry; do
            [[ -z "$entry" ]] && continue
            if gates_glob_match "$REL" "$entry" \
                || gates_glob_match "$FILE_PATH" "$entry" \
                || [[ "$BASENAME" == "$entry" ]]; then
                BLOCKED="Protected by policy (protected_files.extra: $entry)"
                break
            fi
        done < <(gates_policy_section_list protected_files extra)
    fi
fi

if [[ -n "$BLOCKED" ]]; then
    echo "BLOCKED: $BLOCKED" >&2
    echo "File: $FILE_PATH" >&2
    exit 2
fi

exit 0
