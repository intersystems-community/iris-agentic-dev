# Implementation Plan: SQL storage maps (172)

**Branch**: `172-sql-storage-maps` | **Date**: 2026-11-09 | **Spec**: [spec.md](spec.md)
**Input**: `specs/172-sql-storage-maps/spec.md`; decisions in `docs/adr/0004-sql-storage-maps-are-edited-through-a-model.md`

## Summary

One new dispatcher tool, `iris_storage_map`, reads a class's Storage block from its source into a lossless element
tree and a typed model. It draws the map, finds the classes that use a global, samples the global, and writes edits
back in the exact form IRIS stores. Every write goes through a scratch-copy compile, a read-back check and an
automatic restore. Map globals and map code go through iad's existing global and code checks. `iris_doc` routes a
put that changes a SQL storage map through the same safeguards, and `resolve_storage` uses the same reader.

The work is built as small pure modules (parser, writer, model, paths, validation, diagram) with unit tests and no
IRIS, then a thin handler that calls existing helpers for GET, PUT, compile, SQL and sampling, with live tests on
iris-dev-iris.

## Technical Context

**Language/Version**: Rust 2021 (workspace toolchain), crate `iris-agentic-dev-core`
**Primary Dependencies**: existing only: `rmcp`, `schemars`, `serde`/`serde_json`, `reqwest`, `tokio`, `regex`, `uuid`.
No new crate: the parser, writer and FNV-1a digest are hand-written (research R2, R10).
**Storage**: none in iad; IRIS class sources through Atelier REST
**Testing**: `cargo test --features testing` (`unit`, `integration`, `binary` targets); live tests on iris-dev-iris
(2026.2, web port 52780, USER); Mermaid render check with `mmdc` (installed locally at `/opt/homebrew/bin/mmdc`)
**Target Platform**: the single `iris-agentic-dev` binary on macOS, Linux, Windows
**Project Type**: MCP server tool
**Performance Goals**: `read`/`diagram` of a class with 10 maps under 1 s on a local container; sampling bounded by the
caps (200 nodes, 5 s by default)
**Constraints**: byte-identical round-trip; no text splicing inside a map; no new dependency; HTTP-first (no Docker)
**Scale/Scope**: about 2,500 lines of Rust plus tests; fixtures of about 12 classes

## Constitution Check

| Principle                             | How 172 meets it                                                                                                                                  |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-install binary                | Atelier REST and `execute_via_generator` only; no Docker path                                                                                     |
| II. ObjectScript sanity gate          | Every IRIS call verified live (research R4, R5, R11, R12, R13); unverified items are first tasks in their phase                                   |
| III. HTTP-first                       | Same as I                                                                                                                                         |
| IV. Test-first, fixture-driven        | Pure modules take fixture text from `tests/fixtures/storage172/`, valid and malformed; tests before code                                          |
| V. Output shape parity                | `resolve_storage` adds `sql_maps` to its answer; no key removed                                                                                   |
| VI. Environment guard                 | Per-action classification in `CLASSIFICATION`; edit and replace are writes                                                                        |
| VII. Dependency minimalism            | No new crate (R2, R10)                                                                                                                            |
| VIII. 90% coverage                    | Polish phase runs the canonical `cargo llvm-cov` command with `--include-ignored`                                                                 |
| IX. Tool lift                         | Ladder tasks SKILL-26 to SKILL-29 and `lift-results.md`, a release gate before Polish                                                             |
| X. ObjectScript coverage              | Not applicable: 172 ships no ObjectScript classes, only generated snippets and test fixtures                                                      |
| XI. No vacuous tests                  | Live tests use `Where::live()` (`assert_running`); loops assert non-empty first; the `IAD_STORAGE_CORPUS` test is the one opt-in skip and says so |
| XII. Hermetic tests                   | `IAD_STORAGE_CORPUS` and `IAD_REGEN_GOLDEN` are added to `BEHAVIOR_ENV_VARS`                                                                      |
| XIII. Single-source failure detection | PUT errors through the `result.status` helper (from the queue-jumper fix); generator output through `generator_error_message`                     |
| Error code registry                   | New codes in `data-model.md` and `docs/error-codes.md`                                                                                            |
| User-facing strings are API           | Descriptions name only real tools and params; `docs/tools.md` tables are checked                                                                  |

No violations. Two items need care rather than an exception:

- **One action reader (FR-030a).** `storage_map::call_shape(params) -> CallShape { action, seed_counter,
build_indexes, include_values }` is the only reader of the call's action and opt-ins. `classify`, `is_scratch_call`
  (which today reads only `mode`), `needs_service_account` and the handler all call it. An unreadable action is
  judged as `edit` with both opt-ins (the strictest).
