#!/usr/bin/env bash
# constitution.sh (lib) -- constitution as an enforceable contract (feature 004).
#
# The pure parsing/detection core. No side effects, no network: every
# function here reads files and writes stdout only. Sourced by the projected
# constitution.sh entry script and by doctor.sh. Surface-activity evaluation
# (align/check) is layered on top in the same file (see the SURFACE section).
#
# Provides (foundational core, T002):
#   gates_const_fm_field <frag> <key>      # one frontmatter value (double-quotes stripped)
#   gates_const_fm_tags <frag>             # tags array, one per line
#   gates_const_fm_body <frag>             # markdown body after the frontmatter
#   gates_const_manifest_tier <man> <tier> # fragment ids in a manifest tier
#   gates_const_parse <constitution>       # PRINCIPLE / MALFORMED TSV protocol
#   gates_const_detect <constitution> [tpl]# absent | placeholder | filled
#
# All bash 3.2 + jq + BSD awk/sed. No interval regexes, no gensub, no
# associative arrays -- the macOS base toolchain is the floor.

# shellcheck disable=SC2034   # library file; helpers consumed by callers

# The fixed v1 surface set (contracts/annotation-format.md).
GATES_CONST_SURFACES="policy agent-hook git-hook ci accept scanner prose"

# Is <surface> one of the v1 set?
gates_const_is_surface() { # <surface>
    case " $GATES_CONST_SURFACES " in
        *" $1 "*) return 0 ;;
    esac
    return 1
}

# --- Fragment frontmatter (contracts/annotation-format.md) --------------------

# Value of a single frontmatter key from the leading `---`..`---` block. One
# layer of surrounding double quotes is stripped (the corpus quotes strings).
# Empty when the file has no frontmatter or the key is absent.
gates_const_fm_field() { # <fragment> <key>
    local file="${1:-}" key="${2:-}"
    [[ -f "$file" && -n "$key" ]] || return 0
    awk -v key="$key" '
        NR == 1 && $0 != "---" { exit }
        NR == 1 { infm = 1; next }
        infm && $0 == "---" { exit }
        infm {
            idx = index($0, ":")
            if (idx == 0) next
            k = substr($0, 1, idx - 1)
            gsub(/^[ \t]+|[ \t]+$/, "", k)
            if (k != key) next
            v = substr($0, idx + 1)
            gsub(/^[ \t]+|[ \t]+$/, "", v)
            if (v ~ /^".*"$/) v = substr(v, 2, length(v) - 2)
            print v
            exit
        }
    ' "$file"
}

# The `tags: [a, b, c]` frontmatter array, one tag per line.
gates_const_fm_tags() { # <fragment>
    local raw
    raw="$(gates_const_fm_field "${1:-}" tags)"
    [[ -z "$raw" ]] && return 0
    raw="${raw#\[}"
    raw="${raw%\]}"
    printf '%s\n' "$raw" | tr ',' '\n' \
        | sed 's/^[[:space:]]*//; s/[[:space:]]*$//' | grep -v '^$' || true
}

# The markdown body: everything after the closing frontmatter `---`, with a
# single leading blank line trimmed so drafts assemble deterministically.
gates_const_fm_body() { # <fragment>
    local file="${1:-}"
    [[ -f "$file" ]] || return 0
    awk '
        NR == 1 && $0 != "---" { print; body = 1; next }
        NR == 1 { infm = 1; next }
        infm && $0 == "---" { infm = 0; body = 1; started = 0; next }
        infm { next }
        body {
            if (!started && $0 ~ /^[[:space:]]*$/) next
            started = 1
            print
        }
    ' "$file"
}

# --- Manifest (corpus registry) ----------------------------------------------

# Fragment ids listed under a manifest tier. tier is mandatory|recommended|
# optional; the manifest keys are `<tier>_fragments:` (charter-compatible).
gates_const_manifest_tier() { # <manifest> <tier>
    local manifest="${1:-}" tier="${2:-}"
    [[ -f "$manifest" && -n "$tier" ]] || return 0
    awk -v want="${tier}_fragments" '
        /^[a-z_]+:/ {
            key = $0
            sub(/:.*/, "", key)
            cur = (key == want) ? 1 : 0
            next
        }
        cur && /^[ \t]*-[ \t]+/ {
            v = $0
            sub(/^[ \t]*-[ \t]+/, "", v)
            gsub(/^[ \t]+|[ \t]+$/, "", v)
            if (v ~ /^".*"$/) v = substr(v, 2, length(v) - 2)
            print v
        }
    ' "$manifest"
}

# --- Annotation parser (contracts/annotation-format.md) ----------------------

