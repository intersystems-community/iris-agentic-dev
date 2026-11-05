#!/usr/bin/env bash
# spec-gate.sh -- spec-conformance gate for verify.sh (feature 002).
#
# Sourced by verify.sh (and doctor.sh for discovery reporting). Provides:
#   gates_spec_features <root>            # discovered feature dir names (R7)
#   gates_spec_status <spec-md>           # raw Status field value, "" if none
#   gates_spec_complete <spec-md>         # 0 iff Status is exactly "Complete"
#   gates_spec_parse <tasks-md> <outdir>  # accept blocks + checkbox counts (R1/R3)
#   gates_spec_run_block <cmd> <t> <root> <out>  # one block: watchdog + mutation (R4/R5)
#   gates_spec_cover_boundary / gates_spec_cover_ask  # covering (feature 007)
#   gates_spec_covered_at_ci <boundary> <results>      # parity check on covering
#   gates_spec_gate <root> <accept> <json>       # driver; results in SPEC_* globals
#
# Sources lib/watch.sh (gates_group_stop, gates_watch_wait) and lib/timeline.sh (timeline
# and progress lines) from its own directory.
#
# Fenced ```accept blocks in specs/*/tasks.md are executable acceptance
# criteria (exit 0 = the criterion holds). Features whose spec.md Status is
# Complete are enforced: any unchecked task or failing block fails the gate
# at the policy's spec.severity. Everything else is informational. Parse
# errors fail closed for every feature — an unreadable criterion is never
# silently skipped (FR-005).
#
# All bash 3.2 + jq + POSIX awk/sed. Blocks execute with GATES_SPEC_EXEC=1
# so a block that invokes verify.sh cannot re-enter the spec gate (that run
# reports the spec gate as skipped).

# shellcheck disable=SC2034   # library file; SPEC_* globals consumed by callers

# The process-group stop lives in lib/watch.sh. Found next to this file:
# doctor.sh, constitution.sh and the tests source spec-gate.sh alone.
# shellcheck source=watch.sh disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/watch.sh"
# Timeline and progress lines per block (feature 007), no-ops unless
# verify.sh was given the files.
# shellcheck source=timeline.sh disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/timeline.sh"

# Feature discovery (R7): direct children of specs/ containing a spec.md,
# lexicographic order, minus policy spec.exclude globs. Missing specs/ means
# zero features (FR-011) — the gate then passes trivially.
gates_spec_features() { # <root>
    local root="${1:-}" d name g skip excludes
    [[ -d "$root/specs" ]] || return 0
    excludes="$(gates_policy_section_list spec exclude)"
    for d in "$root/specs"/*/; do
        [[ -d "$d" && -f "${d}spec.md" ]] || continue
        name="$(basename "$d")"
        skip=0
        if [[ -n "$excludes" ]]; then
            while IFS= read -r g; do
                [[ -z "$g" ]] && continue
                if gates_glob_match "$name" "$g"; then
                    skip=1
                    break
                fi
            done <<<"$excludes"
        fi
        [[ "$skip" == "1" ]] && continue
        printf '%s\n' "$name"
    done
    return 0
}

# Raw Status field from a feature's spec.md (first match wins, R2).
gates_spec_status() { # <spec-md>
    [[ -f "${1:-}" ]] || return 0
    sed -n 's/^\*\*Status\*\*:[[:space:]]*//p' "$1" | head -n 1 | sed 's/[[:space:]]*$//'
}

# Completion marker (R2): Status exactly "Complete" turns enforcement on.
gates_spec_complete() { # <spec-md>
    [[ "$(gates_spec_status "${1:-}")" == "Complete" ]]
}

# Is <feature> eligible for enforcement per spec.include (default ["*"])?
gates_spec_included() { # <feature-name>
    local includes g
    includes="$(gates_policy_section_list spec include)"
    [[ -z "$includes" ]] && return 0
    while IFS= read -r g; do
        [[ -z "$g" ]] && continue
        gates_glob_match "$1" "$g" && return 0
    done <<<"$includes"
    return 1
}

