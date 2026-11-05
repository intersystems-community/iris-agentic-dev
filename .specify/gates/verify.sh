#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# spec-gates: single verify entrypoint, invoked identically at all three
# enforcement boundaries.
#
#   Agent boundary : Claude Code Stop hook (verify-quality.sh delegates here)
#   Git boundary   : .git/hooks/pre-commit
#   CI boundary    : projected GitHub Actions / GitLab CI / Jenkins job
#
# The parity property — "if the agent boundary passed, git passes; if git
# passed, CI passes" — holds because every boundary runs THIS script with
# THIS policy. tests/test-parity.sh asserts it.
#
# Usage:
#   verify.sh --boundary agent|git|ci [--json] [--dry-run] [--accept <feature|all>]
#   verify.sh --walk [--boundary agent|git|ci] [--json] [--dry-run]
#
# --accept additionally executes the named incomplete feature(s)' accept
# blocks as informational output (feature 002); complete features are
# enforced on every run regardless.
#
# --walk (feature 006, research R8) runs only the built-in per-tool walk
# (prettier, markdownlint, shellcheck, each when the policy enables it),
# whatever the orchestrator: no contract, project, spec or parity gate and
# no attestation (the run that calls it records its own gate). It is the
# stable entry point for custom commands; lib/formatter-dispatch.sh is
# internal. Called from inside a gate run it joins the holder's lock.
#
# Exit codes: 0 = all gates green, 1 = internal error (bad arguments, no jq,
# no or invalid policy; no gate ran), 2 = gate failure.
#
# With --json, an exit-1 refusal still prints exactly one object on stdout,
# {"result":"refused","boundary":...,"reason":...}, next to the stderr
# message, so a workflow step never parses empty output.

BOUNDARY="unspecified"
JSON=0
DRY_RUN=0
ACCEPT_ARG=""
WALK=0

# --json is known before the argument loop so an argument error can answer
# in JSON too.
for _arg in "$@"; do [[ "$_arg" == "--json" ]] && JSON=1; done

json_str() { # <text>: JSON string literal; pure bash, jq may be missing
    local s="$1"
    s="${s//\\/\\\\}"; s="${s//\"/\\\"}"
    s="${s//$'\n'/\\n}"; s="${s//$'\t'/\\t}"; s="${s//$'\r'/\\r}"
    s="$(printf '%s' "$s" | LC_ALL=C tr -d '\000-\037')"
    printf '"%s"' "$s"
}

refused() { # <reason>: the --json refusal object (if --json), exit 1
    if [[ "$JSON" == "1" ]]; then
        printf '{"result":"refused","boundary":%s,"reason":%s}\n' \
            "$(json_str "$BOUNDARY")" "$(json_str "$1")"
    fi
    exit 1
}

refuse() { # <message>: no gate ran; stderr message, exit 1
    echo "gates: $1" >&2
    refused "$1"
}

usage() { # <message>: argument error, exit 1
    echo "gates: $1" >&2
    echo "usage: verify.sh --boundary agent|git|ci [--json] [--dry-run] [--accept <feature|all>]" >&2
    echo "       verify.sh --walk [--boundary agent|git|ci] [--json] [--dry-run]" >&2
    refused "$1"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --boundary)
            [[ $# -ge 2 && "$2" != -* ]] || usage "--boundary needs a value (agent, git or ci)"
            # A second --boundary would replace the first: in CI,
            # `--boundary ci --boundary agent` runs the agent set (#198).
            [[ "$BOUNDARY" == unspecified ]] || usage "--boundary given more than once"
            case "$2" in
                agent | git | ci) BOUNDARY="$2" ;;
                *) usage "--boundary: invalid value: $2 (allowed: agent, git, ci)" ;;
            esac
            shift 2
            ;;
        --json)     JSON=1; shift ;;
        --dry-run)  DRY_RUN=1; shift ;;
        --walk)     WALK=1; shift ;;
        --accept)
            [[ $# -ge 2 && -n "$2" && "$2" != -* ]] || usage "--accept needs a feature name or all"
            ACCEPT_ARG="$2"
            shift 2
            ;;
        *) usage "unknown argument: $1" ;;
    esac
done
# The walk runs no accept blocks.
[[ "$WALK" == "1" && -n "$ACCEPT_ARG" ]] && usage "--accept cannot be combined with --walk"
# A walk never runs the orchestrator, so a walk started from inside a
# walk means that rule broke: a custom command calling --walk would then
# start walks without end and hang every boundary. Fail closed instead.
if [[ "$WALK" == "1" ]]; then
    [[ -n "${GATES_IN_WALK:-}" ]] \
        && refuse "verify.sh --walk called from inside a walk; the built-in walk never runs the orchestrator"
    export GATES_IN_WALK=1
fi
# Without --boundary the run is the 0.3.6 one (#216): every gate, boundary
# "unspecified". It warns instead of refusing, so callers written for
# 0.3.6 keep working after an upgrade; a later release may require it.
# The walk takes --boundary as optional (research R8), so it does not warn.
if [[ "$BOUNDARY" == "unspecified" && "$WALK" != "1" ]]; then
    echo "gates: warning: verify.sh without --boundary is deprecated; it runs every gate as before, and a later release may refuse it. Pass --boundary agent|git|ci" >&2
fi
# Every gate command (project gates, the task and custom steps, the walk)
# can read the boundary it runs at (feature 006, FR-010); "none" is a run
# without --boundary, at which no skip declaration applies.
GATES_BOUNDARY="$BOUNDARY"
[[ "$BOUNDARY" == "unspecified" ]] && GATES_BOUNDARY="none"
export GATES_BOUNDARY

# GATES_POLICY_FILE replaces the whole policy (#196): set for one command
# it would drop every gate the repository declares. The git and CI
# boundaries judge the policy the repository commits, so they ignore it,
# and so does a run without --boundary (#216); at the agent boundary it
# applies. Either way the run says so (stderr, --json and the
# attestation's policy_override).
POLICY_OVERRIDE="${GATES_POLICY_FILE:-}"
POLICY_OVERRIDE_JSON=""
POLICY_OVERRIDE_APPLIED=""
POLICY_OVERRIDE_WHERE="at the $BOUNDARY boundary"
[[ "$BOUNDARY" == "unspecified" ]] && POLICY_OVERRIDE_WHERE="without --boundary"
if [[ -n "$POLICY_OVERRIDE" ]]; then
    case "$BOUNDARY" in
        git | ci | unspecified)
            unset GATES_POLICY_FILE
            POLICY_OVERRIDE_APPLIED=false
            echo "gates: GATES_POLICY_FILE=$POLICY_OVERRIDE is ignored $POLICY_OVERRIDE_WHERE; the repository's policy applies" >&2
            ;;
        *)
            POLICY_OVERRIDE_APPLIED=true
            echo "gates: GATES_POLICY_FILE=$POLICY_OVERRIDE replaces the repository's policy for this run" >&2
            ;;
    esac
    POLICY_OVERRIDE_JSON="{\"file\":$(json_str "$POLICY_OVERRIDE"),\"applied\":$POLICY_OVERRIDE_APPLIED}"
fi

PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
GATES_DIR="$PROJECT_ROOT/.specify/gates"
POLICY_FILE="$GATES_DIR/policy.json"

