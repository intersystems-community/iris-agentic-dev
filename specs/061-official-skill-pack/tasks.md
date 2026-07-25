# Tasks: Official InterSystems Skill Pack (061)

**Input**: Design documents from `/specs/061-official-skill-pack/`
**Branch**: `061-official-skill-pack`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, contracts/cli.md ✅

**Tech stack**: Rust 2021 edition; `iris-agentic-dev-core` + `iris-agentic-dev-bin`; `reqwest` (existing); `dirs = "5"` (existing); no new deps.

**npx install** (FR-012): Compatibility with `npx skills add intersystems-community/iris-agentic-dev` (Vercel Labs `skills` CLI). Requires adding `metadata.json` per skill and `skills.sh.json` manifest. The `light-skills/` → `skills/` rename (T043) is already done.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Parallelizable — different files, no incomplete dependencies
- **[Story]**: [US1] file-based install · [US2] ecosystem discovery · [US3] IRIS mirror · [US4] npx npm package

---

## Phase 1: Setup

**Purpose**: Confirm workspace state and create module skeletons.

- [x] T001 Verify `dirs = "5"` present in `crates/iris-agentic-dev-core/Cargo.toml` (already confirmed — mark done immediately)
- [x] T002 Create `crates/iris-agentic-dev-core/src/skill_install/mod.rs` — empty module with `pub mod claude_code; pub mod opencode; pub mod copilot;` and re-exports
- [x] T003 [P] Create `crates/iris-agentic-dev-core/src/skill_install/claude_code.rs` — empty file with module doc comment
- [x] T004 [P] Create `crates/iris-agentic-dev-core/src/skill_install/opencode.rs` — empty file with module doc comment
- [x] T005 [P] Create `crates/iris-agentic-dev-core/src/skill_install/copilot.rs` — empty file with module doc comment
- [x] T006 Create `crates/iris-agentic-dev-bin/src/cmd/skill.rs` — empty `SkillCommand` enum stub with `install`, `list`, `status` subcommands (clap derive)
- [x] T007 Wire `skill.rs` into `crates/iris-agentic-dev-bin/src/main.rs` — add `Skill(SkillCommand)` variant to top-level command enum (compile-only, no logic yet)
- ~~T008 Create `npm/iris-skills/` npm package~~ **DROPPED** — design pivot to `npx skills add` (Vercel Labs `skills` CLI) means no custom npm package is needed; the `skills` CLI handles install to 70+ agents natively once the repo follows the pack format.

---

## Phase 2: Foundational

**Purpose**: Core data types, path resolution, and fetch mechanism — blocks all user story phases.

**⚠ CRITICAL**: No user story work begins until this phase is complete.

- [x] T009 [P] Write unit tests for `AgentKind` enum and `InstallTarget` path resolution in `crates/iris-agentic-dev-core/src/skill_install/mod.rs` — assert correct paths for Claude Code, OpenCode on macOS/Linux/Windows using temp `HOME` override
- [x] T010 [P] Write unit tests for `managed_by` marker detection in `crates/iris-agentic-dev-core/src/skill_install/mod.rs` — assert: file with marker → overwrite; file without marker → skip; missing file → write
- [x] T011 Implement `AgentKind` enum (`ClaudeCode`, `OpenCode`, `Copilot`) and `InstallTarget` struct (agent, base_path, skill_name → target_path) in `crates/iris-agentic-dev-core/src/skill_install/mod.rs`; use `dirs::home_dir()` and `dirs::config_dir()` for cross-platform paths
- [x] T012 Implement `managed_by` marker detection: `fn is_managed(path: &Path) -> bool` — read first 512 bytes, look for `managed_by: "iris-agentic-dev"`; returns `false` if file missing in `crates/iris-agentic-dev-core/src/skill_install/mod.rs`
- [x] T013 Implement `InstallOutcome` enum (`Written`, `Updated`, `Skipped`, `Failed`) and `SkillInstallResult` struct in `crates/iris-agentic-dev-core/src/skill_install/mod.rs`
- [x] T014 Run T009 + T010 tests — all must pass before proceeding

