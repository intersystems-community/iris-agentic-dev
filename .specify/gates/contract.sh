#!/bin/bash
# shellcheck shell=bash
set -uo pipefail

# spec-gates contract entry script (feature 003): the consumer side of a
# versioned policy contract.
#
#   contract.sh sync                     # fetch the DECLARED version; write pin+snapshot+effective
#   contract.sh sync --update [VERSION]  # move the pin: to VERSION, or the highest tag when omitted
#   contract.sh propose [--rationale T]  # package overlay deviations as an upstream change request
#
# sync is the only network moment the contract machinery has; verify.sh
# proves drift offline from the committed artifacts. policy.json is
# user-owned and never written here.
#
# Exit codes: 0 = success (including "nothing to sync/propose"),
#             1 = usage or environment error,
#             2 = contract failure (fetch, digest, validation, chained
#                 baseline, branch-name version).

GATES_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

# shellcheck source=lib/policy.sh disable=SC1091
source "$GATES_DIR/lib/policy.sh"
# shellcheck source=lib/attest.sh disable=SC1091
source "$GATES_DIR/lib/attest.sh"
# shellcheck source=lib/contract.sh disable=SC1091
source "$GATES_DIR/lib/contract.sh"

if ! command -v jq >/dev/null 2>&1; then
    echo "contract: jq not found (run /speckit.gates.doctor)" >&2
    exit 1
fi
if ! command -v git >/dev/null 2>&1; then
    echo "contract: git not found -- fetching a versioned baseline requires git" >&2
    exit 1
fi

gates_contract_paths "$PROJECT_ROOT"

# Print the deviation inventory (TSV from gates_contract_deviations) as
# human lines. Reads stdin.
print_deviations() {
    local n=0 class path from to rest
    while IFS=$'\t' read -r class path from to rest; do
        [[ -z "$class" ]] && continue
        echo "contract: deviation ($class): $path: baseline $from -> overlay $to"
        n=$((n + 1))
    done
    [[ "$n" -eq 0 ]] && echo "contract: no deviations -- the overlay only adds or strengthens"
    return 0
}

# Fetch + validate + materialize <version>, writing the three artifacts
# only after every check passes. Echoes nothing on the happy path except
# the summary; all failures name their cause and leave prior state intact.
materialize() { # <version>
    local version="$1"
    local work
    work="$(mktemp -d 2>/dev/null || mktemp -d -t gates-sync)" || {
        echo "contract: mktemp failed" >&2
        return 1
    }
    # Fetch the raw baseline document.
    if ! gates_contract_fetch "$CONTRACT_SOURCE" "$version" "$CONTRACT_BASEFILE" "$work/raw.json"; then
        rm -rf "$work"
        return 2
    fi
    # Canonicalize; a baseline that is not valid JSON dies here.
    if ! jq -S . "$work/raw.json" >"$work/baseline.json" 2>/dev/null; then
        rm -rf "$work"
        echo "contract: baseline at $CONTRACT_SOURCE@$version is not valid JSON" >&2
        return 2
    fi
    # Single-level inheritance: a chained baseline is refused.
    if jq -e 'has("extends")' "$work/baseline.json" >/dev/null 2>&1; then
        rm -rf "$work"
        echo "contract: baseline at $CONTRACT_SOURCE@$version itself declares extends -- chained baselines are not supported (v1 is single-level)" >&2
        return 2
    fi
    # The baseline must be a valid policy on its own.
    if ! GATES_POLICY_FILE="$work/baseline.json" gates_validate_policy "$work/baseline.json" >"$work/val.log" 2>&1; then
        echo "contract: baseline at $CONTRACT_SOURCE@$version fails policy validation:" >&2
        sed 's/^/  /' "$work/val.log" >&2
        rm -rf "$work"
        return 2
    fi
    # Materialize and validate the merge result too.
    if ! gates_contract_merge "$work/baseline.json" "$CONTRACT_OVERLAY" >"$work/effective.json"; then
        rm -rf "$work"
        echo "contract: could not merge baseline and overlay" >&2
        return 2
    fi
    if ! GATES_POLICY_FILE="$work/effective.json" gates_validate_policy "$work/effective.json" >"$work/val.log" 2>&1; then
        echo "contract: merged effective policy fails validation (baseline $CONTRACT_SOURCE@$version + overlay):" >&2
        sed 's/^/  /' "$work/val.log" >&2
        rm -rf "$work"
        return 2
    fi
    local digest
    digest="$(gates_sha256 "$work/baseline.json")" || {
        rm -rf "$work"
        return 1
    }
    jq -S -n --arg s "$CONTRACT_SOURCE" --arg v "$version" \
        --arg f "$CONTRACT_BASEFILE" --arg d "sha256:$digest" \
        '{ source: $s, version: $v, file: $f, digest: $d }' >"$work/baseline.lock.json"
    # Every check passed: move the three artifacts into place together.
    MATERIALIZED_DIR="$work"
    return 0
}

