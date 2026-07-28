# Tasks: iris_symbols_local Upgrade (070)

**Input**: `specs/070-symbols-local-upgrade/spec.md`
**Prerequisites**: spec.md ✅

All Rust paths relative to `crates/iris-agentic-dev-core/`.
Tests are written before implementation — each test must **fail** before its implementation
task begins, then **pass** after.

---

## Phase 0: Bump crates

**Purpose**: Get the new grammar in place; verify clean build before any source changes.

- [ ] T001 [US1] In `Cargo.toml` change `tree-sitter-objectscript = "1.7"` to `"1.9"` and
      `tree-sitter-objectscript-routine = "1.7"` to `"1.9"`; run `cargo update`; confirm
      `Cargo.lock` pins `1.9.13`; run `cargo build` to confirm no compilation errors
- [ ] T002 [US1] Add fixture `tests/fixtures/MyApp/PythonBody.cls` — a class with one method
      using `Language = python` body syntax to exercise the grammar fix from PR #52
- [ ] T003 [US1] Write test T070-02 — assert `PythonBody.cls` produces no `PARSE_ERROR`
      and emits the expected method symbol; confirm test **passes** with `1.9` crates

**Checkpoint**: `cargo test` passes; crate versions at `1.9`.

---

## Phase 1: Dead code removal

**Purpose**: Remove the unreachable arm before adding new code so clippy is clean from the
start.

- [ ] T004 [US10] Write test T070-34 — assert `Utils.mac` still yields `Utils:Start` and
      `Utils:Helper` labels; confirm it **passes** (establishes regression guard)
- [ ] T005 [US10] Delete the `"tag_with_params"` arm (~lines 469–486 in `symbols_local.rs`);
      remove any associated `allow` annotation; run `cargo clippy` and confirm no new
      `dead_code` warnings; confirm T070-34 still passes

**Checkpoint**: `cargo clippy` clean; labels tests green.

---

## Phase 2: Struct evolution

**Purpose**: Add `line` field and replace `FormalSpec` type before writing extraction logic,
so the compiler enforces completeness at every construction site.

### Tests (write first, must fail)

- [ ] T006 [US2] Write tests T070-03 and T070-04 — assert `line` is present and correct on
      symbols from `Foo.cls`; confirm they **fail** (field does not exist yet)
- [ ] T007 [US4] Write tests T070-09 through T070-13 using new fixture
      `tests/fixtures/MyApp/FormalSpec.cls`; confirm they **fail**

### Struct changes

- [ ] T008 [US2] Add `pub line: u32` with `#[serde(rename = "line")]` to `Symbol` in
      `src/tools/symbols_local.rs`; fix all construction sites to compile (set `line: 0`
      temporarily)
- [ ] T009 [US4] Add `ArgSpec` struct (fields: `name`, `type_name`, `byref`, `output`,
      `default`) with correct `serde` attributes; change `formal_spec` on `Symbol` from
      `Option<String>` to `Option<Vec<ArgSpec>>`; fix all construction sites (`formal_spec:
None` temporarily); add fixture `FormalSpec.cls`

**Checkpoint**: `cargo build` succeeds with placeholder values.

---

## Phase 3: Line numbers (US2)

- [ ] T010 [US2] Implement line population in every symbol constructor
      (`extract_method_symbol`, `extract_property_symbol`, `extract_parameter_symbol`, class
      symbol in `extract_cls_symbols`, label and macro in `extract_routine_nodes`) using
      `node.start_position().row as u32 + 1`
- [ ] T011 [US2] Write test T070-05 for routine labels in `Utils.mac`; confirm T070-03,
      T070-04, T070-05 all **pass**

---

## Phase 4: Return types (US3)

- [ ] T012 [US3] Add fixture `tests/fixtures/MyApp/TypedMembers.cls` with methods,
      properties, and parameters carrying explicit type annotations; write tests T070-06,
      T070-07, T070-08; confirm they **fail**