# Parse a constitution into principles and their enforcement markers. A
# principle is an `### ` (h3) heading inside the `## Core Principles` section
# (#82): sub-headings under other sections (Additional Constraints,
# Governance, ...) are prose, not principles. Markers bind to the principle
# they fall under (up to the next h2/h3). Emits TAB-separated protocol lines:
#
#   PRINCIPLE<TAB><heading-line><TAB><name><TAB><surface><TAB><ref><TAB><expect>
#       an empty <surface> means the principle is unannotated (legal, FR-013).
#   MALFORMED<TAB><line><TAB><name-or-dash><TAB><message>
#       a fail-closed marker error; callers name constitution.md:<line>.
#   NOCORE
#       the file has no `## Core Principles` section, so no principles.
#
# Marker grammar rules (annotation-format.md): exactly one `gates:enforce`
# per principle (a second is MALFORMED); `surface=` required from the fixed
# set; `ref=` required unless `surface=prose`; unknown keys or `=`-less tokens
# are MALFORMED; a marker before any principle is MALFORMED with name "-";
# a marker under a heading outside Core Principles is MALFORMED (an
# enforcement claim that is never checked is the silent no-op this gate
# exists to catch).
# HTML comments without `gates:enforce` (e.g. the Sync Impact Report) are
# ignored (grammar rule 1).
gates_const_parse() { # <constitution>
    local file="${1:-}"
    [[ -f "$file" ]] || return 0
    awk '
    function is_surface(s) {
        return (s == "policy" || s == "agent-hook" || s == "git-hook" \
            || s == "ci" || s == "accept" || s == "scanner" || s == "prose")
    }
    function flush() {
        if (pline > 0) {
            printf "PRINCIPLE\t%d\t%s\t%s\t%s\t%s\n", \
                pline, pname, psurface, pref, pexpect
        }
    }
    BEGIN { pline = 0; pname = ""; psurface = ""; pref = ""; pexpect = ""; have = 0
            incore = 0; sawcore = 0; outname = ""; fch = ""; flen = 0 }
    # Fenced code blocks (``` or ~~~, up to three spaces of indent) are
    # example content: a heading or marker inside one is not a principle,
    # like the spec gate parser treats fences in tasks.md. A fence closes on
    # the same character, at least as long, with no info string.
    /^ ? ? ?(```|~~~)/ {
        f = $0; sub(/^ */, "", f)
        c = substr(f, 1, 1); n = 0
        while (substr(f, n + 1, 1) == c) n++
        rest = substr(f, n + 1); gsub(/^[ \t]+|[ \t]+$/, "", rest)
        if (fch == "") {
            # A backtick fence info string may not contain a backtick.
            if (!(c == "`" && index(rest, "`") > 0)) { fch = c; flen = n; next }
        } else if (c == fch && n >= flen && rest == "") {
            fch = ""; flen = 0; next
        }
    }
    fch != "" { next }
    /^###[ \t]/ {
        flush()
        h = $0; sub(/^###[ \t]+/, "", h); gsub(/[ \t]+$/, "", h)
        psurface = ""; pref = ""; pexpect = ""; have = 0
        if (incore) { pline = NR; pname = h; outname = "" }
        else { pline = 0; pname = ""; outname = h }
        next
    }
    /^##[ \t]/ {
        flush()
        pline = 0; pname = ""; psurface = ""; pref = ""; pexpect = ""; have = 0; outname = ""
        incore = (tolower($0) ~ /^##[ \t]+core principles/) ? 1 : 0
        if (incore) sawcore = 1
        next
    }
    (index($0, "gates:enforce") > 0 && index($0, "<!--") > 0) {
        m = $0
        sub(/^[^<]*<!--[ \t]*/, "", m)
        sub(/[ \t]*-->.*$/, "", m)
        gsub(/^[ \t]+|[ \t]+$/, "", m)
        n = split(m, tok, /[ \t]+/)
        if (tok[1] != "gates:enforce") next
        name = (pline > 0) ? pname : "-"
        if (have) {
            printf "MALFORMED\t%d\t%s\tsecond gates:enforce marker for one principle (at most one per principle)\n", NR, name
            next
        }
        have = 1
        if (pline == 0 && outname != "") {
            printf "MALFORMED\t%d\t%s\tgates:enforce marker outside Core Principles (move the principle under ## Core Principles)\n", NR, outname
            next
        }
        if (pline == 0) {
            printf "MALFORMED\t%d\t%s\tgates:enforce marker before any principle heading\n", NR, name
            next
        }
        surface = ""; ref = ""; expect = ""; bad = ""
        for (i = 2; i <= n; i++) {
            kv = tok[i]
            eq = index(kv, "=")
            if (eq == 0) { bad = "unparseable token: " kv; break }
            k = substr(kv, 1, eq - 1); val = substr(kv, eq + 1)
            if (k == "surface") surface = val
            else if (k == "ref") ref = val
            else if (k == "expect") expect = val
            else { bad = "unknown key: " k; break }
        }
        if (bad != "") { printf "MALFORMED\t%d\t%s\t%s\n", NR, name, bad; next }
        if (surface == "") { printf "MALFORMED\t%d\t%s\tmissing surface=\n", NR, name; next }
        if (!is_surface(surface)) { printf "MALFORMED\t%d\t%s\tunknown surface: %s\n", NR, name, surface; next }
        if (surface != "prose" && ref == "") { printf "MALFORMED\t%d\t%s\tmissing ref= for surface=%s\n", NR, name, surface; next }
        psurface = surface; pref = ref; pexpect = expect
        next
    }
    END { flush(); if (!sawcore) print "NOCORE" }
    ' "$file"
}

# --- Placeholder detection (R8, FR-014) --------------------------------------

# Classify a constitution for init and the session's mode choice:
#   absent       -- the file does not exist
#   placeholder  -- byte-equal to the shipped template, or carries a
#                   bracket-token signature (`[UPPER_SNAKE]`, 3+ chars)
#   filled       -- concrete content
gates_const_detect() { # <constitution> [<template>]
    local file="${1:-}" template="${2:-}"
    [[ -f "$file" ]] || {
        echo absent
        return 0
    }
    if [[ -n "$template" && -f "$template" ]] && cmp -s "$file" "$template"; then
        echo placeholder
        return 0
    fi
    if grep -Eq '\[[A-Z_][A-Z_][A-Z_]' "$file"; then
        echo placeholder
        return 0
    fi
    echo filled
}

# --- Draft assembly helpers (US1) --------------------------------------------

# Roman numeral for 1..39 (constitutions never have more principles).
gates_const_roman() { # <n>
    local n="${1:-0}" out="" i
    local rvals="10 9 5 4 1" rsyms="X IX V IV I"
    # shellcheck disable=SC2206  # deliberate word-split of fixed literals
    local -a rv=($rvals) rs=($rsyms)
    for i in 0 1 2 3 4; do
        while [[ "$n" -ge "${rv[$i]}" ]]; do
            out="$out${rs[$i]}"
            n=$((n - ${rv[$i]}))
        done
    done
    printf '%s' "$out"
}

# Title-case a hyphenated id segment: "no-secrets" -> "No Secrets".
gates_const_titlecase() { # <hyphenated>
    printf '%s' "${1:-}" | tr '-' ' ' \
        | awk '{ for (i = 1; i <= NF; i++) $i = toupper(substr($i, 1, 1)) substr($i, 2); print }'
}

# Build one enforcement marker from a surface decision.
gates_const_marker() { # <surface> <ref> <expect>
    local s="${1:-}" r="${2:-}" e="${3:-}"
    if [[ "$s" == "prose" ]]; then
        printf '<!-- gates:enforce surface=prose -->'
        return 0
    fi
    local m="<!-- gates:enforce surface=$s ref=$r"
    [[ -n "$e" ]] && m="$m expect=$e"
    printf '%s -->' "$m"
}

# --- fragments (candidate menu, US1) -----------------------------------------

# Filtered/ranked candidate menu for the interview profile. Emits TSV per
# contracts/cli-contracts.md: tier<TAB>id<TAB>statement<TAB>surface<TAB>ref<TAB>rationale.
# Mandatory tier first; within a tier, ranked by profile relevance (exact
# project-type match, then matched postures), ties in manifest order. A
# fragment carrying project-type tags that do not include the profile's type,
# and not tagged all-projects, is filtered out (a docs project never sees
# infra fragments); mandatory fragments are never filtered. Missing profile or
# corpus is a usage error (exit 1); a malformed manifest/fragment is exit 2.
gates_const_fragments() { # <corpus> <profile-json>
    local corpus="${1:-}" profile="${2:-}"
    [[ -d "$corpus" ]] || {
        echo "constitution: corpus directory not found: $corpus" >&2
        return 1
    }
    [[ -f "$profile" ]] || {
        echo "constitution: an interview profile is required (--profile); the menu is never unfiltered" >&2
        return 1
    }
    local manifest="$corpus/manifest.yml"
    [[ -f "$manifest" ]] || {
        echo "constitution: corpus has no manifest.yml: $corpus" >&2
        return 2
    }
    local ptype postures
    ptype="$(jq -r '.project_type // ""' "$profile" 2>/dev/null)" || {
        echo "constitution: profile is not valid JSON: $profile" >&2
        return 2
    }
    postures="$(jq -r '(.postures // [])[]' "$profile" 2>/dev/null)"
    local tab
    tab="$(printf '\t')"
    local tier
    for tier in mandatory recommended optional; do
        local tmp
        tmp="$(mktemp 2>/dev/null || mktemp -t gates-frag)" || return 1
        local id
        while IFS= read -r id; do
            [[ -z "$id" ]] && continue
            local frag="$corpus/fragments/$id.md"
            [[ -f "$frag" ]] || {
                echo "constitution: manifest lists $id but $frag is missing" >&2
                rm -f "$tmp"
                return 2
            }
            local tags score=0 has_ptype=0 all_projects=0 matched=0 t bare
            tags="$(gates_const_fm_tags "$frag")"
            while IFS= read -r t; do
                [[ -z "$t" ]] && continue
                case "$t" in
                    all-projects) all_projects=1 ;;
                    project-type/*)
                        has_ptype=1
                        bare="${t#project-type/}"
                        if [[ "$bare" == "$ptype" ]]; then
                            matched=1
                            score=$((score + 2))
                        fi
                        ;;
                    posture/*)
                        bare="${t#posture/}"
                        if grep -qx "$bare" <<<"$postures"; then
                            score=$((score + 1))
                        fi
                        ;;
                esac
            done <<<"$tags"
            if [[ "$tier" != "mandatory" && "$has_ptype" == "1" \
                && "$all_projects" == "0" && "$matched" == "0" ]]; then
                continue
            fi
            local statement rationale surface ref
            statement="$(gates_const_fm_field "$frag" statement)"
            rationale="$(gates_const_fm_field "$frag" rationale)"
            surface="$(gates_const_fm_field "$frag" surface)"
            ref="$(gates_const_fm_field "$frag" ref)"
            printf '%d\t%s\t%s\t%s\t%s\t%s\t%s\n' \
                "$score" "$tier" "$id" "$statement" "$surface" "$ref" "$rationale" >>"$tmp"
        done < <(gates_const_manifest_tier "$manifest" "$tier")
        # Stable sort by score descending; drop the score column.
        sort -t"$tab" -k1,1nr -s "$tmp" | cut -f2-
        rm -f "$tmp"
    done
    return 0
}

# --- draft (assemble the annotated constitution, US1) ------------------------

# Resolve one selection (index $2 of the selections in $1) into NAME / BODY /
# MARKER / PRINCIPLE globals, validating the surface decision (FR-004). Returns
# 2 with a named cause on any contract failure.
_gates_const_resolve_sel() { # <selections-json> <index> <corpus>
    local sel="$1" i="$2" corpus="$3"
    local id surface ref expect
    id="$(jq -r ".selections[$i].id // \"\"" "$sel")"
    SEL_NAME="$(jq -r ".selections[$i].name // \"\"" "$sel")"
    surface="$(jq -r ".selections[$i].surface // \"\"" "$sel")"
    ref="$(jq -r ".selections[$i].ref // \"\"" "$sel")"
    expect="$(jq -r ".selections[$i].expect // \"\"" "$sel")"
    SEL_PRINCIPLE="$(jq -r ".selections[$i].principle // \"\"" "$sel")"
    SEL_BODY="$(jq -r ".selections[$i].body // \"\"" "$sel")"
    if [[ -z "$surface" ]]; then
        echo "constitution: selection $i carries no surface decision -- every principle must declare one (prose is an explicit choice)" >&2
        return 2
    fi
    if ! gates_const_is_surface "$surface"; then
        echo "constitution: selection $i has unknown surface '$surface'" >&2
        return 2
    fi
    if [[ "$surface" != "prose" && -z "$ref" ]]; then
        echo "constitution: selection $i (surface=$surface) needs a ref" >&2
        return 2
    fi
    if [[ -n "$id" ]]; then
        local frag="$corpus/fragments/$id.md"
        [[ -f "$frag" ]] || {
            echo "constitution: selection $i references unknown fragment '$id'" >&2
            return 2
        }
        [[ -z "$SEL_BODY" ]] && SEL_BODY="$(gates_const_fm_body "$frag")"
        [[ -z "$SEL_NAME" ]] && SEL_NAME="$(gates_const_titlecase "${id##*/}")"
    fi
    # An in-place annotation (augment) needs no body/name of its own.
    if [[ -z "$SEL_PRINCIPLE" ]]; then
        [[ -z "$SEL_NAME" ]] && {
            echo "constitution: selection $i (custom) needs a name" >&2
            return 2
        }
        [[ -z "$SEL_BODY" ]] && {
            echo "constitution: selection $i (custom) needs a body" >&2
            return 2
        }
    fi
    SEL_MARKER="$(gates_const_marker "$surface" "$ref" "$expect")"
    return 0
}

# Assemble the annotated constitution draft (contracts/cli-contracts.md).
# Deterministic: identical inputs produce byte-identical output. Writes only to
# <out>. In --augment mode existing content is preserved verbatim: selections
# with a `principle` field annotate the named existing heading in place (only
# if it is not already annotated), and selections without one are appended as
# new principles in Core Principles order.
gates_const_draft() { # <corpus> <selections-json> <out> [<augment-file>]
    local corpus="${1:-}" sel="${2:-}" out="${3:-}" augment="${4:-}"
    [[ -f "$sel" ]] || {
        echo "constitution: selections file not found: $sel" >&2
        return 1
    }
    [[ -n "$out" ]] || {
        echo "constitution: draft needs an --out path" >&2
        return 1
    }
    local n
    n="$(jq '.selections | length' "$sel" 2>/dev/null)" || {
        echo "constitution: selections is not valid JSON: $sel" >&2
        return 2
    }
    [[ "$n" -ge 1 ]] || {
        echo "constitution: no selections to draft" >&2
        return 2
    }
    local project
    project="$(jq -r '.project_name // "Project"' "$sel")"

    # Resolve every selection up front (fail closed before writing anything).
    local -a names=() bodies=() markers=() principles=()
    local i SEL_NAME SEL_BODY SEL_MARKER SEL_PRINCIPLE
    for ((i = 0; i < n; i++)); do
        _gates_const_resolve_sel "$sel" "$i" "$corpus" || return 2
        names+=("$SEL_NAME")
        bodies+=("$SEL_BODY")
        markers+=("$SEL_MARKER")
        principles+=("$SEL_PRINCIPLE")
    done

    local gov="This constitution supersedes other practice documents where they conflict.
Amendments are made by pull request that edits this file, records the change,
and is approved by the project's maintainers. Every review verifies compliance
with the principles above; the enforcement annotations bind each principle to
the boundary that proves it."

    if [[ -z "$augment" ]]; then
        # Fresh draft in the core template's section shape.
        {
            printf '# %s Constitution\n\n' "$project"
            printf '## Core Principles\n'
            local idx=0 roman
            for ((i = 0; i < n; i++)); do
                idx=$((idx + 1))
                roman="$(gates_const_roman "$idx")"
                printf '\n### %s. %s\n\n%s\n\n%s\n' \
                    "$roman" "${names[$i]}" "${markers[$i]}" "${bodies[$i]}"
            done
            printf '\n## Governance\n\n%s\n\n' "$gov"
            printf '**Version**: 0.0.0 | **Ratified**: pending | **Last Amended**: pending\n'
        } >"$out"
        return 0
    fi

    # Augment mode: annotate/append onto an existing constitution.
    [[ -f "$augment" ]] || {
        echo "constitution: --augment file not found: $augment" >&2
        return 1
    }
    # Which existing principles already carry a marker? (Do not double-annotate.)
    local annotated_tmp
    annotated_tmp="$(mktemp 2>/dev/null || mktemp -t gates-annot)" || return 1
    gates_const_parse "$augment" \
        | awk -F'\t' '$1 == "PRINCIPLE" && $4 != "" { print $3 }' >"$annotated_tmp"
    local existing_count
    existing_count="$(gates_const_parse "$augment" | grep -c '^PRINCIPLE' || true)"

    local markers_file newblock_file
    markers_file="$(mktemp 2>/dev/null || mktemp -t gates-mk)" || return 1
    newblock_file="$(mktemp 2>/dev/null || mktemp -t gates-nb)" || return 1
    : >"$markers_file"
    : >"$newblock_file"

    local k=0 roman
    for ((i = 0; i < n; i++)); do
        if [[ -n "${principles[$i]}" ]]; then
            # Annotate an existing heading in place, unless already annotated.
            if grep -qxF "${principles[$i]}" "$annotated_tmp"; then
                continue
            fi
            printf '%s\t%s\n' "${principles[$i]}" "${markers[$i]}" >>"$markers_file"
        else
            k=$((k + 1))
            roman="$(gates_const_roman $((existing_count + k)))"
            {
                printf '### %s. %s\n\n%s\n\n%s\n\n' \
                    "$roman" "${names[$i]}" "${markers[$i]}" "${bodies[$i]}"
            } >>"$newblock_file"
        fi
    done

    awk -v markersfile="$markers_file" -v newblockfile="$newblock_file" '
    BEGIN {
        while ((getline l < markersfile) > 0) {
            p = index(l, "\t")
            if (p > 0) mk[substr(l, 1, p - 1)] = substr(l, p + 1)
        }
        nb = ""
        while ((getline l < newblockfile) > 0) nb = nb l "\n"
        incore = 0; flushed = 0
    }
    /^## / {
        if (incore && !flushed) { printf "%s", nb; flushed = 1 }
        incore = ($0 ~ /^## Core Principles/) ? 1 : 0
        print; next
    }
    /^### / {
        print
        h = $0; sub(/^### +/, "", h); sub(/ +$/, "", h)
        if (h in mk) print mk[h]
        next
    }
    { print }
    END { if (incore && !flushed) printf "%s", nb }
    ' "$augment" >"$out"

    rm -f "$annotated_tmp" "$markers_file" "$newblock_file"
    return 0
}

# --- Surface-activity evaluation (R4, US2/US3) -------------------------------
#
# Each evaluator is local-only and side-effect free: it answers "is the claim
# this annotation makes actually wired here?" from committed files, never by
# invoking the surface. Echoes one of active | missing | pending-boundary.
# pending-boundary means the enclosing boundary is not projected at all yet
# (e.g. a ci ref with no CI configuration anywhere) — a gap that is not the
# principle's fault. All read $2 (project root) explicitly.

# The policy path a policy ref names, as a full dotted path (#139). A ref
# whose first segment is a top-level policy section (policy.schema.json
# allows no others) is already one -- hooks.markdownlint.severity,
# git.block_main_commits; any other ref is the <hook>.<key> shorthand for
# hooks.<hook>.<key>.
_gates_const_policy_path() { # <ref>
    case "${1%%.*}" in
        attestation | extends | git | hooks | protected_files | spec) printf '%s' "$1" ;;
        *) printf 'hooks.%s' "$1" ;;
    esac
}

# policy: the ref's full path is present in the ENFORCED policy (effective
# when a 003 contract is live -- the policy loader resolves that). A scalar
# must equal expect when one is given, and an enabled-style key must not be
# false. A list or object (#171) is present when non-empty; expect on a list
# names an element it must contain, on an object a key it must have.
_gates_const_eval_policy() { # <ref> <expect>
    local ref="${1:-}" expect="${2:-}"
    local path file state=""
    path="$(_gates_const_policy_path "$ref")"
    file="$(gates_policy_file 2>/dev/null)"
    [[ -n "$file" && -f "$file" ]] && state="$(jq -r --arg p "$path" --arg e "$expect" '
        (try getpath($p | split(".")) catch null) as $v
        | ($v | type) as $t
        | if $v == null then "missing"
          elif $t == "array" or $t == "object" then
            if ($v | length) == 0 then "missing"
            elif $e == "" then "active"
            elif $t == "array" then (if any($v[]; tostring == $e) then "active" else "missing" end)
            elif ($v | has($e)) then "active"
            else "missing" end
          elif $e != "" then (if ($v | tostring) == $e then "active" else "missing" end)
          elif $v == false then "missing"
          else "active" end
    ' "$file" 2>/dev/null)"
    [[ "$state" == "active" ]] && echo active || echo missing
}

# agent-hook: <script.sh> exists under .claude/hooks/gates/, is executable, and
# is referenced from .claude/settings.json.
_gates_const_eval_agent_hook() { # <ref> <root>
    local ref="${1:-}" root="${2:-}"
    [[ -d "$root/.claude" ]] || {
        echo pending-boundary
        return 0
    }
    local f="$root/.claude/hooks/gates/$ref"
    if [[ ! -f "$f" || ! -x "$f" ]]; then
        echo missing
        return 0
    fi
    if [[ -f "$root/.claude/settings.json" ]] && grep -qF "$ref" "$root/.claude/settings.json" 2>/dev/null; then
        echo active
    else
        echo missing
    fi
}

# git-hook: installed pre-commit|commit-msg exists, is executable, and delegates
# to the gates runtime (the same test doctor's git-boundary section uses).
_gates_const_eval_git_hook() { # <ref> <root>
    local ref="${1:-}" root="${2:-}"
    git -C "$root" rev-parse --git-dir >/dev/null 2>&1 || {
        echo pending-boundary
        return 0
    }
    # --git-path hooks: the directory git actually runs hooks from -- the
    # shared one in a linked worktree (whose --git-dir is
    # .git/worktrees/<name>), and core.hooksPath when set.
    local hookdir
    hookdir="$(git -C "$root" rev-parse --git-path hooks)"
    [[ "$hookdir" != /* ]] && hookdir="$root/$hookdir"
    local hf="$hookdir/$ref"
    if [[ ! -f "$hf" || ! -x "$hf" ]]; then
        echo missing
        return 0
    fi
    # A stub (issue #59) is only active while this branch's hook exists.
    if grep -q 'spec-gates hook stub' "$hf" 2>/dev/null; then
        [[ -f "$root/.specify/gates/hooks/$ref" ]] && echo active || echo missing
        return 0
    fi
    grep -q 'gates\|verify.sh' "$hf" 2>/dev/null && echo active || echo missing
}

# ci: a pipeline runs the gates (a proven `verify.sh --boundary ci` step,
# lib/manifest.sh gates_ci_unproven, #198) and the ref: a template step id
# (gates, canary, pr) must be a live step, any other ref must appear in the
# live text. Live means comments, disabled steps and steps that cannot fail
# removed (gates_ci_live, the reading doctor's drift check uses), so a
# `# TODO: wire gates` line or an `|| true` step enforces nothing (#139,
# #171). No
# pipeline file at all is pending-boundary (the CI boundary is simply not
# projected yet).
_gates_const_eval_ci() { # <ref> <root>
    local ref="${1:-}" root="${2:-}"
    # Without the manifest library nothing can show a live step: fail closed.
    declare -f gates_ci_body >/dev/null 2>&1 || {
        echo missing
        return 0
    }
    if [[ ! -d "$root/.github/workflows" && -z "$(gates_ci_candidates "$root")" ]]; then
        echo pending-boundary
        return 0
    fi
    local body re
    body="$(gates_ci_body "$root")"
    if [[ -z "$body" ]]; then
        echo missing
        return 0
    fi
    re="$(gates_ci_step_re "$ref")"
    if [[ -n "$re" ]]; then
        grep -qE "$re" <<<"$body" && echo active || echo missing
    else
        grep -qF -- "$ref" <<<"$body" && echo active || echo missing
    fi
}

# accept: specs/<feature>/tasks.md carries an accept block whose "# verifies:"
# matches <SC-id> and parses (spec-gate lib). No specs/ dir is pending-boundary.
_gates_const_eval_accept() { # <ref> <root>
    local ref="${1:-}" root="${2:-}"
    local feature="${ref%%/*}" sc="${ref#*/}"
    [[ -d "$root/specs" ]] || {
        echo pending-boundary
        return 0
    }
    local tasks="$root/specs/$feature/tasks.md"
    [[ -f "$tasks" ]] || {
        echo missing
        return 0
    }
    declare -f gates_spec_parse >/dev/null 2>&1 || {
        echo missing
        return 0
    }
    local tmp parse
    tmp="$(mktemp -d 2>/dev/null || mktemp -d -t gates-const-accept)" || {
        echo missing
        return 0
    }
    parse="$(gates_spec_parse "$tasks" "$tmp")"
    rm -rf "$tmp"
    if printf '%s\n' "$parse" \
        | awk -F'\t' -v sc="$sc" '$1 == "BLOCK" && $3 == sc { f = 1 } END { exit !f }'; then
        echo active
    else
        echo missing
    fi
}

# scanner: <tool>:<rule> — the tool's config file exists and mentions the rule.
_gates_const_eval_scanner() { # <ref> <root>
    local ref="${1:-}" root="${2:-}"
    local tool="${ref%%:*}" rule="${ref#*:}"
    local cfgs
    case "$tool" in
        checkov) cfgs=".checkov.yml .checkov.yaml" ;;
        gitleaks) cfgs=".gitleaks.toml gitleaks.toml" ;;
        hadolint) cfgs=".hadolint.yaml .hadolint.yml" ;;
        *) cfgs=".$tool.yml .$tool.yaml .$tool.toml .$tool.json" ;;
    esac
    local c
    for c in $cfgs; do
        if [[ -f "$root/$c" ]] && grep -qF "$rule" "$root/$c" 2>/dev/null; then
            echo active
            return 0
        fi
    done
    echo missing
}

