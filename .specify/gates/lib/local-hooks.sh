#!/usr/bin/env bash
# local-hooks.sh -- project-owned rules that survive upgrades (#71).
#
# A project adds its own refusals as scripts in
#   .specify/gates/hooks.local.d/<hook>/*.sh
# where <hook> is protect-files, validate-bash, validate-pr, pre-commit or
# commit-msg. projection, upgrades and the manifest never touch that
# directory. Each hook runs the rules only after every shipped check
# allowed, so a rule can add a refusal but never remove one.
#
# Usage (sourced; bash 3.2):
#   gates_local_has <root> <hook>          # 0 if the hook has local rules
#   gates_run_local <root> <hook> [args]   # 0 = every rule allowed;
#                                          # 1 = refused, GATES_LOCAL_MSG set
#
# Each rule runs as `bash <rule> [args]` (execute bits do not matter), with
# GATES_LOCAL_STDIN on its stdin (the agent hooks pass the tool call JSON;
# the git hooks pass nothing), GATES_HOOK=<hook> and
# GATES_PROJECT_ROOT=<root> in its environment. Exit 0 allows; any other
# exit refuses, and the rule's stderr becomes the refusal message. Rules
# run in lexical order and the first refusal wins. A rule that cannot be
# read (unreadable, a directory, a dangling symlink) refuses: a rule the
# project wrote must never silently not run. A rule still running after
# GATES_LOCAL_TIMEOUT seconds (default 10) is killed with everything it
# started and refuses; so does a rule that exits but leaves a process
# running. A GATES_LOCAL_TIMEOUT that is not a whole number above 0
# refuses before any rule runs.

# shellcheck disable=SC2034   # library file; GATES_LOCAL_MSG is read by callers
GATES_LOCAL_MSG=""
GATES_LOCAL_STDIN="${GATES_LOCAL_STDIN:-}"

