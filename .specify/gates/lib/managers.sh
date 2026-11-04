#!/usr/bin/env bash
# managers.sh -- the git boundary as git actually runs it (#74).
#
# Usage (sourced; bash 3.2):
#   gates_hooks_dir <root>          # the directory git runs hooks from
#   gates_git_probe <root> <hook>   # run it: 0 = a gates refusal reaches
#                                   # git; else 1, with GATES_PROBE_MSG set
#   gates_hook_owner <root> <hook>  # gates | absent | other
#   gates_hook_static <root> <hook> # read it: is the call-through there?
#   gates_git_check <root> <hook> <probe:0|1>
#                                   # probe when gates owns the hook (or on
#                                   # request), static otherwise
#
# The probe runs the hook git would run (honoring core.hooksPath and linked
# worktrees) with GATES_PROBE=1 and a throwaway message file. The projected
# gates hooks answer with `gates-probe:<hook>:<version>` on stderr before
# reading any policy, so the marker proves the whole call chain -- a plain
# stub, husky, lefthook, the pre-commit framework, or a custom script --
# reaches gates, whichever rules the policy turns on or off. In probe mode
# the gates hook also refuses (exit 1), and the hook git runs must then
# exit non-zero too (#202): git refuses a commit exactly when that status
# is non-zero, so the probe proves a refusal reaches git, not only that the
# hook was reached. It runs the hook file rather than `git commit`, which
# would commit when the chain is broken. A hook that is missing, not
# executable (git skips it silently), never prints the marker, or exits 0
# fails the probe.

# shellcheck disable=SC2034   # library file; GATES_PROBE_MSG is read by callers
GATES_PROBE_MSG=""

