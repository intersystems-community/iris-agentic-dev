#!/usr/bin/env bash
# shellcheck shell=bash
# gates-policy-infer.sh -- synthesize .specify/gates/policy.json from host lint configs.
#
# Used by the alpha.12 migration guide (INFRA-029) as the `infer` answer to
# the policy-seed prompt. Reads the host's existing `.prettierignore`,
# `.markdownlint-cli2.yaml`, and (optional) `.specify/gates/shellcheck-excludes.txt`
# and emits a policy whose exclude arrays, when fed through
# gates-generate-configs.sh, regenerate those files byte-equal for the
# patterns the policy covers.
#
# The include arrays, orchestrator choices, severities, and the verify-
# quality / format-changed / post-edit stanzas are copied from the bundled
# scaffold default because they are not discoverable from the tool configs.
# Taskfile auto-detection (has_taskfile_lint_test) seeds
# verify-quality.orchestrator: task when the host's Taskfile exposes both
# lint and test targets; else none. Ambiguous input (malformed YAML,
# unreadable files) falls back to bundled defaults for that field and
# emits a per-field diff summary on stderr.
#
# Usage (sourced):
#   source gates-policy-infer.sh
#   gates_policy_infer <project_dir> <output_path>
#
# Usage (CLI):
#   gates-policy-infer.sh <project_dir> <output_path>
#
# Exit codes:
#   0  policy synthesized and validated
#   2  usage error (missing args, bad project dir)
#   3  bundled default template not found
#   4  resulting policy failed schema validation
#
# Dependencies: jq, awk, grep. No Python or Node runtime.

set -euo pipefail

GATES_INFER_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=policy.sh
# shellcheck disable=SC1091
source "$GATES_INFER_LIB_DIR/policy.sh"
# shellcheck source=taskfile-detect.sh
# shellcheck disable=SC1091
source "$GATES_INFER_LIB_DIR/taskfile-detect.sh"

# --- parsers ---------------------------------------------------------------

# Strip comment and blank lines; emit one entry per line.
_gates_infer_parse_prettierignore() {
    local file="$1"
    [[ -f "$file" ]] || return 0
    awk '
        /^[[:space:]]*#/  { next }
        /^[[:space:]]*$/  { next }
        { sub(/[[:space:]]+$/, ""); print }
    ' "$file"
}

# Extract entries from the single `ignores:` YAML block. Each entry is
# a single-quoted scalar: `  - 'value'`. Intentionally strict (matches
# the shape emitted by gates-generate-configs.sh) to avoid parsing the
# wider YAML surface. Uses awk with -v SQ='\''  so the awk body itself
# contains no literal single quotes (avoids shell quoting collisions).
_gates_infer_parse_markdownlint() {
    local file="$1"
    [[ -f "$file" ]] || return 0
    awk -v SQ="'" '
        BEGIN {
            in_ignores = 0
            entry_re = "^[[:space:]]*-[[:space:]]+" SQ
            tail_re  = SQ "[[:space:]]*$"
            doubled  = SQ SQ
        }
        /^ignores:[[:space:]]*$/ { in_ignores = 1; next }
        in_ignores && /^[^[:space:]-]/ { in_ignores = 0 }
        in_ignores && $0 ~ entry_re {
            sub(entry_re, "")
            sub(tail_re, "")
            gsub(doubled, SQ)
            print
        }
    ' "$file"
}

# Shellcheck excludes are one entry per line, no comments, no quoting.
_gates_infer_parse_shellcheck() {
    local file="$1"
    [[ -f "$file" ]] || return 0
    awk '
        /^[[:space:]]*$/ { next }
        { sub(/[[:space:]]+$/, ""); print }
    ' "$file"
}

# --- defaults loader --------------------------------------------------------

# Absolute path to the bundled scaffold policy. The migration guide
# resolves this via gates_resolve_asset, but the infer path needs raw
# JSON access, so we accept the path as an env var or fall back to the
# on-disk bundled copy.
_gates_infer_default_policy() {
    if [[ -n "${GATES_INFER_DEFAULT_POLICY:-}" && -f "${GATES_INFER_DEFAULT_POLICY}" ]]; then
        printf '%s' "$GATES_INFER_DEFAULT_POLICY"
        return 0
    fi
    # The seed template ships next to the runtime (extension/runtime/), so from
    # lib/ it is one level up. When the runtime is projected into a project it
    # is not copied, but policy-infer runs from the installed extension runtime
    # during init, where the template is present.
    local candidate="$GATES_INFER_LIB_DIR/../policy-template.json"
    if [[ -f "$candidate" ]]; then
        (cd "$(dirname "$candidate")" && printf '%s/policy-template.json' "$(pwd)")
        return 0
    fi
    echo "ERROR: bundled default policy not found" >&2
    return 3
}