- [ ] T013 [US3] Implement `fn extract_typename(node: Node, source: &[u8]) -> Option<String>`
      in `symbols_local.rs`; call from `extract_method_symbol`, `extract_property_symbol`, and
      `extract_parameter_symbol`; confirm T070-06, T070-07, T070-08 **pass**

---

## Phase 5: Structured FormalSpec (US4)

- [ ] T014 [US4] Implement `fn parse_arguments(node: Node, source: &[u8]) -> Vec<ArgSpec>`
      walking `arguments → argument` and extracting name, type, byref, output, default; wire
      into `extract_method_symbol` and classmethod path; confirm T070-09 through T070-13
      **pass**

---

## Phase 6: All member kinds (US5)

- [ ] T015 [US5] Add fixture `tests/fixtures/MyApp/AllMembers.cls` with one definition of
      each new kind (`query`, `trigger`, `index`, `xdata`, `storage`, `relationship`,
      `foreignkey`, `projection`)
- [ ] T016 [US5] Write tests T070-14 through T070-21; confirm they all **fail**
- [ ] T017 [US5] Add `fn extract_member_name(node: Node, source: &[u8]) -> Option<String>`
      helper; extend `match member.kind()` in `extract_cls_members` to cover all eight new
      kinds; confirm T070-14 through T070-21 **pass**

---

## Phase 7: Routine name from grammar (US6)

- [ ] T018 [US6] Add fixture `tests/fixtures/NamedRoutine.mac` with `ROUTINE NamedRoutine`
      header; write tests T070-22, T070-23, T070-24; confirm they **fail**
- [ ] T019 [US6] In `extract_routine_symbols`, walk `root` for
      `routine_definition → routine_name`; use `node_text` result as routine name; fall back to
      file stem when absent; confirm T070-22, T070-23, T070-24 **pass**

---

## Phase 8: Case-insensitive glob (US7)

- [ ] T020 [US7] Write tests T070-25, T070-26, T070-27; confirm they **fail**
- [ ] T021 [US7] At top of `glob_match` add `let query = query.to_uppercase(); let name =
name.to_uppercase();`; confirm T070-25 through T070-27 **pass**; confirm existing glob
      tests still pass

---

## Phase 9: Member-level glob filter (US8)

- [ ] T022 [US8] Write tests T070-28, T070-29, T070-30 using `Foo.cls`; confirm T070-28
      **fails** (member filter not yet applied)
- [ ] T023 [US8] Implement `fn split_member_query(query: &str) -> (String, Option<String>)`;
      apply member-glob filtering in `extract_cls_members`; confirm T070-28, T070-29, T070-30
      all **pass**

---

## Phase 10: `kinds` filter (US9)

- [ ] T024 [US9] Write tests T070-31, T070-32, T070-33; confirm they **fail**
- [ ] T025 [US9] Add `kinds: Option<Vec<String>>` (with `#[serde(default)]`) to
      `SymbolsLocalParams` in `src/tools/mod.rs`; update `scan_workspace` signature to accept
      `kinds: Option<&[String]>`; add filter logic after each symbol construction; update all
      call-sites to pass `None` or the new field; confirm T070-31 through T070-33 **pass**

---

## Phase 11: Structured FormalSpec in `docs_introspect` (US11)

- [ ] T029 [US11] Research `hkimura-intersys/objectscript-lsp` — find the FormalSpec
      parsing code (search for `ByRef`, `FormalSpec`, `formal_spec` in the Rust source);
      determine whether its format is compatible with `ArgSpec` from US4; document findings
      in a comment at the top of the implementation task; if a port or alignment is
      possible, note the source file and function name
- [ ] T030 [US11] Write unit tests T070-36 through T070-38 for `parse_formalspec_string`
      in `symbols_local.rs` `#[cfg(test)]`: standard args, empty string, Output prefix;
      confirm they **fail** (function does not exist yet)