gates_hooks_dir() { # <root>
    local d
    d="$(git -C "$1" rev-parse --git-path hooks 2>/dev/null)" || return 1
    [[ "$d" == /* ]] || d="$1/$d"
    printf '%s\n' "$d"
}

gates_git_probe() { # <root> <hook>
    local root="$1" hook="$2" dir f msg out
    GATES_PROBE_MSG=""
    if ! dir="$(gates_hooks_dir "$root")"; then
        GATES_PROBE_MSG="not a git work tree"
        return 1
    fi
    f="$dir/$hook"
    if [[ ! -e "$f" ]]; then
        GATES_PROBE_MSG="${f#"$root"/} does not exist, so git runs no $hook hook"
        return 1
    fi
    if [[ ! -x "$f" ]]; then
        GATES_PROBE_MSG="${f#"$root"/} is not executable, so git skips it"
        return 1
    fi
    # Call the hook the way git does (#127): commit-msg gets the message
    # file, pre-commit and pre-merge-commit get no arguments (the
    # pre-commit framework's hook refuses any). lefthook's generated hook
    # passes its arguments on to `lefthook run`; it gets `--job <gates
    # job>` so only the gates job runs (#167: --force ran every job,
    # and a `--fix {staged_files}` job rewrote files). Without --force the
    # probe also proves the job runs while nothing is staged.
    local -a args=()
    msg=""
    if [[ "$hook" == "commit-msg" ]]; then
        msg="$(mktemp 2>/dev/null || mktemp -t gates-probe)" || {
            GATES_PROBE_MSG="cannot create a probe message file"
            return 1
        }
        printf 'chore: gates probe\n' >"$msg"
        args=("$msg")
    fi
    if grep -qs 'lefthook' "$f"; then
        gates_manager_wired "$root" lefthook "$hook" || true
        if [[ "$GATES_WIRED_JOB" == "-" ]]; then
            [[ -n "$msg" ]] && rm -f "$msg"
            GATES_PROBE_MSG="the lefthook job that calls the gates $hook hook has no name:, so the probe cannot run it alone (name it spec-gates)"
            return 1
        fi
        args+=(--job "${GATES_WIRED_JOB:-spec-gates}")
    fi
    local rc=0
    out="$(cd "$root" && GATES_PROBE=1 "$f" ${args[@]+"${args[@]}"} 2>&1 </dev/null)" || rc=$?
    [[ -n "$msg" ]] && rm -f "$msg"
    if grep -q "gates-probe:$hook:" <<<"$out"; then
        # The gates hook refused; git refuses the commit only when that
        # reaches the exit status of the hook git runs (#202).
        [[ "$rc" -ne 0 ]] && return 0
        GATES_PROBE_MSG="git runs ${f#"$root"/} and it reaches the gates $hook hook, but it exits 0 although the gates hook refused, so git would not refuse the commit (a || true, &, or a later command masks the status)"
        return 1
    fi
    if grep -q 'no matching staged files\|no files for inspection' <<<"$out"; then
        GATES_PROBE_MSG="git runs ${f#"$root"/}, but lefthook skips the gates job while nothing is staged, so empty commits and amends pass (give it files: and {files} as in the entry project.sh prints)"
        return 1
    fi
    if grep -q 'no job matching' <<<"$out"; then
        GATES_PROBE_MSG="git runs ${f#"$root"/}, but lefthook has no $hook job named '${GATES_WIRED_JOB:-spec-gates}' that calls the gates hook"
        return 1
    fi
    GATES_PROBE_MSG="git runs ${f#"$root"/}, but it does not reach the gates $hook hook (no probe answer)"
    return 1
}

# Who owns the hook git runs (#74): "gates" when it is the gates stub or a
# copied gates hook (the whole chain is gates code), "absent" when there is
# none, "other" for anything else (husky, lefthook, the pre-commit
# framework, a custom script).
gates_hook_owner() { # <root> <hook>
    local dir f
    dir="$(gates_hooks_dir "$1")" || { echo absent; return 0; }
    f="$dir/$2"
    if [[ ! -e "$f" ]]; then
        echo absent
    elif grep -q 'spec-gates hook stub\|Git commit-msg hook\.\|Git pre-commit hook --' "$f" 2>/dev/null; then
        echo gates
    else
        echo other
    fi
}

# A call-through counts only in a form whose failure refuses the commit
# (#202): the gates hook as a whole command, optionally behind `exec` and
# `bash`/`sh`, its path optionally led by a quote, `./`, a `$(...)` or
# `$VAR` directory and a relative path; then only plain arguments and
# redirections, an `|| exit` (bare, `$?` or a non-zero status; gates_call
# then returns 2: the failure ends the script), and at most a trailing
# comment. `|| true`, `&`, a pipe, `;` and a leading `true ||`, `echo` or
# `:` all fail it, as does a call that sits in a shell comment. gates_unq
# strips YAML/TOML quoting (and
# the comment after an unquoted value) from a configuration value.
GATES_CALL_AWK='
    function gates_unq(v,   q, i, c, out) {
        sub(/^[ \t]+/, "", v)
        q = substr(v, 1, 1)
        if (q != "\"" && q != "\047") { sub(/[ \t]+#.*$/, "", v); sub(/[ \t]+$/, "", v); return v }
        out = ""
        for (i = 2; i <= length(v); i++) {
            c = substr(v, i, 1)
            if (c == q) return out
            if (c == "\\" && q == "\"") { i++; c = substr(v, i, 1); if (c != "\"" && c != "\\") c = "\\" c }
            out = out c
        }
        return ";"
    }
    function gates_call(s, needle,   p, pre, post) {
        p = index(s, needle)
        if (p == 0) return 0
        pre = substr(s, 1, p - 1); post = substr(s, p + length(needle))
        sub(/^[ \t]+/, "", pre)
        if (pre !~ /^(exec[ \t]+)?((\/usr\/bin\/env[ \t]+|\/bin\/)?(bash|sh)[ \t]+)?["\047]?((\$\([^()|&;]*\)|\$\{?[A-Za-z_][A-Za-z0-9_]*\}?)?[^ \t|&;()"\047$`<>]*\/)?$/) return 0
        if (post ~ /^["\047]?([ \t]+[^|&;#`()]*)?([ \t]+#.*)?$/) return 1
        if (post ~ /^["\047]?([ \t]+[^|&;#`()]*)?[ \t]*\|\|[ \t]*exit([ \t]+(\$\?|[1-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5]))?[ \t]*([ \t]#.*)?$/) return 2
        return 0
    }
'

# Does the shell script <file> call the projected gates <hook> on a line
# that can run and whose failure refuses the commit (#128, #202)?
# Commented lines do not count, and nothing after a top-level line that
# ends the script does (#167): an unconditional `exit`, an `exec <command>`
# (it replaces the shell; `exec >log` only redirects), or a one-line
# `if ...; then exit`. An indented `exit` sits inside a block and does not
# end the scan. The call must be a top-level line (an indented one is
# conditional) in the form GATES_CALL_AWK accepts, and its status must be
# the script's: with <errexit> (husky runs its scripts under `sh -e`),
# under `set -e` or a `-e` shebang, behind `exec`, followed by `|| exit`,
# or as the last command. Other conditional exits (`[ -n "$CI" ] && exit
# 0`, a multi-line if) are not recognized, and --probe-git is the proof
# for them. When the call is there but does not count, GATES_WIRED_WHY
# says why, as a phrase that follows the file's name.
gates_calls_through() { # <file> <hook> [errexit]
    local out
    GATES_WIRED_WHY=""
    [[ -f "$1" ]] || return 1
    out="$(awk -v needle=".specify/gates/hooks/$2" -v ee="${3:+1}" "$GATES_CALL_AWK"'
        NR == 1 && /^#!/ { if ($0 ~ /[ \t]-[a-zA-Z]*e/) ee = 1; next }
        /^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
        cand { cand = 0; why = "tail\t" candline }
        /^set[ \t]+(-[a-zA-Z]*e|-o[ \t]+errexit)/ { ee = 1 }
        /^set[ \t]+(\+[a-zA-Z]*e|\+o[ \t]+errexit)/ { ee = 0 }
        /^exit([[:space:];]|$)/ { exit }
        index($0, needle) {
            r = ($0 ~ /^[^ \t]/) ? gates_call($0, needle) : 0
            if (r) {
                if (ee || r == 2 || $0 ~ /^exec[ \t]/) { found = 1; exit }
                cand = 1; candline = $0; next
            }
            if (why == "") why = "form\t" $0
            next
        }
        /^exec[[:space:]]+[^[:space:]<>&0-9]/ { exit }
        /^if[[:space:]].*;[[:space:]]*then[[:space:]]+exit([[:space:];]|$)/ { exit }
        END { print ((found || cand) ? "ok" : why) }
    ' "$1")"
    case "$out" in
        ok) return 0 ;;
        form*) GATES_WIRED_WHY="calls .specify/gates/hooks/$2 only as \`${out#*$'\t'}\`, which cannot refuse the commit (call it as a whole command on its own top-level line, not behind || true, &, a pipe, echo, : or a comment)" ;;
        tail*) GATES_WIRED_WHY="calls .specify/gates/hooks/$2 as \`${out#*$'\t'}\`, but more commands follow and the script does not stop on a failure, so their status replaces its refusal (make it the last command, use exec, or add set -e)" ;;
    esac
    return 1
}

# --- Manager configuration, read per hook (#167) ----------------------------
#
# A call-through counts only where the manager runs it for that hook: in
# lefthook, a job under that hook's key that is not skipped and runs while
# nothing is staged (pre-commit); in the pre-commit framework, an item whose
# stages include the hook and that runs without matching files. Each reader
# sets GATES_WIRED_WHY when the call-through is there but does not count,
# and the lefthook reader sets GATES_WIRED_JOB to the job's name (the probe
# runs only that job).
GATES_WIRED_WHY=""
GATES_WIRED_JOB=""

# The files lefthook 2.x reads its configuration from, in the order gates
# looks for them; the -local variants (same names, lefthook-local.*) are
# merged into it.
GATES_LEFTHOOK_CONFIGS="lefthook.yml .lefthook.yml lefthook.yaml .lefthook.yaml lefthook.toml .lefthook.toml lefthook.json .lefthook.json lefthook.jsonc .lefthook.jsonc .config/lefthook.yml .config/lefthook.yaml .config/lefthook.toml .config/lefthook.json .config/lefthook.jsonc"

gates_lefthook_config() { # <root> [local] -> the relative path of the config
    local f
    for f in $GATES_LEFTHOOK_CONFIGS; do
        [[ -n "${2:-}" ]] && f="${f/lefthook./lefthook-local.}"
        [[ -f "$1/$f" ]] && { printf '%s\n' "$f"; return 0; }
    done
    return 1
}

# The verdict on one hook's jobs, from any of the three formats: "ok <job>",
# "staged <job>" (lefthook skips it while nothing is staged), "skip <job>"
# (skip:/only: set on the job or the hook, or the hook's exclude_tags:
# names the job or one of its tags), "form <job>" (its run: is not the
# gates hook as a whole command, #202), "elsewhere" (the call-through sits
# under another key), or "none".
#
# YAML: the block runs from the hook's top-level key (quoted or not) to the
# next top-level line; a job is a key under commands: or an item under
# jobs:. Flow-style YAML ({...}) is not read. A skip:/only: anywhere in a
# job (or at the hook's level) counts as skipping it, and so does an
# exclude_tags: list that is not closed on its line.
GATES_LEFTHOOK_AWK_JUDGE="$GATES_CALL_AWK"'
    function truthy(s, key) {
        if (s !~ ("^" key "[ \t]*:")) return 0
        sub("^" key "[ \t]*:[ \t]*", "", s)
        sub(/[ \t]*(#.*)?$/, "", s)
        gsub(/["\047]/, "", s)
        return s != "false"
    }
    # A tag list ([a, b], a b, or one item of a block list) as " a b ".
    # An unclosed [ list is not read: " * " stands for "any tag".
    function toks(s,   n, a, i, out) {
        sub(/[ \t]#.*$/, "", s)
        if (index(s, "[") && !index(s, "]")) return " *"
        gsub(/[][,"\047]/, " ", s)
        n = split(s, a, /[ \t]+/); out = ""
        for (i = 1; i <= n; i++) if (a[i] != "") out = out " " a[i]
        return out
    }
    function close_job(   v) {
        if (jcalls) {
            v = "ok"
            if (!jprov) v = "form"
            else if (jskip) v = "skip"
            else if (hook == "pre-commit" && (jfilter || !(jall || (jfiles && jtmpl)))) v = "staged"
            nj++; jv[nj] = v; jn[nj] = (jname == "" ? "-" : jname); jt[nj] = jtags " " jname " "
        }
        jname = ""; jcalls = 0; jprov = 0; jskip = 0; jfiles = 0; jtmpl = 0; jall = 0; jfilter = 0; jtags = ""; jlast = ""
    }
    function prop(key, val) {
        if (index(val, needle)) jcalls = 1
        if (key == "run" && gates_call(gates_unq(val), needle)) jprov = 1
        if (index(val, "{files}")) jtmpl = 1
        if (index(val, "{all_files}")) jall = 1
        if (truthy(key ":" val, "skip") || truthy(key ":" val, "only")) jskip = 1
        if (key == "files") jfiles = 1
        if (key == "glob" || key == "file_types" || key == "exclude") jfilter = 1
        if (key == "name" && jname == "") { gsub(/["\047]/, "", val); sub(/[ \t]*(#.*)?$/, "", val); sub(/^[ \t]*/, "", val); jname = val }
        if (key == "tags" || (key == "" && jlast == "tags")) jtags = jtags toks(val)
        if (key != "") jlast = key
    }
    # Hook-level skip:/only:/exclude_tags: may follow the jobs, so they
    # are applied last.
    function excluded(i,   n, a, k) {
        if (hex !~ /[^ ]/) return 0
        if (index(hex " ", " * ") || index(jt[i], " * ")) return 1
        n = split(hex, a, /[ \t]+/)
        for (k = 1; k <= n; k++) if (a[k] != "" && index(" " jt[i] " ", " " a[k] " ")) return 1
        return 0
    }
    function verdict(   i, v, best, bestv) {
        best = 0; bestv = "none"
        for (i = 1; i <= nj; i++) {
            v = jv[i]
            if (v != "form" && (hskip || excluded(i))) v = "skip"
            if (rank[v] > best) { best = rank[v]; bestv = v " " jn[i] }
        }
        if (best == 0 && other) bestv = "elsewhere"
        return bestv
    }
    BEGIN { rank["ok"] = 4; rank["staged"] = 3; rank["skip"] = 2; rank["form"] = 1 }
