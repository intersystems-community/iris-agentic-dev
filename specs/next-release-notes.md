# Pending release notes (post-v0.9.5)

> Staging file. When the next tag is cut, this becomes the GitHub release body via
> `gh release edit <tag> --notes`. The constitution requires What's new / Notable fixes /
> Breaking changes plus a `v0.9.5...<tag>` compare link.
> There is no `CHANGELOG.md` in this repo; the release body is the changelog.
>
> Written against `v0.9.6` as the next tag. All 15 commits in `v0.9.5..79042ed` are covered
> here except `bbdd7a5`, which created this file.

## What's new

### Skills

- **Task → skill router** — the `iris-agentic-dev` skill now has a table mapping common
  tasks to the right skill(s) to load. Load it first and it tells you what else to load.
- **Skill breadcrumbs in tool descriptions** — 26 MCP tool descriptions now name the skill
  that covers them. An agent that reaches a tool cold, with no skill loaded, sees what to
  load next without having to know the skill catalog in advance.
- **Toolset awareness** — skills now document which tools require `toolset = "merged"` and
  what to do when a tool returns `NOT_IMPLEMENTED`.
- **Cross-links** — 10 skills gained a `## Related skills` section pointing at what to load
  next for the task at hand.
- **New tool coverage in existing skills** — `objectscript-navigation` now covers
  `iris_search`, `iris_doc_search`, `iris_execute_method`, `resolve_dynamic_dispatch`,
  `find_subclass_implementations` and `extract_message_map_routing`;
  `objectscript-debugging` covers `iris_debug`; `objectscript-coverage` documents the
  `iris_test(coverage=true)` inline shortcut; `ensemble-production` covers
  `iris_message_body`, `iris_business_rule_info`, `iris_production_diff`,
  `iris_lookup_manage/transfer` and `iris_credential_manage/list`.

Two new skills:

- **`objectscript-mac-routines`** — MAC routine structure, `#include`, `$ZTRAP`, extrinsic
  functions, `Quit` vs `Return`, and reading and writing `.mac` via Atelier. For legacy CHUI
  and pre-class code.
- **`objectscript-fewshot-fixes`** — seven worked ObjectScript bug fixes as
  Bug Pattern → Root Cause → Fix. It measured 53% on a 17-task suite, which is a different
  benchmark from the 22-task repair suite the leaderboard ranks on, so it is listed unranked
  rather than given a misleading position.

The pack is now 33 skills. A new test fails the build if the install manifest,
`skills.sh.json`, and the embedded catalog ever disagree with the directory again.

### VS Code extension

- The extension now publishes to the Marketplace automatically when a release is tagged.
- The `.vsix` no longer ships test files.
- Extension version 0.4.26.

## Notable fixes

### Skill pack

- **`skill install` 404'd on two skills.** The install manifest still listed
  `iris-vector-graph` and `iris-vector-rag`, which moved to their own repos in `1fb7a8c`.
  I removed both. Three skills that were on disk but in no manifest (`aihub-eap`,
  `iris-pgwire`, `irispython-connector`) were never installable at all; they are now.
- **`iris-coverage-run` was silently never installing.** Its `description` was an unquoted
  YAML scalar containing `Prerequisite: iris-coverage-setup`, and a bare `": "` parses as a
  nested mapping, so the CLI skipped the file. The warning was real but buried in a 33-item
  list. Quoting it fixed it, and it was the only such case in 91 `SKILL.md` files.
- **`skill_search` reported `count: 0` for skills that exist.** It only searched the
  `^SKILLS` global, which is empty with no IRIS connection, and said "no skill" when it
  meant "I looked in one place." The 31 pack skills are now compiled into the binary, and
  every response says which sources were searched, including the zero-hit case.
- **Two shipped Python samples could not run.** `import intersystems_iris.dbapi` raises
  `ModuleNotFoundError` (`dbapi` is an attribute, not a submodule), and the `pip install`
  line above it named a different distribution than the import. Fixed both.
- **`compile` and `introspect` skills used env vars that don't exist.** `IRIS_USER`,
  `IRIS_PASS` and `IRIS_NS` appear nowhere in the source; the real names are
  `IRIS_USERNAME`, `IRIS_PASSWORD` and `IRIS_NAMESPACE`. Every snippet silently fell back to
  defaults. `compile` also parsed a response field that is always empty for compiles, and
  routed ObjectScript `Do` through an Atelier endpoint that runs SQL only.
- **The MAC-routine skill referenced three tools that don't exist**
  (`iris_read_document`, `iris_write_document`, `iris_list_documents`). I rewrote it against
  the real Atelier endpoints.

### VS Code extension

- **Auto-install downloaded a release tag that has never existed.** The extension built its
  binary download URL from its own version, so 0.4.25 asked GitHub for
  `releases/download/v0.4.25/…` and got a 404. The extension version and the server binary
  version are separate sequences: the extension ships on its own cadence, the binary is
  tagged `v0.9.x` by the Rust release. Every user without a binary already on `PATH` hit
  this, which is the install path the release notes advertise as "no separate install
  needed." The version to download is now declared explicitly as
  `irisAgenticDev.serverVersion` in `package.json`, a release job fails the build if it
  doesn't match the tag being released, and a unit test fails if it drifts from the
  workspace `Cargo.toml`.