# The index of the commit being made, as an absolute path, for the gate
# commands (#293). They run without git's per-repository variables
# (gates_git_env_clear, lib/watch.sh), and `git commit -a` or
# `git commit <paths>` hands the hook a temporary index, so without this
# a command reading the staged files would see the real index instead.
# GIT_INDEX_FILE="$GATES_GIT_INDEX_FILE" git diff --cached --name-only
# lists exactly what the commit holds. A relative GIT_INDEX_FILE is
# relative to the directory git started the hook in; outside a hook this
# is the repository's own index. Never inherited: a run on another
# project (a fixture an accept block checks) names that project's index.
if [[ -n "${GIT_INDEX_FILE:-}" ]]; then
    case "$GIT_INDEX_FILE" in
        /*) GATES_GIT_INDEX_FILE="$GIT_INDEX_FILE" ;;
        *) GATES_GIT_INDEX_FILE="$PWD/$GIT_INDEX_FILE" ;;
    esac
elif GATES_GIT_INDEX_FILE="$(git -C "$PROJECT_ROOT" rev-parse --git-path index 2>/dev/null)"; then
    [[ "$GATES_GIT_INDEX_FILE" == /* ]] || GATES_GIT_INDEX_FILE="$PROJECT_ROOT/$GATES_GIT_INDEX_FILE"
else
    GATES_GIT_INDEX_FILE=""
fi
export GATES_GIT_INDEX_FILE

# A gate command runs with GATES_GATE_ROOT set to its project's root
# (run_gate). A full run started from it on the same root would run that
# gate again, and so on without end: every boundary would hang instead of
# failing. Refuse it (fail closed). The walk runs no gate command, and runs
# on other roots (sandboxes, fixtures) are not nested in themselves.
if [[ "$WALK" != "1" && -n "${GATES_GATE_ROOT:-}" ]] \
    && [[ "$(cd "$PROJECT_ROOT" 2>/dev/null && pwd -P)" == "$(cd "$GATES_GATE_ROOT" 2>/dev/null && pwd -P)" ]]; then
    refuse "verify.sh called from inside a gate of the same project; a gate command must not run the full gate suite (use verify.sh --walk for the built-in walk)"
fi

if ! command -v jq >/dev/null 2>&1; then
    refuse "jq not found — cannot evaluate policy (run /speckit.gates.doctor)"
fi
# The spec gate proves with git that an accept block left the tree
# unchanged; without git that check would pass blind (#121). Exit 1 like
# jq: the Stop hook then lets the session end instead of locking it.
if ! command -v git >/dev/null 2>&1; then
    refuse "git not found — cannot check the working tree (run /speckit.gates.doctor)"
fi
if [[ ! -f "$POLICY_FILE" ]]; then
    refuse "no policy at $POLICY_FILE (run /speckit.gates.init)"
fi

# Path is resolved at runtime from the projected layout; the source= hint helps
# `shellcheck -x` runs, disable=SC1091 silences the unavoidable static miss.
# shellcheck source=lib/policy.sh disable=SC1091
source "$GATES_DIR/lib/policy.sh"
# shellcheck source=lib/attest.sh disable=SC1091
source "$GATES_DIR/lib/attest.sh"
# shellcheck source=lib/watch.sh disable=SC1091
source "$GATES_DIR/lib/watch.sh"
# shellcheck source=lib/timeline.sh disable=SC1091
source "$GATES_DIR/lib/timeline.sh"
# shellcheck source=lib/spec-gate.sh disable=SC1091
source "$GATES_DIR/lib/spec-gate.sh"
# shellcheck source=lib/contract.sh disable=SC1091
source "$GATES_DIR/lib/contract.sh"

# GATES_TIMELINE and GATES_PROGRESS (feature 007) belong to this run: read
# once here and unset, so no command it starts (gate commands, accept
# blocks, the covering script, a nested verify.sh) writes to them (R15).
gates_timeline_init "$BOUNDARY"

# The loader reads the policy fail-open (a missing or unreadable field is a
# default), so a malformed or schema-invalid policy would silently drop
# gates. Refuse it like a missing policy (#124), before any gate runs: the
# same exit 1, which the Stop hook treats as a setup error (it allows the
# stop and says why) and the git and CI boundaries as a failure.
if ! gates_validate_policy "$(gates_policy_file)"; then
    refuse "invalid policy, no gate ran; fix the errors above (run /speckit.gates.doctor)"
fi

# A declared covering script (spec.cover, feature 007) that is not a
# regular file inside the project is a broken configuration (research R5):
# refused like an invalid policy, at every boundary, before any gate runs.
# The walk runs no accept blocks and does not check it.
COVER_SCRIPT="$(gates_cover_script)"
if [[ -n "$COVER_SCRIPT" && "$WALK" != "1" ]] \
    && ! cover_why="$(gates_cover_script_check "$PROJECT_ROOT" "$COVER_SCRIPT")"; then
    refuse "spec.cover.script $COVER_SCRIPT: $cover_why; no gate ran (fix spec.cover in the policy, run /speckit.gates.doctor)"
fi

# An unknown --accept name is an argument error: refuse it here, before
# any gate runs (also under --dry-run), not after the lint gates (#179).
if [[ -n "$ACCEPT_ARG" && "$ACCEPT_ARG" != "all" ]]; then
    if ! grep -qx -- "$ACCEPT_ARG" <<<"$(gates_spec_features "$PROJECT_ROOT")"; then
        AVAILABLE="$(gates_spec_features "$PROJECT_ROOT" | paste -sd ' ' -)"
        refuse "--accept: unknown feature: $ACCEPT_ARG (available: ${AVAILABLE:-none})"
    fi
fi

# ---------------------------------------------------------------------------
# Gate-level lock (#263): two runs in one working tree see each other's
# writes (build output, test files) and fail each other. A run holds the
# directory .specify/gates/.lock (mkdir is atomic; macOS has no flock),
# with its pid, boundary and process start time in .lock/owner. A second
# run at the agent boundary skips (exit 0: the Stop hook must not wait on
# a commit); at git, ci and without --boundary it waits up to
# GATES_LOCK_WAIT_S seconds (default 600), then refuses (exit 1). A lock
# whose process is gone is stale and taken over. A run started from
# inside the holder (an accept block, or verify.sh --walk from a gate
# command; a full run from a gate command is refused above) finds its
# holder's pid in GATES_LOCK_PID and runs without the lock. --dry-run
# takes no lock.
# ---------------------------------------------------------------------------
LOCK_DIR="$GATES_DIR/.lock"
LOCK_HELD=0
LOCK_PID=""
LOCK_BOUNDARY=""
LOCK_START=""

gates_proc_start() { # <pid>: its start time as ps prints it; empty if unknown
    ps -o lstart= -p "$1" 2>/dev/null || true
}

gates_lock_read() { # sets LOCK_PID, LOCK_BOUNDARY, LOCK_START from .lock/owner
    LOCK_PID=""; LOCK_BOUNDARY=""; LOCK_START=""
    [[ -f "$LOCK_DIR/owner" ]] || return 0
    { IFS= read -r LOCK_PID; IFS= read -r LOCK_BOUNDARY; IFS= read -r LOCK_START; } \
        <"$LOCK_DIR/owner" 2>/dev/null || true
    [[ "$LOCK_PID" =~ ^[0-9]+$ ]] || LOCK_PID=""
}

gates_lock_alive() { # true if the process in LOCK_PID still runs (same start)
    [[ -n "$LOCK_PID" ]] || return 1
    kill -0 "$LOCK_PID" 2>/dev/null || ps -p "$LOCK_PID" >/dev/null 2>&1 || return 1
    # A reused pid (the holder died, e.g. kill -9 or a reboot) is another
    # process: compare start times when ps reports them.
    local now
    now="$(gates_proc_start "$LOCK_PID")"
    [[ -z "$LOCK_START" || -z "$now" || "$now" == "$LOCK_START" ]]
}

# shellcheck disable=SC2329  # called from the traps below
gates_lock_release() {
    rm -f "/tmp/gates-out.$$" "/tmp/gates-out.$$.view" "/tmp/gates-out.$$.timedout"
    [[ "$LOCK_HELD" == "1" ]] || return 0
    LOCK_HELD=0
    gates_lock_read
    [[ "$LOCK_PID" == "$$" ]] && rm -rf "$LOCK_DIR"
    return 0
}

# What an agent-boundary run that skips under a held lock does not run
# (FR-019, constitution Principle III: every gate a boundary does not run is
# named). From the policy, in run order: contract when the policy extends a
# baseline, the orchestrator's gates (the enabled walk tools, task-lint and
# task-test, or custom), the project gates not skipped at agent by
# declaration, spec unless disabled, parity unless off. Sets LOCK_DEFERRED
# and LOCK_DECLARED (the gates skip_at keeps out of the agent boundary
# anyway) as JSON arrays; returns 1 when the policy cannot be read.
gates_lock_deferred() {
    LOCK_DEFERRED="[]"
    LOCK_DECLARED="[]"
    local file orch tool name deferred="" declared=""
    file="$(gates_policy_file)"
    [[ -f "$file" ]] && jq -e 'type == "object"' "$file" >/dev/null 2>&1 || return 1
    if [[ "$WALK" == "1" ]]; then
        # --walk runs the enabled walk tools and nothing else.
        for tool in prettier markdownlint shellcheck; do
            [[ -n "$(gates_policy_list "$tool" "include")" ]] && deferred="$deferred$tool"$'\n'
        done
        LOCK_DEFERRED="$(printf '%s' "$deferred" | jq -Rsc 'split("\n") | map(select(. != ""))')" || return 1
        return 0
    fi
    gates_contract_declared "$GATES_DIR/policy.json" && deferred="contract"$'\n'
    orch="$(gates_policy_get "verify-quality" "orchestrator")"
    case "$orch" in
        task) deferred="${deferred}task-lint"$'\n'"task-test"$'\n' ;;
        custom)
            [[ -n "$(gates_policy_get "verify-quality" "custom_command")" ]] \
                && deferred="${deferred}custom"$'\n'
            ;;
        *)
            for tool in prettier markdownlint shellcheck; do
                [[ -n "$(gates_policy_list "$tool" "include")" ]] && deferred="$deferred$tool"$'\n'
            done
            ;;
    esac
    while IFS= read -r name; do
        [[ -n "$name" ]] || continue
        if gates_declared_skip agent "$(gates_project_gate_skip_at "$name")" >/dev/null; then
            declared="$declared$name"$'\n'
        else
            deferred="$deferred$name"$'\n'
        fi
    done < <(gates_project_gate_names)
    [[ "$(gates_policy_section_get spec enabled)" != "false" ]] && deferred="${deferred}spec"$'\n'
    if [[ "$(gates_policy_section_get attestation enabled)" != "false" \
        && "$(gates_policy_section_get attestation parity)" != "off" ]]; then
        deferred="${deferred}parity"$'\n'
    fi
    LOCK_DEFERRED="$(printf '%s' "$deferred" | jq -Rsc 'split("\n") | map(select(. != ""))')" || return 1
    LOCK_DECLARED="$(printf '%s' "$declared" | jq -Rsc 'split("\n") | map(select(. != ""))')" || return 1
}

gates_lock_acquire() {
    local limit="${GATES_LOCK_WAIT_S:-600}" waited=0 announced=0 unowned=0 own_start
    [[ "$limit" =~ ^[0-9]+$ ]] || limit=600
    # Known before the mkdir, so owner follows it within a printf and a mv.
    own_start="$(gates_proc_start $$)"
    while :; do
        if mkdir "$LOCK_DIR" 2>/dev/null; then
            LOCK_HELD=1
            if ! printf '%s\n%s\n%s\n' "$$" "$BOUNDARY" "$own_start" \
                    >"$LOCK_DIR/owner.$$" 2>/dev/null \
                || ! mv -f "$LOCK_DIR/owner.$$" "$LOCK_DIR/owner" 2>/dev/null; then
                echo "gates: warning: could not write $LOCK_DIR/owner; another gate run may treat the lock as stale" >&2
            fi
            export GATES_LOCK_PID="$$"
            return 0
        fi
        if [[ ! -e "$LOCK_DIR" ]]; then
            # Not creatable (a read-only checkout): run unlocked, say so.
            echo "gates: warning: cannot create $LOCK_DIR; running without the gate lock" >&2
            return 0
        fi
        gates_lock_read
        if [[ -z "$LOCK_PID" ]]; then
            # The holder writes owner right after its mkdir; give it a few
            # seconds before an ownerless lock counts as stale.
            unowned=$((unowned + 1))
            if [[ "$unowned" -lt 3 ]]; then
                sleep 1
                waited=$((waited + 1))
                continue
            fi
        elif [[ "$LOCK_PID" == "${GATES_LOCK_PID:-}" ]] && gates_lock_alive; then
            return 0 # nested in the holder's run
        elif gates_lock_alive; then
            unowned=0
            if [[ "$BOUNDARY" == "agent" ]]; then
                local why="skipped: the gate run of pid $LOCK_PID ($LOCK_BOUNDARY) is in progress"
                local named deferred_json="null" declared_field=""
                if gates_lock_deferred; then
                    deferred_json="$LOCK_DEFERRED"
                    named="; deferred: $(jq -r 'if length == 0 then "none" else join(", ") end' <<<"$LOCK_DEFERRED")"
                    if [[ "$LOCK_DECLARED" != "[]" ]]; then
                        named="$named; skipped by declaration at agent: $(jq -r 'join(", ")' <<<"$LOCK_DECLARED")"
                        declared_field=",\"declared_skip\":$LOCK_DECLARED"
                    fi
                else
                    # The policy could not be read here: name everything,
                    # and say deferred is unknown (null) under --json.
                    named="; deferred: all gates (policy not read)"
                fi
                echo "gates: $why$named" >&2
                if [[ "$JSON" == "1" ]]; then
                    printf '{"result":"skipped","boundary":%s,"reason":%s,"deferred":%s%s}\n' \
                        "$(json_str "$BOUNDARY")" "$(json_str "$why")" "$deferred_json" "$declared_field"
                fi
                exit 0
            fi
            if [[ "$waited" -ge "$limit" ]]; then
                refuse "the gate run of pid $LOCK_PID ($LOCK_BOUNDARY) still holds $LOCK_DIR after ${limit}s, no gate ran (GATES_LOCK_WAIT_S sets the wait)"
            fi
            if [[ "$announced" == "0" ]]; then
                echo "gates: waiting for the gate run of pid $LOCK_PID ($LOCK_BOUNDARY)" >&2
                announced=1
            fi
            sleep 1
            waited=$((waited + 1))
            continue
        fi
        echo "gates: note: taking over the stale gate lock of pid ${LOCK_PID:-unknown} (${LOCK_BOUNDARY:-unknown}), which is no longer running" >&2
        unowned=0
        # Renamed first, then checked: of two runs taking over at once, the
        # later mv may catch the lock the first one just made, and puts it
        # back. Best effort; the window is a few file operations wide.
        local stale_pid="$LOCK_PID" moved="$LOCK_DIR.stale.$$"
        if mv "$LOCK_DIR" "$moved" 2>/dev/null; then
            LOCK_PID=""
            [[ -f "$moved/owner" ]] && { IFS= read -r LOCK_PID <"$moved/owner" || true; } 2>/dev/null
            # (mv onto an existing directory would move it inside.)
            if [[ -n "$LOCK_PID" && "$LOCK_PID" != "$stale_pid" && ! -e "$LOCK_DIR" ]] \
                && mv "$moved" "$LOCK_DIR" 2>/dev/null; then
                continue
            fi
            rm -rf "$moved" 2>/dev/null || true
        elif [[ -e "$LOCK_DIR" ]]; then
            echo "gates: warning: cannot remove the stale $LOCK_DIR; running without the gate lock" >&2
            return 0
        fi
    done
}

if [[ "$DRY_RUN" != "1" ]]; then
    trap 'gates_lock_release' EXIT
    trap 'gates_lock_release; exit 130' INT
    trap 'gates_lock_release; exit 143' TERM
    gates_lock_acquire
fi

# A failing or warning gate's full output (#309) goes to
# .specify/gates/output/<gate>.log: the report names the file, and at ci
# prints the output in the job log, where the forge masks its secrets. A
# file is never masked, so at ci it is first scanned with the secret rules
# (lib/secrets.sh): output that looks like it holds a secret is deleted
# and withheld from the log too, and the report says which rule matched.
# The templates upload no file; a project that does so opts in (docs). Only the
# run that holds the gate lock writes there (not --dry-run, and not a run
# nested in another: a walk inside a gate command, a run in an accept
# block). A gate's file is replaced or removed only when that gate runs:
# a pass removes it, a failure or warning replaces it, and a gate this run
# does not run (skipped by declaration at this boundary, or a deferred run)
# keeps the file of its last run, so an agent-boundary run never deletes
# what a ci run kept. The directory ignores itself (a .gitignore of "*").
# The attestation never carries the output.
OUTPUT_DIR=""
if [[ "$LOCK_HELD" == "1" ]]; then
    OUTPUT_DIR="$GATES_DIR/output"
fi

# The kept-output file of gate <name>.
gates_output_file() { # <gate-name>
    printf '%s/%s.log' "$OUTPUT_DIR" "$(printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_')"
}

# Remove gate <name>'s kept output: it ran and has nothing to keep.
gates_output_drop() { # <gate-name>
    [[ -n "$OUTPUT_DIR" ]] || return 0
    rm -f "$(gates_output_file "$1")" 2>/dev/null || true
}

# Keep /tmp/gates-out.$$.view (run_gate's output, markers stripped) as
# <gate>.log; prints its path relative to the project root, or at ci
# "withheld:<reason>" when the secret scan matched or could not run (the
# file is then removed: fail closed). Best effort otherwise: a file that
# cannot be written never changes the gate's outcome.
gates_output_keep() { # <gate-name>
    local f rc=0
    [[ -n "$OUTPUT_DIR" ]] || return 1
    f="$(gates_output_file "$1")"
    { mkdir -p "$OUTPUT_DIR" \
        && { [[ -f "$OUTPUT_DIR/.gitignore" ]] || printf '*\n' >"$OUTPUT_DIR/.gitignore"; } \
        && cp "/tmp/gates-out.$$.view" "$f"; } 2>/dev/null || return 1
    if [[ "$BOUNDARY" == "ci" ]]; then
        if ! declare -F gates_secret_scan_file >/dev/null; then
            # shellcheck source=lib/secrets.sh disable=SC1091
            source "$GATES_DIR/lib/secrets.sh" 2>/dev/null || true
        fi
        if declare -F gates_secret_scan_file >/dev/null; then
            gates_secret_scan_file "$f" || rc=$?
        else
            rc=2
        fi
        if [[ "$rc" -ne 0 ]]; then
            rm -f "$f"
            if [[ "$rc" -eq 1 ]]; then
                printf 'withheld:it looks like it holds a secret (%s)\n' "${GATES_SECRET_WHAT:-secret rule}"
            else
                printf 'withheld:it could not be scanned for secrets\n'
            fi
            return 0
        fi
    fi
    printf '%s\n' "${f#"$PROJECT_ROOT"/}"
}

# Print a failed gate's kept output in the ci job log, in a collapsed
# section where the forge has one (GitHub Actions ::group::, GitLab CI
# section markers), else its last 40 lines. Each line is indented, so a
# line of the output can never act as a workflow command.
gates_output_log() { # <gate-name> <file>
    local id
    if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
        echo "::group::$1: full output"
        sed 's/^/    | /' "$2" 2>/dev/null || true
        echo "::endgroup::"
    elif [[ "${GITLAB_CI:-}" == "true" ]]; then
        id="gates_$(printf '%s' "$1" | tr -c 'A-Za-z0-9_' '_')"
        printf '\033[0Ksection_start:%s:%s[collapsed=true]\r\033[0K%s\n' "$(date +%s)" "$id" "$1: full output"
        sed 's/^/    | /' "$2" 2>/dev/null || true
        printf '\033[0Ksection_end:%s:%s\r\033[0K\n' "$(date +%s)" "$id"
    else
        echo "    last 40 lines:"
        tail -n 40 "$2" 2>/dev/null | sed 's/^/    | /' || true
    fi
}

FAILED=0
WARNINGS=0
declare -a RESULTS=()
declare -a SUMMARIES=()
# The kept output file of each RESULTS entry, or "" (gates_output_keep).
declare -a OUTPUTS=()
declare -a ATT_GATES=()
# Tools whose pins were looked up; the parity gate checks their pin
# sources for conflicts and unreadable mise files (#260).
declare -a PIN_TOOLS=()

# <summary> is the one line the text report prints for the gate; without
# it the report prints the first line of <detail>.
record() { # name status detail [summary]
    RESULTS+=("{\"name\":\"$1\",\"status\":\"$2\",\"detail\":$(jq -Rn --arg d "$3" '$d')}")
    SUMMARIES+=("${4:-}")
    OUTPUTS+=("")
}

# Append a GateEntry (attestation shape, data-model.md) for an evaluated
# gate. Empty strings become JSON null for the nullable fields.
# [declared_skip] is the boundary a skip declaration named (feature 006):
# written only when set, so every other entry keeps the feature 001 shape.
att_gate() { # name bin version pinned candidates checked result reason duration_s [declared_skip]
    ATT_GATES+=("$(jq -cn \
        --arg name "$1" --arg bin "$2" --arg version "$3" --arg pinned "$4" \
        --arg candidates "$5" --arg checked "$6" --arg result "$7" \
        --arg reason "$8" --arg duration "$9" --arg declared_skip "${10:-}" '
        def num_or_null: if . == "" then null else tonumber end;
        def str_or_null: if . == "" then null else . end;
        { name: $name,
          bin: ($bin | str_or_null),
          version: ($version | str_or_null),
          pinned: ($pinned | str_or_null),
          candidates: ($candidates | num_or_null),
          checked: ($checked | num_or_null),
          result: $result,
          reason: $reason,
          duration_s: ($duration | tonumber) }
        + (if $declared_skip == "" then {} else { declared_skip: $declared_skip } end)')")
}

# Watched mode (project gates, feature 006): run_project_gate sets
# WATCH_S to the gate's time limit and WATCH_CMD to its command for one
# run_gate call. The command then runs from the project root under
# gates_run_watched (lib/watch.sh), in this shell, so GATES_WATCH_DETAIL
# survives; everything else (output capture, the summary rules, the
# severity mapping, FAILED/WARNINGS, att_gate) is the same code path.
WATCH_S=""
WATCH_CMD=""

# Run <cmd...> for run_gate in a subshell without git's per-repository
# variables (#293, lib/watch.sh), from the project root when <root> is 1,
# its output to /tmp/gates-out.$$. With GATES_VERBOSE=1 the output also
# goes to stderr as it is written (#309); the command's status is
# returned either way (pipefail: tee succeeds).
gate_exec() { # <root 0|1> <cmd...>
    local root="$1"
    shift
    if [[ "${GATES_VERBOSE:-}" == "1" ]]; then
        gate_exec_in "$root" "$@" 2>&1 | tee "/tmp/gates-out.$$" >&2
    else
        gate_exec_in "$root" "$@" >"/tmp/gates-out.$$" 2>&1
    fi
}
gate_exec_in() { # <root 0|1> <cmd...>
    local root="$1"
    shift
    (
        if [[ "$root" == "1" ]]; then cd "$PROJECT_ROOT" || exit 1; fi
        gates_git_env_clear
        GATES_GATE_ROOT="$PROJECT_ROOT" "$@"
    )
}

run_gate() { # name severity cmd...
    local name="$1" severity="$2"; shift 2
    if [[ "$DRY_RUN" == "1" ]]; then
        if [[ -n "$WATCH_S" ]]; then
            record "$name" "planned" "$WATCH_CMD (time limit ${WATCH_S}s)"
        else
            record "$name" "planned" "$*"
        fi
        return 0
    fi
    local start end rc=0
    gates_progress_start "$name"
    [[ "${GATES_VERBOSE:-}" == "1" ]] && echo "gates: output of $name (GATES_VERBOSE=1):" >&2
    start="$(date +%s)"
    if [[ -n "$WATCH_S" ]]; then
        local here="$PWD"
        # Read by gates_run_watched: stream the output too (GATES_VERBOSE).
        # shellcheck disable=SC2034
        local GATES_WATCH_STREAM="${GATES_VERBOSE:-}"
        cd "$PROJECT_ROOT" || return 1
        GATES_GATE_ROOT="$PROJECT_ROOT" gates_run_watched "$WATCH_S" "/tmp/gates-out.$$" "$@" || rc=$?
        cd "$here" || true
    elif [[ "$name" == "custom" ]]; then
        # The custom command runs from the project root, as project gates
        # do, wherever verify.sh was started (#276).
        gate_exec 1 "$@" || rc=$?
    else
        gate_exec 0 "$@" || rc=$?
    fi
    end="$(date +%s)"

    # Check-mode metadata (##gates-meta##, emitted by formatter-dispatch) is
    # machinery, not gate output: parse it for the attestation entry, strip
    # it from the visible detail. Non-file gates (task/custom) have none,
    # and a project gate's is never read: its command could print one and
    # claim a tool, counts or a skip.
    local meta="" binname="" bin="" candidates="" checked="" skipped=""
    if [[ -z "$WATCH_S" ]]; then
        meta="$(grep '##gates-meta##' /tmp/gates-out.$$ 2>/dev/null | tail -n 1 || true)"
    fi
    if [[ -n "$meta" ]]; then
        binname="$(printf '%s' "$meta" | sed -n 's/.* binname=\([^ ]*\).*/\1/p')"
        candidates="$(printf '%s' "$meta" | sed -n 's/.* candidates=\([0-9]*\).*/\1/p')"
        checked="$(printf '%s' "$meta" | sed -n 's/.* checked=\([0-9]*\).*/\1/p')"
        skipped="$(printf '%s' "$meta" | sed -n 's/.* skipped=\([^ ]*\).*/\1/p')"
        bin="$(printf '%s' "$meta" | sed -n 's/.* bin=//p')"
    fi
    # The detail keeps the output's last 2000 bytes, whose first line is
    # wherever the cut fell (#262). The summary is read from the whole
    # output instead: its last "##gates-fail## <text>" line (the file
    # linters' dispatch marks their first finding so), else, for a file
    # linter, the first non-empty line, and for a task, custom or project
    # step, which ends on its verdict, the last one (with the first, #296).
    # The marker is machinery too and never shows in the detail. A marked
    # line whose text another line of the output already holds (ANSI colors
    # aside) is dropped from the detail, so the failure is not named twice
    # (#279): the file linters' marks and a nested `verify.sh --walk`'s
    # repeat a line they printed. A marked text found nowhere else stays,
    # without the marker: it may be a step's only statement of what
    # failed. A file linter's marks are always dropped, since shellcheck's
    # joins two of its lines.
    # Three passes over the file: collect the marked texts (the first 20
    # distinct ones, so the search stays linear), find which appear in
    # another line, print. The printed output is the file a failing gate
    # keeps (#309, gates_output_keep).
    local detail summary
    awk -v esc="$(printf '\033')" -v all="${meta:+1}" '
        function plain(s) { gsub(esc "\\[[0-9;]*[A-Za-z]", "", s); return s }
        function text(s) {
            sub(/.*##gates-fail##[ \t]*/, "", s); s = plain(s)
            sub(/[ \t\r]+$/, "", s); return s
        }
        FNR == 1 { pass++ }
        /##gates-meta##/ { next }
        pass == 1 {
            if (/##gates-fail##/ && n < 20) {
                t = text($0)
                if (t != "" && !(t in mark)) { mark[t] = 0; n++ }
            }
            next
        }
        pass == 2 {
            if (/##gates-fail##/) next
            p = plain($0)
            for (t in mark) if (!mark[t] && index(p, t)) mark[t] = 1
            next
        }
        /##gates-fail##/ {
            if (all || mark[text($0)]) next
            sub(/##gates-fail##[ \t]*/, "")
        }
        { print }' /tmp/gates-out.$$ /tmp/gates-out.$$ /tmp/gates-out.$$ \
        >"/tmp/gates-out.$$.view" 2>/dev/null || true
    detail="$(tail -c 2000 "/tmp/gates-out.$$.view" 2>/dev/null || true)"
    summary="$(grep '##gates-fail##' /tmp/gates-out.$$ 2>/dev/null | tail -n 1 \
        | sed 's/.*##gates-fail##[[:space:]]*//' || true)"
    # Without a marked line, a file linter shows its first non-empty line.
    # Any other step shows its last one, preceded by its first when they
    # differ, "<first> | <last>" (#296): tools that print the location
    # first and the message last (uv's "TOML parse error at line 1 ...")
    # keep both in the one line.
    if [[ -z "$summary" ]]; then
        summary="$(awk -v linter="${meta:+1}" '
            /##gates-meta##/ || /^[ \t\r\f\v]*$/ { next }
            !n++ { first = $0 }
            { last = $0 }
            END {
                if (!n) exit
                if (linter || first == last) print first
                else print first " | " last
            }' /tmp/gates-out.$$ 2>/dev/null || true)"
    fi
    rm -f /tmp/gates-out.$$

    local version="" pinned=""
    if [[ -n "$binname" ]]; then
        version="$(gates_tool_version "$binname" "$PROJECT_ROOT" "$bin")"
        pinned="$(gates_pin_version "$binname" "$PROJECT_ROOT")"
        PIN_TOOLS+=("$binname")
    fi
    [[ -n "$bin" && "$bin" == "$PROJECT_ROOT/"* ]] && bin="${bin#"$PROJECT_ROOT"/}"

    # A missing tool is skipped with a reason, never pass (spec edge case).
    # The attestation reason carries no tool output: output can quote file
    # contents, which FR-011 keeps out of records.
    # A project gate that timed out or could not start fails whatever its
    # severity (fail closed, research R3/R4); the reasons are fixed strings.
    local result reason=""
    if [[ -n "$WATCH_S" && "$rc" -eq 124 && -n "${GATES_WATCH_DETAIL:-}" ]]; then
        result="fail"; reason="$GATES_WATCH_DETAIL"; summary="$GATES_WATCH_DETAIL"
    elif [[ -n "$WATCH_S" && ( "$rc" -eq 126 || "$rc" -eq 127 ) ]]; then
        result="fail"; reason="command not found or not executable"
        [[ -n "$summary" ]] || summary="$reason"
    elif [[ "$rc" -eq 0 && -n "$skipped" ]]; then
        result="skipped"; reason="$binname not installed"
    elif [[ "$rc" -eq 0 ]]; then
        result="pass"
    elif [[ "$severity" == "error" ]]; then
        result="fail"; reason="check failed"
    else
        result="warn"; reason="check failed (warning)"
    fi

    case "$result" in
        pass) record "$name" "pass" "" ;;
        skipped) record "$name" "skipped" "$reason" ;;
        fail) record "$name" "fail" "$detail" "$summary"; FAILED=$((FAILED + 1)) ;;
        warn) record "$name" "warn" "$detail" "$summary"; WARNINGS=$((WARNINGS + 1)) ;;
    esac
    if [[ "$result" == "fail" || "$result" == "warn" ]]; then
        OUTPUTS[${#OUTPUTS[@]} - 1]="$(gates_output_keep "$name" || true)"
    else
        gates_output_drop "$name"
    fi
    rm -f "/tmp/gates-out.$$.view"
    att_gate "$name" "$bin" "$version" "$pinned" "$candidates" "$checked" \
        "$result" "$reason" "$((end - start))"
    gates_timeline_line "$name" "$start" "$end" "$result" "$reason"
    gates_progress_end "$name" "$result" "$((end - start))"
}

# Timeline and progress for a gate evaluated outside run_gate (contract,
# spec, parity): the timeline reason is fixed, never the gate's detail.
gate_timeline_end() { # name start end result
    local reason=""
    case "$4" in
        fail) reason="check failed" ;;
        warn) reason="check failed (warning)" ;;
    esac
    gates_timeline_line "$1" "$2" "$3" "$4" "$reason"
    gates_progress_end "$1" "$4" "$(($3 - $2))"
}