'
gates_lefthook_yaml_verdict() { # <file> <hook>
    awk -v hook="$2" -v needle=".specify/gates/hooks/$2" "$GATES_LEFTHOOK_AWK_JUDGE"'
        function ind(s) { match(s, /^ */); return RLENGTH }
        function yprop(s,   k, v) {
            sub(/^ *(- +)?/, "", s)
            k = s; v = s
            if (s ~ /^[A-Za-z_"\047-][^:]*:/) { sub(/[ \t]*:.*$/, "", k); gsub(/["\047]/, "", k); sub(/^[^:]*:/, "", v) } else k = ""
            prop(k, v)
        }
        /^[ \t]*#/ || /^[ \t]*$/ { next }
        /^[^ ]/ {
            if (inb) { close_job(); inb = 0 }
            k = $0; sub(/[ \t]*:.*$/, "", k); gsub(/["\047]/, "", k)
            if (k == hook) { inb = 1; child = -1; mode = ""; jind = -1 }
            else if (index($0, needle)) other = 1
            next
        }
        !inb { if (index($0, needle)) other = 1; next }
        {
            i = ind($0); s = $0; sub(/^ */, "", s)
            if (child < 0) child = i
            if (mode == "excl" && s ~ /^-/) { sub(/^- */, "", s); hex = hex toks(s); next }
            if (i <= child && !(mode == "jobs" && s ~ /^-/ && (jind < 0 || jind == i))) {
                close_job(); mode = ""; jind = -1
                if (truthy(s, "skip") || truthy(s, "only")) hskip = 1
                if (s ~ /^commands[ \t]*:/) mode = "commands"
                else if (s ~ /^jobs[ \t]*:/) mode = "jobs"
                else if (s ~ /^exclude_tags[ \t]*:/) {
                    sub(/^[^:]*:/, "", s)
                    if (s ~ /^[ \t]*(#.*)?$/) mode = "excl"; else hex = hex toks(s)
                }
                next
            }
            if (mode == "" || mode == "excl") next
            if (jind < 0) jind = i
            if (i == jind) {
                close_job()
                if (mode == "commands") { n = s; sub(/[ \t]*:.*$/, "", n); gsub(/["\047]/, "", n); jname = n }
                else yprop(s)
                next
            }
            yprop(s)
        }
        END {
            if (inb) close_job()
            print verdict()
        }
    ' "$1"
}

# TOML: a job is a [<hook>.commands.<name>] table or a [[<hook>.jobs]]
# entry; [<hook>] itself holds the hook-level keys. Inline tables are not
# read.
gates_lefthook_toml_verdict() { # <file> <hook>
    awk -v hook="$2" -v needle=".specify/gates/hooks/$2" "$GATES_LEFTHOOK_AWK_JUDGE"'
        /^[ \t]*#/ || /^[ \t]*$/ { next }
        /^[ \t]*\[/ {
            close_job()
            h = $0; sub(/^[ \t]*\[+/, "", h); sub(/\]+[ \t]*(#.*)?$/, "", h); gsub(/["\047 \t]/, "", h)
            n = split(h, part, ".")
            sect = ""
            if (part[1] == hook) {
                if (n == 1) sect = "hook"
                else if (part[2] == "commands" && n >= 3) { sect = "job"; jname = part[3] }
                else if (part[2] == "jobs") { sect = "job" }
            }
            next
        }
        {
            if (sect == "") { if (index($0, needle)) other = 1; next }
            k = $0; v = $0
            sub(/^[ \t]*/, "", k); sub(/[ \t]*=.*$/, "", k); gsub(/["\047]/, "", k)
            sub(/^[^=]*=[ \t]*/, "", v)
            if (sect == "hook") {
                if ((k == "skip" || k == "only") && truthy(k ":" v, k)) hskip = 1
                if (k == "exclude_tags") hex = hex toks(v)
                next
            }
            prop(k, v)
        }
        END {
            close_job()
            print verdict()
        }
    ' "$1"
}

# JSON (and JSONC with whole-line // comments): read with jq, which lists
# each job that calls the gates hook with its verdict and run:; awk judges
# the run: form (#202) and picks the best job.
gates_lefthook_json_verdict() { # <file> <hook>
    local jobs
    command -v jq >/dev/null 2>&1 || { echo unreadable; return 0; }
    jobs="$(grep -v '^[[:space:]]*//' "$1" | jq -r --arg h "$2" --arg n ".specify/gates/hooks/$2" '
        def truthy: . != null and . != false and . != "false";
        def calls: (tostring | contains($n));
        def list: if . == null then [] elif type == "array" then map(tostring) else tostring | split(" ") end;
        . as $root
        | (.[$h] // {}) as $b
        | ($b.exclude_tags | list) as $ex
        | [($b.commands // {} | to_entries[] | .value + {name: .key}), ($b.jobs // [])[]]
        | map(select(calls)
              | [(if (.skip | truthy) or (.only | truthy) or ($b.skip | truthy) or ($b.only | truthy)
                     or (((.tags | list) + [.name // ""]) as $t | ($t - ($t - $ex)) | length > 0) then "skip"
                  elif $h == "pre-commit" and ((.glob != null) or (.file_types != null) or (.exclude != null)
                      or ((((.run // "") | contains("{all_files}")) or (.files != null and ((.run // "") | contains("{files}")))) | not))
                  then "staged" else "ok" end),
                 (.name // "-"), ((.run // "") | tostring | gsub("[\t\r\n]"; ";"))] | join("\t"))
        | if length > 0 then .[]
          elif ($root | del(.[$h]) | calls) then "elsewhere"
          else "none" end
    ' 2>/dev/null)" || { echo unreadable; return 0; }
    awk -F '\t' -v needle=".specify/gates/hooks/$2" "$GATES_CALL_AWK"'
        BEGIN { rank["ok"] = 4; rank["staged"] = 3; rank["skip"] = 2; rank["form"] = 1 }
        NF < 3 { bestv = $0; next }
        {
            v = $1
            if (!gates_call($3, needle)) v = "form"
            if (rank[v] > best) { best = rank[v]; bestv = v " " $2 }
        }
        END { print (bestv == "" ? "unreadable" : bestv) }
    ' <<<"$jobs"
}

# Is the gates <hook> wired in lefthook's configuration <file> (relative)?
gates_lefthook_wired() { # <root> <file> <hook>
    local f="$1/$2" v
    GATES_WIRED_WHY="" GATES_WIRED_JOB=""
    [[ -f "$f" ]] || return 1
    case "$2" in
        *.toml) v="$(gates_lefthook_toml_verdict "$f" "$3")" ;;
        *.json | *.jsonc) v="$(gates_lefthook_json_verdict "$f" "$3")" ;;
        *) v="$(gates_lefthook_yaml_verdict "$f" "$3")" ;;
    esac
    GATES_WIRED_JOB="${v#* }"
    case "$v" in
        ok\ *) return 0 ;;
        staged\ *) GATES_WIRED_WHY="$2 calls .specify/gates/hooks/$3 in job '$GATES_WIRED_JOB', but lefthook skips that job while nothing is staged (glob/files filters, or no {files}/{all_files} in run), so empty commits, amends and concluded merges pass; use the entry project.sh prints" ;;
        skip\ *) GATES_WIRED_WHY="$2 calls .specify/gates/hooks/$3 in job '$GATES_WIRED_JOB', but skip:/only: is set on the job or the $3 hook, or the hook's exclude_tags: names the job or its tags" ;;
        form\ *) GATES_WIRED_WHY="$2 calls .specify/gates/hooks/$3 in job '$GATES_WIRED_JOB', but its run: is not the gates hook as a whole command (run: \"bash .specify/gates/hooks/$3 ...\", not behind || true, &, a pipe, echo or a shell comment), so its refusal may never reach git" ;;
        elsewhere) GATES_WIRED_WHY="$2 calls .specify/gates/hooks/$3 only outside its $3: block, so lefthook never runs it for $3" ;;
        unreadable) GATES_WIRED_WHY="$2 could not be read (jq missing or not plain JSON); doctor --probe-git runs the chain" ;;
    esac
    # The job a staged/skip/form verdict names stays set: the probe runs it.
    [[ "$v" == *" "* ]] || GATES_WIRED_JOB=""
    return 1
}

# Is the gates <hook> wired in .pre-commit-config.yaml? An item counts when
# its entry: is the gates hook as a whole command (#202), its stages (or default_stages) include
# the hook -- an item with neither counts for pre-commit only -- and,
# except for commit-msg, always_run: true makes it run with no files
# staged. Legacy stage names (commit, merge-commit) are understood;
# flow-style items are not read.
gates_precommit_wired() { # <root> <hook>
    local f="$1/.pre-commit-config.yaml" v
    GATES_WIRED_WHY="" GATES_WIRED_JOB=""
    [[ -f "$f" ]] || return 1
    v="$(awk -v hook="$2" -v needle=".specify/gates/hooks/$2" "$GATES_CALL_AWK"'
        function ind(s) { match(s, /^ */); return RLENGTH }
        function stages(s,   n, a, i, x, out) {
            gsub(/[][,"\047]/, " ", s); sub(/#.*$/, "", s)
            n = split(s, a, /[ \t]+/); out = " "
            for (i = 1; i <= n; i++) {
                x = a[i]; if (x == "") continue
                if (x == "commit") x = "pre-commit"
                if (x == "merge-commit") x = "pre-merge-commit"
                out = out x " "
            }
            return out
        }
        function close_item(   st, v) {
            if (icalls) {
                st = istages; if (!ihas) st = defst
                if (st == "" ) st = " pre-commit "
                v = "ok"
                if (index(st, " " hook " ") == 0) v = "stage"
                else if (!iprov) v = "form"
                else if (hook != "commit-msg" && !ialways) v = "always"
                if (rank[v] > best) { best = rank[v]; bestv = v }
            }
            icalls = 0; iprov = 0; istages = ""; ihas = 0; ialways = 0; slist = 0
        }
        BEGIN { rank["ok"] = 4; rank["always"] = 3; rank["form"] = 2; rank["stage"] = 1; best = 0; bestv = "none"; hind = -1 }
        /^[ \t]*#/ || /^[ \t]*$/ { next }
        {
            i = ind($0); s = $0; sub(/^ */, "", s)
            if (i == 0 && s ~ /^default_stages[ \t]*:/) {
                v = s; sub(/^[^:]*:/, "", v)
                if (v ~ /[^ \t]/) defst = stages(v); else dlist = 1
                next
            }
            if (dlist) { if (s ~ /^-/) { sub(/^- */, "", s); defst = (defst == "" ? " " : defst) substr(stages(s), 2); next } dlist = 0 }
            if (hind >= 0) {
                if (i < hind || (i == hind && !(s ~ /^-/ && (iind < 0 || iind == i)))) { close_item(); hind = -1 }
                else if (s ~ /^-/ && (iind < 0 || i == iind)) { close_item(); iind = i; sub(/^- */, "", s) }
                else if (slist && s ~ /^-/ && i >= sind) { sub(/^- */, "", s); istages = (istages == "" ? " " : istages) substr(stages(s), 2); next }
                else slist = 0
                if (hind >= 0) {
                    if (index(s, needle)) icalls = 1
                    if (s ~ /^entry[ \t]*:/) { v = s; sub(/^[^:]*:/, "", v); if (gates_call(gates_unq(v), needle)) iprov = 1 }
                    if (s ~ /^stages[ \t]*:/) {
                        ihas = 1; v = s; sub(/^[^:]*:/, "", v)
                        if (v ~ /[^ \t]/) istages = stages(v); else { slist = 1; sind = i }
                    }
                    if (s ~ /^always_run[ \t]*:[ \t]*true/) ialways = 1
                    next
                }
            }
            if (s ~ /^(- +)?hooks[ \t]*:/) { hind = i; if (s ~ /^-/) hind = i + 2; iind = -1 }
        }
        END { if (hind >= 0) close_item(); print bestv }
    ' "$f")"
    case "$v" in
        ok) return 0 ;;
        stage) GATES_WIRED_WHY=".pre-commit-config.yaml calls .specify/gates/hooks/$2, but not in an item whose stages: include $2" ;;
        form) GATES_WIRED_WHY=".pre-commit-config.yaml calls .specify/gates/hooks/$2, but the item's entry: is not the gates hook itself (entry: bash .specify/gates/hooks/$2), so its refusal may never reach git" ;;
        always) GATES_WIRED_WHY=".pre-commit-config.yaml calls .specify/gates/hooks/$2, but the item lacks always_run: true, so pre-commit skips it when no files match (empty commits and amends pass)" ;;
    esac
    return 1
}

# Does <file> end the script with a top-level `exit`? A line appended after
# it would never run.
gates_has_toplevel_exit() { # <file>
    [[ -f "$1" ]] && grep -qE '^exit([[:space:];]|$)' "$1"
}

# Which manager runs the hook git runs (#167), judged from that file: husky
# (core.hooksPath under .husky), lefthook or the pre-commit framework (their
# generated scripts say so), else "script" (a custom hook, read itself).
gates_hook_manager() { # <root> <hook>
    local dir hp
    hp="$(git -C "$1" config core.hooksPath 2>/dev/null || true)"
    case "$hp" in
        *.husky*) echo husky; return 0 ;;
    esac
    dir="$(gates_hooks_dir "$1" 2>/dev/null)" || dir=""
    if [[ -n "$dir" ]] && grep -qs 'File generated by pre-commit' "$dir/$2"; then
        echo pre-commit
    elif [[ -n "$dir" ]] && grep -qs 'lefthook' "$dir/$2"; then
        echo lefthook
    else
        echo script
    fi
}

# The gates stub under the pre-commit framework (#201). `pre-commit install`
# moves the hook it finds to <hook>.legacy and, unless installed with -f
# (which deletes it), runs that file first on every call when it is
# executable, failing the hook if it fails. "current" when that file is a
# gates stub that runs the gates hook under its moved name, "stale" for an
# older stub, which refuses every commit there, "none" otherwise (no
# framework hook, no executable .legacy, or not a gates stub).
gates_legacy_stub() { # <root> <hook>
    local dir f
    dir="$(gates_hooks_dir "$1" 2>/dev/null)" || { echo none; return 0; }
    f="$dir/$2.legacy"
    # shellcheck disable=SC2016  # the stub's line, matched literally
    if ! grep -qs 'File generated by pre-commit' "$dir/$2" || [[ ! -x "$f" ]] \
        || ! grep -qs 'spec-gates hook stub' "$f"; then
        echo none
    elif grep -qs '^name="${name%\.legacy}"' "$f"; then
        echo current
    else
        echo stale
    fi
}

# Static check for a hook another tool owns: is the gates call-through in
# the file that tool reads for this hook? Running such a hook would also
# run that tool's own steps (husky's default pre-commit is `npm test`), with
# side effects and, under `sh -e`, a failing step that hides the gates
# line. So by default the chain is read, not run. Only the configuration of
# the manager that runs the hook counts (#167): a stale config of another
# manager calling gates does not.
gates_hook_static() { # <root> <hook>
    local root="$1" hook="$2" dir needle rel mgr
    GATES_PROBE_MSG=""
    needle=".specify/gates/hooks/$hook"
    dir="$(gates_hooks_dir "$root")" || { GATES_PROBE_MSG="not a git work tree"; return 1; }
    # The file git runs must be executable, or git skips it silently
    # (#159: husky 8 runs .husky/<hook> itself, via core.hooksPath=.husky).
    # The files a manager reads (.husky/<hook> under husky 9, the YAML
    # configs) need no execute bit.
    if [[ -e "$dir/$hook" && ! -x "$dir/$hook" ]]; then
        GATES_PROBE_MSG="${dir#"$root"/}/$hook is not executable, so git skips it (fix: chmod +x ${dir#"$root"/}/$hook)"
        return 1
    fi
    mgr="$(gates_hook_manager "$root" "$hook")"
    if [[ "$mgr" == "pre-commit" ]]; then
        case "$(gates_legacy_stub "$root" "$hook")" in
            current) return 0 ;;
            stale)
                GATES_PROBE_MSG="git runs ${dir#"$root"/}/$hook, owned by pre-commit, which first runs ${dir#"$root"/}/$hook.legacy: an older gates stub that refuses every commit under that name (fix: re-run project.sh to refresh it)"
                return 1
                ;;
        esac
    fi
    case "$mgr" in
        husky | lefthook | pre-commit)
            gates_manager_wired "$root" "$mgr" "$hook" && return 0
            rel="$(gates_manager_file "$root" "$mgr" "$hook")"
            GATES_PROBE_MSG="git runs ${dir#"$root"/}/$hook, owned by $mgr, and ${GATES_WIRED_WHY:-$rel does not call $needle for $hook}"
            ;;
        *)
            gates_calls_through "$dir/$hook" "$hook" && return 0
            if [[ -n "$GATES_WIRED_WHY" ]]; then
                GATES_PROBE_MSG="git runs ${dir#"$root"/}/$hook, owned by another tool, and it $GATES_WIRED_WHY"
            else
                GATES_PROBE_MSG="git runs ${dir#"$root"/}/$hook, owned by another tool, and it does not call $needle on a line that runs"
            fi
            ;;
    esac
    return 1
}

# The check doctor and project.sh run per hook: the behavioral probe when
# gates owns the hook (only gates code runs) or when asked (<probe>=1);
# otherwise the static check. GATES_CHECK_KIND says which one ran.
GATES_CHECK_KIND=""
gates_git_check() { # <root> <hook> <probe:0|1>
    local owner
    owner="$(gates_hook_owner "$1" "$2")"
    if [[ "$owner" == "other" && "${3:-0}" != "1" ]]; then
        GATES_CHECK_KIND=static
        gates_hook_static "$1" "$2"
        return
    fi
    GATES_CHECK_KIND=probe
    gates_git_probe "$1" "$2"
}

# --- Hook managers (#74b) ---------------------------------------------------
#
# A hook manager owns the git hooks: it generates the files git runs and
# reads its own, user-owned configuration. gates never edits the generated
# files (the next `husky`, `lefthook install` or `pre-commit install`
# rewrites them, silently dropping a call-through); it adds its entry to the
# configuration the manager reads, and only on request (--wire-manager).
#
#   gates_detect_manager <root>          # husky | lefthook | pre-commit | unknown
#                                        # (another core.hooksPath) | plain
#   gates_manager_file <root> <manager> <hook>   # the user-owned file (relative)
#   gates_manager_wired <root> <manager> <hook>  # 0 if the entry is there
#   gates_manager_entry <manager> <hook> [file]  # the entry, as text
#   gates_manager_apply <root> <manager> <hook>  # append it; 1 if unsafe
#   gates_manager_install_hint <manager> <hook>  # the manager's install command

gates_detect_manager() { # <root>
    local root="$1" hp dir
    hp="$(git -C "$root" config core.hooksPath 2>/dev/null || true)"
    case "$hp" in
        *.husky*) echo husky; return 0 ;;
    esac
    dir="$(gates_hooks_dir "$root" 2>/dev/null)" || dir=""
    if gates_lefthook_config "$root" >/dev/null; then
        echo lefthook
        return 0
    fi
    if [[ -n "$dir" ]] && grep -qs 'lefthook' "$dir/pre-commit" "$dir/commit-msg"; then
        echo lefthook
        return 0
    fi
    if [[ -f "$root/.pre-commit-config.yaml" ]] \
        || { [[ -n "$dir" ]] && grep -qs 'File generated by pre-commit' "$dir/pre-commit" "$dir/commit-msg"; }; then
        echo pre-commit
        return 0
    fi
    # Another core.hooksPath owns every hook. A single custom script in the
    # hooks directory owns only that hook, so it is judged per hook by the
    # caller, and the repo stays "plain".
    if [[ -n "$hp" ]]; then
        echo unknown
        return 0
    fi
    echo plain
}

# lefthook: the configuration lefthook reads, in any of its formats
# (#167), or lefthook.yml when there is none.
gates_manager_file() { # <root> <manager> <hook>
    case "$2" in
        husky) printf '.husky/%s\n' "$3" ;;
        lefthook) gates_lefthook_config "$1" || printf 'lefthook.yml\n' ;;
        pre-commit) printf '.pre-commit-config.yaml\n' ;;
        *) return 1 ;;
    esac
}

# Wired means the manager runs the call-through for <hook> (#167): husky's
# .husky/<hook> script calls it on a line that runs; lefthook's config (or
# its -local file) has a job under the hook's key that runs; the pre-commit
# framework has an item staged for the hook. GATES_WIRED_WHY says why a
# call-through that is there does not count.
gates_manager_wired() { # <root> <manager> <hook>
    local f why job
    GATES_WIRED_WHY="" GATES_WIRED_JOB=""
    f="$(gates_manager_file "$1" "$2" "$3")" || return 1
    case "$2" in
        husky)
            # husky runs .husky/<hook> under `sh -e` (husky 9's shim, husky
            # 8's husky.sh), so a failing call ends the script.
            gates_calls_through "$1/$f" "$3" errexit && return 0
            [[ -n "$GATES_WIRED_WHY" ]] && GATES_WIRED_WHY="$f $GATES_WIRED_WHY"
            return 1
            ;;
        lefthook)
            gates_lefthook_wired "$1" "$f" "$3" && return 0
            why="$GATES_WIRED_WHY" job="$GATES_WIRED_JOB"
            f="$(gates_lefthook_config "$1" local)" && gates_lefthook_wired "$1" "$f" "$3" && return 0
            GATES_WIRED_WHY="$why" GATES_WIRED_JOB="$job"
            return 1
            ;;
        pre-commit) gates_precommit_wired "$1" "$3" ;;
        *) return 1 ;;
    esac
}

