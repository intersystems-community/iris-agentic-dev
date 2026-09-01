#!/bin/bash
# shellcheck shell=bash
set -uo pipefail

# spec-gates canary suite: prove the enforcement layer still blocks.
#
# Each canary plants a known violation in a disposable sandbox and asserts
# the corresponding gate or hook rejects it. A canary that is ACCEPTED means
# a gate silently stopped blocking (the historical no-op-dispatch bug) —
# that is a suite failure naming the gate. User project files are never
# read as probes nor written (FR-006): all probes live under mktemp -d and
# are removed on every exit path.
#
# v1 canary set:
#   format  -- prettier-dirty file    -> verify.sh format gate     (exit 2)
#   shell   -- SC2086-class script    -> verify.sh shellcheck gate (exit 2)
#   bash    -- `rm -rf /` tool call   -> validate-bash.sh hook     (exit 2)
#   protect -- `.env` edit tool call  -> protect-files.sh hook     (exit 2)
#   secret  -- staged AWS-key string  -> pre-commit secret scan    (blocked)
#   spec    -- Complete feature with a failing accept block
#                                     -> verify.sh spec gate       (exit 2)
#   contract -- tampered effective policy in a synced sandbox
#                                     -> verify.sh contract gate   (exit 2)
#
# Usage:
#   canary.sh [--json] [--only <id>[,<id>...]]
#
# Exit codes:
#   0 = every executed canary was blocked (skips allowed for tools that are
#       absent AND not policy-enabled)
#   1 = at least one canary was accepted (broken gate), or a required
#       tool/hook for a policy-enabled canary is missing
#   2 = sandbox setup failure (fail closed)

JSON=0
ONLY=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --json) JSON=1; shift ;;
        --only) ONLY="${2:?}"; shift 2 ;;
        *) echo "canary: unknown argument: $1" >&2; exit 1 ;;
    esac
done

# The suite runs from the projected layout: verify.sh and lib/ are siblings
# of this script (.specify/gates/). Copying FROM here into the sandbox is
# what lets the canaries catch a broken *projected* runtime, not just a
# broken source tree.
CANARY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

CANARY_SET="format shell bash protect secret spec contract"

if [[ -n "$ONLY" ]]; then
    IFS=',' read -r -a _only_ids <<<"$ONLY"
    for _id in "${_only_ids[@]}"; do
        case " $CANARY_SET " in
            *" $_id "*) ;;
            *) echo "canary: unknown canary id: $_id (known: $CANARY_SET)" >&2; exit 1 ;;
        esac
    done
fi

want() { # <id>: selected by --only (or everything when --only is absent)?
    [[ -z "$ONLY" ]] && return 0
    case ",$ONLY," in
        *",$1,"*) return 0 ;;
    esac
    return 1
}

if ! command -v jq >/dev/null 2>&1; then
    echo "canary: jq not found — cannot run canaries (run /speckit.gates.doctor)" >&2
    exit 2
fi

WORKDIR="$(mktemp -d 2>/dev/null || mktemp -d -t gates-canary)" || {
    echo "canary: sandbox setup failed (mktemp)" >&2
    exit 2
}
trap '[[ -n "${WORKDIR:-}" && -d "$WORKDIR" ]] && rm -rf "$WORKDIR"' EXIT

setup_fail() {
    echo "canary: sandbox setup failed: $*" >&2
    exit 2
}

# ---------------------------------------------------------------------------
# Result collection (indexed arrays; bash 3.2 has no associative arrays)
# ---------------------------------------------------------------------------
IDS=()
STATUSES=()
OUTCOMES=()
FAILED=0

record() { # <id> <blocked|accepted|skipped> <outcome> <counts-as-failure:0|1>
    IDS+=("$1")
    STATUSES+=("$2")
    OUTCOMES+=("$3")
    [[ "$4" == "1" ]] && FAILED=$((FAILED + 1))
    return 0
}

