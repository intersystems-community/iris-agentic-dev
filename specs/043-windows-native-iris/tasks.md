# Tasks: 043 — Windows Native IRIS Support

**Input**: Design documents from `/specs/043-windows-native-iris/`
**Prerequisites**: plan.md ✓, spec.md ✓, research.md ✓, data-model.md ✓

**Organization**: Tasks grouped by functional requirement (FR-1 through FR-5). FR-1 through FR-4 are Rust code/docs changes; FR-5 is skill artifacts in `light-skills/`. Each FR is independently testable.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Verify branch and build baseline before any changes

- [X] T001 Confirm `cargo build` passes clean on `043-windows-native-iris` branch
- [X] T002 Run `cargo test` to establish baseline test count (no failures expected)

---

## Phase 2: FR-1 — Native IRIS toml template

**Goal**: User with native IRIS on Windows can generate a `.iris-agentic-dev.toml` template that shows native config (host/web_port) as the primary documented path, container as secondary.

**Independent Test**: Read generated toml content; confirm `host` and `web_port` comment blocks appear before `container` comment block.

### Tests for FR-1

> **Write tests FIRST; they must FAIL before T005 implementation**

- [X] T003 [P] [US1] Add unit test for `generate_toml_content` in `crates/iris-agentic-dev-core/src/iris/workspace_config.rs` test module: assert output contains "Native IRIS" comment section before any "container" reference; assert `web_port = 80` comment appears; assert both sections are commented out by default

### Implementation for FR-1

- [X] T004 [US1] Update `generate_toml_content` in `crates/iris-agentic-dev-core/src/iris/workspace_config.rs` to restructure toml template: native IRIS section (host/web_port/web_prefix/namespace) first as commented block, Docker/container section second as commented block, with clear delimiter comments

**Checkpoint**: `cargo test workspace_config` passes; generated template has native-first layout

---

## Phase 3: FR-2 — Remove Docker-specific guidance when no container configured

**Goal**: When `IRIS_CONTAINER` is unset, error messages never suggest `docker run ...`. `DOCKER_REQUIRED` responses include a one-line HTTP remediation hint instead.

**Independent Test**: Trigger `DOCKER_REQUIRED` path with no container configured; confirm response `error` field contains "Atelier REST" or "http://" and does NOT contain "docker run".

### Tests for FR-2

> **Write tests FIRST; they must FAIL before T007/T008 implementation**

- [X] T005 [P] [US2] Add unit test in `crates/iris-agentic-dev-core/src/iris/discovery.rs` test module: construct a 401 warning message with `IRIS_CONTAINER` unset; assert output contains "check credentials" and does NOT contain "docker run -e IRIS_PASSWORD"
- [X] T006 [P] [US2] Add unit test for `DOCKER_REQUIRED` response string in `crates/iris-agentic-dev-core/src/tools/mod.rs` (or a dedicated test helper): assert the error string includes "http://" or "Atelier REST" and does NOT contain "docker run"

### Implementation for FR-2

- [X] T007 [US2] Update inline 401 warning at `crates/iris-agentic-dev-core/src/iris/discovery.rs` lines 79 and 437: change from Docker-only guidance to conditional message — "check credentials. If using Docker, restart with: `docker run -e IRIS_PASSWORD=SYS ...`" (Docker mention gated; non-Docker path says only "check credentials and verify host/web_port")
- [X] T008 [US2] Update all `DOCKER_REQUIRED` response strings in `crates/iris-agentic-dev-core/src/tools/mod.rs` (lines ~2171, 2484, 2487, 3131, 3226, 3482): append one-line remediation hint — "Ensure HTTP/Atelier REST is reachable: verify `http://<host>:<port>/api/atelier` and set `host`/`web_port` in `.iris-agentic-dev.toml`."

**Checkpoint**: `cargo test` passes; no "docker run" references appear in non-Docker error paths

---

## Phase 4: FR-3 — `check_config` connection source prominence

**Goal**: `check_config` JSON output surfaces `connection_source` as the second field (after `connected`) so users can immediately see whether they are connected via HTTP or Docker.

**Independent Test**: Call `check_config` (or inspect the JSON serialization); confirm `connection_source` appears before `host`, `port`, `container`, and other detail fields.

### Tests for FR-3

> **Write tests FIRST; they must FAIL before T010 implementation**

- [X] T009 [US3] Add unit test in `crates/iris-agentic-dev-core/src/tools/mod.rs` test module: serialize a mock `check_config` response to JSON string; assert `"connection_source"` byte-offset appears before `"host"` byte-offset in the serialized output

### Implementation for FR-3

- [X] T010 [US3] Reorder fields in the `check_config` response struct/inline JSON construction in `crates/iris-agentic-dev-core/src/tools/mod.rs` (~line 2829–2913): move `connection_source` to be the second key after `connected` in serialization order (use `#[serde(rename_all)]` field order or explicit `serde_json::json!` key ordering)

**Checkpoint**: `cargo test` passes; `check_config` output leads with `connected` then `connection_source`

---

## Phase 5: FR-4 — README native IRIS section

