#!/usr/bin/env bash
# manifest.sh -- what projection wrote, and what changed since.
#
# Usage (sourced; bash 3.2, no associative arrays):
#   gates_sha256 <file>                    # 64-hex hash (lib/attest.sh); fails if no tool
#   gates_version_cmp <a> <b>              # prints -1, 0 or 1
#   gates_projection_table <runtime> <0|1> # "<src-rel>\t<target-rel>" lines
#   gates_is_exec_target <target-rel>      # 0 if the target needs +x
#   gates_manifest_load <root>             # sets GATES_MANIFEST_* (below)
#   gates_manifest_hash <target-rel>       # hash recorded for a path
#   gates_manifest_write <root> <version>  # "<hash>  <path>" lines on stdin
#   gates_holds_load <root>                # sets GATES_HOLDS (paths only)
#   gates_is_held <target-rel>
#   gates_known_match <target-rel> <hash>  # 0 if a released version had it
#   gates_holds_ci <root>                  # acknowledged CI step ids (ci:<id>)
#   gates_ci_step_re <id>                  # a template step's regex; empty if unknown
#   gates_ci_candidates <root>             # every pipeline file, gates or not
#   gates_ci_live <file>                   # the file minus comments and inert steps
#   gates_ci_unproven <file>               # why no gates step is proven; empty if one is
#   gates_ci_files <root>                  # pipeline files running the gates; 1 if none
#   gates_ci_inert <root>                  # "<file>\t<why>": verify.sh called, none proven
#   gates_ci_body <root>                   # live text of those files
#   gates_ci_present <root>                # template step ids those files run
#   gates_ci_missing <root>                # template step ids those files lack
#   gates_classify <root> <src-abs> <target-rel>
#       absent | upstream | pristine | edited | held
#
# Formats: specs/005-upgrade-safe-projection/data-model.md.
# GATES_MANIFEST_STATUS is absent, ok, or corrupt (with GATES_MANIFEST_ERROR).
# GATES_KNOWN_FILE (set by the caller) is lib/known-releases.sha256 of the
# extension doing the projection; empty or missing means no fallback.

# shellcheck disable=SC2034   # library file; the GATES_* globals are read by callers

GATES_MANIFEST_REL=".specify/gates/.projected.sha256"
GATES_HOLDS_REL=".specify/gates/.upgrade-holds"
GATES_LOCAL_REL=".specify/gates/hooks.local.d"

# gates_sha256 comes from lib/attest.sh (one implementation: sha256sum, then
# shasum -a 256, else fail).
if ! declare -f gates_sha256 >/dev/null 2>&1; then
    # shellcheck source=/dev/null disable=SC1091
    source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/attest.sh"
fi

gates_version_cmp() { # <a> <b>
    awk -v a="$1" -v b="$2" 'BEGIN {
        na = split(a, x, "."); nb = split(b, y, ".")
        n = (na > nb) ? na : nb
        for (i = 1; i <= n; i++) {
            if ((x[i] + 0) < (y[i] + 0)) { print -1; exit }
            if ((x[i] + 0) > (y[i] + 0)) { print 1; exit }
        }
        print 0
    }'
}

# The projection table: every file projection owns, as source paths relative
# to the extension's runtime/ directory and targets relative to the project.
# lib/*.sh and the agent hooks are enumerated from the source, so a new
# library ships without a table edit.
gates_projection_table() { # <runtime-dir> <agent-hooks:0|1>
    local src="$1" agent="${2:-1}" f n
    for n in verify.sh doctor.sh canary.sh contract.sh constitution.sh pr-check.sh project.sh \
        install-shellcheck.sh shellcheck.sha256 policy.schema.json; do
        [[ -f "$src/$n" ]] && printf '%s\t%s\n' "$n" ".specify/gates/$n"
    done
    for f in "$src"/lib/*.sh; do
        [[ -f "$f" ]] || continue
        n="${f##*/}"
        printf 'lib/%s\t.specify/gates/lib/%s\n' "$n" "$n"
    done
    for n in pre-commit pre-merge-commit commit-msg stub.sh; do
        [[ -f "$src/hooks/git/$n" ]] && printf 'hooks/git/%s\t.specify/gates/hooks/%s\n' "$n" "$n"
    done
    if [[ "$agent" == "1" ]]; then
        for f in "$src"/hooks/claude/*.sh; do
            [[ -f "$f" ]] || continue
            n="${f##*/}"
            printf 'hooks/claude/%s\t.claude/hooks/gates/%s\n' "$n" "$n"
        done
    fi
    return 0
}

gates_is_exec_target() { # <target-rel>
    case "$1" in
        *.sh | .specify/gates/hooks/*) return 0 ;;
    esac
    return 1
}

GATES_MANIFEST_STATUS=absent
GATES_MANIFEST_VERSION=""
GATES_MANIFEST_BODY=""
GATES_MANIFEST_ERROR=""

gates_manifest_load() { # <root>
    local file="$1/$GATES_MANIFEST_REL" header bad
    GATES_MANIFEST_STATUS=absent
    GATES_MANIFEST_VERSION=""
    GATES_MANIFEST_BODY=""
    GATES_MANIFEST_ERROR=""
    [[ -e "$file" ]] || return 0
    GATES_MANIFEST_STATUS=corrupt
    if [[ ! -r "$file" ]]; then
        GATES_MANIFEST_ERROR="not readable"
        return 0
    fi
    header="$(head -n 1 "$file")"
    case "$header" in
        "# spec-gates-manifest v1 version="[0-9]*) ;;
        *)
            GATES_MANIFEST_ERROR="missing or unknown header (line 1)"
            return 0
            ;;
    esac
    GATES_MANIFEST_VERSION="${header#\# spec-gates-manifest v1 version=}"
    if ! grep -qE '^[0-9]+(\.[0-9]+)*$' <<<"$GATES_MANIFEST_VERSION"; then
        GATES_MANIFEST_ERROR="unparseable version in the header"
        return 0
    fi
    # Entries: 64 hex, two spaces, a path inside the projected trees, no "..".
    # No {64} interval: BSD awk and mawk do not all support it.
    bad="$(tail -n +2 "$file" | awk '
        $0 ~ /^[0-9a-f]+  [^ ]+$/ && length($1) == 64 && $2 !~ /\.\./ \
            && ($2 ~ /^\.specify\/gates\// || $2 ~ /^\.claude\/hooks\/gates\//) { next }
        { print NR + 1; exit }')"
    if [[ -n "$bad" ]]; then
        GATES_MANIFEST_ERROR="malformed entry on line $bad"
        return 0
    fi
    GATES_MANIFEST_BODY="$(tail -n +2 "$file")"
    GATES_MANIFEST_STATUS=ok
}

gates_manifest_hash() { # <target-rel>
    [[ -n "$GATES_MANIFEST_BODY" ]] || return 0
    printf '%s\n' "$GATES_MANIFEST_BODY" | awk -v p="$1" '$2 == p { print $1; exit }'
}

gates_manifest_write() { # <root> <version>; entries on stdin
    local file="$1/$GATES_MANIFEST_REL" tmp
    tmp="$file.tmp.$$"
    {
        printf '# spec-gates-manifest v1 version=%s\n' "$2"
        sort -k2
    } >"$tmp" || {
        rm -f "$tmp"
        return 1
    }
    mv -f "$tmp" "$file"
}

GATES_HOLDS=""

gates_holds_load() { # <root>
    local file="$1/$GATES_HOLDS_REL"
    GATES_HOLDS=""
    [[ -f "$file" ]] || return 0
    GATES_HOLDS="$(sed -e 's/#.*$//' -e 's/[[:space:]]*$//' -e 's/^[[:space:]]*//' "$file" \
        | grep -v '^$' | grep -v '^ci:' || true)"
}