# ---------------------------------------------------------------------------
# Host lookups: tool resolution mirrors the gate's own order
# (node_modules/.bin -> PATH), and "policy-enabled" mirrors doctor's gap
# rule — a policy-enabled tool that is missing fails the suite.
# ---------------------------------------------------------------------------
host_tool_bin() { # <binname>
    if [[ -x "$PROJECT_ROOT/node_modules/.bin/$1" ]]; then
        printf '%s\n' "$PROJECT_ROOT/node_modules/.bin/$1"
    elif command -v "$1" >/dev/null 2>&1; then
        command -v "$1"
    fi
}

host_policy_enables() { # <hook>
    local file="$PROJECT_ROOT/.specify/gates/policy.json"
    [[ -f "$file" ]] || return 1
    local n
    n="$(jq -r --arg h "$1" '(.hooks[$h].include // []) | length' "$file" 2>/dev/null || echo 0)"
    [[ "$n" -gt 0 ]]
}

# Locate the projected Claude hooks (real install), falling back to the
# extension source tree (this repo's own dogfood / development checkout).
claude_hook() { # <script-name>
    local d
    for d in "$PROJECT_ROOT/.claude/hooks/gates" \
        "$PROJECT_ROOT/extension/runtime/hooks/claude"; do
        if [[ -f "$d/$1" ]]; then
            printf '%s\n' "$d/$1"
            return 0
        fi
    done
    return 1
}

pre_commit_hook() {
    local f
    for f in "$PROJECT_ROOT/.specify/gates/hooks/pre-commit" \
        "$PROJECT_ROOT/extension/runtime/hooks/git/pre-commit"; do
        if [[ -f "$f" ]]; then
            printf '%s\n' "$f"
            return 0
        fi
    done
    return 1
}

# Project the runtime from CANARY_DIR into a sandbox with a minimal policy.
# Symlinking the host node_modules (never copied, never written) lets the
# sandbox resolve the same pinned linters the real gate uses.
project_sandbox() { # <dir> <policy-json>
    local dir="$1" policy="$2"
    [[ -f "$CANARY_DIR/verify.sh" && -f "$CANARY_DIR/contract.sh" && -d "$CANARY_DIR/lib" ]] \
        || setup_fail "verify.sh/contract.sh/lib not found next to canary.sh in $CANARY_DIR (re-project the runtime)"
    mkdir -p "$dir/.specify/gates/lib" || setup_fail "mkdir $dir"
    cp "$CANARY_DIR/verify.sh" "$CANARY_DIR/contract.sh" "$dir/.specify/gates/" || setup_fail "copy verify.sh/contract.sh"
    cp "$CANARY_DIR/lib/"*.sh "$dir/.specify/gates/lib/" || setup_fail "copy lib"
    printf '%s' "$policy" >"$dir/.specify/gates/policy.json" || setup_fail "write policy"
    if [[ -d "$PROJECT_ROOT/node_modules" ]]; then
        ln -sfn "$PROJECT_ROOT/node_modules" "$dir/node_modules" || setup_fail "link node_modules"
    fi
}

sandbox_verify() { # <dir>: run the sandboxed gate, echo its exit code
    local rc=0
    CLAUDE_PROJECT_DIR="$1" bash "$1/.specify/gates/verify.sh" --boundary ci \
        >/dev/null 2>&1 || rc=$?
    echo "$rc"
}