- **Body reads (FR-021).** The shared `names_argument` predicate does not change. `iris_storage_map` is a
  handler-enforced body read: the handler calls `check_body_read_acknowledged_as` only when `include_values` is
  `true`, and refuses `include_values: true` itself under block and redact.

## Prerequisites (queue-jumpers, Q11)

1. `iris_doc` put reads `result.status`: on master as `87e2cab7`; 172 is based on it and uses
   `check_atelier_put_errors` for every PUT.
2. A separate security check, tracked privately, settled before phase 1 starts (the review found 172 makes its path
   easier to reach). Its outcome may add a guard test that FR-026's tests extend.

## Project Structure

### Documentation (this feature)

```text
specs/172-sql-storage-maps/
├── spec.md
├── plan.md              # this file
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   └── iris_storage_map.md
├── checklists/requirements.md
├── lift-results.md      # written in the lift phase
└── tasks.md             # /speckit.tasks
```

### Source code

```text
crates/iris-agentic-dev-core/src/tools/storage_map/
├── mod.rs        # handler: action routing, calls the modules below; no logic of its own
├── tree.rs       # Node, parse_body, split_class, emit_body, order_key, normalize (pure; port of the prototype)
├── model.rs      # typed StorageModel from a tree, model → JSON, derived facts, map/storage versions (pure)
├── path.rs       # path parsing and set/add/remove/move on the tree (pure)
├── validate.rs   # FR-014 rules (pure)
├── code.rs       # list of code-bearing elements; runs check_objectscript_code_edit on each; dynamic-global test (pure)
├── globals.rs    # global normalisation (^X, X, ^|"NS"|X, ^["NS"]X); code-storage + blocklist check (pure)
├── diagram.rs    # Mermaid flowchart text + JSON nodes/edges (pure)
├── sample.rs     # ObjectScript for capped sampling; parse its output into a shape report (pure + one runner fn)
├── uses.rs       # dictionary queries for `uses` (thin, IRIS)
├── write.rs      # lifecycle: scratch copy, real PUT, read-back, compile, restore, opt-ins (IRIS)
└── explain.rs    # plain-words text for compile error numbers 5508–5596, 5002, 15511, 16023 (pure)

crates/iris-agentic-dev-core/src/tools/params/batch8.rs   # IrisStorageMapParams
crates/iris-agentic-dev-core/src/iris/connection.rs       # + compile_scratch_class (public helper on ScratchGuard)

crates/iris-agentic-dev-core/tests/
├── unit/test_storage_map_tree_172.rs        # parser/writer, fixtures, malformed input
├── unit/test_storage_map_model_172.rs       # model, derived facts, versions
├── unit/test_storage_map_path_172.rs        # path ops
├── unit/test_storage_map_validate_172.rs
├── unit/test_storage_map_gates_172.rs       # both spellings, extended refs, code elements
├── unit/test_storage_map_diagram_172.rs
├── unit/test_storage_map_sample_172.rs
├── unit/test_storage_map_golden_172.rs      # JSON shape pinned (IAD_REGEN_GOLDEN=1 rewrites)
├── integration/test_storage_map_read_172_live.rs
├── integration/test_storage_map_sys_roundtrip_172_live.rs
├── integration/test_storage_map_edit_172_live.rs
├── integration/test_storage_map_data_172_live.rs
├── integration/test_storage_map_doc_172_live.rs
├── integration/test_storage_map_corpus_172_live.rs
└── binary/test_storage_map_schema_172.rs

tests/fixtures/storage172/
├── classes/*.cls          # IadSm172.* fixture classes, canonical form
├── seed/*.json            # per class: global nodes to set, known question (SQL), known answer row
├── malformed/*.txt        # Storage bodies the parser must refuse, with the expected error
└── golden/*.json          # pinned model JSON per fixture

skills/skills/iris-storage-mapping/SKILL.md
docs/storage-maps.md
tests/e2e/tasks/benchmark/skills/SKILL-26.yaml … SKILL-29.yaml
```

**Structure decision**: one module directory under `tools/`, split so each pure file can be built and tested on its
own by one implementer, and the IRIS-facing code stays in `mod.rs`, `uses.rs`, `sample.rs` (runner only) and
`write.rs`.

## Design

### Read path

1. GET `/doc/<Class>.cls` (`doc::fetch_doc_lines`). No class → `STORAGE_MAP_NOT_FOUND`.
2. `tree::split_class` finds the Storage block by parsing elements (NextCode can hold `}` lines). No Storage block
   → model with `maps: []` and a note.
3. `tree::parse_body` → `Vec<Node>`; a parse failure → `STORAGE_MAP_PARSE_FAILED` with the offset and 40 characters
   of context.
4. `model::from_tree` → `StorageModel`; derived facts per research R5; `master`/`used` flags; versions (FNV-1a 64 of
   the canonical text, 16 hex digits).