gates_is_held() { # <target-rel>
    [[ -n "$GATES_HOLDS" ]] || return 1
    grep -qxF -- "$1" <<<"$GATES_HOLDS"
}

GATES_KNOWN_FILE="${GATES_KNOWN_FILE:-}"

# A projected file whose content some released version shipped was never
# edited locally. This is how a project without a manifest (projected by
# 0.3.x) upgrades without every changed file reading as a local edit.
gates_known_match() { # <target-rel> <hash>
    [[ -n "$GATES_KNOWN_FILE" && -f "$GATES_KNOWN_FILE" ]] || return 1
    awk -F '\t' -v p="$1" -v h="$2" '$2 == h && $3 == p { found = 1; exit } END { exit !found }' \
        "$GATES_KNOWN_FILE"
}

gates_classify() { # <root> <src-abs> <target-rel>
    local root="$1" src="$2" rel="$3" tgt cur rec
    tgt="$root/$rel"
    case "$rel" in
        "$GATES_LOCAL_REL"/*) echo local; return 0 ;;
    esac
    if gates_is_held "$rel"; then
        echo held
        return 0
    fi
    rec="$(gates_manifest_hash "$rel")"
    if [[ ! -e "$tgt" ]]; then
        # Deleted after projection is a local change, not a fresh install.
        if [[ -n "$rec" ]]; then echo edited; else echo absent; fi
        return 0
    fi
    if cmp -s "$src" "$tgt"; then
        echo upstream
        return 0
    fi
    cur="$(gates_sha256 "$tgt")" || return 2
    if [[ -n "$rec" && "$cur" == "$rec" ]]; then
        echo pristine
        return 0
    fi
    # No record for this path (no manifest yet, or the path was not
    # projected last time): a released version's content is pristine too.
    if [[ -z "$rec" ]] && gates_known_match "$rel" "$cur"; then
        echo pristine
        return 0
    fi
    echo edited
}

gates_holds_ci() { # <root>
    local file="$1/$GATES_HOLDS_REL"
    [[ -f "$file" ]] || return 0
    sed -e 's/#.*$//' -e 's/[[:space:]]*$//' -e 's/^[[:space:]]*//' "$file" \
        | sed -n 's/^ci://p'
}

# CI-template drift (#70, research R6): the shipped templates' steps,
# identified by the command each runs, so a pipeline adapted by hand on any
# platform is still recognized. <id>\t<extended regex>. The gates step is
# verify.sh with `--boundary ci` among its arguments, in any order (#171);
# whether a file runs it at all is proven by gates_ci_unproven (#198).
gates_ci_steps() {
    printf 'gates\tverify\\.sh[^|;&]*--boundary[[:space:]]+ci([^[:alnum:]_-]|$)\n'
    printf 'canary\tcanary\\.sh\n'
    printf 'pr\tpr-check\\.sh\n'
}

# Callers that run verify.sh without --boundary (#216): package.json,
# Taskfile and Makefile lines that name gates/verify.sh with no --boundary
# before the next ;, & or |. verify.sh still runs them, with a deprecation
# warning. Comment lines are skipped and backslash continuations joined, so
# `verify.sh \` + `--boundary ci` is not listed. One <file>:<line> each.
gates_verify_unbounded() { # <root>
    local root="$1" f seen="" s dup
    for f in package.json Taskfile.yml Taskfile.yaml Makefile makefile GNUmakefile; do
        [[ -f "$root/$f" ]] || continue
        # Makefile and makefile are one file on a case-insensitive disk.
        dup=0
        for s in $seen; do [[ "$root/$f" -ef "$root/$s" ]] && dup=1; done
        [[ "$dup" -eq 1 ]] && continue
        seen="$seen $f"
        awk -v f="$f" '
            function scan(line, at,    rest, seg) {
                while (match(line, /gates\/verify\.sh/)) {
                    rest = substr(line, RSTART + RLENGTH)
                    seg = rest
                    if (match(seg, /[;&|]/)) seg = substr(seg, 1, RSTART - 1)
                    if (seg !~ /--boundary/) { print f ":" at; return }
                    line = rest
                }
            }
            buf == "" && /^[ \t]*#/ { next }
            {
                if (buf == "") start = NR
                if (sub(/\\$/, "")) { buf = buf $0 " "; next }
                scan(buf $0, start)
                buf = ""
            }
            END { if (buf != "") scan(buf, start) }' "$root/$f"
    done
    return 0
}

# The regex of one template step id; empty for an id the template lacks.
gates_ci_step_re() { # <id>
    gates_ci_steps | awk -F '\t' -v id="$1" '$1 == id { print $2 }'
}

# What a ci:<id> hold gives up (#235): the check CI no longer runs. Empty
# for an id the template lacks.
gates_ci_step_omitted() { # <id>
    case "$1" in
        gates) echo "the gates (verify.sh --boundary ci) do not run in CI" ;;
        canary) echo "CI does not prove the gates still block: a gate that accepts its canary, or a linter the policy enables that is not installed, passes unnoticed" ;;
        pr) echo "PR titles, descriptions and Protected-Change declarations are not checked in CI, nor are the PR's commits scanned for secrets and forbidden files" ;;
    esac
}