# policy.schema.json beside this library (the projected runtime and the
# extension both keep it one level up).
GATES_CONST_SCHEMA="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." 2>/dev/null && pwd)/policy.schema.json"

# What policy.schema.json allows at a dotted path:
# "<type>|<enum>|<item enum>|<required>|<minimum>|<keys>" (lists
# comma-joined; keys only for an object that allows no others), "unknown" for a path the
# schema has no room for, empty when the schema cannot be read.
_gates_const_schema_at() { # <path>
    [[ -f "$GATES_CONST_SCHEMA" ]] || return 0
    jq -r --arg p "$1" '
        . as $root
        | def deref: if type == "object" and has("$ref")
              then .["$ref"] as $r | $root | getpath($r | ltrimstr("#/") | split("/"))
              else . end;
        reduce ($p | split("."))[] as $k (.;
            if . == null then null
            else deref
                | if ((.properties // {}) | has($k)) then .properties[$k]
                  elif (.additionalProperties | type) == "object" then .additionalProperties
                  else null end
            end)
        | if . == null then "unknown"
          else deref
            | [(.type // "" | tostring), ((.enum // []) | map(tostring) | join(",")),
               ((.items.enum // []) | map(tostring) | join(",")),
               ((.required // []) | join(",")),
               (.minimum // "" | tostring),
               (if .additionalProperties == false then (.properties // {}) | keys | join(",") else "" end)]
            | join("|")
          end
    ' "$GATES_CONST_SCHEMA" 2>/dev/null
}

# Is <value> in the comma-joined <allowed> list? An empty list allows all.
_gates_const_allows() { # <allowed> <value>
    [[ -z "$1" ]] || grep -qxF -- "$2" <<<"$(tr ',' '\n' <<<"$1")"
}

# Is the integer <value> below the schema's non-negative <minimum>? False
# when there is no minimum. A value too long for shell arithmetic is
# positive and far above any minimum the schema sets.
_gates_const_below() { # <value> <minimum>
    [[ "$2" =~ ^[0-9]+$ ]] || return 1
    [[ "$1" == -* ]] && return 0
    [[ "$1" =~ ^[0-9]{1,18}$ ]] && ((10#$1 < 10#$2))
}

# A policy proposal that converges (#171): applying it makes the principle
# active. The schema decides the shape (a list gets an element added, a
# boolean is set to true); an annotation no valid policy can satisfy -- a
# path the schema lacks, an expect outside the allowed values -- is reported
# as the annotation to fix, never as a policy edit.
_gates_const_policy_proposal() { # <path> <expect>
    local path="$1" expect="$2" info type enum ienum req min keys change=""
    info="$(_gates_const_schema_at "$path")"
    if [[ "$info" == unknown ]]; then
        printf 'fix the annotation: policy.schema.json has no %s, so no policy can satisfy it' "$path"
        return 0
    fi
    IFS='|' read -r type enum ienum req min keys <<<"$info"
    case "$type" in
        boolean)
            if [[ -n "$expect" && "$expect" != true && "$expect" != false ]]; then
                printf 'fix the annotation: %s is a boolean, expect=%s can never match' "$path" "$expect"
                return 0
            fi
            change="set $path = ${expect:-true}"
            ;;
        array)
            if [[ -z "$expect" ]]; then
                change="add at least one entry to $path"
            elif _gates_const_allows "$ienum" "$expect"; then
                change="add \"$expect\" to the $path list"
            else
                printf 'fix the annotation: %s entries are one of %s, expect=%s can never match' "$path" "$ienum" "$expect"
                return 0
            fi
            ;;
        object)
            if [[ -n "$expect" ]] && ! _gates_const_allows "$keys" "$expect"; then
                printf 'fix the annotation: %s has only the keys %s, expect=%s can never match' "$path" "$keys" "$expect"
                return 0
            fi
            change="add $path with at least one key"
            [[ -n "$expect" ]] && change="add key \"$expect\" to $path"
            [[ -n "$req" ]] && change="$change (required: $req)"
            ;;
        *)
            if [[ -n "$expect" ]] && ! _gates_const_allows "$enum" "$expect"; then
                printf 'fix the annotation: %s is one of %s, expect=%s can never match' "$path" "$enum" "$expect"
                return 0
            elif [[ "$type" == integer && -n "$expect" && ! "$expect" =~ ^-?[0-9]+$ ]]; then
                printf 'fix the annotation: %s is an integer, expect=%s can never match' "$path" "$expect"
                return 0
            elif [[ "$type" == integer && -n "$expect" ]] && _gates_const_below "$expect" "$min"; then
                # A value below the schema minimum makes verify.sh refuse
                # the policy, so it is never a change to propose (#199).
                printf 'fix the annotation: %s is an integer >= %s, expect=%s can never match' "$path" "$min" "$expect"
                return 0
            elif [[ -n "$expect" ]]; then
                change="set $path = $expect"
            elif [[ -n "$enum" ]]; then
                change="set $path to one of $enum"
            else
                change="set $path to a value other than false"
            fi
            ;;
    esac
    printf 'edit policy.json (overlay): %s, then re-sync if a contract is live' "$change"
}

# The concrete change proposed for a missing surface (R5). Policy changes name
# the OVERLAY on purpose — with a 003 contract live, that edit flows through
# sync into the effective policy exactly like any overlay deviation.
_gates_const_proposed() { # <surface> <ref> <expect>
    local surface="${1:-}" ref="${2:-}" expect="${3:-}"
    case "$surface" in
        policy)
            _gates_const_policy_proposal "$(_gates_const_policy_path "$ref")" "$expect"
            ;;
        agent-hook)
            printf 'wire .claude/hooks/gates/%s and reference it in settings.json (/speckit.gates.init)' "$ref"
            ;;
        git-hook)
            printf 'install the %s git hook, executable, delegating to the gates runtime (/speckit.gates.init)' "$ref"
            ;;
        ci)
            printf 'add a "%s" check to CI (/speckit.gates.ci <platform>)' "$ref"
            ;;
        accept)
            printf 'add an accept block "# verifies: %s" in specs/%s/tasks.md' "${ref#*/}" "${ref%%/*}"
            ;;
        scanner)
            printf 'enable %s in the %s scanner config' "${ref#*:}" "${ref%%:*}"
            ;;
        *)
            printf '%s' '-'
            ;;
    esac
}

# Evaluate one annotated principle's surface. Echoes "state<TAB>proposed".
_gates_const_eval_surface() { # <surface> <ref> <expect> <root>
    local surface="${1:-}" ref="${2:-}" expect="${3:-}" root="${4:-}"
    local state
    case "$surface" in
        prose) printf 'prose-only\t-'; return 0 ;;
        policy) state="$(_gates_const_eval_policy "$ref" "$expect")" ;;
        agent-hook) state="$(_gates_const_eval_agent_hook "$ref" "$root")" ;;
        git-hook) state="$(_gates_const_eval_git_hook "$ref" "$root")" ;;
        ci) state="$(_gates_const_eval_ci "$ref" "$root")" ;;
        accept) state="$(_gates_const_eval_accept "$ref" "$root")" ;;
        scanner) state="$(_gates_const_eval_scanner "$ref" "$root")" ;;
        *) state="missing" ;;
    esac
    local proposed="-"
    [[ "$state" == "missing" ]] && proposed="$(_gates_const_proposed "$surface" "$ref" "$expect")"
    [[ "$state" == "pending-boundary" ]] && proposed="$(_gates_const_proposed "$surface" "$ref" "$expect")"
    printf '%s\t%s' "$state" "$proposed"
}

