#!/bin/bash
# shellcheck shell=bash
set -uo pipefail

# spec-gates pull/merge request check -- CI boundary (issues #53, #56).
#
# Three checks that need PR/MR context only CI has, so they are deliberately
# NOT verify.sh gates (verify.sh checks the tree identically at every
# boundary):
#
# 1. PR/MR text: the title and description must pass the same message rules
#    as commit-msg (lib/message.sh: AI-isms, branding, emoji,
#    Co-Authored-By, conventional title). On squash-merge repositories this
#    text becomes the commit on the default branch. With a range, the
#    rules come from the base's policy (#147), like the check below.
# 2. Protected changes: every commit in the range that changes a
#    protected_files.extra path must declare it ("Protected-Change: <path>")
#    and name an approver ("Approved-By: <name>") -- the commit-msg rule via
#    gates_protected_check, re-checked server-side for commits that never
#    passed a local hook. The rules come from the policy at the base, not the
#    PR head (#123); the protected list per commit is the union of the
#    policies at the base, the commit and its parent, and policy.json plus
#    hooks.local.d are always protected. Merge commits are checked for the
#    paths they change against every parent. Declarations in the PR/MR
#    description count for every commit (a squash merge keeps the
#    description, not the commit trailers).
# 3. Secrets and forbidden files: every commit in the range is scanned with
#    the pre-commit rules (lib/secrets.sh) for the files it adds or
#    changes, so commits that never ran pre-commit (cherry-pick, rebase,
#    am, revert, --no-verify) are scanned before merge (#212).
#
# A base whose policy is invalid stops the check (exit 2): its rules cannot
# be read, and reading nothing would pass everything (#166). A base with no
# policy at all (the PR that adopts spec-gates) is checked against the PR's
# own policy, with policy.json, the constitution, hooks.local.d and the
# contract artifacts protected whatever that policy says.
#
# The runtime libraries load from $GATES_RUNTIME_DIR/lib when it is set
# (default: the project's .specify/gates). The CI templates use it to run
# the base revision's pr-check.sh and lib against the PR checkout, so a PR
# cannot replace the code that judges it (#166).
#
# Usage:
#   pr-check.sh [--title <text>] [--body-file <file>] [--range <base>..<head>]
#
# Inputs, flag first, then the CI's own variables:
#   title  --title | $GATES_PR_TITLE | $CI_MERGE_REQUEST_TITLE | $CHANGE_TITLE
#   body   --body-file | $GATES_PR_BODY | $CI_MERGE_REQUEST_DESCRIPTION
#          (GitLab: when that is truncated, or unset before GitLab 16.7, the
#          full description is fetched from the API with $GATES_GITLAB_TOKEN
#          or $CI_JOB_TOKEN; a truncated one that cannot be fetched fails)
#   range  --range | $GATES_COMMIT_RANGE | origin/$GITHUB_BASE_REF..HEAD
#          (pull_request events) | $CI_MERGE_REQUEST_DIFF_BASE_SHA..HEAD |
#          origin/$CHANGE_TARGET..HEAD
# A check with no input is skipped: a push to the default branch has no PR.
#
# Exit codes: 0 = pass or skipped, 1 = violation(s), 2 = setup error (for
# example a range that cannot be resolved -- fetch full history).

RANGE="${GATES_COMMIT_RANGE:-}"
TITLE="${GATES_PR_TITLE:-${CI_MERGE_REQUEST_TITLE:-${CHANGE_TITLE:-}}}"
BODY_FILE=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --range) RANGE="${2:?--range needs <base>..<head>}"; shift 2 ;;
        --title) TITLE="${2?--title needs text}"; shift 2 ;;
        --body-file) BODY_FILE="${2:?--body-file needs a path}"; shift 2 ;;
        *) echo "pr-check: unknown argument: $1" >&2; exit 2 ;;
    esac
done

# Both are required: without jq the policy reader returns no protected
# paths, which would read as "nothing protected" and pass (#121).
for _tool in jq git; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "pr-check: $_tool is not installed -- install it in the CI image (apt-get install $_tool, apk add $_tool, brew install $_tool)" >&2
        exit 2
    fi