# --- inference --------------------------------------------------------------

# Build a JSON array string from one-entry-per-line stdin.
_gates_infer_lines_to_json_array() {
    jq -R -s 'split("\n") | map(select(length > 0))'
}

gates_policy_infer() {
    local project_dir="${1:-}"
    local output_path="${2:-}"
    if [[ -z "$project_dir" || -z "$output_path" ]]; then
        echo "Usage: gates_policy_infer <project_dir> <output_path>" >&2
        return 2
    fi
    if [[ ! -d "$project_dir" ]]; then
        echo "ERROR: project directory not found: $project_dir" >&2
        return 2
    fi

    local default_policy
    default_policy="$(_gates_infer_default_policy)" || return 3

    # --- prettier.exclude
    local prettier_source="bundled defaults"
    local prettier_json
    if [[ -f "$project_dir/.prettierignore" ]]; then
        prettier_source="$project_dir/.prettierignore"
        prettier_json="$(_gates_infer_parse_prettierignore "$prettier_source" \
            | _gates_infer_lines_to_json_array)"
    else
        prettier_json="$(jq -c '.hooks.prettier.exclude' "$default_policy")"
    fi

    # --- markdownlint.exclude
    local markdown_source="bundled defaults"
    local markdown_json
    if [[ -f "$project_dir/.markdownlint-cli2.yaml" ]]; then
        markdown_source="$project_dir/.markdownlint-cli2.yaml"
        markdown_json="$(_gates_infer_parse_markdownlint "$markdown_source" \
            | _gates_infer_lines_to_json_array)"
    else
        markdown_json="$(jq -c '.hooks.markdownlint.exclude' "$default_policy")"
    fi

    # --- shellcheck.exclude
    local shellcheck_source="bundled defaults"
    local shellcheck_json
    if [[ -f "$project_dir/.specify/gates/shellcheck-excludes.txt" ]]; then
        shellcheck_source="$project_dir/.specify/gates/shellcheck-excludes.txt"
        shellcheck_json="$(_gates_infer_parse_shellcheck "$shellcheck_source" \
            | _gates_infer_lines_to_json_array)"
    else
        shellcheck_json="$(jq -c '.hooks.shellcheck.exclude' "$default_policy")"
    fi

    # --- verify-quality.orchestrator
    local orchestrator="none"
    if has_taskfile_lint_test "$project_dir"; then
        orchestrator="task"
    fi

    # --- assemble policy
    # Start from bundled default, override three exclude arrays and the
    # verify-quality orchestrator. Everything else (includes, severities,
    # on_missing_*, formatters) stays at bundled defaults.
    local stage_dir
    stage_dir="$(mktemp -d 2>/dev/null || mktemp -d -t 'gates-infer')"
    # shellcheck disable=SC2064
    trap "rm -rf '$stage_dir'" RETURN

    local stage_policy="$stage_dir/policy.json"
    jq \
        --argjson prettier "$prettier_json" \
        --argjson markdown "$markdown_json" \
        --argjson shellcheck "$shellcheck_json" \
        --arg orch "$orchestrator" \
        '.hooks.prettier.exclude = $prettier
        | .hooks.markdownlint.exclude = $markdown
        | .hooks.shellcheck.exclude = $shellcheck
        | .hooks["verify-quality"].orchestrator = $orch' \
        "$default_policy" >"$stage_policy"

    # Validate before writing to host.
    if ! gates_validate_policy "$stage_policy"; then
        return 4
    fi

    # Surface a per-field summary on stderr so the operator can see what
    # was inferred vs. what was left at defaults.
    {
        echo "gates-policy-infer: synthesized policy at $output_path"
        printf '  prettier.exclude       <- %s\n' "$prettier_source"
        printf '  markdownlint.exclude   <- %s\n' "$markdown_source"
        printf '  shellcheck.exclude     <- %s\n' "$shellcheck_source"
        printf '  verify-quality.orchestrator = %s (taskfile lint+test: %s)\n' \
            "$orchestrator" \
            "$(has_taskfile_lint_test "$project_dir" && echo present || echo absent)"
    } >&2

    mkdir -p "$(dirname "$output_path")"
    mv "$stage_policy" "$output_path"
    return 0
}

if [[ "${BASH_SOURCE[0]:-}" == "${0}" ]]; then
    gates_policy_infer "$@"
fi
