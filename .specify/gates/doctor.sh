#!/bin/bash
# shellcheck shell=bash
set -uo pipefail

# spec-gates doctor: check the local environment has what the gate needs.
#
#   Required        -- the hooks and verify.sh cannot run without these.
#   Policy-enabled  -- linters your policy actually turns on; a missing one is
#                      an enforcement GAP (the gate silently skips that tool).
#   Recommended     -- optional; they enhance but are not required.
#
# Exit 0 = everything required (incl. policy-enabled linters) is present.
# Exit 1 = something required is missing.
# Exit 2 = usage error (an unknown option): nothing was checked.
#
# --ci leaves out what only a developer clone has: the git hook stubs in
# .git/hooks (CI never installs them) and the git boundary checks. Every
# other check runs, so a CI step can run doctor (#148).
#
# Options go in any order: --installed-only, --ci, --probe-git; or
# --canary, which runs the canary suite and passes it every other
# argument (canary.sh's --json, --only <ids>). An unknown option is a
# usage error, never a plain run that reads as a pass (#203).

usage() {
    echo "doctor: $*" >&2
    echo "usage: doctor.sh [--installed-only] [--ci] [--probe-git] | doctor.sh --canary [canary.sh options]" >&2
    exit 2
}
INSTALLED_ONLY=0
PROBE_GIT=0
CI_MODE=0
CANARY=0
DOCTOR_FLAGS=""
OTHER_ARGS=()
for _a in "$@"; do
    case "$_a" in
        --installed-only) INSTALLED_ONLY=1 DOCTOR_FLAGS="$DOCTOR_FLAGS $_a" ;;
        --ci) CI_MODE=1 DOCTOR_FLAGS="$DOCTOR_FLAGS $_a" ;;
        # Run hooks another tool owns too (their own steps run with them).
        --probe-git) PROBE_GIT=1 DOCTOR_FLAGS="$DOCTOR_FLAGS $_a" ;;
        --canary) CANARY=1 ;;
        *) OTHER_ARGS+=("$_a") ;;
    esac
done

# --canary delegates to the canary suite (projected as a sibling of this
# script), propagating its exit code and output; canary.sh judges its own
# options.
if [[ "$CANARY" -eq 1 ]]; then
    [[ -z "$DOCTOR_FLAGS" ]] || usage "--canary runs only the canary suite; it takes none of:$DOCTOR_FLAGS"
    CANARY_SH="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/canary.sh"
    if [[ ! -f "$CANARY_SH" ]]; then
        echo "doctor: canary.sh not found next to doctor.sh — re-project the runtime (/speckit.gates.init)" >&2
        exit 1
    fi
    exec bash "$CANARY_SH" ${OTHER_ARGS[@]+"${OTHER_ARGS[@]}"}
fi
[[ "${#OTHER_ARGS[@]}" -eq 0 ]] || usage "unknown option: ${OTHER_ARGS[0]}"

PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
GATES_LIB_DIR="$PROJECT_ROOT/.specify/gates/lib"
# The libraries next to this script: the only ones there are when doctor
# runs from the installed extension before anything is projected.
DOCTOR_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib"
for _l in install-state.sh managers.sh; do
    if [[ -f "$GATES_LIB_DIR/$_l" ]]; then
        # shellcheck source=/dev/null disable=SC1090
        source "$GATES_LIB_DIR/$_l"
    elif [[ -f "$DOCTOR_LIB_DIR/$_l" ]]; then
        # shellcheck source=/dev/null disable=SC1090
        source "$DOCTOR_LIB_DIR/$_l"
    fi
done

# shellcheck source=/dev/null disable=SC1091
[[ -f "$GATES_LIB_DIR/policy.sh" ]] && source "$GATES_LIB_DIR/policy.sh"
# shellcheck source=/dev/null disable=SC1091
[[ -f "$GATES_LIB_DIR/formatter-dispatch.sh" ]] && source "$GATES_LIB_DIR/formatter-dispatch.sh"
# shellcheck source=/dev/null disable=SC1091
[[ -f "$GATES_LIB_DIR/spec-gate.sh" ]] && source "$GATES_LIB_DIR/spec-gate.sh"
# shellcheck source=/dev/null disable=SC1091
[[ -f "$GATES_LIB_DIR/attest.sh" ]] && source "$GATES_LIB_DIR/attest.sh"
# shellcheck source=/dev/null disable=SC1091
[[ -f "$GATES_LIB_DIR/contract.sh" ]] && source "$GATES_LIB_DIR/contract.sh"
# shellcheck source=/dev/null disable=SC1091
[[ -f "$GATES_LIB_DIR/constitution.sh" ]] && source "$GATES_LIB_DIR/constitution.sh"
# shellcheck source=/dev/null disable=SC1091
[[ -f "$GATES_LIB_DIR/manifest.sh" ]] && source "$GATES_LIB_DIR/manifest.sh"

MISSING=0
OK="  [ok]  "
BAD="  [MISSING] "
REC="  [rec] "
SKIP="  [--]  "

have() { command -v "$1" >/dev/null 2>&1; }

INSTALL_HINT="apt-get install, apk add, brew install"

# How to finish a half-done upgrade. A runtime projected by 0.3.x has no
# .specify/gates/project.sh to ask (#203).
if [[ -f "$PROJECT_ROOT/.specify/gates/project.sh" ]]; then
    FINISH_HINT="bash .specify/gates/project.sh --check prints the finishing command"
else
    FINISH_HINT="finish it: specify extension add gates --from <the versioned release URL> (README \"Upgrade\"), then bash .specify/extensions/gates/runtime/project.sh"
fi

# Tools project.sh and the gates cannot run without, shared by the full
# run and --installed-only (#122): each missing one is named with what it
# breaks and how to install it.
base_tool_checks() {
    if have git; then
        echo "${OK}git"
    else
        echo "${BAD}git — not installed; the git and CI boundaries cannot run and project.sh cannot wire the git hooks. Install git ($INSTALL_HINT)."
        MISSING=$((MISSING + 1))
    fi
    if have cmp; then
        echo "${OK}cmp"
    else
        echo "${BAD}cmp — not installed; project.sh refuses to run without it. Install diffutils ($INSTALL_HINT)."
        MISSING=$((MISSING + 1))
    fi
    if have sha256sum || have shasum; then
        echo "${OK}sha256sum or shasum"
    else
        echo "${BAD}sha256sum or shasum — neither is installed; project.sh refuses to run, the contract gate cannot verify its pin, and verify.sh records no attestation. Install coreutils or perl ($INSTALL_HINT)."
        MISSING=$((MISSING + 1))
    fi
}

# Resolve a tool binary the way the gate does (node_modules/.bin -> PATH).
tool_bin() { # <binname>
    if declare -f _gates_tool_bin >/dev/null 2>&1; then
        _gates_tool_bin "$1" "$PROJECT_ROOT"
    elif have "$1"; then
        printf '%s\n' "$1"
    fi
}

# Is a policy linter enabled? (declares include globs)
policy_enables() { # <hook>
    declare -f gates_policy_list >/dev/null 2>&1 || return 1
    [[ -n "$(gates_policy_list "$1" include 2>/dev/null)" ]]
}