done
PROJECT_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
    echo "pr-check: not inside a git repository" >&2
    exit 2
}
RUNTIME_DIR="$PROJECT_ROOT/.specify/gates"
if [[ -n "${GATES_RUNTIME_DIR:-}" ]]; then
    RUNTIME_DIR="$(cd "$GATES_RUNTIME_DIR" 2>/dev/null && pwd)" || {
        echo "pr-check: GATES_RUNTIME_DIR is not a directory: $GATES_RUNTIME_DIR" >&2
        exit 2
    }
fi
cd "$PROJECT_ROOT" || exit 2
# The rules come from the base policy, or the checked-out one on an
# adoption PR, never from the environment (#196): an inherited
# GATES_POLICY_FILE is ignored, and said so.
if [[ -n "${GATES_POLICY_FILE:-}" ]]; then
    echo "pr-check: GATES_POLICY_FILE=$GATES_POLICY_FILE is ignored at the CI boundary; the repository's policy applies" >&2
    unset GATES_POLICY_FILE
fi
POLICY_LIB="$RUNTIME_DIR/lib/policy.sh"
if [[ ! -f "$POLICY_LIB" ]]; then
    echo "pr-check: $POLICY_LIB not found -- re-project the runtime (/speckit.gates.upgrade)" >&2
    exit 2
fi
# shellcheck source=/dev/null disable=SC1091
source "$POLICY_LIB"
MESSAGE_LIB="$RUNTIME_DIR/lib/message.sh"
if ! command -v gates_protected_check >/dev/null 2>&1 || [[ ! -f "$MESSAGE_LIB" ]]; then
    echo "pr-check: projected runtime predates pr-check.sh -- re-project it (/speckit.gates.upgrade)" >&2
    exit 2
fi
# shellcheck source=/dev/null disable=SC1091
source "$MESSAGE_LIB"
SECRETS_LIB="$RUNTIME_DIR/lib/secrets.sh"
if [[ ! -f "$SECRETS_LIB" ]]; then
    echo "pr-check: $SECRETS_LIB not found -- the projected runtime predates the range's secret scan; re-project it (/speckit.gates.upgrade)" >&2
    exit 2
fi
# shellcheck source=/dev/null disable=SC1091
source "$SECRETS_LIB"
if ! command -v gates_secret_scan_range >/dev/null 2>&1; then
    echo "pr-check: $SECRETS_LIB predates pr-check.sh -- re-project the runtime (/speckit.gates.upgrade)" >&2
    exit 2
fi
FAILED=0

BODY=""
if [[ -n "$BODY_FILE" ]]; then
    BODY="$(cat "$BODY_FILE" 2>/dev/null)" || {
        echo "pr-check: cannot read $BODY_FILE" >&2
        exit 2
    }
else
    BODY="${GATES_PR_BODY:-${CI_MERGE_REQUEST_DESCRIPTION:-}}"
fi

# GitLab MR pipelines (#67). The CI variable can be truncated
# (CI_MERGE_REQUEST_DESCRIPTION_IS_TRUNCATED=true), and GitLab before 16.7
# does not set it at all. In both cases fetch the full description through
# the API: GATES_GITLAB_TOKEN (a read_api token) first, then CI_JOB_TOKEN as
# a best effort (its API scope may not cover merge requests).
# http_get <header> <url>: body on stdout, nonzero on any failure. curl when
# present, else python3's urllib: slim CI images (the GitLab template's
# node:*-slim) ship without curl, while python3 is already required.
http_get() { # <header "Name: value"> <url>
    if command -v curl >/dev/null 2>&1; then
        curl -fsS --max-time 20 -H "$1" "$2" 2>/dev/null
        return
    fi
    python3 - "$1" "$2" <<'PYEOF' 2>/dev/null
import sys, urllib.request

name, _, value = sys.argv[1].partition(":")
request = urllib.request.Request(sys.argv[2], headers={name.strip(): value.strip()})
with urllib.request.urlopen(request, timeout=20) as response:
    sys.stdout.write(response.read().decode("utf-8"))
PYEOF
}