# ---------------------------------------------------------------------------
# Gate canaries: known-bad files through the real verify.sh (exit 2 = blocked)
# ---------------------------------------------------------------------------
run_format_canary() {
    if [[ -z "$(host_tool_bin prettier)" ]]; then
        if host_policy_enables prettier; then
            record format skipped "prettier is policy-enabled but not installed — enforcement gap (format gate)" 1
        else
            record format skipped "prettier not installed and not policy-enabled" 0
        fi
        return 0
    fi
    local d="$WORKDIR/format"
    project_sandbox "$d" '{ "hooks": { "prettier": { "include": ["**/*.md"], "orchestrator": "none", "severity": "error" }, "verify-quality": { "orchestrator": "none", "severity": "error" } } }'
    printf '#Bad md\n\n\n- x\n' >"$d/probe.md" || setup_fail "format probe"
    local rc
    rc="$(sandbox_verify "$d")"
    if [[ "$rc" -eq 2 ]]; then
        record format blocked "format gate (prettier) rejected a prettier-dirty file" 0
    else
        record format accepted "verify.sh exit $rc on a prettier-dirty file — the format gate (prettier) did not block" 1
    fi
}

run_shell_canary() {
    if [[ -z "$(host_tool_bin shellcheck)" ]]; then
        if host_policy_enables shellcheck; then
            record shell skipped "shellcheck is policy-enabled but not installed — enforcement gap (shell gate)" 1
        else
            record shell skipped "shellcheck not installed and not policy-enabled" 0
        fi
        return 0
    fi
    local d="$WORKDIR/shell"
    project_sandbox "$d" '{ "hooks": { "shellcheck": { "include": ["**/*.sh"], "orchestrator": "none", "severity": "error" }, "verify-quality": { "orchestrator": "none", "severity": "error" } } }'
    # The probe must contain a literal unquoted $HOME (an SC2086-class
    # finding); the single quotes below are intentional.
    # shellcheck disable=SC2016
    printf '#!/bin/bash\nrm -rf $HOME/x\n' >"$d/probe.sh" || setup_fail "shell probe"
    local rc
    rc="$(sandbox_verify "$d")"
    if [[ "$rc" -eq 2 ]]; then
        record shell blocked "shell gate (shellcheck) rejected a script with a known finding" 0
    else
        record shell accepted "verify.sh exit $rc on a script with a known shellcheck finding — the shell gate (shellcheck) did not block" 1
    fi
}

# ---------------------------------------------------------------------------
# Hook canaries: crafted tool-call JSON through the real hook entrypoints
# (exit 2 = blocked). CLAUDE_PROJECT_DIR points into the sandbox so any
# policy lookup the hook makes stays off the user's project.
# ---------------------------------------------------------------------------
run_hook_canary() { # <id> <script-name> <payload> <gate-label>
    local id="$1" script_name="$2" payload="$3" label="$4"
    local script
    if ! script="$(claude_hook "$script_name")"; then
        record "$id" skipped "$script_name not found — agent boundary not projected ($label)" 1
        return 0
    fi
    local d="$WORKDIR/hookenv"
    mkdir -p "$d" || setup_fail "hook sandbox"
    local rc=0
    printf '%s' "$payload" | CLAUDE_PROJECT_DIR="$d" bash "$script" >/dev/null 2>&1 || rc=$?
    if [[ "$rc" -eq 2 ]]; then
        record "$id" blocked "$label blocked the probe" 0
    else
        record "$id" accepted "$script_name exit $rc on a known-bad tool call — the $label did not block" 1
    fi
}

run_bash_canary() {
    run_hook_canary bash validate-bash.sh \
        '{"tool_input":{"command":"rm -rf /"}}' "validate-bash hook"
}

run_protect_canary() {
    run_hook_canary protect protect-files.sh \
        '{"tool_input":{"file_path":".env"}}' "protect-files hook"
}