install_artifacts() { # <workdir>
    local work="$1"
    cp "$work/baseline.json" "$CONTRACT_SNAPSHOT"
    cp "$work/baseline.lock.json" "$CONTRACT_LOCK"
    cp "$work/effective.json" "$CONTRACT_EFFECTIVE"
    rm -rf "$work"
}

cmd_sync() {
    local update=0 target=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --update)
                update=1
                shift
                if [[ $# -gt 0 && "$1" != --* ]]; then
                    target="$1"
                    shift
                fi
                ;;
            *)
                echo "contract: sync: unknown argument: $1" >&2
                return 1
                ;;
        esac
    done
    if ! gates_contract_declared "$CONTRACT_OVERLAY"; then
        echo "contract: no extends declared in policy.json -- nothing to sync"
        return 0
    fi
    if ! GATES_POLICY_FILE="$CONTRACT_OVERLAY" gates_validate_policy "$CONTRACT_OVERLAY" >/dev/null 2>&1; then
        echo "contract: policy.json itself fails validation -- fix it before syncing" >&2
        return 2
    fi

    if [[ "$update" == "0" ]]; then
        # Plain sync: (re-)materialize the DECLARED version. Never moves a pin.
        local rc=0
        materialize "$CONTRACT_VERSION" || rc=$?
        [[ "$rc" -ne 0 ]] && return "$rc"
        install_artifacts "$MATERIALIZED_DIR"
        echo "contract: synced $CONTRACT_SOURCE@$CONTRACT_VERSION ($CONTRACT_BASEFILE)"
        echo "contract: pinned $(jq -r '.digest' "$CONTRACT_LOCK")"
        gates_contract_deviations "$CONTRACT_SNAPSHOT" "$CONTRACT_EFFECTIVE" | print_deviations
        return 0
    fi

    # Update mode (US2): move the pin to an explicit version or the highest
    # tag, as a reviewable change on its own branch -- never in place.
    local current=""
    [[ -f "$CONTRACT_LOCK" ]] && current="$(jq -r '.version // ""' "$CONTRACT_LOCK" 2>/dev/null)"
    if [[ -z "$current" ]]; then
        echo "contract: sync --update needs an existing pin -- run a plain sync first" >&2
        return 2
    fi
    if [[ -z "$target" ]]; then
        target="$(git ls-remote --tags "$CONTRACT_SOURCE" 2>/dev/null \
            | awk '{print $2}' | grep -v '\^{}$' | sed 's|^refs/tags/||' \
            | grep -E '^v?[0-9]' | gates_contract_version_max)"
        if [[ -z "$target" ]]; then
            echo "contract: no version tags found at $CONTRACT_SOURCE" >&2
            return 2
        fi
    fi
    if [[ "$target" == "$current" ]]; then
        echo "contract: already up to date ($current)"
        return 0
    fi
    local rc=0
    materialize "$target" || rc=$?
    [[ "$rc" -ne 0 ]] && return "$rc"
    local work="$MATERIALIZED_DIR"

    # Enforcement delta for the review body.
    local delta
    delta="$(gates_contract_deviations "$CONTRACT_SNAPSHOT" "$work/baseline.json" 2>/dev/null \
        | awk -F'\t' '{ printf "- %s: %s: %s -> %s\n", $1, $2, $3, $4 }')"
    [[ -z "$delta" ]] && delta="- no rule-level differences (metadata/ordering only)"

    if ! git -C "$PROJECT_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        echo "contract: not a git work tree -- printing the update instead of committing it"
        echo "contract: update available: $current -> $target"
        printf '%s\n' "$delta"
        echo "contract: re-run inside a git work tree to get a reviewable branch"
        rm -rf "$work"
        return 0
    fi

    local branch="gates/baseline-$target"
    if git -C "$PROJECT_ROOT" rev-parse --verify -q "refs/heads/$branch" >/dev/null; then
        rm -rf "$work"
        echo "contract: branch $branch already exists -- review or delete it first" >&2
        return 2
    fi
    # A temporary worktree keeps the user's checkout untouched (SC-003:
    # current enforcement stays at the old pin until the branch merges).
    local wt="$work/wt"
    if ! git -C "$PROJECT_ROOT" worktree add -q -b "$branch" "$wt" HEAD 2>/dev/null; then
        rm -rf "$work"
        echo "contract: could not create a worktree for $branch" >&2
        return 2
    fi
    mkdir -p "$wt/.specify/gates"
    cp "$work/baseline.json" "$wt/.specify/gates/baseline.json"
    cp "$work/baseline.lock.json" "$wt/.specify/gates/baseline.lock.json"
    cp "$work/effective.json" "$wt/.specify/gates/policy.effective.json"
    local msg new_digest
    new_digest="$(jq -r '.digest' "$work/baseline.lock.json")"
    msg="chore: update policy baseline $current -> $target

Source: $CONTRACT_SOURCE ($CONTRACT_BASEFILE)
New digest: $new_digest

Enforcement delta (baseline $current -> $target):
$delta"
    (
        cd "$wt" \
            && git add .specify/gates/baseline.json .specify/gates/baseline.lock.json .specify/gates/policy.effective.json \
            && git -c commit.gpgsign=false commit -q -m "$msg"
    ) || {
        git -C "$PROJECT_ROOT" worktree remove -f "$wt" >/dev/null 2>&1
        rm -rf "$work"
        echo "contract: could not commit the update on $branch" >&2
        return 2
    }
    git -C "$PROJECT_ROOT" worktree remove -f "$wt" >/dev/null 2>&1 || true
    rm -rf "$work"
    echo "contract: update $current -> $target committed on branch $branch"
    printf '%s\n' "$delta"
    if command -v gh >/dev/null 2>&1 \
        && git -C "$PROJECT_ROOT" remote get-url origin 2>/dev/null | grep -q github; then
        if git -C "$PROJECT_ROOT" push -q -u origin "$branch" 2>/dev/null \
            && (cd "$PROJECT_ROOT" && gh pr create --head "$branch" \
                --title "chore: update policy baseline $current -> $target" \
                --body "$msg" 2>/dev/null); then
            echo "contract: pull request opened for $branch"
            return 0
        fi
        echo "contract: could not open the PR automatically -- push and open it from branch $branch"
        return 0
    fi
    echo "contract: review and merge branch $branch to adopt the update"
    return 0
}