5. For default storage, `<Data name=…><Value name="n"><Value>Prop</Value>` becomes `default_storage.nodes[].slots`.

### Write path (edit, replace, and `iris_doc` with a changed map)

```text
GET text ─► tree ─► apply ops on a copy ─► validate (FR-014) ─► gates (FR-026..028) ─► data checks (FR-023)
  ─► new class text = prefix + emit_body(normalize(tree)) + suffix  (only the Storage body is regenerated)
  ─► compile_scratch_class(IrisDevTmp.Sm<uuid12>, rewritten copy)   fail → STORAGE_MAP_COMPILE_FAILED
  ─► PUT real (check_atelier_put_errors)                            fail → UPLOAD_FAILED, nothing else runs
  ─► GET real, parse, compare with normalize(sent)                  diff → STORAGE_MAP_DROPPED / _WRITER_MISMATCH
  ─► compile real                                                   fail → PUT old text, compile → _RESTORED / _RESTORE_FAILED
  ─► seed_counter / build_indexes (through the write gate)
  ─► answer: model, diff (element paths changed), warnings
```

- **Change detection (FR-010a).** iad records the document timestamp from its GET and checks it again just before
  the real PUT and before a restore; a change refuses the write with `STORAGE_MAP_VERSION_CONFLICT`. First task of the
  write phase: check whether Atelier PUT without `ignoreConflict=1` honours a timestamp, and use that if it does.
- **Re-gate (FR-012a).** The read-back's parsed tree goes through the same gate function as the edited tree before the
  real compile; a refusal restores the previous text.
- **Scratch copy (FR-015a).** Built by `write::scratch_source(model, class_header) -> String`: `Class
IrisDevTmp.Sm<uuid12> Extends <same superclasses>`, the properties and indices the map's fields name, `SqlTableName`
  set to the scratch name, and the emitted Storage block. No method, query, XData, parameter or projection from the
  caller's text. It goes through `check_compile_time_code_mode` before the PUT. A live checksum test (first task of
  the write phase) proves compile plus delete leaves every global unchanged.
- **`iris_doc` hand-off (FR-031).** In `do_write` (`doc.rs`, the single sink: put at :579 and :694, and the
  confirmation resume at :310). When the new content holds an SQLMap in any Storage block, of any storage type,
  `do_write` calls `storage_map::write::from_doc(...)`, which runs the safeguards and returns the same answer shape
  as today plus the warning `SQL storage map changed: iris_storage_map edits maps with a version check and keeps the
class readable`. Class XML with an SQLMap is refused (FR-012a). Default storage keeps today's path.
- **Audit (FR-030c).** Each map write adds `{class, map, version_before, version_after, changed_paths, code_changed}`
  to the existing call record; `irisAudit` emission covers the action. No raw code in the record.

### Classification

| Call                               | Write class | Scratch | Categories       | Identity                              |
| ---------------------------------- | ----------- | ------- | ---------------- | ------------------------------------- |
| `read`, `diagram`, `uses`          | ReadOnly    | no      | Docs             | connection user                       |
| `check`                            | ReadOnly    | yes     | Compile          | connection user                       |
| `compare`, `propose`, `preview`    | Write       | yes     | Execute          | service account                       |
| `edit`, `replace`                  | Write       | yes     | Compile          | connection user                       |
| `edit`/`replace` changing map code | Write       | yes     | Compile, Execute | connection user                       |
| `edit`/`replace` + `seed_counter`  | Write       | yes     | Compile, Execute | service account for the counter write |
| `edit`/`replace` + `build_indexes` | Destructive | yes     | Compile, Execute | service account for the build         |

`classify` reads `call_shape`, so the opt-ins raise the class of an edit. `effective_category` gains the arms in the
table; a call with two categories needs both allowed. `iris_storage_map` joins `SERVICE_ACCOUNT_TOOLS`, and
`needs_service_account` uses `call_shape` so that only the rows marked service account switch identity. The opt-ins
run after the real compile, in the same call, and are judged by this classification, never by a synthetic call to
another tool.

### Gates in the handler

