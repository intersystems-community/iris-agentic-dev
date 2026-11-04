#!/bin/bash
set -euo pipefail

# PreToolUse hook for Bash commands.
# Reads JSON from stdin, parses the command field, blocks destructive patterns.
# Exit 2 = block. Exit 0 = allow, or (with the JSON below on stdout) ask.
#
# Never a silent allow (issue #83): without jq, or when the input is not
# valid JSON, the command is read in raw mode (see raw_field) and every
# block rule still applies. A state the hook cannot judge -- an internal
# error, a command it cannot decode -- returns a PreToolUse "ask" decision,
# so a human confirms the call instead of the hook guessing either way.

# ask <reason>: hand the decision to the human (PreToolUse "ask"). Static
# printf, no jq: this must work in exactly the states where jq is missing.
ask() {
    local r="${1//\\/\\\\}"
    r="${r//\"/\\\"}"
    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"%s"}}\n' "gates: $r"
    exit 0
}

trap 'ask "validate-bash.sh failed unexpectedly (line $LINENO); run /speckit.gates.doctor"' ERR

# raw_field <name>: print the decoded value of the JSON string field <name>
# from $INPUT without jq. A JSON string is a regular language, so the sed
# match is exact for it; escapes are decoded below. Returns 1 when the
# field is absent and 2 when the value uses an escape this decoder does not
# handle (\uXXXX could spell a blocked word) or the key appears more than
# once (which one the hook reads would be a guess, #121), which the caller
# turns into "ask".
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

# Every rule below is an `if ... | grep` test: without grep each one is
# silently false, which would turn the hook into an allow-all.
for _tool in grep sed tr awk; do
    command -v "$_tool" >/dev/null 2>&1 \
        || ask "$_tool not found, so validate-bash cannot check this command; run /speckit.gates.doctor"
done
# The text tools read the command as bytes (#238): every pattern here is
# ASCII, and in a UTF-8 locale a byte sequence that is not UTF-8 aborts
# BWK awk ("towc: multibyte conversion failure"), stops BSD tr and sed
# ("Illegal byte sequence") and turns GNU grep -o into "Binary file
# matches". bash itself keeps the caller's locale.
awk() { LC_ALL=C command awk "$@"; }
sed() { LC_ALL=C command sed "$@"; }
tr() { LC_ALL=C command tr "$@"; }
grep() { LC_ALL=C command grep "$@"; }

INPUT=$(cat /dev/stdin)
DEGRADED=""
if command -v jq >/dev/null 2>&1 && printf '%s' "$INPUT" | jq -e . >/dev/null 2>&1; then
    COMMAND=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty')
else
    if command -v jq >/dev/null 2>&1; then
        DEGRADED="the hook input is not valid JSON"
    else
        DEGRADED="jq not found"
    fi
    rc=0
    COMMAND="$(raw_field command)" || rc=$?
    [[ "$rc" -eq 2 ]] && ask "cannot decode the command without jq ($DEGRADED); confirm it is safe"
    # No command field at all: nothing here says the call is harmless.
    [[ "$rc" -eq 1 ]] && ask "no command found in the hook input without jq ($DEGRADED); confirm the call"
fi

if [[ -z "$COMMAND" ]]; then
    exit 0
fi
LROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

BLOCKED=""

