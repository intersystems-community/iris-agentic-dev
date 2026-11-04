#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# Install the shellcheck version the project pins, verified against pinned
# SHA-256 checksums (#138). The pin is the one the parity gate compares
# against (lib/attest.sh): package-lock.json, then mise (mise.lock, else
# the [tools] table of mise.toml and the other project configs mise
# documents), then .tool-versions (#260). The CI templates run this
# so CI resolves the same shellcheck as local runs; the parity gate fails a
# run whose shellcheck differs from the pin.
#
# The shellcheck project publishes no checksum files, so the checksums are
# pinned here, taken from GitHub's own asset digests, in two files: the
# shipped shellcheck.sha256, next to this script, comes with spec-gates and
# is replaced on upgrade; .specify/gates/shellcheck.local.sha256 in the project
# holds the project's own pins, for versions spec-gates does not ship, and
# no upgrade touches it. It is always the project's file, wherever this
# script runs from: in the spec-gates source tree the script sits in
# extension/runtime/, the packaging source, which --update never writes.
# A download that matches neither is never extracted. The asset follows the
# machine (linux/darwin, x86_64/aarch64); anything else fails clearly
# instead of installing a binary that cannot run.
#
# Usage:
#   install-shellcheck.sh [install-dir]   default /usr/local/bin (sudo if needed)
#   install-shellcheck.sh --update        pin the checksums of the pinned
#                                         version (writes them to
#                                         .specify/gates/shellcheck.local.sha256)
#   install-shellcheck.sh --pin           print the pinned version; exit 2
#                                         when nothing pins shellcheck
#
# --update reads each asset's digest from the GitHub release API and checks
# it against a fresh download before writing, so pinning another version is
# one reviewable change: the pin file plus shellcheck.local.sha256.
# GitHub publishes those digests only from shellcheck v0.11.0 on, so an
# older version cannot be pinned this way: --update fails, and its pins go
# into shellcheck.local.sha256 by hand, from assets verified some other way.
#
# The project root is $CLAUDE_PROJECT_DIR when set, else the git work tree
# this script sits in, else the working directory.
#
# Exit codes: 0 installed (or pinned), 1 failure, 2 (--pin only) no pin.
# A project that pins no shellcheck is a failure to install: the CI
# templates ask --pin first and fall back to the distro package on 2 only.
# Pin sources that disagree, or a mise file that cannot be read, fail
# every mode, so CI never falls back to a shellcheck nobody pinned.

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${CLAUDE_PROJECT_DIR:-$(git -C "$HERE" rev-parse --show-toplevel 2>/dev/null || pwd)}"
SUMS="$HERE/shellcheck.sha256"
LOCAL_SUMS="$ROOT/.specify/gates/shellcheck.local.sha256"
API="https://api.github.com/repos/koalaman/shellcheck/releases/tags"

die() {
    echo "install-shellcheck: $*" >&2
    exit 1
}

# One argument at most: --update or the install directory. Any other flag
# is a usage error, never a directory to install into (#172).
usage() {
    echo "install-shellcheck: $*" >&2
    echo "usage: install-shellcheck.sh [install-dir] | --update | --pin" >&2
    exit 1
}
[[ $# -le 1 ]] || usage "too many arguments"
case "${1:-}" in
    --update | --pin) ;;
    -*) usage "unknown option: $1" ;;
esac

# The pin lookup the parity gate uses, so the two cannot disagree.
[[ -f "$HERE/lib/attest.sh" ]] || die "$HERE/lib/attest.sh is missing (re-project the runtime)"
# shellcheck source=lib/attest.sh disable=SC1091
source "$HERE/lib/attest.sh"
PROBLEMS="$(gates_pin_problems shellcheck "$ROOT" | awk 'NF && !seen[$0]++')"
if [[ -n "$PROBLEMS" ]]; then
    while IFS= read -r p; do echo "install-shellcheck: $p" >&2; done <<<"$PROBLEMS"
    exit 1
fi
VERSION="$(gates_pin_version shellcheck "$ROOT")"
if [[ "${1:-}" == "--pin" ]]; then
    [[ -n "$VERSION" ]] || exit 2
    printf '%s\n' "$VERSION"
    exit 0
fi
[[ -n "$VERSION" ]] \
    || die "$ROOT declares no shellcheck version (in mise.toml, mise.lock or .tool-versions)"
BASE="https://github.com/koalaman/shellcheck/releases/download/v$VERSION"