# The template's pr step on its own (#235), for a pipeline that runs the
# gates step but not the rest of the template: the same commands, including
# the base revision's pr-check.sh run through GATES_RUNTIME_DIR (#166).
# docs/how-it-works.md and commands/speckit.gates.ci.md carry the same text.
gates_ci_pr_snippet() { # <github|gitlab|jenkins>
    case "$1" in
        github)
            # shellcheck disable=SC2016  # the text is printed, not run
            printf '%s\n' \
                '# spec-gates pr step (GitHub): add under steps: of the job that runs' \
                '# verify.sh --boundary ci, so its required check covers it. That job' \
                '# checks out with fetch-depth: 0, and the workflow runs on' \
                '# pull_request with types: [opened, synchronize, reopened, edited].' \
                '- name: Check the pull request (text, protected changes, secrets)' \
                "  if: github.event_name == 'pull_request'" \
                '  env:' \
                '    GATES_PR_TITLE: ${{ github.event.pull_request.title }}' \
                '    GATES_PR_BODY: ${{ !contains(fromJSON('"'"'["dependabot[bot]", "renovate[bot]"]'"'"'), github.event.pull_request.user.login) && github.event.pull_request.body || '"'"''"'"' }}' \
                '  run: |' \
                '    base="origin/$GITHUB_BASE_REF"' \
                '    if git cat-file -e "$base:.specify/gates/pr-check.sh" 2>/dev/null; then' \
                '      rt="$(mktemp -d)"' \
                '      git archive "$base" .specify/gates | tar -x -C "$rt"' \
                '      echo "pr-check: running the base revision'"'"'s pr-check.sh ($base)"' \
                '      GATES_RUNTIME_DIR="$rt/.specify/gates" bash "$rt/.specify/gates/pr-check.sh"' \
                '    else' \
                '      echo "pr-check: the base ($base) has no pr-check.sh (adoption PR); running the pull request'"'"'s own copy"' \
                '      bash .specify/gates/pr-check.sh' \
                '    fi'
            ;;
        gitlab)
            # shellcheck disable=SC2016  # the text is printed, not run
            printf '%s\n' \
                '# spec-gates pr step (GitLab): add as a job to the pipeline file that' \
                '# runs verify.sh --boundary ci. pr-check.sh needs bash, git, jq and' \
                '# python3 (curl fetches a truncated description). Editing an MR title' \
                '# or description starts no pipeline; re-run it after such edits.' \
                'gates-pr:' \
                '  stage: test' \
                '  image: node:26-slim' \
                '  timeout: 10m' \
                '  variables:' \
                '    GIT_DEPTH: "0"' \
                '  before_script:' \
                '    - apt-get update -q && apt-get install -y -q jq git python3 curl' \
                '  script:' \
                '    - |' \
                '      set -e' \
                '      base="${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}"' \
                '      if [ -z "$base" ]; then' \
                '        bash .specify/gates/pr-check.sh' \
                '      elif git cat-file -e "$base:.specify/gates/pr-check.sh" 2>/dev/null; then' \
                '        rt="$(mktemp -d)"' \
                '        git archive "$base" .specify/gates | tar -x -C "$rt"' \
                '        echo "pr-check: running the base revision'"'"'s pr-check.sh ($base)"' \
                '        GATES_RUNTIME_DIR="$rt/.specify/gates" bash "$rt/.specify/gates/pr-check.sh"' \
                '      else' \
                '        echo "pr-check: the base ($base) has no pr-check.sh (adoption MR); running the merge request'"'"'s own copy"' \
                '        bash .specify/gates/pr-check.sh' \
                '      fi' \
                '  rules:' \
                "    - if: '\$CI_PIPELINE_SOURCE == \"merge_request_event\"'"
            ;;
        jenkins)
            # shellcheck disable=SC2016  # the text is printed, not run
            printf '%s\n' \
                '// spec-gates pr step (Jenkins): add as a stage to the Jenkinsfile that' \
                '// runs verify.sh --boundary ci. The agent needs bash, git and jq, and a' \
                '// full-history checkout; outside PR builds the check skips itself.' \
                "stage('PR check') {" \
                '    steps {' \
                "        sh '''" \
                '            set -e' \
                '            if [ -z "${CHANGE_TARGET:-}" ]; then' \
                '                bash .specify/gates/pr-check.sh' \
                '                exit 0' \
                '            fi' \
                '            base="origin/$CHANGE_TARGET"' \
                '            if git cat-file -e "$base:.specify/gates/pr-check.sh" 2>/dev/null; then' \
                '                rt="$(mktemp -d)"' \
                '                git archive "$base" .specify/gates | tar -x -C "$rt"' \
                '                echo "pr-check: running the base revision'"'"'s pr-check.sh ($base)"' \
                '                GATES_RUNTIME_DIR="$rt/.specify/gates" bash "$rt/.specify/gates/pr-check.sh"' \
                '            else' \
                '                echo "pr-check: the base ($base) has no pr-check.sh (adoption PR); running the pull request'"'"'s own copy"' \
                '                bash .specify/gates/pr-check.sh' \
                '            fi' \
                "        '''" \
                '    }' \
                '}'
            ;;
        *) return 1 ;;
    esac
}