# A gate that does not start (a declared skip, the spec gate under
# GATES_SPEC_EXEC): its end line alone, with <reason>.
gate_timeline_skipped() { # name reason
    local now
    now="$(date +%s)"
    gates_timeline_line "$1" "$now" "$now" skipped "$2"
    gates_progress_end "$1" skipped 0
}

# ---------------------------------------------------------------------------
# Contract gate (feature 003): when policy.json extends a versioned baseline,
# prove — offline, before any gate consults the policy — that the committed
# snapshot matches the pin, the effective policy matches recomputation, and
# the declaration matches what was synced. Policy integrity precedes policy
# enforcement, so this runs before the tool gates. Dormant (no extends): no
# gate entry at all. Deviations are informational and never change the exit
# code (FR-006).
# ---------------------------------------------------------------------------
CONTRACT_ATT_JSON=""
# A declared baseline is what makes the gate live (gates_contract_check).
[[ "$WALK" != "1" && "$DRY_RUN" != "1" ]] && gates_contract_declared "$GATES_DIR/policy.json" \
    && gates_progress_start contract
contract_start="$(date +%s)"
if [[ "$WALK" == "1" ]]; then
    CONTRACT_STATUS="dormant" # --walk runs the built-in walk only
else
    gates_contract_check "$PROJECT_ROOT"
