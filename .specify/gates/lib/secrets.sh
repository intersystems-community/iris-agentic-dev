# shellcheck shell=bash
# secrets.sh -- the secret and forbidden-file rules, shared by the git
# boundary (hooks/git/pre-commit: the staged files) and the CI boundary
# (pr-check.sh: the files each commit in the PR range adds or changes), so
# the two cannot drift (issue #212). Sourced by hooks that run under macOS
# /bin/bash 3.2 with `set -u`: no associative arrays, guarded empty arrays.
#
#   gates_forbidden_path <path>   # 0 when <path> must never be committed
#   gates_secret_scan             # stdin: NUL-separated "<rev>:<path>"
#                                 # entries; report on stdout; 0 clean,
#                                 # 1 findings, 2 content unreadable
#   gates_secret_scan_range       # stdin: commit ids, one per line,
#                                 # oldest first; the files each adds or
#                                 # changes; report and exit as above
#   gates_secret_scan_file <file> # a file outside git (a gate's kept
#                                 # output); 0 clean, 1 a rule matched
#                                 # (its label in GATES_SECRET_WHAT), 2
#                                 # unreadable
#
# The agent-boundary hooks (protect-files.sh, validate-bash.sh) source this
# file too and call gates_forbidden_path, so the forbidden-name list has one
# source at every boundary (issue #221). They run it under nocasematch.

# Template/example files are meant to be committed even when the base name
# looks sensitive (.env.example, config.sample, .env.template); that check
# comes before the .env.* rule. Parameter expansion and case, no
# subprocess: this runs once per file (issue #133). GATES_FORBIDDEN_WHAT
# names the rule that matched.
gates_forbidden_path() { # <path>
    local file="$1" basename
    # The text after the last slash. ${file##*/} is quadratic in bash 3.2,
    # minutes on a 200 KB word of a command (issue #231); the regex is
    # linear, and the expansion stays for text the regex cannot read.
    if [[ "$file" =~ [^/]*$ ]]; then
        basename="${BASH_REMATCH[0]}"
    else
        basename="${file##*/}"
    fi
    GATES_FORBIDDEN_WHAT=""

    case "$basename" in
        *.example | *.sample | *.template) return 1 ;;
    esac

    case "$basename" in
        .env | .env.*) GATES_FORBIDDEN_WHAT="environment file" ;;
        id_rsa* | id_ed25519* | id_ecdsa* | authorized_keys | known_hosts) GATES_FORBIDDEN_WHAT="SSH key or config" ;;
        *.pem | *.key | *.crt | *.p12 | *.pfx | *.jks | *.keystore) GATES_FORBIDDEN_WHAT="certificate or key store" ;;
        credentials | credentials.json | credentials.yml | credentials.yaml | .netrc | .pypirc)
            GATES_FORBIDDEN_WHAT="credentials file"
            ;;
        gcloud-*.json | service-account*.json | aws-credentials) GATES_FORBIDDEN_WHAT="cloud credentials file" ;;
    esac
    [[ -z "$GATES_FORBIDDEN_WHAT" ]] || return 0

    # A leading slash, so a top-level .ssh/ matches as well.
    case "/$file" in
        */.ssh/* | */.gnupg/* | */.aws/* | */.gcloud/*)
            GATES_FORBIDDEN_WHAT="file in a sensitive directory"
            return 0
            ;;
        # The directory itself, without its trailing slash (~/.ssh).
        */.ssh | */.gnupg | */.aws | */.gcloud)
            GATES_FORBIDDEN_WHAT="sensitive directory"
            return 0
            ;;
    esac

    return 1
}

# One label, grep flag and ERE per rule, checked in this order; a file is
# reported once, for the first rule it matches. The last rule is the
# generic password/secret/token assignment. POSIX classes only: inside a
# bracket expression `\s` and `\x27` are literal characters, so the old
# `[^\s"']` matched spaces and flagged prose such as the runtime's own
# `"unparseable token: " kv` (issue #50). The keyword starts a name or
# follows a character that is not a letter or digit (`DB_PASSWORD`,
# `--password`), so a camel-case name such as `identitySecret` is no
# keyword; a value starting with `$(`, `${` or `$NAME` is a command
# substitution or a variable reference, not a literal secret (issue #310).
GATES_SECRET_LABELS=("AWS key pattern" "OpenAI key pattern" "GitHub token pattern"
    "GitLab token pattern" "Slack token pattern" "Possible credential assignment")