**Goal**: README has a "Connecting to IRIS without Docker" section covering minimal toml config, port guide (80 for 2024.1+ IIS, 52773 for pre-2024.1 PWS), IIS `/api` mapping requirement, and `localhost` vs `127.0.0.1` note.

**Independent Test**: Read README; confirm section exists with all four sub-topics present.

### Implementation for FR-4

- [X] T011 [US4] Add "Connecting to IRIS without Docker" section to `README.md` (after existing configuration sections): include minimal toml snippet (`host`/`web_port`), port guide table (2024.1+ IIS → 80, pre-2024.1 PWS → 52773), IIS `/api` web application mapping note (with step-by-step: IIS Manager → Applications → `/api` → `CSPms.dll` + wildcard handler), and `localhost`/`127.0.0.1` note for older Web Gateway builds

**Checkpoint**: README section present with all four required sub-topics; no broken markdown

---

## Phase 6: FR-5 — Skill artifacts (Windows IIS setup guidance)

**Goal**: Two skill artifacts provide IIS configuration guidance to agents and users.

**Independent Test**: Read each SKILL.md; confirm Common Mistakes row exists in iris-objectscript-eval and new iris-windows-iis-setup skill has all 5 steps with verification commands.

### Implementation for FR-5

- [X] T012 [P] [US5] Add IIS `/api` mapping row to Common Mistakes table in `light-skills/skills/iris-objectscript-eval/SKILL.md`: symptom = "`/api/atelier` returns 404 even when Management Portal works", cause = "IIS missing `/api` web application mapped to `CSPms.dll` with wildcard script handler", fix = "In IIS Manager: Add Web Application at `/api`, point to `CSPms.dll`, add wildcard handler `CSPms.dll` with no verbs restriction"
- [X] T013 [US5] Create `light-skills/skills/iris-windows-iis-setup/SKILL.md` as a how-to guide with 5 numbered steps, each with a verification command or observable outcome:
  - Step 1: Verify `/api` web application in IIS Manager (verification: `curl http://localhost/api/atelier` returns JSON, not 404)
  - Step 2: Verify `csp.ini` has `[APP_PATH:/api]` entry (verification: file contains the section header)
  - Step 3: Port guide — IRIS 2024.1+ IIS → port 80, pre-2024.1 PWS → port 52773 (verification: `curl http://localhost:<port>/api/atelier`)
  - Step 4: `localhost` vs `127.0.0.1` — use `127.0.0.1` on older Web Gateway builds (verification: no per-connection delay when switching)
  - Step 5: Minimal `.iris-agentic-dev.toml` config for native IRIS with `host` + `web_port` (verification: `check_config` returns `connection_source: "http"`)

**Checkpoint**: Both skill files present and complete; all 5 steps in new skill have verification criteria

---

## Phase 7: Polish & Integration

**Purpose**: Final build, format, and test sweep

- [X] T014 Run `cargo fmt --all -- --check` and fix any formatting issues
- [X] T015 Run `cargo test` full suite; confirm no regressions from FR-1 through FR-5 changes
- [X] T016 Run `cargo build --release` to confirm binary compiles clean
- [X] T017 [P] Commit all changes with descriptive message per FR (separate commits or single grouped commit)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies
- **Phase 2 (FR-1)**: Depends on Phase 1
- **Phase 3 (FR-2)**: Depends on Phase 1; independent of Phase 2 (different files)
- **Phase 4 (FR-3)**: Depends on Phase 1; independent of Phases 2–3 (same file `tools/mod.rs`, but different functions)
- **Phase 5 (FR-4)**: Depends on Phase 1; fully independent (README only)
- **Phase 6 (FR-5)**: Depends on Phase 1; fully independent (light-skills only, no Rust)
- **Phase 7 (Polish)**: Depends on all prior phases

### Parallel Opportunities

FR-2 and FR-4 can run in parallel (different files: `discovery.rs` vs `README.md`).
FR-5 (skills) can run at any time — no Rust dependency.
T012 and T013 within FR-5 can run in parallel.

---

## Implementation Strategy

### MVP First

1. Complete Phase 1 (build baseline)
2. Complete Phase 2 (FR-1 — toml template) — minimal Rust change, high user impact
3. Complete Phase 3 (FR-2 — error messages) — highest priority for Windows users hitting DOCKER_REQUIRED
4. STOP and validate: native IRIS user can configure toml and get useful error messages
5. Continue with FR-3, FR-4, FR-5

### Suggested commit order

1. `feat(workspace_config): restructure toml template — native IRIS config first` (FR-1)
2. `fix(discovery,tools): remove Docker guidance from non-Docker error paths` (FR-2)
3. `fix(check_config): surface connection_source prominently` (FR-3)
4. `docs: add native IRIS / Windows IIS setup section to README` (FR-4)
5. `feat(skills): add IIS /api mapping to common-mistakes; add iris-windows-iis-setup skill` (FR-5)

---

## Notes

- [P] tasks = different files, no dependencies between them
- [US1]–[US5] labels map to FR-1 through FR-5 respectively
- `git add -f` required for `*.md` files and `crates/*/src/**` paths (`.gitignore` patterns)
- Remote name is `github` not `origin` in this repo
- `cargo fmt --all -- --check` is mandatory before commit (CI enforces it)
