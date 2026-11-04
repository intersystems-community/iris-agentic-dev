#!/bin/bash
# shellcheck shell=bash
set -uo pipefail

# spec-gates projection: copy the enforcement runtime from the installed
# extension into the project, wire the agent and git boundaries, record what
# was written, and prove the result with the canary suite. One invocation,
# so a reviewer (or a permission classifier) approves one command, not one
# file write per runtime file (issue #72).
#
#   bash .specify/extensions/gates/runtime/project.sh [options]   # install / upgrade
#   bash .specify/gates/project.sh --check                        # projected copy
#
# Contract: specs/005-upgrade-safe-projection/contracts/project-sh.md.
# Never writes .specify/gates/policy.json. Never runs without a policy.
#
# Options:
#   --dry-run               print the plan, write nothing
#   --check                 like --dry-run; exit 1 if anything would change
#   --no-agent-hooks        skip .claude/hooks/gates/ and the settings merge
#   --no-git-hooks          skip git hook wiring
#   --take-upstream <path>  replace one locally edited file (repeatable)
#   --keep-local <path>     keep one locally edited file and hold it (repeatable)
#   --add-lint-ignores      add the vendored paths to .prettierignore
#   --probe-git             also run git hooks another tool owns in the proof
#   --wire-manager          add the gates entry to the hook manager's own
#                           config (.husky/<hook>, lefthook.yml,
#                           .pre-commit-config.yaml)
#   --allow-downgrade       accept a manifest newer than this version
#   --skip-canary           tests only (needs GATES_TEST=1)
#
# Exit: 0 projected and proven (or nothing to do) | 1 projected, but a proof
# failed or a hook needs the maintainer | 2 refused before writing | 3 local
# edits need a decision (nothing written).

say() { printf 'project: %s\n' "$*"; }
refuse() { # <message...>: exit 2 before anything is written
    local l
    for l in "$@"; do printf 'project: %s\n' "$l" >&2; done
    exit 2
}
inlist() { [[ -n "$1" ]] && grep -qxF -- "$2" <<<"$1"; }
addline() { if [[ -z "$1" ]]; then printf '%s' "$2"; else printf '%s\n%s' "$1" "$2"; fi; }

ARGS="$*" # for the projected copy's hand-off to the installed one
DRY=0 CHECK=0 AGENT=1 GITHOOKS=1 DOWNGRADE=0 SKIPCANARY=0 LINTIGN=0 PROBEGIT=0 WIREMGR=0
TAKE="" KEEP=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY=1 ;;
        --check) CHECK=1 DRY=1 ;;
        --no-agent-hooks) AGENT=0 ;;
        --no-git-hooks) GITHOOKS=0 ;;
        --take-upstream) [[ $# -ge 2 ]] || refuse "--take-upstream needs a path"; TAKE="$(addline "$TAKE" "${2#./}")"; shift ;;
        --keep-local) [[ $# -ge 2 ]] || refuse "--keep-local needs a path"; KEEP="$(addline "$KEEP" "${2#./}")"; shift ;;
        --allow-downgrade) DOWNGRADE=1 ;;
        --add-lint-ignores) LINTIGN=1 ;;
        --probe-git) PROBEGIT=1 ;;
        --wire-manager) WIREMGR=1 ;;
        --skip-canary) SKIPCANARY=1 ;;
        -h | --help) sed -n '5,30p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) refuse "unknown option: $1 (see --help)" ;;
    esac
    shift
done
while IFS= read -r p; do
    [[ -n "$p" ]] && inlist "$KEEP" "$p" \
        && refuse "$p is named by both --take-upstream and --keep-local; choose one"
done <<<"$TAKE"
if [[ "$SKIPCANARY" -eq 1 && "${GATES_TEST:-}" != "1" ]]; then
    refuse "--skip-canary is for the test suite only (GATES_TEST=1); projection must prove itself"
fi

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
[[ -d "$ROOT/.specify" ]] || refuse "$ROOT is not a Spec Kit project (no .specify/); run specify init first"
VEND="$ROOT/.specify/extensions/gates/runtime"
REL_ADD='specify extension add gates --from <the versioned release URL you verified> (README "Upgrade")'

for t in jq cmp; do
    command -v "$t" >/dev/null 2>&1 || refuse "$t not found; install it and re-run"
done
for l in manifest.sh install-state.sh managers.sh; do
    [[ -f "$SRC/lib/$l" ]] || refuse "runtime library $SRC/lib/$l is missing; reinstall the extension"
    # shellcheck source=/dev/null disable=SC1090,SC1091
    source "$SRC/lib/$l"
done
gates_sha256 "$SRC/project.sh" >/dev/null || refuse "neither sha256sum nor shasum is available; install one and re-run"

