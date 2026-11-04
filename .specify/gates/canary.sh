#!/bin/bash
# shellcheck shell=bash
set -uo pipefail
# Every git call below works on a sandbox repository. Run from a git hook,
# git's per-invocation variables (GIT_DIR, GIT_INDEX_FILE, absolute in a
# linked worktree) would point those calls at the caller's repository:
# `add -A` staged the caller's tree and a canary commit or tag could land
# there (#173).
unset GIT_DIR GIT_INDEX_FILE GIT_WORK_TREE GIT_PREFIX GIT_COMMON_DIR \
    GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NAMESPACE GIT_QUARANTINE_PATH

# spec-gates canary suite: prove the enforcement layer still blocks.
#
# Each canary plants a known violation in a disposable sandbox and asserts
# the corresponding gate or hook rejects it. A canary that is ACCEPTED means
# a gate silently stopped blocking (the historical no-op-dispatch bug) —
# that is a suite failure naming the gate. User project files are never
# read as probes nor written (FR-006): all probes live under mktemp -d and
# are removed on every exit path.
#
# The canaries, in run order:
#   format  -- prettier-dirty file    -> verify.sh format gate     (exit 2)
#   markdown -- MD018 heading         -> verify.sh markdownlint gate (exit 2)
#   shell   -- SC2086-class script    -> verify.sh shellcheck gate (exit 2)
#   bash    -- `rm -rf /` tool call, with and without jq on PATH
#                                     -> validate-bash.sh hook     (exit 2)
#   protect -- `.env` edit tool call, with and without jq on PATH
#                                     -> protect-files.sh hook     (exit 2)
#   prhook  -- clean PR allowed AND AI-ism PR body AND unreadable
#              --body-file refused
#                                     -> validate-pr.sh hook
#   bulk    -- `git add -A` with git.block_bulk_staging on
#                                     -> validate-bash hook        (exit 2)
#   local   -- a project rule in hooks.local.d refuses its marker command,
#              and a plain command still passes
#                                     -> validate-bash local rules (exit 2)
#   secret  -- staged AWS-key string  -> pre-commit secret scan    (blocked)
#   credential -- staged `token: '...'` assignment
#                                     -> pre-commit generic scan   (blocked)
#   protected -- staged protected file, no Protected-Change trailer
#                                     -> commit-msg trailer check  (blocked)
#   branding -- commit message naming a default AI-branding term
#                                     -> commit-msg branding rule  (blocked)
#   pr      -- PR range with an undeclared protected change, and a PR
#              description with an AI-ism
#                                     -> pr-check.sh (CI boundary) (exit 1)
#   spec    -- Complete feature with a failing accept block
#                                     -> verify.sh spec gate       (exit 2)
#   errexit -- Complete feature whose accept block fails on a line before
#              its last, passing one
#                                     -> verify.sh spec gate       (exit 2)
#   contract -- tampered effective policy in a synced sandbox
#                                     -> verify.sh contract gate   (exit 2)
#   project -- failing project gate declared to skip the agent boundary
#              blocks at ci, and is reported skipped at agent
#                                     -> verify.sh project gates   (exit 2)
#   covered -- failing accept block a covering script vouches for at
#              agent and git fails at ci, and is reported covered at git
#                                     -> verify.sh spec gate       (exit 2)
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