# Text that changes no file (#195): the literal message of a `git commit
# -m` (also a quoted "$(cat <<'EOF' ...)" heredoc) and the literal body,
# title or notes of a `gh` command are blanked in BCMD (and in RMCMD for
# the rm guard below), and PCMD also drops the segments of read-only commands (grep, cat, ls, ...),
# so `grep -n rm <protected path>` and a commit message that names one do
# not ask. A path named in either still counts (NCMD) when another segment
# changes files; a redirect is read from BCMD, so `grep x > <protected
# path>` still asks. A command whose quoting the tokenizer cannot follow
# exactly, or that also runs a shell, eval or xargs, is left as it is.
inert_awk() {
    cat <<'AWK'
    # mode=b: the command with literal commit messages (and gh bodies,
    # titles, notes) blanked; mode=p:
    # also without its read-only segments. Anything whose quoting this
    # cannot follow exactly (an unbalanced quote, $'...', ${...}, a
    # command substitution other than the quoted heredoc, a heredoc, a
    # comment) and a command that feeds a shell, eval or xargs print the
    # command unchanged.
    # The last path component; split, as sub(/.*\//) is quadratic on a
    # long word in BWK awk (macOS, #231).
    function base(w,    n, p) { n = split(w, p, "/"); return n ? p[n] : "" }
    function endword(   o) {
        if (!inw) return
        o = rw
        if (redir) redir = 0
        else if (cmd == "") {
            if (wt !~ /^[A-Za-z_][A-Za-z0-9_]*=/) { cmd = base(wt); if (cmd == ".") guard = 1 }
        } else if (cmd == "git" && sub_ == "") {
            if (pend) pend = 0
            else if (wt ~ /^(-C|-c|--git-dir|--work-tree|--namespace)$/) pend = 1
            else if (wt !~ /^-/) sub_ = wt
        } else if (sub_ == "commit" && wlit) {
            if (prev ~ /^-[a-zA-Z]*m$/ || prev == "--message") o = "''"
            else if (wt ~ /^--message=/) o = "--message=''"
        } else if (cmd == "gh" && wlit) {
            if (prev ~ /^(-b|--body|-t|--title|--notes)$/) o = "''"
            else if (wt ~ /^--(body|title|notes)=/) { o = wt; sub(/=.*/, "=''", o) }
        }
        if (base(wt) ~ /^(sh|bash|zsh|dash|ksh|fish|eval|xargs|source|parallel)$/) guard = 1
        emit(o); prev = wt
        inw = 0; rw = ""; wt = ""; wlit = 1
    }
    # The output is a list of pieces, printed at the end: appending to one
    # string per word is quadratic in BWK awk (#231). ro marks the pieces
    # of read-only segments, which mode=p leaves out.
    function endseg(   k) {
        endword()
        if (cmd ~ /^(grep|egrep|fgrep|cat|head|tail|wc|ls|stat|diff|cmp|echo|printf|type|which|cd|pushd|popd|true)$/)
            for (k = s0; k <= np; k++) ro[k] = 1
        s0 = np + 1; cmd = ""; sub_ = ""; pend = 0; prev = ""; redir = 0
    }
    function emit(c) { pc[++np] = c }
    # A "$(cat <<'X' ... X)" heredoc at position p of s: its end, or 0.
    # An unquoted delimiter counts only when the body expands nothing.
    function heredoc(p,    r, d, q, e, body, k) {
        r = substr(s, p)
        if (!match(r, /^\$\([ \t]*cat[ \t]*<<-?[ \t]*/)) return 0
        r = substr(r, RLENGTH + 1); e = RLENGTH
        q = 0
        if (match(r, /^'[A-Za-z0-9_]+'/) || match(r, /^"[A-Za-z0-9_]+"/)) { d = substr(r, 2, RLENGTH - 2); q = 1 }
        else if (match(r, /^\\[A-Za-z0-9_]+/)) { d = substr(r, 2, RLENGTH - 1); q = 1 }
        else if (match(r, /^[A-Za-z0-9_]+/)) d = substr(r, 1, RLENGTH)
        else return 0
        e += RLENGTH; r = substr(r, RLENGTH + 1)
        if (!match(r, /^[ \t]*\n/)) return 0
        e += RLENGTH; r = substr(r, RLENGTH + 1)
        body = "\n" r
        k = index(body, "\n" d "\n")
        if (k == 0) return 0
        if (!q && substr(body, 1, k) ~ /[$`\\]/) return 0
        r = substr(body, k + length(d) + 2)
        if (!match(r, /^[ \t\n]*\)/)) return 0
        return p + e + k + length(d) + RLENGTH - 1
    }
    { s = s (NR > 1 ? "\n" : "") $0 }
    END {
        # mode=b changes only a commit message or a gh text, so without
        # "commit" or "gh" (quotes and backslashes aside) it prints the
        # command as it is, walking nothing (#231).
        if (mode == "b") {
            t = s; gsub(/["'\\]/, "", t)
            if (!index(t, "commit") && !index(t, "gh")) { print s; exit }
        }
        # Characters from an array split once, runs of plain ones copied
        # with one substr: substr per character is quadratic in BWK awk.
        n = split(s, ch, ""); i = 1; wlit = 1; s0 = 1
        while (i <= n) {
            c = ch[i]
            if (c == "'") {
                j = i + 1
                while (j <= n && ch[j] != "'") j++
                if (j > n) { bail = 1; break }
                rw = rw substr(s, i, j - i + 1); wt = wt substr(s, i + 1, j - i - 1); inw = 1
                i = j + 1; continue
            }
            if (c == "\"") {
                inw = 1; rw = rw c; i++
                while (i <= n && ch[i] != "\"") {
                    c = ch[i]
                    if (c == "$" && ch[i + 1] == "(") {
                        e = heredoc(i)
                        if (!e) { bail = 1; break }
                        rw = rw substr(s, i, e - i + 1); wt = wt "x"; i = e + 1; continue
                    }
                    if (c == "`" || (c == "$" && ch[i + 1] == "{")) { bail = 1; break }
                    if (c == "$" || c == "\\") wlit = 0
                    if (c == "\\") { rw = rw substr(s, i, 2); wt = wt substr(s, i + 1, 1); i += 2; continue }
                    j = i + 1
                    while (j <= n && index("\"$`\\", ch[j]) == 0) j++
                    rw = rw substr(s, i, j - i); wt = wt substr(s, i, j - i); i = j
                }
                if (bail || i > n) { bail = 1; break }
                rw = rw "\""; i++; continue
            }
            if (c == "\\") { rw = rw substr(s, i, 2); wt = wt substr(s, i + 1, 1); wlit = 0; inw = 1; i += 2; continue }
            if (c == " " || c == "\t") { endword(); emit(c); i++; continue }
            if (c == "`" || (c == "#" && !inw) || (c == "$" && ch[i + 1] ~ /[('{]/) \
                || (c == "<" && ch[i + 1] ~ /[(<]/) || (c == ">" && ch[i + 1] == "(")) {
                bail = 1; break
            }
            if (c == "\n" || c == ";" || c == "&" || c == "|" || c == "(" || c == ")") {
                endseg(); emit(c); s0 = np + 1; i++; continue
            }
            if (c == "<" || c == ">") { endword(); emit(c); redir = 1; i++; continue }
            if (c == "$") wlit = 0
            j = i + 1
            while (j <= n && index("'\"\\ \t`#$<>\n;&|()", ch[j]) == 0) j++
            rw = rw substr(s, i, j - i); wt = wt substr(s, i, j - i); inw = 1; i = j
        }
        if (!bail) endseg()
        if (bail || guard) print s
        else {
            for (k = 1; k <= np; k++) if (mode != "p" || !ro[k]) printf "%s", pc[k]
            printf "\n"
        }
    }
AWK
}
INERT_AWK="$(inert_awk)"

# The command without the bodies of its quoted heredocs (<<'X', <<"X",
# <<\X), which are data: `cat > notes.md <<'EOF'` may quote an rm example
# (#218). Bodies stay when any word outside them names a shell or an
# interpreter that could run them (bash <<'EOF', cat <<'EOF' | sh, a
# script written and then run with bash), and everything stays when the
# quoting or a terminator line cannot be followed exactly.
quoted_heredoc_awk() {
    cat <<'AWK'
    # One pass over the lines; a body is the lines after the line that
    # holds its operator, up to the terminator line.
    # Without a "<<" there is no body, and the lines print as they are.
    # Characters come from an array split once per line: substr per
    # character is quadratic in BWK awk (#231).
    { ln[++n] = $0; if (index($0, "<<")) hd = 1 }
    END {
        if (!hd) { for (j = 1; j <= n; j++) print ln[j]; exit }
        q = ""; nb = ""; k = 1
        while (k <= n) {
            s = ln[k]; m = split(s, ch, ""); np = 0; i = 1
            while (i <= m) {
                c = ch[i]
                if (q == "'") { if (c == "'") q = ""; i++; continue }
                if (c == "\\") { i += 2; continue }
                if (q == "\"") { if (c == "\"") q = ""; i++; continue }
                if (c == "'" || c == "\"") { q = c; i++; continue }
                # ch[1] at the start, as substr(s, 0, 1) gives the first
                if (c == "<" && ch[i + 1] == "<" && ch[i + 2] != "<" && ch[i > 1 ? i - 1 : 1] != "<") {
                    r = substr(s, i + 2); t = 0
                    if (substr(r, 1, 1) == "-") { t = 1; r = substr(r, 2) }
                    match(r, /^[ \t]*/); r = substr(r, RLENGTH + 1)
                    if (match(r, /^'[^']+'/) || match(r, /^"[^"]+"/)) { d = substr(r, 2, RLENGTH - 2); hq = 1 }
                    else if (match(r, /^\\[A-Za-z0-9_.-]+/)) { d = substr(r, 2, RLENGTH - 1); hq = 1 }
                    else if (match(r, /^[A-Za-z0-9_.-]+/)) { d = substr(r, 1, RLENGTH); hq = 0 }
                    else { i += 2; continue }
                    np++; pd[np] = d; pq[np] = hq; pt[np] = t
                    i += 2; continue
                }
                i++
            }
            nb = nb s "\n"; k++
            for (h = 1; h <= np; h++) {
                b = k
                while (k <= n) {
                    t = ln[k]; if (pt[h]) sub(/^\t+/, "", t)
                    if (t == pd[h]) break
                    k++
                }
                if (k > n) { all = 1; break }
                if (pq[h]) for (j = b; j < k; j++) drop[j] = 1
                nb = nb ln[k] "\n"; k++
            }
            if (all) break
        }
        if (q != "" || nb ~ /(^|[\n;&|(`])[ \t]*\.[ \t]/) all = 1
        m = all ? 0 : split(nb, w, /[^A-Za-z0-9_.\/-]+/)
        for (j = 1; j <= m; j++) {
            x = w[j]; nx = split(x, p, "/"); x = p[nx]  # as in base(), #231
            if (x ~ /^(sh|bash|zsh|dash|ksh|ash|mksh|fish|eval|exec|xargs|source|parallel|ssh|su|python[0-9.]*|perl|ruby|node|deno|bun|php|osascript)$/) all = 1
        }
        for (j = 1; j <= n; j++) if (all || !drop[j]) print ln[j]
    }
AWK
}

# Destructive filesystem operations (#68). `rm` must be a whole word (a path
# prefix such as /bin/rm is fine), so "brainstorm /" does not match. Two
# rules over each rm command segment:
#   1. root, root wildcard, or home as a complete argument -- always blocked;
#   2. any other absolute path -- blocked unless under a temp root (/tmp,
#      /private/tmp, /var/folders, /private/var/folders, $TMPDIR), so
#      `rm -rf /tmp/build` is fine while `rm -rf /var/data` is not.
# Both read RMCMD, the command without its data (#218, #205): quoted
# heredoc bodies, literal commit messages and gh bodies are left out, a
# backtick ends a segment, and a redirect target (`grep -n rm x >
# /dev/null`, `rm -f x 2>/dev/null`) is not an rm argument.
# POSIX classes only: \b and \s differ between GNU grep (Linux/CI) and BSD
# grep (macOS).
RMCMD="$(printf '%s\n' "$COMMAND" | awk "$(quoted_heredoc_awk)" | awk -v mode=b "$INERT_AWK")"
# shellcheck disable=SC2016
RM_TARGET='(/|/\*|~|~/|~/\*|\$HOME|\$HOME/|\$HOME/\*|\$\{HOME\}|\$\{HOME\}/|"\$HOME"|"\$HOME/")'
# shellcheck disable=SC2016  # the backtick is a literal segment end
if grep -qE '(^|[^[:alnum:]_.-])rm[[:space:]]+([^;&|`]*[[:space:]])?'"$RM_TARGET"'([[:space:]]|[;&|)`]|$)' <<<"$RMCMD"; then
    BLOCKED="Destructive rm command targeting root, home, or wildcard"
fi
if [[ -z "$BLOCKED" ]]; then
    while IFS= read -r _seg; do
        _skip=0
        # The arguments after rm, which the match puts at offset 0 or 1;
        # a substring, not ${_seg#*rm} (#238).
        if [[ "${_seg:0:2}" == rm ]]; then _seg="${_seg:2}"; else _seg="${_seg:3}"; fi
        # shellcheck disable=SC2086  # deliberate word split of the arguments
        for _arg in $_seg; do
            if [[ "$_skip" -eq 1 ]]; then
                _skip=0
                continue
            fi
            case "$_arg" in
                '>' | '>>' | '>|' | '&>' | '&>>' | '<' | '<<<' | [0-9]'>' | [0-9]'>>' | [0-9]'<')
                    _skip=1
                    continue
                    ;;
                '>'* | '<'* | '&>'* | [0-9]'>'* | [0-9]'<'*) continue ;;
            esac
            # One quote off each end; by substring, as ${_arg#[\"\']} and
            # ${_arg%[\"\']} are quadratic in bash 3.2 on a long word (#231).
            case "$_arg" in [\"\']*) _arg="${_arg:1}" ;; esac
            case "${_arg: -1}" in [\"\']) _arg="${_arg:0:${#_arg}-1}" ;; esac
            # The quoted $TMPDIR patterns are literal on purpose: they match
            # the unexpanded command text.
            # shellcheck disable=SC2016
            case "$_arg" in
                /tmp/?* | /private/tmp/?* | /var/folders/?* | /private/var/folders/?*) ;;
                '$TMPDIR'/?* | '${TMPDIR}'/?*) ;;
                /*) BLOCKED="Destructive rm command on an absolute path outside the temp directories ($_arg)" ;;
            esac
        done
    done < <(grep -oE '(^|[^[:alnum:]_.-])rm[[:space:]]+[^;&|`]*' <<<"$RMCMD" || true)
fi

# Force push
if grep -qE 'git\s+push\s+(.*\s)?(-f|--force)(\s|$)' <<<"$COMMAND"; then
    BLOCKED="git push --force"
fi

# Hard reset
if grep -qE 'git\s+reset\s+--hard' <<<"$COMMAND"; then
    BLOCKED="git reset --hard"
fi
if grep -qE 'git\s+clean\s+-[a-zA-Z]*f' <<<"$COMMAND"; then
    BLOCKED="git clean -f"
fi
if grep -qE 'git\s+checkout\s+\.$' <<<"$COMMAND"; then
    BLOCKED="git checkout . (discards all changes)"
fi
if grep -qE 'git\s+restore\s+\.$' <<<"$COMMAND"; then
    BLOCKED="git restore . (discards all changes)"
fi

# Dangerous permissions
if grep -qE 'chmod\s+(-R\s+)?777' <<<"$COMMAND"; then
    BLOCKED="chmod 777"
fi

# Disk destruction
if grep -qE '>\s*/dev/sd' <<<"$COMMAND"; then
    BLOCKED="Write to raw disk device"
fi
if grep -qE 'mkfs\.' <<<"$COMMAND"; then
    BLOCKED="Format filesystem"
fi
if grep -qE 'dd\s+if=/dev/(zero|random)' <<<"$COMMAND"; then
    BLOCKED="dd from zero/random device"
fi

# Fork bomb
if grep -qF ':(){ :|:& };:' <<<"$COMMAND"; then
    BLOCKED="Fork bomb"
fi

# Environment destruction
if grep -qE '(unset\s+PATH|PATH=\s*$)' <<<"$COMMAND"; then
    BLOCKED="PATH destruction"
fi

# Pipe to shell
if grep -qE '(curl|wget)\s.*\|\s*(sh|bash)' <<<"$COMMAND"; then
    BLOCKED="Pipe remote content to shell"
fi

# A shipped "ask" waits until the project's rules have run: a local refusal
# is stronger than a question (#130). defer_ask keeps the first reason.
ASK=""
defer_ask() { [[ -n "$ASK" ]] || ASK="$1"; }

if [[ -z "$DEGRADED" ]]; then
    CWD="$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null || true)"
else
    CWD="$(raw_field cwd || true)"
fi
[[ -n "$CWD" ]] || CWD="$LROOT"

# Bulk staging (#71): with policy git.block_bulk_staging on, refuse a
# `git add` that stages everything or a whole directory, so an untracked
# directory cannot be swept into a commit. Explicit files, -u, --renormalize,
# `git commit -a` and -p stay allowed: they stage only tracked changes. Agent boundary only: pre-commit sees the index, not how
# it was filled. bulk_staging_on: 0 on, 1 off, 2 the policy cannot be read.
bulk_staging_on() {
    local pf="$LROOT/.specify/gates/policy.json" v
    [[ -f "$pf" ]] || return 1
    if [[ -z "$DEGRADED" ]]; then
        v="$(jq -r '.git.block_bulk_staging // false' "$pf" 2>/dev/null)" || return 2
        [[ "$v" == "true" ]] && return 0
        return 1
    fi
    grep -qE '"block_bulk_staging"[[:space:]]*:[[:space:]]*true' <<<"$(tr '\n' ' ' <"$pf")" && return 0
    return 1
}

# git_scan: read every simple command of $COMMAND that runs git -- behind
# environment assignments, env, command, sudo, exec, nohup, time or nice,
# and after git's global options (-C, -c, --no-pager, ...) -- and print one
# finding per line:
#   BULK <cmd>   `git add`/`git stage` staging in bulk: -A, --all, `.`, a
#                directory (quoted or not), "$PWD", a glob or a pathspec
#                with magic (`:/`, `:(top)`), which git expands itself
#   BULKQ <arg>  an argument this check cannot resolve (an unbalanced quote,
#                an escaped space, a variable or a command substitution)
#   HOOKS <what> a git hook bypass: --no-verify, `commit -n`, a
#                core.hooksPath setting, or a hook manager's skip variable
#                (HUSKY=0, LEFTHOOK=0, SKIP=...) before a hook-running git
#                command (#194)
#   NOHOOK <what>  a command that creates commits without running the
#                commit hooks (#187): `cherry-pick`, `rebase`, `am` and
#                `revert`, also with --continue or --skip (both replay
#                further commits); --abort, --quit, --edit-todo and
#                --show-current-patch create none
#   DESTRUCT <what>  a whole-tree discard (#170): `checkout`/`restore`/`rm`
#                of `.`, `:/` or other pathspec magic, in any spelling and
#                position (`restore --staged` and `rm --cached` alone only
#                unstage), `clean` with -f or --force anywhere, and (#194)
#                `checkout -f` without a pathspec, `switch -f` or
#                `--discard-changes`, `stash clear`
#   FORCE <what> a force push (#194): -f, --force, --force-with-lease,
#                --mirror or a `+ref` refspec
#   PUSHDEL <what>  a remote branch deletion: `:ref`, -d, --delete
# Arguments come from xargs (`xargs git add < list`) are unknown (BULKQ).
# A `cd <dir>` segment moves the directory later relative paths resolve
# against; a `cd` this cannot resolve makes them unknown too.
# head_before <word> <bracket>: set HB to the text of <word> before its
# first character in <bracket> (all of it when there is none). A regex and
# a substring, as ${word%%<bracket>*} is quadratic in bash 3.2 on a long
# word (#233); the expansion stays for text the regex cannot read.
head_before() {
    local re="^[^${2:1}*" rest
    if [[ "$1" =~ $re ]]; then
        HB="${BASH_REMATCH[0]}"
        rest="${1:${#HB}}"
        # shellcheck disable=SC2053  # the bracket is a pattern on purpose
        [[ -z "$rest" || "${rest:0:1}" == $2 ]] && return 0
    fi
    # shellcheck disable=SC2295  # the bracket is a pattern on purpose
    HB="${1%%$2*}"
    return 0
}
# odd_quote <quote> <word>: set QO to 1 when <word> holds an odd number of
# <quote> characters, else 0. Regexes, as counting with ${word//[!\"]/} is
# quadratic in bash 3.2 on a long word (#238); the count stays for text the
# regexes cannot read.
odd_quote() {
    local q="$1" re s
    re="^[^${q}]*(${q}[^${q}]*${q}[^${q}]*)*\$"
    if [[ "$2" =~ $re ]]; then QO=0; return 0; fi
    re="^[^${q}]*${q}[^${q}]*(${q}[^${q}]*${q}[^${q}]*)*\$"
    if [[ "$2" =~ $re ]]; then QO=1; return 0; fi
    if [[ "$q" == \" ]]; then s="${2//[!\"]/}"; else s="${2//[!\']/}"; fi
    QO=$((${#s} % 2))
    return 0
}
# unquote <word>: set UQ to <word> without its " and ' characters. A split
# on them, as ${word//\"/} is quadratic in bash 3.2 in the number of quotes
# (#238).
unquote() {
    case "$1" in *[\"\']*) ;; *) UQ="$1"; return 0 ;; esac
    local IFS=\"\' _uq=()
    read -r -a _uq <<<"$1" || true
    IFS=
    UQ="${_uq[*]:-}"
    return 0
}
# after_eq <word>: set AE to the text of <word> after its first =. A regex
# and a substring, as ${word#*=} is quadratic in bash 3.2 on a long word
# (#238); the expansion stays for text the regex cannot read.
after_eq() {
    if [[ "$1" =~ ^[^=]*= ]]; then
        AE="${1:${#BASH_REMATCH[0]}}"
        return 0
    fi
    AE="${1#*=}"
    return 0
}
git_scan() {
    local seg t a v base cdir sub n i dashdash xa whole staged wtree force seqoff k dq sq
    local -a p
    local hskip=""
    local scwd="$CWD"
    local asg='^[A-Za-z_][A-Za-z0-9_]*='
    local -a w
    while IFS= read -r seg; do
        w=()
        read -r -a w <<<"$seg" || true
        n="${#w[@]}"
        [[ "$n" -gt 0 ]] || continue
        if [[ "${w[0]}" == cd || "${w[0]}" == pushd ]]; then
            t="${w[1]:-}"
            case "$t" in [\"\']*) t="${t:1}" ;; esac # not ${t#[\"\']}, #231
            case "${t: -1}" in [\"\']) t="${t:0:${#t}-1}" ;; esac
            # shellcheck disable=SC2016  # literal command text
            case "$t" in
                '' | '~'* | *'$'* | *'`'* | -*) scwd="" ;;
                /*) scwd="$t" ;;
                *) [[ -n "$scwd" ]] && scwd="$scwd/$t" ;;
            esac
            continue
        fi
        i=0
        xa=0
        while [[ "$i" -lt "$n" ]]; do
            t="${w[i]}"
            # A hook manager's skip variable (#194), set for this command
            # or exported before it: husky, lefthook, pre-commit.
            unquote "$t"
            case "$UQ" in
                HUSKY=0 | LEFTHOOK=0 | LEFTHOOK=false | SKIP=?* | LEFTHOOK_EXCLUDE=?*) hskip="$UQ" ;;
            esac
            if [[ ! "$t" =~ $asg ]]; then
                case "$t" in
                    sudo | env | command | exec | nohup | time | nice | export) ;;
                    xargs)
                        # xargs runs the command with arguments read
                        # elsewhere: skip its options up to the command.
                        xa=1
                        while [[ "$((i + 1))" -lt "$n" && "${w[i + 1]}" != git && "${w[i + 1]}" != */git ]]; do
                            i=$((i + 1))
                        done
                        ;;
                    -*) [[ "$i" -gt 0 ]] || break ;;
                    *) break ;;
                esac
            fi
            i=$((i + 1))
        done
        [[ "$i" -lt "$n" ]] || continue
        t="${w[i]}"
        [[ "$t" == git || "$t" == */git ]] || continue
        if grep -qiF 'core.hookspath' <<<"$seg"; then
            printf 'HOOKS %s\n' "core.hooksPath"
        fi
        cdir=""
        i=$((i + 1))
        while [[ "$i" -lt "$n" ]]; do
            t="${w[i]}"
            case "$t" in
                -C | -c | --git-dir | --work-tree | --namespace | --super-prefix | --config-env)
                    [[ "$t" == -C ]] && cdir="${w[i + 1]:-}"
                    i=$((i + 2))
                    continue
                    ;;
                -*) ;;
                *) break ;;
            esac
            i=$((i + 1))
        done
        sub="${w[i]:-}"
        i=$((i + 1))
        case "$sub" in
            commit | push | merge | am | rebase | cherry-pick | revert | pull)
                [[ -z "$hskip" ]] || printf 'HOOKS %s\n' "$hskip"
                ;;
        esac
        base="$scwd"
        if [[ -n "$cdir" ]]; then
            case "$cdir" in [\"\']*) cdir="${cdir:1}" ;; esac # as above, #231
            case "${cdir: -1}" in [\"\']) cdir="${cdir:0:${#cdir}-1}" ;; esac
            if [[ "$cdir" == /* ]]; then base="$cdir"; elif [[ -n "$scwd" ]]; then base="$scwd/$cdir"; fi
        fi
        if [[ "$xa" -eq 1 && ( "$sub" == add || "$sub" == stage ) ]]; then
            printf 'BULKQ %s\n' "(arguments from xargs)"
        fi
        dashdash=0
        seqoff=0
        whole=""
        staged=0
        wtree=0
        force=""
        while [[ "$i" -lt "$n" ]]; do
            a="${w[i]}"
            i=$((i + 1))
            if [[ "$dashdash" -eq 0 ]]; then
                case "$a" in
                    --) dashdash=1; continue ;;
                    --no-veri*) printf 'HOOKS %s\n' "$a"; continue ;;
                esac
                # A commit option's value (`-m "drop --no-verify"`) is
                # skipped, so its text is not read as options.
                if [[ "$sub" == commit && "$a" == -* ]]; then
                    v=""
                    case "$a" in
                        --*=*) after_eq "$a"; v="$AE" ;;
                        --message | --file | --reuse-message | --reedit-message | --template \
                            | --author | --date | --fixup | --squash | --trailer | --cleanup)
                            v="${w[i]:-}"
                            i=$((i + 1))
                            ;;
                        --*) ;;
                        *)
                            # The cluster from its first option that takes
                            # a value; substrings, not ${t#...} (#233).
                            t="${a:1}"
                            head_before "$t" '[mFcCt]'
                            t="${t:${#HB}}"
                            if [[ "${#t}" -eq 1 ]]; then
                                v="${w[i]:-}"
                                i=$((i + 1))
                            elif [[ -n "$t" ]]; then
                                v="${t:1}"
                            fi
                            ;;
                    esac
                    # A quoted value the word split cut: skip to its end.
                    # The quote parity is kept per word added, so a long
                    # value is not counted again for every word (#238).
                    odd_quote \" "$v"
                    dq="$QO"
                    odd_quote \' "$v"
                    sq="$QO"
                    while [[ "$i" -lt "$n" && ( "$dq" -eq 1 || "$sq" -eq 1 ) ]]; do
                        odd_quote \" "${w[i]}"
                        dq=$((dq ^ QO))
                        odd_quote \' "${w[i]}"
                        sq=$((sq ^ QO))
                        i=$((i + 1))
                    done
                fi
                # `-n` is --no-verify for commit, also inside a cluster
                # (`-nm`), up to the first option that takes a value.
                if [[ "$sub" == commit && "$a" == -* && "$a" != --* ]] \
                    && head_before "$a" '[mFcCtSu]' && [[ "$HB" == *n* ]]; then
                    printf 'HOOKS git commit %s\n' "$a"
                    continue
                fi
            fi
            unquote "$a"
            t="$UQ"
            case "$sub" in
                push)
                    if [[ "$dashdash" -eq 0 ]]; then
                        case "$t" in
                            --force* | --mirror) printf 'FORCE git push %s\n' "$t" ;;
                            --delete) printf 'PUSHDEL git push %s\n' "$t" ;;
                            --*) ;;
                            -*f*) printf 'FORCE git push %s\n' "$t" ;;
                            -*d*) printf 'PUSHDEL git push %s\n' "$t" ;;
                        esac
                    fi
                    # `+ref` forces that ref; `:ref` deletes it.
                    case "$t" in
                        +?*) printf 'FORCE git push %s\n' "$t" ;;
                        :?*) printf 'PUSHDEL git push %s\n' "$t" ;;
                    esac
                    continue
                    ;;
                stash)
                    [[ "$t" == clear ]] && printf 'DESTRUCT git stash clear\n'
                    break
                    ;;
                switch)
                    if [[ "$dashdash" -eq 0 ]]; then
                        case "$t" in
                            --discard-changes | --force) printf 'DESTRUCT git switch %s\n' "$t" ;;
                            --*) ;;
                            -*f*) printf 'DESTRUCT git switch %s\n' "$t" ;;
                        esac
                    fi
                    continue
                    ;;
                cherry-pick | rebase | am | revert)
                    if [[ "$dashdash" -eq 0 ]]; then
                        case "$t" in
                            --abort | --quit | --edit-todo | --show-current-patch*) seqoff=1 ;;
                        esac
                    fi
                    continue
                    ;;
                rm)
                    if [[ "$dashdash" -eq 0 ]]; then
                        case "$t" in
                            --cached) staged=1; continue ;;
                            -*) continue ;;
                        esac
                    fi
                    case "$t" in
                        . | ./ | '*' | ./'*' | :*) whole="$a" ;;
                    esac
                    continue
                    ;;
                clean)
                    if [[ "$dashdash" -eq 0 ]]; then
                        case "$t" in
                            --force) printf 'DESTRUCT git clean %s\n' "$t" ;;
                            --*) ;;
                            -*f*) printf 'DESTRUCT git clean %s\n' "$t" ;;
                        esac
                    fi
                    continue
                    ;;
                checkout | restore)
                    if [[ "$dashdash" -eq 0 ]]; then
                        case "$t" in
                            --staged) staged=1; continue ;;
                            --worktree) wtree=1; continue ;;
                            --force) [[ "$sub" == checkout ]] && force="$t"; continue ;;
                            --*) continue ;;
                            -*)
                                [[ "$t" == *S* ]] && staged=1
                                [[ "$t" == *W* ]] && wtree=1
                                [[ "$sub" == checkout && "$t" == *f* ]] && force="$t"
                                continue
                                ;;
                        esac
                    fi
                    # The whole tree: `.`, `./`, `*`, `:/`, `:(top)` or
                    # other magic (the segment split may cut it at the
                    # parenthesis).
                    case "$t" in
                        . | ./ | '*' | ./'*' | :*) whole="$a" ;;
                    esac
                    continue
                    ;;
            esac
            [[ "$sub" == add || "$sub" == stage ]] || continue
            if [[ "$dashdash" -eq 0 ]]; then
                case "$a" in
                    -A | --all | --no-ignore-removal | --pathspec-from-file*)
                        printf 'BULK git %s %s\n' "$sub" "$a"
                        continue
                        ;;
                    --*) continue ;;
                    -*A*)
                        printf 'BULK git %s %s\n' "$sub" "$a"
                        continue
                        ;;
                    -*) continue ;;
                esac
            fi
            # Rejoin an argument the word split cut: a quoted name with a
            # space ("src dir") or an escaped one (src\ dir). The pieces
            # are joined once and the quote parity kept per piece, so a
            # long argument is not copied or counted per word (#238).
            p=("$a")
            k=0
            odd_quote \" "$a"
            dq="$QO"
            odd_quote \' "$a"
            sq="$QO"
            while [[ "$i" -lt "$n" ]]; do
                if [[ "${p[k]: -1}" == \\ ]]; then
                    p[k]="${p[k]:0:${#p[k]}-1}"
                elif [[ "$dq" -eq 0 && "$sq" -eq 0 ]]; then
                    break
                fi
                k=$((k + 1))
                p[k]="${w[i]}"
                odd_quote \" "${w[i]}"
                dq=$((dq ^ QO))
                odd_quote \' "${w[i]}"
                sq=$((sq ^ QO))
                i=$((i + 1))
            done
            a="${p[*]}"
            unquote "$a"
            t="$UQ"
            # Pathspec magic (`:/`, `:(top)`, `:!x`): git expands it, and
            # the segment split may have cut it at the parenthesis.
            if [[ "$t" == :* ]]; then
                printf 'BULK git %s %s\n' "$sub" "$a"
                continue
            fi
            if [[ "${t: -1}" == \\ || "$dq" -eq 1 || "$sq" -eq 1 ]]; then
                printf 'BULKQ %s\n' "$a"
                continue
            fi
            # The quoted patterns are literal command text, not expansions.
            # shellcheck disable=SC2016,SC2088
            case "$t" in
                '$PWD' | '$PWD/'* | '${PWD}' | '${PWD}/'* | '~' | '~/'*)
                    printf 'BULK git %s %s\n' "$sub" "$a"
                    ;;
                *'$'* | *'`'* | *\\*) printf 'BULKQ %s\n' "$a" ;;
                . | ./ | :* | */ | *[*?[]*) printf 'BULK git %s %s\n' "$sub" "$a" ;;
                *)
                    if [[ -z "$base" && "$t" != /* ]]; then
                        printf 'BULKQ %s\n' "$a"
                    elif [[ "$t" == /* && -d "$t" ]] || [[ "$t" != /* && -d "$base/$t" ]]; then
                        printf 'BULK git %s %s\n' "$sub" "$a"
                    fi
                    ;;
            esac
        done
        case "$sub" in
            cherry-pick | rebase | am | revert)
                [[ "$seqoff" -eq 1 ]] || printf 'NOHOOK git %s\n' "$sub"
                ;;
        esac
        if [[ -n "$whole" ]] && { [[ "$sub" == checkout ]] || [[ "$staged" -eq 0 || "$wtree" -eq 1 ]]; }; then
            printf 'DESTRUCT git %s %s\n' "$sub" "$whole"
        fi
        # `checkout -f` without a pathspec throws away every local change.
        if [[ -n "$force" && "$dashdash" -eq 0 ]]; then
            printf 'DESTRUCT git checkout %s\n' "$force"
        fi
    done < <(printf '%s\n' "$COMMAND" | awk '
        # A `...` substitution is an argument this cannot resolve: it
        # stands in as $SUBST, and its own text is scanned as a command.
        { s = $0; inner = ""
          while (match(s, /`[^`]*`/)) {
              inner = inner "\n" substr(s, RSTART + 1, RLENGTH - 2)
              s = substr(s, 1, RSTART - 1) "$SUBST" substr(s, RSTART + RLENGTH)
          }
          s = s inner
          gsub(/&&|\|\||;|\||&|\(|\)|`/, "\n", s)
          # Only a segment that changes directory, runs git or names a
          # hook skip variable can change the result; the loop above
          # skips every other one, and reading each in bash costs seconds
          # on a large command of many lines (#233).
          n = split(s, seg, "\n")
          for (k = 1; k <= n; k++) {
              if (split(seg[k], f, " ") == 0) continue
              t = seg[k]; gsub(/["'"'"']/, "", t)
              if (f[1] == "cd" || f[1] == "pushd" || t ~ /git|HUSKY|LEFTHOOK|SKIP/) print seg[k]
          } }')
    return 0
}
GIT_SCAN="$(git_scan)"
DESTRUCT="$(awk '/^DESTRUCT / { sub(/^DESTRUCT /, ""); print; exit }' <<<"$GIT_SCAN")"
if [[ -n "$DESTRUCT" ]]; then
    BLOCKED="$DESTRUCT (discards uncommitted work)"
fi
FORCE="$(awk '/^FORCE / { sub(/^FORCE /, ""); print; exit }' <<<"$GIT_SCAN")"
if [[ -n "$FORCE" ]]; then
    BLOCKED="$FORCE (force push)"
fi
PUSHDEL="$(awk '/^PUSHDEL / { sub(/^PUSHDEL /, ""); print; exit }' <<<"$GIT_SCAN")"
if [[ -n "$PUSHDEL" ]]; then
    defer_ask "$PUSHDEL deletes a remote branch; confirm it"
fi
if [[ -z "$BLOCKED" ]] && grep -q '^BULK' <<<"$GIT_SCAN"; then
    rc=0
    bulk_staging_on || rc=$?
    BULK="$(awk '/^BULK / { sub(/^BULK /, ""); print; exit }' <<<"$GIT_SCAN")"
    BULKQ="$(awk '/^BULKQ / { sub(/^BULKQ /, ""); print; exit }' <<<"$GIT_SCAN")"
    if [[ "$rc" -eq 0 && -n "$BULK" ]]; then
        BLOCKED="Bulk staging ($BULK) refused by policy git.block_bulk_staging; stage explicit paths"
    elif [[ "$rc" -eq 0 ]]; then
        defer_ask "git add $BULKQ may stage in bulk, which policy git.block_bulk_staging refuses, and this check cannot resolve the argument; confirm it names files"
    elif [[ "$rc" -eq 2 ]]; then
        defer_ask "${BULK:-git add $BULKQ} stages in bulk, and .specify/gates/policy.json cannot be read to check git.block_bulk_staging; run /speckit.gates.doctor"
    fi
fi
HOOKS="$(awk '/^HOOKS / { sub(/^HOOKS /, ""); print; exit }' <<<"$GIT_SCAN")"
if [[ -n "$HOOKS" ]]; then
    defer_ask "this command bypasses the git hooks ($HOOKS), so the commit checks would not run; confirm it"
fi
NOHOOK="$(awk '/^NOHOOK / { sub(/^NOHOOK /, ""); print; exit }' <<<"$GIT_SCAN")"
if [[ -n "$NOHOOK" ]]; then
    defer_ask "$NOHOOK creates commits without running pre-commit or commit-msg (main-branch, secret and message rules), so only the CI boundary checks them; confirm it"
fi
# The spec gate's recursion guard: any verify.sh run that inherits it, a
# commit's hook included, skips the accept blocks (#164). Unsetting it
# (env -u) is fine; setting or exporting it asks.
if grep -qE '(^|[^A-Za-z0-9_])GATES_SPEC_EXEC\+?=|export[[:space:]]+([^;&|]*[[:space:]])?GATES_SPEC_EXEC([[:space:];&|]|$)' <<<"$COMMAND"; then
    defer_ask "this command sets GATES_SPEC_EXEC, so any verify.sh it runs skips the spec gate (accept blocks); confirm it"
fi
# Other variables that weaken enforcement (#196): another policy file, the
# git boundary's emergency skip and main-branch override, another pr-check
# runtime, the test-only projection without canaries. Unsetting is fine.
GATES_WEAK_VARS='GATES_POLICY_FILE|GATES_SKIP|GATES_ALLOW_MAIN_COMMIT|GATES_RUNTIME_DIR|GATES_TEST'
WEAK_SET="$(grep -oE "(^|[^A-Za-z0-9_])($GATES_WEAK_VARS)\+?=|export[[:space:]]+([^;&|]*[[:space:]])?($GATES_WEAK_VARS)([[:space:];&|]|$)" <<<"$COMMAND" || true)"
if [[ -n "$WEAK_SET" ]]; then
    WEAK_SET="$(grep -oE "$GATES_WEAK_VARS" <<<"$WEAK_SET" || true)"
    defer_ask "this command sets ${WEAK_SET%%$'\n'*}, which weakens what the gates enforce; confirm it"
fi

if [[ -n "$BLOCKED" ]]; then
    echo "BLOCKED: $BLOCKED" >&2
    echo "Command: $COMMAND" >&2
    exit 2
fi

# Protected paths through Bash (#95): Write/Edit to a protected path is
# refused by protect-files, but `rm`, `mv`, `sed -i`, a redirect,
# `find -delete` or `git rm` reach it through here. Telling a modification
# from a read by the command text alone is a heuristic, so a command that
# appears to modify one asks the human instead of blocking; reads stay
# allowed. The paths: the project's rules (hooks.local.d), the
# policy-contract artifacts (#137), policy.json and the constitution
# (always, whatever extra says, #165), plus protected_files.extra (glob
# entries by their literal prefix). Without jq, extra is read when it is a
# plain list of strings; when it cannot be read (no jq, or a policy that is
# malformed or fails validation) a command that changes anything asks
# (#121, #165). A path counts as named when it appears in the command, when one
# of its parent directories appears as a whole argument (`rm -rf
# .specify/gates`), when the command first changes into it or a parent
# (`cd .specify/gates && ...`, `git -C`), or relative to the input cwd
# (#191). Matching ignores case, after
# `./`, `//`, `/./`, "$PWD/" and the project root are normalized away
# (#130).
POLICY="$LROOT/.specify/gates/policy.json"
# raw_extra: protected_files.extra without jq, one entry per line. Returns
# 2 when the policy declares an extra this cannot read (escapes, values
# that are not strings, a layout other than a flat array).
raw_extra() {
    local flat body
    flat="$(tr '\n' ' ' <"$POLICY")"
    grep -qE '"extra"[[:space:]]*:' <<<"$flat" || return 0
    body="$(sed -nE 's/.*"protected_files"[[:space:]]*:[[:space:]]*\{[^{}]*"extra"[[:space:]]*:[[:space:]]*\[([^]]*)\].*/=\1/p' <<<"$flat")"
    [[ -n "$body" ]] || return 2
    body="${body#=}"
    grep -qE '^[[:space:]]*("[^"\\]*"[[:space:]]*(,[[:space:]]*"[^"\\]*"[[:space:]]*)*)?$' <<<"$body" || return 2
    { grep -oE '"[^"\\]*"' <<<"$body" || true; } | sed -e 's/^"//' -e 's/"$//'
}
RAW_EXTRA=""
EXTRA_UNREAD=0
EXTRA_WHY="$DEGRADED"
if [[ -n "$DEGRADED" && -f "$POLICY" ]]; then
    rc=0
    RAW_EXTRA="$(raw_extra)" || rc=$?
    [[ "$rc" -eq 2 ]] && EXTRA_UNREAD=1
elif [[ -f "$POLICY" ]]; then
    # The jq reader below returns nothing for a malformed policy or an extra
    # that is not a list of strings, which would read as "nothing protected".
    # Same validation verify.sh and protect-files refuse on (#124, #165).
    _plib="$LROOT/.specify/gates/lib/policy.sh"
    if ! jq -e '(.protected_files.extra // []) | type == "array" and all(type == "string")' \
        "$POLICY" >/dev/null 2>&1; then
        EXTRA_UNREAD=1
    elif [[ -f "$_plib" ]] && "$BASH" -n "$_plib" 2>/dev/null \
        && ! (
            # shellcheck source=/dev/null disable=SC1090
            source "$_plib" && _pf="$(gates_policy_file)" \
                && { [[ ! -f "$_pf" ]] || gates_validate_policy "$_pf"; }
        ) >/dev/null 2>&1; then
        EXTRA_UNREAD=1
    fi
    EXTRA_WHY="the policy is malformed or invalid; run /speckit.gates.doctor"
fi
# The covering script (spec.cover.script, feature 007) is protected like
# the policy whatever extra says: read from policy.json and, in a contract
# repository, the effective policy. Without jq, raw_cover reads a plain
# string in a flat cover object; a cover it cannot read asks for any
# command that changes something.
raw_cover() { # <policy>
    local flat v
    flat="$(tr '\n' ' ' <"$1")"
    grep -qE '"cover"[[:space:]]*:' <<<"$flat" || return 0
    v="$(sed -nE 's/.*"cover"[[:space:]]*:[[:space:]]*\{[^{}]*"script"[[:space:]]*:[[:space:]]*"([^"\\]*)".*/=\1/p' <<<"$flat")"
    [[ -n "$v" ]] || return 2
    printf '%s\n' "${v#=}"
}
COVER_PATHS=""
COVER_UNREAD=0
for _cp in "$POLICY" "$LROOT/.specify/gates/policy.effective.json"; do
    [[ -f "$_cp" ]] || continue
    if [[ -n "$DEGRADED" ]]; then
        rc=0
        _cv="$(raw_cover "$_cp")" || rc=$?
        [[ "$rc" -eq 0 ]] || COVER_UNREAD=1
    else
        _cv="$(jq -r '(.spec.cover.script // empty) | strings' "$_cp" 2>/dev/null)" || _cv=""
    fi
    if [[ -n "$_cv" ]]; then
        COVER_PATHS="$COVER_PATHS$_cv"$'\n'
    fi
done
protected_prefixes() {
    printf '%s\n' ".specify/gates/hooks.local.d" ".specify/gates/baseline.json" \
        ".specify/gates/baseline.lock.json" ".specify/gates/policy.effective.json" \
        ".specify/gates/policy.json" ".specify/memory/constitution.md"
    # ./a, a//b and a/./b name the same file as a, a/b.
    if [[ -n "$COVER_PATHS" ]]; then
        printf '%s' "$COVER_PATHS" | sed -e 's#//*#/#g' -e 's#/\(\./\)*#/#g' -e 's#^\(\./\)*##'
    fi
    {
        if [[ -n "$DEGRADED" ]]; then
            [[ -n "$RAW_EXTRA" ]] && printf '%s\n' "$RAW_EXTRA"
        elif [[ -f "$POLICY" ]]; then
            jq -r '(.protected_files.extra // [])[] | select(type == "string")' "$POLICY" 2>/dev/null || true
        fi
    } | sed -e 's/[*?[].*$//' -e 's:/*$::' | awk 'length($0) > 0' || true
}
# A verb counts after a separator, a path (/bin/rm) or the opening quote of
# a shell string (sh -c 'rm ...', eval "rm ..."), not of any quoted text
# (grep "rm "); a backslash (\rm) is normalized away below (#170). An
# interpreter one-liner (python3 -c, node -e, ...) may write any file, so
# it counts as a change when it names a protected path.
# shellcheck disable=SC2016  # the backtick is a literal command separator
VERB_START='(^|[;&|(`[:space:]/]|(-c|eval)[[:space:]]+["'"'"'])'
MUTATE_VERB="$VERB_START"'(rm|rmdir|unlink|shred|mv|cp|ln|install|truncate|tee|chmod|chown|dd|rsync)([[:space:]]|$)'
MUTATE_EDIT="$VERB_START"'(sed|perl)[[:space:]]+([^;&|]*[[:space:]])?(-[a-zA-Z]*i|--in-place)|'"$VERB_START"'git[[:space:]]+(rm|mv|checkout|restore|reset|clean|stash)([[:space:]]|$)'
MUTATE_FIND="$VERB_START"'find[[:space:]]([^;&|]*[[:space:]])?-(delete|exec|execdir|ok|okdir)([[:space:]]|$)'
MUTATE_INTERP="$VERB_START"'(python[0-9.]*|perl|ruby|node|deno|bun)[[:space:]]+([^;&|]*[[:space:]])?-[a-zA-Z]*[ce]([[:space:]]|$)'
# `sort -o <file>` writes the file; `rg --pre <cmd>` runs <cmd> on every
# file it searches (#205).
MUTATE_TOOL="$VERB_START"'sort[[:space:]]+([^;&|]*[[:space:]])?(-[a-zA-Z]*o|--output)|'"$VERB_START"'rg[[:space:]]+([^;&|]*[[:space:]])?--pre([=[:space:]]|$)'
# The command with path spellings normalized, lowercased. The project root
# is stripped as given and as its real path (/tmp vs /private/tmp, a
# symlinked checkout; #165).
LREAL="$(cd "$LROOT" 2>/dev/null && pwd -P)" || LREAL=""
# shellcheck disable=SC2016  # $PWD is literal command text here
normalize() { printf '%s\n' "$1" | awk -v root="$LROOT" -v rroot="${LREAL:-$LROOT}" '
    function strip(s, p,    i, out) {
        out = ""
        while ((i = index(s, p)) > 0) { out = out substr(s, 1, i - 1); s = substr(s, i + length(p)) }
        return out s
    }
    { s = strip($0, root "/"); s = strip(s, rroot "/"); s = strip(s, "\"$PWD\"/"); s = strip(s, "\"${PWD}\"/")
      s = strip(s, "$PWD/"); s = strip(s, "${PWD}/"); print s }' \
    | sed -E -e 's#\\([^[:space:]\\])#\1#g' \
        -e 's#//+#/#g' -e 's#/(\./)+#/#g' -e "s#(^|[[:space:]\"'=<>;&|(\`])(\./)+#\1#g" \
    | awk '
        # Brace expansion, as the shell does it: policy.{json,x} names
        # policy.json (#170). One group per pass, a bounded number of passes.
        # The first group is found alone and its word matched from where
        # it starts: match() with the word around it tries every position
        # and is quadratic in BWK awk (macOS) on a long line (#231).
        { for (k = 0; k < 20 && match($0, /\{[^{}[:space:]]*,[^{}[:space:]]*\}/); k++) {
              b = RSTART; nw = split(substr($0, 1, b - 1), wp, /[[:space:]{}"'"'"'`]/)
              b -= length(wp[nw])
              match(substr($0, b), /[^[:space:]{}"'"'"'`]*\{[^{}[:space:]]*,[^{}[:space:]]*\}[^[:space:]{}"'"'"'`]*/)
              RSTART += b - 1
              w = substr($0, RSTART, RLENGTH); o = index(w, "{"); c = index(w, "}")
              pre = substr(w, 1, o - 1); post = substr(w, c + 1)
              m = split(substr(w, o + 1, c - o - 1), alt, ","); out = ""
              for (j = 1; j <= m; j++) out = out (j > 1 ? " " : "") pre alt[j] post
              $0 = substr($0, 1, RSTART - 1) out substr($0, RSTART + RLENGTH)
          }
          print }' \
    | tr '[:upper:]' '[:lower:]'; }
NCMD="$(normalize "$COMMAND")"
BCMD="$(normalize "$(printf '%s\n' "$COMMAND" | awk -v mode=b "$INERT_AWK")")"
PCMD="$(normalize "$(printf '%s\n' "$COMMAND" | awk -v mode=p "$INERT_AWK")")"
# A write redirect other than to /dev/null, /dev/std* or a file descriptor.
WRITE_REDIRECT=1
grep -q '>' <<<"$(sed -E -e 's#[0-9]*>>?[[:space:]]*/dev/(null|stdout|stderr|tty)##g' \
    -e 's#[0-9]*>&[0-9-]+##g' -e 's#&>>?[[:space:]]*/dev/null##g' <<<"$BCMD")" || WRITE_REDIRECT=0
MUTATES=0
if grep -qE "$MUTATE_VERB|$MUTATE_EDIT|$MUTATE_FIND|$MUTATE_INTERP|$MUTATE_TOOL" <<<"$PCMD"; then
    MUTATES=1
fi
if [[ "$EXTRA_UNREAD" -eq 1 ]] && [[ "$MUTATES" -eq 1 || "$WRITE_REDIRECT" -eq 1 ]]; then
    defer_ask "policy protected_files.extra cannot be read ($EXTRA_WHY); confirm this command changes no protected path"
fi
if [[ "$COVER_UNREAD" -eq 1 ]] && [[ "$MUTATES" -eq 1 || "$WRITE_REDIRECT" -eq 1 ]]; then
    defer_ask "policy spec.cover.script cannot be read ($DEGRADED); confirm this command does not change the covering script"
fi
ere_escape() { printf '%s' "$1" | sed 's/[][\.*^$+?(){}|/]/\\&/g'; }
# shellcheck disable=SC2016
TOKEN_START='(^|[[:space:]"'"'"'=<>;&|(`])'
# shellcheck disable=SC2016
TOKEN_END='(["'"'"'[:space:];&|)`]|$)'
# The session's working directory relative to the project root, lowercased
# (#191): Claude Code keeps the Bash directory between calls and sends it
# as cwd, so after `cd .specify/gates` a later `rm policy.json` names the
# policy. Empty at the root or outside the project.
CWD_REL=""
_creal="$(cd "$CWD" 2>/dev/null && pwd -P)" || _creal=""
for _c in "${CWD%/}" "$_creal"; do
    for _r in "$LROOT" "$LREAL"; do
        [[ -n "$_c" && -n "$_r" && "$_c" == "$_r"/* ]] || continue
        CWD_REL="$(tr '[:upper:]' '[:lower:]' <<<"${_c#"$_r"/}" | sed -E -e 's#//+#/#g' -e 's#(^|/)(\./)+#\1#g' -e 's#/\.?$##')"
        break 2
    done
done
# rg searches the working directory when no path is named, so a --pre
# command run inside the project may reach any protected file (#205).
if grep -qE "$VERB_START"'rg[[:space:]]+([^;&|]*[[:space:]])?--pre([=[:space:]]|$)' <<<"$PCMD" \
    && [[ -n "$CWD_REL" || "${CWD%/}" == "$LROOT" || ( -n "$LREAL" && "$_creal" == "$LREAL" ) ]]; then
    defer_ask "rg --pre runs its command on every file it searches, protected ones included; confirm the command changes no file"
fi
# The texts the paths are matched in (#194): NCMD, NCMD with its quotes
# removed as the shell does ("pol""icy.json", pol''icy.json), and that with
# the command's own simple assignments expanded (f=.specify/gates; rm
# $f/policy.json), one per line.
NCMD_DQ="$(tr -d "\"'" <<<"$NCMD")"
PCMD="$NCMD"
[[ "$NCMD_DQ" == "$NCMD" ]] || PCMD="$PCMD"$'\n'"$NCMD_DQ"
# One split per line at its $ signs, each name looked up once and the
# output printed in pieces: replacing every assigned name in every line
# scaled with assignments times lines (#238). A second pass expands what a
# value names in turn (g=$f/x).
# shellcheck disable=SC2016  # awk code, not shell
XP_AWK='
    { line[NR] = $0
      n = split($0, w, /[[:space:];&|(]/)
      for (k = 1; k <= n; k++) {
          if (w[k] !~ /^[a-z_][a-z0-9_]*=/) continue
          e = index(w[k], "="); a = substr(w[k], e + 1)
          if ((c = index(a, ")")) > 0) a = substr(a, 1, c - 1)
          v[substr(w[k], 1, e - 1)] = a
      } }
    END { for (k = 1; k <= NR; k++) {
              np = split(line[k], part, /[$]/)
              printf "%s", part[1]
              for (j = 2; j <= np; j++) {
                  r = part[j]
                  if (match(r, /^\{[a-z_][a-z0-9_]*\}/)) nm = substr(r, 2, RLENGTH - 2)
                  else if (match(r, /^[a-z_][a-z0-9_]*/)) nm = substr(r, 1, RLENGTH)
                  else nm = ""
                  if (nm != "" && (nm in v)) printf "%s%s", v[nm], substr(r, RLENGTH + 1)
                  else printf "$%s", r
              }
              printf "\n"
          } }'
_xp="$(awk "$XP_AWK" <<<"$NCMD_DQ")"
[[ "$_xp" != *'$'* ]] || _xp="$(awk "$XP_AWK" <<<"$_xp")"
[[ "$_xp" == "$NCMD_DQ" ]] || PCMD="$PCMD"$'\n'"$_xp"
# pp_spelled <spelling> <anchored>: set _named, _entered and _redirect when
# PCMD names <spelling> (or a parent of it) in a change, changes into it, or
# redirects or writes (--out, --output) to it. An anchored spelling, one
# relative to cwd, counts only at the start of an argument.
pp_spelled() {
    local sp="$1" anch="$2" pre dir e head
    pre="$(ere_escape "$sp")"
    head=""
    [[ "$anch" -eq 1 ]] && head="$TOKEN_START"
    if [[ "$anch" -eq 0 ]]; then
        grep -qF -- "$sp" <<<"$PCMD" && _named=1
    else
        grep -qE -- "$head$pre(/|$TOKEN_END)" <<<"$PCMD" && _named=1
    fi
    dir="$sp"
    while :; do
        e="$(ere_escape "$dir")"
        if grep -qE "(^|[;&|(\`[:space:]])(cd|pushd)[[:space:]]+[\"']?$e/?[\"']?$TOKEN_END|[[:space:]]-c[[:space:]]+[\"']?$e/?[\"']?$TOKEN_END" <<<"$PCMD"; then
            _entered=1
        fi
        # A parent directory counts only as a whole argument (or with a
        # glob under it) in the same command as the change, so `ls .specify
        # && rm build/x` stays allowed.
        if [[ "$dir" != "$sp" && "$MUTATES" -eq 1 ]] \
            && PAT="$TOKEN_START$e(/[^[:space:];&|]*[*?[][^[:space:];&|]*)?/?$TOKEN_END" \
                MUT="$MUTATE_VERB|$MUTATE_EDIT|$MUTATE_FIND|$MUTATE_INTERP|$MUTATE_TOOL" awk '
                    { n = split($0, part, /&&|\|\||;|&/)
                      for (k = 1; k <= n; k++) if (part[k] ~ ENVIRON["PAT"] && part[k] ~ ENVIRON["MUT"]) hit = 1 }
                    END { exit !hit }' <<<"$PCMD"; then
            _named=1
        fi
        [[ "$dir" == */* ]] || break
        dir="${dir%/*}"
    done
    # Redirects read the command with commit-message text blanked (#195).
    if [[ "$anch" -eq 0 ]]; then
        grep -qE "${WRITE_TO}[\"']?[^[:space:];&|]*$pre" <<<"$BCMD" && _redirect=1
    else
        grep -qE "${WRITE_TO}[\"']?$pre(/|$TOKEN_END)" <<<"$BCMD" && _redirect=1
    fi
    return 0
}
# A redirect (also `>|`) or an output option, up to its target.
WRITE_TO='(>[>|]?[[:space:]]*|--(out|output)(=|[[:space:]]+))'
# glob_hit <path> <glob>: protected_files.extra glob matching as in
# gates_glob_match, `*` crossing `/`, also for a `**/` entry at the top.
glob_hit() {
    # shellcheck disable=SC2053  # the glob is a pattern on purpose
    [[ "$1" == $2 ]] && return 0
    # shellcheck disable=SC2053
    [[ "$2" == \*\*/* && "$1" == ${2#\*\*/} ]] && return 0
    return 1
}
# base_name <path>: set BN to the text after the last slash of <path>. A
# regex and a substring, as ${path##*/} is quadratic in bash 3.2 on a long
# word (#233); the expansion stays for text the regex cannot read.
base_name() {
    local head
    if [[ "$1" =~ [^/]*$ ]]; then
        BN="${BASH_REMATCH[0]}"
        head="${1:0:${#1}-${#BN}}"
        [[ -z "$head" || "${head: -1}" == / ]] && return 0
    fi
    BN="${1##*/}"
}
# dir_name <path>: set DN to <path> without its last component, as
# ${path%/*} does, which is quadratic in bash 3.2 in a UTF-8 locale on a
# long last component (#238); <path> itself when it has no slash.
dir_name() {
    if [[ "$1" == */* ]]; then
        base_name "$1"
        DN="${1:0:${#1}-${#BN}-1}"
    else
        DN="$1"
    fi
    return 0
}
# protected_globs: the extra glob entries with no literal prefix
# (`**/*.lock.md`), which protected_prefixes cannot match by text.
protected_globs() {
    {
        if [[ -n "$DEGRADED" ]]; then
            [[ -n "$RAW_EXTRA" ]] && printf '%s\n' "$RAW_EXTRA"
        elif [[ -f "$POLICY" ]]; then
            jq -r '(.protected_files.extra // [])[] | select(type == "string")' "$POLICY" 2>/dev/null || true
        fi
    } | awk '/^[*?[]/' | tr '[:upper:]' '[:lower:]' || true
}
while IFS= read -r _pp; do
    [[ -n "$_pp" ]] || continue
    _pp="$(tr '[:upper:]' '[:lower:]' <<<"$_pp")"
    _named=0
    _entered=0
    _redirect=0
    pp_spelled "$_pp" 0
    if [[ -n "$CWD_REL" ]]; then
        if [[ "$CWD_REL" == "$_pp" || "$CWD_REL" == "$_pp"/* ]]; then
            # The session is inside the protected directory.
            _entered=1
        elif [[ "$_pp" == "$CWD_REL"/* ]]; then
            pp_spelled "${_pp#"$CWD_REL"/}" 1
        fi
    fi
    if { [[ "$_named" -eq 1 && "$MUTATES" -eq 1 ]]; } \
        || { [[ "$_entered" -eq 1 ]] && [[ "$MUTATES" -eq 1 || "$WRITE_REDIRECT" -eq 1 ]]; } \
        || [[ "$_redirect" -eq 1 ]]; then
        defer_ask "this command appears to modify the protected path $_pp; a human or a reviewed change makes that edit"
        break
    fi
    # The rest of the path behind a variable or a substitution this check
    # cannot resolve ($d/policy.json, $(dirname x)/policy.json).
    _b="$(ere_escape "${_pp##*/}")"
    _v="[\$\`)}][^[:space:];&|]*/$_b$TOKEN_END"
    if { [[ "$MUTATES" -eq 1 ]] && grep -qE "$_v" <<<"$NCMD_DQ"; } \
        || grep -qE "${WRITE_TO}[^[:space:];&|]*$_v" <<<"$NCMD_DQ"; then
        defer_ask "this command may modify the protected path $_pp through a variable or substitution this check cannot resolve; confirm it"
        break
    fi
done < <(protected_prefixes)
if [[ "$MUTATES" -eq 1 || "$WRITE_REDIRECT" -eq 1 ]]; then
    PGLOBS="$(protected_globs)"
    if [[ -n "$PGLOBS" ]]; then
        # A change may touch any argument; a redirect only its target.
        if [[ "$MUTATES" -eq 1 ]]; then
            _args="$(tr '<>|;&()=,`' '          ' <<<"$NCMD_DQ")"
        else
            _args="$({ grep -oE "${WRITE_TO}[^[:space:];&|]+" <<<"$NCMD_DQ" || true; } | sed -E "s/^$WRITE_TO//")"
        fi
        _ghit=""
        while IFS= read -r _g && [[ -z "$_ghit" ]]; do
            [[ -n "$_g" ]] || continue
            while read -r -a _w && [[ -z "$_ghit" ]]; do
                for _t in ${_w[@]+"${_w[@]}"}; do
                    # Substrings, not ${_t#./} and ${_t##*/} (#233).
                    case "$_t" in ./*) _t="${_t:2}" ;; esac
                    base_name "$_t"
                    if glob_hit "$_t" "$_g" || glob_hit "$BN" "$_g" \
                        || { [[ -n "$CWD_REL" ]] && glob_hit "$CWD_REL/$_t" "$_g"; }; then
                        _ghit="$_t ($_g)"
                        break
                    fi
                done
            done <<<"$_args"
        done <<<"$PGLOBS"
        [[ -z "$_ghit" ]] || defer_ask "this command appears to modify $_ghit, protected by protected_files.extra; a human or a reviewed change makes that edit"
    fi
fi

# Links (#193): protect-files judges a Write/Edit by its real path, so a
# link that reaches a protected path, or one spelled past the text match
# above (`ln -s ".spec"ify g`), asks. Each ln argument is unquoted and
# resolved from the cwd and from the link's directory; it asks when it is,
# contains or lies under a protected path, or cannot be resolved ($VAR,
# `cmd`, a glob, ~user).
# ln_resolve <absolute path>: every symlink resolved, as protect-files
# does, into LN_OUT; LN_VIA is 1 when a link was followed. No subshell, so
# the caller sees both. ln_real prints the path.
ln_resolve() { # <path> [<real directory a relative path starts from>]
    local todo="$1" out="${2:-}" comp link hops=0
    [[ "$todo" != /* ]] || out=""
    LN_VIA=0
    LN_OUT=""
    while [[ -n "$todo" ]]; do
        while [[ "$todo" == /* ]]; do todo="${todo#/}"; done
        [[ -n "$todo" ]] || break
        # A regex and substrings, as ${todo#*/} is quadratic in bash 3.2
        # on a long first component (#238); the expansions stay for text
        # the regex cannot read.
        if [[ "$todo" =~ ^[^/]*/ ]]; then
            comp="${BASH_REMATCH[0]:0:${#BASH_REMATCH[0]}-1}"
            todo="${todo:${#BASH_REMATCH[0]}}"
        elif [[ "$todo" == */* ]]; then
            comp="${todo%%/*}"
            todo="${todo#*/}"
        else
            comp="$todo"
            todo=""
        fi
        case "$comp" in
            . | '') continue ;;
            ..) dir_name "$out"; out="$DN"; continue ;;
        esac
        if [[ -L "$out/$comp" ]]; then
            LN_VIA=1
            hops=$((hops + 1))
            [[ "$hops" -le 40 ]] && link="$(readlink "$out/$comp" 2>/dev/null)" && [[ -n "$link" ]] || return 1
            [[ "$link" == /* ]] && out=""
            todo="$link${todo:+/$todo}"
        else
            out="$out/$comp"
        fi
    done
    LN_OUT="${out:-/}"
}
ln_real() { ln_resolve "$1" && printf '%s' "$LN_OUT"; }
# lprot_init: _lprot, the protected paths with their own links resolved,
# read once and only when a check needs it.
_lroot="${LREAL:-$LROOT}"
_lprot=()
_lprot_done=0
lprot_init() {
    [[ "$_lprot_done" -eq 0 ]] || return 0
    _lprot_done=1
    while IFS= read -r _pp; do
        [[ -n "$_pp" ]] && _lprot+=("$(ln_real "$_lroot/$_pp" || printf '%s' "$_lroot/$_pp")")
    done < <(protected_prefixes)
}
# One line per ln command: its operands, unquoted (options dropped). ln
# counts as the command word, after a wrapper (sudo, env, xargs, ...) or
# an environment assignment, or as the start of an `sh -c` / eval string.
LN_SEGS="$(printf '%s\n' "$COMMAND" | tr ';&|()' '\n' | awk '
    { out = ""; on = 0; prev = ""
      for (i = 1; i <= NF; i++) {
          t = $i; gsub(/["'"'"'\\]/, "", t)
          if (!on) {
              n = split(t, p, "/")
              if (p[n] == "ln" && (prev == "" || prev ~ /^(sudo|env|command|exec|xargs|nohup|time|nice|builtin|-c|eval)$/ || prev ~ /^[A-Za-z_][A-Za-z0-9_]*=/)) on = 1
              else if (t != "") prev = t
              continue
          }
          if (t ~ /^-/ || t == "") continue
          out = out (out == "" ? "" : " ") t
      }
      if (on && out != "") print out }')"
if [[ -n "$LN_SEGS" ]]; then
    lprot_init
    shopt -s nocasematch
    while IFS= read -r _seg; do
        read -r -a _largs <<<"$_seg"
        _last="${_largs[${#_largs[@]} - 1]}"
        [[ "$_last" == /* ]] || _last="$CWD/$_last"
        dir_name "$_last"
        _bases=("$CWD" "$DN")
        [[ -d "$_last" ]] && _bases+=("$_last")
        for _a in "${_largs[@]}"; do
            [[ "$_a" == \~/* ]] && _a="$HOME/${_a#\~/}"
            case "$_a" in
                *'$'* | *'`'* | *'*'* | *'?'* | *'['* | '~'*)
                    defer_ask "cannot resolve the ln argument $_a; confirm it links to no protected path"
                    continue
                    ;;
            esac
            for _b in "${_bases[@]}"; do
                _r="$_a"
                [[ "$_r" == /* ]] || _r="$_b/$_r"
                if ! _r="$(ln_real "$_r")"; then
                    defer_ask "cannot resolve the symlinks in the ln argument $_a; confirm it links to no protected path"
                    continue
                fi
                for _p in "${_lprot[@]}"; do
                    if [[ "$_r" == / || "$_r" == "$_p" || "$_p" == "$_r/"* || "$_r" == "$_p/"* ]]; then
                        defer_ask "this ln command links to or through the protected path ${_p#"$_lroot"/}; a human makes that link"
                        break 3
                    fi
                done
            done
        done
    done <<<"$LN_SEGS"
    shopt -u nocasematch
fi

# Links that already exist (#211): with pol.json -> policy.json or gdir ->
# .specify/gates in place, `echo x > pol.json` and `rm gdir/policy.json`
# name no protected path. Each redirect target, and for a command that
# changes files each argument, is resolved from the cwd in its original
# case; one that reaches a protected path through a link asks. Quoted
# heredoc bodies are data and are left out; a word with a variable,
# substitution or glob is the business of the checks above.
if [[ "$MUTATES" -eq 1 || "$WRITE_REDIRECT" -eq 1 ]]; then
    _ocmd="$(printf '%s\n' "$COMMAND" | awk "$(quoted_heredoc_awk)" | awk -v mode=b "$INERT_AWK")"
    _lwords="$({ grep -oE "${WRITE_TO}[^[:space:];&|]+" <<<"$_ocmd" || true; } | sed -E "s/^$WRITE_TO//")"
    if [[ "$MUTATES" -eq 1 ]]; then
        _lwords="$_lwords"$'\n'"$(tr '<>|;&()=,`' '          ' <<<"$_ocmd" | tr -s ' \t' '\n')"
    fi
    shopt -s nocasematch
    while IFS= read -r _a; do
        unquote "$_a"
        _a="$UQ"
        case "$_a" in
            '' | -* | *'$'* | *'`'* | *'*'* | *'?'* | *'['* | '~'* | /dev/*) continue ;;
        esac
        _lw="$_a"
        # From the cwd's real path ($_creal), so only the word's own
        # components are walked; a cwd inside a link is CWD_REL's case.
        [[ "$_a" == /* || -n "$_creal" ]] || _a="$CWD/$_a"
        if ! ln_resolve "$_a" "$_creal"; then
            defer_ask "cannot resolve the symlinks in $_lw; confirm this command changes no protected path"
            break
        fi
        [[ "$LN_VIA" -eq 1 ]] || continue
        lprot_init
        for _p in "${_lprot[@]}"; do
            if [[ "$LN_OUT" == "$_p" || "$LN_OUT" == "$_p/"* ]] \
                || { [[ "$MUTATES" -eq 1 ]] && [[ "$LN_OUT" == / || "$_p" == "$LN_OUT/"* ]]; }; then
                defer_ask "this command appears to modify the protected path ${_p#"$_lroot"/} through the link $_lw; a human or a reviewed change makes that edit"
                break 2
            fi
        done
    done < <(awk 'NF && !seen[$0]++' <<<"$_lwords")
    shopt -u nocasematch
fi

# Secret files (#130): protect-files refuses Write/Edit of these; a command
# that names one (`cat .env`, `cp id_rsa x`) asks, since reading it puts
# the secret in the transcript. The names and allowlist are the ones
# pre-commit and pr-check refuse (lib/secrets.sh, #221), matched ignoring
# case; the project's lib, the one this hook was projected beside, or the
# one in the extension's own layout. Without one, a core name still names
# its file in the question, and any other command asks.
# gates_core_secret <path>: the core of the secret-file list, built in,
# for when lib/secrets.sh cannot load: environment files, private keys,
# certificates and key stores, as 0.4.0 blocked them. The rest of the list
# then asks. Called under nocasematch; base_name, not ${1##*/} (#233).
gates_core_secret() {
    base_name "$1"
    case "$BN" in
        *.example | *.sample | *.template) return 1 ;;
        .env | .env.* | id_rsa* | id_ed25519* | id_ecdsa* | *.pem | *.key | *.crt | *.p12 | *.pfx | *.jks | *.keystore)
            return 0
            ;;
    esac
    return 1
}
SECRETS_LIB=missing
for _sl in "$LROOT/.specify/gates/lib/secrets.sh" "${BASH_SOURCE[0]%/*}/../../../.specify/gates/lib/secrets.sh" \
    "${BASH_SOURCE[0]%/*}/../../lib/secrets.sh"; do
    [[ -f "$_sl" ]] || continue
    # shellcheck source=/dev/null disable=SC1090
    if "$BASH" -n "$_sl" 2>/dev/null && source "$_sl" 2>/dev/null \
        && command -v gates_forbidden_path >/dev/null 2>&1; then
        SECRETS_LIB=ok
    fi
    break
done
# A command that reads the contents of every file under a directory it
# names (#238): a recursive grep, rg/ag/ack (recursive by default), an
# archiver, scp, or xargs running a command on listed names. cp -r and
# rsync count as changes (MUTATES) already. Listings (ls -R, find, du,
# stat) read no contents. Like MUTATES, this holds for the whole command.
READ_TREE="$VERB_START"'((e|f)?grep[[:space:]]+([^;&|]*[[:space:]])?(-[a-z]*r[a-z]*|--recursive|--dereference-recursive|--directories(=|[[:space:]]+)recurse|-d[[:space:]]*recurse)|rg|ag|ack|tar|zip|7z|cpio|pax|scp|xargs)([[:space:]]|$)'
READS_TREE=0
grep -qE "$READ_TREE" <<<"$BCMD" && READS_TREE=1
if [[ "$SECRETS_LIB" == ok ]]; then
    SECRET_FILE=""
    shopt -s nocasematch
    while IFS= read -r _t; do
        if gates_forbidden_path "$_t"; then
            # A read-only command that names a sensitive directory
            # (`ls ~/.ssh`, `cat ~/.aws/config`) runs: only the directory
            # matched, not a secret name. A write there, a glob that could
            # read every key in it (`cat ~/.ssh/*`, #229), or a command that
            # reads every file under it (`grep -r x ~/.ssh/`, `tar cf -
            # ~/.ssh/`, #238) still asks.
            if [[ "$GATES_FORBIDDEN_WHAT" == *"sensitive directory" && "$MUTATES" -eq 0 \
                && "$WRITE_REDIRECT" -eq 0 && "$READS_TREE" -eq 0 && "$_t" != *[*?[]* ]]; then
                continue
            fi
            SECRET_FILE="$_t"
            break
        fi
    done < <(printf '%s\n' "$COMMAND" | tr '<>|;&()=,' '         ' | awk '
        { for (i = 1; i <= NF; i++) { t = $i; gsub(/["'"'"'`]/, "", t); if (t != "" && !seen[t]++) print t } }')
    shopt -u nocasematch
    if [[ -n "$SECRET_FILE" ]]; then
        defer_ask "this command names the secret file $SECRET_FILE (${GATES_FORBIDDEN_WHAT:-a forbidden name}); confirm it does not expose a credential"
    fi
else
    # nocasematch, not a tr per word: a fork per word costs seconds on a
    # large command (#233).
    shopt -s nocasematch
    while IFS= read -r _t; do
        if gates_core_secret "$_t"; then
            defer_ask "this command names the secret file $_t; confirm it does not expose a credential"
            break
        fi
    done < <(printf '%s\n' "$COMMAND" | tr '<>|;&()=,' '         ' | awk '
        { for (i = 1; i <= NF; i++) { t = $i; gsub(/["'"'"'`]/, "", t); if (t != "" && !seen[t]++) print t } }')
    shopt -u nocasematch
    defer_ask "lib/secrets.sh cannot load, so this command cannot be checked for secret files; run /speckit.gates.doctor"
fi

# Project-owned rules (#71) run once every shipped rule allowed, so they
# can add a refusal but never remove one. They run before any "ask": a
# project refusal is stronger than a question.
if compgen -G "$LROOT/.specify/gates/hooks.local.d/validate-bash/*.sh" >/dev/null; then
    LLIB="$LROOT/.specify/gates/lib/local-hooks.sh"
    if [[ ! -f "$LLIB" ]] || ! "$BASH" -n "$LLIB" 2>/dev/null; then
        ask "local rules exist in hooks.local.d/validate-bash, but lib/local-hooks.sh cannot load; run /speckit.gates.doctor"
    fi
    # shellcheck source=/dev/null disable=SC1090
    source "$LLIB"
    if ! GATES_LOCAL_STDIN="$INPUT" gates_run_local "$LROOT" validate-bash; then
        echo "BLOCKED: $GATES_LOCAL_MSG" >&2
        echo "Command: $COMMAND" >&2
        exit 2
    fi
fi

[[ -n "$ASK" ]] && ask "$ASK"

if [[ -n "$DEGRADED" ]]; then
    echo "gates: validate-bash checked in raw mode ($DEGRADED); run /speckit.gates.doctor" >&2
fi
exit 0
