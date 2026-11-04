#!/usr/bin/env bash
# message.sh -- the commit / PR message rules, shared by every boundary.
#
#   gates_message_check <commit|pr> <text> [generated]
#
# commit-msg (git boundary), validate-pr.sh (agent boundary) and pr-check.sh
# (CI boundary) all call this, so a message refused at one boundary is
# refused at all of them (issue #56). Prints "ERROR: ..." / "WARN: ..." lines
# on stderr and sets GATES_MSG_ERRORS / GATES_MSG_WARNINGS. Returns 1 when
# there is at least one error.
#
# Emoji are checked in the whole text, subject and body (#190); commit-msg
# drops comment lines and the scissors section before it calls this.
# Modes differ only where the text differs:
#   commit -- body lines > 100 chars warn.
#   pr     -- <text> is "title\n\nbody" (a PR body is free-form markdown,
#             so no line-length rule).
# "generated" marks a subject git wrote itself (a merge, fixup!/squash!/
# amend!): the conventional-format rule is skipped, every other rule runs.
#
# Policy (via lib/policy.sh when sourced first; defaults otherwise):
#   git.forbid_ai_isms, git.conventional_commits, git.ai_branding.

# shellcheck disable=SC2034   # GATES_MSG_* are consumed by callers

_gates_msg_policy_enabled() { # <git-field>: on unless the policy says false
    command -v gates_policy_section_get >/dev/null 2>&1 || return 0
    [[ "$(gates_policy_section_get git "$1")" == "false" ]] && return 1
    return 0
}

# Emoji detection. Returns 0 = emoji found, 1 = none, 2 = cannot check.
# python3 first, perl as the fallback (perl ships with macOS and with the
# Debian base that slim images use). Neither usable means the rule cannot
# run, and the caller fails closed instead of passing silently (#66).
_GATES_EMOJI_RANGES='\x{1F300}-\x{1F9FF}\x{2600}-\x{27BF}\x{FE00}-\x{FE0F}\x{200D}\x{2702}-\x{27B0}\x{1FA00}-\x{1FA6F}\x{1FA70}-\x{1FAFF}'

_gates_msg_has_emoji() { # <text>
    local rc
    if python3 -c 'import re, sys' >/dev/null 2>&1; then
        rc=0
        python3 - "$1" <<'PYEOF' 2>/dev/null || rc=$?
import re, sys

text = sys.argv[1] if len(sys.argv) > 1 else ""
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
sys.exit(0 if emoji_pattern.search(text) else 1)
PYEOF
        [[ "$rc" -le 1 ]] && return "$rc"
    fi
    if perl -e 1 >/dev/null 2>&1; then
        rc=0
        printf '%s' "$1" | perl -CS -0777 -ne "exit(/[$_GATES_EMOJI_RANGES]/ ? 0 : 1)" 2>/dev/null || rc=$?
        [[ "$rc" -le 1 ]] && return "$rc"
    fi
    return 2
}

