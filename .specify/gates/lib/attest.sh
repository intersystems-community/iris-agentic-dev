#!/usr/bin/env bash
# attest.sh -- attestation-record helpers for verify.sh (feature 001).
#
# Sourced by verify.sh. Provides:
#   gates_sha256 <file>                  # SHA-256 hex of file bytes (R3)
#   gates_tool_version <binname> <root>  # resolved tool version, cached (R1)
#   gates_pin_version <pkgname> <root>   # lockfile pin, empty when absent (R2)
#   gates_attest_append <json> <log> <max>  # append + cap the JSONL log (R4)
#
# All bash 3.2 + jq. Version/pin lookups print an empty string when the
# answer is unknown -- callers map empty to JSON null.

# shellcheck disable=SC2034   # library file; vars are consumed by callers

# SHA-256 with a portable resolution chain: sha256sum (GNU) -> shasum -a 256
# (macOS). The policy hash is load-bearing (identity claim), so when neither
# tool exists this FAILS rather than silently skipping.
gates_sha256() { # <file>
    local file="${1:-}"
    if [[ -z "$file" || ! -f "$file" ]]; then
        echo "gates: sha256: no such file: $file" >&2
        return 1
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | cut -d' ' -f1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | cut -d' ' -f1
    else
        echo "gates: sha256: neither sha256sum nor shasum found — cannot attest policy identity" >&2
        return 1
    fi
}

# Per-run version cache (parallel arrays; bash 3.2 has no associative arrays).
_GATES_VER_KEYS=()
_GATES_VER_VALS=()

# Detect a tool's version once per run. Node-resolved tools read
# node_modules/<pkg>/package.json (fast, exact, cannot hang); PATH tools run
# their version command once and take the first version-shaped token.
gates_tool_version() { # <binname> <root>
    local bin="${1:-}" root="${2:-}"
    [[ -z "$bin" ]] && return 0
    local i=0
    while [[ $i -lt ${#_GATES_VER_KEYS[@]} ]]; do
        if [[ "${_GATES_VER_KEYS[$i]}" == "$bin" ]]; then
            printf '%s\n' "${_GATES_VER_VALS[$i]}"
            return 0
        fi
        i=$((i + 1))
    done
    local ver=""
    if [[ -x "$root/node_modules/.bin/$bin" && -f "$root/node_modules/$bin/package.json" ]]; then
        ver="$(jq -r '.version // empty' "$root/node_modules/$bin/package.json" 2>/dev/null || true)"
    elif command -v "$bin" >/dev/null 2>&1; then
        ver="$("$bin" --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+(\.[0-9]+)?' | head -n 1 || true)"
    fi
    _GATES_VER_KEYS+=("$bin")
    _GATES_VER_VALS+=("$ver")
    printf '%s\n' "$ver"
}

# Pin for a tool, in precedence order:
#   1. package-lock.json (lockfileVersion >= 2 carries .packages) -- the
#      shared source of truth for node-resolved linters;
#   2. .tool-versions (asdf format: "<tool> <version>" per line, # comments
#      ignored) -- for tools npm cannot pin, e.g. shellcheck, whose findings
#      differ across releases and so must be identical at every boundary.
# Prints nothing when neither declares the tool -- it is then attested with
# pinned: null and exempt from parity (spec Assumptions), which keeps repos
# that pin nothing working exactly as before.
gates_pin_version() { # <pkgname> <root>
    local pkg="${1:-}" root="${2:-}"
    [[ -n "$pkg" ]] || return 0
    local lock="$root/package-lock.json" ver=""
    if [[ -f "$lock" ]]; then
        ver="$(jq -r --arg p "node_modules/$pkg" '.packages[$p].version // empty' "$lock" 2>/dev/null || true)"
    fi
    if [[ -z "$ver" && -f "$root/.tool-versions" ]]; then
        ver="$(awk -v t="$pkg" '
            { sub(/#.*/, "") }
            $1 == t && $2 != "" { print $2; exit }
        ' "$root/.tool-versions" 2>/dev/null || true)"
    fi
    printf '%s' "$ver"
}

# Pin comparison for the synthetic parity gate (R7): given the assembled
# GateEntry objects as a JSON array, print one "; "-joined line naming each
# drifted tool, or nothing when every pinned tool matches. Tools with no pin
# (pinned null) are exempt; tools with no detected version (skipped/missing)
# are exempt too — their absence is surfaced as `skipped`, not as drift.
gates_pin_mismatches() { # <gates-json-array>
    printf '%s' "${1:-[]}" | jq -r '
        [ .[]
          | select(.pinned != null and .version != null and .version != .pinned)
          | "\(.name): resolved \(.version), pinned \(.pinned) "
            + (if .name == "shellcheck" then "(install the pinned version from .tool-versions)"
               else "(run npm ci)" end) ]
        | join("; ")' 2>/dev/null || true
}

# Append one single-line record, then cap: records are well under PIPE_BUF so
# the append is a single atomic-in-practice write; the cap rewrite goes
# through a temp file in the same directory + mv (atomic rename), so readers
# never observe a truncated file. Concurrent capping is last-writer-wins.
gates_attest_append() { # <record-json> <log-path> <max-records>
    local record="${1:-}" log="${2:-}" max="${3:-200}"
    [[ -n "$record" && -n "$log" ]] || return 1
    local dir
    dir="$(dirname "$log")"
    if [[ ! -d "$dir" ]]; then
        mkdir -p "$dir" || return 1
    fi
    printf '%s\n' "$record" >>"$log" || return 1
    local lines
    lines="$(wc -l <"$log" | tr -d '[:space:]')" || return 1
    if [[ "$lines" -gt "$max" ]]; then
        local tmp="$dir/.attestations.jsonl.$$"
        if ! tail -n "$max" "$log" >"$tmp"; then
            rm -f "$tmp"
            return 1
        fi
        if ! mv "$tmp" "$log"; then
            rm -f "$tmp"
            return 1
        fi
    fi
    return 0
}
