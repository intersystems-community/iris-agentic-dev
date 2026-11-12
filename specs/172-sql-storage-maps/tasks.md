# Tasks: SQL storage maps (172)

**Input**: `specs/172-sql-storage-maps/` — spec.md, plan.md, research.md, data-model.md,
contracts/iris_storage_map.md, quickstart.md; ADR `docs/adr/0004-sql-storage-maps-are-edited-through-a-model.md`

**Who does what**: implementation teammates (sonnet) take one task at a time in their own worktree. The lead reviews
each diff, runs the full suite and commits. A teammate never commits, pushes or merges.

## Rules for every task

- Write the test named in the task first, run it, and see it fail for the right reason. Then write the code, and run
  the same command until it passes. Paste both outputs (red, then green) in your report.
- Read `CLAUDE.md` (worktree root) before your first task: always `--features testing`; a new test file needs a
  `mod` line in its target's `main.rs`; change the environment only through `testing::EnvGuard`.
- Live tests use `live_rig_171::{rig, Where}` (`tests/integration/live_rig_171.rs`): `Where::live()` panics when
  iris-dev-iris is not running. Never return early from a test to skip it.
- Never mock IRIS. Never copy a class, global, field name or code from anywhere outside this repository into a
  fixture: every fixture is invented for this repository.
- Paths below are relative to `crates/iris-agentic-dev-core/` unless they start with `/` (repository root).

## Commands (run from the worktree root)

- `U <filter>` = `RUST_TEST_THREADS=12 cargo test --features testing --test unit <filter>`
- `L <filter>` = `cargo test --features testing --test integration <filter> -- --test-threads=1 --include-ignored`
- `B <filter>` = `cargo build && IAD_BINARY=./target/debug/iris-agentic-dev cargo test --features testing --test binary <filter> -- --include-ignored`
- `LINT` = `cargo fmt --all && cargo clippy --features testing --all-targets -- -D warnings`

Every task ends with `LINT` clean.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story in spec.md (US1 … US11)

---

## Phase 0: Prerequisites (blocking)

- [ ] T001 Lead only: confirm the private prerequisite tracked outside this repository is settled (spec Assumptions,
      plan "Prerequisites" item 2). It gates Phase 6 onward (the write path) and any push of this branch; Phases
      1–5 (read side) may start before it.
- [x] T002 Lead only: rebase `172-sql-storage-maps` onto master once `fix/doc-put-result-status` is merged, so
      `iris::connection::check_atelier_put_errors` exists. Prove: `U doc_put_result_status`.

---

## Phase 1: Setup

**Purpose**: the tool exists, answers `NOT_IMPLEMENTED` for every action, and every registration guard is green.

- [x] T003 Create `src/tools/storage_map/mod.rs` with `pub mod tree; pub mod model; pub mod path; pub mod validate;
pub mod code; pub mod globals; pub mod diagram; pub mod sample; pub mod uses; pub mod write; pub mod explain;
pub mod shape;` and an empty file for each module; add `pub mod storage_map;` to `src/tools/mod.rs` next to
      `pub mod storage_guard;` (line ~122). Prove: `cargo build --features testing`.
- [x] T004 Add `pub mod batch8;` to `src/tools/params/mod.rs` and create `src/tools/params/batch8.rs` with
      `IrisStorageMapParams` exactly as contracts/iris_storage_map.md "Params" lists (fields `action: String` with
      `#[schemars(extend("enum" = ["read","diagram","uses","compare","preview","propose","check","edit","replace"]))]`,
      `class`, `map`, `global`, `namespaces: Option<Vec<String>>`, `version`, `ops: Option<Vec<Value>>`,
      `model: Option<Value>`, `confirm_removal`, `seed_counter`, `build_indexes`, `include_values`,
      `#[serde(rename = "acknowledgePhi")] acknowledge_phi`, `max_nodes: Option<u32>`, `max_rows: Option<u32>`,
      `namespace`, `server`, `confirm`), `#[serde(deny_unknown_fields)]`, derive like `ResolveStorageParams` in
      `batch7.rs:85`. Test first: `tests/unit/test_storage_map_params_172.rs` parses a JSON string with every field
      and one with an unknown field (must fail). Prove: `U test_storage_map_params_172`.
- [x] T005 Add the tool method `iris_storage_map` in `src/tools/mod.rs` beside `resolve_storage` (~:10072), shaped
      like `iris_global` (~:7426): `target_conn()`, `conn_client`, call `storage_map::handle(&iris, client, &p,
self).await`, `record_call`. In `storage_map/mod.rs` add `pub async fn handle(...) -> Result<CallToolResult,
McpError>` that returns `err_json("NOT_IMPLEMENTED", "<action>")` for each known action and `INVALID_ACTION`
      otherwise. Add the `dispatch!("iris_storage_map", IrisStorageMapParams, iris_storage_map);` arm in
      `dispatch_by_name` (~:12030). Prove: `cargo build --features testing`.
- [x] T006 Register the tool name everywhere the guards check (list from plan.md and docs/invariants.md "a tool"
      row): `write_gate.rs:658` `CLASSIFICATION` row from plan.md "Classification" (all actions as in the table; opt-in
      raising comes in T064); `iris/server_manager.rs:971` `tool_to_category` arm (`Docs`); `write_gate.rs:1131`
      `effective_category` arms (check/edit/replace → `Compile`; compare/propose/preview → `Execute`);
      `/crates/iris-agentic-dev-bin/src/cmd/tool.rs:20` `TOOL_NAMES` (sorted); `output_schemas.rs` an
      `IrisStorageMapOk` struct (`success`, `model_version`, `action`, plus `#[serde(flatten)] rest: Value`) and
      `IrisStorageMapResponse` enum like :1320-1336, the name in `tests/unit/test_output_schema.rs:19` and its count at
      :246; toolset counts in `tests/unit/test_toolset.rs:131` and `:264`; `/tests/e2e/skill_eval/tool_applicability.toml`
      (`iris_storage_map = "storage"`). Prove: `U test_gate_classification`, `U test_enum_gate_classes`,
      `U test_tool_category_coverage`, `U test_output_schema`, `U test_toolset`, then the whole `U` target.
- [x] T007 Add `iris_storage_map` to `/docs/tools.md`: a quick-reference row, and a section headed with the tool
      name whose parameter table matches T004's struct exactly. Prove:
      `U every_documented_tool_parameter_is_in_the_input_schema`.
- [x] T008 Add one case per read-only action (`read`, `diagram`, `uses`) to `tests/integration/test_enum_tools_work_live.rs:50`
      `cases()`, expecting `NOT_IMPLEMENTED` for now (later tasks change the expectation). Prove: `L test_enum_tools_work_live`.
- [x] T009 [P] Binary schema test `tests/binary/test_storage_map_schema_172.rs`: `tools/list` advertises
      `iris_storage_map` with `additionalProperties: false`, the `action` enum of nine values, and every property of
      T004. Use `ParamContract` and `assert_advertised_contracts` as in `tests/binary/schema_batch7.rs`. Prove:
      `B test_storage_map_schema_172`.
- [x] T010 Add `IAD_STORAGE_CORPUS` and `IAD_REGEN_GOLDEN` to `BEHAVIOR_ENV_VARS` in `src/testing.rs`. Prove:
      `U behavior_env` (the existing registry guard) and the whole `U` target.
- [x] T010a Constitution IV: `tests/unit/test_storage_map_unreachable_172.rs` calls `iris_storage_map` for every
      action on an `IrisTools` with no connection (`iris: None`) and asserts `IRIS_UNREACHABLE` (not `NOT_IMPLEMENTED`,
      and no panic). Prove: `U test_storage_map_unreachable_172`.

