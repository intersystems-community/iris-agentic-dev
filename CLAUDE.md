# iris-agentic-dev

MCP server that gives Claude Code tools for IRIS development — execute ObjectScript,
query globals, inspect productions, run tests, search code, manage skills, and more.

Written in Rust (2021 edition), two crates: `iris-agentic-dev-core` (tools + MCP server)
and `iris-agentic-dev-bin` (CLI entry point).

## Local dev container

| Container       | TCP port | Web port | Image                   | Atelier REST | WebGateway      |
| --------------- | -------- | -------- | ----------------------- | ------------ | --------------- |
| `iris-dev-iris` | 11975    | 52780    | `iris-community:2026.2` | yes (52780)  | none — PWS only |

**NoPWS note:** Community 2026.2 has PWS on 52780. Enterprise 2026.2.0AI builds do NOT
(DPP-1192) — `atelier_rest=false`, use `docker_only=true` for those.

Verify running before any IRIS-dependent work:

```bash
docker ps --filter name=iris-dev-iris
```

## Commands

```bash
cargo build                          # build
cargo clippy -- -D warnings          # lint (CI enforces clean)
cargo fmt --all                      # format (CI enforces clean)
cargo test --features testing        # unit tests (no IRIS required)
cargo test --features testing -- --include-ignored   # full suite (requires live container)
```

Always pass `--features testing`. Every aggregate test target declares
`required-features = ["testing"]`, and cargo skips them silently without it — a bare `cargo test`
reports green while never compiling anything. CI passes the flag on every job.

For integration/e2e tests always use `--test-threads=1`:

```bash
cargo test --features testing --test '*' -- --test-threads=1 --include-ignored
```

### Test target layout

Both crates declare a handful of `[[test]]` targets, not one per file. Each is an aggregator
(`tests/<dir>/main.rs`) whose only content is `mod` lines:

| Crate  | Target            | Files | What it holds                                      |
| ------ | ----------------- | ----- | -------------------------------------------------- |
| `core` | `unit`            | 100   | Pure logic — parsers, guards, gates, contracts     |
| `core` | `integration`     | 54    | Live IRIS via `iris-dev-iris`; must stay serial    |
| `core` | `binary`          | 14    | Spawn `iris-agentic-dev`, talk JSON-RPC over stdio |
| `core` | `misc`            | 15    | Older top-level `tests/*.rs`                       |
| `core` | `skills`          | 1     | Single file, so it stays its own target            |
| `bin`  | `bin_unit`        | 10    | CLI arg parsing, config resolution                 |
| `bin`  | `bin_integration` | 15    | Spawned-binary and live-IRIS CLI paths             |
| `bin`  | `bin_misc`        | 2     | Older top-level `tests/*.rs`                       |

Cargo runs test binaries strictly one after another and gives you no knob to change that, so a
per-file target charges every run a process spawn. At 233 targets across the two crates that was
~85% of a warm run: 254 s, of which about 40 s was actually running tests. The eight aggregates
bring the same suite in at ~65 s.

Almost all of that came from the core crate. Aggregating the bin crate's 26 targets cut its own
CPU time from 26.8 s to 11.6 s but moved the workspace wall clock barely at all, because the core
build dominates. It is here for the guard coverage and the consistency, not for the clock.

Two consequences:

- **Add a file, add its `mod` line.** An unlisted file compiles nowhere and its tests never run,
  and `cargo test` still reports ok. `test_test_target_layout.rs` fails when that happens — do not
  delete it.
- **Do not add a per-file `[[test]]` block.** `autotests = false` is set, so a file declared as its
  own target _and_ listed in an aggregator compiles twice. The same guard test catches it.

`unit` touches no IRIS and no shared env, so it can run parallel — 5.7 s against 18.6 s serial:

```bash
RUST_TEST_THREADS=12 cargo test --features testing --test unit
```

Every test target isolates its own process on load: a `#[ctor::ctor]` function in its `main.rs`
calls `testing::isolate_process()`, which points `HOME` at an empty temp directory and sets
`IAD_NO_KEYCHAIN=1`. A test never reaches the developer's keychain (a macOS approval dialog once
held a unit run for 34 minutes) or the real `~/.config/iris-agentic-dev`. A new test target needs
the same function; `test_isolation_141.rs` fails when one lacks it. A test that must use the real
keychain is behind the `real-keychain-tests` cargo feature (not `#[ignore]`, which `--include-ignored`
would run): `IAD_TEST_REAL_KEYCHAIN=1 cargo test --features testing,real-keychain-tests`.

