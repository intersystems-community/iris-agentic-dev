#!/bin/bash
set -euo pipefail

# PreToolUse hook for Write/Edit.
# Reads JSON from stdin, parses file_path, blocks modification of sensitive files.
# Exit 2 = block. Exit 0 = allow, or (with the JSON below on stdout) ask.
#
# Never a silent allow (issue #83): without jq, or when the input is not
# valid JSON, file_path is read in raw mode (see raw_field) and the built-in
# name rules still block. What the hook cannot evaluate -- policy
# protected_files.extra without jq, a policy library that fails to load, an
# internal error -- returns a PreToolUse "ask" decision for a human.

# ask <reason>: hand the decision to the human (PreToolUse "ask"). Static
# printf, no jq: this must work in exactly the states where jq is missing.
ask() {
    local r="${1//\\/\\\\}"
    r="${r//\"/\\\"}"
    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"%s"}}\n' "gates: $r"
    exit 0
}

trap 'ask "protect-files.sh failed unexpectedly (line $LINENO); run /speckit.gates.doctor"' ERR

# raw_field <name>: print the decoded value of the JSON string field <name>
# from $INPUT without jq. A JSON string is a regular language, so the sed
# match is exact for it; escapes are decoded below. Returns 1 when the
# field is absent and 2 when the value uses an escape this decoder does not
# handle (\uXXXX could spell a blocked word) or the key appears more than
# once (which one Claude Code acts on would be a guess, #148), which the
# caller turns into "ask".
raw_field() {
    local v n
    n="$({ grep -oE '"'"$1"'"[[:space:]]*:' <<<"$INPUT" || true; } | awk 'END { print NR }')"
    [[ "${n:-0}" -gt 1 ]] && return 2
    # The leading "=" tells an empty value ("") apart from no match.
    v="$(printf '%s' "$INPUT" | tr '\n' ' ' \
        | sed -nE 's/.*"'"$1"'"[[:space:]]*:[[:space:]]*"(([^"\\]|\\.)*)".*/=\1/p')"
    if [[ -z "$v" ]]; then
        # The key is there but its value is not a plain string: undecidable.
        grep -qE '"'"$1"'"[[:space:]]*:' <<<"$INPUT" && return 2
        return 1
    fi
    v="${v#=}"
    [[ "$v" == *'\u'* ]] && return 2
    # bash 3.2's ${v//...} is quadratic in the number of matches: a long
    # value would hang the decode below, so it is undecidable here (#117).
    [[ "${#v}" -gt 16384 ]] && return 2
    v="${v//\\\\/$'\001'}"
    v="${v//\\\"/\"}"
    v="${v//\\\//\/}"
    v="${v//\\n/$'\n'}"
    v="${v//\\t/$'\t'}"
    v="${v//\\r/}"
    v="${v//$'\001'/\\}"
    printf '%s' "$v"
}

# The rules below test with grep/sed: without them each test is silently
# false, which would allow every edit.
for _tool in grep sed tr basename awk; do
    command -v "$_tool" >/dev/null 2>&1 \
        || ask "$_tool not found, so protect-files cannot check this edit; run /speckit.gates.doctor"
done
# The text tools read the path as bytes (#238): in a UTF-8 locale a byte
# sequence that is not UTF-8 stops BSD tr and sed ("Illegal byte
# sequence") and aborts BWK awk. bash itself keeps the caller's locale.
awk() { LC_ALL=C command awk "$@"; }
sed() { LC_ALL=C command sed "$@"; }
tr() { LC_ALL=C command tr "$@"; }
grep() { LC_ALL=C command grep "$@"; }

INPUT=$(cat /dev/stdin)
DEGRADED=""
if command -v jq >/dev/null 2>&1 && printf '%s' "$INPUT" | jq -e . >/dev/null 2>&1; then
    FILE_PATH=$(printf '%s' "$INPUT" | jq -r '.tool_input.file_path // empty')
else
    if command -v jq >/dev/null 2>&1; then
        DEGRADED="the hook input is not valid JSON"
    else
        DEGRADED="jq not found"
    fi
    rc=0
    FILE_PATH="$(raw_field file_path)" || rc=$?
    [[ "$rc" -eq 2 ]] && ask "cannot read the file path without jq ($DEGRADED); confirm this edit"
    # No file_path at all: nothing here says which file the edit touches.
    [[ "$rc" -eq 1 ]] && ask "no file path found in the hook input without jq ($DEGRADED); confirm this edit"
fi

if [[ -z "$FILE_PATH" ]]; then
    exit 0
fi

# Normalize before any rule (#131): `.`, `..` and `//` are resolved
# lexically, so .specify/gates/lib/../policy.json is policy.json. Every
# match below ignores case: macOS APFS is case-insensitive by default, so
# POLICY.json is the same file there, and a false refusal is cheap.
ORIG_PATH="$FILE_PATH"
FILE_PATH="$(printf '%s\n' "$FILE_PATH" | awk '{
    abs = (substr($0, 1, 1) == "/"); n = split($0, c, "/"); k = 0
    for (i = 1; i <= n; i++) {
        if (c[i] == "" || c[i] == ".") continue
        if (c[i] == "..") {
            if (k > 0 && s[k] != "..") { k--; continue }
            if (abs) continue
        }
        s[++k] = c[i]
    }
    out = ""
    for (i = 1; i <= k; i++) out = out (i > 1 ? "/" : "") s[i]
    print (abs ? "/" : "") out }')"
[[ -n "$FILE_PATH" ]] || FILE_PATH="$ORIG_PATH"

# real_path <absolute path>: the path with every symlink resolved, in the
# file and in each parent, the way the kernel walks it (#193): a component
# that is a link is replaced by its target and the walk goes on from there,
# so `..` after a link leaves the link's target. Components that do not
# exist yet are kept as written. Returns 1 on a link loop or a link it
# cannot read, which the caller turns into "ask".
real_path() {
    local todo="$1" out="" comp link hops=0
    while [[ -n "$todo" ]]; do
        while [[ "$todo" == /* ]]; do todo="${todo#/}"; done
        [[ -n "$todo" ]] || break
        if [[ "$todo" == */* ]]; then
            comp="${todo%%/*}"
            todo="${todo#*/}"
        else
            comp="$todo"
            todo=""
        fi
        case "$comp" in
            . | '') continue ;;
            ..) out="${out%/*}"; continue ;;
        esac
        if [[ -L "$out/$comp" ]]; then
            hops=$((hops + 1))
            [[ "$hops" -le 40 ]] || return 1
            command -v readlink >/dev/null 2>&1 || return 1
            link="$(readlink "$out/$comp")" || return 1
            [[ -n "$link" ]] || return 1
            [[ "$link" == /* ]] && out=""
            todo="$link${todo:+/$todo}"
        else
            out="$out/$comp"
        fi
    done
    printf '%s' "${out:-/}"
}

# Every rule below also judges the fully resolved real path (#193): a
# symlink inside the project (gdir -> .specify/gates, pol.json ->
# policy.json) must not turn a protected file into an editable one. A
# relative path is taken from the session's cwd.
if [[ -z "$DEGRADED" ]]; then
    _cwd="$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null)" || _cwd=""
else
    _cwd="$(raw_field cwd)" || _cwd=""
fi
_abs="$ORIG_PATH"
[[ "$_abs" == /* ]] || _abs="${_cwd:-$PWD}/$_abs"
REAL_PATH="$(real_path "$_abs")" \
    || ask "cannot resolve the symlinks in $ORIG_PATH (a loop or an unreadable link); confirm this edit"
REAL_BASE="${REAL_PATH##*/}"

shopt -s nocasematch

BASENAME=$(basename "$FILE_PATH")

BLOCKED=""
NAMEASK=""
CONSTASK=""

# Allowlist: .example / .sample / .template files are safe to edit even when
# the base name looks sensitive (e.g. .env.example). Mirrors the git
# pre-commit forbidden-file allowlist so the two boundaries agree. Both
# names must qualify: x.sample linked to policy.json is policy.json.
allowlisted() { [[ "$1" == *.example || "$1" == *.sample || "$1" == *.template ]]; }
if allowlisted "$BASENAME" && allowlisted "$REAL_BASE"; then
    exit 0
fi

# builtin_rules <path>: the shipped rules for one spelling of the target,
# setting BLOCKED, NAMEASK and CONSTASK. Run for the path as given
# (normalized) and for its real path.
builtin_rules() {
    local FILE_PATH="$1" BASENAME
    BASENAME="${1##*/}"
    allowlisted "$BASENAME" && return 0

    # Secret files: environment files, SSH keys, certificates and key
    # stores, credential files, cloud credentials and sensitive directories,
    # from the list pre-commit and pr-check use (lib/secrets.sh, #221).
    if [[ "$SECRETS_LIB" == ok ]] && gates_forbidden_path "$FILE_PATH"; then
        BLOCKED="Secret file: ${GATES_FORBIDDEN_WHAT:-a forbidden name} (lib/secrets.sh)"
    elif [[ "$SECRETS_LIB" != ok ]] && gates_core_secret "$FILE_PATH"; then
        BLOCKED="Secret file (the built-in core list; lib/secrets.sh cannot load)"
    fi

    # Credentials (#71): an exact credential file name is strong evidence and
    # blocks (above). A sensitive word that merely appears in the name (a
    # test such as test_no_secret_leak.py, a token parser) asks the human
    # instead: blocking it outright left no way to edit such files at all.
    if [[ -z "$BLOCKED" && -z "$NAMEASK" ]]; then
        _word="$(echo "$BASENAME" | grep -oiE 'credentials|secret|password|token|keystore' | head -n 1 || true)"
        [[ -n "$_word" ]] && NAMEASK="the file name contains '$_word'; confirm $FILE_PATH does not hold a credential"
    fi

    # The project's own rules (#95): hooks.local.d holds the refusals the
    # project added on top of the shipped ones. The agent must not be able to
    # rewrite or delete them; a human changes them, and a commit that does
    # needs a Protected-Change trailer.
    case "$FILE_PATH" in
        .specify/gates/hooks.local.d/* | */.specify/gates/hooks.local.d/*)
            BLOCKED="Project rule in .specify/gates/hooks.local.d/ (a human edits these; the commit needs a Protected-Change trailer)"
            ;;
    esac

    # The policy-contract artifacts (#137) decide what is enforced, like
    # policy.json. Only contract.sh sync writes them; a hand edit, even a
    # consistent one the contract gate cannot tell from a sync, is refused.
    case "$FILE_PATH" in
        .specify/gates/baseline.json | */.specify/gates/baseline.json \
            | .specify/gates/baseline.lock.json | */.specify/gates/baseline.lock.json \
            | .specify/gates/policy.effective.json | */.specify/gates/policy.effective.json)
            BLOCKED="Policy-contract artifact (written by contract.sh sync, never by hand; the commit needs a Protected-Change trailer)"
            ;;
    esac

    # The policy is protected whatever protected_files.extra says and whether
    # or not jq is present (#165): an agent that could rewrite it could switch
    # off every other rule, and it is exactly what an invalid or unread policy
    # cannot vouch for.
    case "$FILE_PATH" in
        .specify/gates/policy.json | */.specify/gates/policy.json)
            BLOCKED="Gates policy (a human edits it; the commit needs a Protected-Change trailer)"
            ;;
    esac

    # The constitution asks instead (#200): /speckit-constitution and
    # /speckit.gates.constitution write it as their own step, so a refusal
    # would push the agent to Bash. Every Write/Edit to it, under any policy
    # (an extra entry naming it included), needs one human approval.
    case "$FILE_PATH" in
        .specify/memory/constitution.md | */.specify/memory/constitution.md)
            CONSTASK="this edits the project constitution; confirm the change (committing it needs a Protected-Change trailer when protected_files.extra lists it)"
            ;;
    esac

    # Lock files
    case "$BASENAME" in
        package-lock.json|yarn.lock|pnpm-lock.yaml|Cargo.lock|poetry.lock)
            BLOCKED="Lock file (auto-generated)"
            ;;
    esac
    return 0
}

