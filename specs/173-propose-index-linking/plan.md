# Implementation Plan: `propose` reads index globals, collations and datatypes (173)

**Branch**: `172-sql-storage-maps` (no new branch; 173 ships with 172) | **Date**: 2026-11-26 | **Spec**: [spec.md](spec.md)
**Input**: `specs/173-propose-index-linking/spec.md`; measurements in [research.md](research.md); parent plan
`specs/172-sql-storage-maps/plan.md`.

## Summary

`propose` today samples one global and drafts one delimited data map with placeholder names. 173 teaches it, and
`check`, to go the other way as well as 172 goes forward: find the index globals beside a master (or beside a class's
own master map), link each to the master's row IDs and fields through the server's own collation functions, draft the
index maps (bitmap, value-ID, covering, sparse and conditional included), draft a class with typed properties, and say
what it did not infer. `check` adds the planner's own verdict on each index map. Everything is built at once; there is
no staged delivery.

The linking runs inside IRIS as generated ObjectScript through the sampler's existing path and returns counts,
positions, names and verdicts only. The Rust side is pure modules (types, verdict rules, draft builders, code
generators, parsers) tested without IRIS, plus a thin orchestration in `propose_action` and `check_action`, with live
tests on `iris-dev-iris`.

## Technical Context

**Language/Version**: Rust 2021, crate `iris-agentic-dev-core`; generated ObjectScript (IRIS 2026.2)
**Primary Dependencies**: existing only (`serde_json`, `regex`, `tokio`, `reqwest`). No new crate.
**Storage**: none in iad; IRIS reads and scratch copies only
**Testing**: `cargo test --features testing` (`unit`, `integration`, `binary` aggregates; a new file needs its `mod`
line); live tests serial against `iris-dev-iris`; fixtures invented under `IadSm173`
**Target Platform**: the single `iris-agentic-dev` binary
**Project Type**: MCP server tool (an additive change to two actions of `iris_storage_map`)
**Performance Goals**: a `propose` over a master and two candidates of 1,000 sampled nodes each finishes inside the
5 s cap (SC-005); IRIS round trips bounded by the budget below
**Constraints**: no sampled value returns (FR-012); no collation is implemented by iad (FR-007); every global name goes
through `validate_global_name` and `os_str_expr` (FR-025); `MODEL_VERSION` stays 1 and 172's golden files do not change
except for added keys (FR-026); clean-room: no InterSystems internal source, path, routine or macro appears in any
tracked file (FR-031)
**Scale/Scope**: about 2,000 lines of Rust and 400 of generated ObjectScript templates, 30 or more corpus fixtures

## Constitution Check

| Principle                      | How 173 meets it                                                                                                                                                                                  |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-install binary         | Atelier REST and `execute_via_generator_prefixed` only; no Docker path                                                                                                                            |
| II. ObjectScript sanity gate   | Every IRIS behaviour in `research.md` R1 to R9 was run live first; the three still open (R5 low-privilege listing, R3 `$c(0)` condition, spread against prefix) are the first task of their phase |
| III. HTTP-first                | Same as I                                                                                                                                                                                         |
| IV. Test-first, fixture-driven | Pure modules take captured live output as fixtures; tests are written before the code they test, and each live test asserts the fixture's known answer                                            |
| V. Output shape parity         | New keys only on `propose` and `check`; no key removed or renamed; 172 goldens unchanged except added keys                                                                                        |
| VI. Environment guard          | `propose` stays an execute call as the service account and is refused under strict read-only; `check` stays read-only and scratch; no new category                                                |
| VII. Dependency minimalism     | No new crate                                                                                                                                                                                      |
| VIII. 90% coverage             | Polish runs the canonical coverage command in the background (Tom, 2026-11-19: best effort, never blocks)                                                                                         |
| IX. Tool lift                  | One ladder task (`SKILL-30`, index linking) with `--cap`; result in `lift-results.md`; a release gate for 172 as a whole                                                                          |
| X. ObjectScript coverage       | Not applicable: no ObjectScript class ships; generated snippets only                                                                                                                              |
| XI. No vacuous tests           | Live tests use `Where::live()`; every loop asserts non-empty first; a test that scans answers for seeded values asserts the seeds were non-empty                                                  |
| XII. Hermetic tests            | No new behaviour variable; `IAD_REGEN_GOLDEN` already listed                                                                                                                                      |
| XIII. Single-source failure    | Generator output through `generator_error_message`; scratch compile through `compile_scratch_class`; one verdict function (`link::verdict`) used by the draft builder and the docs table          |
| Error code registry            | No new code; `INVALID_PARAMS`, `STORAGE_MAP_TIME_CAP`, `IRIS_EXECUTE_ERROR` reused (data-model.md)                                                                                                |
| User-facing strings are API    | The tool description and `docs/tools.md` table name `index_globals`, `class` and `field_names`; the doc test lists every new response key                                                         |