A test changes the process environment only through `testing::EnvGuard` (`set`/`unset`, restored
on drop, one process-wide lock; `EnvGuard::clean()` also unsets every behavior variable for a
reader that must not see the shell's). A raw `set_var`/`remove_var` in `crates/*/tests/` fails
`test_env_restore_guard_141.rs` unless its line says `// env-restored: <how>`, and no test calls
`set_current_dir`: pass the directory to the code under test.

## Testing Philosophy — NON-NEGOTIABLE

IRIS is the only valid test object.

- **Always use a live IRIS container for tests.** Never mock IRIS, mock the Atelier
  HTTP client, or stub IRIS responses in unit tests. Mocked IRIS tests lie — they
  pass when the real implementation is broken.
- **Coverage goals require `--include-ignored`** against a live container. Unit tests
  covering pure logic (parsers, guards, gates) are fine, but anything that touches
  IRIS behaviour must run against real IRIS.
- **`--test-threads=1`** is required for all IRIS integration/e2e test runs to prevent
  env-var race conditions. This matters more since the files were aggregated into one
  binary per group: tests in the same target share a process, so a `set_var` in one is
  visible to the next.

## Test Coverage Policy — NON-NEGOTIABLE

Every new feature, tool, CLI flag, config field, and skill must have tests at the
right layer before the PR is considered done. "It compiles" is not enough.

**Three required layers:**

1. **Unit / TOML round-trip** — parse the config string (not a struct literal) and
   assert the resulting struct fields and env vars are correct. Catches serde silent-drop
   (the #110 pattern: field missing from struct, TOML key silently ignored).

2. **Binary invocation** (for any CLI flag or `mcp.rs` wiring) — spawn
   `iris-agentic-dev` as a subprocess, send `initialize` + `tools/list` or
   `tools/call` over stdio, assert on the JSON-RPC response. No live IRIS needed.
   Catches "flag exists but was never wired" (the #111 pattern: `self.config` ignored).
   Use `IAD_BINARY=./target/debug/iris-agentic-dev` and `#[ignore]`; CI builds the
   binary first and passes the env var.

3. **Live IRIS integration** (for any tool that calls IRIS) — `#[ignore]` test against
   `iris-dev-iris` (localhost:52780). Covers actual IRIS behavior, not just wiring.

**Version consistency:** every file that must agree with the workspace version
(`Cargo.toml`, `package.json`, `.claude-plugin/plugin.json`, etc.) must have an
explicit cross-file assertion test. Adding a new version-bearing file without adding
a test for it is a bug waiting to ship.

**When in doubt:** ask "if I changed this flag/field/file silently, would any test
fail?" If the answer is no, the test is missing.

## Release Notes & Changelog — NON-NEGOTIABLE

Before closing any release (tagging, publishing, merging release branch):

1. Run `/no-ai-slop` on all release notes and changelog entries.
2. Address every flagged item before publishing.
3. Release notes must read like a human wrote them for other humans — no filler phrases,
   no hedging, no passive voice, no "This release includes…" boilerplate.

## Issue Closure — NON-NEGOTIABLE

Comment on a fixed issue saying what changed. **Never close it.** Closing is the reporter's
move — they filed it, so they are the one who can confirm the thing they hit stopped
happening. If nobody answers within a week of the release carrying the fix, close it then.
Constitution → Development Workflow → Issue Closure.

## Docs

- `docs/connecting.md` — connection config (toml file, env vars)
- `docs/tools.md` — tool reference
- `docs/skills.md` — skill system
- `docs/troubleshooting.md` — common issues
- `docs/agent-attribution.md` — caller attribution, User-Agent marker, IRIS audit guide

## Active Technologies

- Python 3.11 + `anthropic`, `pyyaml`, `pytest`; results as JSON under `tests/e2e/results/`, no new dependency (118-skill-eval-harness-repair)

- Rust 2021 + `rmcp` 3.1.3, `schemars` 1 (`#[schemars(extend(...))]` for enums), `serde` — no new dependency (113-typed-tool-schemas)

- Rust 2021 + `rmcp`, `tokio`, `serde`/`serde_json`/`toml`; config in `.iris-agentic-dev.toml`, no database (085-write-gate-integrity)

- Dockerfile (no specific version), Bash (GHA steps), Markdown + `gcr.io/distroless/static-debian12` (base image), `docker/build-push-action@v6`, `docker/metadata-action@v5` (068-windows-docker)
- GHCR (`ghcr.io/intersystems-community/iris-agentic-dev`) (068-windows-docker)
- TypeScript 5, Node.js (VS Code extension host runtime) + VS Code API (`vscode`), Node built-ins (`https`, `fs`, (069-vscode-binary-install)
- Two files in `context.globalStorageUri` (VersionMarker + ManagedBinary) (069-vscode-binary-install)

## Recent Changes

- 141-security-hardening (1.7.0): the operator file `~/.iris-agentic-dev.toml` is always read and a workspace or `--config` file loosens nothing until the operator runs `iris-agentic-dev trust` at a terminal (trust covers each loosening setting and the server address it applies to); policies from the two files combine part by part into an effective policy (`policy/effective.rs`), and an undeclared template comes from the server's system mode; every call resolves its target server once in `tools/target.rs` (`dispatch_gated`, `CALL_TARGET`, accessors `target()`/`target_conn()`/`target_exec()`/`exec_context()`), every gate judges that server, and the CLI, compile hook and benchmark go through the same pipeline; every refusal carries `refusal {kind, server, lift, set_aside}` (kinds: acknowledgement required, operator limit, untrusted setting, fixed rule); `check_config` adds `config_sources`, `effective_policy`, `set_aside`, `identity`, `tls`, `audit`, `read_only`; agent code runs as the service account on every path (`tools/exec.rs`, login guard on docker/SSH terminals, `identity` in responses) or refuses; the compile-time gate covers preprocessor directives, braced parameters, projections, `.mac`/`.inc`, class XML and stored sources with their includes, and its refusal is `isError`; secrets go in the `X-IAD-Args` header, never in scratch-class source; one client builder (`iris/http.rs`) with per-connection TLS, keychain service `iris-agentic-dev`, discovery identifies IRIS before sending credentials; the HTTP transport checks Host, Origin and a bearer token; `read_only = "strict"`; `journal_search` walks the journal with scan and time caps (IRIS's `%SYS.Journal.File:Search` logs one `messages.log` line per record and can stop a container); setup skills bind 127.0.0.1 and set one generated password; spawned tests run with an empty `HOME`, a temp cwd and `IAD_NO_KEYCHAIN=1`; live tests that change accounts or system mode use the throwaway container `iad-throwaway-141` (`testing::ThrowawayIris`); glossary in `CONTEXT.md`; error codes in `docs/error-codes.md`
- skill-tiers (1.5.0): each bundled `SKILL.md` has `tier: core|extra|internal` (10 core, 23 extra, `opencode-introspect` internal); `plugin.json` names the 10 core paths plus `nopws-setup`; a bare `skill install` installs core, `--all` core and extra, a named skill any tier; the toml and skills.sh.json list core and extra; MCP `skill_list`/`skill_search` hide internal and carry `tier`, `skill_describe` finds all; the `iris-agentic-dev` skill has an `## Extra skills` index; a bare install prints a hint for managed extra/internal leftovers and `--prune` removes managed copies only, nothing automatic; a fetched file with no tier takes the embedded tier; `is_managed` now reads the whole frontmatter (the 512-byte prefix missed the marker on long-frontmatter skills, so re-installs called guardrails user-authored); guards in `test_skill_tiers.rs`, `test_skill_install_tiers.rs`, `bin_integration::test_skill_install_cli` (local HTTP stub serving the checkout); ADR `docs/adr/0001-skill-tiers.md`
- 132-aihub-139: `iris-ai-hub` 0.2.0 is the one iad-owned AI Hub skill (the vendored copy of upstream's skill and its pin test are gone); it names `github.com/intersystems-community/ai-hub-eap` (master) and a topic-to-file map for the docs, and tells the agent that the installed `%AI` classes decide names and signatures; new container `iad-aihub-iris` (IRISHealth 2026.3.0AI.139.0, web port 52781, gateway sidecar, key and CSP files in `~/.config/iris-agentic-dev/aihub139/`, never in the repo); live tests read `IAD_AIHUB_*`; claims from Gabriel Ing's hackathon skills enter only when a live test on 139 holds them (claim table in `specs/132-aihub-139/research.md`); D1 to D8 in `specs/132-aihub-139/drafts.md` are ai-hub-eap doc mismatches, drafted and not filed; B1: each iad process leaked two or three CSP sessions (one cookie jar per client), which put 139's key into `<LICENSE LIMIT EXCEEDED>` after two ladder tasks; every client now shares `csp_session::shared_cookie_store` and the exit path sends `?IRISLogout=end` to each session; B2: `iris_execute_method` returned an error `%Status` as raw bytes, and now returns `METHOD_RETURNED_ERROR` with the decoded text; B3: `iris_ws_exec` sent multi-line code as one terminal input, so only the first line ran, and now sends each line on its own; ladder tasks SKILL-22 to SKILL-25 (`--side train`, `--web-port`, a `teardown` script per task) and `tool_calls list <run_id>` for FR-016 call numbers; holdout 3 repeats: SKILL-24 tools 0/3, skill 2/3; SKILL-25 tools 0/3, skill 3/3; b=1 c=0, underpowered, no `needs_fix`, no new tool; the run cost $12.40 against a $3 cap, because `COST_PER_SESSION_USD` ($0.085, from 121) was 3 to 35 times low and nothing measured spend, so the train side stopped after 1 of 12 sessions (SC-006 unmet); `run_ladder(cap=, spent=)` and `--cap` now stop a run on opencode's measured `step_finish.cost`; on 2026-10-02 `iad-aihub-iris` moved to 2026.3.0AI.154.0 (139 kept stopped as `iad-aihub-iris-139`); 154 changed `%CanExecute`'s first argument to `toolref` (bare name) and dropped `StreamChat`'s `callbackMethod`, and a fresh container of either build needs `%ConfigStore.DescriptorManager` `RebuildRegistry()` before any `AI.LLM` Create (ERROR #26414); plan at `specs/132-aihub-139/plan.md`
- 131-mdx-cube-facts: `iris_info what=sa_schema` is described as the Studio Assist grammar for an XData namespace URL (was "SQL Analytics schema"); a `name` that is not an http(s) URL is refused with `INVALID_PARAMS` before any IRIS call, and a 404 or empty grammar returns `SA_SCHEMA_NOT_FOUND`, both pointing at `%DeepSee.Utils` `%GetCubeList`/`%GetDimensionList` via `iris_execute`; `sa_schema` had 404ed for every URL, because the slashes were encoded (`sa_schema_path` keeps them raw); fixture cube `tests/fixtures/mdx131/` (8 rows, `IadLive131Sales` + `IadLive131Other`, built and dropped by each test); `test_mdx_131_live` measures PR 142's `iris-mdx` claims, 17 tests, verdicts in `specs/131-mdx-cube-facts/research.md` (false: `%MDX()` on an axis returns empty, `MEASURES.MEMBERS` excludes `%COUNT`, the `MAX` same-axis fix); US5: `iris_execute` sends `&sql` untranslated on the HTTP path (IRIS compiles it in the class method; the 035 rewrite had read columns by expression text, so `COUNT(*) INTO` failed with `<PROPERTY DOES NOT EXIST>`) and translates only on docker exec, where the rewrite now reads by position, sets `SQLCODE`/`%msg`/`%ROWCOUNT` and has no braces; `test_exec_sql_131_live` covers both paths
- 130-content-skills: new skill `iris-query-plans` (plan lines, stale class-added index until `%BuildIndices`, `INSERT %NOINDEX`, `TUNE TABLE`, outlier selectivity); corrections in `objectscript-sql-patterns` (check `%Next(.sc)`), `objectscript-unit-test`/`iris-objectscript-eval` (`:Package.Class` patterns, only `Test*` runs), `ensemble-production` (`GetProductionStatus`, 5 states), `objectscript-tdd`, `objectscript-guardrails` (`New $NAMESPACE`), `iris-agentic-dev` (XML export under `.cls` name); each claim has a live test; ladder tasks SKILL-13 to SKILL-19 on the holdout (one run per arm: tools 7/7, tools+skill 4/7, underpowered, every skill "no lift claim"); `optimize run --surface content-descriptions` edits only the eight `CONTENT_SKILLS` descriptions and scores the routing corpus plus the author-written `tests/e2e/tasks/content/` corpus; iad bugs found are drafted in `specs/130-content-skills/drafts.md`; round 2: the ladder keeps per-session transcripts (`<run_id>.transcripts/`, git-ignored) and flags a skill by `ladder.needs_fix` (skill arm fails ≥2 scored where tools passes ≥2); `IsolatedEnv` now sets `OPENCODE_DISABLE_CLAUDE_CODE=1`, because opencode had been reading the operator's `~/.claude/CLAUDE.md` and `~/.claude/skills` into every harness session, so every skill/tools figure before it is void; the clean SKILL-16 re-run flagged nothing, no skill-arm session ever called the `skill` tool, and SKILL-16's `state = 2` mistake comes from `iris_macro` returning `{}` (drafts #3); round 3: the skill arm's prompt asks for its skill, a skill-arm session that never loads it is unscored (three in a row abort), every arm gets the same autonomy line, and the 118 skill-eval does the same; `iris_macro` now calls the real `getmacro*` routes (drafts #3 fixed locally); `resume --merge` keys on the repeat and the ladder takes `--start-repeat`; full ladder of 19 tasks × 3 repeats, all 57 skill-arm sessions loaded their skill: tools 11/19, tools+skill 12/19, b=3 c=2, underpowered; `needs_fix` flags `iris-query-plans` (SKILL-13: the agent rebuilds the index by hand instead of fixing the loader) and `objectscript-sql-patterns` (SKILL-09: the skill already states the rule, so no edit); `iris-query-plans` now has the `%NOINDEX` loader call `%BuildIndices`; SKILL-13 moved to train and SKILL-20 took its holdout slot (ladder 3/3 both arms, skill arm 6–8 calls against 18–34); isolated opencode sessions deny `external_directory`, because `--dangerously-skip-permissions` approved baseline greps on `/` that ran into the 300 s clock; `_reached_idle` also accepts a last `step_finish` with `reason: stop`, because `opencode run --format json` never emits `session.status`, so since 121 T021 every baseline session with no completed tool call (text answers included) had gone unscored while the skill arm never did; round 4: the judge reads tool args and results up to 8000 characters (they were cut to 120 and 200); SQLCODE-SILENT and SQLCODE-CHECK deleted, because their rubric said `If SQLCODE` fires on success; SKILL-21 (train) added; a skill-eval run is judged valid across the whole run and a thin skill is only left out of the write (FR-022); re-baseline `2026-09-29T031029`, every skill underpowered, three withdrawn with triage verdicts; sql-patterns §§3/5/9 corrected (`If SQLCODE` lumps 100 with errors, -114 is a lock timeout whose INTO variable still holds the row, `COUNT(*) INTO` sets 0); SKILL-09 ladder 2/3 both arms; a graded session now deletes every class it created under the fixture's packages (FR-023), because `Bench.Q2.CountOther`, written by one 2026-09-27 session, ended two later skill-arm sessions at 3 calls and caused round 3's sql-patterns `needs_fix` flag; plan at `specs/130-content-skills/plan.md`
- 129-tool-result-hints: hint wording moved to `src/tools/hints.toml` (matchers stay in `error_hints.rs`; a test checks the ids agree); five new rules (`sys_only_table`, `deep_package_table`, `double_quoted_string`, `reserved_word`, `nonstandard_insert`), each from a live capture; every hinted response gains `hint_ref {skill, section, why}`, dropped by `IAD_CODING_PACK=off`; hint text never names a skill; replay corpus `tests/e2e/tasks/hints/replay.jsonl` (live captures re-checked by `test_hints_replay_129_live`, `IAD_REGEN_HINTS=1` rewrites); `optimize run --surface hints` rewrites only `text`, scored on reach (right skill) and pass (the fix prepares or runs on live IRIS through the `tool` CLI); plan at `specs/129-tool-result-hints/plan.md`
- 118-skill-eval-harness-repair: the skill-eval scorer returns an unscored verdict instead of `score: 0` when it cannot reach the model, so a missing credential stops reading as total failure; pass rates count scored items only; a preflight makes one real scoring call before the first billable session; baseline file goes to schema 2 with provenance and merge-by-skill (the old `save_baseline` overwrote all nine entries from one run); the nightly also ran with no iad tools at all — `tests/e2e/isolated_env.py` hard-coded the Homebrew binary path, which does not exist on the runner; plan at `specs/118-skill-eval-harness-repair/plan.md`
- 114-tool-surface-discovery: `tool --list` / `tool <name> --schema` / `--json` read the tool router with no IRIS connection (7,648 B for the whole surface against 106,658 B for MCP `tools/list`); `tool_catalogue()` + `summarize_description()` in `tools/mod.rs` are the one source both arms read; `prose-only-enum` repaired — it was at zero because `TOOL_DESC` lacked `re.S` (so `iris_admin`'s backslash-continued description was never parsed) and `FIELD_DECL` could not read `pub r#type:`; twelve dispatcher value sets now declared, checked against the handler's own match arms rather than against prose; plan at `specs/114-tool-surface-discovery/plan.md`
- 113-typed-tool-schemas: per-tool params structs replace `AnyParams` so all 81 tools advertise their properties/types/enums; `#[serde(deny_unknown_fields)]` everywhere, so all 81 emit `additionalProperties: false`; one `UNKNOWN_PARAMETER` validation site in `call_tool` after `gate_check`; plan at `specs/113-typed-tool-schemas/plan.md`
- 089-iris-perf-monitoring: new `iris_mirror_status` tool (`%SYSTEM.Mirror` classmethods in %SYS); `iris_database_list` extended with `size_mb`/`free_space_mb`/`max_size_mb`/`free_pct` from `%SYS.DatabaseQuery:FreeSpace`; `my_access`/`capability_matrix` roles decoded from `$LB` via `$LISTTOSTRING`; Server Manager path prefix double-slash fixed; plan at `specs/089-iris-perf-monitoring/plan.md`
- 088-windows-vscdb-credential-fallback: `resolve_credential` on Windows now falls back to `state.vscdb` (safeStorage / AES-256-GCM) when Windows Credential Manager has no entry; `vscode_payload.rs` moved to core; DPAPI error message includes current Windows username; `check-sm-credential` delegates to core
- 087-execute-gate-bypass: `iris_execute` now enforces the destructive gate when `Kill ^<global>` appears literally in the code string; `contains_global_kill` in `write_gate.rs`; 22 unit tests + 4 live IRIS tests; indirection gap documented in spec and error message
- 086-agent-attribution-audit: caller marker in `User-Agent` on every IRIS-bound request, opt-in `%SYS.Audit` emission via `[policy.<server>].irisAudit`, `docs/agent-attribution.md`; plan at `specs/086-agent-attribution-audit/plan.md`
- 085-write-gate-integrity: write/destructive gates resolved as data and enforced once in `call_tool`; plan at `specs/085-write-gate-integrity/plan.md`
- 068-windows-docker: Added Dockerfile (no specific version), Bash (GHA steps), Markdown + `gcr.io/distroless/static-debian12` (base image), `docker/build-push-action@v6`, `docker/metadata-action@v5`

<!-- codebase-memory-mcp: Code Discovery Protocol -->

## Code Discovery Protocol (codebase-memory-mcp)

**ALWAYS use `codebase-memory-mcp` tools FIRST for any code exploration:**

- `search_graph(name_pattern/label/qn_pattern)` — find functions, classes, routes
- `trace_path(function_name, mode=calls|data_flow|cross_service)` — call chains
- `get_code_snippet(qualified_name)` — exact symbol source with precise line ranges
- `query_graph(query)` — complex Cypher patterns across the codebase graph
- `get_architecture(aspects)` — project structure overview
- `search_code(pattern)` — graph-augmented text search

Use `Grep`/`Glob`/`Read` freely for text, configs, and non-code files, and always
`Read` a file before editing it. If the project is not indexed yet, run
`index_repository` first.
