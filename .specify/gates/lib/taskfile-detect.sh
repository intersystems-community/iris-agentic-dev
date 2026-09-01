#!/usr/bin/env bash
# shellcheck shell=bash
# taskfile-detect.sh -- detect whether a project's Taskfile.yml exposes
# both `lint:` and `test:` top-level targets.
#
# INFRA-024 init flow uses this to decide whether to recommend
# verify-quality.orchestrator = "task" on first run. Returns 0 when both
# targets are present, 1 otherwise. Missing Taskfile.yml also returns 1.
#
# Usage (sourced):
#   source taskfile-detect.sh
#   if has_taskfile_lint_test "$CLAUDE_PROJECT_DIR"; then ...
#
# Usage (CLI):
#   taskfile-detect.sh has-lint-test <project_dir>
#
# Detection strategy:
#   1. If `yq` is on PATH, parse the Taskfile and check for both keys
#      under .tasks.
#   2. Otherwise grep for the canonical 2-space-indented target
#      pattern under a top-level `tasks:` block. The grep approach is
#      lossy (won't catch nested or aliased declarations), which is
#      acceptable: a Taskfile that does not expose plain `lint:` and
#      `test:` targets falls through to the "none" default and the
#      user can opt in manually via .specify/gates/policy.json.

set -euo pipefail

has_taskfile_lint_test() {
    local project_dir="${1:-}"
    if [[ -z "$project_dir" ]]; then
        return 1
    fi

    local taskfile=""
    for candidate in Taskfile.yml Taskfile.yaml; do
        if [[ -f "$project_dir/$candidate" ]]; then
            taskfile="$project_dir/$candidate"
            break
        fi
    done
    if [[ -z "$taskfile" ]]; then
        return 1
    fi

    if command -v yq >/dev/null 2>&1; then
        # yq returns "null" (literal) when the key is absent; we accept any
        # non-null, non-empty value as evidence the target is declared.
        local lint_val test_val
        lint_val="$(yq -r '.tasks.lint // "null"' "$taskfile" 2>/dev/null || echo "null")"
        test_val="$(yq -r '.tasks.test // "null"' "$taskfile" 2>/dev/null || echo "null")"
        if [[ "$lint_val" != "null" && "$test_val" != "null" ]]; then
            return 0
        fi
        return 1
    fi

    # Grep fallback. Match `^tasks:` then look ahead for the two targets at
    # exactly 2-space indent. Strict on indent so we do not pick up `lint:`
    # under a nested map.
    if ! grep -qE '^tasks:[[:space:]]*$' "$taskfile"; then
        return 1
    fi
    local has_lint=0 has_test=0
    if grep -qE '^  lint:[[:space:]]*($|#)' "$taskfile"; then
        has_lint=1
    fi
    if grep -qE '^  test:[[:space:]]*($|#)' "$taskfile"; then
        has_test=1
    fi
    if [[ "$has_lint" -eq 1 && "$has_test" -eq 1 ]]; then
        return 0
    fi
    return 1
}

if [[ "${BASH_SOURCE[0]:-}" == "${0}" ]]; then
    case "${1:-}" in
        has-lint-test)
            shift
            if has_taskfile_lint_test "${1:-}"; then
                exit 0
            else
                exit 1
            fi
            ;;
        *)
            cat >&2 <<'USAGE'
Usage: taskfile-detect.sh has-lint-test <project_dir>

Exits 0 when <project_dir>/Taskfile.{yml,yaml} declares both `lint:`
and `test:` top-level tasks. Exits 1 otherwise (including when the
Taskfile is absent).
USAGE
            exit 2
            ;;
    esac
fi
