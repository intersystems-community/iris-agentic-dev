# Plan: winget package (152)

## Shape

| Piece                                  | What it does                                                                            |
| -------------------------------------- | --------------------------------------------------------------------------------------- |
| `packaging/winget/winget_manifests.py` | Writes the version, defaultLocale and installer manifests. Stdlib only.                 |
| `packaging/winget/schemas/1.10.0/`     | The official JSON schemas from `microsoft/winget-cli` (MIT), used by the tests.         |
| `scripts/winget-submit.sh`             | Per release: download, verify against `SHA256SUMS`, generate; `--submit` opens the PR.  |
| `ci.yml` job `winget-install`          | Real winget on `windows-latest`: validate, install, run, uninstall, wrong-hash refusal. |
| `tests/e2e/test_winget_manifests.py`   | Schema validation, field checks, input refusal, submit-script and CI-job guards.        |

## Choices

- **Manifest version 1.10.0.** komac writes 1.28.0 today, but 1.10.0 is accepted by `winget-pkgs`
  and read by every winget client in use. A later `komac update` raises it.
- **Portable, not zip.** The `.exe` is the release asset already; the DLL list (system DLLs and UCRT
  only) means no runtime has to come with it.
- **Contents API, not a clone.** `--submit` writes three files to a branch of the fork through
  `gh api`. A clone of `winget-pkgs` is several hundred thousand files.
- **The CI job installs the published release, not this commit's build.** winget needs an HTTPS URL
  and a hash, and the published asset is what users get. The generator is what changes per commit.
- **winget on the runner** is the image's own (1.11.510 on 2026-11-02, not in the image notes). The
  job falls back to Microsoft's `v1.29.380` release, hash-checked, if an image drops it.

## Follow-ups

- Dedicated bot account, its classic PAT as `WINGET_TOKEN` in the `release` environment, then a
  `publish-winget` job (`vedantmgoyal9/winget-releaser`, pinned by SHA) after `release`.
- Release body: add the `winget install` line once the first submission is merged.
