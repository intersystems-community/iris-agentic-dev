#!/usr/bin/env bash
set -euo pipefail
# Generate the winget manifests for a published release and, with --submit, open the pull
# request to microsoft/winget-pkgs (spec 152).
#
#   scripts/winget-submit.sh 1.7.2            # download, verify, generate, print; writes nothing remote
#   scripts/winget-submit.sh 1.7.2 --submit   # also commit to the fork and open the PR
#
# The hash comes from the release's SHA256SUMS, and the downloaded .exe is checked against it
# before anything is generated. No token is read: --submit uses the gh login on this machine, and
# that login must be the fork owner (WINGET_FORK_OWNER, default isc-tdyar). The fork is
# microsoft/winget-pkgs under that account; the files go in through the contents API, because a
# clone of winget-pkgs is several hundred thousand files.

usage() { echo "usage: $0 <X.Y.Z> [--submit]" >&2; exit 2; }
die() { echo "winget-submit: $*" >&2; exit 1; }

[ $# -ge 1 ] || usage
VERSION="$1"
SUBMIT=0
case "${2:-}" in
  "") ;;
  --submit) SUBMIT=1 ;;
  *) usage ;;
esac
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version must be X.Y.Z, got '$VERSION'"

REPO=intersystems-community/iris-agentic-dev
ASSET=iris-agentic-dev-windows-x86_64.exe
IDENTIFIER=InterSystemsCommunity.IrisAgenticDev
PKG_DIR="manifests/i/InterSystemsCommunity/IrisAgenticDev"
PKG_PATH="$PKG_DIR/$VERSION"
FORK_OWNER="${WINGET_FORK_OWNER:-isc-tdyar}"
HERE="$(cd "$(dirname "$0")/.." && pwd)"

TMP_ROOT="${TMPDIR:-/tmp}"
WORK="$(mktemp -d "${TMP_ROOT%/}/winget-submit.XXXXXX")"
mkdir "$WORK/dl" "$WORK/out"

PUBLISHED="$(gh release view "v$VERSION" --repo "$REPO" --json publishedAt -q .publishedAt 2>/dev/null)" \
  || die "no GitHub release v$VERSION in $REPO"
RELEASE_DATE="${PUBLISHED%%T*}"
gh release download "v$VERSION" --repo "$REPO" -p SHA256SUMS -p "$ASSET" -D "$WORK/dl" \
  || die "could not download SHA256SUMS and $ASSET from v$VERSION"

LINE="$(grep -E "^[0-9a-f]{64}  $ASSET\$" "$WORK/dl/SHA256SUMS" || true)"
[ -n "$LINE" ] || die "SHA256SUMS of v$VERSION has no line for $ASSET"
(cd "$WORK/dl" && printf '%s\n' "$LINE" | shasum -a 256 -c -) >/dev/null \
  || die "$ASSET does not match its SHA256SUMS line"
SHA="${LINE%% *}"

python3 -I "$HERE/packaging/winget/winget_manifests.py" \
  --version "$VERSION" --sha256 "$SHA" --release-date "$RELEASE_DATE" --out "$WORK/out" >/dev/null
OUT="$WORK/out/$PKG_PATH"

echo "Manifests for $IDENTIFIER $VERSION (sha256 $SHA):"
for f in "$OUT"/*.yaml; do echo "  $f"; done
if command -v winget >/dev/null 2>&1; then
  winget validate --manifest "$OUT"
fi

if [ "$SUBMIT" = 1 ]; then
  LOGIN="$(gh api user -q .login)"
  [ "$LOGIN" = "$FORK_OWNER" ] \
    || die "gh is logged in as $LOGIN, not $FORK_OWNER; run: gh auth switch -u $FORK_OWNER"

  if gh api "repos/microsoft/winget-pkgs/contents/$PKG_DIR" >/dev/null 2>&1; then
    KIND="New version"
  else
    KIND="New package"
  fi
  TITLE="$KIND: $IDENTIFIER version $VERSION"

  gh repo view "$FORK_OWNER/winget-pkgs" >/dev/null 2>&1 \
    || gh repo fork microsoft/winget-pkgs --clone=false --default-branch-only
  gh repo sync "$FORK_OWNER/winget-pkgs" --branch master
  BASE="$(gh api "repos/$FORK_OWNER/winget-pkgs/git/ref/heads/master" -q .object.sha)"
  BRANCH="iris-agentic-dev-$VERSION"
  gh api -X POST "repos/$FORK_OWNER/winget-pkgs/git/refs" \
    -f ref="refs/heads/$BRANCH" -f sha="$BASE" >/dev/null

  for f in "$OUT"/*.yaml; do
    name="$(basename "$f")"
    gh api -X PUT "repos/$FORK_OWNER/winget-pkgs/contents/$PKG_PATH/$name" \
      -f message="$TITLE" -f branch="$BRANCH" -f content="$(base64 < "$f" | tr -d '\n')" >/dev/null
  done

  gh pr create --repo microsoft/winget-pkgs --base master --head "$FORK_OWNER:$BRANCH" \
    --title "$TITLE" \
    --body "$IDENTIFIER $VERSION, from https://github.com/$REPO/releases/tag/v$VERSION. Portable x64 executable; the SHA-256 is taken from the release's signed SHA256SUMS."
else
  echo "Dry run: nothing was pushed. Re-run with --submit to open the winget-pkgs pull request."
fi