fi
contract_end="$(date +%s)"
if [[ "$CONTRACT_STATUS" != "dormant" ]]; then
    if [[ "$DRY_RUN" == "1" ]]; then
        record "contract" "planned" "baseline contract integrity (extends)"
    elif [[ "$CONTRACT_STATUS" == "pass" ]]; then
        record "contract" "pass" ""
        att_gate "contract" "" "" "" "" "" "pass" "" "$((contract_end - contract_start))"
        gate_timeline_end contract "$contract_start" "$contract_end" pass
        if [[ "$JSON" == "0" && -n "$CONTRACT_DEVIATIONS" ]]; then
            gates_contract_print_deviations "contract: deviation " <<<"$CONTRACT_DEVIATIONS"
        fi
    else
        # A broken contract is never a warning: fixed error severity (R6).
        record "contract" "fail" "$CONTRACT_DETAIL"
        FAILED=$((FAILED + 1))
        att_gate "contract" "" "" "" "" "" "fail" "$CONTRACT_DETAIL" "$((contract_end - contract_start))"
        gate_timeline_end contract "$contract_start" "$contract_end" fail
    fi
    if [[ "$DRY_RUN" != "1" ]]; then
        CONTRACT_ATT_JSON="$(jq -cn --arg s "$CONTRACT_SOURCE" --arg v "$CONTRACT_VERSION" \
            --arg d "$CONTRACT_PIN_DIGEST" --arg e "$CONTRACT_EFFECTIVE_SHA256" \
            --argjson w "${CONTRACT_WEAKENED:-0}" --argjson c "${CONTRACT_CHANGED:-0}" '
            { source: $s, version: $v,
              digest: (if $d == "" then null else $d end),
              effective_sha256: (if $e == "" then null else $e end),
              deviations: { weakened: $w, changed: $c } }')"
    fi