**Checkpoint**: `cargo test -p iris-agentic-dev-core skill_install` green — path resolution and collision detection verified.

---

## Phase 3: US1 — File-Based Install (P1) 🎯 MVP

**Goal**: `iris-agentic-dev skill install` writes skill files to `~/.claude/skills/` and `~/.config/opencode/skills/` from `raw.githubusercontent.com`. No IRIS required.

**Independent Test**: On a machine with no IRIS configured, run `iris-agentic-dev skill install` and confirm skill files exist at the correct paths with `managed_by` marker present.

### Tests for US1

- [x] T015 [P] [US1] Write unit test: `skill install` with no args fetches pack manifest and returns list of skill names — mock HTTP, assert names list non-empty in `crates/iris-agentic-dev-core/tests/unit/test_skill_install.rs`
- [x] T016 [P] [US1] Write unit test: single skill install writes file to temp dir, marker present, outcome = `Written` in `crates/iris-agentic-dev-core/tests/unit/test_skill_install.rs`
- [x] T017 [P] [US1] Write unit test: re-install of managed file → outcome = `Updated`; user-authored file (no marker) → outcome = `Skipped` in `crates/iris-agentic-dev-core/tests/unit/test_skill_install.rs`
- [x] T018 [P] [US1] Write unit test: `--dry-run` flag — no files written, outcomes report what would happen in `crates/iris-agentic-dev-core/tests/unit/test_skill_install.rs`
- [x] T018b [P] [US1] Write unit test: `--agent all` installs to all three targets (ClaudeCode, OpenCode, Copilot) in one call — assert three `SkillInstallResult` entries returned, one per agent, each with outcome `Written`; use temp dir overrides for all three paths in `crates/iris-agentic-dev-core/tests/unit/test_skill_install.rs`
- [ ] T019 [US1] Write E2E test `#[ignore]` — runs `iris-agentic-dev skill install` against live GitHub, asserts skill files written to temp dir override paths, marker present, exit 0 in `crates/iris-agentic-dev-core/tests/integration/test_skill_install_e2e.rs`

### Implementation for US1

