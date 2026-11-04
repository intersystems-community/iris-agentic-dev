#!/usr/bin/env bash
# attest.sh -- attestation-record helpers for verify.sh (feature 001).
#
# Sourced by verify.sh and install-shellcheck.sh. Provides:
#   gates_sha256 <file>                  # SHA-256 hex of file bytes (R3)
#   gates_tool_version <binname> <root>  # resolved tool version, cached (R1)
#   gates_pin_version <pkgname> <root>   # lockfile pin, empty when absent (R2)
#   gates_pin_problems <pkgname> <root>  # conflicting pins, unreadable mise files
#   gates_attest_append <json> <log> <max>  # append + cap the JSONL log (R4)
#   gates_declared_not_run <b> <declared> <gates>  # parity's gate-set check (006)
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

# Detect a tool's version once per run. The node linters resolved from
# node_modules read node_modules/<pkg>/package.json (fast, exact, cannot
# hang); any other tool runs its version command once and takes the first
# version-shaped token. <path> is the binary the gate ran, when known: the
# version is that binary's. The npm "shellcheck" wrapper's package.json
# carries the wrapper's own version, no shellcheck release (#274), so for
# that tool the binary is always asked.
gates_tool_version() { # <binname> <root> [path]
    local bin="${1:-}" root="${2:-}" path="${3:-}"
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
    [[ -z "$path" ]] && path="$bin"
    if [[ "$_GATES_NODE_LINTERS" == *" $bin "* && -x "$root/node_modules/.bin/$bin" \
        && -f "$root/node_modules/$bin/package.json" ]]; then
        ver="$(jq -r '.version // empty' "$root/node_modules/$bin/package.json" 2>/dev/null || true)"
    elif command -v "$path" >/dev/null 2>&1; then
        ver="$("$path" --version 2>/dev/null | grep -Eo '[0-9]+\.[0-9]+(\.[0-9]+)?' | head -n 1 || true)"
    fi
    _GATES_VER_KEYS+=("$bin")
    _GATES_VER_VALS+=("$ver")
    printf '%s\n' "$ver"
}

# Pin sources for a tool, in precedence order:
#   1. package-lock.json (lockfileVersion >= 2 carries .packages) -- the
#      shared source of truth for the node-resolved linters, and only for
#      them (_GATES_NODE_LINTERS, the binaries formatter-dispatch.sh
#      resolves from node_modules). The npm "shellcheck" package is a
#      downloader wrapper whose version is no shellcheck release, so it
#      pins nothing;
#   2. mise (#260): the [tools] entry of the first project config that has
#      the tool, among the ones mise documents, highest precedence first:
#      .mise.toml, mise.toml, mise/config.toml, .mise/config.toml,
#      .config/mise.toml, .config/mise/mise.toml, .config/mise/config.toml.
#      The version locked in the mise.lock beside that config wins; without
#      one, the config's own version counts when it is exact (x.y.z): a
#      request such as "latest" or "22" pins nothing by itself. Local and
#      environment configs (mise.local.toml, mise.<env>.toml) and conf.d
#      fragments are left out: they are uncommitted or selected per run, so
#      another boundary cannot be held to them;
#   3. .tool-versions (asdf format: "<tool> <version>" per line, # comments
#      ignored) -- for tools npm cannot pin, e.g. shellcheck, whose findings
#      differ across releases and so must be identical at every boundary.
_GATES_NODE_LINTERS=" prettier markdownlint-cli2 "
_GATES_MISE_CONFIGS=".mise.toml mise.toml mise/config.toml .mise/config.toml .config/mise.toml .config/mise/mise.toml .config/mise/config.toml"

