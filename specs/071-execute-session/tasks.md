# Tasks: iris_execute Session State (071)

**Input**: `specs/071-execute-session/plan.md`, `spec.md`

## Phase 1: Setup

**Purpose**: New source file, no existing code to modify first.

- [x] T001 Create `crates/iris-agentic-dev-core/src/tools/execute_session.rs` (empty module, just `pub mod execute_session;` stub in mod.rs)

---

## Phase 2: Foundational — Session state encoding/decoding (no IRIS)

**Purpose**: Pure Rust serialization layer. No IRIS, fully unit-testable. All downstream tasks depend on this.

**⚠️ CRITICAL**: Must pass before any integration work begins.

### Tests (write first — must fail before T006)

- [x] T002 [P] Write unit tests in `crates/iris-agentic-dev-core/src/tools/execute_session.rs` covering:
  - `SessionState::empty()` produces valid Base64 JSON decoding to `{}`
  - `SessionState::from_token(token)` round-trips through `to_token()`
  - `SessionState::from_token` with invalid Base64 returns `Err`
  - `SessionState::from_token` with valid Base64 but invalid JSON returns `Err`
  - OID stub detection: JSON object with `_cls`/`_id` keys is recognised as `PersistentOid`

### Implementation

- [x] T003 Define `SessionState` struct in `execute_session.rs`:
  - Wraps a `serde_json::Value` (the decoded `%ctx` JSON object)
  - `fn empty() -> Self` — returns `SessionState` wrapping `json!({})`
  - `fn from_token(token: &str) -> Result<Self>` — Base64-decode then `serde_json::from_str`
  - `fn to_token(&self) -> String` — `serde_json::to_string` then Base64-encode
  - `fn oid_keys(&self) -> Vec<String>` — returns keys whose value is `{"_cls":..,"_id":..}`

- [x] T004 Write `fn build_session_preamble(token: Option<&str>) -> Result<String, SessionError>` in `execute_session.rs`:
  - If `token` is `None`: returns the single line `Set %ctx = {}`
  - If `token` is `Some`: generates the full restore block from plan.md (Base64Decode, %FromJSON, two-pass OID restore, sentinel on failure)
  - Uses `token.replace('"', "\"")` safety — token is Base64 so no quotes, but guard anyway

- [x] T005 Write `fn build_session_epilogue() -> String` in `execute_session.rs`:
  - Returns the full epilogue block from plan.md (two-pass re-stub, %ToJSON, Base64Encode, sentinel write)
  - No parameters — epilogue is always the same shape

**Checkpoint**: `cargo test -p iris-agentic-dev-core execute_session` — all unit tests green.

---

## Phase 3: User Story 1 — Scalar round-trip (Priority: P1) 🎯 MVP

**Goal**: `use_session: true` with scalar `%ctx` values round-trips correctly. No IRIS required for unit tests; e2e tests use live IRIS.

**Independent Test**: `cargo test e2e_execute_session_scalar -- --include-ignored` passes.

### Tests (write first)

- [x] T006 [P] [US1] Add unit test in `execute_session.rs`:
  - `build_session_preamble(None)` contains `Set %ctx = {}`
  - `build_session_preamble(Some(token))` contains `Base64Decode` and `%FromJSON`
  - `build_session_epilogue()` contains `__SESSION_STATE__:` sentinel and `Base64Encode`
  - Preamble + epilogue contain no `$LENGTH`, `$PIECE`, `$DATA` (must use abbreviated forms)

- [x] T007 [P] [US1] Add e2e test `e2e_execute_session_scalar_roundtrip` in `tests/integration/test_e2e.rs`:
  - Call 1: `use_session: true`, code `Set %ctx.x = 42  Set %ctx.label = "hello"` — assert response has `session_state`
  - Call 2: pass `session_state` from call 1, code `Write %ctx.x, !, %ctx.label, !` — assert output `42\nhello\n`
  - Mark `#[ignore]` (requires `IRIS_HOST`)

### Implementation

- [x] T008 [US1] Add `use_session: bool` and `session_state: Option<String>` to `ExecuteParams` in `crates/iris-agentic-dev-core/src/tools/mod.rs` (serde default `false` / `None`)

