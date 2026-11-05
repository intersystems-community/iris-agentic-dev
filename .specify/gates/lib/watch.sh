#!/usr/bin/env bash
# watch.sh -- run a command under a time limit (feature 006, research R3).
#
# Sourced by verify.sh and lib/spec-gate.sh. Provides:
#   gates_group_stop <pgid>                       # TERM, 2 s grace, KILL
#   gates_run_watched <timeout-s> <outfile> <cmd...>
#   gates_watch_wait <timeout-s> <fd>             # the decision, for callers
#   gates_git_env_clear                           # in a subshell: drop git's
#                                                 # per-repository variables
#
# Pure bash 3.2: stock macOS has no timeout(1). The command runs as a job
# of a `set -m` subshell, so it leads its own process group. When it ends,
# its exit status is written to a FIFO, and the subshell decides with one
# `read -t` on that FIFO: a status that arrives in time is the result, no
# status means the limit passed and the group is stopped. There is no
# timer process, so nothing can report a timeout for a command that has
# already finished. A process that leaves the group (setsid, a double fork
# into a new session) is not stopped.
# Unlike accept blocks, watched commands are not read-only by contract, so
# the accept-block runner's lease and snapshot checks do not apply here.

# shellcheck disable=SC2034   # library file; GATES_WATCH_DETAIL is read by callers

# git's per-repository variables (`git rev-parse --local-env-vars`, plus
# GIT_NAMESPACE and GIT_QUARANTINE_PATH). A git hook exports some of them,
# GIT_DIR and GIT_INDEX_FILE absolute in a linked worktree; a command a
# gate runs would then point every repository it creates or changes at the
# caller's: `git init` and `git add -A` in a temp directory set the main
# repository's core.bare and staged deletions in the worktree (#173,
# #293). Every command the gates run starts without them, in the project
# or the caller's directory, so its own git calls still find this
# repository. Not cleared in verify.sh itself: the contract gate reads the
# index of the commit being made (`git commit <paths>` uses a temporary
# one).
GATES_GIT_LOCAL_ENV="GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG GIT_CONFIG_PARAMETERS
GIT_CONFIG_COUNT GIT_OBJECT_DIRECTORY GIT_DIR GIT_WORK_TREE GIT_IMPLICIT_WORK_TREE
GIT_GRAFT_FILE GIT_INDEX_FILE GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE GIT_PREFIX
GIT_SHALLOW_FILE GIT_COMMON_DIR GIT_NAMESPACE GIT_QUARANTINE_PATH"

# Unset GATES_GIT_LOCAL_ENV in the current shell: call it in the subshell
# that runs a command, never in the caller's.
gates_git_env_clear() {
    local v
    for v in $GATES_GIT_LOCAL_ENV; do unset "$v"; done
}

# Stop process group <pgid>: TERM, a two-second grace, then KILL.
gates_group_stop() { # <pgid>
    local n=0
    kill -TERM -- -"$1" 2>/dev/null || return 0
    while [[ $n -lt 20 ]] && kill -0 -- -"$1" 2>/dev/null; do
        sleep 0.1
        n=$((n + 1))
    done
    kill -KILL -- -"$1" 2>/dev/null
    return 0
}

# Wait at most <timeout-s> seconds for the exit status a watched job
# writes to descriptor <fd>, a FIFO the caller holds open read-write (so
# the read never sees end-of-file). Sets GATES_WATCH_RC to the status, or
# to "" when the time is up. This one read is the whole decision. The
# value decides, not read's own status: on expiry bash 3.2 returns 1 and
# bash 4+ returns above 128. Integer seconds only (bash 3.2).
gates_watch_wait() { # <timeout-s> <fd>
    GATES_WATCH_RC=""
    read -r -t "$1" -u "$2" GATES_WATCH_RC 2>/dev/null || :
}

# Run <cmd...> with stdin /dev/null and stdout and stderr to <outfile>,
# from the current directory. Returns the command's status; on timeout
# its process group is stopped and the return is 124 with
# GATES_WATCH_DETAIL="timeout after <n>s[ at load <x>]". Only the
# decision writes the timeout marker, so a command that exits 124 by
# itself leaves GATES_WATCH_DETAIL empty. Call it in the caller's own
# shell (not in $( )), or GATES_WATCH_DETAIL is lost.
#
# With GATES_WATCH_STREAM=1 (verify.sh's run_gate under GATES_VERBOSE=1,
# #309), the output also goes to the caller's stderr through tee, on
# descriptor 7. The status is then written when the output ends: a
# process the command leaves holding it keeps the gate running, up to the
# limit.
gates_run_watched() { # <timeout-s> <outfile> <cmd...>
    local timeout="$1" outfile="$2" marker="$2.timedout" fifo="$2.status" rc=0 load
    local stream_fd=-
    [[ "${GATES_WATCH_STREAM:-}" == "1" ]] && stream_fd=2
    shift 2
    GATES_WATCH_DETAIL=""
    rm -f "$marker" "$fifo"
    if ! mkfifo "$fifo" 2>/dev/null; then
        GATES_WATCH_DETAIL="cannot watch the command: mkfifo failed"
        return 1
    fi
    (
        set -m
        exec 6<>"$fifo"
        # The job's own stdio is /dev/null, so it never holds a caller's
        # $() pipe open; the command writes to <outfile> and does not get
        # the FIFO's descriptor. The job outlives a TERM to the group (a
        # handler, not an ignore, so the command still gets the signal) and
        # reports the command's status however it ended.
        {
            trap : HUP INT TERM
            gates_git_env_clear
            if [[ "$stream_fd" == "2" ]]; then
                "$@" 2>&1 </dev/null 6>&- 7>&- | tee "$outfile" >&7 6>&- 7>&-
                echo "${PIPESTATUS[0]}" >&6
            else
                "$@" >"$outfile" 2>&1 </dev/null 6>&- 7>&-
                echo "$?" >&6
            fi
        } >/dev/null 2>&1 </dev/null &
        pid=$!
        stopped=""
        trap 'stopped=1; kill -TERM -- -"$pid" 2>/dev/null' HUP INT TERM
        gates_watch_wait "$timeout" 6
        if [[ -n "$GATES_WATCH_RC" ]]; then
            wait "$pid" 2>/dev/null
            exit "$GATES_WATCH_RC"
        fi
        gates_group_stop "$pid"
        crc=0
        wait "$pid" 2>/dev/null || crc=$?
        # Stopped from outside (the caller was interrupted): not a timeout.
        [[ -n "$stopped" ]] || : >"$marker"
        exit "$crc"
    ) 7>&"$stream_fd" 2>/dev/null || rc=$?
    rm -f "$fifo"
    if [[ -f "$marker" ]]; then
        rm -f "$marker"
        load=""
        if declare -F gates_spec_load_avg >/dev/null; then
            load="$(gates_spec_load_avg)" || load=""
        fi
        GATES_WATCH_DETAIL="timeout after ${timeout}s${load:+ at load $load}"
        return 124
    fi
    return "$rc"
}
