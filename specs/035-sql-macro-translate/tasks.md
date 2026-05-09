# Tasks: iris_execute &sql Macro Translation

**Input**: Design documents from `/specs/035-sql-macro-translate/`
**Repo**: `~/ws/iris-dev` (Rust — `crates/iris-dev-core`)
**Constitution**: Principle IV — unit tests first; Principle VII — zero new crates

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Add `translate_sql` field to `ExecuteParams` and create test file stubs — no behavior change yet.

- [x] T001 Add `translate_sql: bool` field with `#[serde(default = "default_translate_sql")]` and helper `fn default_translate_sql() -> bool { true }` to `ExecuteParams` struct in `crates/iris-dev-core/src/tools/mod.rs`
- [x] T002 [P] Create empty test stub `crates/iris-dev-core/tests/unit/test_sql_translate.rs`
- [x] T003 [P] Create empty E2E test stub `crates/iris-dev-core/tests/integration/test_sql_translate_e2e.rs`
- [x] T004 [P] Add `[[test]]` entries for both new test files to `crates/iris-dev-core/Cargo.toml`
- [x] T005 Verify `cargo check -p iris-dev-core` passes with the new field

**Checkpoint**: `cargo check` clean. `translate_sql` field present on `ExecuteParams`, defaulting to `true`.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Implement `translate_sql_macros()` as a pure function and `TranslationResult` struct. Tests first.

### Tests for Phase 2 (write first — must FAIL before implementation)

- [x] T006 [P] Write unit test: `translate_sql_macros()` with no `&sql(...)` → `found: false`, `translated_code` equals input, `warnings` empty in `tests/unit/test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T007 [P] Write unit test: SELECT INTO single variable — `&sql(SELECT Name INTO :name FROM foo WHERE ID = :id)` → correct `%SQL.Statement` prepare/execute/get/no-match code in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T008 [P] Write unit test: SELECT INTO multiple variables — `&sql(SELECT a, b INTO :x, :y FROM foo)` → both `%Get("a")` and `%Get("b")` present, `found: true` in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T009 [P] Write unit test: INSERT DML — `&sql(INSERT INTO foo (Col) VALUES (:val))` → `%ExecDirect` with positional `?`, `found: true` in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T010 [P] Write unit test: UPDATE DML — `&sql(UPDATE foo SET Name = :name WHERE ID = :id)` → `%ExecDirect` with two `?` in order in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T011 [P] Write unit test: DELETE DML — `&sql(DELETE FROM foo WHERE ID = :id)` → `%ExecDirect` in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T012 [P] Write unit test: SQLCODE on next line rewritten — `&sql(SELECT 1 INTO :x)\nif SQLCODE` → `SQLCODE` on that specific next line replaced with `_sqlSQLCODE1`, SQLCODE on OTHER lines NOT replaced in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T013 [P] Write unit test: `%msg` on next line rewritten — `&sql(SELECT 1 INTO :x)\nwrite %msg` → `_sqlrs1.%Message` on that line in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T014 [P] Write unit test: CALL statement falls through with warning — `&sql(CALL MyProc())` → `found: true`, `warnings` contains description, `translated_code` contains original `&sql(CALL...)` unchanged in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T015 [P] Write unit test: multiple `&sql(...)` in one block — each gets its own `_sqlrs1`, `_sqlrs2` etc. (collision avoidance) in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T016 [P] Write unit test: SELECT INTO no-rows semantics — generated code sets vars to `""` in the else branch, matching `&sql` behavior in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T017 [P] Write unit test: paren depth — `&sql(SELECT * FROM foo WHERE x IN (SELECT id FROM bar))` correctly finds the outer closing paren (not the inner one) in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T018 [P] Write unit test: column alias — `&sql(SELECT Name AS n INTO :n FROM foo)` → `%Get("n")` (alias used, not original column) in `test_sql_translate.rs` (WRITE FIRST, must FAIL)

### TDD Gate

- [x] T019 **GATE**: Confirm `cargo test --test test_sql_translate` produces FAILURES (function doesn't exist). Do not proceed until T006–T018 fail to compile.

### Implementation for Phase 2

- [x] T020 Define `TranslationResult` struct (`translated_code: String`, `found: bool`, `warnings: Vec<String>`) in `crates/iris-dev-core/src/tools/mod.rs`
- [x] T021 Implement `translate_sql_macros(code: &str) -> TranslationResult` in `crates/iris-dev-core/src/tools/mod.rs`:
  - Find `&sql(` using paren-depth counting to locate matching `)`
  - Classify statement type (SELECT/INSERT/UPDATE/DELETE/MERGE/CALL/other)
  - For SELECT INTO: extract column list (before INTO), host vars (after INTO), WHERE params; generate `%SQL.Statement.%New()/.%Prepare()/.%Execute()/.%Next()/.%Get()` with no-rows else branch setting vars to `""`
  - For DML: replace `:varname` with `?` in order; generate `%SQL.Statement.%ExecDirect(, "...", var1, var2, ...)`
  - After each translation: check next line for standalone `SQLCODE` → replace with `_sqlSQLCODEn`; check for `%msg` → replace with `_sqlrs{n}.%Message`
  - For CALL/unrecognized: leave unchanged, append to `warnings`
  - Generate unique variable names `_sqlrs1`, `_sqlrs2`, ... checking for conflicts with existing variable names in the code
- [x] T022 Verify T006–T018 all pass GREEN: `cargo test --test test_sql_translate`

**Checkpoint**: All 13 foundational unit tests green. `translate_sql_macros()` handles all basic patterns.

---

## Phase 3: User Story 1 — Agent writes &sql code and it just works (Priority: P1) 🎯 MVP

**Goal**: `iris_execute` with `translate_sql: true` (default) rewrites `&sql(...)` before execution. Response includes `sql_translated: true` and `translated_code`. Code without `&sql(...)` is unaffected.

**Independent Test**: Call `iris_execute` with `&sql(SELECT 1 INTO :x)\nwrite x,!` — output is `1`, response has `sql_translated: true`.

### Tests for US1 (write first — must FAIL before implementation)

- [x] T023 [P] [US1] Write unit test: `iris_execute` handler logic — when `translate_sql: true` and `&sql(...)` present, `sql_translated: true` and `translated_code` in response (mock translation, no IRIS needed) in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T024 [P] [US1] Write unit test: `iris_execute` with no `&sql(...)` and `translate_sql: true` → response has NO `sql_translated` field in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T025 [US1] Write E2E test (`#[ignore]`): call `iris_execute` with `set id=\"%ASQ.AST\"\nset name=\"\"\n&sql(SELECT Name INTO :name FROM %Dictionary.ClassDefinition WHERE ID = :id)\nwrite name,!` → output contains `%ASQ.AST`, response has `sql_translated: true` in `tests/integration/test_sql_translate_e2e.rs` (WRITE FIRST, must FAIL)
- [x] T026 [US1] Write E2E test (`#[ignore]`): call `iris_execute` with INSERT `&sql(INSERT INTO %sqltemp.Test (ID, Name) VALUES (:i, :n))` — or use a safe temp approach — → `sql_translated: true` in `test_sql_translate_e2e.rs` (WRITE FIRST, must FAIL)