# Accept-block parser + fence-aware checkbox accounting (R1/R3). One awk
# state machine; command bodies land in <outdir>/block-N.cmd, metadata on
# stdout as TAB-separated protocol lines:
#   ERROR<TAB><line><TAB><message>
#   WARN<TAB><line><TAB><message>   (doctor's lint; the gate ignores it)
#   BLOCK<TAB><cmdfile><TAB><verifies-or-dash><TAB><ordinal><TAB><task text>
#   TASKS<TAB><total><TAB><unchecked><TAB><first unchecked task text>
# <ordinal> is the block's position (from 1) among the accept blocks under
# its own task line (research R14), so two tasks with the same text number
# their blocks independently. Malformed shapes (unterminated fence, command-less block, block with no
# preceding task, a `!` command before the last command) are ERROR lines,
# never skips. Fences are CommonMark-style runs of 3+ backticks, closed
# only by a run at least as long — prettier rewrites a block whose body
# contains ``` to a ````-fenced block, so exact-three matching would
# silently drop that criterion. Interval regexes and
# gensub are avoided on purpose: the parser must run on BSD awk (macOS).
gates_spec_parse() { # <tasks-md> <outdir>
    local file="${1:-}" outdir="${2:-}"
    [[ -f "$file" && -d "$outdir" ]] || return 0
    awk -v dir="$outdir" '
    # Queue the delimiters of the heredocs a command line opens (#237):
    # `<<`/`<<-` outside quotes, comments, `<<<` and arithmetic `((...))`.
    function hd_scan(s,    n, i, c, sq, dq, ad, j, d, q, strip) {
        n = length(s); sq = 0; dq = 0; ad = 0
        for (i = 1; i <= n; i++) {
            c = substr(s, i, 1)
            if (sq) { if (c == "\047") sq = 0; continue }
            if (c == "\\") { i++; continue }
            if (dq) { if (c == "\"") dq = 0; continue }
            if (c == "\047") { sq = 1; continue }
            if (c == "\"") { dq = 1; continue }
            if (c == "#" && (i == 1 || substr(s, i - 1, 1) ~ /[[:space:]]/)) return
            if (c == "(" && substr(s, i + 1, 1) == "(") { ad++; i++; continue }
            if (c == ")" && substr(s, i + 1, 1) == ")" && ad > 0) { ad--; i++; continue }
            if (c != "<" || substr(s, i + 1, 1) != "<" || ad > 0) continue
            if (substr(s, i + 2, 1) == "<") { i += 2; continue }
            j = i + 2; strip = 0
            if (substr(s, j, 1) == "-") { strip = 1; j++ }
            while (substr(s, j, 1) ~ /[ \t]/) j++
            d = ""
            while (j <= n) {
                c = substr(s, j, 1)
                if (c ~ /[ \t;&|<>()]/) break
                if (c == "\047" || c == "\"") {
                    q = c; j++
                    while (j <= n && substr(s, j, 1) != q) { d = d substr(s, j, 1); j++ }
                    j++
                    continue
                }
                if (c == "\\") j++
                d = d substr(s, j, 1); j++
            }
            if (d != "") { hd_n++; hd_delim[hd_n] = d; hd_strip[hd_n] = strip }
            i = j - 1
        }
    }
    # Does command line <s> pipe into `grep -q` (or --quiet, --silent) or
    # `head`? The reader exits early, the writer dies of SIGPIPE, and
    # under pipefail that is the block status, exit 141 (#302). `||` is
    # not a pipe.
    function pipe_early(s,    t, seg, i) {
        t = s
        while ((i = index(t, "|")) > 0) {
            t = substr(t, i + 1)
            if (substr(t, 1, 1) == "|") { t = substr(t, 2); continue }
            if (substr(t, 1, 1) == "&") t = substr(t, 2)
            seg = t
            sub(/^[ \t]+/, "", seg)
            if (seg ~ /^head([ \t]|$)/) return 1
            if (seg !~ /^[ef]?grep[ \t]/) continue
            if (match(seg, /[|;&]/)) seg = substr(seg, 1, RSTART - 1)
            if (seg ~ /[ \t]-[A-Za-z]*q/ || seg ~ /[ \t]--(quiet|silent)/) return 1
        }
        return 0
    }
    BEGIN {
        pipemsg = "a pipe into `grep -q` or `head` can fail the block with exit 141 (SIGPIPE) under pipefail; capture first: out=\"$(cmd 2>&1)\"; grep -q pat <<<\"$out\""
        in_fence = 0; in_accept = 0
        task = ""; have_task = 0; tord = 0
        nblocks = 0; total = 0; unchecked = 0; first_unc = ""
        negmsg = "a ! command other than the last one in the block never fails it under set -e; write `test -z \"$(...)\"`, `if ...; then exit 1; fi` or `! cmd || exit 1`"
    }
    {
        line = $0
        # Fence-line decomposition: indent, backtick-run length, info string.
        is_fence = 0
        if (line ~ /^[[:space:]]*```/) {
            is_fence = 1
            rest = line
            sub(/^[[:space:]]*/, "", rest)
            flen = 0
            while (substr(rest, flen + 1, 1) == "`") flen++
            finfo = substr(rest, flen + 1)
            sub(/^[[:space:]]*/, "", finfo)
            sub(/[[:space:]]*$/, "", finfo)
        }
        if (in_accept) {
            if (is_fence && finfo == "" && flen >= open_len) {
                if (!orphan && !negerr) {
                    if (ncmds == 0) {
                        printf "ERROR\t%d\taccept block has no command lines\n", open_line
                    } else {
                        nblocks++
                        cmdfile = dir "/block-" nblocks ".cmd"
                        printf "%s", body > cmdfile
                        close(cmdfile)
                        v = (verifies == "") ? "-" : verifies
                        tord++
                        printf "BLOCK\t%s\t%s\t%d\t%s\n", cmdfile, v, tord, btask
                    }
                }
                in_accept = 0
                next
            }
            n = 0
            while (n < indent && substr(line, n + 1, 1) == " ") n++
            stripped = substr(line, n + 1)
            body = body stripped "\n"
            # Heredoc body and terminator lines are data, not commands.
            if (hd_on) {
                t = stripped
                if (hd_strip[hd_i]) sub(/^\t+/, "", t)
                if (t == hd_delim[hd_i] && ++hd_i > hd_n) { hd_n = 0; hd_on = 0 }
                next
            }
            if (stripped ~ /[^[:space:]]/ && stripped !~ /^[[:space:]]*#/) {
                ncmds++
                # Blocks run under errexit, which a `!` command never
                # trips (#236): one followed by any further command line
                # is a parse error. A backslash-continued line belongs to
                # the command before it.
                if (!cont) {
                    if (neg_line && !negerr) {
                        printf "ERROR\t%d\t%s\n", neg_line, negmsg
                        negerr = 1
                    }
                    neg_line = 0
                    if (stripped ~ /^[[:space:]]*![[:space:]]/ && index(stripped, "||") == 0) neg_line = NR
                }
                cont = (stripped ~ /\\$/)
                if (pipe_early(stripped)) printf "WARN\t%d\t%s\n", NR, pipemsg
                hd_scan(stripped)
                if (!cont && hd_n > 0) { hd_on = 1; hd_i = 1 }
            }
            if (verifies == "" && stripped ~ /^# verifies:/) {
                v = stripped
                sub(/^# verifies:[[:space:]]*/, "", v)
                sub(/[[:space:]]*$/, "", v)
                verifies = v
            }
            next
        }
        if (is_fence) {
            if (!in_fence && finfo == "accept") {
                in_accept = 1
                open_line = NR
                open_len = flen
                body = ""; ncmds = 0; verifies = ""; orphan = 0
                neg_line = 0; negerr = 0; cont = 0; hd_n = 0; hd_on = 0
                match(line, /^[[:space:]]*/)
                indent = RLENGTH
                if (!have_task) {
                    printf "ERROR\t%d\taccept block has no preceding task line\n", NR
                    orphan = 1
                }
                btask = task
            } else if (!in_fence) {
                in_fence = 1
                gen_len = flen
            } else if (finfo == "" && flen >= gen_len) {
                in_fence = 0
            }
            # A fence-like line inside a generic fence that does not close it
            # (info string present, or a shorter run) is interior content.
            next
        }
        if (!in_fence && line ~ /^[[:space:]]*- \[[ xX]\] /) {
            total++
            t = line
            sub(/^[[:space:]]*- \[[ xX]\] /, "", t)
            if (line !~ /^[[:space:]]*- \[[xX]\]/) {
                unchecked++
                if (first_unc == "") first_unc = t
            }
            task = t
            have_task = 1
            tord = 0
        }
    }
    END {
        if (in_accept) printf "ERROR\t%d\tunterminated accept fence\n", open_line
        printf "TASKS\t%d\t%d\t%s\n", total, unchecked, first_unc
    }' "$file"
}

# Untracked and ignored paths the gate itself writes while a block runs (a
# nested verify.sh appends its attestation), left out of the read-only
# check together with the policy's spec.snapshot_exclude globs. A tracked
# path is never exempt. gates_spec_gate sets GATES_SPEC_SNAPSHOT_EXCLUDE.
# The gate lock (#263) is held by the run itself, not by a block; a nested
# verify.sh writes each failing gate's output under .specify/gates/output/.
# Editor temporary files (#303) come and go while a block runs whenever a file
# is open in an editor: vim's swap files and its 4913 write probe, backup
# files, Emacs lock and auto-save files. Being gitignored does not exempt
# them, since ignored files are checked too. Agent tooling writes its own
# state in the background the same way (#327): a Claude Code memory plugin
# keeps .claude/mind.* (its store, locks and write-ahead log). Only those
# files: what an agent edits under .claude/ on purpose (settings, skills,
# commands) stays checked.
GATES_SPEC_SNAPSHOT_BUILTIN_EXCLUDE=".specify/gates/attestations.jsonl
.specify/gates/.attestations.jsonl.*
.specify/gates/.lock/
.specify/gates/output/
**/*.swp
**/*.swo
**/*~
**/.#*
**/#*#
**/4913
**/.claude/mind.*"

# A directory below the root with its own .git (file or directory) is a
# nested repository or worktree, such as an agent's worktree under
# .claude/worktrees/. Other processes write there while a block runs, so
# writes in it and everything under it stay out of the ignored-files
# check (#263). A nested repository that appears in the working tree
# during a block still counts: the block made it.
gates_spec_nested_repo() { # <root> <path>
    [[ -n "$2" && -e "$1/${2%/}/.git" ]]
}

gates_spec_excluded() { # <path>
    local g
    while IFS= read -r g; do
        [[ -z "$g" ]] && continue
        # "dir/" names the directory: the ignored root "dir" (listed
        # without its slash) and every path under it (#199).
        [[ "$g" == */ ]] && g="${g%/}/**"
        gates_glob_match "$1" "$g" && return 0
    done <<<"${GATES_SPEC_SNAPSHOT_EXCLUDE:-$GATES_SPEC_SNAPSHOT_BUILTIN_EXCLUDE}"
    return 1
}

# One "<kind><TAB>value<TAB>relpath" line per file under <dir> (exec bit
# and content hash, or a symlink's target), or a single "absent" line.
gates_spec_dir_state() { # <kind> <root> <dir> <scratch> <list>
    local kind="$1" root="$2" dir="$3" scratch="$4" list="$5" path rel entry i
    [[ "$dir" == /* ]] || dir="$root/$dir"
    if [[ ! -d "$dir" ]]; then
        printf '%s\tabsent\t%s\n' "$kind" "$dir"
        return 0
    fi
    find "$dir" \( -type f -o -type l \) -print0 >"$scratch" 2>/dev/null || return 1
    : >"$list"
    local rels=() xs=()
    while IFS= read -r -d '' path; do
        rel="${path#"$dir"/}"
        # info/refs is the dumb-transport ref list that git gc and
        # update-server-info rewrite; it steers nothing (#213).
        [[ "$kind" == info && "$rel" == refs ]] && continue
        if [[ -L "$path" ]]; then
            printf '%s\tlink:%s\t%s\n' "$kind" "$(readlink "$path" 2>/dev/null || true)" "$rel"
            continue
        fi
        printf '%s\n' "$path" >>"$list"
        rels+=("$rel")
        if [[ -x "$path" ]]; then xs+=("x"); else xs+=("-"); fi
    done <"$scratch"
    [[ ${#rels[@]} -gt 0 ]] || return 0
    git -C "$root" hash-object --no-filters --stdin-paths \
        <"$list" >"$scratch" 2>/dev/null || return 1
    i=0
    while IFS= read -r entry; do
        printf '%s\t%s:%s\t%s\n' "$kind" "${xs[i]}" "$entry" "${rels[i]}"
        i=$((i + 1))
    done <"$scratch"
    [[ $i -eq ${#rels[@]} ]]
}

# The branches the worktree whose admin directory is <dir> works on, as
# "oth<TAB>refs/heads/...<TAB><name>" lines: the one HEAD points to, and the
# one an interrupted rebase will update (HEAD is detached meanwhile).
# <keep>, this worktree's own branch, is never listed.
gates_spec_wt_branches() { # <admin-dir> <name> <keep>
    local f line
    for f in "$1/HEAD" "$1/rebase-merge/head-name" "$1/rebase-apply/head-name"; do
        [[ -f "$f" ]] || continue
        line=""
        read -r line <"$f" || true
        line="${line#ref: }"
        [[ "$line" == refs/* && "$line" != "$3" ]] && printf 'oth\t%s\t%s\n' "$line" "$2"
    done
    return 0
}

# Snapshot of everything an accept block must leave alone (R5, #136, #164),
# written sorted to <state> as "kind<TAB>value<TAB>name" lines:
#   tree  one per `git status --porcelain=v1 -z --untracked-files=all`
#         entry; value = status, content hash and rename source. Status
#         alone misses a write to a file that is already dirty or
#         untracked (#136), so those files are hashed.
#   cfg   one per `git config --list --show-origin` entry (every scope, so
#         a --global core.hooksPath counts too); branch.* is left out, since
#         creating a branch in any sibling worktree writes it.
#   hook  one per file under the hooks directory git uses (`--git-path
#         hooks`, which follows core.hooksPath): exec bit and content hash.
#   info  the same for the info directory (attributes, exclude,
#         sparse-checkout): attributes can switch filters and diff drivers.
#         info/refs is left out: git gc and update-server-info rewrite it.
#   idx   one per index entry flagged skip-worktree or assume-unchanged
#         (`git ls-files -v`): the flag and a content hash, since status
#         no longer reports edits to a flagged file (#197).
#   wt    this worktree, when it is a linked one: its path and its lock.
#   oth   one per branch another worktree (main or linked) has checked out
#         or is rebasing. Not compared: the comparison leaves the refs named
#         in either snapshot out, since a commit, a branch switch or a new
#         worktree there is not this block's doing (#206). Other worktrees'
#         HEADs and per-worktree refs are not visible from here at all.
#   ref   HEAD (commit and symbolic target) and every ref except
#         refs/remotes/*, which a background fetch moves.
#   ign   one per ignored root (`git ls-files -o -i --directory`): a file,
#         or a directory that is ignored as a whole. Content changes under
#         a root are found by ctime against a marker instead of hashing,
#         which would cost a full read of node_modules per block (see
#         gates_spec_ignored_writes).
# Returns nonzero when git cannot produce the snapshot.
gates_spec_snapshot() { # <root> <state-file>
    local root="$1" state="$2" scratch="$2.tmp" list="$2.list"
    local entry xy path orig n=0 i hdir kv key val head sym
    local xys=() paths=() origs=() hashes=() fidx=()
    git -C "$root" status --porcelain=v1 -z --untracked-files=all \
        >"$scratch" 2>/dev/null || return 1
    : >"$list"
    while IFS= read -r -d '' entry; do
        xy="${entry:0:2}"
        path="${entry:3}"
        orig=""
        # A staged rename or copy carries its source path as the next entry.
        case "$xy" in
            *R* | *C*) IFS= read -r -d '' orig || true ;;
        esac
        [[ "$xy" == "??" ]] && gates_spec_excluded "$path" && continue
        xys[n]="$xy"
        paths[n]="$path"
        origs[n]="$orig"
        if [[ -L "$root/$path" ]]; then
            hashes[n]="link:$(readlink "$root/$path" 2>/dev/null || true)"
        elif [[ -f "$root/$path" ]]; then
            hashes[n]=""
            printf '%s\n' "$path" >>"$list"
            fidx+=("$n")
        elif [[ -e "$root/$path" ]]; then
            hashes[n]="other"
        else
            hashes[n]="absent"
        fi
        n=$((n + 1))
    done <"$scratch"
    if [[ ${#fidx[@]} -gt 0 ]]; then
        # One hash-object call for every dirty file, paths fed on stdin
        # (an argument list this long would hit the argv limit).
        git -C "$root" hash-object --no-filters --stdin-paths \
            <"$list" >"$scratch" 2>/dev/null || return 1
        i=0
        while IFS= read -r entry; do
            hashes[fidx[i]]="$entry"
            i=$((i + 1))
        done <"$scratch"
        [[ $i -eq ${#fidx[@]} ]] || return 1
    fi
    {
        i=0
        while [[ $i -lt $n ]]; do
            printf 'tree\t%s %s %s\t%s\n' "${xys[i]}" "${hashes[i]}" "${origs[i]}" "${paths[i]}"
            i=$((i + 1))
        done

        git -C "$root" config --list --show-origin -z >"$scratch" 2>/dev/null || return 1
        while IFS= read -r -d '' entry && IFS= read -r -d '' kv; do
            key="${kv%%$'\n'*}"
            case "$key" in branch.*) continue ;; esac
            val=""
            [[ "$kv" == *$'\n'* ]] && val="${kv#*$'\n'}"
            val="${val//[$'\t\n']/ }"
            printf 'cfg\t%s %s\t%s\n' "$entry" "$val" "$key"
        done <"$scratch"

        local gpaths idir cdir wt
        gpaths="$(git -C "$root" rev-parse --git-path hooks --git-path info \
            --git-common-dir 2>/dev/null)" || return 1
        { read -r hdir && read -r idir && read -r cdir; } <<<"$gpaths" || return 1
        gates_spec_dir_state hook "$root" "$hdir" "$scratch" "$list" || return 1
        gates_spec_dir_state info "$root" "$idir" "$scratch" "$list" || return 1

        # Index flags: skip-worktree (S) and assume-unchanged (lowercase)
        # entries hide later edits from git status, so each one is listed
        # with its content hash.
        git -C "$root" ls-files -v -z >"$scratch" 2>/dev/null || return 1
        grep -z -v -e '^H ' "$scratch" >"$list" 2>/dev/null || [[ $? -eq 1 ]] || return 1
        local ftags=() fpaths=()
        : >"$scratch"
        while IFS= read -r -d '' entry; do
            path="${entry:2}"
            if [[ -f "$root/$path" && ! -L "$root/$path" ]]; then
                ftags+=("${entry:0:1}")
                fpaths+=("$path")
                printf '%s\n' "$path" >>"$scratch"
            else
                printf 'idx\t%s absent\t%s\n' "${entry:0:1}" "$path"
            fi
        done <"$list"
        if [[ ${#fpaths[@]} -gt 0 ]]; then
            git -C "$root" hash-object --no-filters --stdin-paths \
                <"$scratch" >"$list" 2>/dev/null || return 1
            i=0
            while IFS= read -r entry; do
                printf 'idx\t%s %s\t%s\n' "${ftags[i]}" "$entry" "${fpaths[i]}"
                i=$((i + 1))
            done <"$list"
            [[ $i -eq ${#fpaths[@]} ]] || return 1
        fi

        # Worktrees, read from <common-dir> (#206). This worktree's entry is
        # compared: where it lives and whether it is locked. Every other
        # worktree contributes only the branch it has checked out (or is
        # rebasing) as an "oth" line, which the comparison uses to leave
        # that branch's ref out and never compares itself.
        local gdir self=""
        gdir="$(git -C "$root" rev-parse --absolute-git-dir 2>/dev/null)" || return 1
        [[ "$cdir" == /* ]] || cdir="$root/$cdir"
        gdir="$(cd "$gdir" 2>/dev/null && pwd -P)" || return 1
        cdir="$(cd "$cdir" 2>/dev/null && pwd -P)" || return 1
        [[ "$gdir" == "$cdir" ]] || self="${gdir##*/}"
        sym="$(git -C "$root" symbolic-ref -q HEAD 2>/dev/null)" || sym="detached"
        [[ -n "$self" ]] && gates_spec_wt_branches "$cdir" main "$sym"
        for wt in "$cdir"/worktrees/*; do
            [[ -d "$wt" ]] || continue
            if [[ "${wt##*/}" != "$self" ]]; then
                gates_spec_wt_branches "$wt" "${wt##*/}" "$sym"
                continue
            fi
            path=""
            [[ -f "$wt/gitdir" ]] && read -r path <"$wt/gitdir"
            printf 'wt\t%s\t%s\n' "${path:-none}" "${wt##*/}"
            [[ -e "$wt/locked" ]] && printf 'wt\tlocked\t%s\n' "${wt##*/}"
        done

        git -C "$root" for-each-ref --format='%(objectname)%09%(refname)' \
            >"$scratch" 2>/dev/null || return 1
        awk -F'\t' '$2 !~ /^refs\/remotes\// { printf "ref\t%s\t%s\n", $1, $2 }' "$scratch"
        head="$(git -C "$root" rev-parse -q --verify HEAD 2>/dev/null)" || head="unborn"
        printf 'ref\t%s %s\tHEAD\n' "$head" "$sym"

        git -C "$root" ls-files -z -o -i --exclude-standard --directory \
            >"$scratch" 2>/dev/null || return 1
        while IFS= read -r -d '' path; do
            path="${path%/}"
            gates_spec_excluded "$path" && continue
            gates_spec_nested_repo "$root" "$path" && continue
            printf 'ign\t-\t%s\n' "$path"
        done <"$scratch"
    } >"$list.out" || return 1
    sort -u "$list.out" >"$state" || return 1
    rm -f "$scratch" "$list" "$list.out"
}

