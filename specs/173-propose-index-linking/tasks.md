# Tasks: `propose` reads index globals, collations and datatypes (173)

**Input**: `specs/173-propose-index-linking/` — spec.md, plan.md, research.md, data-model.md,
contracts/propose-and-check.md, quickstart.md. Parent: `specs/172-sql-storage-maps/` (same branch, ships with it).

**Who does what**: implementation teammates (sonnet) take one task at a time in their own worktree. The lead (Opus)
reviews each diff, runs the test targets and commits. A teammate never commits, pushes or merges. Pushing needs Tom's
words.

## Rules for every task

- Write the test named in the task first, run it, and see it fail for the right reason. Then write the code, and run the
  same command until it passes. Paste both outputs (red, then green) in your report.
- Read `CLAUDE.md` (worktree root) before your first task: always `--features testing`; a new test file needs a `mod`
  line in its target's `main.rs` (`unit`, `integration`, `binary`); change the environment only through
  `testing::EnvGuard`.
- Live tests use `live_rig_171::{rig, Where}`: `Where::live()` panics when iris-dev-iris is not running. Never return
  early from a test to skip it. Every loop over results asserts it is non-empty first.
- Never mock IRIS. Every fixture is invented for this repository: classes under `IadSm173`, globals starting
  `^IadSm173`, our own values. Nothing is copied from an InterSystems class, product source, path, routine or macro, and
  none is named in any tracked file, comment or test (FR-031).
- A test asserts the answer, not the shape: the known field, transform, length, marker, ID position and verdict of the
  fixture, never "non-empty" or a count alone. Never take the known answer from the system's own output.
- Paths are relative to `crates/iris-agentic-dev-core/` unless they start with `/` (repository root).
- On the laptop: `nice -n 15 cargo … -j 2`, one test file at a time. No live tool sweep and no full binary target
  (CI runs them).

## Commands (run from the worktree root)

- `U <filter>` = `RUST_TEST_THREADS=12 nice -n 15 cargo test --features testing -j 2 --test unit <filter>`
- `L <filter>` = `nice -n 15 cargo test --features testing -j 2 --test integration <filter> -- --test-threads=1 --include-ignored`
- `B <filter>` = `cargo build && IAD_BINARY=./target/debug/iris-agentic-dev cargo test --features testing --test binary <filter> -- --include-ignored`
- `LINT` = `cargo fmt --all && cargo clippy --features testing --all-targets -- -D warnings`

Every task ends with `LINT` clean.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story in spec.md (US1, US1a, US2 … US9)

## Fixture catalogue (built in Phase 0 and 1, used by every later phase)

Filer fixtures: a class with maps is loaded, rows are INSERTed through SQL, the class is deleted, and the globals
IRIS filed stay. `propose` is then pointed at the globals alone. Hand fixtures: the rig `Set`s the globals directly.
Master is `^IadSm173Person(id) = Name^City^Age^Born^Status` unless stated; each seed file in
`tests/fixtures/storage173/seed/` carries the known answer. At least 40 rows each (the 20-node floor), except F27 (5 rows), F34 (1,000 or more) and F38 (1,200).