# lefthook's pre-commit entry (#167): lefthook skips a pre-commit job while
# nothing is staged unless it has files to inspect, so `files:` names the
# config file (it exists whenever lefthook runs) and `{files}` sits in a
# shell comment of the quoted run line; the gates hook gets no arguments.
# The entry is written in the config's format (YAML, TOML or JSON).
gates_manager_entry() { # <manager> <hook> [config file, relative]
    local arg="" cfg="${3:-lefthook.yml}" run files=""
    case "$1" in
        husky)
            # shellcheck disable=SC2016  # written literally into .husky/<hook>
            printf 'bash "$(git rev-parse --show-toplevel)/.specify/gates/hooks/%s" "$@"\n' "$2"
            ;;
        lefthook)
            [[ "$2" == "commit-msg" ]] && arg=" {1}"
            run="bash .specify/gates/hooks/$2$arg"
            [[ "$2" == "pre-commit" ]] && run="$run # {files}" files="echo $cfg"
            case "$cfg" in
                *.toml)
                    printf '[%s.commands.spec-gates]\nrun = "%s"\n' "$2" "$run"
                    [[ -n "$files" ]] && printf 'files = "%s"\n' "$files"
                    ;;
                *.json | *.jsonc)
                    printf '"%s": { "commands": { "spec-gates": { "run": "%s"' "$2" "$run"
                    [[ -n "$files" ]] && printf ', "files": "%s"' "$files"
                    printf ' } } }\n'
                    ;;
                *)
                    printf '%s:\n  commands:\n    spec-gates:\n' "$2"
                    if [[ -n "$files" ]]; then
                        printf '      # files: and {files} make lefthook run this job with nothing staged\n'
                        printf '      run: "%s"\n      files: %s\n' "$run" "$files"
                    else
                        printf '      run: %s\n' "$run"
                    fi
                    ;;
            esac
            ;;
        pre-commit)
            printf -- '- repo: local\n  hooks:\n    - id: spec-gates-%s\n      name: spec-gates %s\n      entry: bash .specify/gates/hooks/%s\n      language: system\n' "$2" "$2" "$2"
            # pre-commit and pre-merge-commit (#148) take no file names and
            # run on every commit; commit-msg gets the message file.
            if [[ "$2" != "commit-msg" ]]; then
                printf '      pass_filenames: false\n      always_run: true\n      stages: [%s]\n' "$2"
            else
                printf '      stages: [commit-msg]\n'
            fi
            ;;
        *) return 1 ;;
    esac
}

