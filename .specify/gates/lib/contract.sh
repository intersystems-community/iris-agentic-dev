#!/usr/bin/env bash
# contract.sh (lib) -- policy-as-versioned-contract helpers (feature 003).
#
# Sourced by verify.sh, doctor.sh, and the projected contract.sh entry
# script. Provides:
#   gates_contract_paths <root>            # artifact path globals
#   gates_contract_declared <overlay>      # 0 iff extends present; sets fields
#   gates_contract_merge <snap> <overlay>  # canonical effective JSON on stdout
#   gates_contract_deviations <snap> <eff> # TSV: class<TAB>path<TAB>from<TAB>to...
#   gates_contract_check <root>            # the four invariants (R6)
#   gates_contract_update_staged <root>    # index holds exactly a sync --update commit
#   gates_contract_fetch <src> <ver> <file> <out>  # sync-time fetch (R2)
#   gates_contract_version_max             # stdin versions -> highest (R7)
#
# The overlay (policy.json) is user-owned; this library never writes it.
# Everything below the fetch line is offline: verify-time drift proving
# needs only the committed snapshot, pin, and effective policy. All bash
# 3.2 + jq + git; digests via gates_sha256 (lib/attest.sh).

# shellcheck disable=SC2034   # library file; CONTRACT_* consumed by callers

# Artifact locations (contracts/artifact-layout.md): committed, beside the
# user-owned policy.json.
gates_contract_paths() { # <root>
    local root="${1:-.}"
    CONTRACT_OVERLAY="$root/.specify/gates/policy.json"
    CONTRACT_SNAPSHOT="$root/.specify/gates/baseline.json"
    CONTRACT_LOCK="$root/.specify/gates/baseline.lock.json"
    CONTRACT_EFFECTIVE="$root/.specify/gates/policy.effective.json"
}

# Does <overlay> declare a baseline? 0 = yes (fields in CONTRACT_SOURCE /
# CONTRACT_VERSION / CONTRACT_BASEFILE, file defaulted), 1 = dormant.
gates_contract_declared() { # <overlay>
    local overlay="${1:-}"
    CONTRACT_SOURCE=""
    CONTRACT_VERSION=""
    CONTRACT_BASEFILE=""
    [[ -f "$overlay" ]] || return 1
    grep -q '"extends"' "$overlay" 2>/dev/null || return 1
    local triple
    triple="$(jq -r 'select(has("extends"))
        | "\(.extends.source // "")\t\(.extends.version // "")\t\(.extends.file // "policy.json")"' \
        "$overlay" 2>/dev/null)"
    [[ -z "$triple" ]] && return 1
    IFS=$'\t' read -r CONTRACT_SOURCE CONTRACT_VERSION CONTRACT_BASEFILE <<<"$triple"
    [[ -n "$CONTRACT_SOURCE" && -n "$CONTRACT_VERSION" ]]
}

# Materialize the effective policy (R4): jq recursive object-merge, overlay
# wins, arrays and scalars replace wholesale; the extends section never
# participates and is re-attached verbatim; output canonicalized (jq -S) so
# byte-identity is a stable, recomputable property.
gates_contract_merge() { # <snapshot> <overlay>
    local snap="${1:-}" overlay="${2:-}"
    [[ -f "$snap" && -f "$overlay" ]] || return 1
    jq -S -n --slurpfile b "$snap" --slurpfile o "$overlay" '
        ($b[0] | del(.extends)) as $base
        | ($o[0]) as $ov
        | ($base * ($ov | del(.extends)))
          + (if ($ov | has("extends")) then { extends: $ov.extends } else {} end)
    '
}