# align (contracts/cli-contracts.md): one TSV row per annotated principle,
#   principle<TAB>surface<TAB>ref<TAB>state<TAB>proposed-change
# state in active | missing | pending-boundary (prose-only reported as its own
# state, never a gap). Pure computation — no writes, no network. A policy
# override file (--policy) sets the enforced policy explicitly; otherwise the
# loader resolves it (effective when a 003 contract is live).
gates_const_align() { # <root> <constitution> [<policy-override>]
    local root="${1:-}" constitution="${2:-}" policy="${3:-}"
    [[ -f "$constitution" ]] || {
        echo "constitution: not found: $constitution" >&2
        return 1
    }
    [[ -n "$policy" ]] && export GATES_POLICY_FILE="$policy"
    local tag line name surface ref expect
    while IFS=$'\t' read -r tag line name surface ref expect; do
        [[ "$tag" == "PRINCIPLE" ]] || continue
        [[ -z "$surface" ]] && continue
        local sp
        sp="$(_gates_const_eval_surface "$surface" "$ref" "$expect" "$root")"
        printf '%s\t%s\t%s\t%s\n' "$name" "$surface" "$ref" "$sp"
    done <<<"$(gates_const_parse "$constitution")"
    return 0
}

# --- check (health verdict, US3) ---------------------------------------------

