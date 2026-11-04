#!/usr/bin/env bash
# timeline.sh -- timeline and progress lines for one gate run (feature 007,
# research R8 to R10, R15).
#
# Sourced by verify.sh and lib/spec-gate.sh. Provides:
#   gates_timeline_init <boundary>     # read GATES_TIMELINE / GATES_PROGRESS once
#   gates_timeline_line <name> <start> <end> <result> <reason>
#   gates_progress_start <name>
#   gates_progress_end <name> <result> <seconds>
#   gates_timeline_inside <root>       # is either file inside <root>?
#
# GATES_TIMELINE names a file that gets one tab-separated line per ended
# gate or accept block: name, boundary, start, end (epoch seconds), result,
# reason. GATES_PROGRESS names a file that gets
# "gates: <HH:MM:SS> start <name>" and "gates: <HH:MM:SS> end <name>
# <result> <n>s" lines. Both belong to the run they are given to:
# gates_timeline_init copies them into this run's own variables and unsets
# them, so no command the run starts (gate commands, accept blocks, the
# covering script, a nested verify.sh) inherits them. Without init, or with
# neither variable set, every function here is a no-op. Each line is one
# printf append, so a reader never sees half a line. A file that cannot be
# written is dropped for the rest of the run with one warning; no gate
# outcome changes (FR-016). Callers pass names, results and fixed reasons
# only, never command output (FR-015). Pure bash 3.2 (no printf %(...)T).

# shellcheck disable=SC2034   # library file

gates_timeline_init() { # <boundary>
    # Unset first: an inherited GATES_TL_FILE would stay exported.
    unset GATES_TL_FILE GATES_PG_FILE GATES_TL_BOUNDARY
    GATES_TL_FILE="${GATES_TIMELINE:-}"
    GATES_PG_FILE="${GATES_PROGRESS:-}"
    GATES_TL_BOUNDARY="${1:-unspecified}"
    unset GATES_TIMELINE GATES_PROGRESS
}

# Append <text> and a newline to <file>; nonzero when it cannot be written.
# /dev/stderr (the pre-commit terminal view) is written through fd 2: an
# open of it after the 2>/dev/null that keeps the shell's own error message
# quiet would land in /dev/null.
gates_timeline_append() { # <file> <text>
    case "$1" in
        /dev/stderr | /dev/fd/2 | /proc/self/fd/2) printf '%s\n' "$2" >&2 2>/dev/null ;;
        *) printf '%s\n' "$2" 2>/dev/null >>"$1" ;;
    esac
}

# Fields go on one line: tabs, newlines and carriage returns become spaces.
gates_timeline_field() { # <text>
    local s="$1"
    s="${s//[$'\t\n\r']/ }"
    printf '%s' "$s"
}

gates_timeline_line() { # <name> <start> <end> <result> <reason>
    [[ -n "${GATES_TL_FILE:-}" ]] || return 0
    local line
    line="$(gates_timeline_field "$1")"$'\t'"$(gates_timeline_field "${GATES_TL_BOUNDARY:-unspecified}")"
    line="$line"$'\t'"$2"$'\t'"$3"$'\t'"$(gates_timeline_field "$4")"$'\t'"$(gates_timeline_field "${5:-}")"
    if ! gates_timeline_append "$GATES_TL_FILE" "$line"; then
        echo "gates: warning: cannot write $GATES_TL_FILE; no further timeline lines this run" >&2
        GATES_TL_FILE=""
    fi
    return 0
}

gates_progress_write() { # <text>
    [[ -n "${GATES_PG_FILE:-}" ]] || return 0
    if ! gates_timeline_append "$GATES_PG_FILE" "gates: $(date +%H:%M:%S) $1"; then
        echo "gates: warning: cannot write $GATES_PG_FILE; no further progress lines this run" >&2
        GATES_PG_FILE=""
    fi
    return 0
}

# Does the timeline or the progress file lie inside <root>? The spec gate
# then takes a fresh snapshot before each block, since these lines are
# written between blocks. A /dev path never does.
gates_timeline_inside() { # <root>
    local f dir rootp
    rootp="$(cd "$1" 2>/dev/null && pwd -P)" || return 1
    for f in "${GATES_TL_FILE:-}" "${GATES_PG_FILE:-}"; do
        [[ -n "$f" && "$f" != /dev/* ]] || continue
        dir="$(cd "$(dirname "$f")" 2>/dev/null && pwd -P)" || continue
        [[ "$dir" == "$rootp" || "$dir" == "$rootp"/* ]] && return 0
    done
    return 1
}

gates_progress_start() { # <name>
    gates_progress_write "start $(gates_timeline_field "$1")"
}

gates_progress_end() { # <name> <result> <seconds>
    gates_progress_write "end $(gates_timeline_field "$1") $2 ${3:-0}s"
}