# The secret-file list (#221): the project's lib, the one this hook was
# projected beside (.claude/hooks/gates -> .specify/gates/lib, also when
# CLAUDE_PROJECT_DIR points elsewhere), or the one in the extension's own
# layout. Without one the core list still blocks and the rest asks below,
# once no other rule has blocked.
# gates_core_secret <path>: the core of the secret-file list, built in,
# for when lib/secrets.sh cannot load: environment files, private keys,
# certificates and key stores, as 0.4.0 blocked them. The rest of the list
# then asks.
gates_core_secret() {
    case "${1##*/}" in
        *.example | *.sample | *.template) return 1 ;;
        .env | .env.* | id_rsa* | id_ed25519* | id_ecdsa* | *.pem | *.key | *.crt | *.p12 | *.pfx | *.jks | *.keystore)
            return 0
            ;;
    esac
    return 1
}
PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
SECRETS_LIB=missing
for _sl in "$PROJECT_ROOT/.specify/gates/lib/secrets.sh" "${BASH_SOURCE[0]%/*}/../../../.specify/gates/lib/secrets.sh" \
    "${BASH_SOURCE[0]%/*}/../../lib/secrets.sh"; do
    [[ -f "$_sl" ]] || continue
    # shellcheck source=/dev/null disable=SC1090
    if "$BASH" -n "$_sl" 2>/dev/null && source "$_sl" 2>/dev/null \
        && command -v gates_forbidden_path >/dev/null 2>&1; then
        SECRETS_LIB=ok
    fi
    break