# Append only where the result is certainly still valid: a husky script
# without a top-level `exit` (a line after it never runs); a lefthook block
# only when its top-level key, quoted or not, is absent; a pre-commit item
# only when `repos:` is the last top-level key and a block list (so the item
# lands in that list), at the indentation the file already uses. Tabs or
# any other layout: leave the file alone. gates_manager_appendable says
# whether the append is safe and, when not, sets GATES_MANAGER_WHY to the
# instruction for adding the entry by hand.
GATES_MANAGER_WHY=""
gates_manager_appendable() { # <root> <manager> <hook>
    local root="$1" mgr="$2" hook="$3" rel f last
    GATES_MANAGER_WHY=""
    rel="$(gates_manager_file "$root" "$mgr" "$hook")" || return 1
    f="$root/$rel"
    if [[ -f "$f" ]] && grep -q $'\t' "$f"; then
        GATES_MANAGER_WHY="$rel contains tabs, so gates does not edit it; add this by hand:"
        return 1
    fi
    case "$mgr" in
        husky)
            if gates_has_toplevel_exit "$f"; then
                GATES_MANAGER_WHY="$rel has a top-level exit, so an appended line would never run; add this line by hand, before any exit:"
                return 1
            fi
            ;;
        lefthook)
            case "$rel" in
                *.toml | *.json | *.jsonc)
                    GATES_MANAGER_WHY="$rel is lefthook's configuration and gates edits only YAML; add this to it by hand:"
                    return 1
                    ;;
            esac
            if [[ -f "$f" ]] && grep -qE "^[\"']?${hook}[\"']?[[:space:]]*:" "$f"; then
                GATES_MANAGER_WHY="$rel already has a $hook: block; merge this spec-gates command into your existing $hook: block by hand:"
                return 1
            fi
            ;;
        pre-commit)
            if [[ ! -f "$f" ]]; then
                GATES_MANAGER_WHY="$rel does not exist; create it with a repos: list holding this item:"
                return 1
            fi
            last="$(grep -E "^[\"']?[A-Za-z_][A-Za-z0-9_-]*[\"']?[[:space:]]*:" "$f" | tail -n 1 | cut -d: -f1 | tr -d "\"' ")"
            if [[ "$last" != "repos" ]] || ! grep -qE "^[\"']?repos[\"']?[[:space:]]*:[[:space:]]*(#.*)?$" "$f"; then
                GATES_MANAGER_WHY="$rel cannot be appended to safely (repos: is not the last top-level key, or is not a block list, as in repos: []); add this item to repos: by hand:"
                return 1
            fi
            ;;
        *) return 1 ;;
    esac
}