# Install hygiene (#73), shared by the full run and --installed-only.
install_checks() {
    # A `specify extension add --dev` install renders
    # the gates skills as symlinks into .specify/extensions/gates/.specify-dev/,
    # which does not exist in another clone or CI checkout: committed, they
    # dangle there and no /speckit.gates.* command loads. Every registered
    # gates command must be a regular file (skill or command), so a symlink,
    # a dangling link, or a missing file FAILS here.
    REG="$PROJECT_ROOT/.specify/extensions/.registry"
    if [[ -f "$REG" ]] && have jq; then
        reg_ok=0
        while IFS= read -r rcmd; do
            [[ -n "$rcmd" ]] || continue
            rsk="$PROJECT_ROOT/.claude/skills/${rcmd//./-}/SKILL.md"
            rcm="$PROJECT_ROOT/.claude/commands/$rcmd.md"
            rstate=""
            for rf in "$rsk" "$rcm"; do
                if [[ -L "$rf" || -L "$(dirname "$rf")" ]]; then
                    if [[ -e "$rf" ]]; then rstate="symlink"; else rstate="dangling"; fi
                    break
                elif [[ -f "$rf" ]]; then
                    rstate="ok"
                    break
                fi
            done
            case "$rstate" in
                ok) reg_ok=$((reg_ok + 1)) ;;
                symlink)
                    echo "${BAD}$rcmd is a symlink (a --dev install); it resolves only on this machine — reinstall from a release zip (README \"Upgrade\")"
                    MISSING=$((MISSING + 1))
                    ;;
                dangling)
                    echo "${BAD}$rcmd is a dangling symlink — the command does not load; reinstall from a release zip (README \"Upgrade\")"
                    MISSING=$((MISSING + 1))
                    ;;
                *)
                    echo "${BAD}$rcmd is registered but has no skill or command file — reinstall the extension"
                    MISSING=$((MISSING + 1))
                    ;;
            esac
        done < <(jq -r '(.extensions.gates.registered_commands.claude // [])[]' "$REG" 2>/dev/null)
        [[ "$reg_ok" -gt 0 ]] && echo "${OK}$reg_ok registered gates command(s) installed as regular files"
    fi
    if [[ -d "$PROJECT_ROOT/.specify/extensions/gates/.specify-dev" ]]; then
        echo "${REC}this is a --dev install (for developing spec-gates itself); other clones will not have its files — install from a release zip"
    fi
    # Zip extraction keeps the execute bit only on *.sh, so the vendored git
    # hooks arrive 644 and show as mode changes where .specify/extensions/ is
    # committed. Nothing runs them (the projected copies run), so a nudge.
    VEXEC_N=0
    for vf in "$PROJECT_ROOT/.specify/extensions/gates/runtime"/*.sh \
        "$PROJECT_ROOT/.specify/extensions/gates/runtime/lib"/*.sh \
        "$PROJECT_ROOT/.specify/extensions/gates/runtime/hooks/git"/* \
        "$PROJECT_ROOT/.specify/extensions/gates/runtime/hooks/claude"/*.sh; do
        [[ -f "$vf" && ! -x "$vf" ]] && VEXEC_N=$((VEXEC_N + 1))
    done
    [[ "$VEXEC_N" -gt 0 ]] \
        && echo "${REC}$VEXEC_N installed extension script(s) lack the execute bit (zip extraction) — run bash .specify/extensions/gates/runtime/project.sh"
    return 0
}

# --installed-only (#74): the installed extension alone -- registry, vendored
# copy, skills, install mode -- for a dormant install (nothing projected
# yet) or one without an agent integration. Run it from the installed copy:
#   bash .specify/extensions/gates/runtime/doctor.sh --installed-only
if [[ "$INSTALLED_ONLY" -eq 1 ]]; then
    echo "=== spec-gates doctor (installed extension only) ==="
    echo "project: $PROJECT_ROOT"
    echo ""
    echo "Required:"
    if have jq; then echo "${OK}jq"; else echo "${BAD}jq — not installed. Install jq ($INSTALL_HINT)."; MISSING=$((MISSING + 1)); fi
    base_tool_checks
    echo ""
    echo "Installed extension:"
    IST="$(gates_install_state "$PROJECT_ROOT")"
    IVER="$(gates_extension_version "$PROJECT_ROOT/.specify/extensions/gates/extension.yml")"
    case "$IST" in
        installed) echo "${OK}gates $IVER installed and projected" ;;
        dormant) echo "${OK}gates $IVER installed; the runtime is not projected yet (bash .specify/extensions/gates/runtime/project.sh)" ;;
        dev) echo "${OK}gates $IVER installed" ;;
        removed)
            echo "${BAD}the extension is not installed, but .specify/gates/ is projected — a half-done upgrade ($FINISH_HINT)"
            MISSING=$((MISSING + 1))
            ;;
        mismatch)
            echo "${BAD}the extension registry and .specify/extensions/gates/ disagree — reinstall the extension"
            MISSING=$((MISSING + 1))
            ;;
        unknown) echo "${SKIP}install state not checked: reading .specify/extensions/.registry needs jq" ;;
        *)
            echo "${BAD}the gates extension is not installed"
            MISSING=$((MISSING + 1))
            ;;
    esac
    install_checks
    echo ""
    if [[ "$MISSING" -gt 0 ]]; then
        echo "doctor: $MISSING required item(s) missing."
        exit 1
    fi
    echo "doctor: the installed extension is healthy."
    exit 0
fi

echo "=== spec-gates doctor ==="
echo "project: $PROJECT_ROOT"
echo ""

# A dormant install (#128): the extension is installed but nothing is
# projected, so there is no policy loader, no hooks and no gate to check.
# Reporting each linter as "not enabled in policy" would read as a policy
# choice; say what is missing instead.
if declare -f gates_install_state >/dev/null 2>&1 \
    && [[ "$(gates_install_state "$PROJECT_ROOT")" == "dormant" ]]; then
    echo "${BAD}the gates runtime is not projected: .specify/gates/ has no runtime, so no gate runs at any boundary"
    echo "  project it:  bash .specify/extensions/gates/runtime/project.sh (or /speckit.gates.init)"
    echo "  check the installed extension alone:  bash .specify/extensions/gates/runtime/doctor.sh --installed-only"
    echo ""
    echo "doctor: 1 required item(s) missing."
    exit 1
fi

echo "Required:"
if have jq; then
    echo "${OK}jq"
else
    # Without jq verify.sh refuses (exit 1), so no gate runs: the git hook
    # refuses every commit, CI fails, the Stop hook lets the session end
    # unchecked. The agent hooks run in raw mode (#83): built-in rules only,
    # and an "ask" for every edit only while protected_files.extra has
    # entries, which they cannot read without jq (#172).
    JQ_EXTRA=""
    if [[ -f "$PROJECT_ROOT/.specify/gates/policy.json" ]] \
        && grep -qE '"extra"[[:space:]]*:[[:space:]]*\[[[:space:]]*"' <<<"$(tr '\n' ' ' <"$PROJECT_ROOT/.specify/gates/policy.json")"; then
        JQ_EXTRA="; protected_files.extra is set, so every edit asks for confirmation"
    fi
    echo "${BAD}jq — not installed: verify.sh refuses to run, so no gate runs (every commit is refused, CI fails); the agent hooks run in raw mode (built-in rules only$JQ_EXTRA). Install jq."
    MISSING=$((MISSING + 1))
fi
base_tool_checks
# The PR hook parses commands with python3 (json + re) and fails closed
# without it (#66). The message rules' emoji check runs on python3 or perl.
if python3 -c 'import json, re' >/dev/null 2>&1; then
    echo "${OK}python3 (json, re)"
else
    echo "${BAD}python3 with the json module — the PR hook refuses every PR command without it (install python3; on minimal images confirm that python3 can import json)"
    MISSING=$((MISSING + 1))
fi
if ! python3 -c 'import re' >/dev/null 2>&1 && ! perl -e 1 >/dev/null 2>&1; then
    echo "${BAD}python3 or perl — the emoji rule cannot run, so every commit and PR message is refused"
    MISSING=$((MISSING + 1))
fi

echo ""
# The policy verify.sh enforces (the effective policy in a contract repo).
# Invalid, every boundary refuses to run the gates (#124), and reading
# "enabled" linters from it would only guess.
POLICY_ERR=""
if [[ -f "$PROJECT_ROOT/.specify/gates/policy.json" ]] && have jq \
    && declare -f gates_validate_policy >/dev/null 2>&1; then
    POLICY_ERR="$(gates_validate_policy "$(CLAUDE_PROJECT_DIR="$PROJECT_ROOT" gates_policy_file)" 2>&1)" \
        && POLICY_ERR=""
fi
if [[ ! -f "$PROJECT_ROOT/.specify/gates/policy.json" ]]; then
    echo "Policy: none found at .specify/gates/policy.json (run /speckit.gates.init)"
elif ! declare -f gates_policy_list >/dev/null 2>&1; then
    echo "Policy-enabled linters:"
    echo "${BAD}.specify/gates/lib/policy.sh is missing, so the policy cannot be read and no gate runs — re-project the runtime (bash .specify/extensions/gates/runtime/project.sh)"
    MISSING=$((MISSING + 1))
elif ! have jq; then
    # The policy reader needs jq; without it every linter would read as
    # "not enabled in policy", a claim about the policy nobody checked (#172).
    echo "Policy-enabled linters:"
    echo "${SKIP}not checked: reading the policy needs jq (see above)"
elif [[ -n "$POLICY_ERR" ]]; then
    echo "Policy:"
    echo "${BAD}policy is invalid — verify.sh refuses to run any gate until it is fixed:"
    printf '%s\n' "$POLICY_ERR" | sed 's/^/        /'
    MISSING=$((MISSING + 1))
else
    echo "Policy-enabled linters:"
    # hook name -> binary name
    for pair in "prettier:prettier" "markdownlint:markdownlint-cli2" "shellcheck:shellcheck"; do
        hook="${pair%%:*}"
        bin="${pair##*:}"
        if ! policy_enables "$hook"; then
            echo "${SKIP}$hook (not enabled in policy)"
        elif [[ -n "$(tool_bin "$bin")" ]]; then
            echo "${OK}$hook ($bin)"
        else
            echo "${BAD}$hook ($bin) — enabled in policy but not installed; the gate will skip it"
            MISSING=$((MISSING + 1))
        fi
    done
    # Fields that still validate but that no gate reads (#112).
    if have jq; then
        while IFS= read -r field; do
            [[ -n "$field" ]] || continue
            echo "${REC}hooks.$field has no effect (deprecated) — remove it from policy.json"
        done < <(jq -r '(.hooks // {}) | to_entries[] | select(.value | type == "object")
            | .key as $k | .value | keys[]
            | select(. == "on_missing_runner" or . == "on_missing_tests")
            | "\($k).\(.)"' "$PROJECT_ROOT/.specify/gates/policy.json" 2>/dev/null || true)
    fi
    # Project gates (feature 006, research R9): each with its severity, time
    # limit and the boundaries it skips by declaration, so every skip is
    # visible outside a run. CI never skips one (constitution, Principle III).
    if declare -f gates_project_gate_names >/dev/null 2>&1; then
        PG_LIST="$(gates_project_gate_names)"
        if [[ -n "$PG_LIST" ]]; then
            echo "Project gates:"
            PG_DEFAULT_S="$(gates_policy_get "verify-quality" "gate_timeout_s")"
            [[ -n "$PG_DEFAULT_S" ]] || PG_DEFAULT_S=600
            while IFS= read -r pg; do
                [[ -n "$pg" ]] || continue
                pg_sev="$(gates_project_gate_get "$pg" severity)"
                [[ -n "$pg_sev" ]] || pg_sev="error"
                pg_s="$(gates_project_gate_get "$pg" timeout_s)"
                [[ -n "$pg_s" ]] || pg_s="$PG_DEFAULT_S"
                pg_skip="$(gates_project_gate_skip_at "$pg")"
                pg_where="runs at every boundary"
                [[ -n "$pg_skip" ]] \
                    && pg_where="skipped by declaration at $(jq -r 'join(", ")' <<<"$pg_skip" 2>/dev/null)"
                echo "${OK}gate $pg ($pg_sev, ${pg_s}s): $pg_where"
            done <<<"$PG_LIST"
        fi
    fi
    # lib/formatter-dispatch.sh is internal and may change between
    # releases; verify.sh --walk is the stable way for a command to run the
    # built-in walk (feature 006, research R9).
    # shellcheck disable=SC2016  # literal backticks in the message
    WALK_REC='use `bash .specify/gates/verify.sh --walk` (stable)'
    # Looks one level down: the body of an `npm|pnpm|yarn run <script>`,
    # and any project file the command (or that body) names. Files under
    # .specify/gates/ are the runtime itself and are not read. Sets WALK_VIA
    # to the file that calls the dispatch, empty when the command names it.
    walk_calls_dispatch() { # <command> -> 0 when it reaches the dispatch
        local text="$1" line word
        local -a words
        WALK_VIA=""
        if [[ "$text" =~ ^[[:space:]]*(npm|pnpm|yarn)[[:space:]]+((-s|--silent)[[:space:]]+)?run(-script)?[[:space:]]+((-s|--silent)[[:space:]]+)?([^[:space:]]+) ]]; then
            text+=$'\n'"$(jq -r --arg n "${BASH_REMATCH[7]}" '.scripts[$n] // empty' \
                "$PROJECT_ROOT/package.json" 2>/dev/null)"
        fi
        [[ "$text" == *formatter-dispatch.sh* ]] && return 0
        while IFS= read -r line; do
            read -ra words <<<"$line"
            for word in ${words[@]+"${words[@]}"}; do
                word="${word//[\"\']/}"
                word="${word#./}"
                [[ -n "$word" && "$word" != /* && "$word" != *..* ]] || continue
                [[ "$word" == .specify/gates/* ]] && continue
                [[ -f "$PROJECT_ROOT/$word" ]] || continue
                grep -qF formatter-dispatch.sh "$PROJECT_ROOT/$word" 2>/dev/null \
                    && { WALK_VIA="$word"; return 0; }
            done
        done <<<"$text"
        return 1
    }
    walk_rec() { # <subject> <command>
        walk_calls_dispatch "$2" || return 0
        if [[ -n "$WALK_VIA" ]]; then
            echo "${REC}$1 calls lib/formatter-dispatch.sh (via $WALK_VIA): $WALK_REC"
        else
            echo "${REC}$1 calls lib/formatter-dispatch.sh: $WALK_REC"
        fi
    }
    walk_rec custom_command "$(gates_policy_get "verify-quality" "custom_command")"
    if declare -f gates_project_gate_names >/dev/null 2>&1; then
        while IFS= read -r pg; do
            [[ -n "$pg" ]] || continue
            walk_rec "gate $pg" "$(gates_project_gate_get "$pg" command)"
        done < <(gates_project_gate_names)
    fi
fi

# Runtime projection vs installed extension (issue #33): after an extension
# update (or remove+add — `specify extension update` may not move a
# source:local install), nothing re-projects .specify/gates/ by itself; the
# installed extension and the projected runtime silently diverge until
# /speckit.gates.upgrade runs. A version mismatch is an enforcement-relevant
# failure: the projected gates are not the gates the user thinks they
# installed. Repos running the runtime from source (no installed extension)
# skip this section entirely.
EXT_MANIFEST="$PROJECT_ROOT/.specify/extensions/gates/extension.yml"
if [[ -f "$EXT_MANIFEST" ]]; then
    echo ""
    echo "Runtime projection (installed extension vs .specify/gates/):"
    EXT_VERSION="$(sed -n 's/^  version: "\(.*\)"$/\1/p' "$EXT_MANIFEST" | head -n 1)"
    RTV_FILE="$PROJECT_ROOT/.specify/gates/.runtime-version"
    if [[ ! -f "$RTV_FILE" ]]; then
        echo "${REC}no .runtime-version marker — run /speckit.gates.upgrade to re-project and record the runtime version"
    else
        RTV="$(head -n 1 "$RTV_FILE" 2>/dev/null || true)"
        if [[ -n "$EXT_VERSION" && "$RTV" != "$EXT_VERSION" ]]; then
            echo "${BAD}projected runtime is $RTV but the installed extension is $EXT_VERSION — run /speckit.gates.upgrade (the projected gates are NOT the version you installed)"
            MISSING=$((MISSING + 1))
        else
            echo "${OK}projected runtime $RTV matches the installed extension"
        fi
    fi
    install_checks
    # Constitution corpus presence (issue #31): the guided session needs
    # manifest.yml + fragments/ under the installed extension; 0.3.0 shipped
    # without them, which was invisible until the session died mid-flow.
    if [[ -f "$PROJECT_ROOT/.specify/extensions/gates/constitution/manifest.yml" ]]; then
        echo "${OK}constitution corpus present"
    else
        echo "${REC}constitution corpus not found under the installed extension — /speckit.gates.constitution needs it (re-install from a release that ships gates/constitution/)"
    fi
fi

# Install state (#74): a projected runtime whose extension was removed and
# not added back is a half-done upgrade, and a registry that disagrees with
# the vendored copy an interrupted install. Both are failures.
if declare -f gates_install_state >/dev/null 2>&1 && [[ -d "$PROJECT_ROOT/.specify/gates" ]]; then
    case "$(gates_install_state "$PROJECT_ROOT")" in
        removed)
            echo ""
            echo "${BAD}the gates extension was removed but not added back (half-done upgrade) — $FINISH_HINT"
            MISSING=$((MISSING + 1))
            ;;
        mismatch)
            echo ""
            echo "${BAD}the extension registry and .specify/extensions/gates/ disagree (interrupted install) — reinstall the extension"
            MISSING=$((MISSING + 1))
            ;;
        # jq is reported missing above; the registry says nothing without it.
        unknown)
            echo ""
            echo "${SKIP}install state not checked: reading .specify/extensions/.registry needs jq"
            ;;
    esac
fi

# Upgrade safety (#70): what an upgrade would do, the holds, and CI drift.
# The installed extension's project.sh --check is the single judge of the
# projection (same classification an upgrade uses); a project that opted
# out of the agent hooks is recognized from its manifest, which then lists
# no .claude/hooks/gates/ entries.
if declare -f gates_ci_missing >/dev/null 2>&1 && [[ -d "$PROJECT_ROOT/.specify/gates" ]]; then
    echo ""
    echo "Upgrade safety (projection, holds, CI):"
    VEND_RT="$PROJECT_ROOT/.specify/extensions/gates/runtime"
    PHOLDS=0
    if [[ -f "$VEND_RT/project.sh" ]]; then
        PFLAGS=""
        # The git hook stubs live in .git/hooks, never in a checkout, and
        # are not in the manifest: --ci leaves out only the git wiring.
        [[ "$CI_MODE" -eq 1 ]] && PFLAGS="--no-git-hooks"
        MAN_FILE="$PROJECT_ROOT/.specify/gates/.projected.sha256"
        if [[ -f "$MAN_FILE" ]] && ! grep -q '  \.claude/hooks/gates/' "$MAN_FILE"; then
            PFLAGS="$PFLAGS --no-agent-hooks"
        fi
        prc=0
        # shellcheck disable=SC2086  # PFLAGS holds up to two flags
        POUT="$(cd "$PROJECT_ROOT" && bash "$VEND_RT/project.sh" --check $PFLAGS 2>&1)" || prc=$?
        case "$prc" in
            0) echo "${OK}projection matches the installed extension" ;;
            1)
                # Exit 1 also covers pending work that re-projecting does
                # not finish (#214): name each item, so the advice does not
                # loop on a project.sh that reports "no changes".
                PITEMS=""
                grep -q '^project: planned changes:' <<<"$POUT" \
                    && PITEMS="the projection is not current — run: bash .specify/extensions/gates/runtime/project.sh"
                grep -q 'does not run gates for:\|has no gates entry for:' <<<"$POUT" \
                    && PITEMS="$PITEMS"$'\n'"hook-manager wiring is pending — run: bash .specify/extensions/gates/runtime/project.sh --wire-manager (entries it cannot append are printed below)"
                # shellcheck disable=SC2016  # the backticks are literal
                while IFS= read -r pcmd; do
                    [[ -n "$pcmd" ]] && PITEMS="$PITEMS"$'\n'"the hook manager has not generated the git hooks it is wired for — run: $pcmd"
                done <<<"$(sed -n 's/^project: pending: .* until you run `\(.*\)`$/\1/p' <<<"$POUT" | sort -u)"
                # Stale and missing holds are named once, per hold, in the
                # holds check below (#226), which also counts them.
                grep -q '^project: FAILED: stale holds\|^project: FAILED: held files that do not exist' <<<"$POUT" \
                    && PHOLDS=1
                grep -q '^project: git is not installed' <<<"$POUT" \
                    && PITEMS="$PITEMS"$'\n'"git is not installed, so the git boundary is not wired — install git, then run: bash .specify/extensions/gates/runtime/project.sh"
                grep -q '^project: git refuses this repository: dubious ownership' <<<"$POUT" \
                    && PITEMS="$PITEMS"$'\n'"git refuses this repository (dubious ownership) — trust it with the safe.directory command below, then run: bash .specify/extensions/gates/runtime/project.sh"
                grep -q '^project: another tool owns these git hooks' <<<"$POUT" \
                    && PITEMS="$PITEMS"$'\n'"another tool owns git hooks that do not call gates — add the line printed below to each"
                PITEMS="${PITEMS#$'\n'}"
                [[ -n "$PITEMS" || "$PHOLDS" -eq 1 ]] || PITEMS="project.sh --check reports pending work (below)"
                while IFS= read -r pitem; do
                    [[ -n "$pitem" ]] || continue
                    echo "${BAD}$pitem"
                    MISSING=$((MISSING + 1))
                done <<<"$PITEMS"
                ;;
            3)
                echo "${BAD}projected files were edited locally and are not held — the next upgrade stops on them (keep with --keep-local, or take upstream)"
                MISSING=$((MISSING + 1))
                ;;
            *)
                echo "${BAD}project.sh --check refused (exit $prc):"
                MISSING=$((MISSING + 1))
                ;;
        esac
        [[ "$prc" -ne 0 ]] && printf '%s\n' "$POUT" | grep -v '^project: *$' | sed 's/^/        /'
    else
        echo "${SKIP}no installed extension with project.sh — projection check skipped"
    fi

    gates_holds_load "$PROJECT_ROOT"
    gates_manifest_load "$PROJECT_ROOT"
    HELD_EDITS=0 HOLD_BAD=0
    if [[ -n "$GATES_HOLDS" ]]; then
        HTABLE=""
        [[ -d "$VEND_RT" ]] && HTABLE="$(gates_projection_table "$VEND_RT" 1)"
        while IFS= read -r hp; do
            case "$hp" in
                "$GATES_LOCAL_REL"/*)
                    echo "${REC}hold $hp is redundant: hooks.local.d is never touched by upgrades"
                    continue
                    ;;
            esac
            hsrc="$(printf '%s\n' "$HTABLE" | awk -F '\t' -v p="$hp" '$2 == p { print $1; exit }')"
            # Without the installed table, the projected trees stand in for it.
            hown="$hsrc"
            [[ -z "$HTABLE" ]] && case "$hp" in .specify/gates/* | .claude/hooks/gates/*) hown=1 ;; esac
            if [[ -n "$hown" && ! -s "$PROJECT_ROOT/$hp" ]]; then
                # A held deletion (#168): the hook, gate or canary that runs
                # this file is off, and a missing agent hook exits 127,
                # which Claude Code does not treat as a block. A file
                # emptied to 0 bytes is the same disablement (#203).
                hgone="missing"
                [[ -e "$PROJECT_ROOT/$hp" ]] && hgone="empty"
                echo "${BAD}held file is $hgone: $hp — a deletion cannot be held; the check that runs it is off. Restore it: bash .specify/extensions/gates/runtime/project.sh --take-upstream $hp"
                MISSING=$((MISSING + 1))
                HOLD_BAD=1
                continue
            fi
            if [[ -z "$HTABLE" ]]; then
                echo "${OK}held: $hp"
                HELD_EDITS=1
            elif [[ -z "$hsrc" ]]; then
                echo "${REC}hold $hp names a file projection does not own (remove the line)"
            elif cmp -s "$VEND_RT/$hsrc" "$PROJECT_ROOT/$hp"; then
                echo "${BAD}stale hold: $hp now equals the installed extension's copy — remove it from $GATES_HOLDS_REL so upgrades update it again"
                MISSING=$((MISSING + 1))
                HOLD_BAD=1
            else
                # The manifest keeps the hash projection last wrote for a
                # held file. When the installed copy differs from it, the
                # upstream file changed since the hold was taken (#132).
                hrec="$(gates_manifest_hash "$hp")"
                if [[ -n "$hrec" ]] && declare -f gates_sha256 >/dev/null 2>&1 \
                    && [[ "$(gates_sha256 "$VEND_RT/$hsrc")" != "$hrec" ]]; then
                    echo "${REC}held: $hp — the installed extension changed this file since it was held; compare the two and re-decide (--take-upstream replaces it and releases the hold)"
                else
                    echo "${OK}held: $hp (differs from the installed extension, kept on purpose)"
                fi
                HELD_EDITS=1
            fi
        done <<<"$GATES_HOLDS"
    fi
    # project.sh --check found a hold problem the per-hold check did not
    # name: still a failure.
    if [[ "$PHOLDS" -eq 1 && "$HOLD_BAD" -eq 0 ]]; then
        echo "${BAD}project.sh --check reports stale or missing holds (listed above)"
        MISSING=$((MISSING + 1))
    fi
    # A held edit is checked by nothing above: only the canary suite shows
    # whether the gates still block with it in place.
    [[ "$HELD_EDITS" -eq 1 ]] \
        && echo "${REC}held files run in place of the released ones — prove the gates still block: bash .specify/gates/doctor.sh --canary"

    if CI_FILES="$(gates_ci_files "$PROJECT_ROOT")"; then
        CI_MISSING="$(gates_ci_missing "$PROJECT_ROOT")"
        CI_ACKS="$(gates_holds_ci "$PROJECT_ROOT")"
        if [[ -z "$CI_MISSING" ]]; then
            echo "${OK}CI pipeline ($(printf '%s' "$CI_FILES" | tr '\n' ' ' | sed 's/ $//')) has every template step"
        else
            while IFS= read -r cid; do
                echo "${BAD}CI pipeline lacks the '$cid' step from the template — add it from .specify/extensions/gates/ci/, or record a deliberate omission as 'ci:$cid' in $GATES_HOLDS_REL"
                MISSING=$((MISSING + 1))
            done <<<"$CI_MISSING"
        fi
        # A ci: hold is judged like a file hold: an id the template lacks is
        # a stray line, a hold for a step the pipeline runs is stale.
        CI_PRESENT="$(gates_ci_present "$PROJECT_ROOT")"
        if [[ -n "$CI_ACKS" ]]; then
            while IFS= read -r cid; do
                if [[ -z "$(gates_ci_step_re "$cid")" ]]; then
                    echo "${REC}hold ci:$cid names no template step (gates, canary, pr) — remove the line"
                elif [[ -n "$CI_PRESENT" ]] && grep -qxF "$cid" <<<"$CI_PRESENT"; then
                    echo "${BAD}stale hold: ci:$cid but the pipeline runs the '$cid' step — remove it from $GATES_HOLDS_REL"
                    MISSING=$((MISSING + 1))
                else
                    # A held step is a check CI gives up: say which (#235).
                    echo "${REC}CI step '$cid' held (ci:$cid in $GATES_HOLDS_REL): $(gates_ci_step_omitted "$cid")"
                    [[ "$cid" == pr ]] \
                        && echo "${REC}adopt the pr step on its own: bash .specify/extensions/gates/runtime/project.sh --check prints it for this pipeline"
                fi
            done <<<"$CI_ACKS"
        fi
    else
        # A pipeline that calls verify.sh but runs no proven gates step looks
        # wired and may enforce nothing (#171, #198): a gap, not a nudge.
        CI_INERT="$(gates_ci_inert "$PROJECT_ROOT" 2>/dev/null)"
        if [[ -n "$CI_INERT" ]]; then
            while IFS=$'\t' read -r cf cwhy; do
                echo "${BAD}CI pipeline $cf calls verify.sh but no 'verify.sh --boundary ci' step is proven to run and fail it: $cwhy"
                MISSING=$((MISSING + 1))
            done <<<"$CI_INERT"
        else
            echo "${REC}no CI pipeline runs verify.sh --boundary ci — project one with /speckit.gates.ci"
        fi
    fi
    # Callers written for 0.3.6 run verify.sh without --boundary (#216): it
    # still runs them, with a deprecation warning, so a nudge, not a gap.
    if declare -f gates_verify_unbounded >/dev/null 2>&1; then
        while IFS= read -r caller; do
            [[ -n "$caller" ]] \
                && echo "${REC}$caller runs verify.sh without --boundary (deprecated; a later release may refuse it) — add --boundary agent|git|ci"
        done <<<"$(gates_verify_unbounded "$PROJECT_ROOT")"
    fi
fi

# Execute bits on projected scripts (issue #34): zip installs extract without
# file modes, so projected scripts arrive 644 until init/upgrade chmod them.
# Agent hooks are invoked DIRECTLY by path from .claude/settings.json — a
# non-executable one is silently skipped, the same enforcement-loss class as
# a non-executable git hook (issue #20), so it FAILS. The .specify/gates/
# entry scripts are invoked via `bash path` everywhere the runtime calls
# them, so a missing bit there is a [rec] nudge, not a gap.
if [[ -d "$PROJECT_ROOT/.claude/hooks/gates" ]]; then
    for hf in "$PROJECT_ROOT/.claude/hooks/gates"/*.sh; do
        [[ -f "$hf" ]] || continue
        if [[ ! -x "$hf" ]]; then
            echo ""
            echo "${BAD}agent hook not executable: ${hf#"$PROJECT_ROOT"/} — settings.json invokes it by path, so the agent boundary silently skips it (fix: chmod +x)"
            MISSING=$((MISSING + 1))
        fi
    done
fi
NONEXEC_GATES=""
for gs in "$PROJECT_ROOT/.specify/gates"/*.sh; do
    [[ -f "$gs" && ! -x "$gs" ]] && NONEXEC_GATES="$NONEXEC_GATES ${gs##*/}"