# Minimal TOML reader for the mise files. Prints "pin<TAB><line><TAB><version>"
# for each declaration of <tool>, or one "err<TAB><line><TAB><message>" for
# the first line it cannot read. It reads [tools] entries (a string, an
# inline table's version, an array's first element) and [tools.<key>] or
# [[tools.<key>]] tables' version (mise.toml tool tables; mise.lock
# entries). A key names the tool by its last segment, so
# "aqua:koalaman/shellcheck" and "npm:prettier" match. Strings are lexed
# first, so # and brackets inside them change nothing; multi-line strings
# and arrays of other keys are skipped. POSIX awk: macOS awk, mawk, gawk.
_gates_mise_read() { # <file> <tool>
    awk -v tool="$2" '
        function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t\r]+$/, "", s); return s }
        function norm(k,   i) {
            sub(/\[.*$/, "", k)
            i = index(k, ":"); if (i) k = substr(k, i + 1)
            while ((i = index(k, "/")) > 0) k = substr(k, i + 1)
            return k
        }
        function nsep(s,   n, i) {
            n = 0
            for (i = 1; i <= length(s); i++) if (substr(s, i, 1) == SEP) n++
            return n
        }
        function depthof(s,   n, i, c) {
            n = 0
            for (i = 1; i <= length(s); i++) {
                c = substr(s, i, 1)
                if (c == "[" || c == "{") n++
                else if (c == "]" || c == "}") n--
            }
            return n
        }
        # One line: code is the line with each string replaced by SEP and
        # the comment dropped, sv[1..ns] the string values.
        function lex(s,   n, i, j, c, d, q, rest, val, closed) {
            code = ""; ns = 0; lexerr = ""
            n = length(s); i = 1
            if (mlq != "") {
                j = index(s, mlq)
                if (!j) return
                i = j + 3; mlq = ""
            }
            while (i <= n) {
                c = substr(s, i, 1)
                if (c == "#") break
                if (c == "\"" || c == "\047") {
                    q = c c c
                    if (substr(s, i, 3) == q) {
                        rest = substr(s, i + 3); j = index(rest, q)
                        ns++; code = code SEP
                        if (!j) { sv[ns] = ""; mlq = q; mlstart = NR; return }
                        sv[ns] = substr(rest, 1, j - 1); i = i + j + 5
                        continue
                    }
                    val = ""; closed = 0
                    for (j = i + 1; j <= n; j++) {
                        d = substr(s, j, 1)
                        if (c == "\"" && d == "\\") { val = val substr(s, j + 1, 1); j++; continue }
                        if (d == c) { closed = 1; break }
                        val = val d
                    }
                    if (!closed) { lexerr = "unterminated string"; return }
                    ns++; sv[ns] = val; code = code SEP; i = j + 1
                    continue
                }
                code = code c; i++
            }
        }
        # Text of a code fragment, its strings taken from sv[first] on.
        function expand(s, first,   out, i, c, k) {
            out = ""; k = first
            for (i = 1; i <= length(s); i++) {
                c = substr(s, i, 1)
                if (c == SEP) { out = out sv[k]; k++ } else out = out c
            }
            return out
        }
        # The version in an inline-table fragment whose strings follow
        # sv[base]; "" when it names none.
        function tablever(v, base) {
            if (!match(v, VERRE)) return ""
            return sv[base + nsep(substr(v, 1, RSTART + RLENGTH - 1))]
        }
        function fail(line, msg) { printf "err\t%d\t%s\n", line, msg; failed = 1; exit }
        function pin(v) {
            if (v == "") fail(NR, tool ": empty version")
            printf "pin\t%d\t%s\n", NR, v
        }
        BEGIN {
            SEP = "\034"; VERRE = "[{,][ \t]*version[ \t]*=[ \t]*" SEP
            section = ""; depth = 0; mlq = ""; failed = 0
        }
        {
            wasml = (mlq != "")
            lex($0)
            if (lexerr != "") fail(NR, lexerr)
            t = trim(code)
            if (wasml || depth > 0) {
                depth += depthof(t)
                if (depth < 0) fail(NR, "unbalanced ] or }")
                next
            }
            if (t == "") next
            if (substr(t, 1, 1) == "[") {
                if (substr(t, 1, 2) == "[[") {
                    if (length(t) < 5 || substr(t, length(t) - 1) != "]]") fail(NR, "malformed table header")
                    inner = substr(t, 3, length(t) - 4)
                } else {
                    if (length(t) < 3 || substr(t, length(t)) != "]") fail(NR, "malformed table header")
                    inner = substr(t, 2, length(t) - 2)
                }
                gsub(/[ \t]*\.[ \t]*/, ".", inner)
                nseg = split(trim(inner), seg, ".")
                section = ""; tkey = ""
                if (seg[1] == "tools" && nseg == 1) section = "tools"
                else if (seg[1] == "tools" && nseg == 2) { section = "entry"; tkey = expand(seg[2], 1) }
                next
            }
            eq = index(t, "=")
            if (!eq) fail(NR, "expected key = value")
            key = trim(substr(t, 1, eq - 1)); val = trim(substr(t, eq + 1))
            if (key == "" || val == "") fail(NR, "expected key = value")
            depth = depthof(val); depthstart = NR
            if (depth < 0) fail(NR, "unbalanced ] or }")
            kn = nsep(key)
            if (section == "tools") {
                if (index(key, ".") || norm(expand(key, 1)) != tool) next
                if (depth > 0 || mlq != "") fail(NR, tool ": multi-line value not supported")
                if (val == SEP) { pin(sv[kn + 1]); next }
                if (substr(val, 1, 1) == "{") {
                    v = tablever(val, kn)
                    if (v == "") fail(NR, tool ": inline table names no version")
                    pin(v); next
                }
                if (substr(val, 1, 1) == "[") {
                    first = trim(substr(val, 2))
                    if (substr(first, 1, 1) == SEP) { pin(sv[kn + 1]); next }
                    if (substr(first, 1, 1) == "{") {
                        v = tablever(substr(first, 1, index(first, "}")), kn)
                        if (v != "") { pin(v); next }
                    }
                    fail(NR, tool ": array names no version first")
                }
                fail(NR, tool ": version is not a string")
            }
            if (section == "entry" && norm(tkey) == tool && key == "version") {
                if (val != SEP) fail(NR, tool ": version is not a string")
                pin(sv[kn + 1])
            }
        }
        END {
            if (failed) exit
            if (mlq != "") printf "err\t%d\t%s\n", mlstart, "unterminated multi-line string"
            else if (depth > 0) printf "err\t%d\t%s\n", depthstart, "unterminated array or inline table"
        }
    ' "$1" 2>/dev/null || printf 'err\t0\tcannot be read\n'
}