fi

# ---------------------------------------------------------------------------
# Gate dispatch. Orchestrator semantics carried over from CPF (ADR-005):
#   "none"   -> per-tool walk driven by policy include/exclude globs
#   "task"   -> `task lint` (error class) and `task test` (severity from
#               hooks.verify-quality.test_severity, default warning; 006)
#   "custom" -> policy-supplied command, exit code mapped via severity
# The per-tool walk delegates to lib/formatter-dispatch.sh in check mode.
# ---------------------------------------------------------------------------
ORCH="$(gates_policy_get "verify-quality" "orchestrator")"
[[ -z "$ORCH" ]] && ORCH="none"
# --walk is the per-tool walk whatever the orchestrator (research R8).
[[ "$WALK" == "1" ]] && ORCH="none"

case "$ORCH" in
    task)
        TEST_SEV="$(gates_policy_get "verify-quality" "test_severity")"
        [[ -z "$TEST_SEV" ]] && TEST_SEV="warning"
        run_gate "task-lint" "error"     task lint
        run_gate "task-test" "$TEST_SEV" task test
        ;;
    custom)
        CUSTOM_CMD="$(gates_policy_get "verify-quality" "custom_command")"
        SEV="$(gates_policy_get "verify-quality" "severity")"; [[ -z "$SEV" ]] && SEV="error"
        [[ -n "$CUSTOM_CMD" ]] && run_gate "custom" "$SEV" sh -c "$CUSTOM_CMD"
        ;;
    none|*)
        for tool in prettier markdownlint shellcheck; do
            # A hook is "enabled" when the policy declares include globs for it.
            if [[ -n "$(gates_policy_list "$tool" "include")" ]]; then
                SEV="$(gates_policy_get "$tool" "severity")"; [[ -z "$SEV" ]] && SEV="error"
                run_gate "$tool" "$SEV" \
                    bash "$GATES_DIR/lib/formatter-dispatch.sh" --check --tool "$tool" \
                         --project-root "$PROJECT_ROOT"
            fi
        done
        ;;
