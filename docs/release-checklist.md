# Release checklist

Run this before tagging any release. Each item is a gate — don't move on if it's red.

## 1. Version numbers (do these first — everything downstream depends on them)

- [ ] Bump `[workspace.package] version` in `Cargo.toml`
- [ ] Bump `"irisAgenticDev.serverVersion"` in `vscode-iris-agentic-dev/package.json`
- [ ] Bump `"version"` (VS Code extension semver) in `vscode-iris-agentic-dev/package.json`
- [ ] Bump `"version"` in `.claude-plugin/plugin.json`
- [ ] Verify all four agree — `serverVersion` must match the workspace version and the tag

```bash
grep '^version' Cargo.toml
node -e "const p=require('./vscode-iris-agentic-dev/package.json'); console.log(p.version, p.irisAgenticDev.serverVersion)"
node -e "const p=require('./.claude-plugin/plugin.json'); console.log(p.version)"
```

## 2. Tests

- [ ] `cargo clippy --features testing --all-targets -- -D warnings` — zero warnings
- [ ] `cargo fmt --all -- --check` — zero diffs
- [ ] Full suite against a live `iris-dev-iris` container — all pass:

```bash
IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
  cargo test --features testing --no-fail-fast --test '*' -- --test-threads=1 --include-ignored
```

All four env vars are required. Without them `admin_e2e_tests` and the `params_batch*` live tests
assert `IRIS_HOST must be set` and fail, which reads as a regression and is not one. `--no-fail-fast`
matters too: without it the first failing target aborts the run and you never see the rest.

## 3. Coverage gate (merged/subprocess mode)

- [ ] `IRIS_HOST=localhost IRIS_WEB_PORT=52780 bash scripts/coverage.sh` — overall ≥ 88%
- [ ] No per-file floor violated (`scripts/check-coverage-floors.sh` exits 0)
- [ ] If a new `.rs` file was added, add its floor entry to `coverage-floors.toml`

## 4. Docs and release notes

- [ ] Drain `specs/next-release-notes.md` into `docs/release-notes/vX.Y.Z.md`, then reset it to an
      empty staging state naming the release you just cut as the new baseline
- [ ] `docs/release-notes/vX.Y.Z.md` written
- [ ] Run `/no-ai-slop` detect on the release notes — address all findings
- [ ] All links in the release notes resolve (no 404s, no wrong anchors)
- [ ] Homebrew install command in README matches current tap formula
- [ ] `docs/connecting.md`, `docs/tools.md` updated if tool surface changed

The drain step is first because it is the one that gets skipped. `specs/next-release-notes.md` sat
at v1.0.0 through fourteen tags because nothing here told anyone to empty it. Note that
`.markdownlint-cli2.jsonc` ignores `specs/**`, so linting that file in place reports
"Linting: 0 file(s)" — `prettier --write` still applies.

Any measured figure in the notes (byte counts, tool counts, coverage percentages) has to be
re-measured against the tree you are tagging, not copied forward from the spec that introduced it.

## 5. Skill regression baseline

- [ ] Run `tests/e2e/skill_eval/run_skill_eval.sh` or the GitHub Actions skill-regression workflow
- [ ] No skill shows `regression_flag = true` in the results
- [ ] If a skill improved, update `tests/e2e/results/skill-baseline.json`

## 6. CI pre-flight (before tagging)

Push to `master`, wait for the non-release workflow to go green:

- [ ] `build` job passes (Linux + macOS + Windows)
- [ ] `test` job passes
- [ ] `clippy` job passes
- [ ] `skill-regression` job passes (or known-acceptable variance)

## 7. Tag and release

```bash
git tag vX.Y.Z
git push github vX.Y.Z
```

- [ ] GitHub release workflow triggered (`Actions → Release`)
- [ ] `build-vsix` job passes (npm test includes `serverVersion.test.cjs` — it checks package.json matches Cargo.toml)
- [ ] `publish-vsix` job passes (VS Code Marketplace upload)
- [ ] `update-homebrew-tap` job passes
- [ ] GitHub release page shows six assets: the VSIX plus five bare binaries (linux-x86_64,
      linux-aarch64, macos-x86_64, macos-arm64, windows-x86_64.exe). They are not tarballs, and
      checksums live in the Homebrew formula rather than as release assets.

## 8. Post-release

- [ ] Close fixed issues with thank-you comments
- [ ] Comment on contributor PRs
- [ ] Verify Homebrew tap: `brew tap intersystems-community/tap && brew install iris-agentic-dev` installs the new version
- [ ] Smoke test: `iris-agentic-dev --version` prints `vX.Y.Z`
