#!/usr/bin/env bash
# contract.sh (lib) -- policy-as-versioned-contract helpers (feature 003).
#
# Sourced by verify.sh, doctor.sh, and the projected contract.sh entry
# script. Provides:
#   gates_contract_paths <root>            # artifact path globals
#   gates_contract_declared <overlay>      # 0 iff extends present; sets fields
#   gates_contract_merge <snap> <overlay>  # canonical effective JSON on stdout
#   gates_contract_deviations <snap> <eff> # TSV: class<TAB>path<TAB>from<TAB>to
#   gates_contract_check <root>            # the four invariants (R6)
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
# as "weakened";
# everything else that differs is "changed"; strengthenings and additions
# are not deviations. Output: one TSV line per deviation:
#   <weakened|changed>\t<dot.path>\t<baseline-value>\t<effective-value>\t<path-as-json-array>
# The trailing JSON path is what propose feeds to setpath (dot-joined paths
# would break on keys containing dots).
gates_contract_deviations() { # <snapshot> <effective>
    local snap="${1:-}" eff="${2:-}"
    [[ -f "$snap" && -f "$eff" ]] || return 1
    jq -r -n --slurpfile b "$snap" --slurpfile e "$eff" '
        def sev_rank: { "error": 0, "warning": 1, "info": 2, "off": 3 };
        ($b[0] | del(.extends)) as $base
        | ($e[0] | del(.extends)) as $eff
        | [ $base | paths(type != "object") | select(all(.[]; type == "string")) ] as $ps
        | $ps[]
        | . as $p
        | ($base | getpath($p)) as $bv
        | ($eff | getpath($p)) as $ev
        | select($bv != $ev)
        | ($p | last | tostring) as $leaf
        | ($p | map(tostring) | join(".")) as $path
        | ( if $leaf == "enabled" and $bv == true and $ev == false then "weakened"
            elif $leaf == "enabled" and $bv == false and $ev == true then "skip"
            elif ($leaf == "severity" or $leaf == "parity")
                 and (sev_rank[$bv|tostring] != null) and (sev_rank[$ev|tostring] != null) then
              ( if sev_rank[$ev|tostring] > sev_rank[$bv|tostring] then "weakened"
                elif sev_rank[$ev|tostring] < sev_rank[$bv|tostring] then "skip"
                else "changed" end )
            elif $leaf == "include" and ($bv | type) == "array" and ($ev | type) == "array" then
              ( ($bv - $ev) as $removed | ($ev - $bv) as $added
                | if ($removed | length) > 0 and ($added | length) == 0 then "weakened"
                  elif ($added | length) > 0 and ($removed | length) == 0 then "skip"
                  else "changed" end )
            elif $leaf == "exclude" and ($bv | type) == "array" and ($ev | type) == "array" then
              ( ($ev - $bv) as $added | ($bv - $ev) as $removed
                | if ($added | length) > 0 and ($removed | length) == 0 then "weakened"
                  elif ($removed | length) > 0 and ($added | length) == 0 then "skip"
                  else "changed" end )
            else "changed" end ) as $class
        | select($class != "skip")
        | "\($class)\t\($path)\t\($bv | tojson)\t\($ev | tojson)\t\($p | tojson)"
    '
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
    got="sha256:$(gates_sha256 "$CONTRACT_SNAPSHOT")" || got=""
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
    if git ls-remote --heads "$source" "refs/heads/$version" 2>/dev/null | grep -q .; then
        echo "contract: '$version' is a branch on $source -- pin a tag or commit instead (a moving pin is not a pin)" >&2
        return 2
    fi
    local tmp
    tmp="$(mktemp -d 2>/dev/null || mktemp -d -t gates-contract)" || {
        echo "contract: fetch: mktemp failed" >&2
        return 2
    }
    local clone_ok=0
    if git clone -q --depth 1 --branch "$version" "$source" "$tmp/src" 2>/dev/null; then
        clone_ok=1
    elif git clone -q "$source" "$tmp/src" 2>/dev/null \
        && git -C "$tmp/src" checkout -q "$version" 2>/dev/null; then
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