done
builtin_rules "$FILE_PATH"
[[ -n "$BLOCKED" || "$REAL_PATH" == "$FILE_PATH" ]] || builtin_rules "$REAL_PATH"

# A hard link shares no path with the file it aliases: compare an existing
# target with the project's own protected files by device and inode.
if [[ -z "$BLOCKED" && -z "$CONSTASK" && -f "$REAL_PATH" ]]; then
    for _pf in "$PROJECT_ROOT"/.specify/gates/policy.json "$PROJECT_ROOT"/.specify/gates/baseline.json \
        "$PROJECT_ROOT"/.specify/gates/baseline.lock.json "$PROJECT_ROOT"/.specify/gates/policy.effective.json \
        "$PROJECT_ROOT"/.specify/gates/hooks.local.d/*/*; do
        if [[ -f "$_pf" && "$REAL_PATH" -ef "$_pf" ]]; then
            BLOCKED="Link to the protected file ${_pf#"$PROJECT_ROOT"/}"
            break
        fi
    done
    _pf="$PROJECT_ROOT/.specify/memory/constitution.md"
    if [[ -z "$BLOCKED" && -f "$_pf" && "$REAL_PATH" -ef "$_pf" ]]; then
        CONSTASK="this edits the project constitution through a link; confirm the change"
    fi
fi

# raw_extra <policy>: protected_files.extra without jq, one entry per line,
# the same reader as validate-bash's: a flat array of plain strings, on one
# line or many (#211). Returns 2 when the policy declares an extra this
# cannot read (escapes, values that are not strings, another layout).
raw_extra() {
    local flat body
    flat="$(tr '\n' ' ' <"$1")"
    grep -qE '"extra"[[:space:]]*:' <<<"$flat" || return 0
    body="$(sed -nE 's/.*"protected_files"[[:space:]]*:[[:space:]]*\{[^{}]*"extra"[[:space:]]*:[[:space:]]*\[([^]]*)\].*/=\1/p' <<<"$flat")"
    [[ -n "$body" ]] || return 2
    body="${body#=}"
    grep -qE '^[[:space:]]*("[^"\\]*"[[:space:]]*(,[[:space:]]*"[^"\\]*"[[:space:]]*)*)?$' <<<"$body" || return 2
    { grep -oE '"[^"\\]*"' <<<"$body" || true; } | sed -e 's/^"//' -e 's/"$//'
}

