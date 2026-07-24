# Tasks: VS Code Extension Auto-Installs Binary

**Input**: Design documents from `/specs/069-vscode-binary-install/`
**Prerequisites**: plan.md ✓, spec.md ✓, research.md ✓, data-model.md ✓

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- All paths relative to `vscode-iris-agentic-dev/`

---

## Phase 1: Setup

**Purpose**: Scaffold new source files; verify build still works.

- [ ] T001 Create empty `src/platform.ts` with exported stubs `getBinaryName` and `getDownloadUrl` in `vscode-iris-agentic-dev/src/platform.ts`
- [ ] T002 [P] Create empty `src/download.ts` with exported stub `downloadBinary` in `vscode-iris-agentic-dev/src/download.ts`
- [ ] T003 [P] Create empty `src/managedInstall.ts` with exported stub `resolveServerBinary` in `vscode-iris-agentic-dev/src/managedInstall.ts`
- [ ] T004 Run `npm run compile` in `vscode-iris-agentic-dev/` and confirm zero errors with stub exports

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Platform/URL helpers and download primitive — shared by all three user stories.
All US phases depend on these being correct and tested.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete and tests pass.

### Tests for Platform helpers (write first — must FAIL before T009)

- [ ] T005 [P] Write unit tests for `getBinaryName` covering all 4 supported platform/arch combos (`darwin/arm64` → `iris-agentic-dev-macos-arm64`, `darwin/x64` → `iris-agentic-dev-macos-x86_64`, `linux/x64` → `iris-agentic-dev-linux-x86_64`, `win32/x64` → `iris-agentic-dev-windows-x86_64.exe`) and unsupported combo (`linux/arm64` → `null`) in `vscode-iris-agentic-dev/test/platform.test.cjs`
- [ ] T006 [P] Write unit tests for `getDownloadUrl` verifying correct GitHub Releases URL for each supported combo and `null` for unsupported in `vscode-iris-agentic-dev/test/platform.test.cjs`

### Implementation of Platform helpers

- [ ] T007 Implement `getBinaryName(platform: string, arch: string): string | null` in `vscode-iris-agentic-dev/src/platform.ts` — maps `process.platform`/`process.arch` values per research Decision 2 table; returns `null` for unsupported combos
- [ ] T008 Implement `getDownloadUrl(version: string, platform: string, arch: string): string | null` in `vscode-iris-agentic-dev/src/platform.ts` — returns `https://github.com/intersystems-community/iris-agentic-dev/releases/download/v{VERSION}/{binaryName}` or `null`
- [ ] T009 Run `npm test` (or `node --test test/platform.test.cjs`) and confirm all platform tests pass in `vscode-iris-agentic-dev/`

### Download helper

- [ ] T010 Implement `downloadBinary(url: string, dest: string, onProgress: (fraction: number) => void): Promise<void>` in `vscode-iris-agentic-dev/src/download.ts` — uses Node built-in `https`, follows up to 10 redirects, streams to `dest + '.tmp'`, renames atomically on completion, throws on HTTP error or incomplete transfer (no new npm deps)

**Checkpoint**: Platform helpers tested and passing; download helper implemented. User story work can begin.

---

## Phase 3: User Story 1 — First-time install, no existing binary (Priority: P1) 🎯 MVP

**Goal**: Extension downloads correct binary on first activation and MCP server starts with zero manual steps.

**Independent Test**: Delete `globalStorageUri` cache, activate extension in Extension Development Host — observe progress notification, binary appears in cache, MCP provider registers successfully.

### Tests for US1 (write first — must FAIL before T014)

- [ ] T011 [US1] Write unit test: version marker absent → `resolveServerBinary` triggers download path (mock `downloadBinary` to resolve, mock `fs.promises.readFile` to reject) in `vscode-iris-agentic-dev/test/managedInstall.test.cjs`
- [ ] T012 [P] [US1] Write unit test: version marker matches extension version and binary exists → `resolveServerBinary` returns cached path without calling `downloadBinary` in `vscode-iris-agentic-dev/test/managedInstall.test.cjs`
- [ ] T013 [P] [US1] Write unit test: unsupported platform (`linux/arm64`) → `resolveServerBinary` returns `null` without attempting download (no setting, not on PATH) in `vscode-iris-agentic-dev/test/managedInstall.test.cjs`