# ---------------------------------------------------------------------------
# Secret canary: a real `git commit` in a sandbox repo with the pre-commit
# hook installed must be refused, and refused BY THE SECRET SCAN (a commit
# failing for any other reason is still a broken canary — fail closed).
# ---------------------------------------------------------------------------
run_secret_canary() {
    if ! command -v git >/dev/null 2>&1; then
        record secret skipped "git not installed — enforcement gap (pre-commit secret scan)" 1
        return 0
    fi
    local hook
    if ! hook="$(pre_commit_hook)"; then
        record secret skipped "pre-commit hook not found — git boundary not projected (secret scan)" 1
        return 0
    fi
    local d="$WORKDIR/secret"
    mkdir -p "$d" || setup_fail "secret sandbox"
    git init -q "$d" >/dev/null 2>&1 || setup_fail "secret git init"
    # A non-main branch, so the block-main rule cannot be what refuses the
    # commit (checkout -b works on the unborn HEAD everywhere).
    git -C "$d" checkout -q -b canary-probe 2>/dev/null || setup_fail "secret branch"
    git -C "$d" config user.email canary@example.invalid
    git -C "$d" config user.name "gates-canary"
    cp "$hook" "$d/.git/hooks/pre-commit" || setup_fail "install pre-commit"
    chmod +x "$d/.git/hooks/pre-commit"
    # AKIA + 16 chars, assembled so this script never contains a key-shaped
    # literal itself.
    printf 'AKIA%s\n' "ABCDEFGHIJKLMNOP" >"$d/leak.txt"
    local out rc=0
    out="$(cd "$d" && git add leak.txt && git commit -q -m 'canary secret probe' 2>&1)" || rc=$?
    if [[ "$rc" -ne 0 ]] && printf '%s' "$out" | grep -q 'SECRET'; then
        record secret blocked "pre-commit secret scan refused the staged AWS-key-shaped string" 0
    elif [[ "$rc" -ne 0 ]]; then
        record secret accepted "commit was refused, but not by the secret scan — the pre-commit secret scan did not block" 1
    else
        record secret accepted "commit with an AWS-key-shaped string was ACCEPTED — the pre-commit secret scan did not block" 1
    fi
}

# ---------------------------------------------------------------------------
# Spec canary (feature 002, R8): a sandbox feature marked Complete with a
# `false` accept block must be rejected by the sandboxed spec gate. The run
# clears GATES_SPEC_EXEC so the canary still probes the spec gate when the
# suite is itself invoked from inside an accept block (the sentinel would
# otherwise make the sandboxed verify.sh skip exactly the gate under test).
# ---------------------------------------------------------------------------
run_spec_canary() {
    local d="$WORKDIR/spec"
    project_sandbox "$d" '{ "hooks": { "verify-quality": { "orchestrator": "none", "severity": "error" } } }'
    mkdir -p "$d/specs/900-canary-fixture" || setup_fail "spec fixture dir"
    printf '# Canary Fixture\n\n**Status**: Complete\n' \
        >"$d/specs/900-canary-fixture/spec.md" || setup_fail "spec fixture spec.md"
    {
        echo '- [x] T001 A criterion that must fail'
        echo ''
        echo '  ```accept'
        echo '  false'
        echo '  ```'
    } >"$d/specs/900-canary-fixture/tasks.md" || setup_fail "spec fixture tasks.md"
    local rc=0
    CLAUDE_PROJECT_DIR="$d" env -u GATES_SPEC_EXEC \
        bash "$d/.specify/gates/verify.sh" --boundary ci >/dev/null 2>&1 || rc=$?
    if [[ "$rc" -eq 2 ]]; then
        record spec blocked "spec gate rejected a Complete feature with a failing accept block" 0
    else
        record spec accepted "verify.sh exit $rc on a Complete feature with a failing accept block — the spec gate did not block" 1
    fi
}