esac

# ---------------------------------------------------------------------------
# Project gates (feature 006): hooks.verify-quality.gates, run after the
# orchestrator's gates whatever it is, one at a time in key order, each as
# `sh -c "<command>"` from the project root under its time limit (timeout_s,
# else gate_timeout_s, else 600). Each is its own gate in the report,
# --json and the attestation; the tool fields stay null (a command, not a
# resolved tool). A timeout or a command that cannot start (126, 127)
# fails; any other non-zero exit maps through the gate's severity.
# A gate whose skip_at names this boundary (agent or git; never ci) does not
# start: it is recorded as skipped "by declaration at <boundary>", counts as
# neither passed, failed nor warned, and its attestation entry carries
# declared_skip (gates_declared_skip, lib/policy.sh, research R6).
# ---------------------------------------------------------------------------
run_project_gate() { # name severity timeout-s command
    WATCH_S="$3"
    WATCH_CMD="$4"
    local rc=0
    run_gate "$1" "$2" sh -c "$4" || rc=$?
    WATCH_S=""
    WATCH_CMD=""
    return "$rc"
}

declare -a PROJECT_GATES=()
if [[ "$WALK" != "1" ]]; then
    while IFS= read -r pg_name; do
        [[ -n "$pg_name" ]] && PROJECT_GATES+=("$pg_name")
    done < <(gates_project_gate_names)
