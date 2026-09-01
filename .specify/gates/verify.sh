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
# THIS policy. tests/test-ci-parity.sh asserts it.
#
# Usage:
#   verify.sh --boundary agent|git|ci [--json] [--dry-run] [--accept <feature|all>]
#
# --accept additionally executes the named incomplete feature(s)' accept
# blocks as informational output (feature 002); complete features are
# enforced on every run regardless.
#
# Exit codes: 0 = all gates green, 1 = internal error, 2 = gate failure.

BOUNDARY="unspecified"
JSON=0
DRY_RUN=0
ACCEPT_ARG=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --boundary) BOUNDARY="${2:?}"; shift 2 ;;
        --json)     JSON=1; shift ;;
        --dry-run)  DRY_RUN=1; shift ;;
        --accept)   ACCEPT_ARG="${2:?}"; shift 2 ;;
        *) echo "gates: unknown argument: $1" >&2; exit 1 ;;
    esac
done

PROJECT_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
GATES_DIR="$PROJECT_ROOT/.specify/gates"
POLICY_FILE="$GATES_DIR/policy.json"

if ! command -v jq >/dev/null 2>&1; then
    echo "gates: jq not found — cannot evaluate policy (run /speckit.gates.doctor)" >&2
    exit 1
fi
if [[ ! -f "$POLICY_FILE" ]]; then
    echo "gates: no policy at $POLICY_FILE (run /speckit.gates.init)" >&2
    exit 1
fi

# Path is resolved at runtime from the projected layout; the source= hint helps
# `shellcheck -x` runs, disable=SC1091 silences the unavoidable static miss.
# shellcheck source=lib/policy.sh disable=SC1091
source "$GATES_DIR/lib/policy.sh"
# shellcheck source=lib/attest.sh disable=SC1091
source "$GATES_DIR/lib/attest.sh"
# shellcheck source=lib/spec-gate.sh disable=SC1091
source "$GATES_DIR/lib/spec-gate.sh"
# shellcheck source=lib/contract.sh disable=SC1091
source "$GATES_DIR/lib/contract.sh"

FAILED=0
WARNINGS=0
declare -a RESULTS=()
declare -a ATT_GATES=()

record() { # name status detail
    RESULTS+=("{\"name\":\"$1\",\"status\":\"$2\",\"detail\":$(jq -Rn --arg d "$3" '$d')}")
}

# Append a GateEntry (attestation shape, data-model.md) for an evaluated
# gate. Empty strings become JSON null for the nullable fields.
att_gate() { # name bin version pinned candidates checked result reason duration_s
    ATT_GATES+=("$(jq -cn \
        --arg name "$1" --arg bin "$2" --arg version "$3" --arg pinned "$4" \
        --arg candidates "$5" --arg checked "$6" --arg result "$7" \
        --arg reason "$8" --arg duration "$9" '
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
          duration_s: ($duration | tonumber) }')")
}