gitlab_fetch_description() { # -> full description on stdout, or return 1
    [[ -n "${CI_API_V4_URL:-}" && -n "${CI_PROJECT_ID:-}" && -n "${CI_MERGE_REQUEST_IID:-}" ]] || return 1
    local url="$CI_API_V4_URL/projects/$CI_PROJECT_ID/merge_requests/$CI_MERGE_REQUEST_IID" json
    if [[ -n "${GATES_GITLAB_TOKEN:-}" ]] \
        && json="$(http_get "PRIVATE-TOKEN: $GATES_GITLAB_TOKEN" "$url")" \
        && printf '%s' "$json" | jq -e 'has("description")' >/dev/null 2>&1; then
        printf '%s' "$json" | jq -r '.description // ""'
        return 0
    fi
    if [[ -n "${CI_JOB_TOKEN:-}" ]] \
        && json="$(http_get "JOB-TOKEN: $CI_JOB_TOKEN" "$url")" \
        && printf '%s' "$json" | jq -e 'has("description")' >/dev/null 2>&1; then
        printf '%s' "$json" | jq -r '.description // ""'
        return 0
    fi
    return 1
}
# fetch_fix: what makes the API fetch work. Without curl and python3's
# urllib no token helps, so that names the missing fetcher (#223).
fetch_fix() {
    if ! command -v curl >/dev/null 2>&1 && ! python3 -c 'import urllib.request' >/dev/null 2>&1; then
        printf '%s' "neither curl nor python3's urllib is available to fetch it; install curl or python3"
        [[ -n "${GATES_GITLAB_TOKEN:-}" ]] || printf '%s' " and set GATES_GITLAB_TOKEN to a read_api token"
    else
        printf '%s' "set GATES_GITLAB_TOKEN to a read_api token"
    fi
}
DESCRIPTION_UNCHECKABLE=""
if [[ -z "$BODY_FILE" && -z "${GATES_PR_BODY:-}" ]]; then
    if [[ "${CI_MERGE_REQUEST_DESCRIPTION_IS_TRUNCATED:-}" == "true" ]]; then
        if FULL="$(gitlab_fetch_description)"; then
            BODY="$FULL"
            echo "pr-check: GitLab truncated the MR description; checking the full text fetched from the API"
        else
            DESCRIPTION_UNCHECKABLE="GitLab truncated the MR description and the full text could not be fetched ($(fetch_fix), or shorten the description)"
        fi
    elif [[ -n "${CI_MERGE_REQUEST_IID:-}" && -z "${CI_MERGE_REQUEST_DESCRIPTION+set}" ]]; then
        if FULL="$(gitlab_fetch_description)"; then
            BODY="$FULL"
            echo "pr-check: CI_MERGE_REQUEST_DESCRIPTION is not set (GitLab < 16.7); checking the description fetched from the API"
        else
            echo "pr-check: NOTICE -- CI_MERGE_REQUEST_DESCRIPTION is not set (GitLab < 16.7) and the description could not be fetched from the API; only the MR title is checked ($(fetch_fix) to check the description)"
        fi
    fi
fi
# Web forms submit CRLF line endings.
BODY="$(printf '%s' "$BODY" | tr -d '\r')"
TITLE="$(printf '%s' "$TITLE" | tr -d '\r')"

# GitLab marks a draft MR in its title (#261): "Draft: feat: x" while the MR
# is a draft, "feat: x" once it is ready, and the merged text never carries
# the marker. One documented marker at the very start, any case, followed by
# whitespace, is dropped before the message rules run, so the format and
# length checks see the title that will be merged. WIP: and [WIP] are the
# markers before GitLab 14.0. Only in GitLab jobs: a GitHub draft is a flag,
# and a "WIP:" there would be squash-merged as written. A marker elsewhere
# stays, and a title that is only a marker is checked as it is.
if [[ "${GITLAB_CI:-}" == "true" || -n "${CI_MERGE_REQUEST_IID:-}" ]]; then
    _draft_re='^(draft:|\[draft\]|\(draft\)|wip:|\[wip\])[[:space:]]+([^[:space:]].*)$'
    shopt -s nocasematch
    if [[ "$TITLE" =~ $_draft_re ]]; then
        TITLE="${BASH_REMATCH[2]}"
        echo "pr-check: GitLab draft marker '${BASH_REMATCH[1]}' dropped from the MR title; checking: $TITLE"
    fi
    shopt -u nocasematch
fi

