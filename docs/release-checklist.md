# Release checklist

Run this before tagging any release. Each item is a gate — don't move on if it's red.

## 1. Version numbers (do these first — everything downstream depends on them)

- [ ] Bump `[workspace.package] version` in `Cargo.toml`
- [ ] Bump `"irisAgenticDev.serverVersion"` in `vscode-iris-agentic-dev/package.json`
- [ ] Bump `"version"` (VS Code extension semver) in `vscode-iris-agentic-dev/package.json`
- [ ] Bump `"version"` in `.claude-plugin/plugin.json`
- [ ] Bump the pinned tag in the direct-download commands in `README.md`,
      `docs/cursor-quickstart.md`, `skills/BENCHMARKING.md` and the `VERSION=` line in
      `skills/iris-agentic-dev-setup/SKILL.md` (`install_docs_pin_the_workspace_version` in
      `bin_unit` fails until they match)
- [ ] Verify all four agree — `serverVersion` must match the workspace version and the tag

```bash
grep '^version' Cargo.toml
node -e "const p=require('./vscode-iris-agentic-dev/package.json'); console.log(p.version, p.irisAgenticDev.serverVersion)"
node -e "const p=require('./.claude-plugin/plugin.json'); console.log(p.version)"
```

## 2. Fast checks, locally

- [ ] `cargo clippy --features testing --all-targets -- -D warnings`: zero warnings
- [ ] `cargo fmt --all -- --check`: zero diffs
- [ ] `RUST_TEST_THREADS=12 cargo test --features testing --test unit`, plus the `binary`,
      `bin_unit` and `bin_integration` targets
- [ ] The live tests for what this release changed, against `iris-dev-iris`, for example
      `cargo test --features testing --test integration -- --include-ignored --test-threads=1 <name>`
- [ ] `cargo audit`, the pinned prettier (`node_modules/.bin/prettier --check`), and
      `python3 scripts/gates/antipatterns.py`
- [ ] The full suite against `iris-dev-iris` only when the release touches tests CI skips (the
      `--skip` list in `.github/workflows/ci.yml`: AI Hub, Samples-BI, the 2026.2 container):

```bash
IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
  cargo test --features testing --no-fail-fast --test '*' -- --test-threads=1 --include-ignored
```

## 3. Docs and release notes

- [ ] Drain `specs/next-release-notes.md` into `docs/release-notes/vX.Y.Z.md`, then reset it to an
      empty staging state naming the release you just cut as the new baseline
- [ ] `docs/release-notes/vX.Y.Z.md` written
- [ ] Run `/no-ai-slop` detect on the release notes — address all findings
- [ ] All links in the release notes resolve (no 404s, no wrong anchors)
- [ ] Rebuild the extension's changelog from the release notes:
      `IAD_REGEN_CHANGELOG=1 cargo test --features testing --test unit test_changelog`. VS Code shows
      `vscode-iris-agentic-dev/CHANGELOG.md` as the extension's Changelog tab (#136), and
      `test_changelog` fails while it is behind `docs/release-notes/`
- [ ] Homebrew install command in README matches current tap formula
- [ ] `docs/connecting.md`, `docs/tools.md` updated if tool surface changed

The drain step is first because it is the one that gets skipped. `specs/next-release-notes.md` sat
at v1.0.0 through fourteen tags because nothing here told anyone to empty it. Note that
`.markdownlint-cli2.jsonc` ignores `specs/**`, so linting that file in place reports
"Linting: 0 file(s)" — `prettier --write` still applies.

Any measured figure in the notes (byte counts, tool counts, coverage percentages) has to be
re-measured against the tree you are tagging, not copied forward from the spec that introduced it.

## 4. CI on the release branch, before merging

Push the release branch (`fix/X.Y.Z` or `release/X.Y.Z`). Pushes to `fix/**` and `release/**`
run the same workflows master does: CI (test, the e2e matrix on a fresh community container,
coverage floors, doc-lint, the extension, cross-compile, Docker smoke), Quality Gates and the
dependency audit. Wait for all three to pass before merging.

- [ ] CI green on the release branch, including `e2e-tests` and `coverage-floors`
- [ ] Quality Gates green (prettier, markdownlint, shellcheck, spec, parity)
- [ ] Dependency audit green
- [ ] If a new `.rs` file was added, its floor entry is in `coverage-floors.toml`

This replaced about two hours of local runs per release: the CI e2e command on a fresh 2025.3
container, the full suite on `iris-dev-iris`, and coverage. They stood in for checks that ran
only on master, after 1.5.0 and 1.6.0 went to master red. They were also incomplete: the rm guard
stops a local Quality Gates run before shellcheck, and 1.6.4's first master push failed shellcheck.

## 5. Skill regression baseline

- [ ] Check the last scheduled `skill-regression` run — it is the nightly canary, and its verdict is
      `healthy` or it names what broke. It reports no lift, by design (spec 121 FR-016)
- [ ] Run the eval for the numbers, by hand, because nothing runs it on a schedule any more:

```bash
PYTHONPATH=. python -m tests.e2e.skill_eval --skill <name> --runs 5 \
  --output tests/e2e/results/ --model "openai/gpt-4.1"
```

- [ ] No skill shows `regression_flag = true` in the results
- [ ] If a skill improved, update `tests/e2e/results/skill-baseline.json`

There is no `run_skill_eval.sh`; this item named one for months and nobody noticed, which says
something about how often the gate was actually run. The full sweep is nine skills. For 1.5.0 it
took 90 minutes of sequential opencode sessions and $63.23 of `OPENAI_API_KEY`, measured from
opencode's own cost figures; `iris-ai-hub` alone was $16.06. It is the one gate here that costs
real money, so pass `--cap` to every `skill_eval` run: the per-session estimate has been 3 to 35
times low. That cost is why the nightly stopped running it: three nights in a row
it failed on Δs smaller than eight task-pairs can resolve. The nightly now answers only whether the
harness runs, whether the binary still advertises its tools, and whether `MCP-01`, `SKILL-01` and
`FULL-01` still pass.

## 6. Merge and push master

Merge the release branch into master and push master. The same workflows run again on the merge
commit; wait for them before tagging.

- [ ] CI, Quality Gates and the dependency audit green on master
- [ ] `skill-regression` (nightly) passing, or known-acceptable variance

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

- [ ] Comment on fixed issues saying what changed and which release carries it — **do not close them**.
      The reporter closes; if nobody answers within a week, close it then.
- [ ] Comment on contributor PRs
- [ ] Verify Homebrew tap: `brew tap intersystems-community/tap && brew install iris-agentic-dev` installs the new version
- [ ] Smoke test: `iris-agentic-dev --version` prints `vX.Y.Z`