cmd_propose() {
    local rationale=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --rationale)
                rationale="${2:-}"
                shift 2 || {
                    echo "contract: propose: --rationale needs a value" >&2
                    return 1
                }
                ;;
            *)
                echo "contract: propose: unknown argument: $1" >&2
                return 1
                ;;
        esac
    done
    if ! gates_contract_declared "$CONTRACT_OVERLAY"; then
        echo "contract: no extends declared in policy.json -- nothing to propose"
        return 0
    fi
    if [[ ! -f "$CONTRACT_SNAPSHOT" || ! -f "$CONTRACT_LOCK" || ! -f "$CONTRACT_EFFECTIVE" ]]; then
        echo "contract: not synced -- run contract.sh sync before proposing" >&2
        return 2
    fi
    local deviations
    deviations="$(gates_contract_deviations "$CONTRACT_SNAPSHOT" "$CONTRACT_EFFECTIVE" || true)"
    if [[ -z "$deviations" ]]; then
        echo "contract: nothing to propose -- the overlay does not deviate from the baseline"
        return 0
    fi
    if [[ -z "$rationale" ]]; then
        if [[ -t 0 ]]; then
            printf 'contract: rationale for this proposal (one line): '
            IFS= read -r rationale
        fi
        if [[ -z "$rationale" ]]; then
            echo "contract: a rationale is required -- pass --rationale \"...\" (the baseline maintainer needs the why, not just the diff)" >&2
            return 1
        fi
    fi
    local pinned_version
    pinned_version="$(jq -r '.version' "$CONTRACT_LOCK")"
    local consumer
    consumer="$(basename "$PROJECT_ROOT")"
    local day
    day="$(date +%Y%m%d)"
    local branch="propose/$consumer-$day"

    local work
    work="$(mktemp -d 2>/dev/null || mktemp -d -t gates-propose)" || {
        echo "contract: mktemp failed" >&2
        return 1
    }
    if ! git clone -q "$CONTRACT_SOURCE" "$work/src" 2>/dev/null; then
        rm -rf "$work"
        echo "contract: could not clone $CONTRACT_SOURCE to prepare the proposal" >&2
        return 2
    fi
    if ! git -C "$work/src" checkout -q -b "$branch" "$pinned_version" 2>/dev/null; then
        rm -rf "$work"
        echo "contract: pinned version $pinned_version not found at $CONTRACT_SOURCE" >&2
        return 2
    fi
    # Apply exactly the deviating paths onto the baseline document.
    local paths_json
    paths_json="$(printf '%s\n' "$deviations" | awk -F'\t' '{print $5}' | jq -s -c '.')"
    jq -S --slurpfile eff "$CONTRACT_EFFECTIVE" --argjson ps "$paths_json" '
        reduce $ps[] as $p (.; setpath($p; ($eff[0] | getpath($p))))
    ' "$work/src/$CONTRACT_BASEFILE" >"$work/proposed.json" \
        && mv "$work/proposed.json" "$work/src/$CONTRACT_BASEFILE"
    local body
    body="Origin: $consumer