### Implementation for US1

- [ ] T014 [US1] Implement core of `resolveServerBinary(context: vscode.ExtensionContext): Promise<string | null>` in `vscode-iris-agentic-dev/src/managedInstall.ts`: tier-1 `serverPath` setting check, tier-2 `which.sync` PATH lookup, tier-3 managed download path (version check → cache hit → return; version mismatch or absent → call `downloadBinary` with `vscode.window.withProgress` progress notification → `chmod 0o755` on non-Windows → write version marker → return path)
- [ ] T015 [US1] Add concurrency guard to `resolveServerBinary` in `vscode-iris-agentic-dev/src/managedInstall.ts`: module-level `let activeResolve: Promise<string | null> | undefined` so simultaneous activations share one download
- [ ] T016 [US1] Wire `resolveServerBinary` into `vscode-iris-agentic-dev/src/extension.ts`: make `activate()` async, replace `findIrisDev()` call with `await resolveServerBinary(context)`, pass resolved path into `IrisDevMcpProvider` constructor (store as `private readonly binaryPath`), remove `findIrisDev()` function
- [ ] T017 [US1] Update `IrisDevMcpProvider` in `vscode-iris-agentic-dev/src/extension.ts` to accept `binaryPath: string | null` in constructor and use it in `provideMcpServerDefinitions` instead of calling `findIrisDev()`
- [ ] T018 [US1] Run `npm test` and confirm all US1 unit tests pass in `vscode-iris-agentic-dev/`
- [ ] T019 [US1] Run `npm run compile` and confirm zero TypeScript errors in `vscode-iris-agentic-dev/`

**Checkpoint**: Clean build + unit tests green. Manual smoke: delete cache, activate extension, observe download notification and successful MCP registration.

---

## Phase 4: User Story 2 — Existing PATH/setting binary respected (Priority: P2)

**Goal**: Extension uses an existing `brew`-installed or user-configured binary without downloading a shadow copy.

**Independent Test**: Set `iris-agentic-dev.serverPath` to a dummy executable path or put a dummy `iris-agentic-dev` on PATH — confirm extension uses it and no download occurs.

### Tests for US2 (write first — must FAIL before T022)

- [ ] T020 [US2] Write unit test: `iris-agentic-dev.serverPath` set to an existing executable path → `resolveServerBinary` returns that path immediately without entering managed-download tier in `vscode-iris-agentic-dev/test/managedInstall.test.cjs`
- [ ] T021 [P] [US2] Write unit test: `iris-agentic-dev.serverPath` set to a non-existent path → `resolveServerBinary` throws or returns `null` with a clear error message in `vscode-iris-agentic-dev/test/managedInstall.test.cjs`

### Implementation for US2

- [ ] T022 [US2] Add executable-existence check to the `serverPath` tier in `resolveServerBinary` in `vscode-iris-agentic-dev/src/managedInstall.ts`: if `serverPath` is set but `fs.accessSync(path, fs.constants.X_OK)` throws, log a clear error and return `null` (do not fall through to PATH or download)
- [ ] T023 [US2] Run `npm test` and confirm all US2 unit tests pass in `vscode-iris-agentic-dev/`

**Checkpoint**: Setting and PATH tiers tested. Manual smoke: set `serverPath` to existing binary, confirm no download and provider uses that path.

---

## Phase 5: User Story 3 — Stale cache updated on extension upgrade (Priority: P3)

**Goal**: On extension update, the managed binary is re-downloaded automatically on next activation.

**Independent Test**: Write `"0.0.0"` to the version marker file, activate extension — confirm re-download triggered and version marker updated to current extension version.

### Tests for US3 (write first — must FAIL before T027)