- [ ] T031 [US11] Implement `pub fn parse_formalspec_string(s: &str) -> Vec<ArgSpec>` in
      `symbols_local.rs`; handle comma split, `ByRef`/`Output` prefixes, `:type`, `=default`
      with quoted strings; align with findings from T029; confirm T070-36 through T070-38
      **pass**
- [ ] T032 [US11] Write `#[ignore]` integration test T070-39: call `docs_introspect` on a
      known class in the live container; assert `FormalSpec` field in response is a JSON array
      (not a string); confirm test **fails** (raw string still returned)
- [ ] T033 [US11] In `mod.rs` `docs_introspect` handler (~line 4471), after fetching
      `FormalSpec` string from `%Dictionary.CompiledMethod`, call `parse_formalspec_string`
      and replace the string value with the parsed array in the JSON response; confirm T070-39
      **passes** against live container

---

## Phase 12: XData flow parsing (US12 + US13)

**Purpose**: Expose BPL and DTL logic that lives in XData blocks — invisible to all current
tools — through `docs_introspect` and `extract_message_map_routing`.

### Fixtures (write first)

- [ ] T037 [US12] Create fixture directory `tests/fixtures/xdata/`; add `bpl_simple.xml` —
      a minimal BPL `<process>` with one `<code>` and one `<call async='1'>` step; add
      `bpl_dynamic.xml` — a `<code>` block whose CDATA contains `$classmethod`; add
      `bpl_nested.xml` — a `<sequence>` with a `<if>` wrapping an inner `<call>` step; add
      `dtl_simple.xml` — a `<transform>` with two `<subtransform>` elements and three
      `<assign>` elements
- [ ] T038 [US13] Add `#[cfg(test)]` fixture-loading helpers in `xdata_flow.rs` that read
      the XML files above as `&str`

### Unit tests for `xdata_flow.rs` (write first, must fail)

- [ ] T039 [US12] Write tests T070-40 through T070-44 in
      `tests/xdata_flow_tests.rs`:
  - T070-40: `parse_bpl(bpl_simple.xml)` → one `Code` step + one `Call` step with correct
    `target` and `async_ = true`
  - T070-41: `parse_bpl(bpl_dynamic.xml)` → `has_dynamic_dispatch = true`
  - T070-42: `parse_bpl(bpl_nested.xml)` → outer `If` step with inner `Call` in its
    `steps` vec
  - T070-43: `parse_dtl(dtl_simple.xml)` → two subtransforms, `assign_count = 3`
  - T070-44: `parse_bpl` on empty `<process/>` → `steps` is empty, no panic
    Confirm all **fail** (module does not exist yet)
- [ ] T040 [US12] Write `#[ignore]` integration tests T070-45 through T070-47 in
      `tests/xdata_flow_tests.rs`:
  - T070-45: `docs_introspect` on a live BPL class → response contains `xdata_flow.kind =
"bpl"` and at least one `Call` step
  - T070-46: `docs_introspect` on a live DTL class → response contains `xdata_flow.kind =
"dtl"`, `source_class` and `target_class` populated
  - T070-47: `docs_introspect` on a plain class (non-BPL/DTL) → no `xdata_flow` key in
    response
    Confirm all **fail**
- [ ] T041 [US13] Write `#[ignore]` integration tests T070-48 through T070-52 in
      `tests/xdata_flow_tests.rs`:
  - T070-48: `extract_message_map_routing` on a live BPL class → `kind = "bpl"`, `routes`
    has one entry per `<call>` step
  - T070-49: BPL with `$classmethod` in `<code>` → `note` field present
  - T070-50: `extract_message_map_routing` on a live DTL class → `kind = "dtl"`, `routes`
    empty
  - T070-51: `extract_message_map_routing` on an existing router class → existing behaviour
    unchanged
  - T070-52: `extract_message_map_routing` on a plain class with no MessageMap and no
    BPL/DTL → NOT_FOUND (existing behaviour unchanged)
    Confirm all **fail**