done
if [[ -n "$NONEXEC_GATES" ]]; then
    echo ""
    echo "${REC}projected script(s) not executable:${NONEXEC_GATES} — harmless (they run via bash), but chmod +x keeps direct invocation working"
fi

# Attestation writability (#122): verify.sh still passes or fails as it
# should when it cannot append its record, but the run leaves no evidence,
# and the no-op check below reads exactly that evidence. A failure.
ATT_DIR="$PROJECT_ROOT/.specify/gates"
if [[ -d "$ATT_DIR" ]] && declare -f gates_policy_section_get >/dev/null 2>&1 \
    && [[ "$(gates_policy_section_get attestation enabled 2>/dev/null)" != "false" ]]; then
    ATT_RO=""
    if [[ ! -w "$ATT_DIR" ]]; then
        ATT_RO=".specify/gates/"
    elif [[ -e "$ATT_DIR/attestations.jsonl" && ! -w "$ATT_DIR/attestations.jsonl" ]]; then
        ATT_RO=".specify/gates/attestations.jsonl"
    fi
    if [[ -n "$ATT_RO" ]]; then
        echo ""
        echo "${BAD}attestations cannot be written: $ATT_RO is not writable — verify.sh still runs, but records no evidence (make it writable, or set attestation.enabled=false)"
        MISSING=$((MISSING + 1))
    fi