- [x] T020 [US1] Implement `SkillRegistry::fetch_pack_manifest()` in `crates/iris-agentic-dev-core/src/skill_install/mod.rs` — fetches `iris-agentic-dev.toml` from `raw.githubusercontent.com/intersystems-community/iris-agentic-dev/HEAD/iris-agentic-dev.toml` via raw GitHub URL (no API, no auth), parses `[provides] skills = [...]` list to get skill names; reuse `fetch_manifest` pattern from `crates/iris-agentic-dev-core/src/skills/mod.rs:118`
- [x] T021 [US1] Implement `install_skill(name, content, targets, force) -> Vec<SkillInstallResult>` in `crates/iris-agentic-dev-core/src/skill_install/mod.rs` — creates dirs, checks marker, writes file, returns outcome per target
- [x] T022 [US1] Implement Claude Code writer in `crates/iris-agentic-dev-core/src/skill_install/claude_code.rs` — resolves `~/.claude/skills/<name>/SKILL.md` via `dirs::home_dir()`; Windows: `%APPDATA%\Claude\skills\`
- [x] T023 [US1] Implement OpenCode writer in `crates/iris-agentic-dev-core/src/skill_install/opencode.rs` — resolves `~/.config/opencode/skills/<name>/SKILL.md` via `dirs::config_dir()`
- [x] T024 [US1] Implement `SkillInstallCommand::run()` in `crates/iris-agentic-dev-bin/src/cmd/skill.rs` — parses `--agent`, `--dry-run`, `--force`, optional `[SKILL...]` args; calls `install_skill`; prints progress per contracts/cli.md output format; exits 0/1
- [x] T025 [US1] Run T015–T018 unit tests — all must pass
- [ ] T026 [US1] Run T019 E2E test manually (`cargo test --ignored`) — skill files written, marker present, exit 0

**Phase Gate**: T019 E2E passes. `iris-agentic-dev skill install` works end-to-end with no IRIS.

---

## Phase 4: US2 — Ecosystem Discovery (P2)

**Goal**: Skill pack includes content for ecosystem projects (iris-vector-graph, iris-embedded-python-wrapper, etc.) so agents can answer discovery questions and guide installation of those projects.

**Independent Test**: With pack installed, ask agent "I need vector search in IRIS" — response names `iris-vector-graph` and describes how to obtain it.

### Tests for US2

- [x] T027 [P] [US2] Write unit test: each ecosystem SKILL.md in `skills/skills/` passes frontmatter validation — has `name`, `description`, `managed_by` fields in `crates/iris-agentic-dev-core/tests/unit/test_skill_frontmatter.rs`
- [ ] T028 [P] [US2] Write benchmark task file `benchmark/021/tasks/skill-discovery-001.json` — agent challenge: "I need vector search in IRIS, what should I use?"; success criteria: response names `iris-vector-graph`

### Implementation for US2

- [x] T029 [P] [US2] Write/update `skills/skills/iris-vector-graph/SKILL.md` — what it does, how to install (`zpm "install iris-vector-graph"`), key usage pattern; add `managed_by: iris-agentic-dev` frontmatter
- [x] T030 [P] [US2] Write/update `skills/skills/iris-embedded-python/SKILL.md` — same structure for embedded Python wrapper
- [x] T031 [P] [US2] Write/update `skills/skills/iris-vector-rag/SKILL.md` — same structure for vector RAG
- [ ] T032 [US2] ~~Sync ecosystem skill files to `mcp-skills/` mirror directory~~ **ABANDONED.** This was marked
      done but never happened: 13 of 23 shared skills already differed at the commit that claimed it, and the
      two trees have since diverged bidirectionally (15 of 21 differ as of 2026-07-25). `mcp-skills/` was a
      rename artifact from the old flat `skills/` import, not a mirror — it has zero consumers in Rust, CI,
      Docker, or packaging, and zero commits since 061. The `^SKILLS` mirror it was meant to feed is a stub
      (`mirror_to_iris` bails with `IRIS_UNREACHABLE: not yet implemented`) and when implemented will mirror
      from _installed_ skills, not a repo directory. `skills/skills/` is the single authoritative tree.
      `mcp-skills/` is to be harvested for orphaned content, then deleted.
- [x] T033 [US2] Run T027 frontmatter validation tests — all ecosystem skills pass
- [ ] T034 [US2] Run benchmark task T028 A/B (baseline vs skill installed) — record lift in `specs/061-official-skill-pack/lift-results.md`

**Phase Gate**: T027 passes; T034 lift ≥ +0.20 recorded.

---

## Phase 5: US3 — IRIS Mirror (P3)

**Goal**: `iris-agentic-dev skill install --mirror-to-iris` explicitly copies installed skill content into `^SKILLS` on a connected IRIS instance. Requires live IRIS — entirely optional.

**Independent Test**: With skill pack installed file-side and live IRIS available, run `skill install --mirror-to-iris`; verify `^SKILLS("pyprod")` contains the skill content.

### Tests for US3

- [x] T035 [P] [US3] Write unit test: `--mirror-to-iris` without IRIS configured returns `IRIS_UNREACHABLE` error, not a panic, in `crates/iris-agentic-dev-core/tests/unit/test_skill_install.rs`
- [ ] T036 [US3] Write E2E test `#[ignore]` — with live IRIS, `skill install --mirror-to-iris` writes skill to `^SKILLS`, verifiable via `iris_global` read in `crates/iris-agentic-dev-core/tests/integration/test_skill_install_e2e.rs`

### Implementation for US3