# --- Where are we running from? -------------------------------------------
STATE="$(gates_install_state "$ROOT")"
if [[ "$SRC" -ef "$ROOT/.specify/gates" ]]; then
    # The projected copy. After `extension remove` it is the only copy left,
    # so it diagnoses a half-done remove+add; it never projects itself.
    case "$STATE" in
        removed)
            refuse "the gates extension was removed but not added back (half-done upgrade)." \
                "Finish it: $REL_ADD" \
                "Then run: bash .specify/extensions/gates/runtime/project.sh"
            ;;
        mismatch)
            refuse "the extension registry and .specify/extensions/gates/ disagree (interrupted install?)." \
                "Re-run: specify extension remove gates --keep-config --force && $REL_ADD"
            ;;
        absent) refuse "the gates extension is not installed. Install it: $REL_ADD" ;;
    esac
    [[ -f "$VEND/project.sh" ]] || refuse "the installed extension has no project.sh (older than 0.4.0?). Upgrade it: $REL_ADD"
    if [[ "$CHECK" -eq 1 ]]; then
        # Same flags, so a --no-agent-hooks project is checked as one.
        # shellcheck disable=SC2086  # the original options, split as typed
        exec bash "$VEND/project.sh" $ARGS
    fi
    refuse "run the installed copy, not the projected one: bash .specify/extensions/gates/runtime/project.sh"
fi
VENDORED=0
if [[ "$SRC" -ef "$VEND" ]]; then
    VENDORED=1
    [[ "$STATE" == "mismatch" ]] \
        && refuse "the extension registry and .specify/extensions/gates/ disagree (interrupted install?)." \
            "Re-run: specify extension remove gates --keep-config --force && $REL_ADD"
fi

VERSION="$(gates_extension_version "$SRC/../extension.yml")"
[[ -n "$VERSION" ]] || refuse "cannot read the extension version from $SRC/../extension.yml"
[[ -f "$ROOT/.specify/gates/policy.json" ]] \
    || refuse "no .specify/gates/policy.json yet. /speckit.gates.init infers and approves the policy before projecting."
# Validated by the runtime being projected, since that is what will enforce
# it: projecting under an invalid policy would leave every boundary refusing
# to run (#124).
[[ -f "$SRC/lib/policy.sh" ]] || refuse "runtime library $SRC/lib/policy.sh is missing; reinstall the extension"
# shellcheck source=/dev/null disable=SC1090,SC1091
source "$SRC/lib/policy.sh"
if ! POLICY_ERR="$(gates_validate_policy "$(CLAUDE_PROJECT_DIR="$ROOT" gates_policy_file)" 2>&1)"; then
    refuse "the policy is invalid; fix it before projecting:" "$POLICY_ERR"
fi

gates_manifest_load "$ROOT"
if [[ "$GATES_MANIFEST_STATUS" == "corrupt" ]]; then
    refuse "$GATES_MANIFEST_REL is corrupt ($GATES_MANIFEST_ERROR)." \
        "It records what projection wrote, so it is never overwritten blindly. Inspect it, then delete it to treat every differing file as a local edit."
fi
if [[ "$GATES_MANIFEST_STATUS" == "ok" && "$DOWNGRADE" -eq 0 \
    && "$(gates_version_cmp "$GATES_MANIFEST_VERSION" "$VERSION")" == "1" ]]; then
    refuse "the runtime was projected by $GATES_MANIFEST_VERSION, newer than this extension ($VERSION)." \
        "Install the newer extension, or pass --allow-downgrade."
fi
gates_holds_load "$ROOT"
# shellcheck disable=SC2034  # read by gates_classify in lib/manifest.sh
GATES_KNOWN_FILE="$SRC/lib/known-releases.sha256"

# --- Plan --------------------------------------------------------------------
[[ "$STATE" == "dev" ]] && say "warning: this is a --dev install. Its skills are symlinks into .specify/extensions/gates/.specify-dev/, which do not exist in other clones; install from a release zip instead."

TABLE="$(gates_projection_table "$SRC" "$AGENT")"
targets="$(printf '%s\n' "$TABLE" | cut -f2)"
# Line by line: a path may contain spaces.
while IFS= read -r p; do
    [[ -n "$p" ]] || continue
    inlist "$targets" "$p" || refuse "$p is not a projected file"
done <<<"$(addline "$TAKE" "$KEEP")"

WRITES="" CONFLICTS="" HELD="" HELDGONE="" STALE="" KEPT="" RELEASED="" NEWMAN="" CHANGES=""
while IFS=$'\t' read -r s r; do
    [[ -n "$s" ]] || continue
    st="$(gates_classify "$ROOT" "$SRC/$s" "$r")" || refuse "cannot hash $r"
    # --keep-local is honored for any existing file, edited or not: the
    # maintainer asked for a hold.
    if inlist "$KEEP" "$r" && [[ "$st" != "held" ]]; then
        # A deletion is never held (#168): every projected file is run or
        # read by a hook, a gate, the canary suite or CI, and a missing
        # hook exits 127, which Claude Code treats as non-blocking. A file
        # emptied to 0 bytes turns the check off the same way (#203).
        [[ -s "$ROOT/$r" ]] \
            || refuse "--keep-local $r: there is no local file to keep (it is missing or empty). A deletion cannot be held, and an empty file is one:" \
                "every projected file is run by a hook, a gate, the canary suite or CI, and without it that check is off." \
                "Restore it with --take-upstream $r (or opt out of a whole boundary with --no-agent-hooks / --no-git-hooks)."
        st=edited
    fi
    # --take-upstream on a held file replaces it and releases the hold.
    if [[ "$st" == "held" ]] && inlist "$TAKE" "$r"; then
        RELEASED="$(addline "$RELEASED" "$r")"
        st=edited
    fi
    case "$st" in
        upstream) ;;
        absent | pristine) WRITES="$(addline "$WRITES" "$s"$'\t'"$r")" ;;
        held)
            HELD="$(addline "$HELD" "$r")"
            # A holds file written before #168 may hold a deletion, or a
            # file emptied after the hold was taken (#203).
            [[ -s "$ROOT/$r" ]] || HELDGONE="$(addline "$HELDGONE" "$r")"
            cmp -s "$SRC/$s" "$ROOT/$r" && STALE="$(addline "$STALE" "$r")"
            ;;
        edited)
            if inlist "$TAKE" "$r"; then
                WRITES="$(addline "$WRITES" "$s"$'\t'"$r")"
            elif inlist "$KEEP" "$r"; then
                KEPT="$(addline "$KEPT" "$r")"
            else
                CONFLICTS="$(addline "$CONFLICTS" "$r")"
            fi
            ;;
    esac
    case "$st" in
        held | edited)
            if [[ "$st" == "edited" ]] && inlist "$TAKE" "$r"; then
                NEWMAN="$(addline "$NEWMAN" "$(gates_sha256 "$SRC/$s")  $r")"
            else
                rec="$(gates_manifest_hash "$r")"
                [[ -n "$rec" ]] && NEWMAN="$(addline "$NEWMAN" "$rec  $r")"
            fi
            ;;
        *) NEWMAN="$(addline "$NEWMAN" "$(gates_sha256 "$SRC/$s")  $r")" ;;
    esac