run_gate() { # name severity cmd...
    local name="$1" severity="$2"; shift 2
    if [[ "$DRY_RUN" == "1" ]]; then
        record "$name" "planned" "$*"
        return 0
    fi
    local start end rc=0
    start="$(date +%s)"
    "$@" >/tmp/gates-out.$$ 2>&1 || rc=$?
    end="$(date +%s)"

    # Check-mode metadata (##gates-meta##, emitted by formatter-dispatch) is
    # machinery, not gate output: parse it for the attestation entry, strip
    # it from the visible detail. Non-file gates (task/custom) have none.
    local meta binname="" bin="" candidates="" checked="" skipped=""
    meta="$(grep '##gates-meta##' /tmp/gates-out.$$ 2>/dev/null | tail -n 1 || true)"
    if [[ -n "$meta" ]]; then
        binname="$(printf '%s' "$meta" | sed -n 's/.* binname=\([^ ]*\).*/\1/p')"
        candidates="$(printf '%s' "$meta" | sed -n 's/.* candidates=\([0-9]*\).*/\1/p')"
        checked="$(printf '%s' "$meta" | sed -n 's/.* checked=\([0-9]*\).*/\1/p')"
        skipped="$(printf '%s' "$meta" | sed -n 's/.* skipped=\([^ ]*\).*/\1/p')"
        bin="$(printf '%s' "$meta" | sed -n 's/.* bin=//p')"
    fi
    local detail
    detail="$(grep -v '##gates-meta##' /tmp/gates-out.$$ | tail -c 2000 || true)"
    rm -f /tmp/gates-out.$$

    local version="" pinned=""
    if [[ -n "$binname" ]]; then
        version="$(gates_tool_version "$binname" "$PROJECT_ROOT")"
        pinned="$(gates_pin_version "$binname" "$PROJECT_ROOT")"
    fi
    [[ -n "$bin" && "$bin" == "$PROJECT_ROOT/"* ]] && bin="${bin#"$PROJECT_ROOT"/}"

    # A missing tool is skipped with a reason, never pass (spec edge case).
    # The attestation reason carries no tool output: output can quote file
    # contents, which FR-011 keeps out of records.
    local result reason=""
    if [[ "$rc" -eq 0 && -n "$skipped" ]]; then
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
        fail) record "$name" "fail" "$detail"; FAILED=$((FAILED + 1)) ;;
        warn) record "$name" "warn" "$detail"; WARNINGS=$((WARNINGS + 1)) ;;
    esac
    att_gate "$name" "$bin" "$version" "$pinned" "$candidates" "$checked" \
        "$result" "$reason" "$((end - start))"
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
contract_start="$(date +%s)"
gates_contract_check "$PROJECT_ROOT"
contract_end="$(date +%s)"
if [[ "$CONTRACT_STATUS" != "dormant" ]]; then
    if [[ "$DRY_RUN" == "1" ]]; then
        record "contract" "planned" "baseline contract integrity (extends)"
    elif [[ "$CONTRACT_STATUS" == "pass" ]]; then
        record "contract" "pass" ""
        att_gate "contract" "" "" "" "" "" "pass" "" "$((contract_end - contract_start))"
        if [[ "$JSON" == "0" && -n "$CONTRACT_DEVIATIONS" ]]; then
            # shellcheck disable=SC2034  # dev_rest swallows the JSON-path field
            while IFS=$'\t' read -r dev_class dev_path dev_from dev_to dev_rest; do
                [[ -z "$dev_class" ]] && continue
                echo "contract: deviation ($dev_class): $dev_path: baseline $dev_from -> overlay $dev_to"
            done <<<"$CONTRACT_DEVIATIONS"
        fi
    else
        # A broken contract is never a warning: fixed error severity (R6).
        record "contract" "fail" "$CONTRACT_DETAIL"
        FAILED=$((FAILED + 1))
        att_gate "contract" "" "" "" "" "" "fail" "$CONTRACT_DETAIL" "$((contract_end - contract_start))"
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
#   "task"   -> `task lint` (error class) and `task test` (warning class)
#   "custom" -> policy-supplied command, exit code mapped via severity
# The per-tool walk delegates to lib/formatter-dispatch.sh in check mode.
# ---------------------------------------------------------------------------
ORCH="$(gates_policy_get "verify-quality" "orchestrator")"
[[ -z "$ORCH" ]] && ORCH="none"

case "$ORCH" in
    task)
        run_gate "task-lint" "error"   task lint
        run_gate "task-test" "warning" task test
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
# Spec-conformance gate (feature 002): fenced accept blocks in specs/*/tasks.md
# run as executable acceptance criteria. Complete features (spec.md Status:
# Complete) are enforced at spec.severity; the rest is informational. Skipped
# entirely when GATES_SPEC_EXEC is set — accept blocks export it, so a block
# that invokes verify.sh cannot re-enter accept-block execution.
# ---------------------------------------------------------------------------
SPEC_ENABLED="$(gates_policy_section_get spec enabled)"
SPEC_ATT_JSON=""
if [[ "$SPEC_ENABLED" != "false" && -z "${GATES_SPEC_EXEC:-}" ]]; then
    if [[ "$DRY_RUN" == "1" ]]; then
        record "spec" "planned" "spec conformance (accept blocks)"
    else
        if [[ -n "$ACCEPT_ARG" && "$ACCEPT_ARG" != "all" ]]; then
            if ! gates_spec_features "$PROJECT_ROOT" | grep -qx "$ACCEPT_ARG"; then
                AVAILABLE="$(gates_spec_features "$PROJECT_ROOT" | tr '\n' ' ')"
                echo "gates: --accept: unknown feature: $ACCEPT_ARG (available: ${AVAILABLE:-none})" >&2
                exit 1
            fi
        fi
        SPEC_SEV="$(gates_policy_section_get spec severity)"
        [[ -z "$SPEC_SEV" ]] && SPEC_SEV="error"
        spec_start="$(date +%s)"
        gates_spec_gate "$PROJECT_ROOT" "$ACCEPT_ARG" "$JSON"
        spec_end="$(date +%s)"
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
        SPEC_ENF_EXECUTED="$(jq -r \
            '[.[] | select(.outcome | startswith("enforced")) | .blocks_executed] | add // 0' \
            <<<"$SPEC_RESULTS_JSON")"
        if [[ "$SPEC_RESULT" == "pass" ]]; then
            record "spec" "pass" ""
            att_gate "spec" "" "" "" "$SPEC_ENF_BLOCKS" "$SPEC_ENF_EXECUTED" \
                "pass" "" "$((spec_end - spec_start))"
        elif [[ "$SPEC_SEV" == "error" ]]; then
            record "spec" "fail" "$SPEC_DETAIL"
            FAILED=$((FAILED + 1))
            att_gate "spec" "" "" "" "$SPEC_ENF_BLOCKS" "$SPEC_ENF_EXECUTED" \
                "fail" "$SPEC_DETAIL" "$((spec_end - spec_start))"
        else
            record "spec" "warn" "$SPEC_DETAIL"
            WARNINGS=$((WARNINGS + 1))
            att_gate "spec" "" "" "" "$SPEC_ENF_BLOCKS" "$SPEC_ENF_EXECUTED" \
                "warn" "$SPEC_DETAIL" "$((spec_end - spec_start))"
        fi
        SPEC_ATT_JSON="$(jq -cn --argjson features "$SPEC_FEATURES" \
            --argjson parsed "$SPEC_PARSED" --argjson executed "$SPEC_EXECUTED" \
            --argjson passed "$SPEC_PASSED" --argjson failed "$SPEC_FAILED" \
            --argjson results "$SPEC_RESULTS_JSON" '
            { features: $features, parsed: $parsed, executed: $executed,
              passed: $passed, failed: $failed, results: $results }')"
    fi