gates_message_check() { # <commit|pr> <text> [generated]
    local mode="$1" msg="$2" generated="${3:-}" subject
    GATES_MSG_ERRORS=0
    GATES_MSG_WARNINGS=0
    subject="${msg%%$'\n'*}"

    _err() { echo "ERROR: $*" >&2; GATES_MSG_ERRORS=$((GATES_MSG_ERRORS + 1)); }
    _warn() { echo "WARN: $*" >&2; GATES_MSG_WARNINGS=$((GATES_MSG_WARNINGS + 1)); }

    if ! [[ "$msg" =~ [^[:space:]] ]]; then
        _err "Empty message."
        return 1
    fi

    if [[ ${#subject} -gt 72 ]]; then
        _warn "Subject line exceeds 72 characters (${#subject})."
    fi

    local emoji_rc=0
    _gates_msg_has_emoji "$msg" || emoji_rc=$?
    if [[ "$emoji_rc" -eq 0 ]]; then
        _err "Emoji detected in the message."
    elif [[ "$emoji_rc" -eq 2 ]]; then
        _err "Cannot check for emoji: neither python3 nor perl is usable (install one; see /speckit.gates.doctor)."
    fi

    if _gates_msg_policy_enabled forbid_ai_isms; then
        if grep -qiE '\b(I have|I'\''ve|I updated|I fixed|I added|I removed|I refactored)\b' <<<"$msg"; then
            _err "Self-referential language detected."
        fi
        if grep -qiE '\b(Certainly|I'\''d be happy to|As an AI|Happy to help)\b' <<<"$msg"; then
            _err "AI filler language detected."
        fi
        if grep -qiE '\b(seamless|robust|powerful|elegant|streamlined|polished|enhanced|refined)\b' <<<"$msg"; then
            _err "Marketing adjective detected."
        fi

        # AI branding (policy: git.ai_branding). Allow phrases are literal
        # text removed before both branding checks, ignoring case like the
        # terms (#130); terms match as literal whole words, case-insensitive.
        # Absent keys keep the built-in list; terms: [] disables it.
        local brand_msg="$msg" phrase term terms hits=""
        local -a brand_terms=()
        if command -v gates_policy_path_list >/dev/null 2>&1; then
            while IFS= read -r phrase; do
                # awk, not ${brand_msg//"$phrase"/}: bash 3.2 makes that
                # quadratic in the number of matches (#117).
                [[ -n "$phrase" ]] && brand_msg="$(awk -v p="$phrase" '{
                    out = ""; lp = tolower(p)
                    while ((i = index(tolower($0), lp)) > 0) { out = out substr($0, 1, i - 1); $0 = substr($0, i + length(p)) }
                    print out $0 }' <<<"$brand_msg")"
            done < <(gates_policy_path_list git ai_branding allow_phrases || true)
        fi
        if command -v gates_policy_path_list >/dev/null 2>&1 \
            && terms="$(gates_policy_path_list git ai_branding terms)"; then
            while IFS= read -r term; do
                [[ -n "$term" ]] && brand_terms+=("$term")
            done <<<"$terms"
        else
            brand_terms=(Anthropic GPT OpenAI Copilot)
        fi
        for term in ${brand_terms[@]+"${brand_terms[@]}"}; do
            if grep -qiwF -e "$term" <<<"$brand_msg"; then
                hits="${hits:+$hits, }$term"
            fi
        done
        if [[ -n "$hits" ]]; then
            _err "AI branding detected ($hits)."
            echo "  A legitimate phrase can be allowed via git.ai_branding.allow_phrases." >&2
        fi

        # Standalone "Claude". Strip legitimate references first: the product
        # name ("Claude Code"), the memory file (CLAUDE.md, any casing),
        # .claude/ paths, claude-* kebab identifiers, and parenthesized
        # scopes. Portable sed only (no GNU case-insensitive flag).
        local cleaned
        cleaned="$(printf '%s\n' "$brand_msg" \
            | sed 's/Claude Code//g' \
            | sed 's/[Cc][Ll][Aa][Uu][Dd][Ee]\.md//g' \
            | sed 's#\.claude/[^[:space:]]*##g' \
            | sed 's/[Cc]laude-[A-Za-z0-9._-]*//g' \
            | sed 's/([^)]*)//g')"
        if grep -qiE '\bClaude\b' <<<"$cleaned"; then
            _err "Standalone 'Claude' detected (use 'Claude Code' if needed)."
            echo "  A legitimate phrase (a product or model name your repo integrates) can be allowed via git.ai_branding.allow_phrases." >&2
        fi
    fi

    # A PR edit that changes only the body has no title to judge.
    if _gates_msg_policy_enabled conventional_commits \
        && [[ "$generated" != "generated" ]] \
        && ! [[ "$mode" == "pr" && -z "$subject" ]]; then
        if ! grep -qE '^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?: .+' <<<"$subject"; then
            _err "Subject does not match conventional commit format."
            echo "  Expected: type(scope)?: description" >&2
            echo "  Types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert" >&2
        fi
    fi

    if grep -qiE '\b(WIP|FIXME|TODO|XXX|DO NOT MERGE)\b' <<<"$msg"; then
        _warn "Draft marker detected."
    fi

    if grep -qi 'Co-Authored-By:' <<<"$msg"; then
        _err "Co-Authored-By trailer detected."
    fi
    # The PR-side counterpart: Claude Code's default PR footer, plain or as
    # a markdown link, with or without its emoji (#140), and its common
    # rewordings ("Made with", "Generated by", ...) under any spacing or
    # line break (#170). tr, not ${msg//...}: bash 3.2 makes that quadratic.
    if [[ "$mode" == "pr" ]] \
        && grep -qiE '(generated|made|created|built|written)[[:space:]]+(with|by)[[:space:]]+\[?claude[[:space:]]+code([^[:alnum:]]|$)' \
            <<<"$(tr -s '[:space:]' ' ' <<<"$msg")"; then
        _err "Agent attribution line detected (Generated with Claude Code)."
    fi

    if [[ "$mode" == "commit" ]] && [[ $(printf '%s\n' "$msg" | wc -l) -gt 1 ]]; then
        local long
        long="$(printf '%s\n' "$msg" | tail -n +3 | awk 'length > 100 { count++ } END { print count+0 }')"
        if [[ "$long" -gt 0 ]]; then
            _warn "$long body line(s) exceed 100 characters."
        fi
    fi

    unset -f _err _warn
    [[ "$GATES_MSG_ERRORS" -eq 0 ]]
}