# Deviation inventory (R5, Clarifications): compare baseline leaves against
# the effective policy. A "leaf" is a scalar or a whole array reached via
# object keys only — array interiors are excluded so a list edit is ONE
# deviation on the list, not one per element. Defined-order fields classify
# as "weakened" or "strengthened": enabled and the git toggles true->false,
# severity/parity moving right, include losing globs or exclude gaining
# them, git.ai_branding.terms losing terms. A whole hook the effective side
# drops (null or absent) is ONE weakened deviation marked "removed".
# Everything else that differs is "changed". Strengthenings and additions
# are not deviations. Keys starting with "_" (_comment) are annotations,
# never enforcement, and are not compared.
#
# Project gates (feature 006, contracts/policy-gates.md): a baseline gate
# under hooks.verify-quality.gates that the effective side drops is ONE
# weakened deviation marked "removed", like a hook; a gate's skip_at
# classifies like exclude (gaining a boundary is weakened, losing one
# strengthened; absent is none); a gate's severity (default error) and
# test_severity (default warning) use the severity rank; command,
# timeout_s and gate_timeout_s are "changed". Since these keys are often
# absent from the baseline, the effective side's skip_at, severity,
# timeout_s, test_severity and gate_timeout_s of baseline gates are
# compared too, not only the baseline's leaves.
#
# The covering script (feature 007, contracts/policy-cover.md): spec.cover
# present in the effective policy and absent from the baseline is ONE
# weakened deviation marked "cover script added", although additions are
# otherwise not deviations (a covering script skips proof); a baseline
# spec.cover the effective side drops is ONE strengthened line ("cover
# script removed"). With both present, spec.cover.at gaining a boundary is
# weakened and losing one strengthened; script and timeout_s are
# "changed", the effective side's keys compared too.
#
# With mode "delta" (sync --update: old baseline vs new baseline) the
# strengthenings are reported too, and a hook or gate the new side adds is
# ONE strengthened line marked "added": an update review needs every change.
#
# Output: one TSV line per deviation:
#   <class>\t<dot.path>\t<baseline-value>\t<effective-value>\t<path-as-json-array>\t<summary>\t<removed|added|>
# The JSON path is what propose feeds to setpath (dot-joined paths would
# break on keys containing dots). The summary describes the change without
# quoting policy text (#154): list changes as counts; booleans, numbers and
# severities as from -> to; any other value as set, unset or changed. The
# delta lines that become commit and PR text use it, since a quoted value
# ("Copilot" in git.ai_branding.terms, a word the message rules forbid)
# would make the repo's own commit-msg refuse the update commit.
gates_contract_deviations() { # <snapshot> <effective> [delta]
    local snap="${1:-}" eff="${2:-}" mode="${3:-}"
    [[ -f "$snap" && -f "$eff" ]] || return 1
    jq -r -n --slurpfile b "$snap" --slurpfile e "$eff" --arg mode "$mode" '
        def sev_rank: { "error": 0, "warning": 1, "info": 2, "off": 3 };
        def git_toggles: ["block_main_commits", "conventional_commits", "forbid_ai_isms",
                          "protected_change_trailer", "block_bulk_staging"];
        # Toggles that relax a protection when on (#313): true is the
        # weaker side, and an absent key is its default, false.
        def git_relaxers: ["allow_force_with_lease_on_branches"];
        def leaves: [ paths(type != "object") | select(all(.[]; type == "string"))
                      | select(any(.[]; startswith("_")) | not) ];
        def hook_names: (.hooks // {}) | if type == "object" then
            [ to_entries[] | select(.value | type == "object") | select(.key | startswith("_") | not)
              | .key ] else [] end;
        def gate_names: (try .hooks["verify-quality"].gates catch null) // {}
            | if type == "object" then
                [ to_entries[] | select(.value | type == "object") | select(.key | startswith("_") | not)
                  | .key ] else [] end;
        def is_gate_field($p): ($p | length) == 5 and $p[0] == "hooks" and $p[1] == "verify-quality"
            and $p[2] == "gates";
        def sev_default($p):
            if is_gate_field($p) and $p[4] == "severity" then "error"
            elif $p == ["hooks", "verify-quality", "test_severity"] then "warning"
            else null end;
        def sev_leaf: . == "severity" or . == "parity" or . == "test_severity";
        def summary($leaf; $bv; $ev):
            def plain: type == "boolean" or type == "number" or type == "null";
            if ($bv | type) == "array" and ($ev | type) == "array" then
                (($ev - $bv) | length) as $a | (($bv - $ev) | length) as $r
                | if $a == 0 and $r == 0 then "same entries, order or repeats changed"
                  else "\($a) added, \($r) removed" end
            elif ($ev | type) == "array" and $bv == null then "set (\($ev | length) entries)"
            elif ($bv | type) == "array" and $ev == null then "unset (was \($bv | length) entries)"
            elif (($bv | plain) and ($ev | plain))
                 or (($leaf | sev_leaf)
                     and (sev_rank[$bv|tostring] != null) and (sev_rank[$ev|tostring] != null)) then
                "\($bv | tojson) -> \($ev | tojson)"
            elif $bv == null then "set"
            elif $ev == null then "unset"
            else "value changed" end;
        def row($c; $p; $bv; $ev; $kind):
            [ $c, ($p | map(tostring) | join(".")), ($bv | tojson), ($ev | tojson), ($p | tojson),
              (if $kind == "" then summary($p | last | tostring; $bv; $ev)
               elif ($kind | startswith("cover ")) then $kind
               elif ($p | length) == 4 then "gate \($kind)" else "hook \($kind)" end),
              $kind ]
            | join("\t");
        ($b[0] | del(.extends)) as $base
        | ($e[0] | del(.extends)) as $eff
        | ($base | hook_names) as $bh
        | ($eff | hook_names) as $eh
        | [ $bh[] | select(IN($eh[]) | not) ] as $removed
        | [ $eh[] | select(IN($bh[]) | not) ] as $added
        # Gate rows only when the verify-quality hook itself is on both sides
        # (a removed or added hook is already its one line).
        | (if "verify-quality" | IN(($removed + $added)[]) then [] else ($base | gate_names) end) as $bg
        | (if "verify-quality" | IN(($removed + $added)[]) then [] else ($eff | gate_names) end) as $eg
        | [ $bg[] | select(IN($eg[]) | not) ] as $gremoved
        | [ $eg[] | select(IN($bg[]) | not) ] as $gadded
        | def cover_of: (try .spec.cover catch null) | if type == "object" then . else null end;
          ($base | cover_of) as $bc
        | ($eff | cover_of) as $ec
        | ( $removed[] | row("weakened"; ["hooks", .]; $base.hooks[.]; null; "removed") ),
          ( if $mode == "delta" then
              $added[] | row("strengthened"; ["hooks", .]; null; $eff.hooks[.]; "added")
            else empty end ),
          ( $gremoved[]
            | row("weakened"; ["hooks", "verify-quality", "gates", .];
                  $base.hooks["verify-quality"].gates[.]; null; "removed") ),
          ( if $mode == "delta" then
              $gadded[]
              | row("strengthened"; ["hooks", "verify-quality", "gates", .];
                    null; $eff.hooks["verify-quality"].gates[.]; "added")
            else empty end ),
          ( if $bc == null and $ec != null then
              row("weakened"; ["spec", "cover"]; null; $ec; "cover script added")
            elif $bc != null and $ec == null and $mode == "delta" then
              row("strengthened"; ["spec", "cover"]; $bc; null; "cover script removed")
            else empty end ),
          ( ( ($base | leaves)
              + (if $mode == "delta" then ($eff | leaves)
                 else [ $eff | leaves | .[]
                        | select((. == ["hooks", "verify-quality", "test_severity"])
                                 or (length == 2 and .[0] == "git" and (.[1] | IN(git_relaxers[])))
                                 or (. == ["hooks", "verify-quality", "gate_timeout_s"])
                                 or (is_gate_field(.) and (.[3] | IN($bg[])))
                                 or (.[0:2] == ["spec", "cover"] and $bc != null)) ] end) )
            | unique[]
            | . as $p
            # An added or removed spec.cover is its one line above.
            | select((.[0:2] == ["spec", "cover"] and ($bc == null or $ec == null)) | not)
            | select(($p[0] == "hooks" and (($p[1] // "") | IN(($removed + $added)[])))
                     or ($p == ["hooks"])
                     or (($p | length) >= 4 and $p[0:3] == ["hooks", "verify-quality", "gates"]
                         and ($p[3] | IN(($gremoved + $gadded)[]))) | not)
            | ($base | try getpath($p) catch null) as $bv
            | ($eff | try getpath($p) catch null) as $ev
            | select($bv != $ev)
            | ($p | last | tostring) as $leaf
            | ($bv // sev_default($p)) as $bs
            | ($ev // sev_default($p)) as $es
            | ( if ($p | length) == 2 and $p[0] == "git" and ($leaf | IN(git_relaxers[]))
                   and (($bv // false) | type) == "boolean" and (($ev // false) | type) == "boolean" then
                  (if ($ev // false) then "weakened" else "strengthened" end)
                elif ($leaf == "enabled"
                    or ($p | length) == 2 and $p[0] == "git" and ($leaf | IN(git_toggles[])))
                   and ($bv | type) == "boolean" and ($ev | type) == "boolean" then
                  (if $bv then "weakened" else "strengthened" end)
                elif ($leaf | sev_leaf)
                     and (sev_rank[$bs|tostring] != null) and (sev_rank[$es|tostring] != null) then
                  ( if sev_rank[$es|tostring] > sev_rank[$bs|tostring] then "weakened"
                    elif sev_rank[$es|tostring] < sev_rank[$bs|tostring] then "strengthened"
                    else "changed" end )
                elif ((is_gate_field($p) and $leaf == "skip_at") or $p == ["spec", "cover", "at"])
                     and (($bv // []) | type) == "array" and (($ev // []) | type) == "array" then
                  ( (($ev // []) - ($bv // []) | length) as $gained
                    | (($bv // []) - ($ev // []) | length) as $lost
                    | if $gained > 0 and $lost == 0 then "weakened"
                      elif $lost > 0 and $gained == 0 then "strengthened"
                      else "changed" end )
                elif ($leaf == "include" or $leaf == "exclude")
                     and ($bv | type) == "array" and ($ev | type) == "array" then
                  ( ($ev - $bv | length) as $gained | ($bv - $ev | length) as $lost
                    | (if $leaf == "include" then [$lost, $gained] else [$gained, $lost] end) as $wl
                    | if $wl[0] > 0 and $wl[1] == 0 then "weakened"
                      elif $wl[1] > 0 and $wl[0] == 0 then "strengthened"
                      else "changed" end )
                elif $p == ["git", "ai_branding", "terms"]
                     and ($bv | type) == "array" and ($ev | type) == "array" then
                  ( ($ev - $bv | length) as $gained | ($bv - $ev | length) as $lost
                    | if $gained > 0 and $lost == 0 then "strengthened"
                      elif $lost > 0 and $gained == 0 then "weakened"
                      else "changed" end )
                else "changed" end ) as $class
            | select($mode == "delta" or $class != "strengthened")
            | row($class; $p; $bv; $ev; "") )
    '
}

# Print deviation TSV (stdin) as one line per deviation, after <prefix>:
#   <prefix>(<class>): <path>: baseline <from> -> overlay <to>
#   <prefix>(weakened): <path>: removed
# The update review and propose use the "- " form with the summary instead
# of the values (delta lines): that text becomes commit and PR text (#154).
gates_contract_print_deviations() { # <prefix> [delta]
    local prefix="$1" mode="${2:-}" class path from to jpath summary kind
    while IFS=$'\t' read -r class path from to jpath summary kind; do
        [[ -z "$class" ]] && continue
        if [[ "$mode" == "delta" ]]; then
            if [[ -n "$kind" ]]; then
                printf '%s%s (%s): %s\n' "$prefix" "$kind" "$class" "$path"
            else
                printf '%s%s: %s: %s\n' "$prefix" "$class" "$path" "$summary"
            fi
        elif [[ -n "$kind" ]]; then
            printf '%s(%s): %s: %s\n' "$prefix" "$class" "$path" "$kind"
        else
            printf '%s(%s): %s: baseline %s -> overlay %s\n' "$prefix" "$class" "$path" "$from" "$to"
        fi
    done
    return 0
}

# Shape check for the overlay (policy.json with extends). The overlay is a
# partial policy: hooks may be absent, a hook may set only the fields it
# changes (no severity) or be null (removed). Field values are still
# checked here so a typo is named against policy.json; completeness is the
# merged effective policy's job (validated strictly at sync).
gates_contract_validate_overlay() { # <overlay>
    local overlay="${1:-}" tmp rc=0
    [[ -f "$overlay" ]] || return 2
    if ! jq -e 'type == "object" and ((has("hooks") | not) or (.hooks | type == "object"))' \
        "$overlay" >/dev/null 2>&1; then
        echo "ERROR: $overlay must be an object (with \"hooks\", when present, an object)" >&2
        return 4
    fi
    tmp="$(mktemp 2>/dev/null || mktemp -t gates-overlay)" || return 1
    # Fill what only the merge supplies, then validate the values. A project
    # gate may restate only what it changes (a skip_at on a baseline gate,
    # feature 006), so its command is filled too.
    jq '.hooks = ((.hooks // {}) | with_entries(
            if (.value | type) == "object" then
                .value |= ((if has("severity") then . else . + { severity: "error" } end)
                    | if .orchestrator == "custom" and (has("custom_command") | not)
                      then . + { custom_command: "-" } else . end
                    | if (.gates | type) == "object" then
                        .gates |= with_entries(
                            if (.value | type) == "object" and (.value | has("command") | not)
                            then .value += { command: "-" } else . end)
                      else . end)
            else . end))' "$overlay" >"$tmp" 2>/dev/null || {
        rm -f "$tmp"
        return 3
    }
    local out
    out="$(GATES_POLICY_FILE="$tmp" gates_validate_policy "$tmp" 2>&1)" || rc=$?
    rm -f "$tmp"
    # Name the user's file, not the filled-in copy (a few short lines).
    [[ -n "$out" ]] && printf '%s\n' "${out//"$tmp"/$overlay}" >&2
    return "$rc"
}

# Print <overlay> with extends.version set to <version>, changing only that
# value's text where possible so the reviewed diff is one line (#135). Falls
# back to a jq rewrite (key order kept) when the text edit is not exact.
gates_contract_set_version() { # <overlay> <version>
    local overlay="$1" version="$2" old want got
    want="$(jq -S --arg v "$version" '.extends.version = $v' "$overlay" 2>/dev/null)" || return 1
    old="$(jq -r '.extends.version // ""' "$overlay" 2>/dev/null)"
    if [[ -n "$old" ]]; then
        got="$(awk -v old="\"$old\"" -v new="\"$version\"" '
            !done && /"version"[[:space:]]*:/ {
                i = index($0, "\"version\""); rest = substr($0, i); j = index(rest, old)
                if (j > 0) {
                    $0 = substr($0, 1, i - 1) substr(rest, 1, j - 1) new substr(rest, j + length(old))
                    done = 1
                }
            }
            { print }' "$overlay")"
        if [[ "$(jq -S . <<<"$got" 2>/dev/null)" == "$want" ]]; then
            printf '%s\n' "$got"
            return 0
        fi
    fi
    jq --arg v "$version" '.extends.version = $v' "$overlay"
}

# The four invariants (R6, contracts/artifact-layout.md), proven offline.
# Sets CONTRACT_STATUS = dormant | pass | fail, CONTRACT_DETAIL (fail cause
# naming the artifact), CONTRACT_DEVIATIONS (TSV), CONTRACT_WEAKENED /
# CONTRACT_CHANGED counts, and on pass CONTRACT_EFFECTIVE_SHA256 plus the
# pin fields. Returns 0 unless the check itself could not run.
gates_contract_check() { # <root>
    local root="${1:-.}"
    gates_contract_paths "$root"
    CONTRACT_STATUS="dormant"
    CONTRACT_DETAIL=""
    CONTRACT_DEVIATIONS=""
    CONTRACT_WEAKENED=0
    CONTRACT_CHANGED=0
    CONTRACT_EFFECTIVE_SHA256=""
    CONTRACT_PIN_DIGEST=""
    if ! gates_contract_declared "$CONTRACT_OVERLAY"; then
        return 0
    fi
    # 1: all three artifacts exist.
    local f
    for f in "$CONTRACT_LOCK" "$CONTRACT_SNAPSHOT" "$CONTRACT_EFFECTIVE"; do
        if [[ ! -f "$f" ]]; then
            CONTRACT_STATUS="fail"
            CONTRACT_DETAIL="not synced (${f##*/} missing) -- run contract.sh sync (/speckit.gates.sync)"
            return 0
        fi
    done
    # 2: snapshot matches the pinned digest.
    local want got
    want="$(jq -r '.digest // ""' "$CONTRACT_LOCK" 2>/dev/null)"
    # A failed hash is a missing tool, not a mismatch: say which (#172).
    if ! got="sha256:$(gates_sha256 "$CONTRACT_SNAPSHOT" 2>/dev/null)"; then
        CONTRACT_STATUS="fail"
        CONTRACT_DETAIL="cannot hash baseline.json to check the pin: neither sha256sum nor shasum found -- install one (coreutils or perl)"
        return 0
    fi
    if [[ -z "$want" || "$got" != "$want" ]]; then
        CONTRACT_STATUS="fail"
        CONTRACT_DETAIL="baseline snapshot does not match the pin (baseline.json vs baseline.lock.json digest) -- tampering or a broken sync; re-run contract.sh sync"
        return 0
    fi
    # 3: the declaration still matches the pin. Checked before the recompute
    # on purpose: an edited extends section also perturbs the merge, and the
    # precise "declaration changed" message must win over generic drift.
    local lock_triple
    lock_triple="$(jq -r '"\(.source // "")\t\(.version // "")\t\(.file // "policy.json")"' "$CONTRACT_LOCK" 2>/dev/null)"
    if [[ "$lock_triple" != "$CONTRACT_SOURCE"$'\t'"$CONTRACT_VERSION"$'\t'"$CONTRACT_BASEFILE" ]]; then
        CONTRACT_STATUS="fail"
        CONTRACT_DETAIL="extends declaration changed since the last sync (policy.json vs baseline.lock.json) -- re-run contract.sh sync"
        return 0
    fi
    # 4: effective equals recomputation, byte for byte.
    local recomputed
    if ! recomputed="$(gates_contract_merge "$CONTRACT_SNAPSHOT" "$CONTRACT_OVERLAY")"; then
        CONTRACT_STATUS="fail"
        CONTRACT_DETAIL="could not recompute the effective policy from baseline.json + policy.json"
        return 0
    fi
    if [[ "$recomputed" != "$(cat "$CONTRACT_EFFECTIVE")" ]]; then
        CONTRACT_STATUS="fail"
        CONTRACT_DETAIL="effective policy drifted (policy.effective.json != baseline + overlay) -- edit policy.json and re-run contract.sh sync, never the effective file"
        return 0
    fi
    CONTRACT_STATUS="pass"
    CONTRACT_PIN_DIGEST="$want"
    CONTRACT_EFFECTIVE_SHA256="$(gates_sha256 "$CONTRACT_EFFECTIVE")" || CONTRACT_EFFECTIVE_SHA256=""
    CONTRACT_DEVIATIONS="$(gates_contract_deviations "$CONTRACT_SNAPSHOT" "$CONTRACT_EFFECTIVE" || true)"
    if [[ -n "$CONTRACT_DEVIATIONS" ]]; then
        CONTRACT_WEAKENED="$(printf '%s\n' "$CONTRACT_DEVIATIONS" | grep -c '^weakened' || true)"
        CONTRACT_CHANGED="$(printf '%s\n' "$CONTRACT_DEVIATIONS" | grep -c '^changed' || true)"
    fi
    return 0
}

# Does the index of the repo at <root> hold exactly the commit `contract.sh
# sync --update` makes (#154)? pre-commit asks this only where
# git.protected_change_trailer is false and would otherwise refuse the
# protected contract artifacts outright. All of these must hold, read from
# the index and HEAD, never the work tree:
#   - the branch is gates/baseline-<v> and the staged lock pins <v>;
#   - nothing is staged but policy.json and the three artifacts, and
#     policy.json and the lock are among them;
#   - policy.json differs from HEAD in extends.version only;
#   - the staged four pass the contract invariants (snapshot matches the
#     lock digest, declaration matches the lock, effective = recompute).
# The snapshot's provenance cannot be proven offline; that is what the
# review of the update branch is for. Returns 0 = exactly an update.
gates_contract_update_staged() { # <root>
    local root="${1:-.}" branch version staged f tmp rc=1
    local has_policy=0 has_lock=0
    branch="$(git -C "$root" symbolic-ref --short -q HEAD 2>/dev/null)" || return 1
    [[ "$branch" == gates/baseline-?* ]] || return 1
    version="${branch#gates/baseline-}"
    staged="$(git -C "$root" diff --cached --name-only --no-renames 2>/dev/null)" || return 1
    while IFS= read -r f; do
        case "$f" in
            .specify/gates/policy.json) has_policy=1 ;;
            .specify/gates/baseline.lock.json) has_lock=1 ;;
            .specify/gates/baseline.json | .specify/gates/policy.effective.json) ;;
            *) return 1 ;;
        esac
    done <<<"$staged"
    [[ "$has_policy" == "1" && "$has_lock" == "1" ]] || return 1
    tmp="$(mktemp -d 2>/dev/null || mktemp -d -t gates-update)" || return 1
    mkdir -p "$tmp/.specify/gates"
    for f in policy.json baseline.json baseline.lock.json policy.effective.json; do
        git -C "$root" show ":.specify/gates/$f" >"$tmp/.specify/gates/$f" 2>/dev/null || {
            rm -rf "$tmp"
            return 1
        }
    done
    if git -C "$root" show "HEAD:.specify/gates/policy.json" >"$tmp/head.json" 2>/dev/null \
        && [[ "$(jq -r '.version // ""' "$tmp/.specify/gates/baseline.lock.json" 2>/dev/null)" == "$version" ]] \
        && jq -e '.extends.version | type == "string"' "$tmp/head.json" >/dev/null 2>&1 \
        && [[ "$(jq -S 'del(.extends.version)' "$tmp/head.json" 2>/dev/null)" \
            == "$(jq -S 'del(.extends.version)' "$tmp/.specify/gates/policy.json" 2>/dev/null)" ]] \
        && (gates_contract_check "$tmp" && [[ "$CONTRACT_STATUS" == "pass" ]]); then
        rc=0
    fi
    rm -rf "$tmp"
    return "$rc"
}

# Fetch one document from a versioned git source (R2). Sync-time only --
# the single place the contract machinery may touch the network. Refuses
# branch-name versions (a moving pin is not a pin). Writes the raw document
# to <out>; caller canonicalizes/validates. Returns 0 on success, 2 with a
# named cause on stderr otherwise.
gates_contract_fetch() { # <source> <version> <file> <out>
    local source="${1:-}" version="${2:-}" file="${3:-}" out="${4:-}"
    if [[ -z "$source" || -z "$version" || -z "$file" || -z "$out" ]]; then
        echo "contract: fetch: missing argument" >&2
        return 2
    fi
    if [[ -n "$(git ls-remote --heads "$source" "refs/heads/$version" 2>/dev/null)" ]]; then
        echo "contract: '$version' is a branch on $source -- pin a tag or commit instead (a moving pin is not a pin)" >&2
        return 2
    fi
    local tmp
    tmp="$(mktemp -d 2>/dev/null || mktemp -d -t gates-contract)" || {
        echo "contract: fetch: mktemp failed" >&2
        return 2
    }
    local clone_ok=0
    # Without git's per-repository variables (#293): run from a hook or a
    # script that exports GIT_DIR, `git -C "$tmp/src" checkout` would check
    # the version out in the caller's repository instead of the clone.
    if (
        # shellcheck source=watch.sh disable=SC1091
        declare -F gates_git_env_clear >/dev/null \
            || source "$(dirname "${BASH_SOURCE[0]}")/watch.sh" || exit 1
        gates_git_env_clear
        git clone -q --depth 1 --branch "$version" "$source" "$tmp/src" 2>/dev/null \
            || { git clone -q "$source" "$tmp/src" 2>/dev/null \
                && git -C "$tmp/src" checkout -q "$version" 2>/dev/null; }
    ); then
        clone_ok=1
    fi
    if [[ "$clone_ok" != "1" ]]; then
        rm -rf "$tmp"
        echo "contract: could not fetch $source at '$version' (unreachable source or unknown version)" >&2
        return 2
    fi
    if [[ ! -f "$tmp/src/$file" ]]; then
        rm -rf "$tmp"
        echo "contract: $source@$version does not contain '$file'" >&2
        return 2
    fi
    cp "$tmp/src/$file" "$out"
    rm -rf "$tmp"
    return 0
}

# Highest version from stdin (one per line), numeric segment-wise compare
# after stripping a leading v (R7). No sort -V on the BSD floor.
gates_contract_version_max() {
    awk '
    function cmp(a, b,   x, y, na, nb, ax, bx, i, n) {
        x = a; y = b
        sub(/^v/, "", x); sub(/^v/, "", y)
        na = split(x, ax, ".")
        nb = split(y, bx, ".")
        n = (na > nb) ? na : nb
        for (i = 1; i <= n; i++) {
            if ((ax[i] + 0) > (bx[i] + 0)) return 1
            if ((ax[i] + 0) < (bx[i] + 0)) return -1
        }
        return 0
    }
    NF > 0 { if (best == "" || cmp($1, best) > 0) best = $1 }
    END { if (best != "") print best }
    '
}