- [x] T009 [US1] In `iris_execute` handler (`mod.rs`): when `use_session: true`, call `build_session_preamble` and `build_session_epilogue` and wrap user code:
  ```
  preamble_lines + user_code_lines + epilogue_lines
  ```
  Pass the combined string to `execute_via_generator`.

- [x] T010 [US1] Parse sentinel lines from `execute_via_generator` output in handler:
  - Strip `__SESSION_STATE__:...` line from visible output; extract token → `session_state` field in response JSON
  - Map `__SESSION_INVALID__:` → return `SESSION_INVALID` error immediately (don't execute)
  - Map `__SESSION_RESTORE_FAILED__:` → return `SESSION_RESTORE_FAILED` error
  - Map `__SESSION_SERIALIZE_FAILED__:` → return `SESSION_SERIALIZE_FAILED` error

- [x] T011 [US1] Add `session_state: Option<String>` field to the `iris_execute` JSON response (alongside existing `output`, `success`, etc.)

- [x] T012 [US1] Add unit test confirming `ExecuteParams` with `use_session: false` (default) passes through to `execute_via_generator` with zero modification to the code string

**Checkpoint**: `cargo test e2e_execute_session_scalar -- --include-ignored` green (requires IRIS).

---

## Phase 4: User Story 2 — %Persistent OID round-trip (Priority: P2)

**Goal**: `%ctx` keys holding `%Persistent` objects serialize to OID stubs and restore to live objects on next call.

**Independent Test**: `cargo test e2e_execute_session_persistent -- --include-ignored` passes.

### Tests (write first)

- [x] T013 [P] [US2] Add unit test: `SessionState::oid_keys()` correctly identifies `{"_cls":..,"_id":..}` entries vs plain scalars and nested objects without those keys

- [x] T014 [P] [US2] Add e2e test `e2e_execute_session_persistent_oid` in `test_e2e.rs`:
  - Call 1: `use_session: true`, code opens `Ens.MessageHeader` ID 1, stores in `%ctx.hdr`, writes `%ctx.hdr.SourceConfigName` — assert output contains `Test`, response has `session_state`
  - Call 2: pass `session_state`, code `Write %ctx.hdr.MessageBodyClassName` — assert output `Ens.StringContainer`
  - Mark `#[ignore]`

- [x] T015 [P] [US2] Add e2e test `e2e_execute_session_missing_class`:
  - Manually construct a `session_state` token with `{"missingObj": {"_cls": "NoSuch.Class", "_id": "1"}}`
  - Call `iris_execute` with `use_session: true` and that token — assert `error_code == "SESSION_RESTORE_FAILED"`
  - Mark `#[ignore]`

### Implementation

- [x] T016 [US2] The preamble/epilogue generated by `build_session_preamble`/`build_session_epilogue` already handle OID stubs (implemented in T004/T005). Verify the two-pass scan runs correctly end-to-end. No new Rust code needed beyond confirming the sentinel parse (T010) handles the `__SESSION_RESTORE_FAILED__` case.

**Checkpoint**: `cargo test e2e_execute_session_persistent -- --include-ignored` green.

---

## Phase 5: User Story 3 — %DynamicObject accumulation (Priority: P3)

**Goal**: Nested `%DynamicObject` values in `%ctx` survive round-trips unchanged.

**Independent Test**: `cargo test e2e_execute_session_dynamic -- --include-ignored` passes.

### Tests (write first)

- [x] T017 [US3] Add e2e test `e2e_execute_session_dynamic_accumulation` in `test_e2e.rs`:
  - Call 1: code `Set %ctx.result = {}  Set %ctx.result.step1 = "done"` — assert `session_state` present
  - Call 2: code `Set %ctx.result.step2 = "also done"  Write %ctx.result.%ToJSON()` — assert output contains both `step1` and `step2`
  - Mark `#[ignore]`

### Implementation

- [x] T018 [US3] No new Rust code needed — `%DynamicObject` values are JSON-native and survive `%ToJSON`/`%FromJSON` without special handling. Confirm e2e test passes.

**Checkpoint**: `cargo test e2e_execute_session -- --include-ignored` green (all three story tests).

---

## Phase 6: Lift Measurement (required before merge — Constitution IX)

**Purpose**: Verify the tool earns its place. Required release gate.

- [x] T019 Add benchmark task file `crates/iris-agentic-dev-core/src/benchmark/tasks/session-001.json`:
  ```json
  {
    "id": "session-001",
    "description": "A prior iris_execute call computed patient count = 1247. Without rerunning the query, use that stored value to calculate 5% of the patient population.",
    "success_criteria": ["Output contains 62 or 62.35", "No iris_query call made"],
    "expected_params": {"use_session": true, "session_state": "<token containing count=1247>"}
  }
  ```

- [x] T020 Add benchmark task file `crates/iris-agentic-dev-core/src/benchmark/tasks/session-002.json`:
  ```json
  {
    "id": "session-002",
    "description": "Open Ens.MessageHeader ID 1, store it in session, then in a second call read its MessageBodyClassName without knowing the ID.",
    "success_criteria": ["Second call outputs Ens.StringContainer", "No hardcoded ID in second call"],
    "expected_params": {"use_session": true}
  }
  ```

- [x] T021 Run lift measurement against baseline and record results in `specs/071-execute-session/lift-results.md`. Target: lift ≥ +0.20 on at least one task.

**Checkpoint**: `lift-results.md` exists with measured results. If lift < +0.20, iterate on tool description before continuing.

---

## Phase 7: Polish & Cross-Cutting Concerns

- [x] T022 [P] Update `iris_execute` tool description in `mod.rs` to document `use_session`, `session_state`, `%ctx`, and the three new error codes (`SESSION_INVALID`, `SESSION_RESTORE_FAILED`, `SESSION_SERIALIZE_FAILED`)

- [x] T023 [P] Add `SESSION_INVALID`, `SESSION_RESTORE_FAILED`, `SESSION_SERIALIZE_FAILED` to the error code registry in `specs/071-execute-session/data-model.md`

- [x] T024 [P] Update `docs/tools.md` `iris_execute` section: add `use_session` and `session_state` parameters, `%ctx` carrier variable explanation, three error codes, and a usage example

- [ ] T025 [P] Close GitHub issue #32 with a comment linking to this feature (after merge)

- [x] T026 Run `cargo fmt --all -- --check` — no formatting diff

- [x] T027 Run `cargo clippy -p iris-agentic-dev-core -- -D warnings` — zero warnings

- [~] T028 **Coverage gate** (Constitution VIII): measured 64.87% line / 70.20% function (2026-07-30,
  `--features testing --include-ignored`). `execute_session.rs` itself is 100%. Gap is pre-existing
  and caused by v0.9.7: xdata_flow.rs (+394 lines), skills/bundled.rs (+657 lines), and new mod.rs
  dispatch routing are only exercised via the test_e2e subprocess path, whose profraw files are never
  merged into llvm-cov. Math: 89.25% at v0.9.5 → expected 83.4% after new code → actual 64.87%
  implies ~6,300 previously-counted covered lines now invisible (subprocess coverage gap). Fix needs
  a coverage.sh that merges profraw from the instrumented binary. This feature adds zero to the gap.

- [x] T029 Write "What's new" release notes entry for this feature (Constitution release notes discipline)

---

## Dependencies & Execution Order

- **Phase 1**: No dependencies
- **Phase 2**: Depends on Phase 1. **Blocks all other phases.**
- **Phase 3**: Depends on Phase 2. US1 MVP — implement this first.
- **Phase 4**: Depends on Phase 2. Can start after Phase 3 checkpoint passes.
- **Phase 5**: Depends on Phase 2. Can start after Phase 4 checkpoint passes.
- **Phase 6**: Depends on Phases 3–5 all passing. Required before merge.
- **Phase 7**: Depends on Phase 6. Polish before tagging.

### Within each phase: tests first, then implementation.

Total tasks: 29
Tests: T002, T006, T007, T013, T014, T015, T017 (7 test tasks across 3 stories)