for t in curl tar; do
    command -v "$t" >/dev/null 2>&1 || die "$t not found; install it and re-run"
done
if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    die "neither sha256sum nor shasum found; install one and re-run"
fi

TMP="$(mktemp -d 2>/dev/null || mktemp -d -t install-shellcheck)"
trap 'rm -rf "$TMP"' EXIT

if [[ "${1:-}" == "--update" ]]; then
    command -v jq >/dev/null 2>&1 || die "--update needs jq"
    curl -fsSL -o "$TMP/release.json" "$API/v$VERSION" \
        || die "cannot read the v$VERSION release from the GitHub API"
    : >"$TMP/sums"
    for plat in darwin.aarch64 darwin.x86_64 linux.aarch64 linux.x86_64; do
        asset="shellcheck-v$VERSION.$plat.tar.xz"
        digest="$(jq -r --arg a "$asset" '.assets[] | select(.name == $a) | .digest // empty' \
            "$TMP/release.json" | sed 's/^sha256://')"
        # The shellcheck releases carry GitHub asset digests only from
        # v0.11.0 on; nothing older can be pinned from the API (#203).
        [[ -n "$digest" ]] \
            || die "no published digest for $asset: GitHub publishes asset digests only from shellcheck v0.11.0 on, so --update cannot pin v$VERSION. Pin v0.11.0 or later, or verify the four assets yourself and add their '<sha256>  <asset>' lines to $LOCAL_SUMS"
        curl -fsSL -o "$TMP/$asset" "$BASE/$asset" || die "download of $asset failed"
        [[ "$(sha256 "$TMP/$asset")" == "$digest" ]] \
            || die "$asset does not match its published digest"
        printf '%s  %s\n' "$digest" "$asset" >>"$TMP/sums"
    done
    # Keep the pins for other versions; replace this version's.
    if [[ -f "$LOCAL_SUMS" ]]; then
        grep -vF "shellcheck-v$VERSION." "$LOCAL_SUMS" >>"$TMP/sums" || true
    fi
    { mkdir -p "$(dirname "$LOCAL_SUMS")" && cp "$TMP/sums" "$LOCAL_SUMS"; } \
        || die "cannot write $LOCAL_SUMS"
    echo "install-shellcheck: pinned shellcheck $VERSION in $LOCAL_SUMS"
    exit 0
fi

DEST="${1:-/usr/local/bin}"
case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=darwin ;;
    *) die "unsupported OS $(uname -s)" ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) die "unsupported architecture $(uname -m)" ;;
esac
asset="shellcheck-v$VERSION.$os.$arch.tar.xz"
expected=""
for f in "$SUMS" "$LOCAL_SUMS"; do
    [[ -f "$f" ]] || continue
    expected="$(awk -v a="$asset" '$2 == a { print $1; exit }' "$f")"
    [[ -n "$expected" ]] && break
done
[[ -n "$expected" ]] \
    || die "no pinned checksum for $asset (pin it: bash $HERE/install-shellcheck.sh --update)"

curl -fsSL -o "$TMP/$asset" "$BASE/$asset" || die "download of $asset failed"
actual="$(sha256 "$TMP/$asset")"
if [[ "$actual" != "$expected" ]]; then
    echo "install-shellcheck: checksum mismatch for $asset" >&2
    echo "  expected $expected" >&2
    echo "  got      $actual" >&2
    exit 1
fi
tar -xJf "$TMP/$asset" -C "$TMP" || die "cannot extract $asset (tar needs xz support: install xz-utils)"
[[ -f "$TMP/shellcheck-v$VERSION/shellcheck" ]] || die "$asset holds no shellcheck-v$VERSION/shellcheck"
SUDO=""
mkdir -p "$DEST" 2>/dev/null || true
if [[ ! -d "$DEST" || ! -w "$DEST" ]]; then
    command -v sudo >/dev/null 2>&1 || die "$DEST is not writable and sudo is not available"
    SUDO=sudo
fi
$SUDO mkdir -p "$DEST"
$SUDO install -m 0755 "$TMP/shellcheck-v$VERSION/shellcheck" "$DEST/shellcheck"
installed="$("$DEST/shellcheck" --version | awk '/^version:/ { print $2 }')"
[[ "$installed" == "$VERSION" ]] || die "installed $installed, pinned $VERSION"
echo "install-shellcheck: shellcheck $VERSION ($os.$arch) installed in $DEST, checksum verified"