# Resolve the PR/MR range and load the policy committed at its base, before
# either check runs. -> 0 resolved (BASE, HEAD_REF set; GATES_POLICY_FILE
# points at the base policy when the base has one), 1 no range in this
# context, 2 setup error (message printed).
BASE_POLICY=""
trap '[[ -n "$BASE_POLICY" ]] && rm -f "$BASE_POLICY"' EXIT
resolve_range() {
    if [[ -z "$RANGE" ]]; then
        if [[ "${GITHUB_EVENT_NAME:-}" == pull_request* && -n "${GITHUB_BASE_REF:-}" ]]; then
            RANGE="origin/$GITHUB_BASE_REF..HEAD"
        elif [[ -n "${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}" ]]; then
            RANGE="$CI_MERGE_REQUEST_DIFF_BASE_SHA..HEAD"
        elif [[ -n "${CHANGE_TARGET:-}" ]]; then
            RANGE="origin/$CHANGE_TARGET..HEAD"
        fi
    fi
    if [[ -z "$RANGE" ]]; then
        return 1
    fi
    if [[ "$RANGE" != *..* ]]; then
        echo "pr-check: --range must be <base>..<head>, got: $RANGE" >&2
        return 2
    fi
    BASE="${RANGE%%..*}"
    HEAD_REF="${RANGE##*..}"
    for ref in "$BASE" "$HEAD_REF"; do
        if ! git rev-parse -q --verify "$ref^{commit}" >/dev/null 2>&1; then
            echo "pr-check: cannot resolve '$ref' in this clone -- fetch full history (actions/checkout fetch-depth: 0)" >&2
            return 2
        fi
    done

    # The rules come from the base (#123, #147): the PR under review must
    # not be able to relax them, for its text or its protected changes. An
    # invalid base policy is a setup error, never an empty rule set (#166).
    # A base without a policy (the adoption PR) falls back to the
    # checked-out one, which must be valid too, and widens the
    # always-protected paths below.
    local err head_policy
    BASE_POLICY="$(mktemp 2>/dev/null || mktemp -t gates-policy)" || return 2
    if gates_policy_at_rev "$BASE" "$BASE_POLICY"; then
        if ! err="$(GATES_POLICY_FILE="$BASE_POLICY" gates_validate_policy "$BASE_POLICY" 2>&1)"; then
            echo "pr-check: ERROR -- the policy committed at the base ($BASE) is invalid, so its rules cannot be checked; fix .specify/gates/policy.json on the base branch first:" >&2
            printf '%s\n' "$err" | sed 's/^/  /' >&2
            return 2
        fi
        export GATES_POLICY_FILE="$BASE_POLICY"
    else
        rm -f "$BASE_POLICY"
        BASE_POLICY=""
        ADOPTION=true
        head_policy="$(gates_policy_file)"
        if [[ -f "$head_policy" ]] && ! err="$(gates_validate_policy "$head_policy" 2>&1)"; then
            echo "pr-check: ERROR -- the base ($BASE) has no policy and the pull request's own policy is invalid:" >&2
            printf '%s\n' "$err" | sed 's/^/  /' >&2
            return 2
        fi
        echo "pr-check: NOTICE -- the base ($BASE) has no .specify/gates/policy.json (adoption PR); checking against the pull request's own policy, with policy.json, the constitution, hooks.local.d and the contract artifacts protected regardless"
    fi
    return 0
}
ADOPTION=false
RANGE_RC=0
resolve_range || RANGE_RC=$?
# The rules cannot be read: checking the text against the checkout's policy
# would only add noise to the setup error.
[[ "$RANGE_RC" -eq 2 ]] && exit 2

# --- 1. PR/MR text ---
if [[ -n "$DESCRIPTION_UNCHECKABLE" ]]; then
    # Fail closed: passing on the visible part would hide a violation in the
    # truncated tail.
    echo "pr-check: ERROR -- $DESCRIPTION_UNCHECKABLE" >&2
    FAILED=$((FAILED + 1))
elif [[ -n "$TITLE" ]] || [[ "$BODY" =~ [^[:space:]] ]]; then
    if gates_message_check pr "$TITLE"$'\n\n'"$BODY"; then
        echo "pr-check: PR/MR title and description pass the message rules"
    else
        echo "pr-check: PR/MR title or description violates the message rules (see above)" >&2
        FAILED=$((FAILED + 1))
    fi
else
    echo "pr-check: text skipped -- no PR/MR title or description in this context"
fi