### TDD Gate

- [x] T027 [US1] **GATE**: Confirm T023–T026 all FAIL before implementation

### Implementation for US1

- [x] T028 [US1] Wire `translate_sql_macros()` into `iris_execute` handler in `crates/iris-dev-core/src/tools/mod.rs`: if `p.translate_sql` is true, call `translate_sql_macros(&p.code)`; if result `found`, use `result.translated_code` as the code to execute; add `sql_translated: true`, `translated_code`, and optionally `translation_warning` to the success response
- [x] T029 [US1] Update `iris_execute` tool description in `mod.rs` — remove the existing note about `&sql` not supported; replace with description of `translate_sql` param and automatic translation behavior
- [x] T030 [US1] **GATE-GREEN**: Run `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --test test_sql_translate_e2e -- --ignored` — T025 must pass

**Phase gate**: T025 E2E passes. `iris_execute` transparently translates `&sql(SELECT INTO ...)` and returns correct output.

---

## Phase 4: User Story 2 — Agent opts out of translation (Priority: P2)

**Goal**: `translate_sql: false` sends code to IRIS unchanged. Default remains `true`.

**Independent Test**: Call with `translate_sql: false` and `&sql(SELECT 1)` → IRIS error returned, `sql_translated` absent.

### Tests for US2 (write first — must FAIL before implementation)

- [x] T031 [P] [US2] Write unit test: `translate_sql: false` with `&sql(...)` → translation NOT called, code sent as-is in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T032 [P] [US2] Write unit test: `translate_sql` not specified → defaults to `true`, translation fires in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T033 [US2] Write E2E test (`#[ignore]`): call with `translate_sql: false` and `&sql(SELECT 1 INTO :x)` → response has `success: false` or IRIS error, NO `sql_translated` field in `test_sql_translate_e2e.rs` (WRITE FIRST, must FAIL)

### TDD Gate