fi

# No-op heuristic (FR-004): a gate that PASSED while checking none of its
# candidate files is the historical silent-no-op signature. No legitimate
# instance exists, so it is a doctor FAILURE, not a warning.
ATT_LOG="$PROJECT_ROOT/.specify/gates/attestations.jsonl"
if have jq && [[ -f "$ATT_LOG" ]]; then
    echo ""
    echo "Attestation evidence (latest record):"
    NOOP_GATES="$(tail -n 1 "$ATT_LOG" 2>/dev/null \
        | jq -r '(.gates // [])[]
            | select(.result == "pass" and ((.candidates // 0) > 0) and ((.checked // 0) == 0))
            | .name' 2>/dev/null || true)"
    if [[ -n "$NOOP_GATES" ]]; then
        for g in $NOOP_GATES; do
            echo "${BAD}suspected NO-OP gate: $g — latest run passed with candidates > 0 but checked = 0"
            MISSING=$((MISSING + 1))
        done
    else
        echo "${OK}no no-op signature"
    fi
fi

# Spec conformance (feature 002): what the spec gate sees. Discovery and
# parse counts are informational; a parse error is a doctor FAILURE (the
# gate fails closed on it, so surface it here with the same weight). A
# feature with every task checked but no Complete marker gets a nudge —
# enforcement is one Status flip away.
if declare -f gates_spec_features >/dev/null 2>&1 \
    && declare -f gates_policy_section_list >/dev/null 2>&1; then
    echo ""
    echo "Spec conformance (accept blocks in specs/*/tasks.md):"
    SPEC_TMP="$(mktemp -d 2>/dev/null || mktemp -d -t gates-doctor-spec)"
    SPEC_FEATURES=0
    SPEC_BLOCKS=0
    SPEC_COMPLETE=0
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        SPEC_FEATURES=$((SPEC_FEATURES + 1))
        fdir="$PROJECT_ROOT/specs/$f"
        mkdir -p "$SPEC_TMP/$f"
        parse_out=""
        [[ -f "$fdir/tasks.md" ]] && parse_out="$(gates_spec_parse "$fdir/tasks.md" "$SPEC_TMP/$f")"
        nblocks=0
        tasks_total=0
        tasks_unchecked=0
        ferrors=0
        while IFS=$'\t' read -r tag a1 a2 rest; do
            [[ -z "$tag" ]] && continue
            case "$tag" in
                ERROR)
                    echo "${BAD}specs/$f/tasks.md:$a1: $a2"
                    MISSING=$((MISSING + 1))
                    ferrors=$((ferrors + 1))
                    ;;
                BLOCK) nblocks=$((nblocks + 1)) ;;
                TASKS)
                    tasks_total="$a1"
                    tasks_unchecked="$a2"
                    ;;
            esac
        done <<<"$parse_out"
        SPEC_BLOCKS=$((SPEC_BLOCKS + nblocks))
        if gates_spec_complete "$fdir/spec.md"; then
            SPEC_COMPLETE=$((SPEC_COMPLETE + 1))
            [[ "$ferrors" -eq 0 ]] \
                && echo "${OK}$f — Complete, $nblocks accept block(s), enforced"
        elif [[ "$tasks_total" -gt 0 && "$tasks_unchecked" -eq 0 ]]; then
            echo "${REC}$f — every task checked but Status is not Complete; set **Status**: Complete in spec.md to turn enforcement on"
        elif [[ "$ferrors" -eq 0 ]]; then
            echo "${SKIP}$f — $nblocks accept block(s), not enforced ($tasks_unchecked of $tasks_total tasks open)"
        fi
    done <<<"$(gates_spec_features "$PROJECT_ROOT")"
    rm -rf "$SPEC_TMP"
    echo "  $SPEC_FEATURES feature(s), $SPEC_BLOCKS accept block(s) parsed, $SPEC_COMPLETE complete"
