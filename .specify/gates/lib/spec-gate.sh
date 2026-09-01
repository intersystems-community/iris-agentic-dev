#!/usr/bin/env bash
# spec-gate.sh -- spec-conformance gate for verify.sh (feature 002).
#
# Sourced by verify.sh (and doctor.sh for discovery reporting). Provides:
#   gates_spec_features <root>            # discovered feature dir names (R7)
#   gates_spec_status <spec-md>           # raw Status field value, "" if none
#   gates_spec_complete <spec-md>         # 0 iff Status is exactly "Complete"
#   gates_spec_parse <tasks-md> <outdir>  # accept blocks + checkbox counts (R1/R3)
#   gates_spec_run_block <cmd> <t> <root> <out>  # one block: watchdog + mutation (R4/R5)
#   gates_spec_gate <root> <accept> <json>       # driver; results in SPEC_* globals
#
# Fenced ```accept blocks in specs/*/tasks.md are executable acceptance
# criteria (exit 0 = the criterion holds). Features whose spec.md Status is
# Complete are enforced: any unchecked task or failing block fails the gate
# at the policy's spec.severity. Everything else is informational. Parse
# errors fail closed for every feature — an unreadable criterion is never
# silently skipped (FR-005).
#
# All bash 3.2 + jq + POSIX awk/sed. Blocks execute with GATES_SPEC_EXEC=1
# so a block that invokes verify.sh cannot re-enter the spec gate.

# shellcheck disable=SC2034   # library file; SPEC_* globals consumed by callers

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
#   BLOCK<TAB><cmdfile><TAB><verifies-or-dash><TAB><task text>
#   TASKS<TAB><total><TAB><unchecked><TAB><first unchecked task text>
# Malformed shapes (unterminated fence, command-less block, block with no
# preceding task) are ERROR lines, never skips. Fences are CommonMark-style
# runs of 3+ backticks, closed only by a run at least as long — prettier
# rewrites a block whose body contains ``` to a ````-fenced block, so exact-
# three matching would silently drop that criterion. Interval regexes and
# gensub are avoided on purpose: the parser must run on BSD awk (macOS).
gates_spec_parse() { # <tasks-md> <outdir>
    local file="${1:-}" outdir="${2:-}"
    [[ -f "$file" && -d "$outdir" ]] || return 0
    awk -v dir="$outdir" '
    BEGIN {
        in_fence = 0; in_accept = 0
        task = ""; have_task = 0
        nblocks = 0; total = 0; unchecked = 0; first_unc = ""
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
                if (!orphan) {
                    if (ncmds == 0) {
                        printf "ERROR\t%d\taccept block has no command lines\n", open_line
                    } else {
                        nblocks++
                        cmdfile = dir "/block-" nblocks ".cmd"
                        printf "%s", body > cmdfile
                        close(cmdfile)
                        v = (verifies == "") ? "-" : verifies
                        printf "BLOCK\t%s\t%s\t%s\n", cmdfile, v, btask
                    }
                }
                in_accept = 0
                next
            }
            n = 0
            while (n < indent && substr(line, n + 1, 1) == " ") n++
            stripped = substr(line, n + 1)
            body = body stripped "\n"
            if (stripped ~ /[^[:space:]]/ && stripped !~ /^[[:space:]]*#/) ncmds++
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
        }
    }
    END {
        if (in_accept) printf "ERROR\t%d\tunterminated accept fence\n", open_line
        printf "TASKS\t%d\t%d\t%s\n", total, unchecked, first_unc
    }' "$file"
}