# Policy-declared extra protected paths (protected_files.extra). Matched against
# both the project-relative path and the basename so exact entries and globs
# (e.g. ".specify/memory/constitution.md", "docs/**") both work.
ASK=""
if [[ -z "$BLOCKED" && -z "$CONSTASK" ]]; then
    POLICY_LIB="$PROJECT_ROOT/.specify/gates/lib/policy.sh"
    POLICY_FILE="$PROJECT_ROOT/.specify/gates/policy.json"
    EXTRA=""
    if [[ -n "$DEGRADED" ]]; then
        # Without jq the entries come from raw_extra, matched with the
        # library's glob matcher (plain bash). What it cannot read asks.
        if [[ -f "$POLICY_FILE" ]]; then
            rc=0
            EXTRA="$(raw_extra "$POLICY_FILE")" || rc=$?
            # shellcheck source=/dev/null disable=SC1090
            if [[ "$rc" -ne 0 ]]; then
                ASK="policy protected_files.extra cannot be read ($DEGRADED); confirm $FILE_PATH is not protected"
            elif [[ -n "$EXTRA" ]] && { [[ ! -f "$POLICY_LIB" ]] || ! "$BASH" -n "$POLICY_LIB" 2>/dev/null \
                || ! source "$POLICY_LIB" 2>/dev/null || ! command -v gates_glob_match >/dev/null 2>&1; }; then
                ASK="policy protected_files.extra cannot be checked: the gates policy library failed to load; confirm $FILE_PATH is not protected"
            fi
        fi
    elif [[ -f "$POLICY_LIB" ]]; then
        # shellcheck source=/dev/null disable=SC1091
        # A syntax error in a sourced file aborts the whole hook, so check it
        # first (bash -n parses without running).
        if ! "$BASH" -n "$POLICY_LIB" 2>/dev/null \
            || ! source "$POLICY_LIB" 2>/dev/null \
            || ! command -v gates_policy_section_list >/dev/null 2>&1; then
            ask "the gates policy library failed to load, so protected_files.extra cannot be checked; run /speckit.gates.doctor"
        fi
        # The reader returns no entries for an unparseable or malformed
        # policy, which would read as "nothing protected". Same validation
        # verify.sh refuses on (#124).
        _pf="$(gates_policy_file)"
        if [[ -f "$_pf" ]] && ! gates_validate_policy "$_pf" >/dev/null 2>&1; then
            ask "$_pf is invalid (verify.sh refuses it), so protected_files.extra cannot be checked; run /speckit.gates.doctor"
        fi
        EXTRA="$(gates_policy_section_list protected_files extra)"
    fi
    if [[ -n "$EXTRA" && -z "$ASK" ]]; then
        REL="$FILE_PATH"
        # A case-insensitive match (nocasematch), so the cut is by length.
        [[ "$FILE_PATH" == "$PROJECT_ROOT/"* ]] && REL="${FILE_PATH:$((${#PROJECT_ROOT} + 1))}"
        # The same file under another spelling of the root (#165): /tmp and
        # /private/tmp on macOS, a symlinked checkout, ../proj/x, or a link
        # inside the project (#193). Compare the real project root with the
        # target's fully resolved real path.
        REAL_REL=""
        REAL_ROOT="$(cd "$PROJECT_ROOT" 2>/dev/null && pwd -P)" || REAL_ROOT=""
        if [[ -n "$REAL_ROOT" && "$REAL_PATH" == "$REAL_ROOT/"* ]]; then
            REAL_REL="${REAL_PATH:$((${#REAL_ROOT} + 1))}"
        fi
        while IFS= read -r entry; do
            [[ -z "$entry" ]] && continue
            if gates_glob_match "$REL" "$entry" \
                || { [[ -n "$REAL_REL" ]] && gates_glob_match "$REAL_REL" "$entry"; } \
                || gates_glob_match "$FILE_PATH" "$entry" \
                || [[ "$BASENAME" == "$entry" || "$REAL_BASE" == "$entry" ]]; then
                BLOCKED="Protected by policy (protected_files.extra: $entry)"
                break
            fi
        done <<<"$EXTRA"
    fi
