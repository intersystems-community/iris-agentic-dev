#!/bin/bash
# shellcheck shell=bash
# Shared formatter dispatch library.
# Sourced by post-edit.sh and format-changed.sh.
#
# Defines:
#   format_file <abs_path>         -- run the right formatter for $abs_path
#   find_prettier_root <abs_path>  -- locate nearest package.json
#
# INFRA-019:
#   - The caller must source policy.sh before sourcing this file if it
#     wants policy-driven exclude filtering. When the loader is unavailable
#     (missing-policy fallback path -- ADR-006), format_file still runs but
#     skips exclude filtering, matching alpha.11 behavior.
#   - format_file consults each tool's exclude list (prettier, markdownlint,
#     and shell scope) before invoking the underlying tool and short-circuits
#     silently if the project-relative path matches any glob.
#   - Globs use bash `[[ == pattern ]]` semantics. The helper normalizes
#     `**/` (leading) and `/**` (trailing) so policy conventions like
#     `**/node_modules/**` match a top-level `node_modules/foo.sh`.

find_prettier_root() {
    local file_path="$1"
    local dir
    dir=$(dirname "$file_path")
    local git_root
    git_root=$(git rev-parse --show-toplevel 2>/dev/null || echo "")
    while [[ "$dir" != "/" ]]; do
        if [[ -f "$dir/package.json" ]]; then
            echo "$dir"
            return 0
        fi
        # Stop at git root -- never walk above the project
        if [[ -n "$git_root" && "$dir" == "$git_root" ]]; then
            break
        fi
        dir=$(dirname "$dir")
    done
    local project_root="$git_root"
    if [[ -n "$project_root" ]]; then
        for subdir in "" "frontend" "web" "client" "app"; do
            local candidate="$project_root"
            [[ -n "$subdir" ]] && candidate="$project_root/$subdir"
            if [[ -f "$candidate/package.json" ]]; then
                echo "$candidate"
                return 0
            fi
        done
    fi
    return 1
}

# Project-root resolution shared by exclude lookups.
_gates_dispatch_project_root() {
    if [[ -n "${CLAUDE_PROJECT_DIR:-}" ]]; then
        printf '%s\n' "$CLAUDE_PROJECT_DIR"
        return 0
    fi
    git rev-parse --show-toplevel 2>/dev/null || pwd
}