gates_local_has() { # <root> <hook>
    local f
    for f in "$1/.specify/gates/hooks.local.d/$2"/*.sh; do
        [[ -e "$f" || -L "$f" ]] && return 0
    done
    return 1
}

# Is any live (non-zombie) process left in process group <pgid>? The same
# check as lib/spec-gate.sh: ps when it works, else kill -0 (which also
# counts a zombie that is about to be reaped). An empty group skips ps:
# this runs after every rule, on every tool call.
_gates_local_group_alive() { # <pgid>
    local procs
    kill -0 -- -"$1" 2>/dev/null || return 1
    if procs="$(ps -A -o pgid= -o stat= 2>/dev/null)" && [[ -n "$procs" ]]; then
        awk -v g="$1" '$1 == g && $2 !~ /^Z/ { found = 1 } END { exit !found }' <<<"$procs"
        return
    fi
    kill -0 -- -"$1" 2>/dev/null
}

# Stop process group <pgid>: TERM, a one-second grace, then KILL.
_gates_local_group_stop() { # <pgid>
    local n=0
    kill -TERM -- -"$1" 2>/dev/null || return 0
    while [[ $n -lt 10 ]] && kill -0 -- -"$1" 2>/dev/null; do
        sleep 0.1
        n=$((n + 1))
    done
    kill -KILL -- -"$1" 2>/dev/null
    return 0
}

# _gates_local_exec <rule> [args...]: run one rule with GATES_LOCAL_STDIN on
# its stdin (a here-string: a rule that never reads stdin must not turn a
# large tool call into SIGPIPE), its stderr on stdout, and a watchdog of
# GATES_LOCAL_LIMIT seconds. The rule's stderr goes to a file, not to the
# caller's $() pipe: a child the rule leaves behind would hold the pipe
# open, and the hook would wait for it past any timeout (#189). The exit
# is the rule's; the state file GATES_LOCAL_STATE gets "timeout" when the
# limit stopped it, or "leftover" when it exited but left a process
# running (a file, so no exit code of the rule's own can pose as either).
_gates_local_exec() {
    local f="$1" limit="$GATES_LOCAL_LIMIT" flag="$GATES_LOCAL_STATE" rc=0 errf fifo
    shift
    errf="$(mktemp 2>/dev/null || mktemp -t gates-local)" || return 1
    # The rule's exit status comes back on a FIFO, and one `read -t` with
    # the limit decides: a status in time is the result, none means the
    # limit passed. There is no timer process, so a rule that has finished
    # is never reported as timed out, and no sleep paces the decision. As
    # in lib/watch.sh, which this file does not source: the canary copies
    # it alone.
    fifo="$errf.status"
    if ! mkfifo "$fifo" 2>/dev/null; then
        rm -f "$errf"
        echo "cannot watch the rule: mkfifo failed"
        return 1
    fi
    (
        # set -m gives the rule its own process group, so a timeout stops
        # whatever the rule started, not just the rule's shell. A child that
        # calls setsid leaves the group and is out of reach. The job around
        # the rule outlives a TERM to that group (a handler, not an ignore,
        # so the rule still gets the signal) and reports its status.
        set -m
        exec 6<>"$fifo"
        {
            trap : HUP INT TERM
            GATES_HOOK="$GATES_LOCAL_HOOK" GATES_PROJECT_ROOT="$GATES_LOCAL_ROOT" \
                "$BASH" "$f" "$@" <<<"$GATES_LOCAL_STDIN" >/dev/null 2>"$errf" 6>&-
            echo "$?" >&6
        } </dev/null >/dev/null 2>&1 &
        pid=$!
        rrc=""
        read -r -t "$limit" -u 6 rrc 2>/dev/null || :
        if [[ -z "$rrc" ]]; then
            echo timeout >"$flag"
            _gates_local_group_stop "$pid"
            crc=0
            wait "$pid" 2>/dev/null || crc=$?
            exit "$crc"
        fi
        wait "$pid" 2>/dev/null
        # The rule exited, and whatever it started goes with it. A child
        # already on its way out gets half a second before it counts. A rule
        # that leaves a process running refuses, as an accept block does:
        # the process would outlive the check it was part of.
        n=0
        while [[ $n -lt 5 ]] && _gates_local_group_alive "$pid"; do
            sleep 0.1
            n=$((n + 1))
        done
        if _gates_local_group_alive "$pid"; then
            _gates_local_group_stop "$pid"
            echo leftover >"$flag"
        fi
        exit "$rrc"
    ) || rc=$?
    rm -f "$fifo"
    cat "$errf" 2>/dev/null
    rm -f "$errf"
    return "$rc"
}

gates_run_local() { # <root> <hook> [args...]
    local root="$1" hook="$2" f name out rc state how
    local GATES_LOCAL_LIMIT="${GATES_LOCAL_TIMEOUT:-10}"
    shift 2
    # Callers write `GATES_LOCAL_STDIN="$INPUT" gates_run_local ...`, and bash
    # exports a prefix assignment to a function for the call. Every program
    # started below would inherit the whole tool call, and Linux refuses to
    # exec one with an environment string over 128 KB (E2BIG), which read
    # as a refusal. It reaches the rule through a here-string instead.
    export -n GATES_LOCAL_STDIN 2>/dev/null || true
    GATES_LOCAL_MSG=""
    # A timeout that is not a whole number of seconds above 0 refuses: under
    # set -u a word such as "abc" broke the watchdog and left no timeout at
    # all (#189).
    case "$GATES_LOCAL_LIMIT" in
        '' | *[!0-9]*) GATES_LOCAL_LIMIT=0 ;;
        *) GATES_LOCAL_LIMIT=$((10#$GATES_LOCAL_LIMIT)) ;;
    esac
    if [[ "$GATES_LOCAL_LIMIT" -le 0 ]]; then
        GATES_LOCAL_MSG="gates(local $hook): GATES_LOCAL_TIMEOUT=${GATES_LOCAL_TIMEOUT:-} is not a whole number of seconds above 0, so the rules refuse"
        return 1
    fi
    for f in "$root/.specify/gates/hooks.local.d/$hook"/*.sh; do
        [[ -e "$f" || -L "$f" ]] || continue
        name="${f##*/}"
        if [[ ! -f "$f" || ! -r "$f" ]]; then
            GATES_LOCAL_MSG="gates(local $hook/$name): the rule cannot be read, so it refuses"
            return 1
        fi
        state="$(mktemp 2>/dev/null || mktemp -t gates-local)" || {
            GATES_LOCAL_MSG="gates(local $hook/$name): cannot create a temporary file, so it refuses"
            return 1
        }
        : >"$state"
        rc=0
        out="$(GATES_LOCAL_HOOK="$hook" GATES_LOCAL_ROOT="$root" GATES_LOCAL_STATE="$state" \
            _gates_local_exec "$f" "$@")" || rc=$?
        how="$(cat "$state" 2>/dev/null)"
        rm -f "$state"
        if [[ "$how" == timeout ]]; then
            GATES_LOCAL_MSG="gates(local $hook/$name): still running after ${GATES_LOCAL_LIMIT}s, stopped with everything it started, so it refuses"
            return 1
        fi
        if [[ "$how" == leftover ]]; then
            GATES_LOCAL_MSG="gates(local $hook/$name): exited but left a process running (stopped), so it refuses"
            return 1
        fi
        if [[ "$rc" -ne 0 ]]; then
            GATES_LOCAL_MSG="gates(local $hook/$name): ${out:-refused (exit $rc)}"
            return 1
        fi
    done
    return 0
}
