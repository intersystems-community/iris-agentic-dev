# Pending release notes (post-v0.9.5)

> Staging file. When the next tag is cut, this becomes the GitHub release body via
> `gh release edit <tag> --notes` — the constitution requires What's new / Notable fixes /
> Breaking changes plus a `v0.9.5...<tag>` compare link. Run `/no-ai-slop` over it first.
> There is no `CHANGELOG.md` in this repo; the release body is the changelog.
>
> Commits staged here so far: `ecfb7ed`, `7d7e6ef`, `6954af5`, `ca6aabc`.

## Skills

- **Task → skill router** — the `iris-agentic-dev` skill now has a table mapping common
  tasks to the right skill(s) to load. Load it first and it tells you what else to load.
- **Progressive disclosure via tool descriptions** — 26 MCP tool descriptions now include
  a `Skill:` breadcrumb. When an agent uses a tool cold (no skill loaded), it sees which
  skill to load next. Closes the discovery loop without requiring upfront skill knowledge.
- **Toolset awareness** — skills now document which tools require `toolset = "merged"` and
  what to do when a tool returns `NOT_IMPLEMENTED`.
- **Cross-links** — 10 skills updated with `## Related skills` sections so each skill
  points to what to load next for the current task.
- **New tool coverage in skills**: `objectscript-navigation` now covers `iris_search`,
  `iris_doc_search`, `iris_execute_method`, `resolve_dynamic_dispatch`,
  `find_subclass_implementations`, `extract_message_map_routing`;
  `objectscript-debugging` covers `iris_debug`; `objectscript-coverage` documents the
  `iris_test(coverage=true)` inline shortcut; `ensemble-production` covers
  `iris_message_body`, `iris_business_rule_info`, `iris_production_diff`,
  `iris_lookup_manage/transfer`, `iris_credential_manage/list`.

## Bug fixes (ecfb7ed)

- **`skill install` 404'd on two skills.** The install manifest still listed
  `iris-vector-graph` and `iris-vector-rag`, which moved to their own repos in `1fb7a8c`.
  Removed both. Three skills that were on disk but in no manifest — `aihub-eap`,
  `iris-pgwire`, `irispython-connector` — were never installable at all; they are now.
- **`iris-coverage-run` was silently never installing.** Its `description` was an unquoted
  YAML scalar containing `Prerequisite: iris-coverage-setup`, and a bare `": "` parses as a
  nested mapping, so the CLI skipped the file. The warning was real but buried in a 33-item
  list. Quoting it fixed it; it was the only such case in 91 `SKILL.md` files.
- **`skill_search` reported `count: 0` for skills that exist.** It only searched the
  `^SKILLS` global, which is empty with no IRIS connection, and said "no skill" when it
  meant "I looked in one place." The 31 pack skills are now compiled into the binary, and
  every response says which sources were searched — including the zero-hit case.
- **Two shipped Python samples could not run.** `import intersystems_iris.dbapi` raises
  `ModuleNotFoundError` (`dbapi` is an attribute, not a submodule), and the `pip install`
  line above it named a different distribution than the import. Both corrected.
- **`compile` and `introspect` skills used env vars that don't exist.** `IRIS_USER`,
  `IRIS_PASS` and `IRIS_NS` appear nowhere in the source — the real names are
  `IRIS_USERNAME`, `IRIS_PASSWORD`, `IRIS_NAMESPACE` — so every snippet silently fell back
  to defaults. `compile` additionally parsed a response field that is always empty for
  compiles, and routed ObjectScript `Do` through an Atelier endpoint that runs SQL only.
- **The MAC-routine skill referenced three tools that don't exist**
  (`iris_read_document`, `iris_write_document`, `iris_list_documents`). Rewritten against
  the real Atelier endpoints.
- **Test suite only passed under `--test-threads=1`.** 13 tests across 3 binaries mutated
  process-global env vars while `cargo test` ran them on parallel threads. Now serialized
  per file. Baseline failed 1 run in 6; 20 runs clean after.

## Skills added