GATES_SECRET_FLAGS=(-E -E -E -E -E -i)
GATES_SECRET_PATTERNS=('AKIA[0-9A-Z]{16}' 'sk-[a-zA-Z0-9]{48}' '(ghp_|gho_)[a-zA-Z0-9]{36}'
    'glpat-[a-zA-Z0-9_-]{20}' 'xoxb-[0-9]{10,}'
    '(^|[^A-Za-z0-9])(password|secret|api_key|token)[[:space:]]*[=:][[:space:]]*["'\'']([^$[:space:]"'\''][^[:space:]"'\'']{7,}|\$[^({A-Za-z_[:space:]"'\''][^[:space:]"'\'']{6,})')
GATES_SECRET_HEURISTIC="Possible credential assignment"

# The gates' own files (issue #308): the vendored extension
# (.specify/extensions/gates/**) and the projected runtime
# (.specify/gates/**) are overwritten by every upgrade, so a match of the
# heuristic rule above in them cannot be fixed by editing them. A copy whose
# content is exactly what was projected -- the hash .projected.sha256
# records for it in the same snapshot (the index, or the commit), or one a
# released version shipped (lib/known-releases.sha256) -- skips that rule.
# An edited copy is scanned as before, and the exact key rules scan every
# file.
_GATES_KNOWN_RELEASES=""
case "${BASH_SOURCE[0]}" in
    */*) _GATES_KNOWN_RELEASES="$(cd "${BASH_SOURCE[0]%/*}" 2>/dev/null && pwd)/known-releases.sha256" ;;
esac

# The projected path the manifest records for <path>, on stdout; 1 when
# <path> is not a gates file. The extension's runtime/<src> maps as the
# projection table does (lib/manifest.sh).
_gates_managed_target() { # <path>
    case "$1" in
        .specify/gates/*) printf '%s\n' "$1" ;;
        .specify/extensions/gates/runtime/hooks/git/*) printf '.specify/gates/hooks/%s\n' "${1#.specify/extensions/gates/runtime/hooks/git/}" ;;
        .specify/extensions/gates/runtime/hooks/claude/*) printf '.claude/hooks/gates/%s\n' "${1#.specify/extensions/gates/runtime/hooks/claude/}" ;;
        .specify/extensions/gates/runtime/*) printf '.specify/gates/%s\n' "${1#.specify/extensions/gates/runtime/}" ;;
        *) return 1 ;;
    esac
}

# 0 when the blob <object> (a git object name: ":<path>", "<rev>:<path>" or
# a blob id) holds what the manifest of <rev> (empty: the index) records
# for <path>'s projected target, or what a released version shipped there.
# No hash tool, no manifest, no record: 1, and the file is scanned.
_gates_pristine() { # <rev> <path> <object>
    local target sum
    target="$(_gates_managed_target "$2")" || return 1
    if command -v sha256sum >/dev/null 2>&1; then
        sum="$(git cat-file blob "$3" 2>/dev/null | sha256sum)" || return 1
    elif command -v shasum >/dev/null 2>&1; then
        sum="$(git cat-file blob "$3" 2>/dev/null | shasum -a 256)" || return 1
    else
        return 1
    fi
    sum="${sum%% *}"
    [[ "${#sum}" -eq 64 ]] || return 1
    if git cat-file blob "$1:.specify/gates/.projected.sha256" 2>/dev/null \
        | awk -v p="$target" -v h="$sum" 'NR > 1 && $1 == h && $2 == p { found = 1 } END { exit !found }'; then
        return 0
    fi
    [[ -n "$_GATES_KNOWN_RELEASES" && -f "$_GATES_KNOWN_RELEASES" ]] || return 1
    awk -F '\t' -v p="$target" -v h="$sum" '$2 == h && $3 == p { found = 1 } END { exit !found }' \
        "$_GATES_KNOWN_RELEASES"
}

# Scan one file that git does not hold, such as a gate's kept output
# (verify.sh, #309): the same rules, in the same order, as a staged file.
# GATES_SECRET_WHAT names the first rule that matched. An unreadable file
# returns 2, so it is never taken for a clean one.
gates_secret_scan_file() { # <file>
    local i rc
    # shellcheck disable=SC2034  # read by the caller
    GATES_SECRET_WHAT=""
    [[ -f "$1" && -r "$1" ]] || return 2
    for i in "${!GATES_SECRET_PATTERNS[@]}"; do
        rc=0
        grep -q -E "${GATES_SECRET_FLAGS[$i]}" -e "${GATES_SECRET_PATTERNS[$i]}" -- "$1" 2>/dev/null || rc=$?
        if [[ "$rc" -eq 0 ]]; then
            # shellcheck disable=SC2034  # read by the caller
            GATES_SECRET_WHAT="${GATES_SECRET_LABELS[$i]}"
            return 1
        fi
        [[ "$rc" -eq 1 ]] || return 2
    done
    return 0
}

# Paths (of the NUL-separated list on stdin) whose content matches rule <i>
# in the index (--cached) or in any of the given commits, NUL-separated on
# stdout ("<path>" for the index, "<rev>:<path>" for a commit). One
# `git grep` per rule reads the blobs directly, binary ones included,
# instead of one `git show` per file (issue #133); xargs splits a long
# list, repeating the revisions. Literal pathspecs, so a name such as
# `*.ts` matches only itself. Returns 1 when git grep fails (exit > 1),
# never mistaking unreadable content for clean content. The failure is
# recorded in <fail-file>: xargs exit codes differ (BSD xargs returns 1
# for an invocation that found no match).
_gates_secret_rule() { # <i> <fail-file> --cached | <rev>...
    local i="$1" fail="$2"
    shift 2
    rm -f "$fail"
    # shellcheck disable=SC2016  # the script's $1/$2/$3/$@ belong to sh
    xargs -0 sh -c 'p="$1" f="$2" fail="$3"; shift 3
        git --literal-pathspecs grep --no-color --full-name -l -z -E "$f" -e "$p" "$@"
        [ "$?" -le 1 ] || : >"$fail"' sh "${GATES_SECRET_PATTERNS[$i]}" "${GATES_SECRET_FLAGS[$i]}" \
        "$fail" "$@" -- \
        && [[ ! -e "$fail" ]]
}

# Print the report lines on stdin, each "<rev>\t<line>": an empty <rev>
# (the index) prints the line bare; a commit's lines are grouped under a
# "commit <hash> <subject>:" line and indented.
_gates_secret_print() {
    local line rev last=""
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        rev="${line%%$'\t'*}"
        line="${line#*$'\t'}"
        if [[ -z "$rev" ]]; then
            printf '%s\n' "$line"
            continue
        fi
        if [[ "$rev" != "$last" ]]; then
            echo "commit $(git log -1 --format='%h %s' "$rev"):"
            last="$rev"
        fi
        printf '  %s\n' "$line"
    done
}

# The hit lines on stdin ("H\t<label>\t<rev>:<path>"), without the
# heuristic rule's hits on a gates file that holds what was projected
# (issue #308). Only those hits cost a hash; the rest pass through.
_gates_secret_drop_pristine() {
    local line key rev
    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        case "$line" in
            "H"$'\t'"$GATES_SECRET_HEURISTIC"$'\t'*.specify/*)
                key="${line#H$'\t'"$GATES_SECRET_HEURISTIC"$'\t'}"
                rev="${key%%:*}"
                _gates_pristine "$rev" "${key#*:}" "$key" && continue
                ;;
        esac
        printf '%s\n' "$line"
    done
}

# Scan the entries on stdin, each "<rev>:<path>" (NUL-separated, grouped
# by revision): an empty <rev> means the staged copy in the index, a
# commit means that commit's copy. A forbidden path is reported and not
# scanned; the rest are scanned in one batch per rule over every revision
# at once. A hit counts only for an entry fed in, so a file a commit did
# not change is not reported for it. One line per offending entry, in
# input order: the forbidden-file refusal, or the first rule it matched.
# Commit entries are grouped under a "commit <hash> <subject>:" line and
# indented; index entries print bare, as pre-commit always has.
gates_secret_scan() {
    local entry rev last="" verdicts="" hits="" tmp i rc=0 report
    local -a revs=() paths=()
    while IFS= read -r -d '' entry; do
        rev="${entry%%:*}"
        if gates_forbidden_path "${entry#*:}"; then
            verdicts+="F"$'\t'"$entry"$'\n'
        else
            verdicts+="S"$'\t'"$entry"$'\n'
            paths+=("${entry#*:}")
            if [[ -n "$rev" && "$rev" != "$last" ]]; then
                revs+=("$rev")
                last="$rev"
            fi
        fi
    done

    if [[ "${#paths[@]}" -gt 0 ]]; then
        tmp="$(mktemp 2>/dev/null || mktemp -t gates-scan)" || return 2
        if [[ "${#revs[@]}" -eq 0 ]]; then
            set -- --cached
        else
            set -- "${revs[@]}"
        fi
        for i in "${!GATES_SECRET_PATTERNS[@]}"; do
            if printf '%s\0' "${paths[@]}" | _gates_secret_rule "$i" "$tmp.fail" "$@" >"$tmp"; then
                # The index form prints bare paths: key them ":<path>" like
                # their entries.
                hits+="$(tr '\0' '\n' <"$tmp" | awk -v l="${GATES_SECRET_LABELS[$i]}" \
                    -v idx="${#revs[@]}" 'NF { print "H\t" l "\t" (idx == 0 ? ":" : "") $0 }')"$'\n'
            else
                rc=2
                break
            fi
        done
        rm -f "$tmp" "$tmp.fail"
        hits="$(_gates_secret_drop_pristine <<<"$hits")"$'\n'
    fi

    report="$(printf '%s%s' "$hits" "$verdicts" | awk -F'\t' '
        $1 == "H" { key = substr($0, length($2) + 4); if (!(key in hit)) hit[key] = $2; next }
        {
            key = substr($0, 3); rev = key; sub(/:.*/, "", rev); name = substr(key, length(rev) + 2)
            if ($1 == "F") print rev "\tBLOCKED: forbidden file: " name
            else if (key in hit) print rev "\t  SECRET: " hit[key] " in " name
        }')"
    _gates_secret_print <<<"$report"

    [[ "$rc" -ne 0 ]] && return "$rc"
    [[ -n "$report" ]] && return 1
    return 0
}