Pinned baseline: $CONTRACT_SOURCE@$pinned_version
Rationale: $rationale

Deviations proposed:
$(printf '%s\n' "$deviations" | awk -F'\t' '{ printf "- %s: %s: %s -> %s\n", $1, $2, $3, $4 }')"
    (
        cd "$work/src" \
            && git add "$CONTRACT_BASEFILE" \
            && git -c commit.gpgsign=false -c user.email="gates@$consumer" -c user.name="gates propose ($consumer)" \
                commit -q -m "policy: proposal from $consumer" -m "$body"
    ) || {
        rm -rf "$work"
        echo "contract: could not commit the proposal" >&2
        return 2
    }
    if command -v gh >/dev/null 2>&1 && printf '%s' "$CONTRACT_SOURCE" | grep -q github; then
        if (cd "$work/src" && git push -q origin "$branch" 2>/dev/null \
            && gh pr create --head "$branch" \
                --title "policy: proposal from $consumer" --body "$body" 2>/dev/null); then
            rm -rf "$work"
            echo "contract: proposal pull request opened against $CONTRACT_SOURCE"
            return 0
        fi
    fi
    mkdir -p "$PROJECT_ROOT/.specify/gates/proposals"
    local patch="$PROJECT_ROOT/.specify/gates/proposals/${branch//\//-}.patch"
    git -C "$work/src" format-patch -1 --stdout >"$patch"
    rm -rf "$work"
    echo "contract: proposal written to ${patch#"$PROJECT_ROOT"/}"
    echo "contract: apply it upstream with: git am ${patch#"$PROJECT_ROOT"/} (in a checkout of $CONTRACT_SOURCE)"
    return 0
}

case "${1:-}" in
    sync)
        shift
        cmd_sync "$@"
        ;;
    propose)
        shift
        cmd_propose "$@"
        ;;
    *)
        echo "usage: contract.sh sync [--update [VERSION]] | propose [--rationale TEXT]" >&2
        exit 1
        ;;
esac