# Create <marker> and wait until the clock has visibly moved past it on the
# same filesystem: Linux stamps files from a coarse clock, and some
# filesystems keep whole seconds, so a write right after the marker could
# otherwise carry the marker's own timestamp and go unseen by -cnewer.
gates_spec_mark() { # <marker>
    local probe="$1.probe" n=0
    : >"$1" 2>/dev/null || return 1
    while [[ $n -lt 150 ]]; do
        rm -f "$probe"
        : >"$probe" 2>/dev/null || break
        if [[ -n "$(find "$probe" -newer "$1" 2>/dev/null)" ]]; then
            rm -f "$probe"
            return 0
        fi
        sleep 0.01
        n=$((n + 1))
    done
    rm -f "$probe"
    return 0
}

# Ignored paths written since <marker>: every file or directory under the
# ignored roots of <state> whose ctime is newer. ctime moves on any write,
# chmod, create, delete or rename inside a directory, and unprivileged code
# cannot set it back (touch -t sets mtime only).
gates_spec_ignored_writes() { # <root> <state> <marker>
    local root="$1" state="$2" marker="$3" kind val path
    while IFS=$'\t' read -r kind val path; do
        [[ "$kind" == ign ]] || continue
        [[ -e "$root/$path" || -L "$root/$path" ]] || continue
        printf './%s\0' "$path"
    done <"$state" >"$state.roots"
    if [[ -s "$state.roots" ]]; then
        # The same walk also prints each .git below a root (and does not
        # descend into it): its parent is a nested repository, left out
        # with everything under it (gates_spec_nested_repo).
        # shellcheck disable=SC2016  # expanded by the inner sh
        (cd "$root" && xargs -0 sh -c \
            'find "$@" \( -name .git -prune -print0 \) -o \( -cnewer "$0" -print0 \)' "$marker" \
            <"$state.roots") >"$state.found" 2>/dev/null || true
        local nested=() found=() n
        while IFS= read -r -d '' path; do
            path="${path#./}"
            if [[ "${path##*/}" == .git ]]; then
                nested+=("${path%/.git}")
            else
                found+=("$path")
            fi
        done <"$state.found"
        if [[ ${#found[@]} -gt 0 ]]; then
            for path in "${found[@]}"; do
                gates_spec_excluded "$path" && continue
                if [[ ${#nested[@]} -gt 0 ]]; then
                    for n in "${nested[@]}"; do
                        [[ "$path" == "$n" || "$path" == "$n"/* ]] && continue 2
                    done
                fi
                printf 'ign\t%s\n' "$path"
            done
        fi
    fi
    rm -f "$state.roots" "$state.found"
}

# One detail string from sorted "kind<TAB>name" change lines, at most ten
# names per kind.
gates_spec_change_detail() { # <changes-file>
    awk -F'\t' '
        { n[$1]++; if (n[$1] <= 10) l[$1] = l[$1] (l[$1] == "" ? "" : " ") $2 }
        END {
            nk = split("tree ign idx cfg hook info ref wt", order, " ")
            label["tree"] = "working tree modified"
            label["ign"] = "ignored files modified"
            label["idx"] = "index flags modified"
            label["cfg"] = "git config modified"
            label["hook"] = "git hooks modified"
            label["info"] = "git info files modified"
            label["ref"] = "refs modified"
            label["wt"] = "worktrees modified"
            out = ""
            for (i = 1; i <= nk; i++) {
                k = order[i]
                if (!(k in n)) continue
                s = label[k] ": " l[k]
                if (n[k] > 10) s = s " (+" (n[k] - 10) " more)"
                out = out (out == "" ? "" : "; ") s
            }
            printf "%s", out
        }' "$1"
}

# Is any live (non-zombie) process left in process group <pgid>? ps is the
# precise check; without it (minimal images) kill -0, which also counts a
# zombie that is about to be reaped.
gates_spec_group_alive() { # <pgid>
    local procs
    if procs="$(ps -A -o pgid= -o stat= 2>/dev/null)" && [[ -n "$procs" ]]; then
        awk -v g="$1" '$1 == g && $2 !~ /^Z/ { found = 1 } END { exit !found }' <<<"$procs"
        return
    fi
    kill -0 -- -"$1" 2>/dev/null
}

# Stop process group <pgid>: TERM, a two-second grace, then KILL. The
# implementation is gates_group_stop in lib/watch.sh (shared with project
# gates); this name stays for existing callers.
gates_spec_group_stop() { gates_group_stop "$@"; } # <pgid>

# PIDs of live processes whose environment holds GATES_SPEC_BLOCK=<id>, one
# per line. Every process a block starts inherits the variable, and leaving
# the process group or session (set -m, setsid, a double fork) does not
# drop it. Linux: /proc/<pid>/environ, matched as a whole entry. macOS and
# other BSDs: `ps -E`, which appends the environment to the command; the
# table is captured to a file first, so the matching awk is not in it.
# macOS shows no environment for Apple-signed binaries (/bin/sh, /bin/sleep,
# /usr/bin/git), so there this only finds other binaries; the lease
# descriptor (gates_spec_lease_holders) covers the rest. Both read the
# environment a process was started with; a zombie shows none. Returns 2
# when neither source is available.
gates_spec_marked() { # <id> <scratch>
    local needle="GATES_SPEC_BLOCK=$1" scratch="$2" path
    if [[ -r /proc/self/environ ]]; then
        (cd /proc && grep -l -a -z -x -F -e "$needle" [0-9]*/environ) \
            >"$scratch" 2>/dev/null || true
        while IFS= read -r path; do
            printf '%s\n' "${path%%/*}"
        done <"$scratch"
        return 0
    fi
    ps -A -ww -E -o pid= -o command= >"$scratch" 2>/dev/null || return 2
    [[ -s "$scratch" ]] || return 2
    awk -v n="$needle" 'index($0 " ", " " n " ") { print $1 }' "$scratch"
}

# PIDs of processes holding the write end of lease FIFO <fifo>, one per
# line. Linux: /proc/<pid>/fd links to the FIFO, kept when fdinfo shows a
# write mode (the gate's own reader holds the read end). Elsewhere: lsof
# over this user's processes (given the FIFO's path, macOS lsof matches
# nothing), compared against the physical path. That takes most of a
# second, but it only runs on the failure path, to name what to stop: the
# pass/fail decision never depends on it.
gates_spec_lease_holders() { # <fifo> <scratch>
    local fifo="$1" scratch="$2" path pid fd key flags real
    if [[ -d /proc/self/fd ]]; then
        find /proc/[0-9]*/fd -maxdepth 1 -lname "$fifo" >"$scratch" 2>/dev/null || true
        while IFS= read -r path; do
            pid="${path#/proc/}"
            pid="${pid%%/*}"
            fd="${path##*/}"
            flags=""
            while read -r key flags; do
                [[ "$key" == "flags:" ]] && break
                flags=""
            done <"/proc/$pid/fdinfo/$fd" 2>/dev/null
            [[ "$flags" =~ ^[0-7]+$ ]] && (((8#$flags & 3) != 0)) && printf '%s\n' "$pid"
        done <"$scratch"
        return 0
    fi
    command -v lsof >/dev/null 2>&1 || return 0
    real="$(cd "${fifo%/*}" 2>/dev/null && pwd -P)/${fifo##*/}" || return 0
    # -b -n -P: no blocking stat of network mounts, no name lookups.
    lsof -b -n -P -w -a -u "$(id -u)" -d 0-255 -F pan >"$scratch" 2>/dev/null || true
    awk -v f="$real" '/^p/ { p = substr($0, 2) } /^a/ { a = substr($0, 2) }
        /^n/ { if (substr($0, 2) == f && a ~ /[wu]/) print p }' "$scratch"
}

# Of <pid...>, the live (non-zombie) processes outside process group
# <pgid>, one per line: the block's own group is stopped and reported on
# its own, and a member killed a moment ago can still hold the lease.
# Without ps every PID is printed.
gates_spec_foreign() { # <pgid> <pid...>
    local g="$1" procs
    shift
    [[ $# -gt 0 ]] || return 0
    if procs="$(ps -A -o pid= -o pgid= -o stat= 2>/dev/null)" && [[ -n "$procs" ]]; then
        awk -v g="$g" -v want=" $* " 'index(want, " " $1 " ") && $2 != g && $3 !~ /^Z/ { print $1 }' <<<"$procs"
        return 0
    fi
    printf '%s\n' "$@"
}

# Stop every process outside the block's group <pgid> that the block left
# behind (#197): anything carrying block marker <id> and, when the lease
# was still held (<held> non-empty), anything holding the lease FIFO
# <fifo>. They are killed. Then, when the lease was held, the last holder
# exiting is the event that ends the wait: the lease reader writes "eof"
# to descriptor <fd>. That wait has a cap, so a holder that neither /proc
# nor lsof can name (or one that cannot die) does not hang the gate; the
# cap decides nothing, since a lease still held at this point fails the
# block either way. Returns 0 when none was left, 1 when some was.
gates_spec_detached_stop() { # <id> <scratch> <fifo> <pgid> <held> <fd>
    local id="$1" scratch="$2" fifo="$3" pgid="$4" held="$5" fd="$6" pids
    pids="$(gates_spec_marked "$id" "$scratch")" || pids=""
    [[ -n "$held" ]] && pids="$pids $(gates_spec_lease_holders "$fifo" "$scratch")"
    # shellcheck disable=SC2086  # a list of numeric PIDs
    pids="$(gates_spec_foreign "$pgid" $pids)"
    [[ -z "$held" && -z "$pids" ]] && return 0
    # shellcheck disable=SC2086  # a list of numeric PIDs
    [[ -n "$pids" ]] && kill -KILL $pids 2>/dev/null
    if [[ -n "$held" ]]; then
        gates_watch_wait 5 "$fd"
        [[ -n "$GATES_WATCH_RC" ]] || return 1
    fi
    [[ -z "$pids" ]]
}

# The 1-minute load average, for the timeout detail (#264): without it a
# block that times out on a loaded machine reads like a hang. From
# /proc/loadavg (Linux), else `sysctl -n vm.loadavg` (macOS and the BSDs:
# "{ 1.23 1.10 1.05 }"), else the "load average(s):" field of uptime.
# Prints nothing and returns 1 when none of them can be read.
# GATES_SPEC_LOADAVG_FILE replaces /proc/loadavg (tests).
gates_spec_load_avg() {
    local f="${GATES_SPEC_LOADAVG_FILE:-/proc/loadavg}" load="" out
    local num='^[{[:space:]]*([0-9]+[.,][0-9]+)' up='load averages?:[[:space:]]*([0-9]+[.,][0-9]+)'
    if [[ -r "$f" ]] && read -r out _ <"$f" 2>/dev/null && [[ "$out" =~ $num ]]; then
        load="${BASH_REMATCH[1]}"
    fi
    if [[ -z "$load" ]] && out="$(sysctl -n vm.loadavg 2>/dev/null)" && [[ "$out" =~ $num ]]; then
        load="${BASH_REMATCH[1]}"
    fi
    if [[ -z "$load" ]] && out="$(uptime 2>/dev/null)" && [[ "$out" =~ $up ]]; then
        load="${BASH_REMATCH[1]}"
    fi
    [[ -n "$load" ]] || return 1
    printf '%s' "${load/,/.}"
}

# Execute one accept block (R4/R5): `bash -eo pipefail`, so any failing
# line fails the block (#236), repo-root cwd, pure-shell watchdog (no
# timeout(1) on macOS base), snapshots before and after. Outside a git work
# tree the block does not run: a mutation check that cannot happen fails
# closed. The block is a job of a `set -m` subshell, so it leads its own
# process group. A timeout stops that whole group (#136). A block ends
# when every process holding its lease descriptor has exited: the lease
# reader's "eof" is the event, awaited for the rest of the block's time
# limit, so a child the block killed without waiting passes however long
# its exit takes (#294). A process still holding the lease at the limit
# fails the block (#164): a late write would land after the snapshot. So
# does a process left in the group after the lease is released (it closed
# the descriptor); that last check reads the process table once. A child
# that left the group (set -m, setsid, a double fork) is found by the
# lease or the GATES_SPEC_BLOCK marker in its environment, stopped, and
# fails the block too (#197). A process that closed the descriptor and
# re-executed without the marker (or, on macOS, runs an Apple-signed
# binary, whose environment ps cannot read) is not found.
# The previous block's after-snapshot is reused as this block's before-
# snapshot (SPEC_SNAP_PREV), since the gate writes nothing in between.
# Returns 0 pass, 1 fail, 2 timeout, 3 mutation; detail in SPEC_BLOCK_DETAIL.
gates_spec_run_block() { # <cmdfile> <timeout-s> <root> <outfile>
    local cmdfile="$1" timeout="$2" root="$3" outfile="$4"
    local marker="$outfile.timedout" leftover="$outfile.leftover"
    local before="$outfile.before" after="$outfile.after" mark gitdir
    SPEC_BLOCK_DETAIL=""
    : >"$outfile"
    if [[ "$(git -C "$root" rev-parse --is-inside-work-tree 2>/dev/null)" != "true" ]]; then
        SPEC_SNAP_PREV=""
        SPEC_BLOCK_DETAIL="cannot check for mutations: not a git work tree"
        return 1
    fi
    if [[ -n "${SPEC_SNAP_PREV:-}" && -f "$SPEC_SNAP_PREV" ]]; then
        before="$SPEC_SNAP_PREV"
    elif ! gates_spec_snapshot "$root" "$before"; then
        SPEC_BLOCK_DETAIL="cannot check for mutations: git snapshot failed"
        return 1
    fi
    SPEC_SNAP_PREV=""
    # The ctime marker lives in the git dir, on the repository's own
    # filesystem, so it shares the timestamp resolution of what it guards.
    gitdir="$(git -C "$root" rev-parse --absolute-git-dir 2>/dev/null || true)"
    mark="$gitdir/gates-spec-mark.$$"
    if [[ -z "$gitdir" ]] || ! gates_spec_mark "$mark"; then
        mark="$outfile.mark"
        gates_spec_mark "$mark"
    fi
    rm -f "$marker" "$leftover" "$outfile.detached"
    local rc=0 procs="$outfile.procs" lease="$outfile.lease" leased="$outfile.leased"
    # Unique per block and gate run; set only in the block's environment.
    GATES_SPEC_BLOCK_SEQ=$((${GATES_SPEC_BLOCK_SEQ:-0} + 1))
    local bid="$$-$GATES_SPEC_BLOCK_SEQ-$RANDOM$RANDOM"
    # The lease (#197): the block inherits the write end of a FIFO on fd 7,
    # and so does every process it starts, in or out of its group or
    # session, unless it closes the descriptor. The lease reader sees EOF
    # only once the last holder has exited, and then writes "eof" to a
    # second FIFO, <leased>. The block's exit status comes back on a third,
    # <status>. One read with the block's time limit decides on the status
    # (gates_watch_wait, lib/watch.sh): no timer process, so a block that
    # has finished is never reported as timed out, and only that decision
    # writes the timeout marker. A second read, for the rest of the limit,
    # waits for "eof". No sleep decides either outcome.
    local status="$outfile.status"
    rm -f "$lease" "$leased" "$status"
    if ! mkfifo "$lease" "$leased" "$status" 2>/dev/null; then
        rm -f "$mark" "$lease" "$leased" "$status"
        SPEC_BLOCK_DETAIL="cannot watch the block: mkfifo failed"
        return 1
    fi
    (
        set -m
        exec 6<>"$status" 5<>"$leased"
        SECONDS=0
        # The reader starts first (opening the write end waits for a
        # reader) and is its own job, so stopping the block's group does
        # not take it along.
        { cat "$lease" >/dev/null 2>&1; echo eof >&5; } </dev/null >/dev/null 2>&1 6>&- &
        reader=$!
        # A git hook runs this with GIT_DIR and GIT_INDEX_FILE set (absolute
        # in a linked worktree): a block that builds a sandbox repository
        # would then add, commit and tag in the caller's repository (#173).
        # The block gets a clean git environment (gates_git_env_clear,
        # lib/watch.sh); it runs in the project directory, so its own git
        # calls still find this repository.
        # The job around it leads the block's process group, outlives a TERM
        # to that group (a handler, not an ignore, so the block still gets
        # the signal), and reports the block's status however it ended. Its
        # own stdio is /dev/null, so it never holds a caller's $() pipe.
        {
            trap : HUP INT TERM
            gates_git_env_clear
            (cd "$root" && GATES_SPEC_EXEC=1 GATES_SPEC_BLOCK="$bid" exec bash -eo pipefail "$cmdfile") \
                7>"$lease" >"$outfile" 2>&1 </dev/null 6>&-
            echo "$?" >&6
        } >/dev/null 2>&1 </dev/null 5>&- &
        pid=$!
        stopped=""
        trap 'stopped=1; kill -TERM -- -"$pid" 2>/dev/null' HUP INT TERM
        gates_watch_wait "$timeout" 6
        brc="$GATES_WATCH_RC"
        held=1
        if [[ -n "$brc" ]]; then
            wait "$pid" 2>/dev/null
            # What the block started may finish within its limit: a child
            # it killed without waiting is gone once it releases the lease,
            # however long that takes on a loaded machine. At least one
            # second, so a block that ended at its limit still gets a read.
            left=$((timeout - SECONDS))
            [[ "$left" -ge 1 ]] || left=1
            gates_watch_wait "$left" 5
            [[ -n "$GATES_WATCH_RC" ]] && held=""
            # Still in the group after the lease was released (a process
            # that closed descriptor 7), or still holding it at the limit.
            if gates_spec_group_alive "$pid"; then
                : >"$leftover"
                gates_spec_group_stop "$pid"
            fi
        else
            gates_spec_group_stop "$pid"
            wait "$pid" 2>/dev/null
        fi
        # A process outside the group is stopped on every path, pass or not.
        gates_spec_detached_stop "$bid" "$procs" "$lease" "$pid" "$held" 5 ||
            : >"$outfile.detached"
        kill -TERM -- -"$reader" 2>/dev/null
        wait "$reader" 2>/dev/null
        [[ -n "$brc" ]] && exit "$brc"
        # Stopped from outside (the caller was interrupted): not a timeout.
        [[ -n "$stopped" ]] && exit 143
        : >"$marker"
        exit 2
    ) 2>/dev/null || rc=$?
    rm -f "$status" "$lease" "$leased"
    local detached=""
    [[ -f "$outfile.detached" ]] && detached="left a detached process running (stopped)"
    rm -f "$outfile.detached"
    if [[ -f "$marker" ]]; then
        rm -f "$marker" "$leftover" "$mark"
        local load
        load="$(gates_spec_load_avg)" || load=""
        SPEC_BLOCK_DETAIL="timeout after ${timeout}s${load:+ at load $load}${detached:+; $detached}"
        return 2
    fi
    if [[ "$rc" -ne 0 ]]; then
        SPEC_BLOCK_DETAIL="exit $rc"
        # 141 is 128 + SIGPIPE: under pipefail a `| grep -q` or `| head`
        # that exits early fails the block (#302).
        [[ "$rc" -eq 141 ]] &&
            SPEC_BLOCK_DETAIL="exit 141 (SIGPIPE: a pipe reader exited early under pipefail)"
        [[ -f "$leftover" ]] && SPEC_BLOCK_DETAIL="$SPEC_BLOCK_DETAIL; left a process running (stopped)"
        SPEC_BLOCK_DETAIL="$SPEC_BLOCK_DETAIL${detached:+; $detached}"
        rm -f "$leftover" "$mark"
        return 1
    fi
    if [[ -f "$leftover" ]]; then
        rm -f "$leftover" "$mark"
        SPEC_BLOCK_DETAIL="left a process running after it exited (stopped)${detached:+; $detached}"
        return 1
    fi
    if [[ -n "$detached" ]]; then
        rm -f "$mark"
        SPEC_BLOCK_DETAIL="$detached"
        return 1
    fi
    if ! gates_spec_snapshot "$root" "$after"; then
        rm -f "$mark"
        SPEC_BLOCK_DETAIL="cannot check for mutations: git snapshot failed"
        return 1
    fi
    {
        # Refs of branches another worktree has checked out, before or
        # after the block, are that worktree's doing (#206).
        sort "$before" "$after" | uniq -u | cut -f1,3 >"$outfile.diff"
        awk -F'\t' -v diff="$outfile.diff" '
            FILENAME != diff { if ($1 == "oth") skip[$2] = 1; next }
            $1 == "oth" { next }
            $1 == "ref" && ($2 in skip) { next }
            { print }' "$before" "$after" "$outfile.diff"
        gates_spec_ignored_writes "$root" "$after" "$mark"
    } | sort -u >"$outfile.changes"
    rm -f "$mark" "$outfile.diff"
    SPEC_SNAP_PREV="$after"
    if [[ -s "$outfile.changes" ]]; then
        SPEC_BLOCK_DETAIL="$(gates_spec_change_detail "$outfile.changes")"
        return 3
    fi
    return 0
}

# Covering (feature 007). May accept blocks be covered at this run's
# boundary? The one boundary-skip decision of feature 006 applied to
# spec.cover.at: agent or git when declared, never ci or a run without
# --boundary (constitution, Principle III). Its own function so the canary
# test can stub it and prove the parity gate still catches a block covered
# at ci.
gates_spec_cover_boundary() {
    [[ -n "$(gates_cover_script)" ]] || return 1
    gates_declared_skip "${GATES_BOUNDARY:-none}" "$(gates_cover_at)" >/dev/null
}

# Ask covering script <script> (project-relative) about one block (research
# R2): `<script> <feature> <task-id> <block-file>` from <root>, under
# <timeout-s> seconds through the watchdog (lib/watch.sh), stdout to
# <outfile> and stderr discarded. Covered (return 0, the reason in
# SPEC_COVER_REASON) means exit 0 and exactly one non-empty line; the line
# is cut to 200 characters with control characters removed (FR-008).
# Anything else returns 1 and the block runs: a non-zero exit, no line,
# several lines, a script that is not executable. A timeout (the script's
# process group is stopped) returns 124, so the caller can say why the
# block ran (#295); the block runs then too. GATES_GATE_ROOT keeps a script
# that starts verify.sh on this project from running the full suite inside
# the run.
gates_spec_cover_ask() { # <root> <script> <timeout-s> <feature> <task-id> <block-file> <outfile>
    local root="$1" script="$1/$2" outfile="$7" rc=0 lines line here="$PWD"
    SPEC_COVER_REASON=""
    [[ -f "$script" && -x "$script" ]] || return 1
    # In this shell, not a subshell: only gates_run_watched's own
    # GATES_WATCH_DETAIL tells its timeout from a script that exits 124.
    cd "$root" || return 1
    # shellcheck disable=SC2016  # expanded by the inner sh
    GATES_GATE_ROOT="$root" gates_run_watched "$3" "$outfile" \
        sh -c '"$0" "$@" 2>/dev/null' "$script" "$4" "$5" "$6" || rc=$?
    cd "$here" 2>/dev/null || cd "$root" || :
    [[ "$rc" -eq 124 && -n "$GATES_WATCH_DETAIL" ]] && return 124
    [[ "$rc" -eq 0 && -s "$outfile" ]] || return 1
    lines="$(awk 'END { print NR }' "$outfile")"
    [[ "$lines" == "1" ]] || return 1
    IFS= read -r line <"$outfile" || [[ -n "$line" ]] || return 1
    line="$(printf '%s' "$line" | LC_ALL=C tr -d '\000-\037\177')"
    line="${line:0:200}"
    [[ -n "${line//[[:space:]]/}" ]] || return 1
    SPEC_COVER_REASON="$line"
    return 0
}

# The parity check on covering (research R6): at ci, every block entry of
# <spec-results-json> (SPEC_RESULTS_JSON) with result "covered" is a
# failure. Prints "accept blocks covered at ci: <feature> <task-id>#<n>,
# ..." and returns 0 when there is any; returns 1 otherwise and at every
# other boundary. Covering never runs at ci (gates_spec_cover_boundary),
# so this fires only if that construction breaks.
gates_spec_covered_at_ci() { # <boundary> <spec-results-json>
    local named
    [[ "$1" == "ci" && -n "${2:-}" ]] || return 1
    named="$(jq -r '[.[]? | .feature as $f | (.blocks // [])[]
        | select(.result == "covered") | "\($f) \(.task)#\(.ordinal)"] | join(", ")' <<<"$2" 2>/dev/null)" \
        || named="the spec results could not be read"
    [[ -n "$named" ]] || return 1
    printf 'accept blocks covered at ci: %s\n' "$named"
}

# One attestation block entry (data-model.md): <verifies> "-" is null, an
# empty <declared-skip> leaves the field out (it names the boundary of a
# covered block only).
gates_spec_block_entry() { # <task> <ordinal> <verifies> <result> <reason> <declared-skip> <duration-s>
    jq -cn --arg t "$1" --argjson o "$2" --arg v "$3" --arg res "$4" --arg r "$5" \
        --arg b "$6" --argjson d "$7" '
        { task: $t, ordinal: $o,
          verifies: (if $v == "-" then null else $v end),
          result: $res, reason: $r }
        + (if $b == "" then {} else { declared_skip: $b } end)
        + { duration_s: $d }'
}

# Driver. Sets:
#   SPEC_RESULT   pass|fail            (severity mapping is the caller's job)
#   SPEC_DETAIL   one-line summary (pass) or "; "-joined failures (fail)
#   SPEC_FEATURES SPEC_PARSED SPEC_EXECUTED SPEC_PASSED SPEC_FAILED  counts
#   SPEC_COVERED  blocks covered (feature 007), not run, so not executed;
#   SPEC_COVERED_NAMES "<feature> <task-id>#<n>, ..." of those blocks
#   SPEC_RESULTS_JSON  FeatureConformance array (data-model.md)
# In text mode (json=0) informational lines print directly: per-criterion
# results for --accept runs, and one summary line counting the features
# that are not enforced (#300). GATES_SPEC_VERBOSE=1 lists each of them
# instead, with its Status; --json always carries every feature.
gates_spec_gate() { # <root> <accept-arg> <json 0|1>
    local root="${1:-}" accept="${2:-}" json="${3:-0}"
    SPEC_RESULT="pass"
    SPEC_DETAIL=""
    SPEC_FEATURES=0
    SPEC_PARSED=0
    SPEC_EXECUTED=0
    SPEC_PASSED=0
    SPEC_FAILED=0
    SPEC_RESULTS_JSON="[]"
    SPEC_COVERED=0
    SPEC_COVERED_NAMES=""
    local timeout
    timeout="$(gates_policy_section_get spec timeout_s)"
    [[ -z "$timeout" ]] && timeout=30
    # Covering (feature 007): asked only where spec.cover.at names this
    # run's boundary.
    local cover_script="" cover_s="" cover_b=""
    if gates_spec_cover_boundary; then
        cover_script="$(gates_cover_script)"
        cover_s="$(gates_cover_timeout)"
        cover_b="${GATES_BOUNDARY:-none}"
    fi
    SPEC_SNAP_PREV=""
    # A timeline or progress file inside the project is written between
    # blocks: each block then takes its own before-snapshot instead of the
    # last block's after-one, which predates that write.
    local tl_fresh=0
    gates_timeline_inside "$root" && tl_fresh=1
    GATES_SPEC_SNAPSHOT_EXCLUDE="$GATES_SPEC_SNAPSHOT_BUILTIN_EXCLUDE
$(gates_policy_section_list spec snapshot_exclude)"
    local tmp
    if ! tmp="$(mktemp -d 2>/dev/null || mktemp -d -t gates-spec)"; then
        SPEC_RESULT="fail"
        SPEC_DETAIL="spec gate: could not create work dir (mktemp)"
        return 0
    fi
    local failures="" results="" complete_n=0 info_n=0 f
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        SPEC_FEATURES=$((SPEC_FEATURES + 1))
        local fdir="$root/specs/$f"
        mkdir -p "$tmp/$f"
        local tasks_total=0 tasks_unchecked=0 first_unchecked=""
        local bfiles=() bverifies=() btasks=() bords=()
        local parse_out=""
        if [[ -f "$fdir/tasks.md" ]]; then
            parse_out="$(gates_spec_parse "$fdir/tasks.md" "$tmp/$f")"
        fi
        local line tag a1 a2 rest
        local ferrors=0
        while IFS=$'\t' read -r tag a1 a2 rest; do
            [[ -z "$tag" ]] && continue
            case "$tag" in
                ERROR)
                    failures="${failures:+$failures; }specs/$f/tasks.md:$a1: $a2"
                    ferrors=$((ferrors + 1))
                    ;;
                BLOCK)
                    bfiles+=("$a1")
                    bverifies+=("$a2")
                    # <ordinal><TAB><task text>; read drops the tab of an
                    # empty task text.
                    bords+=("${rest%%$'\t'*}")
                    if [[ "$rest" == *$'\t'* ]]; then
                        btasks+=("${rest#*$'\t'}")
                    else
                        btasks+=("")
                    fi
                    SPEC_PARSED=$((SPEC_PARSED + 1))
                    ;;
                TASKS)
                    tasks_total="$a1"
                    tasks_unchecked="$a2"
                    first_unchecked="$rest"
                    ;;
            esac
        done <<<"$parse_out"
        local nblocks=${#bfiles[@]}
        local complete=0 enforced=0
        gates_spec_complete "$fdir/spec.md" && complete=1
        if [[ "$complete" == "1" ]] && gates_spec_included "$f"; then
            enforced=1
            complete_n=$((complete_n + 1))
        fi
        local do_exec=0
        if [[ "$enforced" == "1" ]]; then
            do_exec=1
        elif [[ "$accept" == "all" || "$accept" == "$f" ]]; then
            do_exec=1
        fi
        local i=0 fpassed=0 ffailed=0 fexecuted=0 fcovered=0 fblocks="" tid ord
        local tid_re='^(T[0-9]+)([^A-Za-z0-9_]|$)'
        if [[ "$do_exec" == "1" ]]; then
            while [[ $i -lt $nblocks ]]; do
                local label="${bverifies[$i]}"
                [[ "$label" == "-" ]] && label="${btasks[$i]}"
                # Block identity (research R14): <feature> <task-id>#<n>, n
                # the position among the blocks under its own task line (the
                # parser counts them).
                tid="-"
                [[ "${btasks[$i]}" =~ $tid_re ]] && tid="${BASH_REMATCH[1]}"
                ord="${bords[$i]}"
                # The start line comes before any of the block's work,
                # covering included (SC-006).
                local bname="accept $f $tid#$ord" estart
                [[ "$tl_fresh" == "1" ]] && SPEC_SNAP_PREV=""
                gates_progress_start "$bname"
                estart="$(date +%s)"
                # Why a block under covering ran anyway, when the script
                # ran out of time (#295): part of the block's reason.
                local cnote=""
                if [[ -n "$cover_b" ]]; then
                    local cstart cend covered=0 crc=0
                    cstart="$estart"
                    gates_spec_cover_ask "$root" "$cover_script" "$cover_s" "$f" "$tid" \
                        "${bfiles[$i]}" "$tmp/$f/cover-$i" || crc=$?
                    [[ "$crc" -eq 0 ]] && covered=1
                    [[ "$crc" -eq 124 ]] && cnote="covering timed out after ${cover_s}s; block ran"
                    cend="$(date +%s)"
                    # The script may write to the tree: the next block takes
                    # a fresh before-snapshot instead of the last after-one.
                    SPEC_SNAP_PREV=""
                    if [[ "$covered" == "1" ]]; then
                        # A covered block does not run and counts as neither
                        # passed nor failed (FR-004).
                        fcovered=$((fcovered + 1))
                        SPEC_COVERED_NAMES="${SPEC_COVERED_NAMES:+$SPEC_COVERED_NAMES, }$f $tid#$ord"
                        local cdur=$((cend - cstart))
                        [[ "$cdur" -lt 0 ]] && cdur=0
                        fblocks="$fblocks$(gates_spec_block_entry "$tid" "$ord" "${bverifies[$i]}" \
                            covered "$SPEC_COVER_REASON" "$cover_b" "$cdur"),"
                        gates_timeline_line "$bname" "$cstart" "$cend" covered "$SPEC_COVER_REASON"
                        gates_progress_end "$bname" covered "$cdur"
                        if [[ "$json" == "0" && "$enforced" == "1" ]]; then
                            echo "spec: $f: \"$label\" -- covered: $SPEC_COVER_REASON"
                        elif [[ "$json" == "0" ]]; then
                            echo "spec: $f: \"$label\" -- covered (informational): $SPEC_COVER_REASON"
                        fi
                        i=$((i + 1))
                        continue
                    fi
                fi
                local brc=0 bstart bend bdur bres breason
                bstart="$(date +%s)"
                gates_spec_run_block "${bfiles[$i]}" "$timeout" "$root" "$tmp/$f/out-$i" || brc=$?
                bend="$(date +%s)"
                bdur=$((bend - bstart))
                [[ "$bdur" -lt 0 ]] && bdur=0
                fexecuted=$((fexecuted + 1))
                # The record keeps fixed reasons only (FR-015): the detail
                # names files and exit codes, the output stays in the report.
                case "$brc" in
                    0) bres="pass"; breason="" ;;
                    2) bres="timeout"; breason="timeout after ${timeout}s" ;;
                    3) bres="mutation"; breason="read-only check failed" ;;
                    *) bres="fail"; breason="check failed" ;;
                esac
                if [[ -n "$cnote" ]]; then
                    breason="$cnote${breason:+; $breason}"
                    SPEC_BLOCK_DETAIL="$cnote${SPEC_BLOCK_DETAIL:+; $SPEC_BLOCK_DETAIL}"
                fi
                fblocks="$fblocks$(gates_spec_block_entry "$tid" "$ord" "${bverifies[$i]}" \
                    "$bres" "$breason" "" "$bdur"),"
                gates_timeline_line "$bname" "$estart" "$bend" "$bres" "$breason"
                gates_progress_end "$bname" "$bres" "$((bend - estart < 0 ? 0 : bend - estart))"
                if [[ "$brc" -eq 0 ]]; then
                    fpassed=$((fpassed + 1))
                    if [[ "$enforced" == "0" && "$json" == "0" ]]; then
                        echo "spec: $f: \"$label\" -- pass${cnote:+ ($cnote)}"
                    elif [[ -n "$cnote" && "$json" == "0" ]]; then
                        echo "spec: $f: \"$label\" -- pass ($cnote)"
                    fi
                else
                    ffailed=$((ffailed + 1))
                    if [[ "$enforced" == "1" ]]; then
                        failures="${failures:+$failures; }$f: \"$label\": $SPEC_BLOCK_DETAIL"
                        if [[ "$json" == "0" ]]; then
                            echo "spec: $f: \"$label\" -- FAILED ($SPEC_BLOCK_DETAIL):"
                            sed 's/^/  | /' "$tmp/$f/out-$i" | tail -n 20
                        fi
                    elif [[ "$json" == "0" ]]; then
                        echo "spec: $f: \"$label\" -- fail ($SPEC_BLOCK_DETAIL, informational):"
                        sed 's/^/  | /' "$tmp/$f/out-$i" | tail -n 20
                    fi
                fi
                i=$((i + 1))
            done
        fi
        SPEC_EXECUTED=$((SPEC_EXECUTED + fexecuted))
        SPEC_PASSED=$((SPEC_PASSED + fpassed))
        SPEC_FAILED=$((SPEC_FAILED + ffailed))
        SPEC_COVERED=$((SPEC_COVERED + fcovered))
        # Outcome classification (data-model.md): top-down, first match wins.
        # Task drift blocks even with zero accept blocks; no-criteria only
        # applies once every task is checked.
        local outcome
        if [[ "$enforced" == "1" ]]; then
            if [[ "$tasks_unchecked" -gt 0 ]]; then
                failures="${failures:+$failures; }$f: unchecked task: \"$first_unchecked\" ($tasks_unchecked of $tasks_total unchecked)"
                outcome="enforced-fail"
            elif [[ "$ffailed" -gt 0 ]]; then
                outcome="enforced-fail"
            elif [[ "$nblocks" -eq 0 ]]; then
                outcome="no-criteria"
                [[ "$json" == "0" ]] && echo "spec: $f -- marked Complete with no accept blocks (nothing executable to hold it to)"
            else
                outcome="enforced-pass"
            fi
        else
            outcome="informational"
            if [[ "$accept" != "all" && "$accept" != "$f" ]]; then
                info_n=$((info_n + 1))
            fi
            if [[ "$json" == "0" && "$accept" != "all" && "$accept" != "$f" \
                && "${GATES_SPEC_VERBOSE:-}" == "1" ]]; then
                local status
                status="$(gates_spec_status "$fdir/spec.md")"
                # Display-clip the raw Status value (issue #34): enforcement
                # keys on the exact token "Complete", but a long free-form
                # status would render lossily in narrow output.
                [[ "${#status}" -gt 40 ]] && status="${status:0:37}..."
                echo "spec: $f -- $nblocks criteria parsed, not enforced (Status: ${status:-none})"
            fi
        fi
        local cb=false
        [[ "$complete" == "1" ]] && cb=true
        local rj
        rj="$(jq -cn --arg f "$f" --arg o "$outcome" --argjson c "$cb" \
            --argjson tt "$tasks_total" --argjson tu "$tasks_unchecked" \
            --argjson bp "$nblocks" --argjson be "$fexecuted" \
            --argjson bs "$fpassed" --argjson bf "$ffailed" \
            --argjson bc "$fcovered" --argjson bl "[${fblocks%,}]" '
            { feature: $f, complete: $c, tasks_total: $tt,
              tasks_unchecked: $tu, blocks_parsed: $bp, blocks_executed: $be,
              blocks_passed: $bs, blocks_failed: $bf, outcome: $o,
              # Feature 007: one entry per evaluated block, in order.
              blocks_covered: $bc, blocks: $bl }')"
        results="$results$rj,"
    done <<<"$(gates_spec_features "$root")"
    SPEC_RESULTS_JSON="[${results%,}]"
    rm -rf "$tmp"
    # One line for every feature not enforced, not one each (#300): a
    # repository with many open specs would bury the gate summary.
    if [[ "$json" == "0" && "$info_n" -gt 0 && "${GATES_SPEC_VERBOSE:-}" != "1" ]]; then
        echo "spec: $info_n informational, $complete_n enforced -- not enforced unless spec.md says exactly **Status**: Complete (GATES_SPEC_VERBOSE=1 lists each)"
    fi
    # Every covered block is named in the summary, so --json names it too
    # (constitution, Principle III); with none covered the detail is as
    # before.
    local covered_detail=""
    [[ "$SPEC_COVERED" -gt 0 ]] && covered_detail="$SPEC_COVERED covered at $cover_b: $SPEC_COVERED_NAMES"
    if [[ -n "$failures" ]]; then
        SPEC_RESULT="fail"
        SPEC_DETAIL="$failures${covered_detail:+; $covered_detail}"
    elif [[ -n "$covered_detail" ]]; then
        SPEC_DETAIL="$SPEC_FEATURES features, $SPEC_PARSED blocks: $SPEC_PASSED passed, $covered_detail"
    else
        SPEC_DETAIL="$SPEC_FEATURES feature(s), $complete_n enforced, $SPEC_PARSED criteria parsed, $SPEC_EXECUTED executed"
    fi
    return 0
}
