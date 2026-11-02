# Feature Specification: winget package

**Feature Branch**: `152-winget`
**Created**: 2026-11-01
**Status**: Draft
**Input**: Tom, 2026-11-01: "let's spec and do a winget package asap". Issue #102 ("Add Windows installer / winget support").

## Context

macOS and Linux users install with `brew install intersystems-community/tap/iris-agentic-dev`. Windows users download `iris-agentic-dev-windows-x86_64.exe` from the release page, rename it, and put it on `PATH` by hand. Nothing from InterSystems is in winget's community repository (`microsoft/winget-pkgs`) today.

The Windows binary is a single executable that loads only Windows system DLLs (the UCRT `api-ms-win-crt-*` set, `kernel32`, `bcrypt`, `crypt32`, `ws2_32` and similar), so it fits winget's `portable` installer type as it is: winget downloads it, checks its SHA-256, and puts a `iris-agentic-dev` command on `PATH`. No installer, no zip.

## Decisions (Tom, 2026-11-01)

- **D1. Identifier**: `InterSystemsCommunity.IrisAgenticDev`. It matches the GitHub org, the Homebrew tap and the Marketplace publisher, and does not claim the corporate `InterSystems` publisher name in Microsoft's catalogue.
- **D2. No stored winget token for now.** The per-release automation (`winget-releaser`, `wingetcreate`, `komac`) needs a classic PAT with `public_repo`. A classic PAT on `isc-tdyar` can write to every public repository that account can push to, including this one, where `isc-tdyar` is the only ruleset bypass. Until a dedicated bot account with no other rights exists, each release is submitted from Tom's machine with one script. Automation is a follow-up once the bot's token is in the `release` environment.

## User Scenarios & Testing

### User Story 1 — Install on Windows with winget (P1)

A Windows user runs `winget install InterSystemsCommunity.IrisAgenticDev` and gets an `iris-agentic-dev` command on `PATH` that prints the released version.

**Independent test**: on a Windows runner, install from the generated manifests with `winget install --manifest`, then run `iris-agentic-dev --version`.

**Acceptance**:

1. Given the manifests for a released version, when `winget validate` reads them, then it reports no error.
2. Given those manifests, when `winget install --manifest <dir>` runs, then `iris-agentic-dev --version` prints `iris-agentic-dev <version>`.
3. Given a manifest whose `InstallerSha256` does not match the release asset, when winget installs it, then the install fails (winget's own hash check; the test asserts the generator takes the hash from the asset, never from a constant).

### User Story 2 — Submit a release to winget in one command (P1)

After a release, the maintainer runs `scripts/winget-submit.sh <version>`. It downloads `SHA256SUMS` from that GitHub release, generates the three manifests for the Windows asset, and prints the files and the next command. With `--submit` it pushes them to a branch of the maintainer's `winget-pkgs` fork and opens the pull request to `microsoft/winget-pkgs`.

**Acceptance**:

1. Given `1.7.1`, when the script runs without `--submit`, then it writes `manifests/i/InterSystemsCommunity/IrisAgenticDev/1.7.1/` with three files and makes no network write.
2. Given a version with no GitHub release, or a `SHA256SUMS` without the Windows asset, then the script exits non-zero and names what is missing.
3. Without `--submit` it never pushes or opens a pull request.

### User Story 3 — Manifests stay correct release after release (P2)

The manifests are generated from the version and the asset's hash, never edited by hand, and checked against the official winget manifest schema in CI.

**Acceptance**:

1. The generated YAML validates against the vendored 1.10.0 JSON schemas for `version`, `defaultLocale` and `installer`.
2. `PackageIdentifier` is `InterSystemsCommunity.IrisAgenticDev` in all three files; `PackageVersion` is the version passed in; `InstallerUrl` is the release asset URL for that tag; `InstallerType` is `portable`; `Commands` is `iris-agentic-dev`; `License` matches `Cargo.toml`.
3. The generator refuses a version that is not `X.Y.Z` and a hash that is not 64 hex characters.

## Requirements

- **FR-001**: `packaging/winget/winget_manifests.py` writes the three manifests (version, defaultLocale `en-US`, installer) for a given version, Windows SHA-256 and release date. Python standard library only, so it runs on a bare runner.
- **FR-002**: The installer manifest is `InstallerType: portable`, one `x64` installer whose URL is `https://github.com/intersystems-community/iris-agentic-dev/releases/download/v<version>/iris-agentic-dev-windows-x86_64.exe`, `Commands: [iris-agentic-dev]`.
- **FR-003**: `scripts/winget-submit.sh <version> [--submit]` takes the hash from that release's `SHA256SUMS` and verifies the downloaded asset against it before generating anything.
- **FR-004**: A CI job on `windows-latest` generates manifests for the latest published release, validates them and installs from them with winget, then runs `iris-agentic-dev --version`.
- **FR-005**: README and `docs/` give the winget command, marked as pending until the first submission is accepted; the release checklist gains the submit step.
- **FR-006**: No token is added to any workflow or environment by this spec.

## Out of scope

- The bot account and the `winget-releaser` job (follow-up after D2).
- Code signing of the Windows binary (owned by IPE; SmartScreen may still warn).
- A Windows arm64 build. winget installs the x64 binary on arm64 Windows under emulation.

## Risks

- Microsoft's validation pipeline may flag an unsigned binary for manual review. That slows the first submission, it does not block it.
- Pull requests to `microsoft/winget-pkgs` need the Microsoft CLA signed by the submitting account. Tom decides whether `isc-tdyar` signs it.
