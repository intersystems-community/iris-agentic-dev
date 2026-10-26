# Verifying a release

Each release is built by `.github/workflows/release.yml` from a tag on master, and the workflow
signs what it publishes. You can check that a binary, the `.vsix` or the container image came from
that workflow, at that tag, and was not changed afterwards. The checks below need no key from us:
the signatures are keyless (Sigstore), and the certificate inside each one names the workflow
file and the tag that produced it.

Releases cut before this workflow change carry `SHA256SUMS` only. For those, use the checksum
step at the end of this page.

## Binaries and the `.vsix`, with the GitHub CLI

Download the file from the release page, then:

```bash
gh attestation verify iris-agentic-dev-macos-arm64 \
  --repo intersystems-community/iris-agentic-dev \
  --signer-workflow intersystems-community/iris-agentic-dev/.github/workflows/release.yml
```

A pass prints the workflow, the commit and the tag the file was built from. A file that was
replaced or modified after the build has no matching attestation, and the command fails.

The same command works for the `.vsix` and the Windows `.exe`. The CycloneDX SBOM is attested
against each binary as well:

```bash
gh attestation verify iris-agentic-dev-linux-x86_64 \
  --repo intersystems-community/iris-agentic-dev \
  --predicate-type https://cyclonedx.org/bom
```

## `SHA256SUMS`, with cosign

Each release carries `SHA256SUMS.sigstore.json`, a signature bundle for `SHA256SUMS`. Download
both, replace `vX.Y.Z` with the release you downloaded, then:

```bash
cosign verify-blob SHA256SUMS \
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity "https://github.com/intersystems-community/iris-agentic-dev/.github/workflows/release.yml@refs/tags/vX.Y.Z" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

Once `SHA256SUMS` verifies, the checksum step below proves the binaries match it.

## The container image

The image is signed by digest, and its build provenance is stored in GHCR beside it:

```bash
gh attestation verify oci://ghcr.io/intersystems-community/iris-agentic-dev:X.Y.Z \
  --repo intersystems-community/iris-agentic-dev

cosign verify ghcr.io/intersystems-community/iris-agentic-dev:X.Y.Z \
  --certificate-identity-regexp '^https://github\.com/intersystems-community/iris-agentic-dev/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

Image tags have no leading `v` (`1.6.4`, not `v1.6.4`). For a deployment, pull by the digest
these commands print, not by tag: a tag can be moved, a digest cannot.

## Checksums

```bash
# macOS
shasum -a 256 -c --ignore-missing SHA256SUMS
# Linux
sha256sum -c --ignore-missing SHA256SUMS
```

`--ignore-missing` skips the platforms you did not download. Every file you did download must
print `OK`.

On its own this only shows that the files match a checksum list published in the same place.
Someone able to replace a binary on the release page could replace `SHA256SUMS` too. That's what
the signed checks above are for.