fi

# raw_cover <policy>: spec.cover.script without jq (feature 007), the same
# kind of reader as raw_extra: a plain string inside a flat cover object.
# Returns 2 when the policy names a cover this cannot read (escapes, a
# layout other than that, or another "cover" key after it), which asks.
raw_cover() {
    local flat v
    flat="$(tr '\n' ' ' <"$1")"
    grep -qE '"cover"[[:space:]]*:' <<<"$flat" || return 0
    v="$(sed -nE 's/.*"cover"[[:space:]]*:[[:space:]]*\{[^{}]*"script"[[:space:]]*:[[:space:]]*"([^"\\]*)".*/=\1/p' <<<"$flat")"
    [[ -n "$v" ]] || return 2
    printf '%s\n' "${v#=}"
}

# The covering script (spec.cover.script, feature 007) decides which accept
# blocks are skipped, so it is protected like the policy whatever
# protected_files.extra says. Every edit asks, as for the constitution
# (#200): writing it is a legitimate step a human confirms, and a refusal
# would push the agent to Bash. Without jq the raw reader decides; what it
# cannot read asks.
COVERASK=""
if [[ -z "$BLOCKED" && -z "$CONSTASK" ]]; then
    COVER=""
    POLICY_FILE="$PROJECT_ROOT/.specify/gates/policy.json"
    if [[ -n "$DEGRADED" ]]; then
        if [[ -f "$POLICY_FILE" ]]; then
            rc=0
            COVER="$(raw_cover "$POLICY_FILE")" || rc=$?
            [[ "$rc" -eq 0 ]] \
                || COVERASK="policy spec.cover.script cannot be read ($DEGRADED); confirm $FILE_PATH is not the covering script"
        fi
    elif command -v gates_cover_script >/dev/null 2>&1; then
        COVER="$(gates_cover_script)"
    elif [[ -f "$POLICY_FILE" ]]; then
        COVER="$(jq -r '(.spec.cover.script // empty) | strings' "$POLICY_FILE" 2>/dev/null)" || COVER=""
    fi
    # ./a, a//b and a/./b name the same file as a, a/b.
    COVER="$(printf '%s' "$COVER" | sed -e 's#//*#/#g' -e 's#/\(\./\)*#/#g' -e 's#^\(\./\)*##')"
    if [[ -n "$COVER" && -z "$COVERASK" ]]; then
        CREL="$FILE_PATH"
        [[ "$FILE_PATH" == "$PROJECT_ROOT/"* ]] && CREL="${FILE_PATH:$((${#PROJECT_ROOT} + 1))}"
        CREL="$(printf '%s' "$CREL" | sed -e 's#//*#/#g' -e 's#/\(\./\)*#/#g' -e 's#^\(\./\)*##')"
        CREAL=""
        CROOT="$(cd "$PROJECT_ROOT" 2>/dev/null && pwd -P)" || CROOT=""
        [[ -n "$CROOT" && "$REAL_PATH" == "$CROOT/"* ]] && CREAL="${REAL_PATH:$((${#CROOT} + 1))}"
        # The real path also catches a link to the script; -ef a hard link.
        if [[ "$CREL" == "$COVER" || "$CREAL" == "$COVER" ]] \
            || { [[ -f "$REAL_PATH" && -f "$PROJECT_ROOT/$COVER" ]] && [[ "$REAL_PATH" -ef "$PROJECT_ROOT/$COVER" ]]; }; then
            COVERASK="this edits the covering script $COVER (spec.cover.script), protected like the policy; confirm the change (committing it needs Protected-Change and Approved-By trailers)"
        fi
    fi
fi

shopt -u nocasematch
if [[ -n "$BLOCKED" ]]; then
    echo "BLOCKED: $BLOCKED" >&2
    echo "File: $ORIG_PATH" >&2
    exit 2
fi

# Project-owned rules (#71) run once no shipped rule blocked, so they can
# add a refusal but never remove one. They run before any "ask": a project
# refusal is stronger than a question.
LROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
if compgen -G "$LROOT/.specify/gates/hooks.local.d/protect-files/*.sh" >/dev/null; then
    LLIB="$LROOT/.specify/gates/lib/local-hooks.sh"
    if [[ ! -f "$LLIB" ]] || ! "$BASH" -n "$LLIB" 2>/dev/null; then
        ask "local rules exist in hooks.local.d/protect-files, but lib/local-hooks.sh cannot load; run /speckit.gates.doctor"
    fi
    # shellcheck source=/dev/null disable=SC1090
    source "$LLIB"
    if ! GATES_LOCAL_STDIN="$INPUT" gates_run_local "$LROOT" protect-files; then
        echo "BLOCKED: $GATES_LOCAL_MSG" >&2
        echo "File: $FILE_PATH" >&2
        exit 2
    fi
fi

[[ "$SECRETS_LIB" == ok ]] \
    || ask "lib/secrets.sh cannot load, so this edit cannot be checked against the secret-file list; run /speckit.gates.doctor"
[[ -n "$CONSTASK" ]] && ask "$CONSTASK"
[[ -n "$COVERASK" ]] && ask "$COVERASK"
[[ -n "$ASK" ]] && ask "$ASK"
[[ -n "$NAMEASK" ]] && ask "$NAMEASK"

if [[ -n "$DEGRADED" ]]; then
    echo "gates: protect-files checked in raw mode ($DEGRADED); run /speckit.gates.doctor" >&2
fi
exit 0