**Checkpoint**: the full `U` target is green, and `iris_storage_map` is listed.

---

## Phase 2: Foundational (fixtures, tree, model base, call shape)

**Purpose**: the pure parser and writer, the fixtures and the one action reader. Nothing after this phase starts
until T011–T036 are done.

### Fixtures (live; each task writes one class and proves it on iris-dev-iris)

Each fixture is `/crates/iris-agentic-dev-core/tests/fixtures/storage172/classes/<Name>.cls` in canonical form (the
exact text IRIS returns on GET after a PUT) plus `seed/<Name>.json`:
`{"class": "...", "globals": [{"ref": "^IadSm172X(1)", "value": "..."}], "question": "<SELECT ...>", "answer": [[...]]}`.
Package `IadSm172`, globals `^IadSm172*`, invented domain (a small library: members, loans, books, branches).

- [x] T011 Live rig `tests/integration/storage172_rig.rs` (add `mod` line): `pub async fn load_fixture(r: &Rig, name:
&str)` (drop first, kill its globals, set the seed nodes via `iris_global set`, PUT with `iris_doc` put +
      compile, assert success), `pub async fn drop_fixture(r: &Rig, name: &str)` (delete class, kill globals, assert),
      `pub async fn ask(r: &Rig, name: &str) -> Vec<Vec<Value>>` (runs the seed's question with `iris_query`). Test:
      `tests/integration/test_storage172_rig_live.rs` loads `Member` (T012), asks, asserts the seed's answer, drops,
      asserts the class is gone. Prove: `L test_storage172_rig_live`.
- [x] T012 [P] Fixture `Member`: one data map, `^`-delimited pieces on the ID node, ID from `SqlIdExpression`
      `$i(^IadSm172Member)`. Known answer: name of member 2. Prove: a case in `test_storage172_rig_live`.
- [x] T013 [P] Fixture `LoanList`: data fields with no `<Delimiter>` (so `$LIST`). Prove: rig case.
- [x] T014 [P] Fixture `BookMulti`: fields on three different nodes (`"title"`, `"pub"`, `"loc"`). Prove: rig case.
- [x] T015 [P] Fixture `Branch`: a computed field whose `RetrievalCode` reads the global through `{L1}`, with a `<`
      and `&` in the code (raw characters, CDATA). Prove: rig case asks the computed column.
- [x] T016 [P] Fixture `Shelf`: a subscript level with multi-line `NextCode` that contains a line that is only `}` and
      a tab character. Prove: rig case.
- [x] T017 [P] Fixture `MemberByCity`: `Member`'s shape plus an index map `CityIdx` on `^IadSm172MemberI`, index built
      by the seed. Prove: rig case asks `WHERE City = ?`.
- [x] T018 [P] Fixture `ActiveLoan`: a conditional index map (`PopulationType conditional`, a `Condition`). Prove: rig
      case.
- [x] T019 [P] Fixture `Copy`: composite row ID of three parts (RowIdSpec 1..3). Prove: rig case.
- [x] T020 [P] Fixtures `Loan` and `LoanEvent` (D11, nested): one global `^IadSm172Loan`; `Loan` rows at
      `^IadSm172Loan(id)="member^book^date"`, `LoanEvent` rows inside them at `^IadSm172Loan(id,"EV",n)`; the
      child's level-1 subscript is the parent ID and its level 2 is the literal `"EV"`. Prove: rig case joins them, and
      a `Loan` query returns no row for an `"EV"` node.
- [x] T021 [P] Fixture `MemberD11` (D11 inside one map): `^IadSm172MemberD(id)="name^city"` holds two fields and
      `^IadSm172MemberD(id,"contact")="email^phone"` two more, all one row. Prove: rig case returns all four columns.
- [x] T022 [P] Fixture `LoanUnion`: a `RowReference` map. Prove: rig case.
- [x] T023 [P] Fixture `Genre`: a bitmap index map. Prove: rig case.
- [x] T024 [P] Fixture `Notes`: a storage `<Description>` (CDATA and `///` lines), a code value containing `]]`, and
      `BlockCount 0` / `ConditionalWithHostVars 0` written by hand (IRIS drops them; the canonical file has them
      dropped). Prove: rig case.
- [x] T025 [P] Malformed bodies `tests/fixtures/storage172/malformed/*.txt`, each with a first line `# expect: <error
substring>`: unterminated element, unterminated CDATA, attribute other than `name`, text inside a container,
      two attributes, nesting depth 65, a 2 MiB body, a body that is only `]]` repeated. Prove: files exist (used by
      T031).

- [x] T026 Live D11 checks (FR-023b), recorded in research.md by the lead, run before any read or write code: (a) DELETE one `Loan` row through
      its map: are its `"EV"` children removed too? (b) a map that walks level 2 of `^IadSm172Loan` as row IDs: does
      it return the `"EV"` node as a row? (c) UPDATE of one `MemberD11` field: are the `contact` fields kept? Each
      as a test asserting what IRIS does. Put them in `tests/integration/test_storage_map_d11_172_live.rs`. Prove: `L test_storage_map_d11_172`.

### Tree (pure; port of `.iad-local/storage-maps/proto/model.py`, ask the lead for a copy)

- [x] T027 [P] `src/tools/storage_map/tree.rs`: `pub struct Node { pub tag: String, pub name: Option<String>, pub value:
Option<String>, pub children: Option<Vec<Node>> }`, `pub fn is_canonical_number(s: &str) -> bool`,
      `pub fn order_key(name: Option<&str>) -> OrderKey` (`$Order` collation: none < canonical numbers numerically <
      strings by code point), `pub fn sort_key(n: &Node)`. Test first `tests/unit/test_storage_map_tree_172.rs`:
      `1 < 2 < 10 < 11 < "A" < "a"`, `"01"` and `"1.0"` are strings, `-1 < 0`. Prove: `U test_storage_map_tree_172`.
- [x] T028 `tree.rs`: `pub fn parse_body(text: &str) -> Result<Vec<Node>, ParseError>` (`ParseError { offset: usize,
context: String, reason: String }`), iterative (an explicit stack, no recursion), linear time, no regex
      backtracking on values; limits `MAX_BODY_BYTES = 1 MiB`, `MAX_DEPTH = 32`. Tests first: each fixture's Storage
      body parses; the tree for `Member` has the expected tags in order. Prove: `U test_storage_map_tree_172`.
- [x] T029 `tree.rs`: `pub fn emit_body(nodes: &[Node]) -> String` with research R3 rules 1–2 (sort, CDATA on a new
      line for `<`, `>`, `&`, split at every `]]`) and `Description` always CDATA. Test first: for every fixture,
      `emit_body(parse_body(body))` equals the body byte for byte. Prove: `U test_storage_map_tree_172`.
- [x] T030 `tree.rs`: `pub fn normalize(nodes: Vec<Node>) -> Vec<Node>` (drop empty leaves; drop `BlockCount` and
      `ConditionalWithHostVars` equal to `0`; keep the R3 rule-4 list). Test first: `Notes`'s hand-written input with
      the defaults normalizes and emits to the canonical fixture text. Prove: `U test_storage_map_tree_172`.
- [x] T031 `tree.rs`: `pub fn split_class(text: &str) -> Result<Vec<StorageSpan>, ParseError>` returning every Storage
      block (`StorageSpan { name, start, body_start, body_end, end }`), the body end found by parsing elements, never
      by counting braces. Tests first: `Shelf` (NextCode with `}` lines) splits correctly; a class with two Storage
      blocks returns two; every malformed file returns `Err` whose reason contains its `# expect:` text, within 100 ms
      each. Prove: `U test_storage_map_tree_172`.
- [x] T032 Live round-trip over `%SYS`: `tests/integration/test_storage_map_sys_roundtrip_172_live.rs` lists every
      class with a compiled SQL map in `%SYS` (`SELECT DISTINCT parent FROM %Dictionary.CompiledStorageSQLMap`), GETs
      each, and asserts `emit_body(normalize(parse_body(body)))` equals the body for all of them; asserts the list is
      not empty (expect about 160 on 2026.2) and prints the count. Read-only. Prove:
      `L test_storage_map_sys_roundtrip_172`.

### Model base and call shape (pure)

- [x] T033 [P] `src/tools/storage_map/model.rs`: `pub const MODEL_VERSION: u32 = 1;` and `pub fn fnv1a64(bytes: &[u8])
-> u64` with `pub fn version_hex(text: &str) -> String` (16 lowercase hex digits). Test first
      `tests/unit/test_storage_map_model_172.rs`: the published FNV-1a 64 test vectors (`""` →
      `cbf29ce484222325`, `"a"` → `af63dc4c8601ec8c`). Prove: `U test_storage_map_model_172`.
- [x] T034 [P] `src/tools/storage_map/mod.rs`: `pub struct CallShape { pub action: Action, pub seed_counter: bool,
pub build_indexes: bool, pub include_values: bool, pub code_changed: Option<bool> }`, `pub enum Action` (nine
      variants), `pub fn call_shape(params: &Value) -> CallShape`. An action it cannot read (missing, not a string,
      unknown, any case or whitespace variant) becomes `Action::Edit` with both opt-ins `true` (FR-030a). Test first
      `tests/unit/test_storage_map_call_shape_172.rs`: every action, `" EDIT "`, `"Read"`, a number, a missing
      action, opt-ins as `true`/`false`/absent/`"true"` (a string is not `true`). Prove: `U test_storage_map_call_shape_172`.
      Fixed 2026-11-11 in review: `call_shape` had trimmed and lowercased the action, so `"Read"` was judged a read,
      and `handle` read the action itself. Both now go through `call_shape`, exact names only; `CallShape` gained
      `unreadable` and `acknowledge_phi`.
- [x] T035 Make `write_gate::classify` (`write_gate.rs:997`) call `storage_map::call_shape` for `iris_storage_map`
      only, and make `is_scratch_call` (`write_gate.rs:1064`) do the same (scratch for every action except read,
      diagram, uses). Test first in `tests/unit/test_storage_map_call_shape_172.rs`: `classify` and `is_scratch_call`
      for each action and for an unreadable one. Prove: `U test_storage_map_call_shape_172` and `U test_gate_classification`.
- [x] T036 Make `exec::needs_service_account` (`tools/exec.rs:72`) use `call_shape` for `iris_storage_map` and add the
      tool to `SERVICE_ACCOUNT_TOOLS` (`exec.rs:43`): service account for compare, propose, preview and when an
      opt-in is set; connection user otherwise (plan "Classification"). Test first in the same file: the decision for
      each row of that table. Prove: `U test_storage_map_call_shape_172`, `U service_account`.

**Checkpoint**: fixtures load and answer; every fixture and every `%SYS` map round-trips; `call_shape` is the one
reader.

---

## Phase 3: User Story 1 — Read a map as structured data (P1) 🎯 MVP

**Goal**: `read` returns the storage model; `resolve_storage` returns the maps.
**Independent test**: read each fixture; fields match `%Dictionary.CompiledStorageSQLMap*` for that class.

- [x] T037 [P] [US1] `model.rs`: the typed structs of data-model.md (`StorageModel`, `SqlMap`, `Subscript`,
      `DataField`, `RowIdPart`, `ExtraElement`) with `serde::Serialize`, field names exactly as data-model.md. Test
      first: serialising a hand-built `SqlMap` gives the documented keys. Prove: `U test_storage_map_model_172`.
- [x] T038 [US1] `model.rs`: `pub fn from_tree(class: &str, storage_name: &str, nodes: &[Node]) -> StorageModel`;
      unknown tags go to `extra` with tag, name and value; integers parsed from `name=` for subscripts, row ID parts,
      access variables and invalid conditions (a non-integer becomes a `derived` warning, not a panic). Test first:
      `Member`, `BookMulti`, `Copy` and `Shelf` give the expected models (assert specific fields, not only counts).
      Prove: `U test_storage_map_model_172`.
- [x] T039 [US1] `model.rs`: `pub fn derived_facts(m: &StorageModel) -> Vec<Derived>` per research R5 and FR-003
      (no delimiter → `$LIST`; master map = first data map; later data maps `used: false`; RetrievalCode → computed,
      not filed; writable only when the master map's global starts with `^`, `$` or `@`). Test first with `LoanList`,
      `Branch`, and a hand-built model with two data maps. Prove: `U test_storage_map_model_172`.
- [x] T040 [US1] `model.rs` D11 facts (FR-003a): `derived_facts` reports a row node that holds fields and has
      sub-nodes; `pub fn nested_maps(models: &[StorageModel]) -> Vec<(String, String)>` pairs a data map stored inside
      another (same global, extra literal level). Test first: `MemberD11` gives the first fact; `Loan` + `LoanEvent`
      give one pair; `Member` gives neither. Prove: `U test_storage_map_model_172`.
- [x] T041 [US1] `model.rs`: map and storage versions: `version_hex(emit_body(&[map_node]))` per map and over the
      whole normalized body. Test first: the version is stable across two parses and changes when one piece changes.
      Prove: `U test_storage_map_model_172`.
- [x] T042 [US1] `model.rs`: `pub fn default_storage(nodes: &[Node]) -> Option<DefaultStorage>` for default storage
      (`<Data name=…><Value name="n"><Value>Prop</Value>`), plus `editable` and `not_editable_reason` per data-model.md.
      Test first: a default-storage class text in `tests/fixtures/storage172/classes/DefaultBook.cls` (write it; plain
      `%Persistent` with three properties, as IRIS stores it). Prove: `U test_storage_map_model_172`.
- [x] T043 [US1] `mod.rs`: the `read` action: GET via `doc::fetch_doc_lines` (make it `pub(crate)`), `split_class`,
      parse each body, build the model; class missing → `STORAGE_MAP_NOT_FOUND`; parse failure →
      `STORAGE_MAP_PARSE_FAILED` with offset and context; class XML → refused (FR-012a). Answer per
      contracts "Answers". Test first `tests/integration/test_storage_map_read_172_live.rs`: read `Member` and assert
      global, subscript expression and each field's node/delimiter/piece, and compare them with
      `SELECT ... FROM %Dictionary.CompiledStorageSQLMapData WHERE parent = ?`; read a missing class; read
      `DefaultBook`. Prove: `L test_storage_map_read_172`.
- [x] T044 [US1] `admin_tools.rs:2366` `resolve_storage_impl`: add `sql_maps` (the `read` model's maps) to each
      storage of type `%Storage.SQL`; no key removed. Test first: a case in `test_storage_map_read_172_live.rs` for
      `Member`, plus a key-set check that the old keys are all still present. Prove: `L test_storage_map_read_172`.
- [x] T045 [US1] Change T008's `read` case to expect success. Prove: `L test_enum_tools_work_live`.

**Checkpoint**: US1 works alone (MVP).

---

## Phase 4: User Story 2 — Draw a map, find who uses a global (P1)

**Independent test**: diagram renders with `mmdc`; `uses` returns exactly the fixture classes.

- [x] T046 [P] [US2] `src/tools/storage_map/diagram.rs`: `pub fn mermaid(m: &StorageModel, map: Option<&str>) ->
String` and `pub fn graph(m: &StorageModel, map: Option<&str>) -> Graph { nodes: Vec<GNode{id, kind, label}>,
edges: Vec<GEdge{from, to}> }` per plan "Diagram"; `fn label(s: &str) -> String` keeps only letters, digits,
      space and `_.,:^(){}%$-`, everything else becomes `#<code>;`. Test first `tests/unit/test_storage_map_diagram_172.rs`:
      `Member` gives the expected node list; a field named `x"];click A call alert()` and a value containing
      `%%{init` produce no `%%` and no raw `"` or `]` in a label (`click` stays as inert quoted text), and a `%`
      that starts a `%%` pair is escaped. Prove: `U test_storage_map_diagram_172`.
- [x] T047 [US2] Render check `tests/integration/test_storage_map_diagram_render_172_live.rs`: for every fixture,
      write `mermaid()` to a temp file and run `mmdc -i <f> -o <f>.svg`; assert exit 0. If `mmdc` is missing, panic
      with the install hint unless `IAD_ALLOW_SKIP=1`. Prove: `L test_storage_map_diagram_render_172`.
- [x] T048 [US2] `mod.rs`: the `diagram` action (read, then `mermaid` + `graph`). Test first: a case in
      `test_storage_map_read_172_live.rs`. Prove: `L test_storage_map_read_172`.
- [x] T049 [P] [US2] `src/tools/storage_map/globals.rs`: `pub fn normalize_global(raw: &str) -> Result<GlobalRef,
String>` with `GlobalRef { name: String, extended: Option<String>, subscripts_prefix: Option<String> }`; accepts
      `^X`, `X`, `^X(...)`, `^|"NS"|X`, `^["NS"]X`, `^|"^^/db"|X`; anything with `@` or `$Name` → `Err("dynamic")`.
      Test first `tests/unit/test_storage_map_gates_172.rs`: each form, both caret spellings, case kept. Prove:
      `U test_storage_map_gates_172`.
- [x] T050 [US2] `src/tools/storage_map/uses.rs`: `pub async fn uses(iris, client, global: &GlobalRef, namespaces:
&[String]) -> Result<Value, ...>` with the two queries of research R11 through `iris.query`, matching after
      `normalize_global` on both sides. Test first `tests/integration/test_storage_map_uses_172_live.rs`: load
      `Member` and `MemberByCity`; `uses ^IadSm172Member` returns both data maps; `uses ^IadSm172MemberI` returns the
      index map; an unused global returns `[]` and names the namespace searched. Prove: `L test_storage_map_uses_172`.
- [x] T051 [US2] Wire `uses` in `mod.rs`; change T008's `diagram` and `uses` cases to expect success. Prove:
      `L test_enum_tools_work_live` (CI only: the sweep is not run locally, see the XDR note in plan.md).

---

## Phase 5: User Story 8 — Map globals and map code go through iad's gates (P1)

**Independent test**: each refusal from both entry points, both spellings; each handler calls the gate.

- [x] T052 [P] [US8] `src/tools/storage_map/code.rs`: `pub const CODE_TAGS: &[&str]` (research R6 list) and
      `pub fn code_elements(m: &[Node]) -> Vec<(Path, String)>`; every element value is treated as code unless its tag
      is in `LITERAL_TAGS` (`Type`, `Delimiter`, `Piece`, `Node`, `PopulationType`, `PopulationPct`, `BlockCount`,
      `Structure`, `Deferred`, `ConditionalWithHostVars`, `Field`, `AccessType`). Test first
      `tests/unit/test_storage_map_gates_172.rs`: `Shelf` and `Branch` yield their code elements; an unknown tag's
      value is yielded as code. Prove: `U test_storage_map_gates_172`.
- [x] T053 [US8] `code.rs`: `pub fn check_map_code(nodes: &[Node], server: &str) -> Option<Value>` calling
      `policy::code_edit_gate::check_objectscript_code_edit` on each code element. Test first: a NextCode that names
      a code-storage global and one that uses `$system.OBJ.Compile` are refused with `CODE_EDIT_BLOCKED`; `Shelf`'s
      NextCode passes. Prove: `U test_storage_map_gates_172`.
- [x] T054 [US8] `code.rs`: `pub fn dynamic_globals(nodes: &[Node]) -> Vec<String>` (FR-028: `AccessType Global`,
      `@`, `$Name` in `Global`, `DataAccess`, `RowReference`) and `pub fn code_changed(before: &[Node], after: &[Node])
-> bool`. Test first: three dynamic forms found, a plain map none; `code_changed` true when only a NextCode
      changes, false when only a piece changes. Prove: `U test_storage_map_gates_172`.
- [x] T055 [US8] Make `CODE_STORAGE_GLOBALS` (`policy/code_edit_gate.rs:84`) `pub(crate)`. `globals.rs`: `pub fn
check_map_globals(nodes: &[Node], policy: &EffectivePolicy, server: &str, action: Action) -> Option<Value>`:
      every global a map or storage keyword names (plan "Gates in the handler" step 1) through `CODE_STORAGE_GLOBALS`,
      `check_system_blocklist` (with the server's blocklist), `check_phi_name_gate`; an extended reference refused for
      writes and sampling. Test first: `^ROUTINE`, `ROUTINE`, `^rOBJ` in `Global`, `^oddDEF` in `IdLocation`, a
      custom-blocklist global, a PHI-named global, `^["%SYS"]X` on `edit` (refused) and on `read` (allowed). Prove:
      `U test_storage_map_gates_172`.
- [x] T056 [US8] Add `STORAGE_MAP_DYNAMIC_GLOBAL_BLOCKED` to `policy/refusal.rs` `REFUSAL_CODES` (:35) with a
      `describe` arm (operator limit; lift names `allowDynamicMapGlobals`), and a row to `/docs/error-codes.md`.
      Prove: `U test_refusal_codes_guard_141`, `U test_docs_contract`.
- [x] T057 [US8] Add the loosening setting `allow_dynamic_map_globals: bool` (TOML key `allowDynamicMapGlobals`, camelCase like the other `[policy.x]` keys) to the per-server policy:
      `iris/workspace_config.rs` (beside `data_policy_kill_allowlist`), `PolicyLayer` in `policy/effective.rs`,
      `strip_operator_loosening` (:576) and the untrusted-file set-aside, `check_config` output, `/docs/connecting.md`.
      Test first `tests/unit/test_storage_map_policy_172.rs`: parse a TOML string with the key (unit/TOML layer), an
      untrusted workspace file sets it aside, an operator file keeps it. Prove: `U test_storage_map_policy_172`.
- [x] T058 [US8] `mod.rs`: `pub fn gate_tree(nodes: &[Node], shape: &CallShape, policy, server) -> Option<Value>` runs
      T055, T053, T054 (refuse dynamic unless the setting) in that order. Test first: one case per refusal through
      `gate_tree`. Prove: `U test_storage_map_gates_172`.
- [x] T059 [US8] Handler-calls-gate test `tests/integration/test_storage_map_gates_172_live.rs`: for `check`, `edit`,
      `replace`, `compare`, `preview` and `propose`, a call whose model names `^ROUTINE` is refused with the fixed-rule
      refusal and nothing is written (GET the class before and after); same for `^routine` spelled without the caret.
      Prove: `L test_storage_map_gates_172` (it fails until the actions exist; the lead keeps it red-listed until
      T086).
      Written 2026-11-11 and red as planned: `check`, `preview`, `replace` and `edit` answer `NOT_IMPLEMENTED`;
      `propose` is already refused by the dispatch gate on its `global` param. `compare` takes no caller tree, so its
      gate is covered by `gate_tree` in the unit tests.

---

## Phase 6: User Story 4 — Check a change before the real class is touched (P1)

**Independent test**: a map that fails to compile leaves the real class and table working.

- [x] T060 [US4] Live check first (research R7 open item): `tests/integration/test_storage_map_scratch_172_live.rs`
      loads `Member`, records `$Query` checksums of `^IadSm172Member*`, compiles a scratch copy over the same
      globals and deletes it, and asserts the checksums are unchanged. Use `iris_doc` put/compile/delete for now.
      Prove: `L test_storage_map_scratch_172`.
      Done 2026-11-11: compile and Atelier delete of a copy over `Member`'s real globals left every node's CRC
      unchanged (the test also proves the checksum sees a one-node change). R7's open item is closed.
- [x] T061 [US4] `iris/connection.rs`: `pub async fn compile_scratch_class(&self, source: &str, namespace: &str,
client: &reqwest::Client, prefix: &str) -> anyhow::Result<(String, CompileResult)>` built on `ScratchGuard`,
      `compile_document` and `delete_scratch`; returns the scratch class name; uses `check_atelier_put_errors`.
      Test first: a case in `test_storage_map_scratch_172_live.rs` (a valid source compiles and is gone afterwards; an
      invalid one returns its errors and is gone). Prove: `L test_storage_map_scratch_172`.
      Done 2026-11-11: takes `source: impl FnOnce(&str) -> String` (the name is generated first, so the source
      can name it); refuses under strict read-only; a stored-but-flagged PUT (#16023) comes back as a failed result.
- [x] T062 [US4] `write.rs`: `pub fn scratch_source(real_text: &str, storage: &[Node], scratch_class: &str) ->
Result<String, String>` per FR-015a (class header with the same superclasses, the properties and indices the map
      names, `SqlTableName` = scratch name, emitted Storage; nothing else). Test first
      `tests/unit/test_storage_map_write_172.rs`: from `Member` plus an added method, a query and an XData block, the
      output has none of the three and has the Storage block byte-identical to `emit_body`. Prove:
      `U test_storage_map_write_172`.
      Done 2026-11-11: keeps `Import` lines, `SqlRowIdName`/`SqlRowIdPrivate`, IdKey indices and indices named
      like a map. Refuses default storage (no SQLMap): read-only per FR-002, and IRIS refuses a second class on its
      globals (#5564, measured on `DefaultBook`). A `Relationship` is not carried. Live: every SQL-storage fixture's
      scratch copy compiles (`test_storage_map_scratch_172_scratch_source_compiles_for_every_fixture`).
- [x] T063 [P] [US4] `src/tools/storage_map/explain.rs`: `pub fn explain(iris_error: &str) -> Option<String>` for
      5002, 5508–5511, 5513, 5524, 5538, 5539, 5541–5549, 5583, 5585, 5596, 15511, 16023, each in plain words from
      research R4/R5. Test first `tests/unit/test_storage_map_explain_172.rs`: each number maps to text; an unknown
      number gives `None`. Prove: `U test_storage_map_explain_172`.
      Done 2026-11-11: texts from `^%qMsg("%ObjectErrors")` on 2026.2 plus R4/R5; the first known `#nnnn` wins, so
      the #5030 wrapper never hides the map error.
- [x] T064 [US4] `classify` raise for opt-ins and code: `call_shape` sets `code_changed`; `classify` returns Write
      for compare/propose/preview, Destructive with `build_indexes`; `effective_category` adds Execute when
      `seed_counter`, `build_indexes` or `code_changed`. Test first in `test_storage_map_call_shape_172.rs`, one case
      per row of plan "Classification". Prove: `U test_storage_map_call_shape_172`, `U test_gate_classification`.
      Done 2026-11-11: `code_changed` is judged from the call alone (an op path or model key outside the literal
      list, or unreadable ops), so it can over-flag and never under-flag; `effective_category` now reads
      `call_shape` (`check` was falling back to `docs`).
- [x] T065 [US4] `mod.rs`: the `check` action: build the tree from `model` (or the class), `gate_tree`,
      `validate` (T070 lands later; until then `check` calls a stub that returns `Ok`), `scratch_source`,
      `check_compile_time_code_mode`, `compile_scratch_class`; failure → `STORAGE_MAP_COMPILE_FAILED` with
      `iris_errors` and `explanations`. Test first `tests/integration/test_storage_map_edit_172_live.rs`: `check` of
      `Member` with `<Type>DATA</Type>` fails with #5511 explained, the real class is unchanged and still answers its
      known question; a valid `check` succeeds and leaves no `IrisDevTmp` class. Prove: `L test_storage_map_edit_172`.
      Done 2026-11-11: a `model` replaces the map named by `map` (or its own `name`; a new name adds a map), via
      `model::map_node` (unknown keys are `INVALID_PARAMS`). Default storage is `STORAGE_MAP_READ_ONLY`. The
      compile-time gate runs on the copy under a stand-in name before any request. Handler refusals now leave
      through `classify_answer`, so `check` in T059 is a fixed-rule refusal. `read` had lost every
      `Invalidcondition` expression (IRIS stores it as an `<Expression>` child); fixed with the inverse.
- [x] T066 [US4] Strict read-only live test: in `test_storage_map_edit_172_live.rs`, under `read_only = "strict"`,
      `check`, `edit`, `replace`, `preview`, `compare`, `propose` return `READ_ONLY_STRICT` and `read`, `diagram`,
      `uses` succeed; and add `iris_storage_map` cases to `tests/integration/test_strict_sweep_171_live.rs`. Prove:
      `L test_storage_map_edit_172`, `L test_strict_sweep_171`.
      Done 2026-11-11: `test_storage_map_edit_172_strict_refuses_writes_and_keeps_reads`, and
      `storage_map_writes_are_refused_under_strict_without_compiling` in the strict sweep file (focused, not the full
      sweep, which is not run locally). `live_rig_171::rig_with` takes the operator file.

---

## Phase 7: User Story 3 — Edit a map without losing anything (P1)

**Independent test**: no-change write is byte-identical; a one-field edit changes one element.

- [x] T067 [P] [US3] `src/tools/storage_map/path.rs`: `pub enum PathSeg`, `pub fn parse_path(p: &str) ->
Result<Vec<PathSeg>, String>` for every path form in data-model.md "Paths". Test first
      `tests/unit/test_storage_map_path_172.rs`: each documented path; `/maps//x`, `/maps/Master/subscripts/a/x`
      rejected. Prove: `U test_storage_map_path_172`.
      Done 2026-11-11: paths map JSON keys to element tags level by level; `/keywords/<Tag>` takes the 27 plain
      properties of `%Dictionary.StorageDefinition` (read from the compiled dictionary); integer names are canonical.
- [x] T068 [US3] `path.rs`: `pub fn apply(nodes: &mut Vec<Node>, ops: &[Value]) -> Result<Vec<String>, String>`
      (`set`, `set` null removes, `add` fails if present, `remove`, `move` integer names only; all-or-nothing on a
      copy; returns changed paths). Test first: one test per op on `Member`; a failing third op leaves the tree
      unchanged; `remove` on `extra` works, `set` on `extra` fails. Prove: `U test_storage_map_path_172`.
      Done 2026-11-11: `set` on a whole element takes an object (`model::element_node`), `add` builds from the
      same model-JSON builders as `check`.
- [x] T069 [US3] Single-field-edit property test: for each fixture and each leaf, `set` it to a new value, emit, and
      assert the line diff against the original is exactly that element (plus CDATA wrapping where the value needs
      it). Prove: `U test_storage_map_path_172`.
      Done 2026-11-11: every addressable leaf of all 15 fixtures (prefix edit, so multi-line NextCode and CDATA
      keep their shape).
- [x] T070 [P] [US3] `src/tools/storage_map/validate.rs`: `pub fn validate(nodes: &[Node]) -> Result<(), Vec<Invalid>>`
      with every rule in data-model.md "Validation" (`Invalid { path, rule }`). Test first
      `tests/unit/test_storage_map_validate_172.rs`: one failing case per rule, and every fixture passes. Prove:
      `U test_storage_map_validate_172`.
      Done 2026-11-11: plus `validate_change(before, after)` so an element IRIS already holds is not refused as
      unknown; element names per level read from `%Dictionary.Storage*Definition` on 2026.2.
- [x] T071 [US3] `write.rs`: `pub fn new_class_text(original: &str, span: &StorageSpan, nodes: &[Node]) -> String`
      (prefix + `emit_body(normalize(nodes))` + suffix; only the Storage body changes) and `pub fn readback_diff(sent:
&[Node], stored: &[Node]) -> ReadbackDiff { dropped: Vec<String>, unexplained: Vec<String> }`. Test first in
      `test_storage_map_write_172.rs`: no-op gives the original text; an unknown element in `sent` shows in `dropped`;
      a reordered element shows in `unexplained`. Prove: `U test_storage_map_write_172`.
      Done 2026-11-11: a changed Description also rewrites the `///` lines IRIS writes above `Storage` (R3.3).
- [x] T072 [US3] `write.rs`: change detection (FR-010a). Live check first in `test_storage_map_edit_172_live.rs`:
      does Atelier PUT without `ignoreConflict=1` refuse a stale document timestamp on 2026.2? Record the answer in
      research.md R8 (lead edits the doc). Then `pub async fn guarded_put(iris, client, name, lines, ns,
expected_ts: &str) -> Result<String, Value>` that uses whichever works (timestamp header, or a GET just before
      the PUT comparing timestamps) and returns `STORAGE_MAP_VERSION_CONFLICT` on a change. Test: a write after
      another client's PUT is refused. Prove: `L test_storage_map_edit_172`.
      Done 2026-11-11: measured that a PUT without `ignoreConflict` honours `If-None-Match: <ts>` (409 on a stale
      or missing one); `guarded_put` relies on it (research R8).
- [x] T073 [US3] `write.rs`: `pub async fn write_map(ctx: &WriteCtx, original: &str, new_nodes: Vec<Node>) ->
Result<Value, Value>` running plan "Write path" in order: validate → `gate_tree` → data checks (stubbed `Ok`
      until T093) → scratch check → `guarded_put` → read-back + `gate_tree` again on what IRIS stored (FR-012a) →
      compile → restore on failure → opt-ins (stubbed until T094) → answer with `changed`, `versions`, `warnings`.
      Test first in `test_storage_map_edit_172_live.rs`: (a) no-change write of every fixture is byte-identical on GET;
      (b) adding `Email` to `Member` returns the new model and the known answer still holds; (c) an edit whose real
      compile fails restores the previous text and says `STORAGE_MAP_RESTORED`. Prove: `L test_storage_map_edit_172`.
      Done 2026-11-11. Found: a property no data map files fails the real compile (#5502) while the scratch copy,
      which carried only mapped properties, passed; the copy now carries every property and #5502 is explained.
      The restore test uses a method generator, code only the real compile runs.
- [x] T074 [US3] `mod.rs`: the `edit` action (version check against `read`'s map version → `STORAGE_MAP_VERSION_CONFLICT`
      with `current`; `apply`; `write_map`). Test first: stale version refused with the current map; columnar map and
      non-`%Storage.SQL` storage refused with `STORAGE_MAP_READ_ONLY`. Prove: `L test_storage_map_edit_172`.
      Done 2026-11-11: opt-ins are refused (`NOT_IMPLEMENTED`) until T094 rather than ignored after a write.
- [x] T075 [US3] `mod.rs`: the `replace` action (whole map from `model`; first call lists `removed` and returns
      `STORAGE_MAP_CONFIRM_REMOVAL`; second call with `confirm_removal: true` writes). Test first: both calls; a
      replace that removes nothing writes on the first call. Prove: `L test_storage_map_edit_172`.
      Done 2026-11-11: `removed` lists elements as well as leaves (an empty `<Data name="X"/>` counts).
- [x] T076 [US3] Audit (FR-030c): each map write adds `{class, map, version_before, version_after, changed_paths,
code_changed}` to the call record and to `irisAudit` emission when enabled; no code text. Test first
      `tests/unit/test_storage_map_audit_172.rs` on the record builder; a live case reads the local call log. Prove:
      `U test_storage_map_audit_172`, `L test_storage_map_edit_172`.
      Done 2026-11-11: a second audit entry, status `written`, after the write; params hold only paths and
      versions.
- [x] T077 [US3] Now that `validate` exists, replace T065's stub. Prove: `L test_storage_map_edit_172`.
      Done 2026-11-11 (with T070).

**Checkpoint**: US1–US4 and US8 done. T059 should now pass for `check`, `edit`, `replace`.

---

## Phase 8: User Story 9 — `iris_doc` and the off-limits rule (P2)

**Independent test**: an `iris_doc` put that breaks the map leaves the real class working and warns.

- [ ] T078 [US9] `write.rs`: `pub fn content_has_sql_map(text: &str) -> bool` (any Storage block holding an
      `SQLMap`, any storage type) and `pub fn is_class_xml(text: &str) -> bool`. Test first in
      `test_storage_map_write_172.rs`: fixtures true, `DefaultBook` false, an XML export with an SQLMap true for XML.
      Prove: `U test_storage_map_write_172`.
- [ ] T079 [US9] In `tools/doc.rs` `do_write` (:860), the single write sink: when `content_has_sql_map`, call
      `storage_map::write::from_doc(...)` (FR-031: FR-012, FR-012a, FR-014–FR-018, FR-023, FR-026–FR-028; no version
      check, no confirm-removal) and return today's answer shape plus the warning text from plan.md; class XML with a
      map is refused. Test first `tests/integration/test_storage_map_doc_172_live.rs`: (a) put of `Member` with
      `<Type>DATA</Type>` leaves the real class answering and returns the warning; (b) put with no map change adds no
      warning; (c) the confirmation-resume path (`doc.rs:310`) also goes through the safeguards; (d) a map over
      `^ROUTINE` via `iris_doc` is refused. Prove: `L test_storage_map_doc_172`.
- [ ] T080 [US9] Update `iris_doc`'s tool description (in `src/tools/mod.rs`) so it says default storage is off-limits
      and SQL storage maps are edited with `iris_storage_map`. Test first: a binary test in
      `tests/binary/test_storage_map_schema_172.rs` reads `tools/list` and asserts both statements. Prove:
      `B test_storage_map_schema_172`, `U test_description_tiers`.
- [ ] T081 [US9] Regression: run every existing `iris_doc` live test. Prove: `L test_doc_live`, `L test_doc_gate`,
      `L test_storage` (all existing).

---

## Phase 9: User Story 5 — Check a map against the real data (P2)

**Independent test**: `compare` reports a planted mismatch; `preview` returns the known row.

- [ ] T082 [P] [US5] `src/tools/storage_map/sample.rs`: `pub fn build_code(global: &GlobalRef, max_nodes: u32,
seconds: u32, include_values: bool) -> String` (FR-030b: the global name is validated and re-quoted by iad; no
      other caller text) and `pub fn parse(out: &str) -> Result<ShapeReport, String>` (`shape.rs` holds `ShapeReport`
      per data-model.md). Test first `tests/unit/test_storage_map_sample_172.rs`: the code for `^IadSm172Member`
      contains no caller text beyond the name; a name with `)` or `"` is rejected before code is built; `parse` of a
      captured output gives the expected report; the splice detector (`objectscript-splice`) passes on the file.
      Prove: `U test_storage_map_sample_172`.
- [ ] T083 [US5] `sample.rs`: `pub async fn sample(iris, client, ns, global, caps, include_values) -> Result<ShapeReport,
Value>` through `execute_via_generator` (service account per T036). Test first
      `tests/integration/test_storage_map_data_172_live.rs`: `Member` gives depth 1, delimiter `^`, pieces as seeded;
      `LoanList` gives `$LIST`; a 2,000-node global stops at 200 with `capped: true`; an empty value is plain. Prove:
      `L test_storage_map_data_172`.
- [ ] T084 [US5] `$Data` states in sampling (FR-020a): `sample` counts per level the nodes with `$Data` 1, 10 and 11,
      and lists masked literal sub-node names under value-holding nodes. Test first in
      `test_storage_map_data_172_live.rs`: `MemberD11` shows level 1 all 11 with one sub-node; `Loan` shows level 1
      all 11 with sub-node `"EV"`; `Member` shows level 1 all 1. Prove: `L test_storage_map_data_172`.
- [ ] T085 [US5] `compare` flags D11 mismatches (FR-020a): `MemberD11` with its `contact` fields removed from the map
      reports "row node has children the map does not read"; a map of only the `contact` fields reports the
      unread value. Prove: `L test_storage_map_data_172`.
- [ ] T086 [US5] `mod.rs`: the `compare` action (match the shape report against the map; `mismatches`). Test first:
      `Member` with a field moved to the wrong piece reports the mismatch with its node; a matching map reports none.
      Prove: `L test_storage_map_data_172`, then `L test_storage_map_gates_172` (T059) for compare.
- [ ] T087 [US5] `mod.rs`: the `preview` action: `gate_tree`, scratch copy of the proposed map, `SELECT TOP max_rows`
      through the scratch table as the service account, values masked unless `include_values`; scratch removed. Test
      first: preview of `Member`'s map returns the seed's known answer row (with `include_values`) and masked values
      without it. Prove: `L test_storage_map_data_172`.
- [ ] T088 [US5] Data policy (FR-021): add `iris_storage_map` to `HANDLER_ENFORCED_TOOLS` (`policy/data_policy_gate.rs:30`);
      in the handler, `include_values: true` is refused under block and redact and needs `acknowledgePhi` under allow
      (`check_body_read_acknowledged_as`); `names_argument` unchanged. Test first
      `tests/unit/test_storage_map_policy_172.rs`: each policy × include_values true/false/absent; and
      `test_body_read_ack_171` stays green. Prove: `U test_storage_map_policy_172`, `U test_body_read_ack_171`.
- [ ] T089 [US5] Identity: in `test_storage_map_data_172_live.rs`, with a service account configured (see
      `test_scratch_cleanup_141_live.rs` for the throwaway setup), `compare`/`preview` answers carry the service
      account in `identity`, and refuse when the service account cannot be used. Prove: `L test_storage_map_data_172`.

---

## Phase 10: User Story 6 — Refuse edits that damage existing data (P2)

**Independent test**: `$i` over a counterless global is refused; `seed_counter` fixes it.

- [ ] T090 [P] [US6] `write.rs`: `pub fn id_counter_ref(storage_keywords, master: &SqlMap) -> Option<GlobalRef>` (plain
      `$i(^G)` / `$i(^G(subs))` only; anything else → "not verifiable" text, FR-024). Test first in
      `test_storage_map_write_172.rs`: the forms, and three non-verifiable ones. Prove: `U test_storage_map_write_172`.
- [ ] T091 [US6] Live checks first in `tests/integration/test_storage_map_data_172_live.rs`: on a throwaway global with
      rows 1..3 and no counter, an INSERT through a `$i` map overwrites row 1 (reproduces research R5); a new index map
      over existing data answers nothing for a present value. Prove: `L test_storage_map_data_172`.
- [ ] T092 [US6] `write.rs` D11 check (FR-023b): refuse an edit whose walked level holds literal sub-node names the
      walk does not skip, naming the level and the masked node. Test first: the (b) map from T026 is refused; the same
      map with a `NextCode` that skips non-numeric subscripts passes. Prove: `L test_storage_map_data_172`.
- [ ] T093 [US6] `write.rs` data checks (FR-023): `STORAGE_MAP_ID_REUSE` naming counter and highest ID;
      `STORAGE_MAP_INDEX_UNBUILT`; FR-023a sharing checks (index range used by another map or non-empty; counter used
      by another class via `uses`). Replace T073's stub. Test first: each refusal in the data live test. Prove:
      `L test_storage_map_data_172`.
- [ ] T094 [US6] Opt-ins: `seed_counter` sets the counter (reports the old value), `build_indexes` runs
      `##class(<Class>).%BuildIndices($lb("<Map>"))` as the service account after compile; both classed per T064.
      Replace T073's stub. Test first: after `seed_counter`, an INSERT does not overwrite; after `build_indexes`, the
      present value is found; `build_indexes` with destructive tools off is refused. Prove: `L test_storage_map_data_172`.

---

## Phase 11: User Story 10 — A contract a GUI can build on (P2)

- [ ] T095 [P] [US10] Golden files: `tests/unit/test_storage_map_golden_172.rs` builds the `read`, `diagram` JSON for
      every fixture from its `.cls` text (no IRIS) and compares with `tests/fixtures/storage172/golden/<Name>.<action>.json`;
      `IAD_REGEN_GOLDEN=1` rewrites them (through `EnvGuard`); a test asserts `MODEL_VERSION` matches a constant in the
      golden directory's `VERSION` file, so a shape change without a bump fails. Prove: `U test_storage_map_golden_172`.
- [ ] T096 [US10] Golden live answers for `compare`, `preview`, `propose` on `Member` and `BookMulti`, stored with
      values masked. Prove: `L test_storage_map_golden_172`.
- [ ] T097 [US10] Lead writes `/docs/storage-maps.md` (model, actions, error codes, measured traps, the residual risk
      of saved map code from FR-027); a teammate adds `tests/unit/test_storage_maps_doc_172.rs` asserting every error
      code and action named there exists in the code and every documented trap names its live test. Prove:
      `U test_storage_maps_doc_172`.
- [ ] T098 [US10] Skill `/skills/skills/iris-storage-mapping/SKILL.md` (`tier: extra`, `managed_by: iris-agentic-dev`),
      `embedded_skill!("iris-storage-mapping")` in `src/skills/bundled.rs:145`, listed in `/iris-agentic-dev.toml`
      `provides.skills` and `/skills.sh.json`, and a line under `## Extra skills` in
      `/skills/skills/iris-agentic-dev/SKILL.md`. Lead writes the skill text; every factual claim cites a live test
      by name. Prove: `U test_skill_tiers`, `U test_bundled_skills`.
- [ ] T099 [US10] `tests/integration/test_storage_mapping_skill_172_live.rs`: one test per claim in the skill (the
      skill lists them; the test file names match). Prove: `L test_storage_mapping_skill_172`.

---

## Phase 12: User Story 11 — A public corpus with known answers (P2)

- [ ] T100 [US11] Lead: from the internal pattern catalogue's coverage list (kept outside the repo), write the list of
      corpus classes still missing after T012–T024 into `tests/fixtures/storage172/CORPUS.md` in generic words
      (pattern, not origin). Prove: file exists.
- [ ] T101 [P] [US11] One task per class in `CORPUS.md` (the lead splits this into T101a, T101b, …): an invented
      fixture plus seed with its known answer, as T012. Prove: rig case per class.
- [ ] T102 [P] [US11] Dimensions file `/crates/iris-agentic-dev-core/tests/fixtures/storage172/dimensions.toml`
      (lead supplies the dimension names and values from the catalogue; values only, no counts): map type, subscript
      depth, delimiter class per field, node layout, row-node state (value only / children only / value and
      children, i.e. D11), nested in another map yes/no, row ID parts, each code element (one dimension per element,
      present/absent), conditional, parent-child, access type per level, global form. Prove: a unit test in
      `tests/unit/test_storage_map_gen_172.rs` parses the file and asserts every dimension has at least two values.
- [ ] T103 [US11] Covering-array builder `tests/unit/storage172_gen/pairwise.rs` (shared test-support module, no
      new crate): `pub fn pairwise(dims: &[Dim], forbidden: &[Pair], seed: u64) -> Vec<Vec<usize>>`, greedy one
      row at a time, deterministic for a seed, never emits a forbidden pair. Test first in `test_storage_map_gen_172.rs`:
      every allowed value pair appears at least once (check by brute force over the output); no forbidden pair
      appears; the same seed gives the same rows; for 3 dimensions of 3 values the output has at most 12 rows.
      Prove: `U test_storage_map_gen_172`.
- [ ] T104 [US11] Row model and class writer `tests/unit/storage172_gen/class.rs`: `pub fn class_for(row: &[usize],
dims: &[Dim], n: usize) -> GenClass { name, cls_text, rows: Vec<Vec<String>>, nodes: Vec<(String, String)>,
question, answer }`. Names `IadSm172Gen.C<nnn>`, globals `^IadSm172Gen<nnn>`, invented field names. The node
      values are built by this module's own encoder (pieces joined with the delimiter, or `$LB` text), never by
      `storage_map` code, and `answer` is the row model itself, so the known answer does not come from the system
      under test. Test first: a two-field `^`-delimited row writes `"a^b"`; a `$LIST` row writes the `$LB` form; the
      answer equals `rows` ordered by ID. Prove: `U test_storage_map_gen_172`.
- [ ] T105 [US11] Compile oracle `tests/integration/test_storage_map_generated_172_live.rs`: for each row of
      `pairwise`, PUT and compile the class on iris-dev-iris. A class that compiles is written to
      `tests/fixtures/storage172/generated/` with its seed; a class that fails adds the pair(s) and the IRIS error
      number to `tests/fixtures/storage172/forbidden.toml` (with `IAD_REGEN_GENERATED=1` only; otherwise the test
      asserts the committed files equal a fresh generation). Repeat until no new forbidden pair appears. Every class
      is dropped afterwards. Add `IAD_REGEN_GENERATED` to `BEHAVIOR_ENV_VARS`. Prove: `L test_storage_map_generated_172`.
- [ ] T106 [US11] Every pair in `forbidden.toml` becomes a case in `tests/unit/test_storage_map_validate_172.rs`: the
      validator refuses it before any write, or the case says why only IRIS can tell (the compile check covers it).
      Every error number found has an `explain.rs` entry. Prove: `U test_storage_map_validate_172`,
      `U test_storage_map_explain_172`.
- [ ] T107 [US11] Random-tree writer test `tests/integration/test_storage_map_random_172_live.rs`: 200 random
      grammar-valid Storage trees from a fixed seed (inline xorshift, no new crate), each emitted, PUT, read back and
      compared with `normalize(sent)`; any difference fails with the seed and the tree. Prove:
      `L test_storage_map_random_172`.
- [ ] T108 [US11] `tests/integration/test_storage_map_corpus_172_live.rs`: for every fixture (hand-written and
      generated), round-trip byte-identical,
      compile, and the known answer. Prove: `L test_storage_map_corpus_172`.
- [ ] T109 [US11] Local-only coverage test in the same file: reads `IAD_STORAGE_CORPUS` (a directory of feature-vector
      JSON produced outside the repo); unset → the test prints that it was not run and passes only with
      `IAD_ALLOW_SKIP=1`, otherwise panics; set → reports combinations with no public fixture. Prove: run with the
      variable unset and `IAD_ALLOW_SKIP=1`; the lead runs it with the variable set.

---

## Phase 13: User Story 7 — Draft a map for an unmapped global (P3)

- [ ] T110 [P] [US7] `src/tools/storage_map/shape.rs`: `pub fn draft(report: &ShapeReport) -> SqlMap` with every
      inferred name and type marked `guess: true`. Test first `tests/unit/test_storage_map_sample_172.rs`: a report
      for depth 1, `^`, 3 pieces gives three fields piece 1..3 on the ID node. Prove: `U test_storage_map_sample_172`.
- [ ] T111 [US7] `mod.rs`: the `propose` action (sample, draft, no write). Test first in
      `test_storage_map_data_172_live.rs`: `propose ^IadSm172Member` draft matches the seed's shape; saving the draft
      through `replace` returns the known answer; no class or global changed by `propose` alone. Prove:
      `L test_storage_map_data_172`.

---

## Phase 14: Lift (release gate, constitution IX)

- [ ] T112 Ladder tasks `/tests/e2e/tasks/benchmark/skills/SKILL-26.yaml` … `SKILL-29.yaml` (format as SKILL-25):
      add a column to a map; explain an old global's layout; draft a table over an unmapped global; fix a map that
      reads `$LIST` on `^`-delimited data. Each `check` asserts the fixture's known answer; each has a `teardown`;
      add the IDs to `split.toml` (holdout). Prove: the ladder's own validator.
- [ ] T113 Lead: run the ladder with `--cap` on measured cost, arms tools vs tools+skill, and write
      `specs/172-sql-storage-maps/lift-results.md`. Release blocked until lift ≥ +0.20 or a documented exception.

---

## Phase 15: Polish

- [ ] T114 Remove every `NOT_IMPLEMENTED` from `storage_map/`. Prove: `grep -rn NOT_IMPLEMENTED src/tools/storage_map`
      is empty.
- [ ] T115 Every `#[ignore]` test in this feature appears in `quickstart.md`'s commands. Prove: lead check.
- [ ] T116 Full suite: `cargo test --features testing -- --test-threads=1 --include-ignored` (all targets) green.
- [ ] T117 Coverage: the canonical `cargo llvm-cov --summary-only -p iris-agentic-dev-core -- --include-ignored`
      command from the constitution; TOTAL line coverage ≥ 90%, and every file under `storage_map/` ≥ 90%.
- [ ] T118 Lead: `/CLAUDE.md` Recent Changes entry, release-notes draft through `/wordcraft` and `/no-ai-slop`, and
      clean up the throwaway `Iad172.*` classes and globals in USER on iris-dev-iris.

---

## Dependencies & execution order

- T001 blocks Phase 6 onward and any push; T002 is done. Phase 1 → Phase 2 → stories.
- US1 (Phase 3) → US2 (4) and US8 (5) can run side by side → US4 (6) → US3 (7) → US9 (8).
- US5 (9) needs US8 and T036; US6 (10) needs US3; US10 (11) needs US1–US3; US11 (12) needs T011; US7 (13) needs US5.
- Lift (14) needs every story; Polish (15) last.
- Inside a phase: the test task before its code task; tasks touching `src/tools/mod.rs`, `write_gate.rs` or
  `doc.rs` never run in parallel with each other.

## Parallel examples

- Phase 2: T012–T025 (fixtures, one teammate each; they share iris-dev-iris, so run their live proofs one at a time)
  alongside T027 and T033–T034 (pure).
- Phase 3: T037 while T046 and T049 start in other worktrees.
- Phase 5: T052 and T055 side by side; T056 and T057 after.

## Implementation strategy

1. MVP: Phases 0–3 (read and `resolve_storage`). Stop and check against US1's independent test.
2. Then US2 and US8, then US4 + US3 (the write path), then US9, US5, US6.
3. Contract, corpus, propose, lift, polish.

Optional git hooks (`before_tasks`, `after_tasks`) were not run: commits happen only when Tom asks.