# ---------------------------------------------------------------------------
# Contract canary (feature 003): a synced sandbox contract whose effective
# policy is then tampered must be rejected by the sandboxed contract gate.
# The fixture baseline lives inside the sandbox (plain-path git remote) --
# no network, no user files. Requires git only because sync does.
# ---------------------------------------------------------------------------
run_contract_canary() {
    if ! command -v git >/dev/null 2>&1; then
        record contract skipped "git not installed -- a policy contract cannot exist without it" 0
        return 0
    fi
    local b="$WORKDIR/contract-base"
    mkdir -p "$b" || setup_fail "contract baseline dir"
    git init -q "$b" >/dev/null 2>&1 || setup_fail "contract baseline git init"
    printf '%s' '{"hooks":{"verify-quality":{"orchestrator":"none","severity":"error"}}}' \
        | jq -S . >"$b/policy.json" || setup_fail "contract baseline policy"
    git -C "$b" add -A >/dev/null 2>&1
    git -C "$b" -c user.email=canary@example.invalid -c user.name=gates-canary \
        commit -qm "canary baseline" >/dev/null 2>&1 || setup_fail "contract baseline commit"
    git -C "$b" tag v1.0.0 >/dev/null 2>&1 || setup_fail "contract baseline tag"
    local d="$WORKDIR/contract"
    project_sandbox "$d" "$(jq -cn --arg src "$b" \
        '{hooks: {"verify-quality": {orchestrator: "none", severity: "error"}}, extends: {source: $src, version: "v1.0.0"}}')"
    CLAUDE_PROJECT_DIR="$d" bash "$d/.specify/gates/contract.sh" sync >/dev/null 2>&1 \
        || setup_fail "contract sandbox sync"
    printf ' ' >>"$d/.specify/gates/policy.effective.json" || setup_fail "contract tamper"
    local rc
    rc="$(sandbox_verify "$d")"
    if [[ "$rc" -eq 2 ]]; then
        record contract blocked "contract gate rejected a tampered effective policy" 0
    else
        record contract accepted "verify.sh exit $rc on a tampered effective policy -- the contract gate did not block" 1
    fi
}

# ---------------------------------------------------------------------------
# Run + report
# ---------------------------------------------------------------------------
for id in $CANARY_SET; do
    want "$id" || continue
    case "$id" in
        format) run_format_canary ;;
        shell) run_shell_canary ;;
        bash) run_bash_canary ;;
        protect) run_protect_canary ;;
        secret) run_secret_canary ;;
        spec) run_spec_canary ;;
        contract) run_contract_canary ;;
    esac
done

BLOCKED=0
ACCEPTED=0
SKIPPED=0
i=0
while [[ $i -lt ${#IDS[@]} ]]; do
    case "${STATUSES[$i]}" in
        blocked) BLOCKED=$((BLOCKED + 1)) ;;
        accepted) ACCEPTED=$((ACCEPTED + 1)) ;;
        skipped) SKIPPED=$((SKIPPED + 1)) ;;
    esac
    i=$((i + 1))
done

if [[ "$JSON" == "1" ]]; then
    joined=""
    i=0
    while [[ $i -lt ${#IDS[@]} ]]; do
        entry="$(jq -cn --arg id "${IDS[$i]}" --arg st "${STATUSES[$i]}" --arg out "${OUTCOMES[$i]}" \
            '{id: $id, expected: "blocked", outcome: $out, status: $st}')"
        joined="$joined$entry,"
        i=$((i + 1))
    done
    printf '{"canaries":[%s],"failed":%d}\n' "${joined%,}" "$FAILED"
else
    i=0
    while [[ $i -lt ${#IDS[@]} ]]; do
        case "${STATUSES[$i]}" in
            blocked) echo "canary: ${IDS[$i]} -- blocked: ${OUTCOMES[$i]}" ;;
            accepted) echo "canary: ${IDS[$i]} -- ACCEPTED (broken gate): ${OUTCOMES[$i]}" ;;
            skipped) echo "canary: ${IDS[$i]} -- skipped: ${OUTCOMES[$i]}" ;;
        esac
        i=$((i + 1))
    done
    echo "canary: ${#IDS[@]} run, $BLOCKED blocked, $ACCEPTED accepted, $SKIPPED skipped"
    if [[ "$FAILED" -gt 0 ]]; then
        echo "canary: FAILED — $FAILED enforcement gap(s) proven above" >&2
    fi
fi

[[ "$FAILED" -gt 0 ]] && exit 1
exit 0