fi
if [[ ${#PROJECT_GATES[@]} -gt 0 ]]; then
    PG_DEFAULT_S="$(gates_policy_get "verify-quality" "gate_timeout_s")"
    [[ -z "$PG_DEFAULT_S" ]] && PG_DEFAULT_S=600
    for pg_name in "${PROJECT_GATES[@]}"; do
        pg_sev="$(gates_project_gate_get "$pg_name" severity)"
        [[ -z "$pg_sev" ]] && pg_sev="error"
        pg_s="$(gates_project_gate_get "$pg_name" timeout_s)"
        [[ -z "$pg_s" ]] && pg_s="$PG_DEFAULT_S"
        if pg_skip="$(gates_declared_skip "$BOUNDARY" "$(gates_project_gate_skip_at "$pg_name")")"; then
            if [[ "$DRY_RUN" == "1" ]]; then
                record "$pg_name" "planned" "$(gates_project_gate_get "$pg_name" command) (time limit ${pg_s}s; $pg_skip)"
            else
                record "$pg_name" "skipped" "$pg_skip"
                att_gate "$pg_name" "" "" "" "" "" "skipped" "$pg_skip" 0 "$BOUNDARY"
                gate_timeline_skipped "$pg_name" "$pg_skip"
            fi
            continue
        fi
        run_project_gate "$pg_name" "$pg_sev" "$pg_s" "$(gates_project_gate_get "$pg_name" command)"
    done
fi

# ---------------------------------------------------------------------------
# Spec-conformance gate (feature 002): fenced accept blocks in specs/*/tasks.md
# run as executable acceptance criteria. Complete features (spec.md Status:
# Complete) are enforced at spec.severity; the rest is informational.
# GATES_SPEC_EXEC is the recursion guard: accept blocks export it, so a block
# that invokes verify.sh does not re-enter accept-block execution. Any caller
# can set it, so a guarded run records the spec gate as skipped with the
# reason (text, --json and the attestation) instead of dropping it (#164).
# ---------------------------------------------------------------------------
SPEC_ENABLED="$(gates_policy_section_get spec enabled)"
[[ "$WALK" == "1" ]] && SPEC_ENABLED="false"
SPEC_ATT_JSON=""
if [[ "$SPEC_ENABLED" != "false" && -n "${GATES_SPEC_EXEC:-}" ]]; then
    SPEC_SKIP_REASON="GATES_SPEC_EXEC is set (run from inside an accept block), so accept blocks did not run"
    record "spec" "skipped" "$SPEC_SKIP_REASON"
    if [[ "$DRY_RUN" != "1" ]]; then
        att_gate "spec" "" "" "" "" "" "skipped" "$SPEC_SKIP_REASON" 0
        gate_timeline_skipped spec "$SPEC_SKIP_REASON"
    fi
elif [[ "$SPEC_ENABLED" != "false" ]]; then
    if [[ "$DRY_RUN" == "1" ]]; then
        record "spec" "planned" "spec conformance (accept blocks)"
    else
        SPEC_SEV="$(gates_policy_section_get spec severity)"
        [[ -z "$SPEC_SEV" ]] && SPEC_SEV="error"
        gates_progress_start spec
        spec_start="$(date +%s)"
        gates_spec_gate "$PROJECT_ROOT" "$ACCEPT_ARG" "$JSON"
        spec_end="$(date +%s)"
        SPEC_TL_RESULT="pass"
        # Attestation candidates/checked count accept blocks of ENFORCED
        # features, not features discovered: a Complete feature with zero
        # blocks is a legitimate "nothing to check", and counting features
        # made it indistinguishable from a silently broken gate — doctor's
        # no-op heuristic then false-positived on healthy IaC/docs repos
        # (issue #32). Enforced blocks that exist but never execute keep
        # producing the real no-op signature.
        SPEC_ENF_BLOCKS="$(jq -r \
            '[.[] | select(.outcome | startswith("enforced")) | .blocks_parsed] | add // 0' \
            <<<"$SPEC_RESULTS_JSON")"
        # A covered block (feature 007) was looked at, by the covering
        # script, so it counts as checked: a run whose every block was
        # covered is not the no-op signature doctor looks for.
        SPEC_ENF_EXECUTED="$(jq -r \
            '[.[] | select(.outcome | startswith("enforced")) | .blocks_executed + (.blocks_covered // 0)] | add // 0' \
            <<<"$SPEC_RESULTS_JSON")"
        if [[ "$SPEC_RESULT" == "pass" && "$SPEC_COVERED" -gt 0 ]]; then
            # The summary names every covered block (Principle III).
            record "spec" "pass" "$SPEC_DETAIL"
            att_gate "spec" "" "" "" "$SPEC_ENF_BLOCKS" "$SPEC_ENF_EXECUTED" \
                "pass" "" "$((spec_end - spec_start))"
        elif [[ "$SPEC_RESULT" == "pass" ]]; then
            record "spec" "pass" ""
            att_gate "spec" "" "" "" "$SPEC_ENF_BLOCKS" "$SPEC_ENF_EXECUTED" \
                "pass" "" "$((spec_end - spec_start))"
        elif [[ "$SPEC_SEV" == "error" ]]; then
            record "spec" "fail" "$SPEC_DETAIL"
            FAILED=$((FAILED + 1))
            SPEC_TL_RESULT="fail"
            att_gate "spec" "" "" "" "$SPEC_ENF_BLOCKS" "$SPEC_ENF_EXECUTED" \
                "fail" "$SPEC_DETAIL" "$((spec_end - spec_start))"
        else
            record "spec" "warn" "$SPEC_DETAIL"
            WARNINGS=$((WARNINGS + 1))
            SPEC_TL_RESULT="warn"
            att_gate "spec" "" "" "" "$SPEC_ENF_BLOCKS" "$SPEC_ENF_EXECUTED" \
                "warn" "$SPEC_DETAIL" "$((spec_end - spec_start))"
        fi
        gate_timeline_end spec "$spec_start" "$spec_end" "$SPEC_TL_RESULT"
        SPEC_ATT_JSON="$(jq -cn --argjson features "$SPEC_FEATURES" \
            --argjson parsed "$SPEC_PARSED" --argjson executed "$SPEC_EXECUTED" \
            --argjson passed "$SPEC_PASSED" --argjson failed "$SPEC_FAILED" \
            --argjson results "$SPEC_RESULTS_JSON" '
            { features: $features, parsed: $parsed, executed: $executed,
              passed: $passed, failed: $failed, results: $results }')"
    fi
fi

ATT_ENABLED="$(gates_policy_section_get attestation enabled)"
# --walk writes no attestation and so runs no parity gate: the run that
# calls it attests its own gate (research R8).
[[ "$WALK" == "1" ]] && ATT_ENABLED="false"

# ---------------------------------------------------------------------------
# Synthetic parity gate (R7): every boundary compares the tool versions just
# detected against the lockfile pins, so agent, git, and CI runs are proven
# equivalent transitively — no attestation transport needed. Severity comes
# from attestation.parity (default error; warning reports without failing;
# off omits the entry). Disabled attestations disable parity too. --dry-run
# lists it as planned, like every other gate it would run.
# ---------------------------------------------------------------------------
PARITY_SEV="$(gates_policy_section_get attestation parity)"
[[ -z "$PARITY_SEV" ]] && PARITY_SEV="error"
if [[ "$DRY_RUN" == "1" && "$ATT_ENABLED" != "false" && "$PARITY_SEV" != "off" ]]; then
    record "parity" "planned" "tool versions against lockfile pins"
elif [[ "$DRY_RUN" != "1" &&"$ATT_ENABLED" != "false" && "$PARITY_SEV" != "off" ]]; then
    gates_progress_start parity
    parity_start="$(date +%s)"
    parity_input="[]"
    if [[ ${#ATT_GATES[@]} -gt 0 ]]; then
        parity_input="[$(IFS=,; printf '%s' "${ATT_GATES[*]}")]"
    fi
    PARITY_DRIFT="$(gates_pin_mismatches "$parity_input" "$PROJECT_ROOT")"
    # A pin the sources disagree on, or a mise file that cannot be read,
    # fails like drift: the comparison above would rest on a guess.
    # Each problem is named once, however many tools or configs repeat it.
    pin_problems=""
    if [[ ${#PIN_TOOLS[@]} -gt 0 ]]; then
        pin_problems="$(for pin_tool in "${PIN_TOOLS[@]}"; do
            gates_pin_problems "$pin_tool" "$PROJECT_ROOT"
        done | awk 'NF && !seen[$0]++ { printf "%s%s", sep, $0; sep = "; " }')"
    fi
    [[ -n "$pin_problems" ]] && PARITY_DRIFT="$pin_problems${PARITY_DRIFT:+; $PARITY_DRIFT}"
    # The gate set (feature 006, research R7): every project gate the policy
    # declares ran, or, at agent and git, was skipped by a declaration naming
    # this boundary. At ci every declared gate must have run (constitution,
    # Principle III). Reported next to any version drift, not instead of it.
    if pg_declared="$(gates_project_gates_declared)"; then
        pg_unrun="$(gates_declared_not_run "$GATES_BOUNDARY" "$pg_declared" "$parity_input")"
    else
        pg_unrun="declared gates not run at $GATES_BOUNDARY: the policy's gate set could not be read"
    fi
    [[ -n "$pg_unrun" ]] && PARITY_DRIFT="${PARITY_DRIFT:+$PARITY_DRIFT; }$pg_unrun"
    # Covering (feature 007, research R6): at ci no accept block may have
    # been covered (constitution, Principle III). The spec gate never asks
    # the covering script there, so this fires only if that breaks. The
    # check reads the block entries of this run's spec results (empty when
    # the spec gate did not run); gates_spec_covered_at_ci is the seam the
    # parity tests feed.
    if pc_cov="$(gates_spec_covered_at_ci "$GATES_BOUNDARY" "${SPEC_RESULTS_JSON:-}")"; then
        PARITY_DRIFT="${PARITY_DRIFT:+$PARITY_DRIFT; }$pc_cov"
    fi
    if [[ -z "$PARITY_DRIFT" ]]; then
        record "parity" "pass" ""
        att_gate "parity" "" "" "" "" "" "pass" "" 0
        parity_result="pass"
    elif [[ "$PARITY_SEV" == "error" ]]; then
        record "parity" "fail" "$PARITY_DRIFT"
        FAILED=$((FAILED + 1))
        att_gate "parity" "" "" "" "" "" "fail" "$PARITY_DRIFT" 0
        parity_result="fail"
    else
        record "parity" "warn" "$PARITY_DRIFT"
        WARNINGS=$((WARNINGS + 1))
        att_gate "parity" "" "" "" "" "" "warn" "$PARITY_DRIFT" 0
        parity_result="warn"
    fi
    gate_timeline_end parity "$parity_start" "$(date +%s)" "$parity_result"
fi

# ---------------------------------------------------------------------------
# Attestation record (feature 001): one per non-dry run unless the policy
# disables it. A failure to hash or to write the log is a stderr warning
# only — evidence loss must never mask or manufacture a gate outcome.
# ---------------------------------------------------------------------------
EXIT_CODE=0
[[ "$FAILED" -gt 0 ]] && EXIT_CODE=2

ATTESTATION=""
if [[ "$DRY_RUN" != "1" && "$ATT_ENABLED" != "false" ]]; then
    # The hash is of the policy the run actually enforced: the materialized
    # effective policy in a contract repo, policy.json everywhere else.
    if POLICY_SHA="$(gates_sha256 "$(gates_policy_file)")"; then
        RUNTIME_VERSION=""
        if [[ -f "$GATES_DIR/.runtime-version" ]]; then
            RUNTIME_VERSION="$(head -n 1 "$GATES_DIR/.runtime-version" 2>/dev/null || true)"
        fi
        ATT_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
        att_joined=""
        if [[ ${#ATT_GATES[@]} -gt 0 ]]; then
            att_joined="$(IFS=,; printf '%s' "${ATT_GATES[*]}")"
        fi
        # The optional spec (002), contract (003) and policy_override (#196)
        # objects are additive; v stays 1 and consumers ignore unknown fields (001 rule).
        ATTESTATION="$(jq -cn --arg ts "$ATT_TS" --arg boundary "$BOUNDARY" \
            --arg sha "$POLICY_SHA" --arg rv "$RUNTIME_VERSION" \
            --argjson exit "$EXIT_CODE" --argjson gates "[$att_joined]" \
            --argjson spec "${SPEC_ATT_JSON:-null}" \
            --argjson contract "${CONTRACT_ATT_JSON:-null}" \
            --argjson override "${POLICY_OVERRIDE_JSON:-null}" '
            { v: 1,
              ts: $ts,
              boundary: (if ["agent","git","ci"] | index($boundary) then $boundary else "unspecified" end),
              policy_sha256: $sha,
              runtime_version: (if $rv == "" then null else $rv end),
              exit: $exit,
              gates: $gates }
            + (if $spec != null then { spec: $spec } else {} end)
            + (if $contract != null then { contract: $contract } else {} end)
            + (if $override != null then { policy_override: $override } else {} end)')"
        MAX_RECORDS="$(gates_policy_section_get attestation max_records)"
        [[ -z "$MAX_RECORDS" ]] && MAX_RECORDS=200
        if ! gates_attest_append "$ATTESTATION" "$GATES_DIR/attestations.jsonl" "$MAX_RECORDS"; then
            echo "gates: warning: could not write $GATES_DIR/attestations.jsonl, so this run left no evidence record (gate outcome unaffected; run /speckit.gates.doctor)" >&2
        fi
    else
        echo "gates: warning: attestation skipped — cannot hash policy (gate outcome unaffected)" >&2
    fi
fi

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
# Note: "${RESULTS[@]}" on an empty array is an unbound-variable error under
# `set -u` on bash 3.2 (the default /bin/bash on macOS, where these hooks run),
# so every access is guarded by an element-count check.
if [[ "$JSON" == "1" ]]; then
    joined=""
    if [[ ${#RESULTS[@]} -gt 0 ]]; then
        joined="$(IFS=,; printf '%s' "${RESULTS[*]}")"
    fi
    override_field=""
    [[ -n "$POLICY_OVERRIDE_JSON" ]] && override_field=",\"policy_override\":$POLICY_OVERRIDE_JSON"
    if [[ -n "$ATTESTATION" ]]; then
        printf '{"boundary":"%s","failed":%d,"warnings":%d%s,"gates":[%s],"attestation":%s}\n' \
            "$BOUNDARY" "$FAILED" "$WARNINGS" "$override_field" "$joined" "$ATTESTATION"
    else
        printf '{"boundary":"%s","failed":%d,"warnings":%d%s,"gates":[%s]}\n' \
            "$BOUNDARY" "$FAILED" "$WARNINGS" "$override_field" "$joined"
    fi
else
    echo "gates: boundary=$BOUNDARY failed=$FAILED warnings=$WARNINGS"
    if [[ "$POLICY_OVERRIDE_APPLIED" == "true" ]]; then
        echo "  [override] policy -- GATES_POLICY_FILE=$POLICY_OVERRIDE was enforced instead of the repository's policy"
    elif [[ -n "$POLICY_OVERRIDE" ]]; then
        echo "  [override] policy -- GATES_POLICY_FILE=$POLICY_OVERRIDE was ignored $POLICY_OVERRIDE_WHERE"
    fi
    if [[ ${#RESULTS[@]} -gt 0 ]]; then
        i=0
        for r in "${RESULTS[@]}"; do
            name="$(printf '%s' "$r" | jq -r '.name')"
            status="$(printf '%s' "$r" | jq -r '.status')"
            detail="${SUMMARIES[i]}"
            kept="${OUTPUTS[i]}"
            i=$((i + 1))
            if [[ -z "$detail" ]]; then
                detail="$(jq -r '.detail' <<<"$r")"
                detail="${detail%%$'\n'*}"
            fi
            # Output the secret scan withheld (gates_output_keep) is not
            # quoted in the summary either.
            [[ "$kept" == withheld:* ]] \
                && detail="output withheld: ${kept#withheld:}; run the step locally to see it"
            if [[ -n "$detail" ]]; then
                echo "  [$status] $name -- $detail"
            else
                echo "  [$status] $name"
            fi
            # The kept output (#309); at ci, where no one reruns the step by
            # hand, a failed gate's output goes to the log too.
            if [[ -n "$kept" && "$kept" != withheld:* ]]; then
                echo "    output: $kept"
                if [[ "$BOUNDARY" == "ci" && "$status" == "fail" ]]; then
                    gates_output_log "$name" "$PROJECT_ROOT/$kept"
                fi
            fi
            [[ "$status" == "fail" && -z "${WALK_FIRST_FAIL:-}" ]] \
                && WALK_FIRST_FAIL="$name -- $detail"
        done
    fi
    # A walk inside a gate run (the holder's GATES_LOCK_PID is not this
    # run's) marks its first failure for the calling gate's summary
    # (##gates-fail##, as formatter-dispatch does), so a custom command's
    # report line names the file even when a later tool passed. The caller
    # strips the marker; a walk run by hand prints none.
    if [[ "$WALK" == "1" && -n "${WALK_FIRST_FAIL:-}" \
        && -n "${GATES_LOCK_PID:-}" && "$GATES_LOCK_PID" != "$$" ]]; then
        echo "##gates-fail## $WALK_FIRST_FAIL"
    fi
fi

exit "$EXIT_CODE"