done <<<"$TABLE"

if [[ -n "$CONFLICTS" ]]; then
    say "these projected files were changed locally since they were projected:" >&2
    printf '%s\n' "$CONFLICTS" | sed 's/^/project:   /' >&2
    say "nothing was written. Decide per file and re-run with:" >&2
    say "  --take-upstream <path>   replace it with $VERSION" >&2
    say "  --keep-local <path>      keep it; it is added to $GATES_HOLDS_REL and never overwritten" >&2
    exit 3
fi

# Planned side changes beyond file copies.
RTV_FILE="$ROOT/.specify/gates/.runtime-version"

# Policy settings added since the version that projected this project
# (#71): listed, never written -- policy.json belongs to the project. A
# schema property marked "x-since" newer than that version and absent from
# the policy is new to this project. A fresh install has no previous
# version and gets no notices.
PREV=""
if [[ "$GATES_MANIFEST_STATUS" == "ok" ]]; then
    PREV="$GATES_MANIFEST_VERSION"
else
    PREV="$(head -n 1 "$RTV_FILE" 2>/dev/null || true)"
fi
NEWKEYS=""
if [[ -n "$PREV" && -f "$SRC/policy.schema.json" ]]; then
    while IFS=$'\t' read -r key since dflt; do
        [[ -n "$key" ]] || continue
        [[ "$(gates_version_cmp "$since" "$PREV")" == "1" ]] || continue
        [[ "$(gates_version_cmp "$since" "$VERSION")" == "1" ]] && continue
        jq -e --arg k "$key" 'getpath($k | split(".")) != null' "$ROOT/.specify/gates/policy.json" >/dev/null 2>&1 \
            && continue
        NEWKEYS="$(addline "$NEWKEYS" "$key (since $since, default $dflt)")"
    done < <(jq -r 'paths(objects | has("x-since")) as $p
        | getpath($p) as $o
        | [($p | map(select(. != "properties")) | join(".")), $o["x-since"], ($o.default | tostring)]
        | @tsv' "$SRC/policy.schema.json" 2>/dev/null)
fi
NEED_RTV=0
[[ "$(head -n 1 "$RTV_FILE" 2>/dev/null || true)" == "$VERSION" ]] || NEED_RTV=1
GI="$ROOT/.specify/gates/.gitignore"
# The attestation log and the gate lock (#263) are run-time files, never
# source: one line each, added when missing.
NEED_GI=0
GI_ADD=""
for gi_line in attestations.jsonl .lock/; do
    grep -qxF "$gi_line" "$GI" 2>/dev/null && continue
    NEED_GI=1
    GI_ADD="$(addline "$GI_ADD" "$gi_line")"
done

SETTINGS="$ROOT/.claude/settings.json"
FRAG="$SRC/hooks/claude/settings.fragment.json"
MERGED=""
if [[ "$AGENT" -eq 1 ]]; then
    [[ -f "$FRAG" ]] || refuse "$FRAG is missing; reinstall the extension"
    cur='{}'
    if [[ -f "$SETTINGS" ]]; then
        jq -e . "$SETTINGS" >/dev/null 2>&1 || refuse "$SETTINGS is not valid JSON; fix it and re-run"
        cur="$(cat "$SETTINGS")"
    fi
    # Append-only, idempotent: add each fragment hook whose command path is
    # not already wired for that event; never remove or reorder user entries.
    MERGED="$(jq --slurpfile f "$FRAG" '
        reduce ($f[0].hooks | to_entries[]) as $e (.;
            reduce $e.value[] as $entry (.;
                ([(.hooks[$e.key] // [])[] | (.hooks // [])[] | .command]) as $have
                | ($entry | .hooks |= map(select(.command as $c | ($have | any(. == $c)) | not))) as $new
                | if ($new.hooks | length) > 0
                  then .hooks[$e.key] = ((.hooks[$e.key] // []) + [$new])
                  else . end))' <<<"$cur")" || refuse "cannot merge the agent hook settings into $SETTINGS"
    if [[ "$(jq -S . <<<"$cur")" == "$(jq -S . <<<"$MERGED")" ]]; then
        MERGED=""
    fi
fi

# Git boundary plan (#74): with no hook manager, the stub goes into the
# hooks directory git reads. A hook manager (husky, lefthook, the
# pre-commit framework) gets the gates entry in its own configuration --
# never in the files it generates -- and only with --wire-manager.
# Anything else that owns the hooks gets the call-through printed.
HOOKPLAN="" LEGACYPLAN="" FOREIGN="" GITNOTE="" NOGIT=0 GITREFUSED=0 MANAGER="" MGRPLAN="" MGRAPPLY="" MGRMANUAL="" MGRDONE=""
MGRINSTALL=""
STUB="$SRC/hooks/git/stub.sh"
if [[ "$GITHOOKS" -eq 1 ]]; then
    if ! command -v git >/dev/null 2>&1; then
        GITNOTE="git is not installed: the git boundary is not wired. Install git and run this again."
        NOGIT=1
    elif ! GITERR="$(git -C "$ROOT" rev-parse --is-inside-work-tree 2>&1 >/dev/null)"; then
        if grep -q 'dubious ownership' <<<"$GITERR"; then
            # A repository owned by another user (#203): git refuses it, so
            # it is no greenfield. Name the cause; the boundary is unwired.
            GITREPO="$(sed -n "s/.*dubious ownership in repository at '\(.*\)'.*/\1/p" <<<"$GITERR")"
            GITNOTE="git refuses this repository: dubious ownership (another user owns ${GITREPO:-$ROOT}), so the git boundary is not wired. If you trust it, run: git config --global --add safe.directory '${GITREPO:-$ROOT}', then run this again."
            GITREFUSED=1
        else
            GITNOTE="not a git work tree: the git boundary is not wired. Run this again after git init."
        fi
    else
        HOOKSDIR="$(cd "$ROOT" && git rev-parse --git-path hooks)"
        [[ "$HOOKSDIR" == /* ]] || HOOKSDIR="$ROOT/$HOOKSDIR"
        MANAGER="$(gates_detect_manager "$ROOT")"
        for n in pre-commit pre-merge-commit commit-msg; do
            f="$HOOKSDIR/$n"
            # `pre-commit install` moved the gates stub to <hook>.legacy and
            # runs it from there (#201): refresh an older copy (it refuses
            # under that name), keeping its mode. An executable one runs
            # gates, so the config entry is only added on --wire-manager.
            LEGACY=0
            if grep -qs 'File generated by pre-commit' "$f" && grep -qs 'spec-gates hook stub' "$f.legacy"; then
                cmp -s "$STUB" "$f.legacy" || LEGACYPLAN="$(addline "$LEGACYPLAN" "$n")"
                [[ -x "$f.legacy" ]] && LEGACY=1
            fi
            if [[ "$MANAGER" == "husky" || "$MANAGER" == "lefthook" || "$MANAGER" == "pre-commit" ]]; then
                if [[ "$LEGACY" -eq 1 && "$WIREMGR" -eq 0 ]]; then
                    :
                elif ! gates_manager_wired "$ROOT" "$MANAGER" "$n"; then
                    MGRPLAN="$(addline "$MGRPLAN" "$n")"
                    # --wire-manager appends only where that is safe (#128);
                    # the rest is printed for the user to add.
                    if [[ "$WIREMGR" -eq 1 ]]; then
                        if gates_manager_appendable "$ROOT" "$MANAGER" "$n"; then
                            MGRAPPLY="$(addline "$MGRAPPLY" "$n")"
                        else
                            MGRMANUAL="$(addline "$MGRMANUAL" "$n")"
                        fi
                    fi
                elif [[ ! -e "$f" ]]; then
                    # Wired, but the manager's install command has not
                    # generated the hook (#148): pending, in --check too
                    # (#203).
                    MGRINSTALL="$(addline "$MGRINSTALL" "$n")"
                fi
            elif [[ "$MANAGER" == "unknown" ]]; then
                if ! gates_calls_through "$f" "$n"; then
                    grep -qs 'spec-gates hook stub' "$f" || FOREIGN="$(addline "$FOREIGN" "$n")"
                fi
            elif [[ ! -e "$f" ]] || cmp -s "$STUB" "$f"; then
                { [[ -e "$f" ]] && [[ -x "$f" ]]; } || HOOKPLAN="$(addline "$HOOKPLAN" "$n")"
            elif grep -q 'spec-gates hook stub\|Git commit-msg hook\.\|Git pre-commit hook --' "$f"; then
                HOOKPLAN="$(addline "$HOOKPLAN" "$n")" # an older stub or a copied gates hook
            elif ! gates_calls_through "$f" "$n"; then
                FOREIGN="$(addline "$FOREIGN" "$n")"
            fi
        done
    fi
fi

# Lint scope (#73): a repo-wide prettier run reaches the vendored and
# projected files, which upgrades overwrite, so formatting them locally is
# lost work. Report the missing .prettierignore entries; add them only on
# --add-lint-ignores (the file is the project's).
LINT_MISSING=""
PIGN="$ROOT/.prettierignore"
uses_prettier() {
    [[ -f "$PIGN" ]] && return 0
    compgen -G "$ROOT/.prettierrc*" >/dev/null && return 0
    compgen -G "$ROOT/prettier.config.*" >/dev/null && return 0
    [[ -f "$ROOT/package.json" ]] && grep -q '"prettier"' "$ROOT/package.json" && return 0
    return 1
}
if uses_prettier; then
    # Without carriage returns, so a CRLF file matches line by line.
    PIGN_TEXT=""
    [[ -f "$PIGN" ]] && PIGN_TEXT="$(tr -d '\r' <"$PIGN")"
    for lp in .specify/gates/ .specify/extensions/ .claude/hooks/gates/; do
        # An entry for the path or any parent directory (.specify/,
        # .claude/**) excludes it too (#217).
        lcov=0 lpre=""
        if [[ -f "$PIGN" ]]; then
            IFS=/ read -r -a lparts <<<"${lp%/}"
            for lpart in "${lparts[@]}"; do
                lpre="${lpre:+$lpre/}$lpart"
                if grep -qxE "/?${lpre//./\\.}(/|/\*|/\*\*)?" <<<"$PIGN_TEXT"; then
                    lcov=1
                    break
                fi
            done
        fi
        [[ "$lcov" -eq 1 ]] || LINT_MISSING="$(addline "$LINT_MISSING" "$lp")"
    done
fi

NEED_MAN=0
if [[ "$GATES_MANIFEST_STATUS" != "ok" || "$GATES_MANIFEST_VERSION" != "$VERSION" ]] \
    || [[ "$(printf '%s\n' "$NEWMAN" | sort -k2)" != "$(printf '%s\n' "$GATES_MANIFEST_BODY" | sort -k2)" ]]; then
    NEED_MAN=1
fi

VEXEC=""
if [[ "$VENDORED" -eq 1 ]]; then
    # Spec Kit's zip extraction keeps the execute bit on *.sh only, so the
    # extension-less git hooks arrive 644 (FR-005a).
    for f in "$SRC"/*.sh "$SRC"/lib/*.sh "$SRC"/hooks/git/* "$SRC"/hooks/claude/*.sh; do
        [[ -f "$f" && ! -x "$f" ]] && VEXEC="$(addline "$VEXEC" "$f")"
    done
fi

[[ -n "$WRITES" ]] && CHANGES="$(addline "$CHANGES" "$(printf '%s\n' "$WRITES" | cut -f2 | sed 's/^/write /')")"
[[ -n "$KEPT" ]] && CHANGES="$(addline "$CHANGES" "$(printf '%s\n' "$KEPT" | sed 's/^/hold (keep local) /')")"
[[ -n "$RELEASED" ]] && CHANGES="$(addline "$CHANGES" "$(printf '%s\n' "$RELEASED" | sed 's/^/release the hold on /')")"
# Projected scripts that lost their execute bit, listed by name: git and
# Claude Code silently skip a hook that is not executable.
EXECFIX=""
while IFS=$'\t' read -r s r; do
    [[ -n "$r" && -f "$ROOT/$r" && ! -x "$ROOT/$r" ]] && gates_is_exec_target "$r" \
        && ! inlist "$(printf '%s\n' "$WRITES" | cut -f2)" "$r" && EXECFIX="$(addline "$EXECFIX" "$r")"
done <<<"$TABLE"
[[ -n "$EXECFIX" ]] && CHANGES="$(addline "$CHANGES" "$(printf '%s\n' "$EXECFIX" | sed 's/^/restore the execute bit on /')")"
if [[ "$NEED_RTV" -eq 1 ]]; then
    OLD_RTV="$(head -n 1 "$RTV_FILE" 2>/dev/null || true)"
    if [[ -n "$OLD_RTV" ]]; then
        CHANGES="$(addline "$CHANGES" "record runtime version $VERSION (.runtime-version said $OLD_RTV)")"
    else
        CHANGES="$(addline "$CHANGES" "record runtime version $VERSION")"
    fi
fi
[[ "$NEED_GI" -eq 1 ]] && CHANGES="$(addline "$CHANGES" "add $(printf '%s\n' "$GI_ADD" | paste -sd ' ' -) to .specify/gates/.gitignore")"
[[ -n "$MERGED" ]] && CHANGES="$(addline "$CHANGES" "merge agent hooks into .claude/settings.json")"
[[ -n "$HOOKPLAN" ]] && CHANGES="$(addline "$CHANGES" "$(printf '%s\n' "$HOOKPLAN" | sed 's/^/install the gates stub as git hook /')")"
[[ -n "$LEGACYPLAN" ]] && CHANGES="$(addline "$CHANGES" "$(printf '%s\n' "$LEGACYPLAN" | sed 's/.*/refresh the gates stub the pre-commit framework runs as &.legacy/')")"
[[ -n "$VEXEC" ]] && CHANGES="$(addline "$CHANGES" "restore execute bits on $(printf '%s\n' "$VEXEC" | wc -l | tr -d ' ') vendored file(s)")"
[[ "$NEED_MAN" -eq 1 ]] && CHANGES="$(addline "$CHANGES" "write $GATES_MANIFEST_REL")"
for n in $MGRAPPLY; do
    CHANGES="$(addline "$CHANGES" "add the gates $n entry to $(gates_manager_file "$ROOT" "$MANAGER" "$n") ($MANAGER)")"
done
[[ "$LINTIGN" -eq 1 && -n "$LINT_MISSING" ]] && CHANGES="$(addline "$CHANGES" "add $(printf '%s' "$LINT_MISSING" | tr '\n' ' ' | sed 's/ $//') to .prettierignore")"

report_side() {
    if [[ -n "$NEWKEYS" ]]; then
        say "new policy settings since $PREV (policy.json is yours, so nothing was changed):"
        printf '%s\n' "$NEWKEYS" | sed 's/^/project:   /'
        say "  adopt one in a reviewed change to .specify/gates/policy.json, or upstream with /speckit.gates.propose"
    fi
    if [[ -n "$HELD" ]]; then
        say "held (never overwritten, see $GATES_HOLDS_REL):"
        printf '%s\n' "$HELD" | sed 's/^/project:   /'
    fi
    if [[ -n "$HELDGONE" ]]; then
        say "FAILED: held files that do not exist or are empty (a deletion cannot be held; the hook, gate or canary that runs it is off):" >&2
        printf '%s\n' "$HELDGONE" | sed 's/^/project:   /' >&2
        say "  restore each with: bash .specify/extensions/gates/runtime/project.sh --take-upstream <path>" >&2
    fi
    # A stale hold keeps upgrades from updating the file; doctor fails on
    # it, so this run does too (#203).
    if [[ -n "$STALE" ]]; then
        say "FAILED: stale holds (the held file now equals $VERSION, so upgrades would never update it; remove the line from $GATES_HOLDS_REL):" >&2
        printf '%s\n' "$STALE" | sed 's/^/project:   /' >&2
    fi
    if [[ -n "$LINT_MISSING" && "$LINTIGN" -eq 0 ]]; then
        say "this repo runs prettier, and .prettierignore does not exclude (upgrades overwrite these, so local formatting is lost):"
        printf '%s\n' "$LINT_MISSING" | sed 's/^/project:   /'
        say "  re-run with --add-lint-ignores to append them"
    fi
    local missing
    missing="$(gates_ci_missing "$ROOT")"
    if [[ -n "$missing" ]]; then
        say "the CI pipeline ($(gates_ci_files "$ROOT" | tr '\n' ' ' | sed 's/ $//')) lacks these template steps:"
        printf '%s\n' "$missing" | sed 's/^/project:   /'
        say "  add them from .specify/extensions/gates/ci/, or record a deliberate omission as ci:<step> in $GATES_HOLDS_REL"
    fi
    # A ci: hold keeps a check out of CI: name what it gives up (#235). A
    # hold for a step that runs, or for no template step, is doctor's.
    local cid present held="" wantpr=0
    if gates_ci_files "$ROOT" >/dev/null; then
        present="$(gates_ci_present "$ROOT")"
        while IFS= read -r cid; do
            [[ -n "$cid" && -n "$(gates_ci_step_re "$cid")" ]] || continue
            [[ -n "$present" ]] && grep -qxF "$cid" <<<"$present" && continue
            held="$held $cid"
            say "[rec] CI step '$cid' held (ci:$cid in $GATES_HOLDS_REL): $(gates_ci_step_omitted "$cid")"
        done <<<"$(gates_holds_ci "$ROOT")"
    fi
    grep -qxF pr <<<"$missing" && wantpr=1
    case " $held " in *" pr "*) wantpr=1 ;; esac
    # The pr step adopted on its own, for each platform the gates run on;
    # printed only, never written into a pipeline file.
    if [[ "$wantpr" -eq 1 ]]; then
        local kinds="" f k
        while IFS= read -r f; do
            [[ -n "$f" ]] || continue
            k="$(_gates_ci_kind "$ROOT/$f")"
            case " $kinds " in *" $k "*) ;; *) kinds="$kinds $k" ;; esac
        done <<<"$(gates_ci_files "$ROOT")"
        for k in $kinds; do
            if gates_ci_pr_snippet "$k" >/dev/null; then
                say "  the pr step on its own ($k), without the rest of the template:"
                gates_ci_pr_snippet "$k" | sed 's/^/project:     /'
            else
                say "  the pr step on its own: see \"Adopting only the pr step\" in .specify/extensions/gates/commands/speckit.gates.ci.md"
            fi
        done
    fi
    # Callers written for 0.3.6 (#216): verify.sh still runs them, with a
    # deprecation warning.
    local unbounded
    unbounded="$(gates_verify_unbounded "$ROOT")"
    if [[ -n "$unbounded" ]]; then
        say "these callers run verify.sh without --boundary (deprecated: it warns and runs every gate; a later release may refuse it):"
        printf '%s\n' "$unbounded" | sed 's/^/project:   /'
        say "  add --boundary agent|git|ci to each"
    fi
    [[ -n "$GITNOTE" ]] && say "$GITNOTE"
    local n mf notrun="" stubrun=""
    if [[ "$WIREMGR" -eq 0 && -n "$MGRPLAN" ]]; then
        # Before the manager's install command replaces them, the gates
        # stubs still run (#203): name those apart, so the summary does
        # not claim a hook runs no gates when it does today.
        for n in $MGRPLAN; do
            if [[ "$(gates_hook_owner "$ROOT" "$n")" == "gates" ]]; then
                stubrun="$stubrun $n"
            else
                notrun="$notrun $n"
            fi
        done
        [[ -n "$notrun" ]] && say "$MANAGER owns the git hooks and does not run gates for:$notrun"
        [[ -n "$stubrun" ]] \
            && say "$(gates_manager_file "$ROOT" "$MANAGER" pre-commit) has no gates entry for:$stubrun (git still runs the gates stub for them, until $MANAGER's install command replaces it)"
        for n in $MGRPLAN; do
            mf="$(gates_manager_file "$ROOT" "$MANAGER" "$n")"
            if gates_manager_appendable "$ROOT" "$MANAGER" "$n"; then
                say "  $n: add to $mf (or re-run with --wire-manager to append it):"
            else
                say "  $n: $GATES_MANAGER_WHY"
            fi
            gates_manager_entry "$MANAGER" "$n" "$mf" | sed 's/^/project:     /'
        done
    fi
    if [[ -n "$MGRMANUAL" ]]; then
        for n in $MGRMANUAL; do
            mf="$(gates_manager_file "$ROOT" "$MANAGER" "$n")"
            if gates_manager_appendable "$ROOT" "$MANAGER" "$n" || [[ -z "$GATES_MANAGER_WHY" ]]; then
                GATES_MANAGER_WHY="$mf could not be written; add this by hand:"
            fi
            say "  $n: $GATES_MANAGER_WHY"
            gates_manager_entry "$MANAGER" "$n" "$mf" | sed 's/^/project:     /'
        done
    fi
    for n in $MGRDONE; do
        if [[ -x "$HOOKSDIR/$n.legacy" ]] && grep -qs 'spec-gates hook stub' "$HOOKSDIR/$n.legacy"; then
            say "  $n: pre-commit also runs the gates stub it moved to $n.legacy, so gates runs twice; \`pre-commit install -f --hook-type $n\` deletes it"
        fi
        [[ -e "$HOOKSDIR/$n" ]] && continue
        say "  $n: added to $(gates_manager_file "$ROOT" "$MANAGER" "$n"); now run \`$(gates_manager_install_hint "$MANAGER" "$n")\` so git runs $MANAGER for $n"
    done
    if [[ -n "$FOREIGN" ]]; then
        say "another tool owns these git hooks, so they were not touched:"
        for n in $FOREIGN; do
            say "  $n: add this line to it, before any exit, to run the gates hook:"
            say "    bash \"\$(git rev-parse --show-toplevel)/.specify/gates/hooks/$n\" \"\$@\" || exit \$?"
        done
    fi
    return 0
}

if [[ "$DRY" -eq 1 ]]; then
    say "spec-gates $VERSION, install state: $STATE"
    if [[ -z "$CHANGES" ]]; then
        say "no changes"
    else
        say "planned changes:"
        printf '%s\n' "$CHANGES" | sed 's/^/project:   /'
    fi
    report_side
    # The same pending state the full run reports in its proof (#203).
    for n in $MGRINSTALL; do
        say "pending: $(gates_manager_file "$ROOT" "$MANAGER" "$n") calls the gates $n hook, but git runs no $n hook until you run \`$(gates_manager_install_hint "$MANAGER" "$n")\`"
    done
    # An unwired git hook is pending work too, even when no file changes;
    # so is a git boundary left unwired because git is missing (#172) or
    # refuses the repository, a hook the manager has not generated yet
    # and a stale or emptied hold (#203).
    if [[ "$CHECK" -eq 1 ]] && [[ -n "$CHANGES" || -n "$FOREIGN" || -n "$MGRPLAN" || -n "$MGRINSTALL" \
        || -n "$HELDGONE" || -n "$STALE" || "$NOGIT" -eq 1 || "$GITREFUSED" -eq 1 ]]; then exit 1; fi
    exit 0
fi

# --- Write -------------------------------------------------------------------
fail_write() { say "write failed: $*" >&2; exit 1; }
while IFS=$'\t' read -r s r; do
    [[ -n "$s" ]] || continue
    mkdir -p "$(dirname "$ROOT/$r")" || fail_write "mkdir for $r"
    cp "$SRC/$s" "$ROOT/$r" || fail_write "$r"
done <<<"$WRITES"
# Execute bits on every projected script, not only the files written now:
# git and Claude Code silently skip a hook that is not executable.
while IFS=$'\t' read -r s r; do
    [[ -n "$r" && -f "$ROOT/$r" && ! -x "$ROOT/$r" ]] && gates_is_exec_target "$r" \
        && { chmod +x "$ROOT/$r" || fail_write "chmod $r"; }
done <<<"$TABLE"
if [[ -n "$VEXEC" ]]; then
    while IFS= read -r f; do chmod +x "$f" || fail_write "chmod $f"; done <<<"$VEXEC"
fi
[[ "$NEED_RTV" -eq 1 ]] && { printf '%s\n' "$VERSION" >"$RTV_FILE" || fail_write "$RTV_FILE"; }
if [[ "$NEED_GI" -eq 1 ]]; then
    # A last line without its newline would merge with the first added one.
    [[ -s "$GI" && -n "$(tail -c 1 "$GI")" ]] && { echo >>"$GI" || fail_write "$GI"; }
    printf '%s\n' "$GI_ADD" >>"$GI" || fail_write "$GI"
fi
if [[ -n "$MERGED" ]]; then
    mkdir -p "$ROOT/.claude" || fail_write ".claude/"
    { jq . <<<"$MERGED" >"$SETTINGS.tmp.$$" && mv -f "$SETTINGS.tmp.$$" "$SETTINGS"; } || fail_write "$SETTINGS"
fi
if [[ -n "$HOOKPLAN" ]]; then
    mkdir -p "$HOOKSDIR" || fail_write "$HOOKSDIR"
    for n in $HOOKPLAN; do
        { cp "$STUB" "$HOOKSDIR/$n" && chmod +x "$HOOKSDIR/$n"; } || fail_write "$HOOKSDIR/$n"
    done
fi
for n in $LEGACYPLAN; do
    # cp into the existing file keeps its mode: pre-commit runs it only
    # when it is executable, and that stays the user's choice.
    cp "$STUB" "$HOOKSDIR/$n.legacy" || fail_write "$HOOKSDIR/$n.legacy"
done
if [[ -n "$RELEASED" ]]; then
    # grep -v exits 1 when no line is left; that is an empty holds file.
    grep -vxF -f <(printf '%s\n' "$RELEASED") "$ROOT/$GATES_HOLDS_REL" >"$ROOT/$GATES_HOLDS_REL.tmp.$$"
    if [[ "$?" -gt 1 ]] || ! mv -f "$ROOT/$GATES_HOLDS_REL.tmp.$$" "$ROOT/$GATES_HOLDS_REL"; then
        fail_write "$GATES_HOLDS_REL"
    fi
fi
if [[ -n "$KEPT" ]]; then
    printf '%s\n' "$KEPT" >>"$ROOT/$GATES_HOLDS_REL" || fail_write "$GATES_HOLDS_REL"
fi
if [[ -n "$MGRAPPLY" ]]; then
    for n in $MGRAPPLY; do
        if gates_manager_apply "$ROOT" "$MANAGER" "$n"; then
            MGRDONE="$(addline "$MGRDONE" "$n")"
        else
            MGRMANUAL="$(addline "$MGRMANUAL" "$n")"
            CHANGES="$(grep -vxF "add the gates $n entry to $(gates_manager_file "$ROOT" "$MANAGER" "$n") ($MANAGER)" <<<"$CHANGES" || true)"
        fi
    done
fi
if [[ "$LINTIGN" -eq 1 && -n "$LINT_MISSING" ]]; then
    PIGN_NL=""
    [[ -s "$PIGN" && -n "$(tail -c 1 "$PIGN")" ]] && PIGN_NL=$'\n'
    {
        printf '%s' "$PIGN_NL"
        echo "# spec-gates: vendored and projected files; upgrades overwrite them."
        printf '%s\n' "$LINT_MISSING"
    } >>"$PIGN" || fail_write ".prettierignore"
fi
if [[ "$NEED_MAN" -eq 1 ]]; then
    printf '%s\n' "$NEWMAN" | grep -v '^$' | gates_manifest_write "$ROOT" "$VERSION" || fail_write "$GATES_MANIFEST_REL"
fi

if [[ -z "$CHANGES" ]]; then
    say "no changes (spec-gates $VERSION already projected)"
else
    say "projected spec-gates $VERSION:"
    printf '%s\n' "$CHANGES" | sed 's/^/project:   /'
fi
report_side

# --- Prove -------------------------------------------------------------------
RC=0
if [[ "$SKIPCANARY" -eq 0 ]]; then
    say "running the canary suite (every gate must block a planted violation):"
    if ! (cd "$ROOT" && CLAUDE_PROJECT_DIR="$ROOT" bash .specify/gates/canary.sh); then
        say "FAILED: a canary was accepted or could not run; see above. Run /speckit.gates.doctor." >&2
        RC=1
    fi
fi
# The git boundary (#74): a hook gates owns is run with GATES_PROBE=1 and
# must answer the marker; a hook another tool owns is read, not run (its
# own steps would run too), unless --probe-git asks for the full chain.
# Entries not (yet) in the manager's config leave the boundary unwired.
MGRPENDING="$MGRMANUAL"
[[ "$WIREMGR" -eq 0 ]] && MGRPENDING="$MGRPLAN"
if [[ "$GITHOOKS" -eq 1 && -z "$GITNOTE" && -z "$FOREIGN" && -z "$MGRPENDING" ]]; then
    for n in pre-commit pre-merge-commit commit-msg; do
        # The manager's config calls gates, but git runs no hook yet: the
        # manager generates it on its install command (#148). That is the
        # maintainer's next step, not a broken chain; doctor flags it until
        # the hook exists.
        if [[ "$MANAGER" == "husky" || "$MANAGER" == "lefthook" || "$MANAGER" == "pre-commit" ]] \
            && [[ ! -e "$HOOKSDIR/$n" ]] && gates_manager_wired "$ROOT" "$MANAGER" "$n"; then
            say "pending: $(gates_manager_file "$ROOT" "$MANAGER" "$n") calls the gates $n hook, but git runs no $n hook until you run \`$(gates_manager_install_hint "$MANAGER" "$n")\`; then check with /speckit.gates.doctor"
            RC=1
            continue
        fi
        if gates_git_check "$ROOT" "$n" "$PROBEGIT"; then
            if [[ "$GATES_CHECK_KIND" == "probe" ]]; then
                say "git probe: $n reaches the gates hook, and its refusal reaches git"
            else
                say "git check (static): another tool owns $n and calls the gates hook"
            fi
        else
            say "FAILED: git $GATES_CHECK_KIND: $GATES_PROBE_MSG" >&2
            RC=1
        fi
    done
fi
[[ -n "$FOREIGN" || -n "$MGRPENDING" || -n "$HELDGONE" || -n "$STALE" || "$GITREFUSED" -eq 1 ]] && RC=1
exit "$RC"