- [x] T037 [US3] Add `--mirror-to-iris` flag to `SkillInstallCommand` in `crates/iris-agentic-dev-bin/src/cmd/skill.rs`
- [ ] T038 [US3] Implement `mirror_to_iris(skills: &[InstalledSkill], iris: &IrisConnection)` in `crates/iris-agentic-dev-core/src/skill_install/mod.rs` — writes each skill content to `^SKILLS(name)` via `iris_execute`; returns `IRIS_UNREACHABLE` if no connection
- [x] T039 [US3] Run T035 unit test — IRIS_UNREACHABLE path verified
- [ ] T040 [US3] Run T036 E2E test manually — mirror confirmed in `^SKILLS`

**Phase Gate**: T036 E2E passes with live IRIS.

---

## Phase 6: US4 — npx skills Compatibility (P2)

**Goal**: `npx skills add intersystems-community/iris-agentic-dev` installs the full skill pack with no prior `iris-agentic-dev` binary and no IRIS instance — works for Claude Code, OpenCode, Copilot, Cursor, and 70+ other agents via the `skills` CLI.

**Independent Test**: On a clean machine with only Node.js installed, `npx skills add intersystems-community/iris-agentic-dev` completes successfully and skill files exist at `~/.claude/skills/`.

### Tests for US4

- [ ] T041 [P] [US4] Smoke test: `npx skills add intersystems-community/iris-agentic-dev --list` exits 0 and lists all skills without installing (the `--list` flag is the verified non-destructive discovery check; `--dry-run` does not exist in the `skills` CLI); record output in `specs/061-official-skill-pack/lift-results.md`
- [ ] T042 [US4] Smoke test: `npx skills add intersystems-community/iris-agentic-dev` on a clean temp env writes skill files to `~/.claude/skills/` with correct frontmatter (manual verification)

### Implementation for US4

- [x] T043 [US4] Rename `light-skills/` → `skills/` at repo root; update all internal references (`tests/`, `specs/`, Rust source paths) — this is the primary structural change for `skills` CLI compatibility ✓ done
- [x] T044 [P] [US4] Add `metadata.json` to each skill under `skills/skills/<name>/metadata.json` — fields: `version`, `organization: "InterSystems Community"`, `abstract` (one paragraph), `references` (project URLs); **optional — for skills.sh web registry display only, not required for `npx skills add` to work**; priority order: (1) `objectscript-review`, (2) `objectscript-guardrails`, (3) `iris-sql`, (4) `iris-vector-ai`, (5) `objectscript-list-patterns`; add remaining skills in leaderboard order
- [x] T045 [US4] Create `skills.sh.json` at repo root — group skills by category (ObjectScript, IRIS SQL, Interoperability, MCP/Tools, Ecosystem projects); **affects skills.sh discovery web page only — does NOT affect `npx skills add` install behavior**; use schema `$schema: "https://skills.sh/schemas/skills.sh.schema.json"` and required `groupings` array per verified schema at skills.sh
- [x] T046 [US4] Verify each `skills/skills/<name>/SKILL.md` has `name` and `description` frontmatter fields (the only fields required by the `skills` CLI); `license: MIT` is conventional but optional — add to any missing it; correct path is `skills/skills/<name>/SKILL.md` not `skills/<name>/SKILL.md`
- [ ] T047 [US4] Run T041–T042 smoke tests — `npx skills add` works end-to-end
- [ ] T048 [US4] Register pack on skills.sh — submit `intersystems-community/iris-agentic-dev` to the skills.sh registry (visit skills.sh, connect GitHub account, add repo); confirms the pack appears alongside Supabase, Firebase, Anthropic packs; prerequisite: T045 (`skills.sh.json`) must exist so the groupings page renders correctly

**Phase Gate**: T042 passes. `npx skills add intersystems-community/iris-agentic-dev` installs skills to Claude Code with no IRIS required.

---

## Phase 7: `skill list` and `skill status` Subcommands

**Purpose**: Supporting CLI commands from contracts/cli.md — cross-cutting, no single user story.