fi

ATT_ENABLED="$(gates_policy_section_get attestation enabled)"

# ---------------------------------------------------------------------------
# Synthetic parity gate (R7): every boundary compares the tool versions just
# detected against the lockfile pins, so agent, git, and CI runs are proven
# equivalent transitively — no attestation transport needed. Severity comes
# from attestation.parity (default error; warning reports without failing;
# off omits the entry). Disabled attestations disable parity too.
# ---------------------------------------------------------------------------
PARITY_SEV="$(gates_policy_section_get attestation parity)"
[[ -z "$PARITY_SEV" ]] && PARITY_SEV="error"
if [[ "$DRY_RUN" != "1" && "$ATT_ENABLED" != "false" && "$PARITY_SEV" != "off" ]]; then
    parity_input="[]"
    if [[ ${#ATT_GATES[@]} -gt 0 ]]; then
        parity_input="[$(IFS=,; printf '%s' "${ATT_GATES[*]}")]"
    fi
    PARITY_DRIFT="$(gates_pin_mismatches "$parity_input")"
    if [[ -z "$PARITY_DRIFT" ]]; then
        record "parity" "pass" ""
        att_gate "parity" "" "" "" "" "" "pass" "" 0
    elif [[ "$PARITY_SEV" == "error" ]]; then
        record "parity" "fail" "$PARITY_DRIFT"
        FAILED=$((FAILED + 1))
        att_gate "parity" "" "" "" "" "" "fail" "$PARITY_DRIFT" 0
    else
        record "parity" "warn" "$PARITY_DRIFT"
        WARNINGS=$((WARNINGS + 1))
        att_gate "parity" "" "" "" "" "" "warn" "$PARITY_DRIFT" 0
    fi
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
        # The optional spec (002) and contract (003) objects are additive;
        # v stays 1 and consumers ignore unknown fields (001 rule).
        ATTESTATION="$(jq -cn --arg ts "$ATT_TS" --arg boundary "$BOUNDARY" \
            --arg sha "$POLICY_SHA" --arg rv "$RUNTIME_VERSION" \
            --argjson exit "$EXIT_CODE" --argjson gates "[$att_joined]" \
            --argjson spec "${SPEC_ATT_JSON:-null}" \
            --argjson contract "${CONTRACT_ATT_JSON:-null}" '
            { v: 1,
              ts: $ts,
              boundary: (if ["agent","git","ci"] | index($boundary) then $boundary else "unspecified" end),
              policy_sha256: $sha,
              runtime_version: (if $rv == "" then null else $rv end),
              exit: $exit,
              gates: $gates }
            + (if $spec != null then { spec: $spec } else {} end)
            + (if $contract != null then { contract: $contract } else {} end)')"
        MAX_RECORDS="$(gates_policy_section_get attestation max_records)"
        [[ -z "$MAX_RECORDS" ]] && MAX_RECORDS=200
        if ! gates_attest_append "$ATTESTATION" "$GATES_DIR/attestations.jsonl" "$MAX_RECORDS"; then
            echo "gates: warning: could not write $GATES_DIR/attestations.jsonl (gate outcome unaffected)" >&2
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
    if [[ -n "$ATTESTATION" ]]; then
        printf '{"boundary":"%s","failed":%d,"warnings":%d,"gates":[%s],"attestation":%s}\n' \
            "$BOUNDARY" "$FAILED" "$WARNINGS" "$joined" "$ATTESTATION"
    else
        printf '{"boundary":"%s","failed":%d,"warnings":%d,"gates":[%s]}\n' \
            "$BOUNDARY" "$FAILED" "$WARNINGS" "$joined"
    fi
else
    echo "gates: boundary=$BOUNDARY failed=$FAILED warnings=$WARNINGS"
    if [[ ${#RESULTS[@]} -gt 0 ]]; then
        for r in "${RESULTS[@]}"; do
            name="$(printf '%s' "$r" | jq -r '.name')"
            status="$(printf '%s' "$r" | jq -r '.status')"
            detail="$(printf '%s' "$r" | jq -r '.detail' | head -n 1)"
            if [[ -n "$detail" ]]; then
                echo "  [$status] $name -- $detail"
            else
                echo "  [$status] $name"
            fi
        done
    fi
fi

exit "$EXIT_CODE"