# Every pipeline file a supported platform reads, gates or not.
gates_ci_candidates() { # <root>
    local root="$1" f
    for f in "$root"/.github/workflows/*.yml "$root"/.github/workflows/*.yaml \
        "$root"/.gitlab-ci.yml "$root"/*.gitlab-ci.yml "$root"/Jenkinsfile*; do
        [[ -f "$f" ]] && printf '%s\n' "${f#"$root"/}"
    done
    return 0
}

# The platform a pipeline file belongs to: github, gitlab, jenkins or yaml.
_gates_ci_kind() { # <file>
    case "${1##*/}" in
        Jenkinsfile*) echo jenkins; return 0 ;;
        *gitlab-ci.yml) echo gitlab; return 0 ;;
    esac
    case "$1" in
        */.github/workflows/*) echo github ;;
        *) echo yaml ;;
    esac
}

# A pipeline file without its comments: YAML `#`; Groovy `//` and `/* */`
# in a Jenkinsfile. A comment marker needs a blank or the line start before
# it, so `https://` and `dist/*` stay intact.
_gates_ci_uncomment() { # <file>
    if [[ "$(_gates_ci_kind "$1")" == jenkins ]]; then
        awk '
            {
                s = $0; out = ""
                while (1) {
                    if (inc) {
                        p = index(s, "*/")
                        if (!p) { s = ""; break }
                        s = substr(s, p + 2); inc = 0; continue
                    }
                    q = match(s, /(^|[ \t])\/\//) ? RSTART : 0
                    p = match(s, /(^|[ \t])\/\*/) ? RSTART : 0
                    if (q && (!p || q < p)) { out = out substr(s, 1, q - 1); break }
                    if (p) { out = out substr(s, 1, p - 1) " "; s = substr(s, p + RLENGTH); inc = 1; continue }
                    out = out s; break
                }
                print out
            }' "$1"
    else
        awk '/^[ \t]*#/ { next } { sub(/[ \t]+#.*$/, ""); print }' "$1"
    fi
}

# YAML nodes that never run, or whose failure is ignored, dropped with their
# whole block (stdin to stdout):
#   - a step or job whose `if:` is literally false (`if: false`,
#     `if: ${{ false }}`, `if: false && ...`);
#   - GitHub: `continue-on-error: true` on a step or job, and a workflow
#     whose every trigger is workflow_dispatch or schedule (never a push or
#     a pull request);
#   - GitLab: `allow_failure: true`, `when: manual|never` on the job, rules
#     that never let it run (every rule up to the first unconditional one
#     is `when: never` or `manual`; under `workflow:`, the whole file), an
#     `only:`/`except:` that keeps it out of branch and merge request
#     pipelines (`only: [tags]`, `except: [branches]`), and a hidden
#     `.name:` job nothing extends or aliases;
#   - the rest of a run block (`run: |`, a script list) after an
#     unconditional `exit 0` or `exit` line.
_gates_ci_yaml_drop() { # <kind>
    awk -v kind="$1" '
        function indent(s) { return match(s, /[^ \t]/) ? RSTART - 1 : -1 }
        function keycol(s) { match(s, /^[ \t]*(-[ \t]+)?/); return RLENGTH }
        function body(s) { sub(/^[ \t]*(-[ \t]+)?/, "", s); sub(/[ \t;]*$/, "", s); return s }
        # The node a key in column c belongs to: the list item whose content
        # starts in that column, or the mapping key it is indented under.
        # Sets NB to the node indent; 0 if there is none.
        function node(i, c,   j, d) {
            for (j = i; j >= 1; j--) {
                if (match(line[j], /^[ \t]*-[ \t]+/) && RLENGTH == c) { NB = indent(line[j]); return j }
                d = indent(line[j])
                if (j < i && d >= 0 && d < c) { NB = d; return j }
            }
            return 0
        }
        function drop_node(i, c,   s, k, d) {
            s = node(i, c)
            if (!s) return
            for (k = s; k <= n; k++) {
                if (k > s) { d = indent(line[k]); if (d >= 0 && d <= NB) break }
                drop[k] = 1
            }
        }
        # w appears on another line as a whole word (extends:, !reference,
        # an alias): a hidden job used that way runs as part of another.
        function used(w, i,   j, s, p, a, b) {
            for (j = 1; j <= n; j++) {
                if (j == i) continue
                s = line[j]
                while ((p = index(s, w)) > 0) {
                    a = p > 1 ? substr(s, p - 1, 1) : ""
                    b = substr(s, p + length(w), 1)
                    if (a !~ /[A-Za-z0-9_.-]/ && b !~ /[A-Za-z0-9_.-]/) return 1
                    s = substr(s, p + length(w))
                }
            }
            return 0
        }
        # The rules list under line i lets nothing run: every rule up to the
        # first one without if:, changes: or exists: (which always matches)
        # says when: never or manual.
        function never_rules(i,   k, d, di, cond, w, seen, r) {
            di = -1; seen = 0
            for (k = i + 1; k <= n; k++) {
                d = indent(line[k])
                if (d < 0) continue
                if (di < 0) {
                    if (d < indent(line[i]) || line[k] !~ /^[ \t]*-/) return 0
                    di = d
                }
                if (d < di || (d == di && line[k] !~ /^[ \t]*-/)) break
                if (d == di) {
                    if (seen && (r = rule_end(w, cond)) >= 0) return r
                    seen = 1; cond = 0; w = ""
                }
                if (line[k] ~ /^[ \t]*(-[ \t]+)?(if|changes|exists):/) cond = 1
                if (match(line[k], /^[ \t]*(-[ \t]+)?when:[ \t]*/)) {
                    w = substr(line[k], RLENGTH + 1); gsub(/["\047 \t]/, "", w)
                }
            }
            if (seen && (r = rule_end(w, cond)) >= 0) return r
            return seen
        }
        # One rule settled: 0 it can run, 1 it always matches and never
        # runs, -1 the next rule decides.
        function rule_end(w, cond) {
            if (w !~ /^(never|manual)$/) return 0
            return cond ? -1 : 1
        }
        # The value of a list key (only:, except:, refs:) on line k, as
        # blank-separated words; a mapping value yields its refs: list, or
        # nothing when it has none.
        function listval(k,   t, d, j, dj, ci, out, u) {
            t = line[k]; sub(/^[^:]*:[ \t]*/, "", t)
            if (t != "") { gsub(/\[/, " ", t); gsub(/\]/, " ", t); gsub(/[",\047]/, " ", t); return t }
            d = indent(line[k]); out = ""; ci = -1
            for (j = k + 1; j <= n; j++) {
                dj = indent(line[j]); if (dj < 0) continue
                if (dj <= d) break
                if (ci < 0) ci = dj
                if (dj != ci) continue
                if (line[j] ~ /^[ \t]*-/) { u = body(line[j]); gsub(/["\047]/, "", u); out = out " " u }
                else if (line[j] ~ /^[ \t]*refs:/) return listval(j)
            }
            return out
        }
        function jobkey(i, key,   k, d, ci) {
            ci = -1
            for (k = i + 1; k <= n; k++) {
                d = indent(line[k]); if (d < 0) continue
                if (d == 0) break
                if (ci < 0) ci = d
                if (d == ci && line[k] ~ ("^[ \t]*" key ":")) return listval(k)
            }
            return "-"
        }
        # A GitLab job whose only:/except: still lets it run in a branch or
        # merge request pipeline. A ref name or pattern counts as a branch.
        function branch_job(i,   o, e, m, w, k, okb, okm) {
            o = jobkey(i, "only"); e = jobkey(i, "except")
            if (o == "-" || o ~ /^[ \t]*$/) o = "branches tags"
            okb = 0; okm = 0
            m = split(o, w, /[ \t]+/)
            for (k = 1; k <= m; k++) {
                if (w[k] == "") continue
                if (w[k] == "merge_requests") okm = 1
                else if (w[k] !~ /^(tags|schedules|triggers|web|api|pipelines|external|external_pull_requests|chat)$/) okb = 1
            }
            if (e != "-") {
                m = split(e, w, /[ \t]+/)
                for (k = 1; k <= m; k++) {
                    if (w[k] == "branches" || w[k] == "pushes") okb = 0
                    if (w[k] == "merge_requests") okm = 0
                }
            }
            return okb || okm
        }
        { n++; line[n] = $0 }
        END {
            if (kind == "github") {
                for (i = 1; i <= n; i++) if (line[i] ~ /^["\047]?on["\047]?:/) break
                if (i <= n) {
                    t = line[i]; sub(/^[^:]*:[ \t]*/, "", t)
                    m = 0
                    if (t != "") {
                        gsub(/\[/, " ", t); gsub(/\]/, " ", t); gsub(/[",\047]/, " ", t)
                        m = split(t, w, /[ \t]+/)
                    } else {
                        ci = -1
                        for (j = i + 1; j <= n; j++) {
                            d = indent(line[j])
                            if (d < 0) continue
                            if (d == 0) break
                            if (ci < 0) ci = d
                            if (d != ci) continue
                            t = body(line[j]); sub(/:.*$/, "", t); gsub(/["\047]/, "", t)
                            w[++m] = t
                        }
                    }
                    cnt = 0; runs = 0
                    for (k = 1; k <= m; k++) {
                        if (w[k] == "") continue
                        cnt++
                        if (w[k] != "workflow_dispatch" && w[k] != "schedule") runs = 1
                    }
                    if (cnt && !runs) exit
                }
            }
            for (i = 1; i <= n; i++) {
                s = line[i]; c = keycol(s)
                if (s ~ /^[ \t]*(-[ \t]+)?if:[ \t]*["\047]?(\$\{\{[ \t]*)?false[ \t]*(&&|\}\}|["\047]|$)/) { drop_node(i, c); continue }
                if (kind == "github" && s ~ /^[ \t]*(-[ \t]+)?continue-on-error:[ \t]*["\047]?(\$\{\{[ \t]*)?true[ \t]*(\}\})?[ \t]*["\047]?[ \t]*$/) { drop_node(i, c); continue }
                if (kind == "gitlab") {
                    if (s ~ /^[ \t]*(-[ \t]+)?allow_failure:[ \t]*true[ \t]*$/) { drop_node(i, c); continue }
                    if (s ~ /^[ \t]*when:[ \t]*["\047]?(manual|never)["\047]?[ \t]*$/) { drop_node(i, c); continue }
                    if (s ~ /^[ \t]*rules:[ \t]*$/) {
                        if (!never_rules(i)) continue
                        # Workflow rules that never match: no pipeline runs.
                        if (line[node(i, indent(s))] ~ /^workflow:/) exit
                        drop_node(i, indent(s))
                        continue
                    }
                    if (indent(s) == 0 && s ~ /^[^ \t-][^:]*:/ && !branch_job(i)) {
                        for (k = i; k <= n; k++) {
                            if (k > i && indent(line[k]) == 0) break
                            drop[k] = 1
                        }
                        continue
                    }
                    if (s ~ /^\.[^ \t:]*:/) {
                        nm = s; sub(/:.*$/, "", nm)
                        an = ""
                        if (match(s, /&[A-Za-z0-9_-]+/)) an = "*" substr(s, RSTART + 1, RLENGTH - 1)
                        if (!used(nm, i) && (an == "" || !used(an, i))) {
                            for (k = i; k <= n; k++) {
                                if (k > i && indent(line[k]) == 0) break
                                drop[k] = 1
                            }
                        }
                        continue
                    }
                }
                if (s !~ /^[ \t]*(-[ \t]+)?exit([ \t]+0)?[ \t;]*$/) continue
                # exit inside an if/for/case/function body, or near a heredoc,
                # is not shown to run unconditionally: leave the block alone.
                c = indent(s); item = (s ~ /^[ \t]*-/); depth = 0; here = 0
                for (j = i - 1; j >= 1; j--) {
                    d = indent(line[j])
                    if (d < 0) continue
                    if (d < c || (item && d == c && line[j] !~ /^[ \t]*-/)) break
                    b = body(line[j])
                    if (b ~ /<</) here = 1
                    if (b ~ /^(if|for|while|until|case|select)([ \t]|$)/ || b ~ /\{$/) depth++
                    if (b ~ /^(fi|done|esac|\})([ \t;]|$)/ || b ~ /;[ \t]*(fi|done|esac)$/) depth--
                }
                if (depth > 0 || here) continue
                for (k = i + 1; k <= n; k++) {
                    d = indent(line[k])
                    if (d >= 0 && (d < c || (item && d == c && line[k] !~ /^[ \t]*-/))) break
                    drop[k] = 1
                }
            }
            for (i = 1; i <= n; i++) if (!drop[i]) print line[i]
        }'
}

# A Jenkinsfile stage whose `when` is literally false
# (`when { expression { false } }`, or `return false`) never runs: blanked
# from its opening to its closing brace (stdin to stdout).
_gates_ci_jenkins_drop() {
    awk '
        { t = t $0 "\n" }
        END {
            L = length(t); sp = 0; last = 0
            for (i = 1; i <= L; i++) {
                ch = substr(t, i, 1)
                if (ch == "{") {
                    head[i] = substr(t, last + 1, i - last - 1)
                    up[i] = sp ? st[sp] : 0
                    st[++sp] = i; last = i
                } else if (ch == "}") {
                    if (sp) { cl[st[sp]] = i; sp-- }
                    last = i
                }
            }
            any = 0
            for (p in head) {
                if (!(p in cl) || head[p] !~ /(^|[^A-Za-z0-9_])when[ \t\n]*$/) continue
                b = substr(t, p + 1, cl[p] - p - 1); gsub(/[ \t\n;]+/, "", b)
                if (b != "expression{false}" && b != "expression{returnfalse}") continue
                for (q = up[p]; q; q = up[q])
                    if (head[q] ~ /(^|[^A-Za-z0-9_])stage[ \t\n]*\([^)]*\)[ \t\n]*$/) break
                if (q && (q in cl)) { for (k = q; k <= cl[q]; k++) cut[k] = 1; any = 1 }
            }
            if (!any) { printf "%s", t; exit }
            out = ""
            for (i = 1; i <= L; i++) {
                ch = substr(t, i, 1)
                if (ch == "\n") { print out; out = ""; continue }
                out = out ((i in cut) ? " " : ch)
            }
        }'
}

# Shell command segments (split at ; && || |) that cannot enforce, removed
# from each line (stdin to stdout): text an `echo` or `printf` prints, a
# command with `--dry-run`, one whose failure `|| true`, `|| :`,
# `|| exit 0` or `|| echo` swallows, and everything after an unconditional
# `exit 0` on the same line. A YAML key (`run:`) or a Jenkins `sh '` in
# front of the first command is kept as is; a Jenkins `sh` step with
# `returnStatus: true` never fails the build and is dropped.
_gates_ci_segments() { # <kind>
    awk -v kind="$1" '
        function word(s) {
            while (match(s, /^[ \t]+/) || match(s, /^["\047({!]/) \
                || match(s, /^(then|do|else|time)[ \t]+/) \
                || match(s, /^[A-Za-z_][A-Za-z0-9_]*=[^ \t]*[ \t]+/)) s = substr(s, RLENGTH + 1)
            match(s, /^[^ \t"\047;)}]*/)
            return substr(s, 1, RLENGTH)
        }
        # The fallback after || runs instead of failing: true, :, exit 0, or
        # a message (a { ...; exit 1; } group still fails).
        function swallows(s,   w) {
            if (s ~ /^[ \t]*[({]/) return 0
            w = word(s)
            return w == "true" || w == ":" || w == "echo" || w == "printf" \
                || s ~ /^[ \t({"\047]*exit[ \t]+0([^0-9]|$)/
        }
        {
            s = $0; pre = ""
            if (kind == "jenkins") {
                if (s ~ /returnStatus[ \t]*:[ \t]*true/) { print ""; next }
                if (match(s, /(^|[^A-Za-z0-9_.])(sh|bat|powershell|pwsh)[ \t]*\(?[ \t]*(script[ \t]*:[ \t]*)?/)) {
                    pre = substr(s, 1, RSTART + RLENGTH - 1); s = substr(s, RSTART + RLENGTH)
                }
            } else if (match(s, /^[ \t]*(-[ \t]+)?([A-Za-z_][A-Za-z0-9_-]*:([ \t]+|$))?/)) {
                pre = substr(s, 1, RLENGTH); s = substr(s, RLENGTH + 1)
            }
            n = 0; buf = ""; L = length(s)
            for (i = 1; i <= L; i++) {
                c2 = substr(s, i, 2); c1 = substr(s, i, 1)
                if (c2 == "&&" || c2 == "||") { seg[++n] = buf; sep[n] = c2; buf = ""; i++; continue }
                if (c1 == ";" || c1 == "|") { seg[++n] = buf; sep[n] = c1; buf = ""; continue }
                buf = buf c1
            }
            seg[++n] = buf; sep[n] = ""
            sw[n + 1] = 0
            for (k = n; k >= 1; k--) {
                sw[k] = 0
                if (sep[k] == "||" && swallows(seg[k + 1])) sw[k] = 1
                else if ((sep[k] == "&&" || sep[k] == "|") && sw[k + 1]) sw[k] = 1
            }
            killed = 0; out = pre
            for (k = 1; k <= n; k++) {
                w = word(seg[k])
                dead = killed || sw[k] || w == "echo" || w == "printf" \
                    || seg[k] ~ /(^|[ \t])--dry-run([^A-Za-z0-9_-]|$)/
                if (w == "exit" && seg[k] ~ /^[ \t({"\047]*exit([ \t]+0)?[ \t"\047)}]*$/ \
                    && (k == 1 || sep[k - 1] == ";")) killed = 1
                out = out (dead ? "" : seg[k]) sep[k]
            }
            print out
        }'
}

# The part of a pipeline file that runs and can fail the pipeline (#139,
# #171), as text: comments removed, then the never-run and cannot-fail parts
# above. The canary and pr steps are found in this text; the gates step is
# proven on top of it (gates_ci_unproven). A text check cannot see
# everything: a script that wraps a step, an expression-valued `if:` or
# `continue-on-error:`, conditional GitLab rules, and a job reached only
# through `extends:` or an alias are read as live. The CI run's own log is
# the proof that a step ran.
gates_ci_live() { # <file>
    local kind
    kind="$(_gates_ci_kind "$1")"
    if [[ "$kind" == jenkins ]]; then
        _gates_ci_uncomment "$1" | _gates_ci_jenkins_drop
    else
        _gates_ci_uncomment "$1" | _gates_ci_yaml_drop "$kind"
    fi | _gates_ci_segments "$kind"
}

# The gates step is proven, not merely found (#198): denying the inert forms
# one by one always leaves another (`; exit 0`, `&`, `if ...; then`). A step
# counts only in a form this check can show runs and fails the pipeline,
# and anything else is reported with what to change.
#
# The command, after one layer of matching quotes is removed, must be
# exactly `[bash ][./][.specify/gates/]verify.sh --boundary ci`, with an
# optional `--json` before or after `--boundary ci`: no other words, so no
# `;`, `&&`, `||`, `&`, `|`, `:`, `if`, `exit`, `true`, env prefix, second
# `--boundary` or `--dry-run`. It is the whole value of a GitHub `run:`, a
# GitLab `script:` or `before_script:` item (not `after_script:`, which
# cannot fail the job), or the string of a Jenkins `sh` step; or the last
# line of such a `|` block or `'''` string, with no heredoc, `trap` or
# `exit 0` before it and no `\` continuation into it.
_gates_ci_bare_awk='
    function bare(s,   q) {
        sub(/^[ \t]+/, "", s); sub(/[ \t]+$/, "", s)
        q = substr(s, 1, 1)
        if (length(s) >= 2 && (q == "\"" || q == "\047") && substr(s, length(s), 1) == q)
            s = substr(s, 2, length(s) - 2)
        return s ~ /^(bash[ \t]+)?(\.\/)?(\.specify\/gates\/)?verify\.sh[ \t]+(--json[ \t]+)?--boundary[ \t]+ci([ \t]+--json)?$/
    }
    # b[1..t-1] the script lines that run before the step in the same shell:
    # none can skip it (exit 0), take over its status (trap), make it text
    # (a heredoc) or continue into it (a trailing \).
    function before_ok(t,   k, p) {
        p = ""
        for (k = 1; k < t; k++) {
            if (b[k] ~ /<</ || b[k] ~ /(^|[^A-Za-z0-9_])trap([ \t]|$)/ \
                || b[k] ~ /(^|[^A-Za-z0-9_])exit([ \t]+0)?([^A-Za-z0-9_]|$)/ && b[k] !~ /(^|[^A-Za-z0-9_])exit[ \t]+[1-9]/) return 0
            if (b[k] ~ /[^ \t]/) p = b[k]
        }
        return p !~ /\\[ \t]*$/
    }
    # b[1..m] the lines of a script block, t the verify line: the bare
    # command, nothing after it, nothing unsafe before it.
    function last_line(t, m,   k) {
        if (!bare(b[t])) return 0
        for (k = t + 1; k <= m; k++) if (b[k] ~ /[^ \t]/) return 0
        return before_ok(t)
    }
'

# GitHub: the events a workflow runs on (its `on:` key), one per line.
_gates_ci_events() {
    awk '
        function indent(s) { return match(s, /[^ \t]/) ? RSTART - 1 : -1 }
        { n++; line[n] = $0 }
        END {
            for (i = 1; i <= n; i++) if (line[i] ~ /^["\047]?on["\047]?:/) break
            if (i > n) exit
            t = line[i]; sub(/^[^:]*:[ \t]*/, "", t)
            if (t != "") {
                gsub(/\[/, " ", t); gsub(/\]/, " ", t); gsub(/[",\047]/, " ", t)
                m = split(t, w, /[ \t]+/)
                for (k = 1; k <= m; k++) if (w[k] != "") print w[k]
                exit
            }
            ci = -1
            for (j = i + 1; j <= n; j++) {
                d = indent(line[j])
                if (d < 0) continue
                if (d == 0) break
                if (ci < 0) ci = d
                if (d != ci) continue
                t = line[j]; sub(/^[ \t]*(-[ \t]+)?/, "", t); sub(/:.*$/, "", t); gsub(/["\047]/, "", t)
                print t
            }
        }'
}

# GitHub and GitLab (stdin: the text after _gates_ci_yaml_drop): prints why
# no gates step is proven, nothing when one is.
_gates_ci_prove_yaml() { # <kind>
    awk -v kind="$1" "$_gates_ci_bare_awk"'
        function indent(s) { return match(s, /[^ \t]/) ? RSTART - 1 : -1 }
        function keycol(s) { match(s, /^[ \t]*(-[ \t]+)?/); return RLENGTH }
        # The key a GitLab list item on line i belongs to; PJ is its line.
        function parent(i,   j, d, di, s) {
            di = indent(line[i]); PJ = 0
            for (j = i - 1; j >= 1; j--) {
                d = indent(line[j])
                if (d < 0) continue
                if (d < di || (d == di && line[j] !~ /^[ \t]*-/)) { PJ = j; s = line[j]; sub(/^[ \t]*/, "", s); sub(/:.*$/, "", s); return s }
            }
            return ""
        }
        # Copies lines a..z into b[]; returns the count.
        function take(a, z,   k, m) { m = 0; for (k = a; k <= z; k++) b[++m] = line[k]; return m }
        function script_key(k) {
            if (k == "script" || k == "before_script") return 1
            if (k == "after_script" && why == "") why = "an after_script: command cannot fail the job — move it to script:"
            return 0
        }
        function proven(i,   s, v, j, h, th, m, t, k, d) {
            s = line[i]
            if (kind == "github" && match(s, /^[ \t]*(-[ \t]+)?run:[ \t]+/)) return bare(substr(s, RLENGTH + 1))
            if (kind == "gitlab" && match(s, /^[ \t]*(before_|after_)?script:[ \t]+/)) {
                v = substr(s, RLENGTH + 1); h = s; sub(/^[ \t]*/, "", h); sub(/:.*$/, "", h)
                return script_key(h) && bare(v)
            }
            # A GitLab script item runs in one shell with the items above it.
            if (kind == "gitlab" && match(s, /^[ \t]*-[ \t]+/)) {
                v = substr(s, RLENGTH + 1)
                if (!script_key(parent(i)) || !bare(v)) return 0
                return before_ok(take(PJ + 1, i))
            }
            # A line of a block scalar: its header is the nearest line above
            # indented less.
            for (j = i - 1; j >= 1; j--) { d = indent(line[j]); if (d >= 0 && d < indent(s)) break }
            if (j < 1) return 0
            h = line[j]; k = j + 1
            if (kind == "github") {
                if (h !~ /^[ \t]*(-[ \t]+)?run:[ \t]*\|[-+0-9]*[ \t]*$/) return 0
                th = keycol(h)
            } else if (h ~ /^[ \t]*-[ \t]+\|[-+0-9]*[ \t]*$/) {
                if (!script_key(parent(j))) return 0
                th = indent(h); k = PJ + 1
            } else if (h ~ /^[ \t]*(before_|after_)?script:[ \t]*\|[-+0-9]*[ \t]*$/) {
                v = h; sub(/^[ \t]*/, "", v); sub(/:.*$/, "", v)
                if (!script_key(v)) return 0
                th = keycol(h)
            } else return 0
            for (m = j + 1; m <= n; m++) { d = indent(line[m]); if (d >= 0 && d <= th) break }
            m = take(k, m - 1); t = i - k + 1
            return last_line(t, m)
        }
        { n++; line[n] = $0 }
        END {
            seen = 0; why = ""
            for (i = 1; i <= n; i++) {
                if (line[i] !~ /verify\.sh/) continue
                seen = 1
                if (proven(i)) exit
            }
            if (why != "") print why
            else if (seen) print "the verify.sh command is not in a form that can be shown to fail the job — make \"bash .specify/gates/verify.sh --boundary ci\" the whole command (or the last line of its script), with no ;, &&, ||, &, if, exit, true or second --boundary"
            else print "no step that runs and can fail calls it — the job is disabled, manual, never triggered or its failure is ignored (if: false, continue-on-error, allow_failure, when: manual|never, only:/except:, a hidden job, exit 0 before it)"
        }'
}

# Jenkins (stdin: the text after _gates_ci_jenkins_drop): prints why no gates
# step is proven, nothing when one is. An `sh` step inside `catchError`,
# `warnError` or `try` has its failure swallowed.
_gates_ci_prove_jenkins() {
    awk "$_gates_ci_bare_awk"'
        # The sh step opener on a line: sets SHQ (its quote, single or
        # triple) and SHP (the offset after it); 0 if there is none.
        function opener(s) {
            if (!match(s, /(^|[ \t{;(])sh[ \t]*(\([ \t]*)?(script[ \t]*:[ \t]*)?["\047]/)) return 0
            SHP = RSTART + RLENGTH; SHQ = substr(s, SHP - 1, 1)
            if (substr(s, SHP, 2) == SHQ SHQ) { SHQ = SHQ SHQ SHQ; SHP += 2 }
            return RSTART
        }
        # The step at offset p of the text runs inside a block that swallows
        # its failure.
        function wrapped(p,   o) {
            for (o in head)
                if (o + 0 < p && (!(o in cl) || cl[o] > p) \
                    && head[o] ~ /(^|[^A-Za-z0-9_])(catchError|warnError|try)[ \t\n]*(\([^)]*\))?[ \t\n]*$/) return 1
            return 0
        }
        function proven(i,   s, o, r, q, e, j, k, m, t) {
            s = line[i]
            if ((o = opener(s)) && length(SHQ) == 1) {
                r = substr(s, SHP); q = index(r, SHQ)
                if (!q || substr(r, q + 1) !~ /^[ \t]*\)?[ \t]*([;}].*)?$/ || !bare(substr(r, 1, q - 1))) return 0
                return !wrapped(start[i] + o)
            }
            for (j = i - 1; j >= 1; j--) if ((o = opener(line[j])) && length(SHQ) == 3) break
            if (j < 1 || index(substr(line[j], SHP), SHQ)) return 0
            q = SHQ; m = 0
            for (k = j + 1; k <= n; k++) {
                e = index(line[k], q)
                b[++m] = e ? substr(line[k], 1, e - 1) : line[k]
                if (k == i) t = m
                if (e) break
            }
            if (k < i || k > n) return 0
            if (!last_line(t, m)) return 0
            return !wrapped(start[j] + o)
        }
        { n++; line[n] = $0; start[n] = L; txt = txt $0 "\n"; L += length($0) + 1 }
        END {
            sp = 0; last = 0
            for (i = 1; i <= L; i++) {
                ch = substr(txt, i, 1)
                if (ch == "{") { head[i] = substr(txt, last + 1, i - last - 1); st[++sp] = i; last = i }
                else if (ch == "}") { if (sp) { cl[st[sp]] = i; sp-- } last = i }
            }
            seen = 0
            for (i = 1; i <= n; i++) {
                if (line[i] !~ /verify\.sh/) continue
                seen = 1
                if (proven(i)) exit
            }
            if (!seen) print "no stage that runs calls it — the stage is under when { expression { false } } or the step uses returnStatus: true"
            else print "no sh step is shown to fail the build — make sh \047bash .specify/gates/verify.sh --boundary ci\047 the whole step (or the last line of its script), outside catchError, warnError and try, without returnStatus"
        }'
}

# Why the gates step of a pipeline file is not proven; nothing when it is.
# A pipeline that sets GATES_SPEC_EXEC (skips the spec gate) or
# GATES_POLICY_FILE (replaces the policy) anywhere weakens every step.
gates_ci_unproven() { # <file>
    local kind text ev
    kind="$(_gates_ci_kind "$1")"
    text="$(_gates_ci_uncomment "$1")"
    if grep -qE '(^|[^A-Za-z0-9_])(GATES_SPEC_EXEC|GATES_POLICY_FILE)([^A-Za-z0-9_]|$)' <<<"$text"; then
        echo "it sets GATES_SPEC_EXEC or GATES_POLICY_FILE, which skip or replace gates — remove them from the pipeline"
        return 0
    fi
    case "$kind" in
        jenkins)
            _gates_ci_jenkins_drop <<<"$text" | _gates_ci_prove_jenkins
            ;;
        *)
            if [[ "$kind" == github ]]; then
                ev="$(_gates_ci_events <<<"$text")"
                if ! grep -qxE 'push|pull_request' <<<"$ev"; then
                    echo "the workflow does not run on push or pull_request — add one of them under on:"
                    return 0
                fi
            fi
            _gates_ci_yaml_drop "$kind" <<<"$text" | _gates_ci_prove_yaml "$kind"
            ;;
    esac
}

# Pipeline files that run the gates (a proven `gates` step). A step may live
# in any of them, so drift is judged over their union.
gates_ci_files() { # <root>
    local root="$1" f found=1
    while IFS= read -r f; do
        [[ -n "$f" ]] || continue
        if [[ -z "$(gates_ci_unproven "$root/$f")" ]]; then
            printf '%s\n' "$f"
            found=0
        fi
    done <<<"$(gates_ci_candidates "$root")"
    return "$found"
}

# Pipeline files that call verify.sh outside a comment yet run no proven
# gates step (#171, #198), as "<file>\t<why>" lines. Such a pipeline looks
# wired and may enforce nothing.
gates_ci_inert() { # <root>
    local root="$1" f why
    while IFS= read -r f; do
        [[ -n "$f" ]] || continue
        grep -q 'verify\.sh' <<<"$(_gates_ci_uncomment "$root/$f")" || continue
        why="$(gates_ci_unproven "$root/$f")"
        [[ -z "$why" ]] || printf '%s\t%s\n' "$f" "$why"
    done <<<"$(gates_ci_candidates "$root")"
    return 0
}

# The live text of every gates pipeline, one after the other; empty if none.
gates_ci_body() { # <root>
    local root="$1" files f
    files="$(gates_ci_files "$root")" || return 0
    while IFS= read -r f; do
        gates_ci_live "$root/$f"
    done <<<"$files"
}

# Template step ids the gates pipelines run.
gates_ci_present() { # <root>
    local body id re
    body="$(gates_ci_body "$1")"
    [[ -n "$body" ]] || return 0
    while IFS=$'\t' read -r id re; do
        [[ -n "$id" ]] || continue
        if grep -qE "$re" <<<"$body"; then printf '%s\n' "$id"; fi
    done < <(gates_ci_steps)
    return 0
}

gates_ci_missing() { # <root>
    local root="$1" id re acks present
    gates_ci_files "$root" >/dev/null || return 0
    acks="$(gates_holds_ci "$root")"
    present="$(gates_ci_present "$root")"
    while IFS=$'\t' read -r id re; do
        [[ -n "$id" ]] || continue
        [[ -n "$present" ]] && grep -qxF "$id" <<<"$present" && continue
        if [[ -z "$acks" ]] || ! grep -qxF "$id" <<<"$acks"; then
            printf '%s\n' "$id"
        fi
    done < <(gates_ci_steps)
}