### Implementation

- [ ] T042 [US12] Create `crates/iris-agentic-dev-core/src/tools/xdata_flow.rs`; add
      `quick-xml = "0.37"` to `Cargo.toml`; implement `XDataFlow`, `BplFlow`, `BplStep`,
      `DtlFlow`, `ContextProperty`, `Subtransform` structs with `Serialize`; implement
      `pub fn parse_bpl(xml: &str) -> Result<BplFlow>` and
      `pub fn parse_dtl(xml: &str) -> Result<DtlFlow>` using `quick-xml` pull reader;
      confirm T070-40 through T070-44 all **pass**
- [ ] T043 [US12] In `mod.rs` `docs_introspect` handler: after fetching class name and
      superclass from `%Dictionary.CompiledClass`, check `Super` for `Ens.BusinessProcessBPL`
      / `Ens.DataTransformDTL`; if detected, call `iris_execute` with
      `$system.OBJ.ExportToStream` to get the class XML, extract the CDATA from the matching
      `<XData name="BPL">` or `<XData name="DTL">` block, call `parse_bpl` or `parse_dtl`,
      serialize result into `xdata_flow` key in response JSON; confirm T070-45 through
      T070-47 **pass** against live container
- [ ] T044 [US13] In `mod.rs` `extract_message_map_routing` handler: after the existing
      NOT_FOUND path, add BPL/DTL detection (same superclass check); call
      `xdata_flow::parse_bpl` / `parse_dtl` (shared with T042); map `BplStep::Call` entries
      to `routes` vec; include `note` when `has_dynamic_dispatch`; for DTL return `kind =
    "dtl"` with source/target and empty routes; confirm T070-48 through T070-52 **pass**
      against live container

---

## Phase 13: Final validation

- [ ] T034 Run full test suite: `cargo test -p iris-agentic-dev-core`; confirm all tests
      pass including pre-existing `symbols_local_tests.rs` tests
- [ ] T035 Run `cargo clippy -p iris-agentic-dev-core -- -D warnings`; confirm zero warnings
      in `symbols_local.rs`, `mod.rs`, and `xdata_flow.rs`
- [ ] T036 Update tool description string in `mod.rs` for `iris_symbols_local` to mention
      `kinds` filter and `line` field; update `docs_introspect` description to note
      structured `FormalSpec` and `xdata_flow` for BPL/DTL classes; update
      `extract_message_map_routing` description to note BPL/DTL support

---

## Dependency graph

```text
T001 (bump crates)
  └─ T002, T003 (US1 fixture + test)

T004, T005 (US10 dead code)  ← independent of T001

T006, T007 (write US2/US4 tests)
  └─ T008, T009 (struct evolution) ← requires T005
    ├─ T010, T011 (US2 line numbers)
    ├─ T012, T013 (US3 return types)
    ├─ T014     (US4 formal spec) ← also enables T029–T032
    ├─ T015–T017 (US5 new kinds)
    ├─ T018, T019 (US6 routine name)
    ├─ T020, T021 (US7 case-insensitive)
    ├─ T022, T023 (US8 member glob)
    └─ T024, T025 (US9 kinds filter)

T029 (US11 research) ← independent, do before T031
T030–T033 (US11 impl) ← requires T014 (ArgSpec struct) + T029

T037, T038 (US12/US13 fixtures)  ← independent of all above
T039, T040, T041 (US12/US13 tests, must fail)
  └─ T042 (xdata_flow.rs + quick-xml, US12 unit tests pass)
    ├─ T043 (docs_introspect BPL/DTL, US12 integration tests pass)
    └─ T044 (extract_message_map_routing BPL/DTL, US13 integration tests pass)

T034, T035, T036 (final validation) ← all of above
```