# Scan the commits on stdin (one id per line, oldest first) for the files
# each adds or changes, with the report gates_secret_scan gives for the
# same "<commit>:<path>" entries. One `git diff-tree --stdin` pass lists
# every commit's changed paths with their blob ids (a merge: the paths
# whose result differs from every parent, -c; a rename under its new
# name). Each distinct blob is scanned once per rule, through a temporary
# index whose entries are named by blob id, and a hit is mapped back to
# every commit and path that added that blob (issue #228): the time
# follows the distinct content the range adds, not commits x files. Only
# regular files are read, as git grep over a commit skips symlinks and
# submodules.
gates_secret_scan_range() {
    local tmp p i rc=0 report
    tmp="$(mktemp -d 2>/dev/null || mktemp -d -t gates-scan)" || return 2
    if ! git diff-tree --stdin -r --root -c --no-renames --raw -z --diff-filter=ACMRT >"$tmp/raw"; then
        rm -rf "$tmp"
        return 2
    fi
    # Records "<commit>\0" and ":<modes> <blobs> <status>\0<path>\0": one
    # colon per parent, then a mode and a blob id per parent plus the
    # commit's own (the last of each), then the status. awk turns them
    # into "<commit>\t<mode>\t<blob>\t<path>" lines; a newline inside a
    # path travels as \001 and is restored where the path is used. A bash
    # read loop over the stream would read it a byte at a time.
    tr '\n\0' '\001\n' <"$tmp/raw" | awk '
        want { print commit "\t" mode "\t" blob "\t" $0; want = 0; next }
        /^:/ {
            sub(/^:+/, ""); n = split($0, f, " ")
            mode = f[(n - 1) / 2]; blob = f[n - 1]; want = 1; next
        }
        { commit = $0 }' >"$tmp/all"
    # The forbidden-name check once per distinct path.
    cut -f4- "$tmp/all" | LC_ALL=C sort -u | while IFS= read -r p; do
        if gates_forbidden_path "${p//$'\001'/$'\n'}"; then
            printf '%s\n' "$p"
        fi
    done >"$tmp/forbidden"
    # Per entry: F (forbidden, not scanned) or S (a regular file to scan),
    # and the distinct blobs to scan.
    awk -F'\t' -v forb="$tmp/forbidden" -v blobs="$tmp/blobs" '
        FILENAME == forb { bad[$0] = 1; next }
        {
            p = $0
            for (k = 0; k < 3; k++) p = substr(p, index(p, "\t") + 1)
            if (p in bad) print "F\t" $1 "\t\t" p
            else if ($2 ~ /^100/) {
                print "S\t" $1 "\t" $3 "\t" p
                if (!($3 in seen)) { seen[$3] = 1; print "100644 " $3 "\t" $3 >blobs }
            }
        }' "$tmp/forbidden" "$tmp/all" >"$tmp/entries"
    touch "$tmp/blobs"

    : >"$tmp/hits"
    if [[ -s "$tmp/blobs" ]]; then
        if ! GIT_INDEX_FILE="$tmp/index" git -c core.splitIndex=false update-index --add --index-info <"$tmp/blobs"; then
            rm -rf "$tmp"
            return 2
        fi
        for i in "${!GATES_SECRET_PATTERNS[@]}"; do
            GIT_INDEX_FILE="$tmp/index" git grep --cached --no-color --full-name -l -z -E \
                "${GATES_SECRET_FLAGS[$i]}" -e "${GATES_SECRET_PATTERNS[$i]}" -- ':(top)' >"$tmp/hit"
            if [[ "$?" -gt 1 ]]; then
                rc=2
                break
            fi
            tr '\0' '\n' <"$tmp/hit" | awk -v l="${GATES_SECRET_LABELS[$i]}" 'NF { print l "\t" $0 }' >>"$tmp/hits"
        done
    fi

    # A gates file whose first match is the heuristic rule and whose blob
    # holds what that commit's manifest records (issue #308): its
    # "<commit>\t<path>" goes to the skip list. Only those entries cost a
    # hash.
    : >"$tmp/skip"
    if grep -q "^$GATES_SECRET_HEURISTIC"$'\t' "$tmp/hits"; then
        awk -F'\t' -v hits="$tmp/hits" -v l="$GATES_SECRET_HEURISTIC" '
            FILENAME == hits { if (!($2 in first)) first[$2] = $1; next }
            $1 == "S" && ($3 in first) && first[$3] == l {
                p = $0
                for (k = 0; k < 3; k++) p = substr(p, index(p, "\t") + 1)
                if (p ~ /^\.specify\//) print $2 "\t" $3 "\t" p
            }' "$tmp/hits" "$tmp/entries" | while IFS=$'\t' read -r c b p; do
            _gates_pristine "$c" "$p" "$b" && printf '%s\t%s\n' "$c" "$p"
        done >"$tmp/skip"
    fi

    # Per entry, in input order: the forbidden-file refusal, or the first
    # rule its blob matched. The path is everything after the third tab.
    report="$(awk -F'\t' -v hits="$tmp/hits" -v skip="$tmp/skip" -v l="$GATES_SECRET_HEURISTIC" '
        FILENAME == hits { if (!($2 in hit)) hit[$2] = $1; next }
        FILENAME == skip { sk[$0] = 1; next }
        {
            p = $0
            for (k = 0; k < 3; k++) p = substr(p, index(p, "\t") + 1)
            pristine = ($3 in hit) && hit[$3] == l && (($2 "\t" p) in sk)
            gsub("\001", "\n", p)
            if ($1 == "F") print $2 "\tBLOCKED: forbidden file: " p
            else if (($3 in hit) && !pristine) print $2 "\t  SECRET: " hit[$3] " in " p
        }' "$tmp/hits" "$tmp/skip" "$tmp/entries")"
    rm -rf "$tmp"
    _gates_secret_print <<<"$report"

    [[ "$rc" -ne 0 ]] && return "$rc"
    [[ -n "$report" ]] && return 1
    return 0
}