gates_manager_apply() { # <root> <manager> <hook>
    local root="$1" mgr="$2" hook="$3" rel f nl="" ind="  " first new=0
    gates_manager_appendable "$root" "$mgr" "$hook" || return 1
    rel="$(gates_manager_file "$root" "$mgr" "$hook")" || return 1
    f="$root/$rel"
    [[ -s "$f" && -n "$(tail -c 1 "$f")" ]] && nl=$'\n'
    case "$mgr" in
        husky)
            mkdir -p "$root/.husky" || return 1
            [[ -e "$f" ]] || new=1
            printf '%s%s' "$nl" "$(gates_manager_entry husky "$hook")"$'\n' >>"$f" || return 1
            # husky 8 (core.hooksPath=.husky) has git run this file itself,
            # and git skips one without the execute bit (#159); husky 9
            # does not need it. A file the user already had keeps its mode:
            # the git check reports one git would skip.
            if [[ "$new" -eq 1 ]]; then
                chmod +x "$f" || return 1
            fi
            ;;
        lefthook)
            printf '%s%s' "$nl" "$(gates_manager_entry lefthook "$hook" "$rel")"$'\n' >>"$f" || return 1
            ;;
        pre-commit)
            first="$(sed -nE '/^ *- +repo:/{p;q;}' "$f")"
            [[ -n "$first" ]] && ind="$(sed -E 's/^( *).*/\1/' <<<"$first")"
            printf '%s%s' "$nl" "$(gates_manager_entry pre-commit "$hook" | sed "s/^/$ind/")"$'\n' >>"$f" || return 1
            ;;
        *) return 1 ;;
    esac
}

gates_manager_install_hint() { # <manager> <hook>
    case "$1" in
        lefthook) echo "lefthook install" ;;
        pre-commit) echo "pre-commit install --hook-type $2" ;;
        husky) echo "npx husky" ;;
    esac
}