- [ ] T024 [US3] Write unit test: version marker contains old version string → `resolveServerBinary` calls `downloadBinary` and updates version marker in `vscode-iris-agentic-dev/test/managedInstall.test.cjs`
- [ ] T025 [P] [US3] Write unit test: download fails (mock `downloadBinary` to reject) and stale binary exists → `resolveServerBinary` returns stale binary path and logs a warning (does not throw) in `vscode-iris-agentic-dev/test/managedInstall.test.cjs`
- [ ] T026 [P] [US3] Write unit test: download fails and no cached binary exists → `resolveServerBinary` returns `null` in `vscode-iris-agentic-dev/test/managedInstall.test.cjs`

### Implementation for US3

- [ ] T027 [US3] Add Windows rename-before-replace logic to the managed download path in `vscode-iris-agentic-dev/src/managedInstall.ts`: before writing new binary on `win32`, attempt `fs.promises.rename(binaryPath, binaryPath + '.old')` (ignore `ENOENT`); attempt to delete `.old` file on cache-hit activation (ignore all errors)
- [ ] T028 [US3] Add download-failure fallback logic in `vscode-iris-agentic-dev/src/managedInstall.ts`: on `downloadBinary` rejection, if stale binary path exists return it with `this.log.warn(...)`, else return `null`
- [ ] T029 [US3] Run `npm test` and confirm all US3 unit tests pass in `vscode-iris-agentic-dev/`

**Checkpoint**: All three user stories tested. Full `npm test` suite green.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [ ] T030 Update `package.json` test script in `vscode-iris-agentic-dev/package.json` to run all three test files: `"test": "esbuild src/redact.ts ... && node --test test/redact.test.cjs test/platform.test.cjs test/managedInstall.test.cjs"`
- [ ] T031 [P] Update `vscode-iris-agentic-dev/README.md` — remove "binary must be on PATH" prerequisite; add auto-install note; keep `serverPath` setting documented as an override
- [ ] T032 [P] Update error message in `vscode-iris-agentic-dev/src/extension.ts` for the `null` binary case to explain that auto-install failed and link to manual install docs (replaces the existing "Download from github…" message at line 104-109)
- [ ] T033 Run full `npm test` in `vscode-iris-agentic-dev/` and confirm all tests pass (redact + platform + managedInstall)
- [ ] T034 Run `npm run compile` in `vscode-iris-agentic-dev/` and confirm zero TypeScript errors
- [ ] T035 Run quickstart.md manual smoke test: clean cache on macOS, activate in Extension Development Host, confirm download progress notification, binary cached, MCP server starts
- [ ] T036 Write release notes entry for 069 feature in `specs/069-vscode-binary-install/release-notes-0.9.5.md` — "VS Code extension now auto-installs the binary on first activation" with Windows context

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — blocks all user stories
- **Phase 3 (US1)**: Depends on Phase 2 — MVP deliverable
- **Phase 4 (US2)**: Depends on Phase 2 — can run after Phase 2, independent of Phase 3
- **Phase 5 (US3)**: Depends on Phase 3 (reuses managed-download logic from T014/T028) — run after Phase 3
- **Phase 6 (Polish)**: Depends on all story phases complete

### Parallel Opportunities

Within Phase 2: T005 and T006 can run in parallel (same file, but both are additive test cases — write sequentially to avoid conflict); T007 and T008 are sequential (T008 calls `getBinaryName`).

Within Phase 3: T011, T012, T013 can all be written in parallel (all in same test file, additive); T016 and T017 are tightly coupled (same class, do sequentially).

Within Phase 6: T030, T031, T032 can run in parallel (different files).

---

## Implementation Strategy

### MVP (User Story 1 only)

1. Complete Phase 1 (Setup)
2. Complete Phase 2 (Foundational — platform helpers + download)
3. Complete Phase 3 (US1 — core managed install + extension wiring)
4. Validate: `npm test` green + manual smoke on macOS
5. This delivers the primary Windows value (Docker path already exists; binary auto-install removes the last manual step everywhere)

### Incremental Delivery

- Phase 3 alone delivers the "zero steps for new users" story
- Phase 4 adds the explicit-override guarantee (important for brew users)
- Phase 5 adds the auto-update guarantee (stale cache handling)
- Each phase is independently releasable