- [x] T034 [US2] **GATE**: Confirm T031–T033 FAIL before implementation

### Implementation for US2

- [x] T035 [US2] The handler logic from T028 already handles `translate_sql: false` by skipping the translation call — verify this is correct with T031–T032 unit tests. No additional implementation needed unless tests fail.
- [x] T036 [US2] **GATE-GREEN**: Run E2E T033 — must pass

**Phase gate**: T033 E2E passes. `translate_sql: false` bypasses translation correctly.

---

## Phase 5: User Story 3 — Agent inspects translated code (Priority: P2)

**Goal**: `translated_code` in response contains valid, readable `%SQL.Statement` ObjectScript matching the original `&sql(...)`. Untranslatable constructs produce `translation_warning`.

**Independent Test**: Call with multi-variable SELECT INTO, verify `translated_code` contains all expected `%Get(...)` calls and correct variable names.

### Tests for US3 (write first — must FAIL before implementation)

- [x] T037 [P] [US3] Write unit test: multi-column SELECT INTO → `translated_code` contains `%Get("ColA")` and `%Get("ColB")` in correct order in `test_sql_translate.rs` (WRITE FIRST, must FAIL — verifies existing foundational code produces correct field in response)
- [x] T038 [P] [US3] Write unit test: CALL statement → response contains `translation_warning` with description of untranslatable construct in `test_sql_translate.rs` (WRITE FIRST, must FAIL)
- [x] T039 [US3] Write E2E test (`#[ignore]`): call with `&sql(CALL)` equivalent → `translation_warning` present in response, tool does not crash in `test_sql_translate_e2e.rs` (WRITE FIRST, must FAIL)

### TDD Gate

- [x] T040 [US3] **GATE**: Confirm T037–T039 FAIL before implementation (US3 is largely covered by foundational + US1 work; gate confirms end-to-end wiring)

### Implementation for US3

- [x] T041 [US3] Verify T037–T039 pass after Phase 3 implementation. If `translation_warning` is not wired into the response, add it in `mod.rs` iris_execute handler alongside `sql_translated`.
- [x] T042 [US3] **GATE-GREEN**: Run E2E T039 — must pass

**Phase gate**: T039 E2E passes. `translated_code` and `translation_warning` correctly surfaced in response.

---

## Phase 6: Polish & Cross-Cutting

- [x] T043 [P] Add `check_config` and `iris_execute translate_sql` to README.md — update the `iris_execute` row in the tools table to mention `&sql` macro translation
- [x] T044 [P] Run `cargo clippy --all-targets -- -D warnings` — must be clean
- [x] T045 [P] Run `cargo fmt --all -- --check` — must be clean
- [x] T046 [P] Run full unit test suite: `cargo test -p iris-dev-core` — all pass, no regressions
- [x] T047 [P] Run all E2E tests: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --test test_sql_translate_e2e -- --ignored` — all 4 E2E tests pass
- [x] T048 [P] Write SC-001 validation test: ≥15 `&sql(...)` patterns (SELECT INTO single/multi var, INSERT, UPDATE, DELETE, SQLCODE check, %msg check, no-rows case, nested parens, column alias, multiple macros) as unit tests — all translate correctly in `test_sql_translate.rs`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1
- **Phase 3 (US1)**: Depends on Phase 2 — MVP gate
- **Phase 4 (US2)**: Depends on Phase 3 (uses same handler wiring)
- **Phase 5 (US3)**: Depends on Phase 2; largely covered by Phase 3 work
- **Phase 6 (Polish)**: Depends on all phases complete

### Critical Path

```
T001-T005 (setup) → T006-T022 (translate_sql_macros) → T023-T030 (US1 wiring)
                                                      → T031-T036 (US2 opt-out)
                                                      → T037-T042 (US3 inspect)
T030 (US1 gate) → T043-T048 (polish)
```

### Parallel Opportunities

**Phase 2 tests** (T006–T018): all touch same file but independent test functions — write concurrently.

**Phase 6**: all tasks [P] — run simultaneously.

---

## Implementation Strategy

### MVP: Phases 1–3

1. `translate_sql` field on `ExecuteParams` (Phase 1)
2. `translate_sql_macros()` pure function with all patterns (Phase 2)
3. Wire into `iris_execute` handler, verify E2E SELECT INTO (Phase 3)
4. **VALIDATE**: `iris_execute` with `&sql(SELECT INTO ...)` just works

### Full Feature

5. Phase 4: `translate_sql: false` opt-out verified
6. Phase 5: `translation_warning` for CALL/untranslatable confirmed
7. Phase 6: polish, ≥15 pattern SC-001 test
