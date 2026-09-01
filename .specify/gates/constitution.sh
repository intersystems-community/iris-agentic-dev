#!/bin/bash
# shellcheck shell=bash
set -uo pipefail

# spec-gates constitution entry script (feature 004): turn a constitution
# into an enforceable contract. The deterministic half of the guided session
# (/speckit.gates.constitution owns the conversation; every step below is
# pure and testable).
#
#   constitution.sh fragments --corpus DIR --profile ANSWERS.json
#   constitution.sh draft --corpus DIR --selections SEL.json --out FILE [--augment EXISTING.md]
#   constitution.sh detect [--constitution FILE]
#   constitution.sh align  [--constitution FILE] [--policy FILE]   # (US2)
#   constitution.sh check  [--constitution FILE]                   # (US3)
#
# Exit codes: 0 success, 1 usage/environment error, 2 contract failure
# (malformed corpus, selections, or annotations).

GATES_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

# shellcheck source=lib/policy.sh disable=SC1091
[[ -f "$GATES_DIR/lib/policy.sh" ]] && source "$GATES_DIR/lib/policy.sh"
# shellcheck source=lib/spec-gate.sh disable=SC1091
[[ -f "$GATES_DIR/lib/spec-gate.sh" ]] && source "$GATES_DIR/lib/spec-gate.sh"
# shellcheck source=lib/attest.sh disable=SC1091
[[ -f "$GATES_DIR/lib/attest.sh" ]] && source "$GATES_DIR/lib/attest.sh"
# shellcheck source=lib/contract.sh disable=SC1091
[[ -f "$GATES_DIR/lib/contract.sh" ]] && source "$GATES_DIR/lib/contract.sh"
# shellcheck source=lib/constitution.sh disable=SC1091
source "$GATES_DIR/lib/constitution.sh"

if ! command -v jq >/dev/null 2>&1; then
    echo "constitution: jq not found (run /speckit.gates.doctor)" >&2
    exit 1
fi

DEFAULT_CONSTITUTION="$PROJECT_ROOT/.specify/memory/constitution.md"
DEFAULT_TEMPLATE="$PROJECT_ROOT/.specify/templates/constitution-template.md"

cmd_fragments() {
    local corpus="" profile=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --corpus)
                corpus="${2:-}"
                shift 2 || return 1
                ;;
            --profile)
                profile="${2:-}"
                shift 2 || return 1
                ;;
            *)
                echo "constitution: fragments: unknown argument: $1" >&2
                return 1
                ;;
        esac
    done
    [[ -n "$corpus" ]] || {
        echo "constitution: fragments needs --corpus DIR" >&2
        return 1
    }
    gates_const_fragments "$corpus" "$profile"
}

cmd_draft() {
    local corpus="" selections="" out="" augment=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --corpus)
                corpus="${2:-}"
                shift 2 || return 1
                ;;
            --selections)
                selections="${2:-}"
                shift 2 || return 1
                ;;
            --out)
                out="${2:-}"
                shift 2 || return 1
                ;;
            --augment)
                augment="${2:-}"
                shift 2 || return 1
                ;;
            *)
                echo "constitution: draft: unknown argument: $1" >&2
                return 1
                ;;
        esac
    done
    [[ -n "$corpus" ]] || {
        echo "constitution: draft needs --corpus DIR" >&2
        return 1
    }
    [[ -n "$selections" ]] || {
        echo "constitution: draft needs --selections FILE" >&2
        return 1
    }
    [[ -n "$out" ]] || {
        echo "constitution: draft needs --out FILE" >&2
        return 1
    }
    gates_const_draft "$corpus" "$selections" "$out" "$augment"
}

cmd_detect() {
    local constitution="$DEFAULT_CONSTITUTION"
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --constitution)
                constitution="${2:-}"
                shift 2 || return 1
                ;;
            *)
                echo "constitution: detect: unknown argument: $1" >&2
                return 1
                ;;
        esac
    done
    gates_const_detect "$constitution" "$DEFAULT_TEMPLATE"
}

cmd_align() {
    local constitution="$DEFAULT_CONSTITUTION" policy=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --constitution)
                constitution="${2:-}"
                shift 2 || return 1
                ;;
            --policy)
                policy="${2:-}"
                shift 2 || return 1
                ;;
            *)
                echo "constitution: align: unknown argument: $1" >&2
                return 1
                ;;
        esac
    done
    gates_const_align "$PROJECT_ROOT" "$constitution" "$policy"
}

cmd_check() {
    local constitution="$DEFAULT_CONSTITUTION" policy=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --constitution)
                constitution="${2:-}"
                shift 2 || return 1
                ;;
            --policy)
                policy="${2:-}"
                shift 2 || return 1
                ;;
            *)
                echo "constitution: check: unknown argument: $1" >&2
                return 1
                ;;
        esac
    done
    gates_const_check "$PROJECT_ROOT" "$constitution" "$policy" "$(basename "$constitution")"
}

case "${1:-}" in
    fragments)
        shift
        cmd_fragments "$@"
        ;;
    draft)
        shift
        cmd_draft "$@"
        ;;
    detect)
        shift
        cmd_detect "$@"
        ;;
    align)
        shift
        cmd_align "$@"
        ;;
    check)
        shift
        cmd_check "$@"
        ;;
    *)
        echo "usage: constitution.sh fragments --corpus DIR --profile ANSWERS.json" >&2
        echo "       constitution.sh draft --corpus DIR --selections SEL.json --out FILE [--augment EXISTING.md]" >&2
        echo "       constitution.sh detect [--constitution FILE]" >&2
        echo "       constitution.sh align [--constitution FILE] [--policy FILE]" >&2
        echo "       constitution.sh check [--constitution FILE] [--policy FILE]" >&2
        exit 1
        ;;
esac