CANARY_SET="format markdown shell bash protect prhook bulk local secret credential protected branding pr spec errexit contract project covered"

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
# verify.sh refuses without git (#121) and the sandboxes are git work trees,
# so every probe would fail for that reason alone: name git instead (#172).
if ! command -v git >/dev/null 2>&1; then
    echo "canary: git not found — cannot run canaries (verify.sh refuses without it; run /speckit.gates.doctor)" >&2
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
# (node_modules/.bin -> PATH; for shellcheck a PATH install other than the
# npm wrapper first, #274), and "policy-enabled" mirrors doctor's gap
# rule — a policy-enabled tool that is missing fails the suite.
# ---------------------------------------------------------------------------
host_tool_bin() { # <binname>
    local p
    if [[ "$1" == shellcheck ]]; then
        while IFS= read -r p; do
            [[ -z "$p" || "$p" == */node_modules/.bin/shellcheck ]] && continue
            printf '%s\n' "$p"
            return 0
        done <<<"$(type -ap shellcheck 2>/dev/null)"
    fi
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

pre_commit_hook() { git_hook pre-commit; }
commit_msg_hook() { git_hook commit-msg; }

git_hook() { # <name>
    local f
    for f in "$PROJECT_ROOT/.specify/gates/hooks/$1" \
        "$PROJECT_ROOT/extension/runtime/hooks/git/$1"; do
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

run_markdown_canary() {
    if [[ -z "$(host_tool_bin markdownlint-cli2)" ]]; then
        if host_policy_enables markdownlint; then
            record markdown skipped "markdownlint-cli2 is policy-enabled but not installed — enforcement gap (markdownlint gate)" 1
        else
            record markdown skipped "markdownlint-cli2 not installed and not policy-enabled" 0
        fi
        return 0
    fi
    local d="$WORKDIR/markdown"
    project_sandbox "$d" '{ "hooks": { "markdownlint": { "include": ["**/*.md"], "orchestrator": "none", "severity": "error" }, "verify-quality": { "orchestrator": "none", "severity": "error" } } }'
    # MD018 (no space after the heading hash), on by default.
    printf '#Bad heading\n' >"$d/probe.md" || setup_fail "markdown probe"
    local rc
    rc="$(sandbox_verify "$d")"
    if [[ "$rc" -eq 2 ]]; then
        record markdown blocked "markdownlint gate rejected a file with a known finding" 0
    else
        record markdown accepted "verify.sh exit $rc on a file with a known markdownlint finding — the markdownlint gate did not block" 1
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
    # Run it by path, as Claude Code does: the shebang (#!/bin/bash, which
    # is bash 3.2 on macOS) picks the interpreter, and a hook without its
    # execute bit is a gap, not something `bash <hook>` should paper over.
    if [[ ! -x "$script" ]]; then
        record "$id" accepted "$script_name is not executable — Claude Code runs it by path, so the $label never fires" 1
        return 0
    fi
    local rc=0
    printf '%s' "$payload" | CLAUDE_PROJECT_DIR="$d" "$script" >/dev/null 2>&1 || rc=$?
    if [[ "$rc" -ne 2 ]]; then
        record "$id" accepted "$script_name exit $rc on a known-bad tool call — the $label did not block" 1
        return 0
    fi
    # The same probe without jq on PATH (issue #83): the hook must still
    # block in raw mode instead of allowing everything.
    local nojq="$WORKDIR/path-nojq" t p
    if [[ ! -d "$nojq" ]]; then
        mkdir -p "$nojq" || setup_fail "no-jq PATH"
        for t in cat grep sed tr head tail basename dirname git awk; do
            p="$(type -P "$t" 2>/dev/null)" && ln -s "$p" "$nojq/$t"
        done
    fi
    rc=0
    printf '%s' "$payload" | PATH="$nojq" CLAUDE_PROJECT_DIR="$d" "$script" >/dev/null 2>&1 || rc=$?
    if [[ "$rc" -eq 2 ]]; then
        record "$id" blocked "$label blocked the probe, with and without jq" 0
    else
        record "$id" accepted "$script_name exit $rc on a known-bad tool call without jq — the $label did not block in raw mode" 1
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

# PR-hook canary: validate-pr.sh must ALLOW a clean `gh pr create` and BLOCK
# an AI-ism body with its own message. Both halves matter: a hook that fails
# to parse also exits 2 (0.3.4 shipped one under macOS bash 3.2), which a
# block-only probe would count as "blocked".
run_prhook_canary() {
    local script
    if ! script="$(claude_hook validate-pr.sh)"; then
        record prhook skipped "validate-pr.sh not found — agent boundary not projected (PR hook)" 1
        return 0
    fi
    if [[ ! -x "$script" ]]; then
        record prhook accepted "validate-pr.sh is not executable — Claude Code runs it by path, so the PR hook never fires" 1
        return 0
    fi
    # Without python3 the hook refuses every PR command (it fails closed,
    # #66). Name the tool: "the hook is broken" alone points at the hook
    # (#122).
    if ! python3 -c 'import json, re' >/dev/null 2>&1; then
        record prhook accepted "python3 with the json module is not installed — validate-pr.sh refuses every PR command until it is (install python3)" 1
        return 0
    fi
    local d="$WORKDIR/prhookenv" rc_ok=0 rc_bad=0 out_bad
    mkdir -p "$d/.specify/gates/lib" || setup_fail "prhook sandbox"
    cp "$CANARY_DIR/lib/"*.sh "$d/.specify/gates/lib/" || setup_fail "prhook lib"
    printf '%s' '{"tool_input":{"command":"gh pr create --title \"feat: canary\" --body \"Adds a parser.\""}}' \
        | CLAUDE_PROJECT_DIR="$d" "$script" >/dev/null 2>&1 || rc_ok=$?
    out_bad="$(printf '%s' '{"tool_input":{"command":"gh pr create --title \"feat: canary\" --body \"I have made this seamless.\""}}' \
        | CLAUDE_PROJECT_DIR="$d" "$script" 2>&1 >/dev/null)" || rc_bad=$?
    # An unreadable --body-file must be refused, not skipped (issue #65).
    local rc_nf=0 out_nf
    out_nf="$(printf '%s' "{\"tool_input\":{\"command\":\"gh pr create --title \\\"feat: canary\\\" --body-file $d/missing-body.md\"}}" \
        | CLAUDE_PROJECT_DIR="$d" "$script" 2>&1 >/dev/null)" || rc_nf=$?
    if [[ "$rc_ok" -ne 0 ]]; then
        record prhook accepted "validate-pr.sh exit $rc_ok on a CLEAN PR — the hook is broken (it blocks every PR command)" 1
    elif [[ "$rc_bad" -ne 2 ]] || ! grep -q 'PR validation failed' <<<"$out_bad"; then
        record prhook accepted "validate-pr.sh exit $rc_bad on an AI-ism PR body — the PR hook did not block" 1
    elif [[ "$rc_nf" -ne 2 ]] || ! grep -q 'cannot read --body-file' <<<"$out_nf"; then
        record prhook accepted "validate-pr.sh exit $rc_nf on an unreadable --body-file — the body went unchecked" 1
    else
        record prhook blocked "validate-pr.sh allowed a clean PR and refused an AI-ism body and an unreadable body file" 0
    fi
}

# Bulk-staging canary (#71): with the policy knob on in a sandbox, the
# command hook must refuse `git add -A`, and refuse it for that reason.
run_bulk_canary() {
    local script
    if ! script="$(claude_hook validate-bash.sh)"; then
        record bulk skipped "validate-bash.sh not found — agent boundary not projected (bulk staging)" 1
        return 0
    fi
    local d="$WORKDIR/bulkenv" rc=0 out
    mkdir -p "$d/.specify/gates" || setup_fail "bulk sandbox"
    printf '%s' '{ "hooks": {}, "git": { "block_bulk_staging": true } }' >"$d/.specify/gates/policy.json" \
        || setup_fail "bulk policy"
    out="$(printf '{"cwd":"%s","tool_input":{"command":"git add -A"}}' "$d" \
        | CLAUDE_PROJECT_DIR="$d" "$script" 2>&1 >/dev/null)" || rc=$?
    if [[ "$rc" -eq 2 ]] && grep -q 'block_bulk_staging' <<<"$out"; then
        record bulk blocked "validate-bash refused git add -A under git.block_bulk_staging" 0
    else
        record bulk accepted "validate-bash.sh exit $rc on git add -A with git.block_bulk_staging on — bulk staging was not refused" 1
    fi
}

# Local-rule canary (#71): a project rule in hooks.local.d must refuse the
# command it targets, and must not refuse anything else.
run_local_canary() {
    local script
    if ! script="$(claude_hook validate-bash.sh)"; then
        record local skipped "validate-bash.sh not found — agent boundary not projected (local rules)" 1
        return 0
    fi
    local d="$WORKDIR/localenv" rc_hit=0 rc_ok=0 out
    mkdir -p "$d/.specify/gates/lib" "$d/.specify/gates/hooks.local.d/validate-bash" || setup_fail "local sandbox"
    cp "$CANARY_DIR/lib/local-hooks.sh" "$d/.specify/gates/lib/" 2>/dev/null \
        || { record local accepted "lib/local-hooks.sh is missing — project rules in hooks.local.d would never run" 1; return 0; }
    printf '%s\n' 'if grep -q gates-local-canary; then echo "canary rule refused" >&2; exit 1; fi' \
        >"$d/.specify/gates/hooks.local.d/validate-bash/10-canary.sh" || setup_fail "local rule"
    out="$(printf '%s' '{"tool_input":{"command":"echo gates-local-canary"}}' \
        | CLAUDE_PROJECT_DIR="$d" "$script" 2>&1 >/dev/null)" || rc_hit=$?
    printf '%s' '{"tool_input":{"command":"ls"}}' | CLAUDE_PROJECT_DIR="$d" "$script" >/dev/null 2>&1 || rc_ok=$?
    if [[ "$rc_hit" -ne 2 ]] || ! grep -q 'gates(local validate-bash/10-canary.sh)' <<<"$out"; then
        record local accepted "validate-bash.sh exit $rc_hit on a command a local rule refuses — hooks.local.d rules do not run" 1
    elif [[ "$rc_ok" -ne 0 ]]; then
        record local accepted "validate-bash.sh exit $rc_ok on a plain command with a local rule present — the local rule plumbing blocks everything" 1
    else
        record local blocked "a hooks.local.d rule refused its command, and a plain command passed" 0
    fi
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
    # The scan's rules are a library (issue #212); without it the hook
    # skips the scan on a repository that never adopted gates.
    if [[ -f "$CANARY_DIR/lib/secrets.sh" ]]; then
        { mkdir -p "$d/.specify/gates/lib" && cp "$CANARY_DIR/lib/secrets.sh" "$d/.specify/gates/lib/"; } \
            || setup_fail "secret lib"
    fi
    # AKIA + 16 chars, assembled so this script never contains a key-shaped
    # literal itself.
    printf 'AKIA%s\n' "ABCDEFGHIJKLMNOP" >"$d/leak.txt"
    local out rc=0
    out="$(cd "$d" && git add leak.txt && git commit -q -m 'canary secret probe' 2>&1)" || rc=$?
    if [[ "$rc" -ne 0 ]] && grep -q 'SECRET' <<<"$out"; then
        record secret blocked "pre-commit secret scan refused the staged AWS-key-shaped string" 0
    elif [[ "$rc" -ne 0 ]]; then
        record secret accepted "commit was refused, but not by the secret scan — the pre-commit secret scan did not block" 1
    else
        record secret accepted "commit with an AWS-key-shaped string was ACCEPTED — the pre-commit secret scan did not block" 1
    fi
}

# ---------------------------------------------------------------------------
# Git-hook sandbox: a repo on a non-main branch with <hooks...> installed and
# the projected policy lib, so policy-reading hooks behave as in a project.
# Echoes the sandbox path. Probes run with CLAUDE_PROJECT_DIR pointed at the
# sandbox: the policy loader prefers it over the git toplevel, so an inherited
# value would judge the probe by the host project's policy.
# ---------------------------------------------------------------------------
git_sandbox() { # <id> <policy-json> <hook-path>...
    local id="$1" policy="$2"
    shift 2
    local d="$WORKDIR/$id"
    mkdir -p "$d/.specify/gates/lib" || setup_fail "$id sandbox"
    git init -q "$d" >/dev/null 2>&1 || setup_fail "$id git init"
    git -C "$d" checkout -q -b canary-probe 2>/dev/null || setup_fail "$id branch"
    git -C "$d" config user.email canary@example.invalid
    git -C "$d" config user.name "gates-canary"
    cp "$CANARY_DIR/lib/"*.sh "$d/.specify/gates/lib/" || setup_fail "$id lib"
    printf '%s\n' "$policy" >"$d/.specify/gates/policy.json"
    # Install the way init does: the hook is projected into
    # .specify/gates/hooks/ and .git/hooks holds the stub that runs it
    # (issue #59), so the canaries also prove the stub dispatches. Without
    # a stub (older projection) the hook is copied in directly.
    local h name stub=""
    stub="$(git_hook stub.sh 2>/dev/null)" || stub=""
    mkdir -p "$d/.specify/gates/hooks" || setup_fail "$id hooks dir"
    for h in "$@"; do
        name="$(basename "$h")"
        cp "$h" "$d/.specify/gates/hooks/$name" || setup_fail "$id project hook"
        if [[ -n "$stub" ]]; then
            cp "$stub" "$d/.git/hooks/$name" || setup_fail "$id install stub"
        else
            cp "$h" "$d/.git/hooks/$name" || setup_fail "$id install hook"
        fi
        chmod +x "$d/.git/hooks/$name" "$d/.specify/gates/hooks/$name"
    done
    printf '%s\n' "$d"
}

# Credential canary (issue #50): the generic assignment scan still blocks a
# single-quoted token after the POSIX-class fix (the pre-fix regex never
# matched single quotes at all).
run_credential_canary() {
    local hook
    if ! command -v git >/dev/null 2>&1 || ! hook="$(pre_commit_hook)"; then
        record credential skipped "git or pre-commit hook missing — enforcement gap (generic credential scan)" 1
        return 0
    fi
    local d out rc=0
    d="$(git_sandbox credential '{ "hooks": {} }' "$hook")"
    printf "token: '%s'\n" "abcdefgh12" >"$d/conf.yml"
    out="$(cd "$d" && git add conf.yml && CLAUDE_PROJECT_DIR="$d" git commit -q -m 'chore: canary credential probe' 2>&1)" || rc=$?
    if [[ "$rc" -ne 0 ]] && grep -q 'credential assignment' <<<"$out"; then
        record credential blocked "pre-commit refused a staged token assignment" 0
    else
        record credential accepted "a staged token assignment was not refused by the generic credential scan (exit $rc)" 1
    fi
}

# Protected canary (issue #47): a staged protected_files.extra path with no
# Protected-Change trailer must be refused by commit-msg.
run_protected_canary() {
    local pre msg
    if ! command -v git >/dev/null 2>&1 || ! pre="$(pre_commit_hook)" || ! msg="$(commit_msg_hook)"; then
        record protected skipped "git or git hooks missing — enforcement gap (protected-change trailer)" 1
        return 0
    fi
    local d out rc=0
    d="$(git_sandbox protected '{ "hooks": {}, "protected_files": { "extra": ["charter.md"] } }' "$pre" "$msg")"
    printf '# charter\n' >"$d/charter.md"
    out="$(cd "$d" && git add charter.md && CLAUDE_PROJECT_DIR="$d" git commit -q -m 'chore: canary protected probe' 2>&1)" || rc=$?
    if [[ "$rc" -ne 0 ]] && grep -q 'protected file changed without a declaration' <<<"$out"; then
        record protected blocked "commit-msg refused a protected file staged without a Protected-Change trailer" 0
    else
        record protected accepted "a protected file was committed without a Protected-Change trailer (exit $rc)" 1
    fi
}

# Branding canary (issue #52): the default AI-branding list still refuses.
run_branding_canary() {
    local msg
    if ! command -v git >/dev/null 2>&1 || ! msg="$(commit_msg_hook)"; then
        record branding skipped "git or commit-msg hook missing — enforcement gap (AI-branding rule)" 1
        return 0
    fi
    local d out rc=0
    d="$(git_sandbox branding '{ "hooks": {} }' "$msg")"
    printf 'x\n' >"$d/x.txt"
    out="$(cd "$d" && git add x.txt && CLAUDE_PROJECT_DIR="$d" git commit -q -m 'feat: written with Copilot' 2>&1)" || rc=$?
    if [[ "$rc" -ne 0 ]] && grep -q 'AI branding detected' <<<"$out"; then
        record branding blocked "commit-msg refused a message naming a default AI-branding term" 0
    else
        record branding accepted "a message naming a default AI-branding term was accepted (exit $rc)" 1
    fi
}

# PR canary (issues #53, #56): pr-check.sh must refuse a commit range with an
# undeclared protected change AND a PR description with an AI-ism. Inherited
# CI variables are cleared so a real pipeline's PR context cannot leak in.
# The seed commits the policy: the range check reads committed policy only.
pr_check_script() {
    local f
    for f in "$CANARY_DIR/pr-check.sh" "$PROJECT_ROOT/extension/runtime/pr-check.sh"; do
        [[ -f "$f" ]] && { printf '%s\n' "$f"; return 0; }
    done
    return 1
}

run_pr_canary() {
    local script
    if ! command -v git >/dev/null 2>&1 || ! script="$(pr_check_script)"; then
        record pr skipped "git or pr-check.sh missing — enforcement gap (CI PR check)" 1
        return 0
    fi
    local d base rc1=0 rc2=0 out1 out2
    d="$(git_sandbox pr '{ "hooks": {}, "protected_files": { "extra": ["charter.md"] } }')"
    cp "$script" "$d/.specify/gates/pr-check.sh" || setup_fail "pr install"
    (cd "$d" && printf 'x\n' >seed.txt && git add -A && git commit -q -m 'chore: seed') >/dev/null 2>&1 \
        || setup_fail "pr seed"
    base="$(git -C "$d" rev-parse HEAD)"
    (cd "$d" && printf '# charter\n' >charter.md && git add charter.md && git commit -q -m 'docs: charter') >/dev/null 2>&1 \
        || setup_fail "pr commit"
    local -a clean=(env -u GITHUB_EVENT_NAME -u GITHUB_BASE_REF -u CI_MERGE_REQUEST_DIFF_BASE_SHA
        -u CI_MERGE_REQUEST_TITLE -u CI_MERGE_REQUEST_DESCRIPTION -u CHANGE_TARGET -u CHANGE_TITLE
        -u GATES_PR_TITLE -u GATES_PR_BODY -u GATES_COMMIT_RANGE -u GATES_RUNTIME_DIR CLAUDE_PROJECT_DIR="$d")
    out1="$(cd "$d" && "${clean[@]}" bash .specify/gates/pr-check.sh --range "$base..HEAD" 2>&1)" || rc1=$?
    out2="$(cd "$d" && "${clean[@]}" GATES_PR_TITLE='feat: canary' GATES_PR_BODY='I have made this seamless.' \
        bash .specify/gates/pr-check.sh 2>&1)" || rc2=$?
    if [[ "$rc1" -ne 1 ]] || ! grep -q 'changed without a declaration' <<<"$out1"; then
        record pr accepted "pr-check.sh exit $rc1 on an undeclared protected change in the PR range — the CI protected check did not block" 1
    elif [[ "$rc2" -ne 1 ]] || ! grep -q 'Self-referential' <<<"$out2"; then
        record pr accepted "pr-check.sh exit $rc2 on a PR description with an AI-ism — the CI text check did not block" 1
    else
        record pr blocked "pr-check.sh refused an undeclared protected change and an AI-ism PR description" 0
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
    # A git work tree, so the block is blocked for failing and not because
    # it cannot be checked for mutations (#136).
    git init -q "$d" >/dev/null 2>&1 || setup_fail "spec fixture git init"
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
# Errexit canary (#236): the accept block fails on its first line and ends
# on a passing one. A runner that judges a block by its last command alone
# accepts it, so the sandboxed spec gate must reject it.
# ---------------------------------------------------------------------------
run_errexit_canary() {
    local d="$WORKDIR/errexit"
    project_sandbox "$d" '{ "hooks": { "verify-quality": { "orchestrator": "none", "severity": "error" } } }'
    mkdir -p "$d/specs/900-canary-fixture" || setup_fail "errexit fixture dir"
    git init -q "$d" >/dev/null 2>&1 || setup_fail "errexit fixture git init"
    printf '# Canary Fixture\n\n**Status**: Complete\n' \
        >"$d/specs/900-canary-fixture/spec.md" || setup_fail "errexit fixture spec.md"
    {
        echo '- [x] T001 A criterion whose first line fails'
        echo ''
        echo '  ```accept'
        echo '  false'
        echo '  true'
        echo '  ```'
    } >"$d/specs/900-canary-fixture/tasks.md" || setup_fail "errexit fixture tasks.md"
    local rc=0
    CLAUDE_PROJECT_DIR="$d" env -u GATES_SPEC_EXEC \
        bash "$d/.specify/gates/verify.sh" --boundary ci >/dev/null 2>&1 || rc=$?
    if [[ "$rc" -eq 2 ]]; then
        record errexit blocked "spec gate rejected an accept block that fails before its last line" 0
    else
        record errexit accepted "verify.sh exit $rc on an accept block that fails before its last line — the spec gate judged only its last command" 1
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
    # sync pins the baseline by its SHA-256; without a tool for it the
    # sandbox cannot be built, which is not a broken sandbox (#122).
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        record contract skipped "neither sha256sum nor shasum is installed -- the contract gate cannot verify a pin (install one)" 1
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
# Project canary (feature 006, research R12): a project gate that always
# fails, declared to skip the agent boundary. The sandboxed verify.sh must
# fail at ci naming it (a declared skip never reaches CI, constitution
# Principle III), and at agent report it skipped by declaration, not run.
# The agent run clears GATES_POLICY_FILE, which applies at that boundary
# (#196) and would replace the sandbox's policy.
# ---------------------------------------------------------------------------
run_project_canary() {
    local d="$WORKDIR/project"
    project_sandbox "$d" '{ "hooks": { "verify-quality": { "orchestrator": "none", "severity": "error", "gates": { "canary-fail": { "command": "exit 1", "severity": "error", "skip_at": ["agent"] } } } } }'
    local rc=0 out agent
    out="$(CLAUDE_PROJECT_DIR="$d" env -u GATES_SPEC_EXEC \
        bash "$d/.specify/gates/verify.sh" --boundary ci 2>&1)" || rc=$?
    agent="$(CLAUDE_PROJECT_DIR="$d" env -u GATES_SPEC_EXEC -u GATES_POLICY_FILE \
        bash "$d/.specify/gates/verify.sh" --boundary agent --json 2>/dev/null \
        | jq -r '.gates[]? | select(.name == "canary-fail") | "\(.status): \(.detail)"' 2>/dev/null)" || true
    if [[ "$rc" -ne 2 ]] || ! grep -q '^  \[fail\] canary-fail' <<<"$out"; then
        record project accepted "verify.sh exit $rc at ci on a failing project gate declared to skip agent -- the project gate did not block at ci" 1
    elif [[ "$agent" != "skipped: skipped by declaration at agent" ]]; then
        record project accepted "at agent the gate declared to skip it was reported as '${agent:-absent}', not skipped by declaration" 1
    else
        record project blocked "a failing project gate blocked at ci and was reported skipped by declaration at agent" 0
    fi
}

# ---------------------------------------------------------------------------
# Covered canary (feature 007, research R12): a Complete feature whose one
# accept block fails, and a covering script that vouches for every block at
# agent and git. At ci the spec gate itself must fail naming the block (CI
# never asks the script, constitution Principle III); the parity gate
# failing alone does not count. At git the block must be reported covered
# and the run pass, proving the mechanism works where it is declared.
# ---------------------------------------------------------------------------
run_covered_canary() {
    local d="$WORKDIR/covered"
    project_sandbox "$d" '{ "hooks": { "verify-quality": { "orchestrator": "none", "severity": "error" } }, "spec": { "severity": "error", "cover": { "script": "cover.sh", "at": ["agent", "git"] } } }'
    mkdir -p "$d/specs/900-canary-fixture" || setup_fail "covered fixture dir"
    git init -q "$d" >/dev/null 2>&1 || setup_fail "covered fixture git init"
    printf '# Canary Fixture\n\n**Status**: Complete\n' \
        >"$d/specs/900-canary-fixture/spec.md" || setup_fail "covered fixture spec.md"
    {
        echo '- [x] T001 A criterion that must fail'
        echo ''
        echo '  ```accept'
        echo '  false'
        echo '  ```'
    } >"$d/specs/900-canary-fixture/tasks.md" || setup_fail "covered fixture tasks.md"
    printf '#!/bin/sh\necho "covered by canary"\n' >"$d/cover.sh" || setup_fail "covered fixture script"
    chmod +x "$d/cover.sh" || setup_fail "covered fixture chmod"
    local rc=0 grc=0 out gout
    out="$(CLAUDE_PROJECT_DIR="$d" env -u GATES_SPEC_EXEC \
        bash "$d/.specify/gates/verify.sh" --boundary ci 2>&1)" || rc=$?
    gout="$(CLAUDE_PROJECT_DIR="$d" env -u GATES_SPEC_EXEC \
        bash "$d/.specify/gates/verify.sh" --boundary git 2>&1)" || grc=$?
    if [[ "$rc" -ne 2 ]] || ! grep -q '^  \[fail\] spec -- 900-canary-fixture: "T001 A criterion that must fail"' <<<"$out"; then
        record covered accepted "verify.sh exit $rc at ci on a failing block a covering script vouches for -- the spec gate did not fail it at ci" 1
    elif [[ "$grc" -ne 0 ]] || ! grep -q -- '-- covered: covered by canary' <<<"$gout"; then
        record covered accepted "at git the declared covering script did not cover the block (exit $grc)" 1
    else
        record covered blocked "a failing block a covering script vouches for failed at ci and was reported covered at git" 0
    fi
}

# ---------------------------------------------------------------------------
# Run + report
# ---------------------------------------------------------------------------
for id in $CANARY_SET; do
    want "$id" || continue
    case "$id" in
        format) run_format_canary ;;
        markdown) run_markdown_canary ;;
        shell) run_shell_canary ;;
        bash) run_bash_canary ;;
        protect) run_protect_canary ;;
        prhook) run_prhook_canary ;;
        bulk) run_bulk_canary ;;
        local) run_local_canary ;;
        secret) run_secret_canary ;;
        credential) run_credential_canary ;;
        protected) run_protected_canary ;;
        branding) run_branding_canary ;;
        pr) run_pr_canary ;;
        spec) run_spec_canary ;;
        errexit) run_errexit_canary ;;
        contract) run_contract_canary ;;
        project) run_project_canary ;;
        covered) run_covered_canary ;;
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