# The pin one mise file gives <tool>: "pin<TAB><version>", or
# "err<TAB><file>:<line>: <message>" (also for a tool declared twice at two
# versions), or nothing.
_gates_mise_pick() { # <root> <rel> <tool>
    local out kind line v ver="" at=""
    out="$(_gates_mise_read "$1/$2" "$3")"
    while IFS="$(printf '\t')" read -r kind line v; do
        if [[ "$kind" == "err" ]]; then
            printf 'err\t%s:%s: %s\n' "$2" "$line" "$v"
            return 0
        fi
        if [[ "$kind" == "pin" && -z "$ver" ]]; then
            ver="$v"; at="$line"
        elif [[ "$kind" == "pin" && "$v" != "$ver" ]]; then
            printf 'err\t%s:%s: %s is declared twice, at %s (line %s) and %s\n' \
                "$2" "$line" "$3" "$ver" "$at" "$v"
            return 0
        fi
    done <<<"$out"
    [[ -n "$ver" ]] && printf 'pin\t%s\n' "$ver"
    return 0
}

# Every pin of <tool>: one "<source><TAB><version>" line per source that
# declares it, in precedence order, plus one "error<TAB><text>" line per
# mise file that cannot be read. Every mise file present is read, whether
# or not an earlier one has the tool, so a broken file never hides.
_gates_pin_sources() { # <pkgname> <root>
    local pkg="$1" root="$2" ver rel dir lock cfg lk tab found=""
    tab="$(printf '\t')"
    if [[ "$_GATES_NODE_LINTERS" == *" $pkg "* && -f "$root/package-lock.json" ]]; then
        ver="$(jq -r --arg p "node_modules/$pkg" '.packages[$p].version // empty' \
            "$root/package-lock.json" 2>/dev/null || true)"
        [[ -n "$ver" ]] && printf 'package-lock.json\t%s\n' "$ver"
    fi
    for rel in $_GATES_MISE_CONFIGS; do
        [[ -f "$root/$rel" ]] || continue
        dir="$(dirname "$rel")"
        lock="mise.lock"
        [[ "$dir" != "." ]] && lock="$dir/mise.lock"
        cfg="$(_gates_mise_pick "$root" "$rel" "$pkg")"
        lk=""
        [[ -f "$root/$lock" ]] && lk="$(_gates_mise_pick "$root" "$lock" "$pkg")"
        [[ "$cfg" == err"$tab"* ]] && printf 'error\t%s\n' "${cfg#err"$tab"}"
        [[ "$lk" == err"$tab"* ]] && printf 'error\t%s\n' "${lk#err"$tab"}"
        [[ -n "$found" || "$cfg" != pin"$tab"* ]] && continue
        found=1
        if [[ "$lk" == pin"$tab"* ]]; then
            printf '%s\t%s\n' "$lock" "${lk#pin"$tab"}"
        elif [[ "${cfg#pin"$tab"}" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+ ]]; then
            printf '%s\t%s\n' "$rel" "${cfg#pin"$tab"}"
        fi
    done
    if [[ -f "$root/.tool-versions" ]]; then
        ver="$(awk -v t="$pkg" '
            { sub(/#.*/, "") }
            $1 == t && $2 != "" { print $2; exit }
        ' "$root/.tool-versions" 2>/dev/null || true)"
        [[ -n "$ver" ]] && printf '.tool-versions\t%s\n' "$ver"
    fi
    return 0
}

# The pin: the first source's version. Prints nothing when no source
# declares the tool -- it is then attested with pinned: null and exempt
# from parity (spec Assumptions), which keeps repos that pin nothing
# working exactly as before.
gates_pin_version() { # <pkgname> <root>
    local pkg="${1:-}" root="${2:-}" src v
    [[ -n "$pkg" ]] || return 0
    while IFS="$(printf '\t')" read -r src v; do
        if [[ -n "$src" && "$src" != "error" ]]; then
            printf '%s' "$v"
            return 0
        fi
    done <<<"$(_gates_pin_sources "$pkg" "$root")"
    return 0
}

# What keeps a pin from being trusted, one problem per line (nothing when
# nothing does): each mise file that cannot be read, as file:line, and
# sources that pin the tool at different versions, all of them named --
# the run does not pick one (#260). Two configs sharing one mise.lock, or
# two tools, repeat a file's error; callers print each line once.
gates_pin_problems() { # <pkgname> <root>
    local pkg="${1:-}" root="${2:-}" src v pins="" first="" differ=""
    [[ -n "$pkg" ]] || return 0
    while IFS="$(printf '\t')" read -r src v; do
        [[ -n "$src" ]] || continue
        if [[ "$src" == "error" ]]; then
            printf '%s\n' "$v"
            continue
        fi
        pins="${pins:+$pins, }$src pins $v"
        if [[ -z "$first" ]]; then
            first="$v"
        elif [[ "$v" != "$first" ]]; then
            differ=1
        fi
    done <<<"$(_gates_pin_sources "$pkg" "$root")"
    [[ -n "$differ" ]] && printf '%s\n' "$pkg: conflicting pins: $pins (pin one version)"
    return 0
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
            + (if .name == "shellcheck" then "(install the pinned version: bash .specify/gates/install-shellcheck.sh)"
               else "(run npm ci)" end) ]
        | join("; ")' 2>/dev/null || true
}

# Gate-set comparison for the synthetic parity gate (feature 006, research
# R7): <declared-json> is the policy's project gates as
# [{"name": ..., "skip_at": [...]}] in declared order, <gates-json> this
# run's GateEntry objects. A declared gate counts as covered when its entry
# ran (pass, fail, warn), or, at agent or git only, when its entry is
# skipped with declared_skip naming <boundary> AND its skip_at names it
# too. Prints "declared gates not run at <boundary>: <names>" naming every
# other declared gate, or nothing. At ci (and none) that means every
# declared gate ran: CI runs the full policy (constitution, Principle III).
gates_declared_not_run() { # <boundary> <declared-json> <gates-json>
    jq -rn --arg b "${1:-}" --argjson d "${2:-[]}" --argjson g "${3:-[]}" '
        [ $d[]
          | . as $decl
          | ([ $g[] | select(.name == $decl.name) ] | first) as $e
          | select(
                ($e != null and ($e.result | IN("pass", "fail", "warn")))
                or ($e != null and ($b | IN("agent", "git"))
                    and $e.result == "skipped" and $e.declared_skip == $b
                    and (($decl.skip_at // []) | index($b)) != null)
              | not)
          | $decl.name ]
        | if length == 0 then empty
          else "declared gates not run at \($b): \(join(", "))" end' 2>/dev/null \
        || printf 'declared gates not run at %s: the gate set could not be compared\n' "${1:-}"
}

# Keep the attestation log out of version control (#69): it is disposable
# run evidence, documented as gitignored. Adds the entry to the .gitignore
# next to the log, creating the file when absent and never rewriting other
# lines. Best effort: a failure here never affects the gate outcome.
gates_attest_ignore() { # <log-path>
    local log="${1:-}" dir name ignore
    [[ -n "$log" ]] || return 0
    dir="$(dirname "$log")"
    name="$(basename "$log")"
    ignore="$dir/.gitignore"
    if [[ -f "$ignore" ]] && grep -qxF -e "$name" -e "/$name" "$ignore" 2>/dev/null; then
        return 0
    fi
    local sep=""
    [[ -s "$ignore" && -n "$(tail -c 1 "$ignore" 2>/dev/null)" ]] && sep=$'\n'
    { printf '%s%s\n' "$sep" "$name" >>"$ignore"; } 2>/dev/null || true
    return 0
}

# Append one single-line record, then cap: records are well under PIPE_BUF so
# the append is a single atomic-in-practice write; the cap rewrite goes
# through a temp file in the same directory + mv (atomic rename), so readers
# never observe a truncated file. Concurrent capping is last-writer-wins.
# A directory or log that cannot be written returns 1 without output (#122):
# the caller prints the one warning; bash's own "Permission denied" for a
# failed redirection would otherwise leak. Redirection errors go to the
# shell's stderr, so the group, not the command, is silenced.
gates_attest_append() { # <record-json> <log-path> <max-records>
    local record="${1:-}" log="${2:-}" max="${3:-200}"
    [[ -n "$record" && -n "$log" ]] || return 1
    local dir
    dir="$(dirname "$log")"
    if [[ ! -d "$dir" ]]; then
        mkdir -p "$dir" 2>/dev/null || return 1
    fi
    { printf '%s\n' "$record" >>"$log"; } 2>/dev/null || return 1
    gates_attest_ignore "$log"
    local lines
    lines="$(wc -l <"$log" | tr -d '[:space:]')" || return 1
    if [[ "$lines" -gt "$max" ]]; then
        local tmp="$dir/.attestations.jsonl.$$"
        if ! { tail -n "$max" "$log" >"$tmp"; } 2>/dev/null; then
            rm -f "$tmp" 2>/dev/null
            return 1
        fi
        if ! mv "$tmp" "$log" 2>/dev/null; then
            rm -f "$tmp" 2>/dev/null
            return 1
        fi
    fi
    return 0
}