- [x] T052 [P] Write unit test: `skill list` output format — correct columns, correct `installed`/`not installed`/`n/a` values per agent in `crates/iris-agentic-dev-core/tests/unit/test_skill_install.rs`
- [x] T053 [US1] Implement `SkillListCommand::run()` in `crates/iris-agentic-dev-bin/src/cmd/skill.rs` — reads install paths per agent, checks file existence, prints tabular output per contracts/cli.md
- [x] T054 [US1] Implement `SkillStatusCommand::run()` in `crates/iris-agentic-dev-bin/src/cmd/skill.rs` — reads all skill files in all install paths, reports managed-by status per file
- [x] T055 Run T052 unit test — passes

---

## Phase 8: Polish & Cross-Cutting Concerns

- [x] T056 Update `skills/README.md` leaderboard table — add any new ecosystem skills added in T029–T031
- [ ] T057 Add `skill install` to `docs/tools.md` or main README quickstart section
- [ ] T058 Update `specs/061-official-skill-pack/quickstart.md` — verify all commands in the doc work against the implementation
- [x] T059 Run `cargo fmt --all -- --check` — zero formatting diff
- [x] T060 Run `cargo clippy -p iris-agentic-dev-core -p iris-agentic-dev-bin -- -D warnings` — zero warnings
- [ ] T061 **Coverage gate** (Constitution VIII — NON-NEGOTIABLE): run `IRIS_HOST=localhost IRIS_PORT=52780 cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored --features testing` and confirm TOTAL line coverage ≥ 90%. Add integration tests for uncovered branches if below.
- [ ] T062 **Lift gate** (SC-006 — NON-NEGOTIABLE; Constitution IX is N/A — no new MCP tool): confirm `lift-results.md` exists in `specs/061-official-skill-pack/` with PYPR benchmark results showing ≥ +0.20 lift. Pre-validated at +1.83–2.33 (baseline 0.17/3 → 2.00–2.50/3) but must be formally recorded before merge.
- [x] T063 Run `markdownlint-cli2 --fix` + `prettier --write` on all `.md` files touched by this feature

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No deps — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — BLOCKS Phase 3, 4, 5
- **Phase 3 (US1 — File Install)**: Depends on Phase 2 — MVP gate
- **Phase 4 (US2 — Discovery)**: Depends on Phase 2; can start in parallel with Phase 3 (different files)
- **Phase 5 (US3 — IRIS Mirror)**: Depends on Phase 3 (needs install infrastructure)
- **Phase 6 (US4 — npx)**: Depends on Phase 1 only — pure JS, independent of Rust phases
- **Phase 7 (list/status)**: Depends on Phase 3 (needs path resolution + install infrastructure)
- **Phase 8 (Polish)**: Depends on all phases complete

### Parallel Opportunities

Phase 3 (Rust install) and Phase 6 (npm package) can run in parallel — no shared files.
Phase 4 (ecosystem skill content) can run alongside Phase 3 — different files.

---

## Implementation Strategy

### MVP (US1 only)

1. Complete Phase 1 + Phase 2
2. Complete Phase 3 (US1)
3. **STOP and VALIDATE**: `iris-agentic-dev skill install` works with no IRIS
4. Ship as initial release of the feature

### Full Delivery

1. MVP first (Phases 1–3)
2. Add ecosystem skills (Phase 4) + npm package (Phase 6) in parallel
3. Add IRIS mirror (Phase 5) after Phase 3
4. list/status commands (Phase 7)
5. Polish (Phase 8)

---

## Notes

- `[P]` = different files, no incomplete task dependencies — safe to parallelize
- Tests are written FIRST within each phase and MUST FAIL before implementation begins
- Phase gates (E2E tests) MUST pass before the next phase begins
- `--test-threads=1` required for any test that touches IRIS (race conditions)
- No custom npm package — `npx skills add` handles install via Vercel Labs `skills` CLI; Phase 6 is repo structure + metadata only