# Execute one accept block (R4/R5): repo-root cwd, pure-shell watchdog (no
# timeout(1) on macOS base), before/after `git status --porcelain` snapshots.
# The exec keeps the killed pid the block itself, not a wrapper subshell.
# Returns 0 pass, 1 fail, 2 timeout, 3 mutation; detail in SPEC_BLOCK_DETAIL.
gates_spec_run_block() { # <cmdfile> <timeout-s> <root> <outfile>
    local cmdfile="$1" timeout="$2" root="$3" outfile="$4"
    SPEC_BLOCK_DETAIL=""
    local in_git=0 before="" after=""
    if git -C "$root" rev-parse --git-dir >/dev/null 2>&1; then
        in_git=1
        before="$(git -C "$root" status --porcelain 2>/dev/null || true)"
    fi
    (cd "$root" && GATES_SPEC_EXEC=1 exec bash "$cmdfile") >"$outfile" 2>&1 &
    local pid=$!
    # The watchdog gets /dev/null stdio: it (and the sleep it may orphan)
    # must not hold inherited fds, or a block that captures a nested
    # verify.sh via $() would wait on the pipe until the sleep expires.
    (
        sleep "$timeout"
        kill "$pid" 2>/dev/null
    ) >/dev/null 2>&1 </dev/null &
    local watcher=$!
    local rc=0
    wait "$pid" 2>/dev/null || rc=$?
    kill "$watcher" 2>/dev/null || true
    wait "$watcher" 2>/dev/null || true
    if [[ "$rc" -eq 143 ]]; then
        SPEC_BLOCK_DETAIL="timeout after ${timeout}s"
        return 2
    fi
    if [[ "$rc" -ne 0 ]]; then
        SPEC_BLOCK_DETAIL="exit $rc"
        return 1
    fi
    if [[ "$in_git" == "1" ]]; then
        after="$(git -C "$root" status --porcelain 2>/dev/null || true)"
        if [[ "$before" != "$after" ]]; then
            local changed
            changed="$({
                printf '%s\n' "$before"
                printf '%s\n' "$after"
            } | grep -v '^$' | sort | uniq -u | cut -c4- | sort -u | tr '\n' ' ')"
            SPEC_BLOCK_DETAIL="working tree modified: ${changed% }"
            return 3
        fi
    fi
    return 0
}

# Driver. Sets:
#   SPEC_RESULT   pass|fail            (severity mapping is the caller's job)
#   SPEC_DETAIL   one-line summary (pass) or "; "-joined failures (fail)
#   SPEC_FEATURES SPEC_PARSED SPEC_EXECUTED SPEC_PASSED SPEC_FAILED  counts
#   SPEC_RESULTS_JSON  FeatureConformance array (data-model.md)
# In text mode (json=0) informational lines print directly: per-criterion
# results for --accept runs and one line per not-enforced feature.
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
    local timeout
    timeout="$(gates_policy_section_get spec timeout_s)"
    [[ -z "$timeout" ]] && timeout=30
    local tmp
    if ! tmp="$(mktemp -d 2>/dev/null || mktemp -d -t gates-spec)"; then
        SPEC_RESULT="fail"
        SPEC_DETAIL="spec gate: could not create work dir (mktemp)"
        return 0
    fi
    local failures="" results="" complete_n=0 f
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        SPEC_FEATURES=$((SPEC_FEATURES + 1))
        local fdir="$root/specs/$f"
        mkdir -p "$tmp/$f"
        local tasks_total=0 tasks_unchecked=0 first_unchecked=""
        local bfiles=() bverifies=() btasks=()
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
                    btasks+=("$rest")
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
        local i=0 fpassed=0 ffailed=0 fexecuted=0
        if [[ "$do_exec" == "1" ]]; then
            while [[ $i -lt $nblocks ]]; do
                local label="${bverifies[$i]}"
                [[ "$label" == "-" ]] && label="${btasks[$i]}"
                local brc=0
                gates_spec_run_block "${bfiles[$i]}" "$timeout" "$root" "$tmp/$f/out-$i" || brc=$?
                fexecuted=$((fexecuted + 1))
                if [[ "$brc" -eq 0 ]]; then
                    fpassed=$((fpassed + 1))
                    if [[ "$enforced" == "0" && "$json" == "0" ]]; then
                        echo "spec: $f: \"$label\" -- pass"
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
            if [[ "$json" == "0" && "$accept" != "all" && "$accept" != "$f" ]]; then
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
            --argjson bs "$fpassed" --argjson bf "$ffailed" '
            { feature: $f, complete: $c, tasks_total: $tt,
              tasks_unchecked: $tu, blocks_parsed: $bp, blocks_executed: $be,
              blocks_passed: $bs, blocks_failed: $bf, outcome: $o }')"
        results="$results$rj,"
    done <<<"$(gates_spec_features "$root")"
    SPEC_RESULTS_JSON="[${results%,}]"
    rm -rf "$tmp"
    if [[ -n "$failures" ]]; then
        SPEC_RESULT="fail"
        SPEC_DETAIL="$failures"
    else
        SPEC_DETAIL="$SPEC_FEATURES feature(s), $complete_n enforced, $SPEC_PARSED criteria parsed, $SPEC_EXECUTED executed"
    fi
    return 0
}