| ID  | Fixture                                                                                                    | Known answer                                                                     |
| --- | ---------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| F01 | index on City, plain `{City}`                                                                              | City, exact                                                                      |
| F02 | index `$$SQLUPPER({Name})`                                                                                 | Name, sqlupper                                                                   |
| F03 | `$$SQLSTRING({Name})`                                                                                      | Name, sqlstring (names with trailing blanks, so `" "_{F}` loses)                 |
| F04 | `$$STRING({Name})`                                                                                         | Name, string                                                                     |
| F05 | `$$ALPHAUP({Name})`                                                                                        | Name, alphaup                                                                    |
| F06 | `$$UPPER({Name})`                                                                                          | Name, upper                                                                      |
| F07 | `$$MVR({Name})`                                                                                            | Name, mvr                                                                        |
| F08 | `$$TRUNCATE({Name},3)`                                                                                     | Name, truncate, length 3                                                         |
| F09 | `$$SQLUPPER({Name},4)`                                                                                     | Name, sqlupper, length 4 (the prefix counts)                                     |
| F10 | `$p($$SQLUPPER({Name}),",",1)`                                                                             | Name, sqlupper, piece 1                                                          |
| F11 | `{City}`, `{Age}`, `{ID}`                                                                                  | ID position 3; City exact; Age `ambiguous` with `alphaup`, `upper` (digits only) |
| F12 | composite key `^IadSm173Key(a,b)`, index `(Name, a, b)`                                                    | ID positions 2 and 3, k = 2                                                      |
| F13 | `$$SQLUPPER({Name})` with the ID in the node value                                                         | `id.form = value`                                                                |
| F14 | covering: node value is a `$LIST` of City and Age                                                          | `data_in_value` City element 1, Age element 2                                    |
| F15 | empty-string nulls under `{City}` (map-compiled `$c(0)`)                                                   | marker kind `c0`                                                                 |
| F16 | numeric nulls under `{Age}`                                                                                | marker kind `minus1e14`; Age `ambiguous` (digits only)                           |
| F17 | class-compiled index, SQLUPPER default, empty values                                                       | marker kind `space`                                                              |
| F18 | hand-written index with one constant marker of length 5                                                    | `discovered`, length 5, no literal without `include_values`                      |
| F19 | map-compiled bitmap `Type bitmap`, `{Status}` only                                                         | `bitmaps`: chunk is the last subscript, all IDs found                            |
| F20 | class-compiled bitmap over Status (default storage master)                                                 | chunk is the last subscript, index name first                                    |
| F21 | master `^IadSm173Bits(0, chunk) = $BIT`                                                                    | `chunk_map_draft` returns the IDs the bits name                                  |
| F22 | sparse: index skips NULL City                                                                              | rule NULL, draft `{City}'=""`                                                    |
| F23 | sparse: index skips empty-string City                                                                      | rule empty string                                                                |
| F24 | sparse: index skips Status other than `A`                                                                  | rule `{Status}="A"`                                                              |
| F25 | sparse: index skips every 7th row                                                                          | no rule fit, share reported                                                      |
| F26 | two identical columns, index over one                                                                      | `ambiguous`, both fields in `alternatives`                                       |
| F27 | 5 rows                                                                                                     | `too_few_nodes`                                                                  |
| F28 | constant column indexed                                                                                    | position `constant`                                                              |
| F29 | Age equals ID on every row (34 against 34 …), index over Age                                               | never `inferred` for ID from Age                                                 |
| F30 | dense ID run, index over a column unrelated to the rows                                                    | no wrong `inferred`                                                              |
| F31 | `^IadSm173PersonAlpha`, unrelated, shares the master's prefix                                              | `not_linked` with best agreement                                                 |
| F32 | index inside the master global `^IadSm173Person("idx", …)`                                                 | `found_by = sub_node`, linked with the prefix cut off                            |
| F33 | index over a date format of Born (computed subscript)                                                      | position `not_explained`, no code drafted                                        |
| F34 | far-end separation: names that differ under `$$UPPER` and `$$ALPHAUP` only past the first 600 keys         | spread `inferred`, a prefix walk `ambiguous`                                     |
| F35 | class `IadSm173.Person` with a master map and an unmapped index global                                     | class mode drafts the index in the class's own field names                       |
| F36 | same class with the index already mapped                                                                   | `already_mapped` naming the map                                                  |
| F37 | typing columns: integer, decimal, date-like, text                                                          | `%Integer`, `%Numeric` scale 2, date guess with fit count, `%String` MAXLEN      |
| F38 | 1,200 rows, two index globals over Name and City                                                           | both `inferred`, whole call under 5 s (SC-005)                                   |
| F39 | index with seven field subscripts (City, Age, Status, Born, Name, SQLUPPER and ALPHAUP of Name) and `{ID}` | first 6 examined, the 7th named in `not_inferred`                                |
| F40 | index over Age holding numbers `34` and a few strings `"34"`, stored as written                            | Age exact, compared as stored, no `not_explained`                                |
| F41 | non-ASCII names under `$$SQLUPPER`                                                                         | Name, sqlupper (the server's function decides case)                              |
| F42 | two index globals over City                                                                                | two drafts with different map names, both `inferred`                             |
| F43 | candidate global that is also another class's master                                                       | linked as an index candidate, not merged with that class                         |
| F44 | 100 rows, 3 master rows deleted after the load                                                             | inferred, no `orphan_nodes`                                                      |
| F45 | 100 rows, 8 master rows deleted                                                                            | inferred, `orphan_nodes` count 8                                                 |
| F46 | 100 rows, 11 master rows deleted                                                                           | `not_linked`, agreement 0.89                                                     |

---

## Phase 0: Measure first (blocking)

**Purpose**: the facts the rest rests on, as live tests (spec Context facts 1 to 12, research R1 to R7, three open
items). A failure stops the piece it guards and goes to Tom; the spec is amended only with his decision.

- [x] T001 Create `tests/integration/storage173_rig.rs` (declare it in `tests/integration/main.rs`): `Fixture` read
      from `tests/fixtures/storage173/seed/<name>.json` (class file, rows, hand globals, known answer), plus
      `load_fixture_173(r, f)` (load class, INSERT rows, delete class, keep globals), `drop_fixture_173` (kill every
      `^IadSm173*` the fixture names and delete the class), `ask_propose(r, args)` and `ask_check(r, args)` returning
      the tool's JSON. Model it on `storage172_rig.rs`. Test first: a fixture with one row loads, the global exists,
      and `drop_fixture_173` leaves `$Order(^IadSm173...)` empty. Prove: `L storage173_rig`.
- [x] T002 Create `tests/integration/test_storage_map_measured_173_live.rs` (+ `mod` line). Test
      `fact_1_closed_set_of_collation_functions`: compile one class per function in `$$SQLUPPER`, `$$SQLSTRING`,
      `$$STRING`, `$$ALPHAUP`, `$$UPPER`, `$$MVR`, `$$TRUNCATE({F},3)` (each compiles) and `$$EXACT`, `$$PLUS`,
      `$$MINUS`, `$$SPACE` (each fails to compile). Prove: `L fact_1`.
- [x] T003 Same file: `fact_2_stored_forms` asserts the stored subscript for `"  Smith, Anna  "`-style values under
      each function (`" SMITH, ANNA"` with the leading blanks kept, `" Smith, Anna"`, upper-case with spaces removed,
      letters and digits only, upper-case keeping blanks, `$$TRUNCATE({F},3)` cutting to 3); `fact_3_length_variant`
      (`$$SQLUPPER({F},4)` stores `" SMI"`); `fact_4_name_piece` (`$p($$SQLUPPER({F}),",",1)` compiles and stores
      `" SMITH"`). Prove: `L fact_2 fact_3 fact_4`.
- [x] T004 Same file: `fact_5_null_markers`: a map-compiled index stores `$c(0)` for an empty string, `-1E14` for
      an empty number and `" "_$c(0)` under `$$SQLUPPER`; a class-compiled SQLUPPER index stores `" "_$c(0)` for an empty string and a lone `" "` for NULL; a subscript
      with its own `NullMarker` stores that marker. Prove: `L fact_5`.
- [x] T005 Same file: `fact_6_planner_words`: `EXPLAIN` of `WHERE Name = ?` over `{Name}` contains "no index
      subscript collation"; over `$$SQLUPPER({Name})` contains "Read index map <map>, using the given"; over `$$SQLUPPER({Name},4)` contains
      "%SQLUPPER(4)". Pin the three strings (they are `usability.rs`'s matchers). Prove: `L fact_6`.
- [x] T006 Same file: `fact_7_bitmap_decode` (ID = (chunk − 1) × 64000 + position − 1, using `$BitFind`, with a
      chunk-2 bit to prove the multiplier) and `fact_8_chunk_data_map` (`IadSm172.Bits`-shaped class returns the IDs
      its bits name). Prove: `L fact_7 fact_8`.
- [x] T007 Same file: `fact_9_function_class` (for each of the 8 methods of `%SYSTEM.SQL.Functions` that matches a
      map function, the method's result equals the subscript a map-compiled index filed for the same value, including
      the length variants and an empty string), `fact_10_bitmap_map_layout` (research R1: one subscript files
      `(value, chunk)` holding the bits; `{ID}` and `{ID}\64000+1` append a subscript before the chunk),
      `fact_11_id_in_value` (R2: the map with `<Data name="Id"/>` and RowIdSpec `{Id}` returns row 2 for
      `Name='bob'` and the plan says "Read index map"), `fact_12_conditional_filing` (R3: `Status="A"` skips B rows;
      `{City}'=""` skips NULL and keeps the empty string). Prove: `L fact_9 fact_10 fact_11 fact_12`.
- [x] T008 Same file: `r4_collate_forms` (`$$COLLATE({Name},"SQLUPPER")` and `$$COLLATE({Name})` store the value
      unchanged; `$$COLLATE({Name},7)` fails to compile; `" "_{Name}` files the same string as `$$SQLSTRING` on
      clean values), `r6_multipart_key` (composite IdKey `(A,B)`, master with RowIdSpec `{L1}` `{L2}`, index map
      `$$SQLUPPER({Name})`, `{A}`, `{B}`; query returns both rows). Prove: `L r4 r6`.
- [x] T009 Same file, the three open items, each a test that holds both outcomes and records which it saw in its
      failure message: `open_listing_as_the_connection_user` (the listing query as the rig user; the no-admin-role case stays open and is
      held by T040: either it lists the fixture's globals, or it fails and the test asserts the failure
      text is non-empty so FR-004's message can quote it), `open_c0_condition` (`{City}'=$c(0)` compiles in a
      conditional map and skips exactly the empty-string rows), `open_spread_beats_prefix` (fixture F34: build it, walk
      count the keys, then walk the first 200 in order and 200 by stride, score `$$UPPER` against `$$ALPHAUP`; the test
      asserts the prefix walk cannot separate them and the stride can). Prove: `L open_`.
- [X] T010 Lead only: read T002 to T009. Any failure stops the piece it guards (a map form fails → its draft goes to
      `not_inferred`; the spread does not beat the prefix → FR-004a is amended) and goes to Tom with the output before
      Phase 3. Record each outcome in `specs/173-propose-index-linking/research.md` (a line under the item).

**Checkpoint**: every fact has a test that passes, or a decision from Tom.

---

## Phase 1: Setup

**Purpose**: the new params and modules exist, nothing changes behaviour, every guard is green.

- [x] T011 Add `pub mod link; pub mod link_code; pub mod candidates; pub mod master; pub mod typing; pub mod bitmap;
pub mod usability;` to `src/tools/storage_map/mod.rs` with an empty file for each. Prove: `cargo build --features testing`.
- [x] T012 Test first: `tests/unit/test_storage_map_params_173.rs` (+ `mod`): parse JSON strings with `index_globals`,
      with `class` on `propose`, with `field_names`, and with a misspelt key (must fail through
      `deny_unknown_fields`). Then add `index_globals: Option<Vec<String>>` and `field_names: Option<Vec<String>>` to
      `IrisStorageMapParams` in `src/tools/params/batch8.rs` (`class` exists). Prove: `U test_storage_map_params_173`.
- [x] T013 Update the tool description of `iris_storage_map` in `src/tools/mod.rs` and its `docs/tools.md` row so
      `propose` names `index_globals`, `class`, `field_names`; run the existing guards (`tool_catalogue` text, params
      docs). Test first: extend `tests/unit/test_storage_maps_doc_172.rs`'s param list check with the three names.
      Prove: `U test_storage_maps_doc_172`.
- [x] T014 Param-use validation: `index_globals` or `field_names` on an action other than `propose` is
      `INVALID_PARAMS` naming the param and the action; `class` with `global`, or neither, on `propose` is
      `INVALID_PARAMS`. Test first in `tests/unit/test_storage_map_params_173.rs` against `storage_map::call_shape`'s
      validator. Prove: `U test_storage_map_params_173`.
- [x] T015 [P] Binary schema test `tests/binary/test_storage_map_schema_173.rs` (+ `mod`): spawn the binary, `tools/list`,
      assert the `iris_storage_map` schema has `index_globals` (array of string), `field_names` (array of string) and
      still `additionalProperties: false`. Prove: `B storage_map_schema_173`.
- [x] T016 Create `tests/fixtures/storage173/{classes,seed,captured,golden}/` and write `F01` to `F10` (class file and
      seed JSON with the known answer from the catalogue; 40 rows each, names chosen so each transform differs from
      its neighbours, including trailing blanks for F03 and a punctuation-bearing name for F04/F05). Prove:
      `L storage173_rig` loads and drops each.

---

## Phase 2: Foundational — pure modules (no IRIS) [all parallel]

**Purpose**: everything that can be built and tested without IRIS, each in its own file by its own teammate.
Signatures come from `data-model.md`.

- [x] T017 [P] Test first, `tests/unit/test_storage_map_link_173.rs` (+ `mod`): `spelling(transform, field, length)`
      returns exactly the strings of data-model.md's table (`$$SQLUPPER({Name},4)`, `{City}`,
      `$p($$ALPHAUP({Name}),",",2)` …) and refuses a transform outside the closed set; `verdict(scores)` returns
      `inferred` at 20 nodes and 0.95, `too_few_nodes` at 19 nodes, `ambiguous` for two pairs 0.005 apart, `not_linked`
      for an ID agreement of 0.89, `not_explained` for a best of 0.94, `constant` for one distinct value; `map_name`
      makes `X` and `X2` for two candidates ending in `.x`. Then implement `src/tools/storage_map/link.rs`: `Transform`,
      `Verdict`, `PositionScore`, `spelling`, `verdict`, `map_name`. Prove: `U test_storage_map_link_173`.
- [x] T018 [P] Same file, parser: `parse_link_output(out) -> Result<LinkReport, String>` on three captured outputs in
      `tests/fixtures/storage173/captured/` (write them by hand from the output format in `link_code.rs`'s doc comment
      for now; T032 replaces them with live captures): a normal run, a run with a trailing `done` line missing (an
      error, not a short report) and an IRIS error text (an error). Implement `parse_link_output` and `LinkReport`.
      Prove: `U test_storage_map_link_173`.
- [x] T019 [P] Same file, drafts: `draft_index_map(link, master) -> (Value, Vec<String>)` returns the model JSON for F02's
      expected link (subscript 1 `$$SQLUPPER({Name})`, subscript 2 `{ID}`, `Type index`), for a composite key (subscripts
      end `{A}`, `{B}`), for the value form (`Data name="Id"`, RowIdSpec `{Id}`), and for a covering link (Data
      entries per covered field); each draft passes `model::validate` and lists its guessed paths. Implement it.
      Prove: `U test_storage_map_link_173`.
- [x] T020 [P] Test first, `tests/unit/test_storage_map_master_173.rs` (+ `mod`): `MasterSpec::from_draft(&ShapeReport,
&[String] field_names)` gives `Field1…n`, or the caller's names, and refuses a short list, a duplicate and a
      name that fails `master::is_valid_property_name` (new, in `master.rs`: a letter or `%` first, then letters and
      digits, at most 128 characters; no such validator exists yet, and `objectscript::validate_class_name` is for class
      names); `MasterSpec::from_class(&StorageModel)` reads the master map's
      subscripts, RowIdSpec parts and Data fields from `tests/fixtures/storage172/classes/Copy.cls` (k = 3) and
      `Member.cls` (k = 1), refuses default storage, a columnar map and a class with no `data` master map with the
      reasons `read` gives. Implement `src/tools/storage_map/master.rs`. Prove: `U test_storage_map_master_173`.
- [x] T021 [P] Test first, `tests/unit/test_storage_map_candidates_173.rs` (+ `mod`): `from_sub_nodes(&ShapeReport)`
      returns each distinct literal first subscript that holds a sub-tree, `dedupe` removes a global named twice
      (parameter wins over `name_prefix`), `cap(candidates, 8)` keeps the first 8 and names the rest,
      `validate_index_globals(&[String])` refuses `X) Kill ^Y`, `^A("x")`, `A B`, an extended reference and more
      than 8 names with `INVALID_PARAMS` text, and `listing_code(master_name)` quotes the mask through `os_str_expr`
      (assert the generated text contains the quoted mask and no unquoted caller text). Implement the pure half of
      `src/tools/storage_map/candidates.rs`. Prove: `U test_storage_map_candidates_173`.
- [x] T022 [P] Test first, `tests/unit/test_storage_map_typing_173.rs` (+ `mod`): `property_type(evidence)` gives
      `%Integer` for 200 of 200 integers, `%Numeric` with scale 2 for decimals with at most 2 fraction digits,
      `date guess` only when ≥ 95% fit and reports the fit count, `%String(MAXLEN=n)` rounded up to the next of
      32, 64, 128, 255, 1024 from the longest seen; `collation_parameter(transform, length)` gives none for
      `sqlupper`, `COLLATION = "EXACT"` for exact, `"SQLSTRING"` for sqlstring, `"STRING"` for string, `"SQLUPPER(4)"`
      for the length variant; `class_source(name, properties)` builds text that contains no sampled value (the
      evidence struct has no value field, so assert by type) and marks each property in `guesses`. Implement
      `src/tools/storage_map/typing.rs` (pure half). Prove: `U test_storage_map_typing_173`.
- [x] T023 [P] Test first, `tests/unit/test_storage_map_bitmap_173.rs` (+ `mod`): `decode_id(chunk, position)` equals
      `(chunk-1)*64000 + position - 1` for (1,2)→1, (2,1)→64000, (1,64000)→63999; `bitmap_map(field_spelling)` returns
      a map of `Type bitmap` with the one subscript `{Field}`; `chunk_map_draft(const)` returns a data
      map equal to the structure of `tests/fixtures/storage172/classes/Bits.cls` (third level `AccessType Other`, one
      access variable, `NextCode`, one `Invalidcondition`), parsed back through `model::from_tree`. Implement
      `src/tools/storage_map/bitmap.rs`. Prove: `U test_storage_map_bitmap_173`.
- [x] T024 [P] Test first, `tests/unit/test_storage_map_usability_173.rs` (+ `mod`): `explain_query(class, field)` builds
      `EXPLAIN SELECT %ID FROM <scratch table> WHERE <field> = ?` with the field name validated;
      `parse_plan(text, map)` returns `Used(map)` for text containing "Read index map <map>, using the given" (the planner may also scan a map whole, with the warning: the warning decides), `Unused(text)`
      for each of the two warning forms pinned in T005, and `Unknown` for anything else, using plan texts captured in
      T005 (copy them into `tests/fixtures/storage173/captured/plan_*.txt`). Implement
      `src/tools/storage_map/usability.rs`. Prove: `U test_storage_map_usability_173`.
- [x] T025 [P] Test first, `tests/unit/test_storage_map_link_code_173.rs` (+ `mod`): `build_code(master, candidate, caps)`
      for F02's master and a hostile-looking candidate returns `Err` for a name that fails `validate_global_name`;
      for a good name the text contains the quoted global, contains `%SYSTEM.SQL.Functions` for each transform in the
      set, contains no `$ZU` and no `$ZCVT` and no ObjectScript `$ZCONVERT` (iad implements no collation), contains
      no `Write` of a field or subscript value (assert the only `Write` calls are the JSON line builders for counts,
      positions and names), and contains the `$Order` spread walk and the cap checks. Implement the skeleton of
      `src/tools/storage_map/link_code.rs` that makes this pass (the full generator grows in Phase 3). Prove:
      `U test_storage_map_link_code_173`.

**Checkpoint**: `U storage_map_link storage_map_master storage_map_candidates storage_map_typing storage_map_bitmap storage_map_usability storage_map_link_code` green.

---

## Phase 3: User Story 1 — Link an index global to its master and draft the map (P1) 🎯 MVP

**Goal**: for a master and an index global, `propose` returns the link and the drafted index map, for every transform,
ID form and null marker. **Independent test**: F01 to F18 each return their known answer, and each drafted map compiles
in a scratch copy and returns a known row through an indexed query.

- [x] T026 [US1] Test first, `tests/integration/test_storage_map_link_173_live.rs` (+ `mod`): for each of F01 to F10, load
      the fixture, call `propose` with `global=^IadSm173Person` and `index_globals=[<index global>]`, assert
      `indexes[0].subscripts[0]` has the known `field`, `transform`, `length` and `agreement ≥ 0.95`, assert
      `indexes[0].id.positions` and `verdict == inferred`. All fail now. Prove (red): `L test_storage_map_link_173_live`.
- [x] T027 [US1] Implement the linker body in `src/tools/storage_map/link_code.rs`: counting pass then every k-th key
      over the first subscript (FR-004a, `distinct_first_values`), master row by direct `$Get` on the master reference, ID position
      search over runs of k subscripts and the node value (FR-005), per-field per-position scoring over the transform
      set through `##class(%SYSTEM.SQL.Functions)` (FR-006/007), length variants confirmed on every node (FR-008),
      JSON lines out as in data-model.md. Wire `propose_action` in `mod.rs`: after the master draft, for each
      `index_globals` entry run `link_code::build_code` through `execute_via_generator_prefixed(…, "IrisDevSm")`, parse,
      and add `indexes` to the answer. Prove: `L test_storage_map_link_173_live` green for F01 to F10.
- [x] T028 [US1] Draft and verify: for each `inferred` link call `link::draft_index_map`, run it through `model::validate`
      and the no-IRIS write checks (FR-014), then verify it in a scratch copy: compile the master class text with the
      draft added through `compile_scratch_class` and query for a known row by the indexed field
      (`verified {compiled, row_found}`); a failed verification demotes the draft to `not_inferred_detail` with the
      reason. Test first: extend T026's test to assert `verified.row_found == true` and that the drafted expression
      string equals the fixture's declared expression. Prove: `L test_storage_map_link_173_live`.
- [x] T029 [US1] Fixtures F11 to F18 (catalogue), then test first in the same live file: F11 (ID position 3, two
      fields), F12 (k = 2 positions 2 and 3, draft subscripts end `{Key1}`,`{ID}`, the names the master draft gives the parts), F13 (`id.form = value`, draft has
      `Data name="Id"` and RowIdSpec `{Id}`), F14 (`data_in_value`), F15 to F18 (marker kinds; F18 has no `literal` key
      and its `guesses` names the missing `NullMarker`; with `include_values` and `dataPolicy=allow` plus
      `acknowledgePhi` the `literal` appears and the drafted subscript carries `NullMarker`; under `block` the call
      is refused with `DATA_POLICY_BLOCKED`). Prove: `L test_storage_map_link_173_live`.
- [x] T030 [US1] Implement what T029 needs in `link_code.rs` and `link.rs`: composite ID runs, value-ID form, `$LIST`
      covering (element-by-element test, FR-010), the null-marker set and discovered constant (FR-009), `NullMarker`
      emission only for a non-default marker with the literal under the data policy. Prove:
      `L test_storage_map_link_173_live`.
- [x] T031 [US1] Name-piece forms and `field_names`: test first that F10 returns `$p($$SQLUPPER({Name}),",",1)` and that
      `field_names=["Name","City","Age","Born","Status"]` replaces `Field1…5` in the master draft, the index drafts
      and `not_explained`; a short list is `INVALID_PARAMS` naming the sampled count. Implement. Prove:
      `L test_storage_map_link_173_live` and `U test_storage_map_master_173`.
- [x] T032 [US1] Capture live linker output for F02, F11, F13 and F27 into `tests/fixtures/storage173/captured/*.jsonl`
      (an env switch `IAD_REGEN_CAPTURED=1` in the live test rewrites them), replace T018's hand-written captures,
      and re-run the parser tests. The parser tests assert each fixture's declared known answer from its seed file, never a
      value read back from the capture (the capture only supplies the line format). Prove: `U test_storage_map_link_173` and
      `IAD_REGEN_CAPTURED=1 L capture` then `U test_storage_map_link_173`.

**Checkpoint**: a developer names an index global and gets the right map for every transform and ID form.

---

## Phase 4: User Story 3 — Say how sure, and what was not inferred (P1)

**Goal**: every link carries a verdict, an agreement and its alternatives; false links are not called inferred.
**Independent test**: F26 to F34 return their verdicts.

- [x] T033 [US3] Fixtures F26 to F34 (catalogue). Test first, `tests/integration/test_storage_map_verdicts_173_live.rs`
      (+ `mod`): F26 `ambiguous` with both fields in `alternatives` and the draft's pick in `guesses`; F27
      `too_few_nodes` and no `map`; F28 position `constant`; F29 (Age = ID) the ID position is the true one and Age is
      never reported as the ID; F30 no `inferred` link for the unrelated index; F31 `not_linked` with `agreement`;
      F32 (see US2); F33 position `not_explained` with `best_agreement` and no expression; F34 `inferred`. Prove (red):
      `L test_storage_map_verdicts_173_live`.
- [x] T034 [US3] Implement the remaining verdict paths in `link_code.rs` (ID tie-break to the last position, field
      preference over a one-value field, `constant`, the 0.9 ID floor, the 5% orphan rule for deleted rows with a
      reported share above it). Prove: `L test_storage_map_verdicts_173_live` green.
- [x] T035 [US3] `not_inferred` and `not_inferred_detail` per FR-028 and FR-032: update `shape::not_inferred()` to the new
      list (names what was not examined, not explained, computed subscripts, `COLLATE`, numeric collations, datatypes
      beyond four verdicts, conditions no rule explained, the ID counter) and add `not_inferred_detail`. Test first in
      `tests/unit/test_storage_map_link_173.rs`: every `not_inferred` string has a matching detail item; F33's call
      names its computed position. Prove: `U test_storage_map_link_173` and the 172 golden test
      (`U test_storage_map_golden_172`, which must change only by the added key; regenerate with
      `IAD_REGEN_GOLDEN=1` and check the diff is additive).
- [x] T036 [US3] The sampling claim: assert in `test_storage_map_verdicts_173_live.rs` that no answer key contains
      "unique", "cardinality" or "distinct count" except `distinct_first_values`. Prove:
      `L test_storage_map_verdicts_173_live`.

- [x] T037 [US3] Edge-case fixtures F38 to F43 (catalogue). Test first in `tests/integration/test_storage_map_verdicts_173_live.rs`:
      F39 examines six subscripts and `not_inferred` names the seventh; F40 compares `34` and `"34"` as stored and
      reports Age exact with no `not_explained`; F41 returns Name, sqlupper for non-ASCII names; F42 returns two drafts
      whose map names differ (FR-015); F43 links the candidate as an index candidate without merging it with the other
      class (`uses` still lists both); F38 is held to the 5 s cap in the budget test. Implement the six-subscript cap
      and the as-stored numeric comparison in `link_code.rs`. Prove: `L test_storage_map_verdicts_173_live`.

**Checkpoint**: a link at 200 of 200 reads differently from a guess, and the false-link set is clean (SC-003).

---

## Phase 5: User Story 2 — Find the index globals without being told (P1)

**Goal**: naming only the master finds its index globals. **Independent test**: F31, F32 and a namespace with
`^IadSm173A` and `^IadSm173A.x`.

- [x] T038 [US2] Fixtures for the three layouts (name prefix F47 `^IadSm173F47.x`, in-master sub-node F32, the 10-candidate
      case F48; made by `make_seeds_candidates.py`; F31 exists from T033). Test first,
      `tests/integration/test_storage_map_candidates_173_live.rs` (+ `mod`): `propose global=^IadSm173A` lists
      `^IadSm173A.x` with `found_by = name_prefix`; F31 is listed `not_linked`; F32's sub-tree is a `sub_node`
      candidate linked with the prefix cut off; with 10 candidates the answer links 8 and names 2 in
      `index_candidates.not_examined`. Prove (red): `L test_storage_map_candidates_173_live`.
- [x] T039 [US2] Implement the runner half of `candidates.rs` (listing through `%SYS.GlobalQuery:NameSpaceList` with the
      quoted mask `<master>*`, master excluded; failure is `listing: "unavailable"` with the error text, FR-004) and
      the wiring in `propose_action` (each candidate through the same global gates as the master before any code is
      built; a refusal is listed `refused`). Prove: `L test_storage_map_candidates_173_live` green.
      Done in `candidates_run.rs` (the runner), `candidates.rs` and `link_run::link_candidates`. The sub-trees inside
      the master are found by a scan of the master's string first subscripts in the same IRIS call as the listing
      (`sub_tree_code`), not from the master sample's `sub_nodes`: the sample reads numbers first and stops at its cap,
      and class mode has no sample. The scan runs only when the master's row ID is its first subscript.
- [x] T040 [US2] The failure path: a live test that makes the listing fail (the outcome T009 recorded; if the service
      account can always list, use a namespace argument the account cannot read) and asserts the answer says
      `listing: "unavailable"`, still links the explicit and in-master candidates, and `not_inferred` says the
      namespace listing was not available. Prove: `L test_storage_map_candidates_173_live`.
      R5 measured: a user holding only `%DB_USER` lists the namespace's globals, and no namespace leaves the master
      readable but not listable, so the test names a class query IRIS does not have
      (`candidates::LISTING_QUERY_FOR_TESTS`, compiled only with the `testing` feature); the text in the answer is
      IRIS's own (`Query '...' does not exist`).

**Checkpoint**: nobody lists their index globals to a tool that is meant to find them.

---

## Phase 6: User Story 1a — Add the missing index maps to a class with a master (P1)

**Goal**: `propose class=…` drafts index maps in the class's own names. **Independent test**: F35 and F36.

- [x] T041 [US1a] Fixtures F35, F36 (classes `IadSm173.F35` and `IadSm173.F36`). Test first, `tests/integration/test_storage_map_class_mode_173_live.rs` (+ `mod`):
      F35 `propose class=IadSm173.Person` drafts an index whose expressions use `{Name}` and the class's row-ID parts and
      whose `check` compiles with the draft added; F36 reports `already_mapped` naming the map and drafts nothing; a
      default-storage class, a columnar class and a class with no master map are refused with `read`'s reasons;
      `class` with `global` is `INVALID_PARAMS`; `field_names` with `class` is `INVALID_PARAMS`. Prove (red):
      `L test_storage_map_class_mode_173_live`.
- [x] T042 [US1a] Implement class mode in `propose_action`: load the class (`load_model`), `MasterSpec::from_class`, skip
      the master sample, run candidate discovery with `already_mapped` detection against the class's index maps'
      globals, link, draft, verify. The answer omits `draft`, `shape` and master `guesses`. Prove:
      `L test_storage_map_class_mode_173_live` green.
      Done in `propose_class.rs` (called from `handle` when `class` is given), with the class's map globals put
      through `check_map_globals` as `check` does.

**Checkpoint**: Benjamin's real case, with the names already right.

---

## Phase 7: User Story 8 — The same safety as the sampler (P1)

**Goal**: the new calls run under the existing controls. **Independent test**: the hostile-name suite and the control
tests.

- [x] T043 [US8] Test first, `tests/integration/test_storage_map_safety_173_live.rs` (+ `mod`): hostile `index_globals`
      (`X") Kill ^Y`, `^A("x")`, `a b`, `^|"OTHER"|G`, a 9-name list, an empty string) each return `INVALID_PARAMS` and
      the connection's request counter (a counting TCP proxy, `Where::through`) shows no request; an empty string or a
      non-string name is refused by the parameter types (`bad params for iris_storage_map`) before the handler, also with
      no request; strict read-only refuses `propose` as before; a
      sensitive-named candidate is judged by the global gate and listed `refused`; with `max_nodes=30` the answer's
      `limits.stopped` names each candidate and the cap that stopped it (`count_cap` for a candidate with more first
      subscripts than ten times the node cap, `nodes` otherwise); a time cap forced to 0 s on one candidate through
      `link_run::CANDIDATE_TIME_CAP_FOR_TESTS` (testing feature; a 1 s cap cannot be hit on a fixture this small)
      names the stopped candidate and keeps the other. Prove (red then green): `L test_storage_map_safety_173_live`.
- [x] T044 [US8] No value leaves IRIS (SC-004): a test that runs `propose` and `check` over F01 to F18 and F26 to F43,
      seeded with the rows' own long strings (case-insensitively), and over F02, F13 and F32 with the Name column
      replaced by `zq-seed-NNNN` (values that cannot occur in field names), serialises every answer to text and asserts
      none of the seeded values occurs, after asserting the seed list is non-empty and each seed is in the fixture's
      globals. `check` cannot run for F32 (the write gate refuses its class once data fills `idx`), so F32 is `propose`
      only. Red on F32: `shape.sub_nodes["2"]` held the index keys; `ShapeReport::for_answer` now withholds the keys of
      a depth of three or more once-only literal subscripts (below the first subscript) from the answer. The one
      exception test is F18 under the data policy (T029). Prove: `L test_storage_map_safety_173_live`,
      `U test_storage_map_safety_173`.
- [x] T045 [US8] Budget test: for a master with two candidates the connection's request counter shows at most 11 IRIS
      calls (plan.md table, corrected from 10) and the whole call stays under 5 s on the local container with
      1,000-node candidates (SC-005). Prove: `L test_storage_map_safety_173_live`. **Measured 2026-11-26: 11 IRIS calls
      (45 HTTP requests) in 0.76 s over F38 with `max_nodes` 1000.** A call is one scratch class PUT (the counter's
      unit): master sample 1, listing 1, link 2, typing 1, each draft verification 2 (the `Sm` scratch copy and the
      `IrisDevRun` helper that `compile_scratch_class_queries` runs for it), class draft compile 2 (the same pair). The
      first plan counted a scratch compile as one call; the ceiling is now 11 and a call added later fails the test.
- [x] T046 [US8] Strict read-only, service-account identity and gate classification: the classification check has no
      table in `tests/unit/test_storage_map_gates_172.rs`, so two tests are added there: `propose` with `index_globals`
      and with `class` is `Write`, `Execute`, service account and scratch; `check` is read-only, `Compile` and scratch;
      an unreadable `propose` spelling is judged as `edit` with both opt-ins. Prove: `U storage_map_gates`.

- [x] T047 [US8] Writes nothing (FR-027): test first in `tests/integration/test_storage_map_safety_173_live.rs`: before and
      after `propose` (global mode over F02, F13 and F19; class mode over F35; `check` over F35's class) record, per
      fixture global, the node count and the sum of node-value lengths, and the set of classes in `IrisDevTmp`; assert
      each is identical and no `IrisDevTmp.Sm*` class remains, after asserting the recorded sets are non-empty.
      Prove: `L test_storage_map_safety_173_live`.

**Checkpoint**: P1 stories done. A safe, useful `propose` can ship from here.

---

## Phase 8: User Story 4 — Bitmap indexes and bitmap-chunk data globals (P2)

**Goal**: bitmaps are recognised, decoded and drafted. **Independent test**: F19, F20, F21.

- [x] T048 [US4] Fixtures F19 to F21. Test first, `tests/integration/test_storage_map_bitmap_173_live.rs` (+ `mod`): F19
      `bitmaps[0]` has the chunk as its last subscript, `ids_checked == ids_found_in_master`, the drafted
      map is `Type bitmap` with the field as its only subscript and `verified.row_found`; F20 has the same with the
      index name first (`index_name`, and the drafted map's first subscript is that name, `"StatusIdx"`); a bitmap whose decoded IDs are not in the master is `not_linked`; a decode that needs an
      unconfirmed chunk size is `ambiguous` with `assumed` naming it; F21 returns `chunk_map_draft` whose
      scratch-compiled table returns the IDs the bits name. Prove (red): `L test_storage_map_bitmap_173_live`.
- [x] T049 [US4] Recognise a bit-string master in the linker (plan.md: the sampler and `ShapeReport` do not change). Test
      first, `tests/unit/test_storage_map_link_code_173.rs`: `master_bit_probe(global, caps)` quotes the global through
      `os_str_expr`, writes only counts and positions, and decodes with `$BitFind`; live in T048's file: F21's master
      is recognised and an ordinary delimited master (F01) is not. Implement `link_code::master_bit_probe` and call it
      from `propose_action` only when the master sample shows depth-2 nodes with a numeric last subscript and no
      `$LIST` shape (a dense bit string holds the delimiter bytes the sampler looks for, so `delimited` does not rule
      it out; the probe decides). Prove: `U test_storage_map_link_code_173`, `L test_storage_map_bitmap_173_live`, and
      `U test_storage_map_golden_172` unchanged.
- [x] T050 [US4] Implement bitmap detection and decode in `link_code.rs` (`$BitFind`, the chunk as last subscript, up to 1,000
      IDs, at most 40 from one node so the sample spans values; IDs from chunk 2 on are counted apart because only
      they confirm the chunk size) and the drafts in `bitmap.rs` wired into `propose_action` (`bitmaps`, `chunk_map_draft`, each
      verified in a scratch copy before `inferred`). Prove: `L test_storage_map_bitmap_173_live` green.

---

## Phase 9: User Story 5 — Sparse and conditional indexes (P2)

**Goal**: rows the index lacks are reported and, when a rule fits, drafted as a condition. **Independent test**: F22 to F25.

- [x] T051 [US5] Fixtures F22 to F25. Test first, `tests/integration/test_storage_map_sparse_173_live.rs` (+ `mod`): F22
      `missing_rows.rule` NULL and the draft carries `Condition {City}'=""` with `PopulationType conditional`; F23 rule
      empty-string and the draft's condition is the form T009 proved (or the case is in `not_inferred_detail` if T009
      failed); F24 the condition `{Status}="A"`; F25 no rule and `missing_rows.share` equals 1/7 within 0.02; each
      drafted condition passes the scratch verification (same rows through the map as through the master). Prove
      (red): `L test_storage_map_sparse_173_live`.
- [x] T052 [US5] Implement missing-row classification in `link_code.rs` (rows of the master sample with no index node:
      NULL, empty string, one shared value, else no rule; the rule must explain every missing row and no present row)
      and the condition draft in `link.rs`. Prove: `L test_storage_map_sparse_173_live` green. Done: `MISSING_BODY` in
      `link_code.rs`, `missing_rows_json`, `condition_for` and `apply_condition` in `link.rs`, the kept-rows check
      (`verify_with`) in `link_verify.rs`, and the wiring in `link_run.rs`. The empty-string rule is `{F}'=$c(0)`, not
      `{F}'=""`, which skips NULL only.

---

## Phase 10: User Story 6 — A draft class with typed properties (P2)

**Goal**: `class_draft` with guessed types and collations. **Independent test**: F37 and the linking fixtures.

- [x] T053 [US6] Fixture F37. Test first, `tests/integration/test_storage_map_typing_173_live.rs` (+ `mod`): the integer
      column is `%Integer` with evidence `{integer: n, of: n}`, decimals `%Numeric` scale 2, the date-like column a date
      guess with `date_fit` ≥ 0.95 of `of`, text `%String` with `MAXLEN` ≥ the longest value; the property for a
      field linked as exact has `COLLATION = "EXACT"`, as sqlstring `"SQLSTRING"`, as sqlupper none; `class_draft.compiled`
      is true and the source compiles in a scratch copy with the drafted maps; every property path is in `guesses`;
      F26-style data does not make a date. Only a `%String` takes `COLLATION` (IRIS refuses it on `%Integer`, `%Numeric`
      and `%Date`, ERROR #5480), so the other types carry none. Prove (red): `L test_storage_map_typing_173_live`.
- [x] T054 [US6] Implement the generator and runner in `typing_run.rs` (one call, verdict counts only, over a spread of the
      master's rows to the node cap: the linker returns no row IDs, so the call walks the master itself) and the wiring (scratch compile of the class draft with the
      drafted maps, `compiled` set from the real result). Prove: `L test_storage_map_typing_173_live` green.

---

## Phase 11: User Story 7 — `check` says whether an index map would be used (P2)

**Goal**: the planner's own words per index map. **Independent test**: the T005 pair.

- [x] T055 [US7] Test first, `tests/integration/test_storage_map_usability_173_live.rs` (+ `mod`): `check` with a two-subscript index map `({City}, {Age})` returns one line for `City` and one for `City` + `Age`, and with an index map
      `{Name}` over a default-collated class returns `usability[0].status == "unused"` and `text` containing "no index
      subscript collation"; with `$$SQLUPPER({Name})` `"used"` and the map's name; with `$$SQLUPPER({Name},4)` `"unused"`
      and "%SQLUPPER(4)"; a plan the parser cannot read gives `unknown` and `check` still succeeds; the lines are also in
      `warnings`; `check`'s `success` and refusals are the same as before for every 172 fixture. Prove (red):
      `L test_storage_map_usability_173_live`.
- [x] T056 [US7] Implement in `check_action` (`mod.rs`): after the scratch compile, one `EXPLAIN` per index map and
      field-prefix in the scratch copy through `usability::explain_query`, parsed by `usability::parse_plan`, into
      `usability` and `warnings`. Prove: `L test_storage_map_usability_173_live` green and `L storage_map_edit_172 storage_map_paths_172 storage_map_gates_172` (the
      172 tests that call `check`) unchanged.
- [x] T057 [US7] Wire the usability line into `propose`'s verification (T028): an `inferred` draft is reported `verified`
      only when its own usability line is `used` for the field it links (SC-006). Prove:
      `L test_storage_map_link_173_live test_storage_map_usability_173_live`.

---

## Phase 12: User Story 9 — A public corpus with known answers, and honest docs (P2)

**Goal**: the corpus holds the answers, and the docs say only what a test proves.

- [x] T058 [US9] The corpus: `tests/integration/test_storage_map_corpus_173_live.rs` (+ `mod`) runs every fixture (every seed file: F01 to F48,
      F19c, F19n, and the boundary pair F49, F50)
      through `propose`, asserts each fixture's known answer (SC-001: at least 30 fixtures, 100% of them), that every
      `inferred` draft compiles and returns the fixture's known row (SC-002), that no false-link fixture is called
      `inferred` for the wrong field (SC-003), and compares the answer to `golden/<fixture>.json`
      (`IAD_REGEN_GOLDEN=1` rewrites). The test asserts the fixture count ≥ 30 before the loop. Prove:
      `L test_storage_map_corpus_173_live`.
- [x] T059 [US9] Mutation check: temporarily change one transform's spelling in `link::spelling` and one verdict threshold
      and confirm the corpus fails on each (record the two failing test names in the task's report; revert). This is the
      "if I changed this silently would a test fail" check. Prove: the report names both failures.
      Done: the corpus is one table-driven test
      (`test_storage_map_corpus_173_live::every_fixture_gives_its_known_answer_and_matches_its_golden`) that names each
      failing fixture. Spelling mutation (`Transform::Upper => "ALPHAUP"`) failed F06 and F34. `MIN_NODES` 20 to 4 failed
      F27. `EXPLAINED` 0.95 to 0.90 did NOT fail (no fixture sat between 0.90 and 0.95), so F49 (agreement exactly 0.95,
      inferred) and F50 (0.925, not_explained) were added; the same mutation now fails F50 and 0.95 to 0.96 fails F49.
      All mutations reverted.
- [x] T060 [US9] Docs: `docs/storage-maps.md` gains the new params, the new keys of data-model.md, the verdict table and a
      "measured" table listing facts 1 to 12 each with its test name from T002 to T009. Test first,
      `tests/unit/test_storage_map_docs_173.rs` (+ `mod`): every response key of data-model.md appears in the doc, every
      `fact_*` and `r*_` test name the doc cites exists in `test_storage_map_measured_173_live.rs`, and every test in
      that file is cited. Prove: `U test_storage_map_docs_173`.
- [x] T061 [US9] Skill: `skills/skills/iris-storage-mapping/SKILL.md` gains "What `propose` infers and what it does not"
      (verdicts, the closed expression set, bitmap map with the field alone, the empty-string versus NULL condition, drafts are
      guesses to `check`, `class` mode); each claim names its live test. Bump the skill version. Prove: the existing
      skill tests (`U test_skill_tiers`, `U storage_map_skill`) and a new assertion in T060's file that each claim line
      cites an existing test.
- [x] T062 [US9] Glossary: `CONTEXT.md` gains candidate index global, link, transform, usability line, spread sample
      (definitions only, no implementation). Prove: `markdownlint-cli2 --fix CONTEXT.md` and `prettier --write CONTEXT.md`.
- [x] T063 [US9] Clean-room scan: a unit test `tests/unit/test_storage_map_cleanroom_173.rs` (+ `mod`) reads every file under
      `specs/173-propose-index-linking/`, `tests/fixtures/storage173/`, the new `src/` files, `docs/storage-maps.md` and
      the skill and fails on a source-control word, a depot path, an internal collation call or compiler-routine name,
      or a `$$$` macro from the compiler's include files. The forbidden words live in the test's own list, and the
      scan skips that test file and `tasks.md` (which names them). It asserts the scanned file list is non-empty and
      that a planted forbidden word in a temp copy is caught. Prove: `U test_storage_map_cleanroom_173`.

---

## Phase 13: Lift (release gate for 172, constitution IX)

- [X] T064 Write `tests/e2e/tasks/skills/iris-storage-mapping/SKILL-74.yaml` (a task: a class that maps only
      `^IadSm173Person`, an index global `^IadSm173PersonX` beside it, "make lookups by Name use it"; fixture, seed,
      teardown, check with the known answer from F02's seed rows). The id is SKILL-74 and the directory is the
      authored tree, not `tests/e2e/tasks/benchmark/skills/SKILL-30.yaml`: `SKILL-30` is already
      `objectscript-sql-patterns`'s task, `SKILL-26` to `SKILL-29` exist under `tasks/skills/objectscript-guardrails/`,
      and 172's own `iris-storage-mapping` tasks are `SKILL-70` to `SKILL-73` in that directory (the loader refuses a
      duplicate id and refuses a task filed under another skill's name). `split.toml` lists it as holdout. The
      reference was run live: fixture FAIL, reference PASS, and four wrong answers (plain `{Name}`, `$$SQLSTRING`, a
      map on another global, a map on an empty global) FAIL. Test first in
      `tests/e2e/skill_eval/test_storage_index_lift_173.py`. Prove: `pytest tests/e2e/skill_eval/test_storage_index_lift_173.py`
      (`-k skill_tasks` matches no test in this tree) and `validate_live` on the file.
- [ ] T065 Lead only, with Tom's go-ahead on cost: run the ladder for SKILL-74 with `--cap 3` (measured cost, not the
      estimator) and write `specs/173-propose-index-linking/lift-results.md` (tools arm, tools+skill arm, repeats, b and
      c counts, "underpowered" when it is). A `needs_fix` flag becomes a task. Constitution IX's +0.20 threshold is judged on 172's tool as a whole; this run adds
      the evidence for `propose`'s new behaviour.

---

## Phase 14: Polish

- [X] T066 [P] `docs/tools.md` and `docs/error-codes.md`: the `iris_storage_map` row names the three params; no new
      code, so the error-code guard must stay green. Prove: `U docs_tools error_codes`.
- [X] T067 [P] `CLAUDE.md` Recent Changes: one entry for 173 inside the 172 entry's neighbourhood (what `propose` and
      `check` now return, the bitmap layout finding, the NULL versus empty-string finding, `%SYSTEM.SQL.Functions`
      as the server's own collation, plan path). Prove: `markdownlint-cli2 --fix CLAUDE.md`.
- [X] T068 [P] Release notes: add the 173 lines to `specs/172-sql-storage-maps/release-notes-draft.md`, then `/wordcraft`
      and `/no-ai-slop` on the finished text, apply, and show Tom. Prove: the reviewer block is empty or addressed.
- [ ] T069 Background coverage (best effort, never blocks): `nice -n 15 cargo llvm-cov` on the new modules with
      `--include-ignored` against the live container, compare with the committed baseline, report a drop when it lands.
- [X] T070 Lead only: delete the probe classes and globals left on `iris-dev-iris` by Phase 0 research
      (`Iad173.*`, `^Iad173*`), confirm `$Order(^IadSm173...)` is empty after the full live run, and run
      `LINT`, `U storage_map`, and `L _173_live` once from the worktree root. Report any failure with its output.
- [X] T071 Lead only: commit locally in logical commits (spec and plan; Phase 0; each story), `/simplify` before each
      commit, `cargo fmt --all -- --check`. Never push without Tom's words.

---

---

## Requirement coverage

| Requirement            | Tasks                                                                     |
| ---------------------- | ------------------------------------------------------------------------- |
| FR-001, FR-001a        | T012, T014, T020, T021, T026, T027, T041, T042                            |
| FR-002, FR-003, FR-004 | T009, T021, T033, T038, T039, T040                                        |
| FR-004a (spread)       | T009, T027                                                                |
| FR-005 to FR-008       | T017, T025, T027, T031                                                    |
| FR-009, FR-010         | T029, T030                                                                |
| FR-011, FR-012         | T017, T033, T034, T044                                                    |
| FR-013 to FR-015a      | T017, T019, T020, T028, T031, T037                                        |
| FR-016 to FR-018       | T023, T048, T050, T049                                                    |
| FR-019                 | T051, T052                                                                |
| FR-020 to FR-022       | T022, T053, T054                                                          |
| FR-023                 | T024, T055, T056                                                          |
| FR-024, FR-025         | T021, T025, T043, T045                                                    |
| FR-026                 | T035, T058                                                                |
| FR-027                 | T047, T070                                                                |
| FR-028, FR-032         | T035                                                                      |
| FR-029, FR-030         | T002 to T009, T060, T061, T062                                            |
| FR-031                 | T063                                                                      |
| SC-001 to SC-003       | T033, T058                                                                |
| SC-004                 | T044                                                                      |
| SC-005, SC-006         | T045, T057                                                                |
| Edge cases             | F27 T033; deleted rows T034; extended reference T043; the six others T037 |

---

## Dependencies & execution order

- Phase 0 first: T001 then T002 to T009 in parallel (one file; split across teammates by `mod` blocks only if the
  lead merges), T010 gates Phase 3 onward.
- Phase 1 needs nothing from Phase 0 except T001's rig for T016. Phase 2 (T017 to T025) is all parallel after T011.
- Phase 3 (US1) needs Phase 2 and T010. Phase 4 (US3) needs Phase 3. Phase 5 (US2) and Phase 6 (US1a) need Phase 3 and
  can run in parallel with Phase 4. Phase 7 (US8) needs Phases 3 and 5 (it asserts over their behaviour).
- Phases 8 to 11 (US4 to US7) each need Phase 3; US4, US5 and US6 are independent of each other; US7 (T057) needs US1's
  verification step.
- Phase 12 (US9) needs every story. Phase 13 needs Phase 12. Phase 14 last.

## Parallel examples

- Phase 2: seven teammates, one per pure module (T017+T018+T019 link; T020 master; T021 candidates; T022 typing; T023
  bitmap; T024 usability; T025 link_code skeleton), each in its own worktree.
- After Phase 3: US4, US5, US6 and US1a in parallel worktrees; the lead merges them in story order and re-runs the
  live files serially.

## Implementation strategy

- **First ship point**: Phases 0 to 7 (US1, US3, US2, US1a, US8). It already answers Benjamin's request: link, draft,
  verdicts and `not_inferred`, with every safety control.
- **All of 173 is built before 172's maps branch merges** (Tom, 2026-11-26): Phases 8 to 12 are not deferred, only
  ordered after the P1 stories because they build on them.
- Count: 71 tasks. Phase 0 measure 10; setup 6; foundational 9; US1 7; US3 5; US2 3; US1a 2; US8 5; US4 3; US5 2; US6 2; US7 3; US9 6; lift 2; polish 6.