Storage globals and map code are not tool params, so `dispatch_gate_for` does not see them. The handler runs, in
this order, on the edited tree (and `iris_doc`'s hand-off runs the same function):

1. `globals::check_map_globals`: every global a map or storage keyword names (map `Global`, `IdLocation`,
   `DataLocation`, `IndexLocation`, `StreamLocation`, counters in `SqlIdExpression`), normalised; `CODE_STORAGE_GLOBALS`
   (made `pub(crate)`), `check_system_blocklist` with the server's `global_blocklist`, and `check_phi_name_gate`.
   Runs on every action that reaches IRIS except `read`, `diagram` and `uses` (FR-026). Extended references are
   refused for writes and sampling.
2. `code::check_map_code`: for each code-bearing element (research R6), `check_objectscript_code_edit(value,
server)` → the same refusal body as `iris_execute`. `code::code_changed(before, after) -> bool` compares the
   code-bearing elements of the two trees; `call_shape` sets it, so the classifier adds Execute (FR-027) and the
   audit record carries it. The same applies to the `iris_doc` hand-off.
3. `code::dynamic_global`: `AccessType Global`, `@` or `$Name`-built references in `DataAccess`/`RowReference`/
   `Global` → `STORAGE_MAP_DYNAMIC_GLOBAL_BLOCKED` (operator limit), naming the expression, unless the effective
   policy for the server has `allow_dynamic_map_globals = true`. That field is a new loosening setting: added to
   `PolicyLayer`, set aside from an untrusted file and from an operator layer whose address binding withholds
   loosening (`strip_operator_loosening`), reported by `check_config`, and listed in `docs/connecting.md`.
4. `check_compile_time_code_mode(new_text, doc)` on the whole new class text, as `do_write` does.

New refusal code `STORAGE_MAP_DYNAMIC_GLOBAL_BLOCKED` is added to `REFUSAL_CODES` with a `describe` arm (operator
limit, lift names the setting).

### Sampling

`sample::build_code(global: &CanonicalGlobal, max_nodes: u32, seconds: u32, include_values: bool)` takes only a
validated canonical global name and integers; any other caller value travels in `X-IAD-Args` (FR-030b). It generates one `$Query` walk that writes one line per
node: `depth|format|delimiter|pieces|value-or-empty`, then `DONE|count|capped`. It is run with
`execute_via_generator` and parsed by `sample::parse`. Format per research R12. `compare` matches the shape report
against the map's subscripts and data fields; `propose` turns it into a draft tree with `"guess": true` markers in
the model JSON.

### Diagram

`flowchart LR`: `G["^Global"]` → `S1["sub 1: {ID} (Sub)"]` → … → one node per data node listing `Field: piece n`, and
`R["row ID: f1, f2"]`. Index maps go in `subgraph` blocks. Labels keep letters, digits, space and `_.,:^(){}%$-`; every other character becomes a Mermaid entity code, and no
`click`, link or `%%{init}` text is ever emitted from input (FR-005). JSON: `{nodes: [{id, kind,
label}], edges: [{from, to}]}`.

## Phases (for tasks.md; tasks.md splits these into Phases 0–15)

Each phase ends with its tests green; no phase starts before the previous one's gate.

1. **Setup**: fixtures dir, empty modules, params struct, registration in every list from the integration map, tool
   answering `NOT_IMPLEMENTED` per action (removed before merge), guards green.
2. **Tree** (US3 base): parser, writer, normalize, parser caps (FR-014a); fixture and `%SYS` round-trip (live, read-only).
3. **Model and read** (US1): model, derived facts, versions, `read`, `resolve_storage` reuse.
4. **Diagram and uses** (US2).
5. **Validation and gates** (US8, FR-014).
6. **Write lifecycle** (US3, US4): paths, `compile_scratch_class`, edit, replace, read-back, restore, strict.
7. **Data** (US5, US6): sampling, compare, preview, data policy, ID reuse, index build.
8. **Propose** (US7).
9. **`iris_doc`** (US9).
10. **Contract** (US10, US11): golden files, corpus, `IAD_STORAGE_CORPUS` test, `docs/storage-maps.md`, skill. The corpus classes are written from the coverage list in the internal pattern
    catalogue (`.iad-local/storage-maps/pattern-catalogue.md`, never copied into the repo): invented domain, our own
    code, one class per listed pattern or combination. A pairwise generator (FR-037a) fills the combinations the hand-written
    fixtures miss, with live IRIS as the judge of which pairs are legal; a random-tree test checks the writer (FR-009a).
11. **Lift** (release gate): ladder tasks, `lift-results.md`.
12. **Polish**: coverage ≥ 90%, docs/tools.md, error-codes.md, CLAUDE.md Recent Changes, release notes draft.

## Delegation

Implementation teammates run on sonnet (Tom, 2026-11-10; haiku stopped partway). Each task names its files, the test to write first, the
function signatures, and the command that proves it. The lead reviews every diff and runs the full suite before
anything is committed. Pure-module tasks (phases 2 to 5, diagram, explain) can run in parallel worktrees; IRIS
tasks run one at a time against iris-dev-iris (shared container, `--test-threads=1`). Locally, run only the unit
target and the storage-map live tests: endpoint security killed the session three times on 2026-11-10 during the
full binary target and the live tool sweep, which CI runs instead.

## Complexity Tracking

No constitution violations to justify.