- **`objectscript-mac-routines`** — MAC routine structure, `#include`, `$ZTRAP`, extrinsic
  functions, `Quit` vs `Return`, and reading/writing `.mac` via Atelier. For legacy CHUI
  and pre-class code.
- **`objectscript-fewshot-fixes`** — seven worked ObjectScript bug fixes as
  Bug Pattern → Root Cause → Fix. Measured 53% on a 17-task suite; that is a different
  benchmark from the 22-task repair suite the leaderboard ranks on, so it is listed
  unranked rather than given a misleading position.

Skill pack is now 33 skills, and a new test fails the build if the install manifest,
`skills.sh.json`, and the embedded catalog ever disagree with the directory again.

## Infrastructure

- **`mcp-skills/` deleted** — a marketplace prototype from 061 that never got a consumer
  (`mirror_to_iris` was a stub, nothing read the tree, no commits since it landed). 21 of
  its 23 skills were stale duplicates, and the spec task claiming it was kept in sync was
  already false when it was checked off. It was also a hazard for `npx skills add`, which
  globs the whole checkout for `SKILL.md` and ignores both manifests — any name existing
  only there would have won. Its genuinely unique content was merged into the canonical
  skills first; nothing was lost.
- Skill-pack docs: fixed dead `light-skills/` paths in `docs/connecting.md` and three
  broken leaderboard links in `skills/README.md`.
- **Skill-regression CI failed with an unexplained 401** on the first fixture upload. The
  real cause was upstream: community images ship `_SYSTEM` with an expired password, so
  Atelier REST refuses every call until the expiry is cleared. Two guards existed for
  exactly this and both waved it through — the readiness probe counted HTTP 401 as "ready",
  and credential detection fell back to `_SYSTEM`/`SYS` after all four candidates failed.
  The probe now accepts only 200 and clears the expiry on a 401 before retrying, so the
  common case self-heals; credential detection now fails loudly instead of guessing.
  (Internal-only, no user impact.)
- **The skill eval let the agent under test edit the benchmark it was being scored on.**
  `run_opencode` defaulted the subprocess `cwd` to `os.getcwd()`, which during a CI run is
  the repo checkout — and the agent runs with `--dangerously-skip-permissions`. Four of the
  nine call sites passed no `working_dir`, so the fire-rate stage ran in the repo. Given
  "Fix the method `EvalDemo.ListUtil.FilterNonEmpty`" and no fixture loaded into IRIS, the
  only copy of that method on disk is the one embedded in
  `tests/e2e/tasks/skills/targeted/LIST-ITERATE.yaml`, so the agent edited it there and
  broke the `content: |` block scalar. The lift stage then failed parsing it 13 minutes
  later, which read like a harness bug rather than the eval overwriting its own inputs.
  The `cwd` fallback is gone: with no `working_dir` the agent gets a fresh throwaway
  directory. That alone did not fix it — the agent also inherited `os.environ` wholesale,
  and under CI `PYTHONPATH` and `GITHUB_WORKSPACE` are both the repo root, so it followed
  those to the checkout and rewrote the fixture from a sandboxed cwd. The environment
  handed to the agent is now scrubbed of every value naming the repo, keeping `PATH`,
  `HOME` and the caller's explicit vars. A new `test_task_corpus.py` parses every task and
  eval config in the unit-test step and checks each referenced benchmark task resolves;
  because it runs first, a parse failure later in the same run can only mean the corpus
  was modified mid-run, and `measure_lift` now says exactly that instead of surfacing a
  bare `ScannerError`.
  (Internal-only, no user impact.)

- `light-skills/` orphan directory removed (content was identical to `skills/skills/pyprod/`)
- Stale `light-skills/` references fixed in `docs/skills.md` and `CONTRIBUTING.md`
- Docker image size gate bumped 20 MB → 100 MB (binary growth since gate was set)
- quinn-proto bumped 0.11.14 → 0.11.16 (CVE: remote memory exhaustion)
- Global `~/.cargo/config.toml` Artifactory source redirect removed from CI path
- Stale draft release v0.7.0 deleted
- `AGENTS.md`: "Codex" → "Claude Code"
- `docs/tools.md`: "powered by" → "using"