fi

# The covering script (spec.cover, feature 007, FR-019): where it may cover
# accept blocks (never at ci), its time limit, and that it is protected. A
# declared script that is missing makes verify.sh refuse every run; one that
# is not executable covers nothing (every block runs).
if declare -f gates_cover_script >/dev/null 2>&1 && have jq; then
    COVER_PATH="$(gates_cover_script)"
    if [[ -n "$COVER_PATH" ]]; then
        COVER_AT="$(gates_cover_at | jq -r 'join(", ")' 2>/dev/null)" || COVER_AT=""
        COVER_PROT="protected"
        grep -qxF -- "$(printf '%s' "$COVER_PATH" | sed -e 's#//*#/#g' -e 's#/\(\./\)*#/#g' -e 's#^\(\./\)*##')" \
            <<<"$(cd "$PROJECT_ROOT" && gates_protected_list 2>/dev/null)" || COVER_PROT="NOT protected"
        if ! COVER_WHY="$(gates_cover_script_check "$PROJECT_ROOT" "$COVER_PATH")"; then
            echo "${BAD}cover script $COVER_PATH: $COVER_WHY, so verify.sh refuses every run — add the script or remove spec.cover"
            MISSING=$((MISSING + 1))
        elif [[ "$COVER_PROT" != "protected" ]]; then
            echo "${BAD}cover script $COVER_PATH is not in the protected list — re-project the runtime"
            MISSING=$((MISSING + 1))
        else
            echo "${OK}cover script $COVER_PATH at ${COVER_AT:-?} ($(gates_cover_timeout)s), $COVER_PROT"
            [[ -x "$PROJECT_ROOT/$COVER_PATH" ]] \
                || echo "${REC}cover script $COVER_PATH is not executable, so it covers nothing (every block runs) — chmod +x it"
        fi
    fi
