#!/usr/bin/env bash
# install-state.sh -- where the gates extension stands in a project.
#
# Usage (sourced):
#   gates_extension_version <extension.yml>   # prints X.Y.Z (empty if unreadable)
#   gates_install_state <project-root>        # prints one state word, below
#
# States (specs/005-upgrade-safe-projection/data-model.md):
#   installed  registered in .specify/extensions/.registry, vendored copy
#              present, versions agree
#   dev        installed with `specify extension add --dev` (.specify-dev/)
#   dormant    installed, but .specify/gates/ has not been projected
#   removed    .specify/gates/ was projected by init or project.sh, but
#              the extension is gone (`extension remove` without the
#              matching `add`)
#   mismatch   registry and vendored copy disagree, or only one of them
#              exists
#   absent     nothing installed, nothing projected
#   unknown    a registry exists but jq is not installed, so it cannot be
#              read; nothing is known about the install (#122)
#
# "Projected" means a projection marker exists (.runtime-version, written by
# init and project.sh, or .projected.sha256). A runtime copied in by hand
# or by CI -- this repository runs its own gates that way -- carries no
# marker and is not an install, so it never reads as "removed".
#
# `installed` and `dev` can coexist with `dormant`; dormant wins only for a
# regular install, so a dev install is always reported as dev.

gates_extension_version() { # <extension.yml>
    [[ -f "${1:-}" ]] || return 0
    sed -n 's/^  version: *"\{0,1\}\([0-9][0-9.]*\)"\{0,1\} *$/\1/p' "$1" | head -n 1
}

gates_install_state() { # <project-root>
    local root="${1:?}"
    local vend="$root/.specify/extensions/gates"
    local reg="$root/.specify/extensions/.registry"
    local reg_version="" vend_version="" projected=0
    [[ -f "$root/.specify/gates/.runtime-version" || -f "$root/.specify/gates/.projected.sha256" ]] \
        && projected=1
    if [[ -f "$reg" ]] && ! command -v jq >/dev/null 2>&1; then
        echo unknown
        return 0
    fi
    if [[ -f "$reg" ]]; then
        reg_version="$(jq -r '.extensions.gates.version // empty' "$reg" 2>/dev/null || true)"
    fi
    vend_version="$(gates_extension_version "$vend/extension.yml")"
    if [[ -z "$reg_version" && -z "$vend_version" ]]; then
        if [[ "$projected" -eq 1 ]]; then echo removed; else echo absent; fi
        return 0
    fi
    if [[ -z "$reg_version" || -z "$vend_version" || "$reg_version" != "$vend_version" ]]; then
        echo mismatch
        return 0
    fi
    if [[ -d "$vend/.specify-dev" ]]; then
        echo dev
    elif [[ "$projected" -eq 0 ]]; then
        echo dormant
    else
        echo installed
    fi
}