- **A `vscode-v*` tag cut a full binary release.** The release workflow triggered on `v*`,
  which also matches `vscode-v0.4.24`, so an extension-only version bump published binaries,
  a Docker image and a Homebrew formula update under a tag belonging to neither version
  sequence. The trigger is now `v[0-9]*`.
- **A failed Marketplace publish sank the whole release.** `vsce publish` refuses to
  republish an existing version, and a binary release usually carries an unchanged extension
  version, so that step failed by design. It no longer fails the release, and it skips with a
  warning when `VSCE_PAT` is unset. The `.vsix` is attached to the release either way.

### Test suite and CI

- **The test suite only passed under `--test-threads=1`.** 13 tests across 3 binaries
  mutated process-global env vars while `cargo test` ran them on parallel threads. They are
  now serialized per file. The baseline failed 1 run in 6; 20 runs were clean after.
- **The extension's tests had never run on CI.** The `vscode` module stub lived in a
  hand-created `node_modules/vscode/`, which `npm ci` wipes, so the test file failed to load
  and only passed on machines where someone had made the stub by hand. The stub is checked
  in now, `which` is stubbed too so the `PATH`-lookup tier doesn't depend on what the
  developer happens to have installed, and a CI job runs `tsc --noEmit` plus the tests on
  every push.
- **js-yaml** bumped to `>=5.2.2` (CVE: DoS in flow collections).
- **quinn-proto** bumped 0.11.14 → 0.11.16 (CVE: remote memory exhaustion).

## Internal (no user-facing change)

- **`mcp-skills/` deleted** — a marketplace prototype from 061 that never got a consumer.
  `mirror_to_iris` was a stub, nothing read the tree, and there were no commits since it
  landed. 21 of its 23 skills were stale duplicates, and the spec task claiming it was kept
  in sync was already false when it was checked off. It was also a hazard for
  `npx skills add`, which globs the whole checkout for `SKILL.md` and ignores both
  manifests, so any name existing only there would have won. I merged its unique content
  into the canonical skills first; nothing was lost.
- **Skill-regression CI failed with an unexplained 401** on the first fixture upload. The
  real cause was upstream: community images ship `_SYSTEM` with an expired password, so
  Atelier REST refuses every call until the expiry is cleared. Two guards existed for
  exactly this and both waved it through. The readiness probe counted HTTP 401 as "ready",
  and credential detection fell back to `_SYSTEM`/`SYS` after all four candidates failed.
  The probe now accepts only 200 and clears the expiry on a 401 before retrying, so the
  common case self-heals, and credential detection fails loudly instead of guessing.
- **The skill eval let the agent under test edit the benchmark it was being scored on.**
  `run_opencode` defaulted the subprocess `cwd` to `os.getcwd()`, which during a CI run is
  the repo checkout, and the agent runs with `--dangerously-skip-permissions`. Four of the
  nine call sites passed no `working_dir`, so the fire-rate stage ran in the repo.

  Given "Fix the method `EvalDemo.ListUtil.FilterNonEmpty`" and no fixture loaded into IRIS,
  the only copy of that method on disk is the one embedded in
  `tests/e2e/tasks/skills/targeted/LIST-ITERATE.yaml`. The agent edited it there and broke
  the `content: |` block scalar. The lift stage then failed parsing it 13 minutes later,
  which read like a harness bug.

  The `cwd` fallback is gone: with no `working_dir` the agent gets a fresh throwaway
  directory. That alone did not fix it. The agent also inherited `os.environ` wholesale, and
  under CI `PYTHONPATH` and `GITHUB_WORKSPACE` are both the repo root, so it followed those
  back to the checkout and rewrote the fixture from a sandboxed cwd. The environment handed
  to the agent is now scrubbed of every value naming the repo, keeping `PATH`, `HOME` and
  the caller's explicit vars.

  A new `test_task_corpus.py` parses every task and eval config in the unit-test step and
  checks that each referenced benchmark task resolves. Because it runs first, a parse
  failure later in the same run can only mean the corpus was modified mid-run, and
  `measure_lift` now says exactly that instead of surfacing a bare `ScannerError`.

- Skill-pack docs: fixed dead `light-skills/` paths in `docs/connecting.md` and three broken
  leaderboard links in `skills/README.md`.
- `light-skills/` orphan directory removed; its content was identical to
  `skills/skills/pyprod/`. Stale references fixed in `docs/skills.md` and `CONTRIBUTING.md`.
- Docker image size gate bumped 20 MB → 100 MB, to match binary growth since the gate was
  set.
- Global `~/.cargo/config.toml` Artifactory source redirect removed from the CI path.
- Stale draft release v0.7.0 deleted.
- `AGENTS.md`: "Codex" → "Claude Code". `docs/tools.md`: "powered by" → "using".

## Breaking changes

None.

**Full changelog:**
[`v0.9.5...v0.9.6`](https://github.com/intersystems-community/iris-agentic-dev/compare/v0.9.5...v0.9.6)