No violations. Three items need care rather than an exception:

- **A bit-string master is recognised in the linker, not the sampler.** `sample.rs` and `ShapeReport` do not change, so
  the 172 golden files stay as they are. When the master sample shows depth-2 nodes with a numeric last subscript and
  no delimiter or `$LIST` shape, `link_code::master_bit_probe` runs one more generator call (counted in the budget
  below) that decodes the nodes with `$BitFind`; the chunk-data-map draft (FR-018) is accepted only after the scratch
  verification returns the IDs the bits name.
- **Bug class `objectscript-splice`.** The only caller text that reaches generated code is `index_globals` (and the
  names derived from a class's maps). Each passes `validate_global_name` and is quoted by `os_str_expr`; field names
  from `field_names` never enter ObjectScript (they enter Rust-built JSON and class text only, after
  `is_valid_property_name`). A hostile-name suite (US8) fails the build if a name reaches code unquoted.
- **No values out.** A null marker's literal is the one value that can leave, and only under `include_values` with the
  data policy's refusal and acknowledgement (FR-009). The scan test (SC-004) walks every corpus answer for the seeded
  values.

## Project Structure

### Documentation (this feature)

```text
specs/173-propose-index-linking/
├── spec.md
├── plan.md              # this file
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   └── propose-and-check.md
├── checklists/requirements.md
├── lift-results.md      # written in the lift phase
└── tasks.md             # /speckit.tasks
```

### Source code

```text
crates/iris-agentic-dev-core/src/tools/storage_map/
├── link.rs        # pure: Transform, Link, Verdict, `verdict()`, expression spellings (closed set), map-name
│                  #   uniqueness, draft_index_map(link, master) -> Value; parse of the linker's JSON lines
├── link_code.rs   # pure: build_code(master, candidate, caps) -> String (the ObjectScript linker); spread walk,
│                  #   ID position search, per-position transform scoring, null markers, covering, bitmap decode,
│                  #   missing-row classification
├── candidates.rs  # pure + one runner: listing code, in-master sub-node candidates from the ShapeReport,
│                  #   dedupe, the 8-candidate cap, the `found_by` labels
├── master.rs      # pure: MasterSpec from a draft (global mode) or from a class's master map (class mode)
├── typing.rs      # pure: build_code for datatype verdicts, parse, class_draft source builder, property-name check
├── bitmap.rs      # pure: chunk-data-map draft (computed third level, from IadSm172.Bits), bitmap index draft
├── usability.rs   # pure: the EXPLAIN query per index map, the parse of "used" and the warning forms
├── shape.rs       # not_inferred() updated; draft_with_links assembles the additive keys
└── mod.rs         # propose_action and check_action call the modules above; no logic of their own

crates/iris-agentic-dev-core/src/tools/params/batch8.rs   # IrisStorageMapParams: + index_globals, class (propose), field_names

crates/iris-agentic-dev-core/tests/
├── unit/test_storage_map_link_173.rs          # verdict rules, spellings, drafts, parser on captured live output
├── unit/test_storage_map_link_code_173.rs     # generated code: names quoted, no value written, collation calls named
├── unit/test_storage_map_candidates_173.rs
├── unit/test_storage_map_master_173.rs
├── unit/test_storage_map_typing_173.rs
├── unit/test_storage_map_bitmap_173.rs
├── unit/test_storage_map_usability_173.rs
├── unit/test_storage_map_docs_173.rs          # every new key and measured fact listed and cited
├── integration/test_storage_map_measured_173_live.rs   # facts 1 to 8 and R1 to R7: one live test each
├── integration/test_storage_map_link_173_live.rs       # transforms, ID positions, null markers, covering
├── integration/test_storage_map_verdicts_173_live.rs   # false-link set, ties, too few, constant, not explained
├── integration/test_storage_map_candidates_173_live.rs
├── integration/test_storage_map_class_mode_173_live.rs
├── integration/test_storage_map_bitmap_173_live.rs
├── integration/test_storage_map_sparse_173_live.rs
├── integration/test_storage_map_typing_173_live.rs
├── integration/test_storage_map_usability_173_live.rs
├── integration/test_storage_map_safety_173_live.rs     # hostile names, caps, strict, gates, no-value scan, budget
├── integration/test_storage_map_corpus_173_live.rs     # the 30-fixture corpus and SC-001 to SC-006
└── binary/test_storage_map_schema_173.rs               # the tool's schema lists the new params

tests/fixtures/storage173/
├── classes/*.cls        # IadSm173.* masters and index-holding classes, canonical form
├── seed/*.json          # per fixture: globals to set, the known answer (field, transform, length, marker, id position, verdict)
├── captured/*.jsonl     # linker output captured from live runs, for the pure parser tests
└── golden/*.json        # pinned `propose` answers per fixture (IAD_REGEN_GOLDEN=1 rewrites)

skills/skills/iris-storage-mapping/SKILL.md   # new section: what propose infers and what it does not
docs/storage-maps.md                          # new keys, params, measured facts with test names
CONTEXT.md                                    # glossary: candidate index global, link, transform, usability line
tests/e2e/tasks/benchmark/skills/SKILL-30.yaml
```

**Structure decision**: one new pure module per concern so each can be built and tested alone by one implementer;
`mod.rs` only wires. The generator (`link_code.rs`) is the one large piece and is split from `link.rs` so its output
shape is pinned by captured-output tests on the parser side and by name-quoting tests on the generator side.

## Design

### Where it runs and what returns

`propose_action` today: normalise the global, refuse values without the data policy, sample, draft. 173 keeps that
order and adds, after the master draft (or after reading the class's master map in class mode):

1. **Candidates** (`candidates.rs`): the `index_globals` list; the namespace listing through
   `%SYS.GlobalQuery:NameSpaceList` with mask `<master>*` (a failure is reported, not fatal); literal first
   subscripts of the master from the master's `ShapeReport`. Cap 8, with the rest named. Each is judged by the same
   global gates as the master before any code is built.
2. **Link per candidate** (`link_code.rs` → one generator call each): spread walk, ID position, per-position scoring
   over the transform set, null markers, covering, bitmap decode, missing-row classification. The generated code calls
   `##class(%SYSTEM.SQL.Functions).<Name>` for every transform (research R7) and reads the master row by a direct
   `$Get` on its reference, one lookup per node. It writes one JSON line per position and one summary line, with no
   subscript or field value in them.
3. **Verdict and draft** (`link.rs`): the verdict function is the one place the thresholds live (≥ 20 usable nodes,
   ≥ 0.95 agreement, ties, constant, not explained). `draft_index_map` builds the map from the closed set of
   spellings; a spelling outside the set is a programming error caught by a unit test, never emitted.
4. **Verify drafts**: each `inferred` draft goes through the model validator and the write path's no-IRIS checks
   (FR-014), then the scratch copy (the `check` path) compiled with the draft added and queried for a known row by the
   indexed field (SC-002); failure demotes the draft to `not_inferred` with the reason.
5. **Typing and class draft** (`typing.rs`): one generator call over the master rows the linker reached through the
   sampled IDs plus the master's first nodes to the cap; verdict counts only. The class source is built in Rust from
   the verdicts and the linking evidence; it is compiled in a scratch copy with the drafted maps before it is returned.
6. **Assemble** (`shape::draft_with_links`): additive keys only (data-model.md).

`check_action` adds, after its scratch compile, one `EXPLAIN` per index map and field-prefix in the same scratch
copy, parsed by `usability.rs` into `used`, the planner's warning, or `unknown`; the result goes under `usability` and
into `warnings`, never into the pass or fail decision.

### The spread walk (FR-004a)

A prefix walk is not enough (measured, `open_spread_beats_prefix`): over 1,000 keys whose first 800 are plain and last
200 contain blanks and hyphens, the first 200 nodes score `$$UPPER` and `$$ALPHAUP` equally, and a stride over all keys
separates them. So the generated walk is two passes. A counting pass steps the first subscript with `$Order`, under its
own cap of 10 times the node cap and inside the time cap, keeping the key list. A second pass reads every k-th kept key
(k so the sample reaches the node cap) and up to three nodes under it with a nested `$Order`. If the counting pass sees
fewer than seven distinct values it restarts as an ordered walk to the cap. When the counting pass stops at its cap the
answer says the spread covers only the part walked. It reports `distinct_first_values`. Nothing in the answer claims
uniqueness or cardinality.

### Position scoring

For each master field and each transform in the set, per non-ID position, the code counts nodes where
`transform(field value of the row with that ID)` equals the subscript, over nodes with a master row. A node whose
field is empty is judged against the null-marker set instead (spec FR-009). Selection follows spec FR-011, in this
order: constant position (one value across the sample) → `constant`; fewer than 20 usable nodes → `too_few_nodes`; best
score below 0.95 with the ID found → `not_explained`; two (field, transform) pairs within 0.01 of the best →
`ambiguous` with every tied pair; otherwise `inferred`. A length variant is only tried when no untruncated transform
reaches 0.95, and its length is the longest subscript seen at that position, confirmed on every node (FR-008).

### Class mode (`class` instead of `global`)

`master.rs` reads the class through the existing reader (`load_model`), requires an editable `%Storage.SQL` with a
`data` master map, and builds the `MasterSpec` from the map's own subscripts, `RowIdSpec` parts and `Data` fields.
Candidates whose global is already used by one of the class's index maps are marked `already_mapped` and not linked.
The master is not re-sampled. `field_names` with `class` is `INVALID_PARAMS`.

### Gates and caps (unchanged, restated)

Classification of `propose` is as in 172's table (execute, service account, refused under strict read-only). The node
cap is per candidate (default 200, max 1000), the 5 s cap is for the whole call, and the answer's `limits` says which
cap stopped which candidate. Under `block` and `redact` a request for the null-marker literal is refused with the
existing `values_refusal`; without `include_values` the linking answer carries no value and needs no acknowledgement.

### IRIS round trips (budget test)

| Step                        | Calls                                                                                    |
| --------------------------- | ---------------------------------------------------------------------------------------- |
| Master sample (global mode) | 1 (existing)                                                                             |
| Class read (class mode)     | 1 GET                                                                                    |
| Namespace listing           | 1                                                                                        |
| Link                        | 1 per candidate, at most 8                                                               |
| Typing                      | 1                                                                                        |
| Draft verification          | 2 per `inferred` draft (the scratch compile and the helper class the query runs through) |
| Class draft compile         | 2 (the same pair)                                                                        |

A live test counts calls through the connection's request counter for a master with two candidates and fails above
`1 + 1 + 2 + 1 + 2×2 + 2 = 11` (12 with the bit-string probe, which the F21 test allows for). Measured 2026-11-26: 11
calls, 45 HTTP requests, 0.76 s over F38. The first plan counted a scratch compile as one call; each is two (the scratch
class PUT and the helper class the query runs through), a cost of 172's scratch-compile helper.

## Phases (tasks.md regroups these by user story; its order is the one to follow)

Each phase ends with its tests green; no phase starts before the previous one's gate. Phase 0 is research and
measurement; its tests are the facts the rest rests on.

0. **Measure first**: a live test for each of spec context facts 1 to 8 and research R1 to R7; the three open items
   (R3 `$c(0)` condition expression, R5 low-privilege listing, R8 spread against prefix). A failure stops the piece it
   guards and goes to the owner (spec, Clarifications 2026-11-26).
1. **Setup**: fixtures directory, empty modules, params (`index_globals`, `class`, `field_names`), `mod` lines, the
   docs and invariants guards green.
2. **Pure core** (parallel): `link.rs`, `master.rs`, `candidates.rs` (pure half), `typing.rs`, `bitmap.rs`,
   `usability.rs`, each with unit tests first.
3. **Linker** (US1, US3, US8 start): `link_code.rs`; transforms, ID positions, null markers, covering, composite key,
   value form; the false-link set.
4. **Candidates** (US2): listing, sub-node candidates, cap, failure path.
5. **Class mode** (US1a).
6. **Bitmaps** (US4): index and chunk-data-map drafts.
7. **Sparse** (US5).
8. **Datatypes and class draft** (US6).
9. **`check` usability** (US7).
10. **Safety** (US8): hostile names, caps, strict, gates, no-value scan, budget.
11. **Corpus, docs, skill** (US9): 30-fixture corpus, SC-001 to SC-006, `docs/storage-maps.md`, skill, `CONTEXT.md`.
12. **Lift** (release gate for 172; constitution IX measures the tool, and `SKILL-30` adds the evidence for this action): `SKILL-30`, `lift-results.md`.
13. **Polish**: background coverage, `docs/tools.md`, `CLAUDE.md` Recent Changes, release notes through `/no-ai-slop`.

## Delegation

Implementation teammates run on sonnet; the Opus lead specifies, reviews each diff (`git diff` in the teammate's
worktree), and runs the test targets before anything is committed (team rules in `~/.claude/CLAUDE.md`). Phase 2's six
pure modules go to parallel worktrees; each teammate gets its file, its tests-first list, the function signatures from
`data-model.md`, and the command that proves it. IRIS tasks run one at a time against `iris-dev-iris`
(`--test-threads=1`). On the laptop: `nice -n 15 cargo … -j 2`, one test file at a time, no live tool sweep or full
binary target (both are CI's); long runs go to mac-studio.

## Complexity Tracking

No constitution violations to justify.