# Test a single path against a single glob using bash `[[ == ]]`.
# Normalizes `**/` (leading) and `/**` (trailing) so the common policy
# convention works without globstar (which `[[ == ]]` ignores anyway).
_gates_glob_match() {
    local path="$1" glob="$2"
    [[ -z "$glob" ]] && return 1
    # shellcheck disable=SC2053,SC2295  # intentional unquoted glob pattern
    if [[ $path == $glob ]]; then
        return 0
    fi
    # Strip a single trailing /** so `foo/**` also matches `foo/bar/baz`
    # via the `foo/*` form, not just descendants of `foo`.
    if [[ "$glob" == */\*\* ]]; then
        local trimmed="${glob%/\*\*}"
        # shellcheck disable=SC2053,SC2295
        if [[ $path == $trimmed/* || $path == "$trimmed" ]]; then
            return 0
        fi
    fi
    # `**/x` should match top-level `x` too (no leading slash).
    if [[ "$glob" == \*\*/* ]]; then
        local rest="${glob#\*\*/}"
        # shellcheck disable=SC2053,SC2295
        if [[ $path == $rest || $path == */$rest ]]; then
            return 0
        fi
    fi
    return 1
}

# Return 0 (excluded) if $path matches any exclude glob for $tool.
# Tries both the absolute and project-relative path so policy globs that
# omit a leading slash still work. Returns 1 (not excluded) when the policy
# loader is unavailable -- the caller's fallback path.
_gates_path_excluded_for_tool() {
    local tool="$1" abs_path="$2"
    if ! command -v gates_policy_list >/dev/null 2>&1; then
        return 1
    fi
    local project_root rel_path
    project_root="$(_gates_dispatch_project_root)"
    rel_path="$abs_path"
    if [[ -n "$project_root" && "$abs_path" == "$project_root/"* ]]; then
        rel_path="${abs_path#"$project_root"/}"
    fi
    local glob
    while IFS= read -r glob; do
        [[ -z "$glob" ]] && continue
        if _gates_glob_match "$rel_path" "$glob" \
            || _gates_glob_match "$abs_path" "$glob"; then
            return 0
        fi
    done < <(gates_policy_list "$tool" exclude 2>/dev/null || true)
    return 1
}

# Run a formatter command and return its exit code. Stderr is swallowed when
# GATES_FORMAT_VERBOSE is unset to keep hook output quiet under normal runs.
_gates_run_tool() {
    if [[ -n "${GATES_FORMAT_VERBOSE:-}" ]]; then
        "$@"
    else
        "$@" 2>/dev/null
    fi
}

# format_file <abs_path>
# Returns 0 on success, on exclude-skip, or when no formatter is installed
# for the extension. Returns nonzero only when a present formatter actually
# fails on the path. Callers that care about severity (format-changed,
# post-edit) check this rc; legacy callers can ignore it.
format_file() {
    local file_path="$1"
    [[ -z "$file_path" ]] && return 0
    [[ ! -f "$file_path" ]] && return 0

    local ext="${file_path##*.}"
    local rc=0

    case "$ext" in
        ts | tsx | js | jsx | json | css | html | md | yaml | yml)
            # Prettier handles all of the above. .md additionally goes
            # through the markdownlint exclude list -- the two overlap.
            if _gates_path_excluded_for_tool prettier "$file_path"; then
                return 0
            fi
            if [[ "$ext" == "md" ]] \
                && _gates_path_excluded_for_tool markdownlint "$file_path"; then
                return 0
            fi
            if PRETTIER_ROOT=$(find_prettier_root "$file_path"); then
                if command -v npx >/dev/null 2>&1; then
                    _gates_run_tool npx --prefix "$PRETTIER_ROOT" prettier \
                        --write "$file_path" || rc=$?
                fi
            elif command -v prettier >/dev/null 2>&1; then
                _gates_run_tool prettier --write "$file_path" || rc=$?
            fi
            ;;
        py)
            if command -v ruff >/dev/null 2>&1; then
                _gates_run_tool ruff format "$file_path" || rc=$?
                _gates_run_tool ruff check --fix "$file_path" || rc=$?
            elif command -v black >/dev/null 2>&1; then
                _gates_run_tool black "$file_path" || rc=$?
            elif command -v autopep8 >/dev/null 2>&1; then
                _gates_run_tool autopep8 --in-place "$file_path" || rc=$?
            fi
            ;;
        rs)
            if command -v rustfmt >/dev/null 2>&1; then
                _gates_run_tool rustfmt "$file_path" || rc=$?
            fi
            ;;
        sh)
            # Shellcheck excludes also gate the .sh formatter pass: the
            # user's intent on the exclude list is "leave that file alone."
            # shfmt is a formatter rather than a linter, but the exclude
            # list is the single source of truth for shell scope.
            if _gates_path_excluded_for_tool shellcheck "$file_path"; then
                return 0
            fi
            if command -v shfmt >/dev/null 2>&1; then
                _gates_run_tool shfmt -w "$file_path" || rc=$?
            fi
            ;;
        go)
            if command -v gofmt >/dev/null 2>&1; then
                _gates_run_tool gofmt -w "$file_path" || rc=$?
            fi
            ;;
        rb)
            if command -v rubocop >/dev/null 2>&1; then
                _gates_run_tool rubocop -a "$file_path" || rc=$?
            fi
            ;;
        java | kt)
            if command -v google-java-format >/dev/null 2>&1; then
                _gates_run_tool google-java-format --replace "$file_path" || rc=$?
            fi
            ;;
    esac

    return "$rc"
}

# ---------------------------------------------------------------------------
# Check-mode (non-mutating) support, invoked by verify.sh's `none` orchestrator:
#
#   formatter-dispatch.sh --check --tool <prettier|markdownlint|shellcheck> \
#                         --project-root <dir>
#
# Expands the tool's include globs (minus its exclude globs) to a file list and
# runs the tool in check mode. Exit 0 = clean, nothing to check, or the tool is
# not installed; nonzero = the tool reported problems. bash 3.2-safe (no
# globstar, no mapfile).
# ---------------------------------------------------------------------------

# Print the project-relative paths (NUL-separated) that match <tool>'s include
# globs and none of its exclude globs, under <root>.
_gates_collect_files() { # <tool> <root>
    local tool="$1" root="$2" f rel inc matched
    while IFS= read -r -d '' f; do
        rel="${f#"$root"/}"
        matched=0
        while IFS= read -r inc; do
            [[ -z "$inc" ]] && continue
            if _gates_glob_match "$rel" "$inc"; then
                matched=1
                break
            fi
        done < <(gates_policy_list "$tool" include 2>/dev/null || true)
        [[ "$matched" -eq 1 ]] || continue
        _gates_path_excluded_for_tool "$tool" "$f" && continue
        printf '%s\0' "$rel"
    done < <(find "$root" \
        \( -name .git -o -name node_modules -o -name .venv -o -name target -o -name dist \) -prune \
        -o -type f -print0)
}

# Resolve the binary for <tool> under <root>, preferring the project-pinned
# install so the version is deterministic across the agent, git, and CI
# boundaries. Order: node_modules/.bin (lockfile-pinned) -> PATH -> none.
# Deliberately NOT `npx <tool>`: bare npx downloads "latest" (or a stale cached
# version), which manufactures a tool version and breaks parity. Echoes the
# binary path, or nothing when the tool is unavailable.
_gates_tool_bin() { # <binary-name> <root>
    local binname="$1" root="$2"
    if [[ -x "$root/node_modules/.bin/$binname" ]]; then
        printf '%s\n' "$root/node_modules/.bin/$binname"
    elif command -v "$binname" >/dev/null 2>&1; then
        printf '%s\n' "$binname"
    fi
}

# Machine-readable check-mode metadata, consumed by verify.sh for the
# attestation record (feature 001). One line on stderr; `bin` is last so a
# path containing spaces cannot corrupt the other fields.
_gates_emit_meta() { # <tool> <binname> <candidates> <checked> <skipped> <bin>
    printf '##gates-meta## tool=%s binname=%s candidates=%s checked=%s skipped=%s bin=%s\n' \
        "$1" "$2" "$3" "$4" "$5" "$6" >&2
}

# Run <tool> in check mode over its policy-selected files under <root>.
_gates_check_tool() { # <tool> <root>
    local tool="$1" root="$2"
    local files=()
    local rel
    while IFS= read -r -d '' rel; do
        files+=("$rel")
    done < <(_gates_collect_files "$tool" "$root")
    local count="${#files[@]}"

    local binname
    case "$tool" in
        prettier) binname="prettier" ;;
        markdownlint) binname="markdownlint-cli2" ;;
        shellcheck) binname="shellcheck" ;;
        *)
            echo "gates: unknown check tool: $tool" >&2
            return 1
            ;;
    esac
    local bin
    bin="$(_gates_tool_bin "$binname" "$root")"

    if [[ "$count" -eq 0 ]]; then
        echo "gates: $tool: no matching files"
        _gates_emit_meta "$tool" "$binname" 0 0 "" "$bin"
        return 0
    fi
    if [[ -z "$bin" ]]; then
        echo "gates: $binname not installed (node_modules/.bin or PATH); skipping" >&2
        _gates_emit_meta "$tool" "$binname" "$count" 0 "not-installed" ""
        return 0
    fi

    cd "$root" || return 1

    local rc=0
    case "$tool" in
        prettier)
            "$bin" --check "${files[@]}" || rc=$?
            ;;
        markdownlint)
            # --no-globs: markdownlint-cli2 UNIONS a config file's "globs"
            # with explicit file args, which would sweep in files the policy
            # excludes. The policy's file selection must be authoritative.
            "$bin" --no-globs "${files[@]}" || rc=$?
            ;;
        shellcheck)
            "$bin" "${files[@]}" || rc=$?
            ;;
    esac
    _gates_emit_meta "$tool" "$binname" "$count" "$count" "" "$bin"
    return "$rc"
}

# CLI entrypoint (only when executed, not when sourced by the hooks).
if [[ "${BASH_SOURCE[0]:-}" == "${0}" ]]; then
    _CHECK=0
    _TOOL=""
    _ROOT=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --check) _CHECK=1; shift ;;
            --tool) _TOOL="${2:-}"; shift 2 ;;
            --project-root) _ROOT="${2:-}"; shift 2 ;;
            *) echo "formatter-dispatch: unknown argument: $1" >&2; exit 2 ;;
        esac
    done

    if [[ "$_CHECK" != "1" || -z "$_TOOL" || -z "$_ROOT" ]]; then
        echo "usage: formatter-dispatch.sh --check --tool <tool> --project-root <dir>" >&2
        exit 2
    fi

    # The exclude/include lookups need the policy loader and a project root.
    export CLAUDE_PROJECT_DIR="$_ROOT"
    _lib_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    # shellcheck source=policy.sh disable=SC1091
    [[ -f "$_lib_dir/policy.sh" ]] && source "$_lib_dir/policy.sh"

    _gates_check_tool "$_TOOL" "$_ROOT"
    exit $?
fi