# --- 2. Protected changes over the commit range ---
# Paths checked whatever the policy says: the runtime's built-in entries plus
# the policy file itself, so a PR cannot switch its own check off (#123).
ALWAYS_PROTECTED="$GATES_BUILTIN_PROTECTED"$'\n'".specify/gates/policy.json"
ALWAYS_NAMES="policy.json, hooks.local.d"
# Adoption PR: no base policy decides what is protected, so the constitution
# joins the list rather than depending on the policy the PR brings (#166).
if [[ "$ADOPTION" == "true" ]]; then
    ALWAYS_PROTECTED="$ALWAYS_PROTECTED"$'\n'".specify/memory/constitution.md"
    ALWAYS_NAMES="policy.json, the constitution, hooks.local.d"
fi
protected_range_check() { # -> 0 pass/skip, 1 violations, 2 setup error
    if [[ "$RANGE_RC" -eq 1 ]]; then
        echo "pr-check: protected range skipped -- no pull/merge request range (pass --range or set GATES_COMMIT_RANGE)"
        return 0
    fi
    [[ "$RANGE_RC" -eq 0 ]] || return 2
    FULL_LIST=true
    if ! gates_protected_trailer_enabled; then
        FULL_LIST=false
        echo "pr-check: git.protected_change_trailer is false -- checking only the always-protected paths ($ALWAYS_NAMES, the covering script)"
    fi

    BODY_DECLARED="$(printf '%s\n' "$BODY" | gates_trailer_values protected-change)"
    BODY_APPROVERS="$(printf '%s\n' "$BODY" | gates_trailer_values approved-by)"

    # The range is read in a few passes over all commits, not a handful of
    # processes per commit (#221): one rev-list with parents, one diff-tree
    # for every commit's paths, one cat-file for the policy blobs, one
    # rev-list naming the commits that may carry trailers.
    local tmp
    tmp="$(mktemp -d 2>/dev/null || mktemp -d -t gates-range)" || return 2
    # Merge commits included (#123): a merge is judged on its own edits.
    git rev-list --reverse --parents "$BASE..$HEAD_REF" >"$tmp/revs" 2>/dev/null
    local -a commits=() merges=() pkeys=() trl=()
    local c rest line n=0 i
    while read -r c rest; do
        [[ -z "$c" ]] && continue
        commits[n]="$c"
        merges[n]=0
        [[ "$rest" == *" "* ]] && merges[n]=1
        n=$((n + 1))
    done <"$tmp/revs"
    # One header line per commit (--always), then its paths. With the
    # parents given, -c is the combined diff for a merge and the plain diff
    # otherwise: for a merge, the paths whose result differs from every
    # parent. A path equal to one parent came from the base or from a
    # commit in this range, checked on its own.
    # pairs: "<commit index><TAB><path>"; c.<index>: that commit's paths.
    git diff-tree --stdin -r --root -c --name-only --no-renames --always \
        --format=$'\001%H' <"$tmp/revs" 2>/dev/null \
        | awk 'BEGIN { i = -1 } /^\001/ { i++; next } i >= 0 && $0 != "" { print i "\t" $0 }' >"$tmp/pairs"
    awk -F'\t' -v d="$tmp" 'NR == 1 || $1 != last { if (f != "") close(f); last = $1; f = d "/c." $1 }
        { print substr($0, length($1) + 2) > f }' "$tmp/pairs"
    # The protected list per commit is the union of the policies at the
    # base, the commit's parent and the commit. It depends only on the
    # policy blobs there, so it is built once per distinct set of blobs.
    if [[ "$FULL_LIST" == "true" ]]; then
        {
            for c in "$BASE" "${commits[@]+"${commits[@]}"}"; do
                [[ "$c" != "$BASE" ]] && printf '%s^:.specify/gates/policy.json\n%s^:.specify/gates/policy.effective.json\n' "$c" "$c"
                printf '%s:.specify/gates/policy.json\n%s:.specify/gates/policy.effective.json\n' "$c" "$c"
            done
        } | git cat-file --batch-check='%(objectname)' >"$tmp/blobs" 2>/dev/null
        local -a keys=()
        local k=0 key
        while read -r line; do
            [[ "$line" == *" missing" ]] && line="-"
            keys[k]="$line"
            k=$((k + 1))
        done <"$tmp/blobs"
        i=0
        while [[ $i -lt $n ]]; do
            # base | parent | commit; an incomplete read is never cached.
            key="${keys[0]:-?},${keys[1]:-?}|${keys[2 + 4 * i]:-?},${keys[3 + 4 * i]:-?}|${keys[4 + 4 * i]:-?},${keys[5 + 4 * i]:-?}"
            [[ "$key" == *"?"* ]] && key="?$i"
            pkeys[i]="$key"
            i=$((i + 1))
        done
    fi
    # Only a message that names a trailer key can yield its values. If the
    # search fails, every message is parsed.
    if git rev-list --regexp-ignore-case --fixed-strings --grep=protected-change \
        --grep=approved-by "$BASE..$HEAD_REF" >"$tmp/trl" 2>/dev/null; then
        i=0
        while read -r line; do
            trl[i]="$line"
            i=$((i + 1))
        done < <(awk 'FILENAME == ARGV[1] { t[$1] = 1; next } NF { print (($1 in t) ? 1 : 0) }' "$tmp/trl" "$tmp/revs")
    fi

    # The protected list of each commit (one of set_vals), then each
    # distinct path matched once per list: a long range changes the same
    # paths over and over. gates_match_protected reads paths with `read`,
    # which trims blanks, so the paths are trimmed the same way first.
    local -a set_keys=() set_vals=() prot=()
    local idx j key patterns changed protected trailers declared approvers path always="$ALWAYS_PROTECTED"
    # The covering script (spec.cover.script, feature 007) is protected like
    # the policy, so it stays checked with the trailer rule off: the one
    # declared by the base or any commit of the range, for every commit.
    if [[ "$FULL_LIST" != "true" ]]; then
        always="$always"$'\n'"$(gates_cover_protected "$BASE" "${commits[@]+"${commits[@]}"}" 2>/dev/null)"
    fi
    for ((idx = 0; idx < n; idx++)); do
        key="${pkeys[idx]:-all}"
        for ((j = 0; j < ${#set_keys[@]}; j++)); do
            [[ "${set_keys[j]}" == "$key" ]] && break
        done
        if [[ $j -eq ${#set_keys[@]} ]]; then
            if [[ "$FULL_LIST" == "true" ]]; then
                patterns="$(gates_protected_list "$BASE" "${commits[idx]}^" "${commits[idx]}" 2>/dev/null)"$'\n'"$ALWAYS_PROTECTED"
            else
                patterns="$always"
            fi
            set_keys[j]="$key"
            set_vals[j]="$patterns"
        fi
        printf '%s\t%s\n' "$idx" "$j"
    done >"$tmp/psets"
    awk -F'\t' 'FILENAME == ARGV[1] { s[$1] = $2; next }
        { p = substr($0, length($1) + 2); gsub(/^[ \t]+|[ \t]+$/, "", p)
          if (p != "") print $1 "\t" s[$1] "\t" p }' "$tmp/psets" "$tmp/pairs" >"$tmp/keyed"
    for ((j = 0; j < ${#set_keys[@]}; j++)); do
        awk -F'\t' -v j="$j" '$2 == j && !seen[$3]++ { print $3 }' "$tmp/keyed" \
            | gates_match_protected "${set_vals[j]}" | awk -v j="$j" '{ print j "\t" $0 }'
    done >"$tmp/matched"
    while IFS=$'\t' read -r idx path; do
        prot[idx]="${prot[idx]:-}$path"$'\n'
    done < <(awk -F'\t' 'FILENAME == ARGV[1] { m[$0] = 1; next } (($2 "\t" $3) in m) { print $1 "\t" $3 }' \
        "$tmp/matched" "$tmp/keyed")

    CHECKED=0
    TOUCHING=0
    VIOLATIONS=0
    : >"$tmp/changed"
    for ((idx = 0; idx < n; idx++)); do
        c="${commits[idx]}"
        CHECKED=$((CHECKED + 1))
        changed=""
        if [[ "${merges[idx]}" == 1 ]]; then
            # Declarations may name anything the merge brought in relative
            # to its first parent.
            changed="$(git diff --name-only --no-renames "$c^1" "$c")"
            printf '%s\n' "$changed" >>"$tmp/changed"
        fi
        protected="${prot[idx]:-}"
        protected="${protected%$'\n'}"
        declared=""
        approvers=""
        if [[ "${trl[idx]:-1}" == 1 ]]; then
            trailers="$(git log -1 --format=%B "$c" | git interpret-trailers --parse --no-divider 2>/dev/null)"
            declared="$(printf '%s\n' "$trailers" | gates_trailer_values protected-change)"
            approvers="$(printf '%s\n' "$trailers" | gates_trailer_values approved-by)"
        fi
        [[ -z "$protected" && -z "$declared" ]] && continue
        if [[ "${merges[idx]}" != 1 && -f "$tmp/c.$idx" ]]; then
            changed="$(cat "$tmp/c.$idx")"
        fi
        [[ -n "$protected" ]] && TOUCHING=$((TOUCHING + 1))
        # Description declarations cover every commit; whether they name a real
        # change is judged once against the whole range below.
        all_declared="$(printf '%s\n%s\n' "$declared" "$BODY_DECLARED" | awk 'NF')"
        all_approvers="$(printf '%s\n%s\n' "$approvers" "$BODY_APPROVERS" | awk 'NF')"
        out="$(gates_protected_check "$protected" "$changed"$'\n'"$BODY_DECLARED" "$all_declared" "$all_approvers" 2>&1)"
        rc=$?
        if [[ "$rc" -ne 0 ]]; then
            echo "commit $(git log -1 --format='%h %s' "$c"):" >&2
            printf '%s\n' "$out" | sed 's/^/  /' >&2
            VIOLATIONS=$((VIOLATIONS + rc))
        fi
    done

    if [[ -n "$BODY_DECLARED" ]]; then
        # Every path the range changes: each commit's own, and for a merge
        # everything it brought in relative to its first parent.
        cut -f2- "$tmp/pairs" >>"$tmp/changed"
        out="$(gates_protected_check "" "$(cat "$tmp/changed")" "$BODY_DECLARED" "x" 2>&1)"
        rc=$?
        if [[ "$rc" -ne 0 ]]; then
            echo "pull/merge request description:" >&2
            printf '%s\n' "$out" | sed 's/^/  /' >&2
            VIOLATIONS=$((VIOLATIONS + rc))
        fi
    fi
    rm -rf "$tmp"

    echo "pr-check: $RANGE -- $CHECKED commit(s) checked, $TOUCHING touching protected paths, $VIOLATIONS violation(s)"
    [[ "$VIOLATIONS" -gt 0 ]] && return 1
    return 0
}

# --- 3. Secrets and forbidden files over the commit range ---
# The pre-commit scan, with its rules (lib/secrets.sh), over the files each
# commit in the range adds or changes (issue #212): commits made by
# cherry-pick, rebase, am or revert, or with --no-verify, never ran
# pre-commit. Each commit's own copy is scanned, so a secret added and
# removed again within the range still fails: it is in the history the
# range pushes and merges, and needs rotating and the branch rewriting. A
# rename is listed under its new name (--no-renames), as in pre-commit. A
# merge commit is scanned for the paths whose result differs from every
# parent, so merging the base in does not re-scan the base's files.
# gates_secret_scan_range reads each distinct blob the range adds once per
# rule (#228).
secret_range_check() { # -> 0 pass/skip, 1 findings, 2 setup error
    if [[ "$RANGE_RC" -eq 1 ]]; then
        echo "pr-check: secret scan skipped -- no pull/merge request range (pass --range or set GATES_COMMIT_RANGE)"
        return 0
    fi
    [[ "$RANGE_RC" -eq 0 ]] || return 2
    local n out src=0
    n="$(git rev-list --count "$BASE..$HEAD_REF")" || return 2
    out="$(git rev-list --reverse "$BASE..$HEAD_REF" | gates_secret_scan_range)" || src=$?
    if [[ "$src" -eq 2 ]]; then
        echo "pr-check: ERROR -- cannot read the range's content for the secret scan (git grep failed)" >&2
        return 2
    fi
    if [[ "$src" -ne 0 ]]; then
        printf '%s\n' "$out" >&2
        echo "pr-check: $RANGE -- secrets or forbidden files in $n commit(s) scanned; remove them from the history (rewrite the branch) and rotate any exposed credential" >&2
        return 1
    fi
    echo "pr-check: $RANGE -- $n commit(s) scanned for secrets and forbidden files, none found"
    return 0
}

rc=0
protected_range_check || rc=$?
[[ "$rc" -eq 2 ]] && exit 2
[[ "$rc" -ne 0 ]] && FAILED=$((FAILED + 1))
rc=0
secret_range_check || rc=$?
[[ "$rc" -eq 2 ]] && exit 2
[[ "$rc" -ne 0 ]] && FAILED=$((FAILED + 1))
[[ "$FAILED" -gt 0 ]] && exit 1
exit 0