# Emit a machine protocol for one constitution's enforcement health and return
# 0 (every principle enforced/prose-only/unannotated) or 1 (>=1 gap or
# malformed marker — FR-009 fixed severity). Reuses align's surface evaluators.
# Protocol lines (TAB-separated), for both the CLI and doctor:
#   ENFORCED<TAB><principle><TAB><surface><TAB><ref>
#   GAP<TAB><principle><TAB><surface><TAB><ref><TAB><state>
#   PROSE<TAB><principle>
#   MALFORMED<TAB><line><TAB><message>
#   UNANNOTATED<TAB><count>
gates_const_check_raw() { # <root> <constitution> [<policy>]
    local root="${1:-}" constitution="${2:-}" policy="${3:-}"
    [[ -f "$constitution" ]] || {
        printf 'NOFILE\t%s\n' "$constitution"
        return 1
    }
    [[ -n "$policy" ]] && export GATES_POLICY_FILE="$policy"
    local rc=0 unann=0
    local tag line name surface ref expect
    while IFS=$'\t' read -r tag line name surface ref expect; do
        case "$tag" in
            MALFORMED)
                # parse cols: MALFORMED<TAB>line<TAB>name<TAB>message
                printf 'MALFORMED\t%s\t%s\n' "$line" "$surface"
                rc=1
                ;;
            NOCORE) printf 'NOCORE\n' ;;
            PRINCIPLE)
                if [[ -z "$surface" ]]; then
                    unann=$((unann + 1))
                    continue
                fi
                if [[ "$surface" == "prose" ]]; then
                    printf 'PROSE\t%s\n' "$name"
                    continue
                fi
                local sp state
                sp="$(_gates_const_eval_surface "$surface" "$ref" "$expect" "$root")"
                state="${sp%%$'\t'*}"
                if [[ "$state" == "active" ]]; then
                    printf 'ENFORCED\t%s\t%s\t%s\n' "$name" "$surface" "$ref"
                else
                    printf 'GAP\t%s\t%s\t%s\t%s\n' "$name" "$surface" "$ref" "$state"
                    rc=1
                fi
                ;;
        esac
    done <<<"$(gates_const_parse "$constitution")"
    printf 'UNANNOTATED\t%d\n' "$unann"
    return "$rc"
}

# CLI formatter for `check`: one human line per principle, exit 0/1. <label> is
# the display name for malformed markers (e.g. constitution.md).
gates_const_check() { # <root> <constitution> [<policy>] [<label>]
    local root="${1:-}" constitution="${2:-}" policy="${3:-}" label="${4:-constitution.md}"
    local raw rc=0
    raw="$(gates_const_check_raw "$root" "$constitution" "$policy")" || rc=1
    local tag a b c d
    while IFS=$'\t' read -r tag a b c d; do
        case "$tag" in
            NOFILE) echo "constitution: no constitution at $a" >&2 ;;
            ENFORCED) echo "  enforced: $a ($b:$c)" ;;
            PROSE) echo "  prose-only: $a" ;;
            GAP) echo "  gap: $a — $b ref=$c not wired ($d)" ;;
            MALFORMED) echo "  $label:$a: malformed marker: $b" ;;
            NOCORE) echo "  $label has no '## Core Principles' section, so it declares no principles" ;;
            UNANNOTATED)
                [[ "$a" -gt 0 ]] && echo "  $a principle(s) unannotated (informational)"
                ;;
        esac
    done <<<"$raw"
    return "$rc"
}