fi

# Git boundary wiring (issues #20/#23): an INSTALLED hook that git will
# not run is silent enforcement loss — the worst class. Non-executable or
# non-delegating installed hooks are enforcement GAPS (exit 1), and so is a
# hook manager whose config calls gates while git runs no hook for it
# (#167: its install command never ran); hooks that were never wired get a
# [rec] nudge only (agent+CI-only repos are a legitimate setup). Zip installs drop execute bits (Python extraction),
# which is exactly how downstream repos end up in the gap state.
DOC_GITERR="$(git -C "$PROJECT_ROOT" rev-parse --git-dir 2>&1 >/dev/null)" || true
if [[ "$CI_MODE" -eq 1 ]]; then
    echo ""
    echo "${SKIP}git boundary not checked (--ci: git hooks exist only in a developer clone)"
elif grep -q 'dubious ownership' <<<"$DOC_GITERR"; then
    # git refuses a repository another user owns (#203): no hook can be
    # read or proven, and skipping the section would hide why.
    echo ""
    echo "${BAD}git boundary not checked: git refuses this repository (dubious ownership: another user owns it). If you trust it: git config --global --add safe.directory '$PROJECT_ROOT'"
    MISSING=$((MISSING + 1))
elif git -C "$PROJECT_ROOT" rev-parse --git-dir >/dev/null 2>&1; then
    echo ""
    echo "Git boundary (hooks git actually runs):"
    # --git-path hooks resolves the directory git actually runs hooks from:
    # the shared one for a linked worktree (whose --git-dir is
    # .git/worktrees/<name>), and core.hooksPath when set.
    HOOK_DIR="$(git -C "$PROJECT_ROOT" rev-parse --git-path hooks)"
    [[ "$HOOK_DIR" != /* ]] && HOOK_DIR="$PROJECT_ROOT/$HOOK_DIR"
    HOOKS_PATH="$(git -C "$PROJECT_ROOT" config core.hooksPath 2>/dev/null || true)"
    if [[ -n "$HOOKS_PATH" ]]; then
        echo "${REC}core.hooksPath is set ($HOOKS_PATH) — a hook manager may own this boundary; the checks below inspect that path"
    fi
    # pre-merge-commit (#148) is what `git merge` runs instead of
    # pre-commit; git before 2.24 never calls it.
    GIT_VER="$(git --version 2>/dev/null | awk '{ print $3 }')"
    if declare -f gates_version_cmp >/dev/null 2>&1 && [[ -n "$GIT_VER" ]] \
        && [[ "$(gates_version_cmp "$GIT_VER" 2.24)" == "-1" ]]; then
        echo "${REC}git $GIT_VER is older than 2.24 and never runs pre-merge-commit — a local merge commit skips the pre-commit checks (upgrade git)"
    fi
    DOC_MGR=""
    declare -f gates_detect_manager >/dev/null 2>&1 && DOC_MGR="$(gates_detect_manager "$PROJECT_ROOT")"
    for h in pre-commit pre-merge-commit commit-msg; do
        hf="$HOOK_DIR/$h"
        if [[ ! -f "$hf" ]] && [[ "$DOC_MGR" == "husky" || "$DOC_MGR" == "lefthook" || "$DOC_MGR" == "pre-commit" ]] \
            && gates_manager_wired "$PROJECT_ROOT" "$DOC_MGR" "$h"; then
            # Wired in the manager's config, but its install command has
            # not generated the hook yet (#148): a commit runs no gates
            # check, so this fails (#167).
            echo "${BAD}$h not installed — $(gates_manager_file "$PROJECT_ROOT" "$DOC_MGR" "$h") calls the gates hook, but git runs no $h hook until you run \`$(gates_manager_install_hint "$DOC_MGR" "$h")\`"
            MISSING=$((MISSING + 1))
        elif [[ ! -f "$hf" ]]; then
            echo "${REC}$h not installed — the git boundary is not enforced here (run /speckit.gates.init to wire it)"
        elif [[ ! -x "$hf" ]]; then
            echo "${BAD}$h installed but NOT executable — git silently skips it (fix: chmod +x ${hf#"$PROJECT_ROOT"/})"
            MISSING=$((MISSING + 1))
        elif grep -q 'spec-gates hook stub' "$hf" 2>/dev/null; then
            # Stub (issue #59): runs the checked-out branch's projected hook.
            if [[ -f "$PROJECT_ROOT/.specify/gates/hooks/$h" ]]; then
                echo "${OK}$h installed as a stub, runs this branch's .specify/gates/hooks/$h"
            else
                echo "${BAD}$h stub installed but .specify/gates/hooks/$h is missing — the stub refuses every commit on this branch until it is restored (fix: /speckit.gates.upgrade)"
                MISSING=$((MISSING + 1))
            fi
        elif grep -q 'Git commit-msg hook\|Git pre-commit hook --' "$hf" 2>/dev/null; then
            echo "${OK}$h installed, executable, delegates to the gates runtime"
            echo "${REC}$h is a copied hook: it stays at the version it was installed with on every branch — run /speckit.gates.upgrade to install the branch-following stub"
        elif declare -f gates_legacy_stub >/dev/null 2>&1 \
            && legacy="$(gates_legacy_stub "$PROJECT_ROOT" "$h")" && [[ "$legacy" != "none" ]]; then
            # `pre-commit install` moved the stub to <hook>.legacy and runs
            # it first (#201). A stale copy refuses every commit; the static
            # check below reports it.
            if [[ "$legacy" == "current" ]]; then
                echo "${OK}$h is the pre-commit framework's hook and runs the gates stub it moved to $h.legacy (\`pre-commit install -f\` deletes that file; add the gates entry to .pre-commit-config.yaml first)"
                if gates_manager_wired "$PROJECT_ROOT" pre-commit "$h"; then
                    echo "${REC}$h runs gates twice: .pre-commit-config.yaml calls the gates hook as well (fix: \`pre-commit install -f --hook-type $h\` deletes $h.legacy)"
                fi
            fi
        elif declare -f gates_calls_through >/dev/null 2>&1 && gates_calls_through "$hf" "$h"; then
            # The call-through to .specify/gates/hooks/<name> on a line that
            # runs, not any mention of "gates" (#128).
            echo "${OK}$h installed, executable, delegates to the gates runtime"
        elif ! { declare -f gates_hook_static >/dev/null 2>&1 && gates_hook_static "$PROJECT_ROOT" "$h"; }; then
            # A manager hook wired correctly is reported once, by the
            # static check below (#167).
            echo "${REC}$h is executable but does not call .specify/gates/hooks/$h itself — another tool owns it; gates checks run on commit only if that tool calls the gates hook"
        fi
    done
    # Proof (#74). When gates owns the hook (the stub), run it the way git
    # does with GATES_PROBE=1: only gates code runs, and only the gates hook
    # answers. A hook another tool owns is read, not run -- running it would
    # run that tool's steps too (husky's default is `npm test`) -- unless
    # --probe-git asks for the full chain.
    if declare -f gates_git_check >/dev/null 2>&1; then
        for h in pre-commit pre-merge-commit commit-msg; do
            [[ -x "$HOOK_DIR/$h" ]] || continue
            if gates_git_check "$PROJECT_ROOT" "$h" "$PROBE_GIT"; then
                if [[ "$GATES_CHECK_KIND" == "probe" ]]; then
                    echo "${OK}$h probe: the hook git runs reaches the gates $h hook, and its refusal reaches git"
                else
                    echo "${OK}$h (static): another tool owns the hook and calls the gates $h hook (doctor --probe-git runs the chain)"
                fi
            else
                echo "${BAD}$h ($GATES_CHECK_KIND): $GATES_PROBE_MSG — the git boundary is not enforced for $h"
                MISSING=$((MISSING + 1))
            fi
        done
    fi
    # Protected-change path (issue #47): how a staged protected_files.extra
    # entry is treated at this boundary. Informational, never a failure.
    if declare -f gates_protected_trailer_enabled >/dev/null 2>&1; then
        if gates_protected_trailer_enabled; then
            echo "${OK}protected-change trailer enabled — protected files commit with 'Protected-Change: <path>' + 'Approved-By: <name>' trailers"
        else
            echo "${REC}protected-change trailer disabled (git.protected_change_trailer=false) — staged protected files are refused outright"
        fi
    fi
fi

# Policy contract (feature 003): what the contract gate sees, from local
# information only. Doctor fails on exactly the drift conditions the gate
# blocks on; a declared-but-never-synced contract gets the actionable
# nudge alongside the failure (the gate is already blocking runs).
if declare -f gates_contract_check >/dev/null 2>&1 \
    && declare -f gates_sha256 >/dev/null 2>&1; then
    gates_contract_paths "$PROJECT_ROOT"
    if gates_contract_declared "$CONTRACT_OVERLAY"; then
        echo ""
        echo "Policy contract (extends baseline):"
        echo "  source: $CONTRACT_SOURCE @ $CONTRACT_VERSION ($CONTRACT_BASEFILE)"
        if [[ ! -f "$CONTRACT_LOCK" && ! -f "$CONTRACT_SNAPSHOT" && ! -f "$CONTRACT_EFFECTIVE" ]]; then
            echo "${BAD}declared but never synced -- gate runs fail closed until the contract is materialized"
            echo "${REC}run /speckit.gates.sync (or: bash .specify/gates/contract.sh sync) to pin and materialize"
            MISSING=$((MISSING + 1))
        else
            gates_contract_check "$PROJECT_ROOT"
            if [[ "$CONTRACT_STATUS" == "pass" ]]; then
                echo "${OK}pinned $CONTRACT_PIN_DIGEST"
                echo "${OK}snapshot matches the pin; effective policy matches recomputation"
                if [[ -n "$CONTRACT_DEVIATIONS" ]]; then
                    echo "  deviations: $CONTRACT_WEAKENED weakened, $CONTRACT_CHANGED changed"
                    gates_contract_print_deviations "  [dev]  " <<<"$CONTRACT_DEVIATIONS"
                else
                    echo "${OK}no deviations -- the overlay only adds or strengthens"
                fi
            else
                echo "${BAD}$CONTRACT_DETAIL"
                MISSING=$((MISSING + 1))
            fi
        fi
    fi
fi

# Constitution enforcement (feature 004): a constitution whose principles carry
# gates:enforce markers is proven here from local files only. A gap or a
# malformed marker is a doctor FAILURE at fixed severity (FR-009) naming the
# principle + surface (+ line). A constitution with no markers gets one
# informational nudge; no constitution at all is silent (init offers the
# session). prose-only is listed, never failed (FR-013).
if declare -f gates_const_check_raw >/dev/null 2>&1; then
    CONST_MD="$PROJECT_ROOT/.specify/memory/constitution.md"
    if [[ -f "$CONST_MD" ]]; then
        if grep -q 'gates:enforce' "$CONST_MD" 2>/dev/null; then
            echo ""
            echo "Constitution enforcement (gates:enforce annotations):"
            CONST_RAW="$(gates_const_check_raw "$PROJECT_ROOT" "$CONST_MD" || true)"
            while IFS=$'\t' read -r ctag c1 c2 c3 c4; do
                case "$ctag" in
                    ENFORCED) echo "${OK}$c1 ($c2:$c3)" ;;
                    PROSE) echo "${SKIP}$c1 (prose-only)" ;;
                    GAP)
                        echo "${BAD}$c1 — $c2 ref=$c3 not enforced ($c4)"
                        MISSING=$((MISSING + 1))
                        ;;
                    MALFORMED)
                        echo "${BAD}constitution.md:$c1: malformed marker: $c2"
                        MISSING=$((MISSING + 1))
                        ;;
                    UNANNOTATED)
                        [[ "$c1" -gt 0 ]] && echo "  $c1 principle(s) unannotated (informational)"
                        ;;
                    NOCORE)
                        echo "${REC}constitution.md has no '## Core Principles' section, so it declares no principles (#82)"
                        ;;
                esac
            done <<<"$CONST_RAW"
        else
            echo ""
            echo "${REC}constitution has no enforcement annotations — /speckit.gates.constitution can add them"
        fi
    fi
fi

echo ""
echo "Recommended (optional):"
have node && echo "${OK}node (to install pinned linters via npm ci)" \
    || echo "${REC}node — install pinned prettier/markdownlint-cli2 for reproducible gates"
have shfmt && echo "${OK}shfmt (shell auto-format in post-edit)" \
    || echo "${REC}shfmt — enables shell auto-formatting"
have task && echo "${OK}task (only needed for orchestrator: task)" \
    || echo "${REC}task — only if your policy uses orchestrator: task"

echo ""
if [[ "$MISSING" -gt 0 ]]; then
    echo "doctor: $MISSING required item(s) missing."
    exit 1
fi
echo "doctor: all required tooling present."
exit 0
