# Tasks: Typed input schemas for every MCP tool

**Input**: Design documents from `/specs/113-typed-tool-schemas/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: Mandatory at every layer. Per the constitution's Test Coverage Policy, each batch
writes its schema assertions before the struct exists, so the test fails first. Layer 2 (spawn
the binary, read `tools/list`) is the phase gate for each conversion batch; Layer 3 (live
`iris-dev-iris`) proves the parameter is still honored at runtime.

**Organization**: Grouped by user story. US1 is delivered as 8 conversion batches rather than
31 per-tool tasks — see plan.md's task decomposition rule (Clarification Prompt 3, ≤40 tasks).
Where a test and its implementation live in one small file, they are one task whose description
states test-first ordering; that keeps the count at 42 without deferring a single test. The
release-gate phase (7) sits ahead of Polish because Principle IX puts behavioral evidence there,
and because a schema that is correct but degrades task success is a blocker, not a polish item.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: US1–US4 from spec.md
- Paths are relative to the repository root

## Path conventions

- Core crate: `crates/iris-agentic-dev-core/src/`
- Tests: `crates/iris-agentic-dev-core/tests/{unit,binary,integration}/`
- Gates: `scripts/gates/`

## Before starting

```bash
docker ps --filter name=iris-dev-iris     # required for every Layer 3 task
cargo build                               # required for every Layer 2 task
```

`iris-dev-iris` (TCP 11975, web 52780) is the only container this feature may touch. Run all
integration and binary tests with `--test-threads=1`.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: the module tree and test scaffolding every batch depends on.

- [x] T001 Create the params module tree at `crates/iris-agentic-dev-core/src/tools/params/mod.rs` (one submodule per batch: `batch2`…`batch7`) and declare `pub mod params;` in `crates/iris-agentic-dev-core/src/tools/mod.rs`
- [x] T002 Add `StringOrI64` to `crates/iris-agentic-dev-core/src/tools/params/mod.rs`, test first: write the failing unit test in `crates/iris-agentic-dev-core/tests/unit/test_params_schema.rs` that parses `{"since_id": 12345}` and `{"since_id": "12345"}` from JSON strings into the same `i64` and asserts the generated schema declares `"type": ["integer", "string"]`, then implement with `#[serde(untagged)]` plus `#[schemars(extend("type" = ["integer", "string"]))]`
- [x] T003 [P] Add the shared Layer 2 helper `advertised_schemas()` to `crates/iris-agentic-dev-core/tests/binary/schema_census.rs`: spawn the binary via `testing::clean_mcp_command(&require_iad_binary())`, send `initialize` + `tools/list`, assert `nextCursor` is null and the tool count is ≥81, and return a `BTreeMap<String, Value>` of name → `inputSchema`. Also add `resolve_property(schema, name)` which follows `anyOf[0]` when present and panics if the property is absent (Principle XI: never return `None` silently)
- [x] T004 [P] Add the shared source-extraction helper `read_keys(tool_name)` to `crates/iris-agentic-dev-core/tests/unit/handler_keys.rs`, returning a `BTreeSet<String>` from whichever source is authoritative for that tool's current state: if the `#[tool]` signature is `Parameters<AnyParams>`, every `p.get("…")` literal in the handler body; otherwise the field names of the struct named in `Parameters<T>`, parsed from its `pub struct T` block and honoring `#[serde(rename = "…")]`. Panic if neither source resolves — a tool the helper cannot classify must fail the test, not return an empty set. Assert non-empty for all 81 tools, which requires the typed-struct branch to work today for the existing 50
- [x] T004a [P] Add `handler_uses_field(tool_name, field)` to the same file: assert the handler body textually references the field, as `p.get("<field>")` for an unconverted tool or as `.<field>` for a converted one. This is the check that survives conversion — once a tool is typed, `read_keys` and the advertised schema both derive from the same struct, so only this assertion still proves the handler reads what it advertises

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: close the no-argument surface and stand up the progress metric every batch moves.
No conversion batch may start until these pass.

- [x] T005 Close `NoParams` in `crates/iris-agentic-dev-core/src/tools/mod.rs` (~line 937), test first: write the failing unit test in `tests/unit/test_params_schema.rs` asserting the generated schema has `"properties": {}` present as a key and `"additionalProperties": false`, then add `#[serde(deny_unknown_fields)]`
- [x] T006 Give `iris_import_servers` (~`mod.rs:8271`) and `iris_reload_pool` (~`mod.rs:8044`) a `Parameters<NoParams>` argument, test first: write the failing Layer 2 test in `tests/binary/schema_census.rs` asserting each advertises a `properties` key (empty object) and `additionalProperties: false`
- [x] T007 Add the progress-bar test to `tests/binary/schema_census.rs`: count tools whose `properties` map is empty while `read_keys` returns a non-empty set, and assert the count is at or below a constant `REMAINING_UNDECLARED` (initially 31). Each batch lowers the constant; US4 turns it into a hard zero

**Phase gate**: `cargo test --test '*' -- --test-threads=1 --include-ignored` passes and the census test reports exactly 31 remaining.

---

## Phase 3: User Story 1 — An agent can discover a tool's parameters without reading prose (P1)

**Goal**: all 81 tools advertise the parameter names, types, and optionality their handlers read.

**Independent test**: `advertised_schemas()` shows no tool with an empty `properties` map while
its handler reads keys, and per tool the advertised set equals `read_keys(tool)`.
Batch 1 alone satisfies this test for its four tools, so the story is deliverable incrementally.

Every batch follows the same two-task shape. The test task asserts, per tool in the batch:
advertised property names equal both `read_keys` and the tool's row in data-model.md's frozen
inventory, `handler_uses_field` holds for every field, each property declares a type matching
data-model.md's non-string table, `additionalProperties` is `false`, and `required` contains only
unconditionally required names (FR-007). The live task exercises each advertised parameter
through a real `tools/call` against `iris-dev-iris` and asserts the response is unchanged from
the pre-conversion baseline (FR-004).

### Batch 1 — the four tools whose structs already exist (wiring only)

- [x] T008 [US1] Write the failing Layer 2 + live tests for `compare_document`, `compare_namespace`, `global_preview`, `global_kill` in `tests/binary/schema_batch1.rs` and `tests/integration/params_batch1.rs`. `global_preview.count` must be `integer`; `global_kill` must keep `confirm_token` optional. The live test for `global_kill` sets `IRIS_WRITE_TOOLS_ENABLED=1 IRIS_DESTRUCTIVE_TOOLS_ENABLED=1` itself (Principle XII)
- [x] T009 [US1] Add `src/tools/params/batch1.rs` with four new wire-contract structs, wire the `#[tool]` signatures in `src/tools/mod.rs`, add `#[serde(deny_unknown_fields)]` to each, and lower `REMAINING_UNDECLARED` to 27. **Corrected during implementation:** this task originally said to wire the existing `CompareDocumentParams`/`CompareNamespaceParams` (`src/tools/comparison_tools.rs`) and `GlobalPreviewParams`/`GlobalKillParams` (`src/tools/admin_tools.rs`). Those four are argument bundles for the `_impl` functions and carry `Arc<IrisConnection>` and `Arc<reqwest::Client>`, so they can derive neither `Deserialize` nor `JsonSchema` and cannot appear in a `#[tool]` signature. Batch 1 therefore needs new structs like every other batch; only the `_impl` call sites keep using the existing ones

**Phase gate**: batch 1's Layer 2 and live tests pass. This is the pattern every later batch copies.

### Batch 2 — simple, mostly `server`

- [x] T010 [US1] Write the failing Layer 2 + live tests for `iris_namespace_list`, `iris_namespace_create`, `iris_database_list`, `iris_database_stats`, `iris_containers` in `tests/binary/schema_batch2.rs` and `tests/integration/params_batch2.rs`. **Corrected during implementation:** nine slots, not twelve — this task assumed all five tools take `server`, and `iris_containers` does not (it selects a Docker container, not a registered instance). The nine: `server` ×4, `name` ×2 (`iris_namespace_create` and `iris_containers`), `db_path`, `db`, `action`
- [x] T011 [US1] Add `src/tools/params/batch2.rs` with the five structs, wire the `#[tool]` signatures, and lower `REMAINING_UNDECLARED` to 22

### Batch 3 — `iris_admin`, 26 keys, action-dependent

- [x] T012 [US1] Write the failing Layer 2 + live tests for `iris_admin` in `tests/binary/schema_batch3.rs` and `tests/integration/params_batch3.rs`: all 26 keys from data-model.md advertised, `action` the only entry in `required` (FR-007), `confirm`/`enabled` `boolean`, `primary_port`/`async_member_type`/`max_records` `integer`, and one live call per non-destructive action proving its keys are still honored

  **Notes from implementation:** `ParamContract` grew a `required` field so the advertised `required`
  array is compared as a set, not just asserted empty — tightening `required` anywhere now fails a
  test. Three key groups are not live-proven and the test file says why: `journal_search` is refused
  by the bulk-PHI gate before the handler reads anything, and with `dataPolicy = "allow"` it scans
  the whole journal file (no answer in 25 s); `mirror_add_async` would reconfigure mirroring on the
  dev instance; `new_password` changes the account the suite authenticates with. The type-rejection
  test covers their non-string keys. Wrong types are now refused, but serde's message names the
  expected type, not the parameter — T032 adds the parameter name and the `error_code`, and the test
  comment points at it.

- [x] T013 [US1] Add `src/tools/params/batch3.rs` with `IrisAdminParams` — every field `Option<_>` except `action`, per research decision 4 — wire the signature, and lower `REMAINING_UNDECLARED` to 21

  **Notes from implementation:** `time_range` became a typed `JournalTimeRange { from, to }` rather
  than an untyped object, and `#[schemars(inline)]` had to go on that struct, not on the field —
  it is a container attribute in schemars_derive 1.2, and as a field attribute it fails to compile.

### Batch 4 — system and audit tools

- [x] T014 [US1] Write the failing Layer 2 + live tests for `iris_system_performance`, `iris_mirror_status`, `journal_search`, `query_audit_log`, `my_access`, `capability_matrix` in `tests/binary/schema_batch4.rs` and `tests/integration/params_batch4.rs`. `journal_search.max_entries` and `query_audit_log.limit` must be `integer`

  **Notes from implementation:** writing these tests found two runtime defects in `journal_search`,
  both recorded in `specs/113-typed-tool-schemas/parameter-audit.md` (F1, F2) and both out of this
  feature's scope: `start`/`end` compare with ObjectScript's _numeric_ `<`, so inside one calendar
  year they filter nothing; and the scan skips with `Continue`, which never advances the cursor, so
  any filtered-out record spins an IRIS job until the 90-second HTTP timeout. That is why the three
  journal filters have no discriminating live assertion — every candidate has to send a value that
  skips a record. `max_entries` and `server` are proven live; a live ObjectScript test pins the
  comparison semantics F1 rests on. Third finding (F3): `profile` is _not_ a closed set despite docs
  and error text listing six names, so a test now asserts it advertises no enum before US2 adds one.

- [x] T015 [US1] Add `src/tools/params/batch4.rs` with the six structs, wire the signatures, and lower `REMAINING_UNDECLARED` to 15

### Batch 5 — interoperability

- [x] T016 [US1] Write the failing Layer 2 + live tests for `iris_interop_query`, `iris_production_item`, `iris_production_diff`, `iris_message_body`, `iris_business_rule_info` in `tests/binary/schema_batch5.rs` and `tests/integration/params_batch5.rs`. Assert `session_id`/`since_id` advertise `["integer", "string"]` and that the live test passes each **both** ways (`12345` and `"12345"`) with identical results — the FR-008 regression guard. `search_table` and `settings` are `object`; `limit`/`max_bytes` are `integer`
- [x] T017 [US1] Add `src/tools/params/batch5.rs` with the five structs, using `StringOrI64` for `session_id`/`since_id`, wire the signatures, and lower `REMAINING_UNDECLARED` to 10

### Batch 6 — credentials and lookup tables (write-gated)

**T016/T017 notes.** Three things came out of this batch that are not visible in the diff.
(1) `#[schemars(inline)]` had to go on `StringOrI64` and on the new `SearchTableParam`, or schemars
emits `{"$ref": "#/$defs/..."}` for `session_id`, `since_id` and `search_table` — a client that does
not resolve `$defs` would have seen three parameters with no type and no shape, which is the same
blindness this feature exists to remove. (2) `#[serde(deny_unknown_fields)]` already rejects a
misspelled parameter at the rmcp deserialization boundary, before US4's validation site exists — so
that site's job is narrower than the plan assumed: replace serde's `unknown field ...` with a
message that names the tool, not create the refusal. `an_unknown_parameter_is_refused_by_serde_until_us4_names_it`
pins the current text. (3) Three `iris_admin` payloads in `tests/integration/test_handlers_live.rs`
sent `"roles": []`; the handler has always read `roles` with `as_str()`, so the array was discarded.
Declaring the type turned that into a hard error and the payloads now send `"%All"` — recorded as F5.
`iris_message_body.namespace`/`max_bytes`/`acknowledgePhi`/`dataPolicy` and
`iris_production_item.item`/`settings` have no discriminating live assertion; the module doc of
`tests/integration/params_batch5.rs` says why for each.

- [x] T018 [US1] Write the failing Layer 2 + live tests for `iris_credential_list`, `iris_credential_manage`, `iris_lookup_manage`, `iris_lookup_transfer` in `tests/binary/schema_batch6.rs` and `tests/integration/params_batch6.rs`. The live tests set `IRIS_WRITE_TOOLS_ENABLED=1` themselves; add one test per write-gated tool asserting the gate refusal still arrives with writes disabled and typed params in place
- [x] T019 [US1] Add `src/tools/params/batch6.rs` with the four structs, wire the signatures, and lower `REMAINING_UNDECLARED` to 6

**T018 notes.** Two findings, both now in `parameter-audit.md`. F6: none of the four reads `server`,
because their handlers call `self.iris_arc()` instead of resolving from the pool — so batch 6
declares no `server` property and a test pins the absence rather than papering over it. F7: with the
destructive tier closed, a misspelled `iris_lookup_manage` action answers
`DESTRUCTIVE_TOOLS_DISABLED`, not `INVALID_ACTION`, so the live test opens a second session for the
one assertion that needs the dispatcher. Deleting a lookup table's last entry also removes the table,
so the post-delete miss is `TABLE_NOT_FOUND` rather than `KEY_NOT_FOUND`.

### Batch 7 — schema, diagram, and stream tools

- [x] T020 [US1] Write the failing Layer 2 + live tests for `hl7_schema_list`, `hl7_schema_inspect`, `mermaid_class`, `mermaid_production`, `resolve_storage`, `stream_inspect` in `tests/binary/schema_batch7.rs` and `tests/integration/params_batch7.rs`. `mermaid_class.depth` is `integer`. Assert `stream_inspect` advertises exactly `namespace`, `oid`, `server` and **not** `max_chars` — the docs-side resolution is T029
- [x] T021 [US1] Add `src/tools/params/batch7.rs` with the six structs, wire the signatures, and lower `REMAINING_UNDECLARED` to 0

### Batch 8 — the already-typed 50

- [x] T022 [US1] Write the failing Layer 2 suite-wide assertions in `tests/binary/schema_census.rs`: (a) `additionalProperties: false` on **all 81** tools, counted, with the offending tool names listed in the failure message; (b) every tool advertising `server` emits an identical type, optionality, and `description` string — the spec assumes one phrasing rather than 31 and nothing else checks it; (c) the converted 31 advertise at least 132 property slots over at least 71 distinct names, so SC-002's total is asserted as a total and not only per batch
- [x] T023 [US1] Add `#[serde(deny_unknown_fields)]` to every existing params struct across `src/tools/` (50 tools; grep for `#[derive(` lines carrying `JsonSchema` in the crate), then run `cargo test --test '*' -- --test-threads=1 --include-ignored` and fix the `call_for_test` callers that break — per research decision 8, existing tests passing stray keys now fail, and that is the detector working

**Phase gate (US1 complete)**: `REMAINING_UNDECLARED` is 0, all 81 tools emit
`additionalProperties: false`, and every batch's live round-trips pass. US1's acceptance
scenarios 1–4 hold; scenario 5 (misspelling rejected) is delivered in US4.

---

## Phase 4: User Story 2 — Fixed value sets are stated, not described (P2)

**Goal**: every closed-value-set parameter advertises its permitted values as `enum`.

**Independent test**: for each of the 17 parameters, the advertised `enum` (resolved through
`anyOf[0]`) equals the literals the handler branches on, extracted from `mod.rs` rather than
transcribed. Comparing against description prose does not satisfy this test.

- [x] T024 [P] [US2] Write the failing test in `tests/unit/test_enum_contract.rs`: add `handler_match_arms(tool, param)` extracting the string literals a handler matches on for that parameter from `src/tools/mod.rs`, assert the extracted set is non-empty for each of the 17 parameters, and compare it to `resolve_property(schema, param)["enum"]` from `advertised_schemas()`. Assert the `enum` array is present and non-empty **before** comparing — after `normalize_schema_openapi3` an optional parameter's enum lives at `anyOf[0].enum`, so a "compare if present" form passes vacuously forever
- [x] T025 [US2] Declare all 17 enums with `#[schemars(extend("enum" = [...]))]`, taking every value list from the handler's match arms rather than the tool description. Nine on converted tools in `src/tools/params/batch*.rs`: `iris_system_performance.mode` (start/status/last_runid), `iris_interop_query.what`, and `action` on `iris_containers`, `iris_production_item`, `iris_business_rule_info`, `iris_credential_manage`, `iris_lookup_manage`, `iris_lookup_transfer`, `iris_admin`. Eight on already-typed tools: `iris_query.mode` (read/explain/count/write), `iris_coverage.mode` (check/report/run/start/stop), `iris_doc.category` (ALL/CLS/INC/INT/MAC), `iris_doc.compiled_type` (INT/OBJ), `iris_doc.elicitation_answer` (yes/no), `iris_test.test_type` (auto/testcase/testproduction), `iris_generate.gen_type` (class/test), `iris_add_server.scheme` (http/https)
- [x] T026 [US2] Add the live test in `tests/integration/enum_rejection.rs` asserting a value outside the set returns `{success: false}` naming the parameter and listing the permitted values (FR-006), e.g. `iris_system_performance {"mode": "bogus"}` → `unknown mode …; valid values: start, status, last_runid`, and `iris_admin {"action": "bogus"}` → `error_code: "INVALID_ACTION"` per `contracts/rejection-response.md`. Handler validation is deliberately unchanged (research decision 2); this test pins that it still produces the good error

**Phase gate**: T024 passes with all 17 comparisons non-vacuous, and T026's live rejection test passes.

---

## Phase 5: User Story 3 — A documented parameter that nothing reads cannot survive (P2)

**Goal**: zero disagreements among documented, advertised, and read parameters across all 81 tools.

**Independent test**: `parameter-audit.md`'s Findings section lists every disagreement found with
its resolution and which source was wrong, and the docs-contract test passes with no fallback.

- [x] T027 [US3] Write the failing three-way audit test in `tests/unit/test_docs_contract.rs`: for all 81 tools compare `docs/tools.md`'s parameter table, the advertised properties, and `read_keys`, plus the frozen pre-conversion inventory table in `specs/113-typed-tool-schemas/data-model.md` as a fourth column, failing with every disagreement listed in both directions. Assert the comparison covered all 81 tools, so a table-parsing miss cannot read as agreement. For every converted tool also assert `handler_uses_field` for each advertised field, so a struct field nobody reads fails the audit rather than passing because the schema and the read set share a source
- [x] T028 [US3] Resolve every disagreement T027 reports — implement the parameter or remove the promise from `docs/tools.md` — and record each row in `specs/113-typed-tool-schemas/parameter-audit.md` (tool, parameter, documented?, advertised?, read?, resolution, which source was wrong), marking every batch "audited: yes". Include the `stream_inspect`/`max_chars` regression assertion: absent from docs, schema, and handler, checked in all three places so the bug cannot return under a new name. Run `markdownlint-cli2 --fix` and `prettier --write` on any `docs/*.md` touched

**Phase gate**: T027 reports zero disagreements and `parameter-audit.md` has no unresolved rows.

---

## Phase 6: User Story 4 — The next tool cannot reintroduce the gap (P3)

**Goal**: unknown parameters are rejected legibly, `AnyParams` no longer exists, and both the
suite and the gates fail on a reintroduction.

**Independent test**: add a tool advertising an open parameter set on a scratch branch, confirm
the suite and `scripts/gates/antipatterns.py` both fail; remove it and confirm both pass.

**Ordering**: this phase must come after batch 8. A tool advertising no properties would have
every argument rejected by the validation site, so enabling it earlier breaks the tools it protects.

- [x] T029 [US4] Write the failing tests for the rejection response per `contracts/rejection-response.md`: a unit test over the message builder (names the offending key quoted as the caller spelled it, lists accepted names sorted, reports **all** unknown keys not just the first, suggests a near match at edit distance 1 and omits the sentence otherwise), plus a Layer 2 test asserting `iris_query {"query":"SELECT 1","namesapce":"USER"}` returns `{"success": false, "error_code": "UNKNOWN_PARAMETER", …}` — the exact call that returns `success: true` today
- [x] T030 [US4] Implement the validation site in the `call_tool` override in `src/tools/mod.rs` (~line 9098, after `write_gate::gate_check` and before `tool_router.call`): build a cached `name → BTreeSet<String>` map from `ToolRouter::list_all()`'s `input_schema` properties, diff the incoming argument keys against it, and return the refusal via `err_result` with `error_code: "UNKNOWN_PARAMETER"`. No literal tool-name list, no per-handler check (Principle XIII)
- [x] T031 [US4] Add the live ordering tests in `tests/integration/rejection_order.rs`: call `global_kill` with destructive tools disabled **and** a bogus key, asserting the gate's code comes back (`DESTRUCTIVE_TOOLS_DISABLED` / `DESTRUCTIVE_REQUIRES_WRITES`), not `UNKNOWN_PARAMETER`; and a second test proving a rejected call did no work — the target global is byte-identical after the rejection (FR-016)
- [x] T032 [US4] Normalize the type-error path: catch the router's deserialization failure in `call_tool` and re-emit it as `{"success": false, "error_code": "INVALID_PARAMS", "error": …}` preserving serde's original text, with Layer 2 tests on `iris_query {"query": 42}` — which today returns bare `isError` text with no `error_code` (FR-017) — and on `query_audit_log {"limit": "100"}`, an integer-only parameter that today silently discards the string and answers with the default. That second assertion is the only place the one intentional per-parameter behavior change from research decision 3 is pinned
- [x] T033 [US4] Remove every escape hatch in one commit. Delete `AnyParams` (`src/tools/mod.rs:73-91`) and the `dispatch_any!` macro in `call_for_test` (~line 10990) — the compiler is now the detector for this class. Change `REMAINING_UNDECLARED` to a hard `assert_eq!(count, 0)` in `tests/binary/schema_census.rs`. In `tests/unit/test_docs_contract.rs`, delete the `None => match handler_body(tool)` fallback (~line 894) together with the `by_handler > 0` tail assertion — that assertion requires the fallback to fire, so it fails the moment the last tool is converted — then delete `handler_body` (~line 990), keep the `iris_admin` per-action table extractor (~line 1290), and add an assertion that the fallback is gone so it cannot return as a permanent exemption (FR-012)
- [x] T034 [US4] Add two rules to `scripts/gates/antipatterns.py`. `undeclared-params` flags a params struct without `#[serde(deny_unknown_fields)]`, any `Parameters<serde_json::Value>`, and any `#[serde(flatten)]` into a `Value` or `Map` — that third shape satisfies the first two while reopening the surface. `prose-only-enum` flags a tool description carrying a pipe- or comma-separated value list for a parameter whose advertised property has no `enum`, which is what makes SC-004's "count reaches zero" measurable rather than assumed for the 17 known cases. Each rule gets a canary test firing on a synthetic sample, and `scripts/gates/antipatterns-baseline.txt` gains no line for either class (never baselined). Add the matching canary for the cargo half of SC-006: a test proving the census assertion in `tests/binary/schema_census.rs` fails on a fixture tool that reads a key it does not advertise, so "two failures" is demonstrated rather than asserted
- [x] T035 [US4] Amend `.specify/memory/constitution.md` via `/speckit.constitution` — the file is edit-protected, and the Amendment Procedure requires the updated constitution in the same PR as the code change that motivated it, which is T034. Append both bug classes to the Bug Class Registry: "Undeclared parameter" (detector: the compiler once `AnyParams` is gone, plus the zero-empty-properties census test) and "Silently discarded param" (detector: `undeclared-params`). Add `undeclared-params` and `prose-only-enum` to the never-baselined list. Record the Principle VIII / Release Discipline coverage conflict as an open item with a decision, not a note: Principle VIII says ≥ 90%, Release Discipline says ≥ 88%, and `cargo llvm-cov` measures 87.68%, so no plan can satisfy both MUSTs. The enforceable number is 88% (it is what `scripts/coverage.sh` and CI check), and aligning Principle VIII to it weakens a non-negotiable constraint — a MAJOR bump under the Versioning Policy, out of scope for this amendment. Write the open item with the measured number, the two conflicting clauses, the owner (Tom), and the note that resolution requires its own `/speckit.constitution` run at 2.0.0. This amendment is PATCH to 1.5.2 and adds registry entries only

**Phase gate**: `python3 scripts/gates/antipatterns.py` is clean, the docs-contract test passes with no fallback, the rejection tests (unit, Layer 2, live) all pass, and the constitution carries both registry rows.

---

## Phase 7: Release Gate — behavioral evidence

**Not optional, not deferrable, and ahead of Polish by design.** Schema changes alter what every
agent sees across 38% of the surface, and calls that previously "succeeded" now fail. This phase
gates the merge.

- [x] T036 Run the GEPA eval harness against the converted surface and record the result in `specs/113-typed-tool-schemas/lift-results.md` as a **regression check, not a lift claim** (research decision 10). A correct schema that degrades task success is a release blocker
- [x] T036a Add description suppression to the eval harness: a `--suppress-tool-description <tool>` flag (or equivalent config field) that replaces the named tool's `description` with an empty string in the `tools/list` the harness serves, leaving the schema intact. Unit-test the flag against the emitted listing before using it
- [x] T036b Inventory `crates/iris-agentic-dev-core/src/benchmark/tasks/` against the 31 converted tools and write a minimal one-call task for each tool that has none — a single realistic invocation with a checkable result, not a scenario
- [x] T037 Prove SC-007 with the same harness: for each of the 31 converted tools, run its task with `--suppress-tool-description` so only the advertised schema is visible, and assert the agent's first call is accepted. Record pass/fail per tool in `lift-results.md`, and list any tool still lacking a task as an explicit gap against SC-007 rather than a pass. This is the only test that measures whether the schemas are sufficient rather than merely present

**Phase gate**: no regression against the pre-change baseline, and T037 passes for all 31.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [x] T038 [P] Update `docs/tools.md` for every newly advertised parameter and every enum now structural, and add to `docs/troubleshooting.md` what `UNKNOWN_PARAMETER` and `INVALID_ACTION` responses mean and how to fix the call. Measure `tools/list` payload growth before and after (the probe in `quickstart.md` prints it), record it under `research.md`'s Open items, and trim per-property `description` lines where the parameter name already says it — schema bytes are context every session pays for. Run `markdownlint-cli2 --fix` and `prettier --write` on every `.md` touched
- [x] T039 [P] Run `cargo fmt --all`, `cargo clippy -- -D warnings`, then `cargo llvm-cov clean && ./scripts/coverage.sh` and `./scripts/check-coverage-floors.sh`. TOTAL line coverage must be ≥ 88% with no per-file floor regression; the new `params/` modules and the validation site are the two areas at risk

---

## Dependencies

```text
Phase 1 (T001–T004a) ─┐
Phase 2 (T005–T007)  ─┴─→ US1 batches 1–8 (T008–T023)
                              │
                              ├─→ US2 enums (T024–T026)      # needs the structs to attach to
                              ├─→ US3 audit  (T027–T028)     # needs the advertised set to compare
                              └─→ US4 guards (T029–T035)     # MUST be last: needs all 81 declared
                                        │
                                        └─→ Release gate (T036–T037b) ─→ Polish (T038–T039)
```

- **Within a batch**: the test task blocks its implementation task (test-first).
- **Between batches**: independent except for `REMAINING_UNDECLARED`, which every batch edits — so a batch's two tasks are sequential, and batches should land one at a time to avoid contending on that constant and on `mod.rs`.
- **US2 per tool** depends only on that tool's batch, so enum work can start on batch 4's tools while batch 5 is in flight.
- **US4 depends on all of US1**, including batch 8. T030 turned on early would reject every argument to any still-unconverted tool.
- **T033 is deliberately one commit**: the `by_handler > 0` assertion requires the fallback to fire, so it fails the moment the last tool is converted. Splitting it leaves a red suite in between.
- **T035 ships with T034**, not after it — the constitution amendment and the detector it registers belong in the same PR.

## Parallel execution examples

```bash
# Phase 1: three independent files
T002 (tests/unit/test_params_schema.rs + src/tools/params/mod.rs)
T003 (tests/binary/schema_census.rs)
T004 + T004a (tests/unit/handler_keys.rs)

# Polish: different files
T038 (docs/ + research.md), T039 (no source edits)
```

Batch test tasks (T008, T010, T012, T014, T016, T018, T020) are `[P]`-eligible against one
another — separate test files — but each is blocked by its own implementation task, so writing
them in parallel only helps if all seven are drafted before any struct is added.

## Implementation strategy

**MVP**: Phase 1 + Phase 2 + Batch 1 (T001–T009). Ten tasks, four tools converted, and US1's
independent test passes for them. It proves the pattern — the struct shape, the Layer 2 assertion
through `normalize_schema_openapi3`, the live round-trip — at the lowest risk, because those four
structs already exist and only need wiring.

**Then**: batches 2–7 in order, each a shippable increment that lowers `REMAINING_UNDECLARED`.
Batch 3 (`iris_admin`) stays isolated because its 26 action-dependent keys are the one genuinely
hard shape; do not bundle it with a neighbour.

**Last**: batch 8 and US4 together. Batch 8 is what makes FR-002 and SC-004 reachable
(`additionalProperties: false` is on 0 of 81 tools today, not 31), and US4's validation site
cannot be turned on until every tool declares its surface.

**Do not ship partway through US4.** Between T033 and T034 the suite has no detector for a
reintroduced open surface: `AnyParams` is gone from the compiler's view but the scanner rules are
not in place yet.

## Total: 42 tasks

| Phase            | Tasks               | Count |
| ---------------- | ------------------- | ----- |
| 1 — Setup        | T001–T004a          | 5     |
| 2 — Foundational | T005–T007           | 3     |
| 3 — US1 (P1)     | T008–T023           | 16    |
| 4 — US2 (P2)     | T024–T026           | 3     |
| 5 — US3 (P2)     | T027–T028           | 2     |
| 6 — US4 (P3)     | T029–T035           | 7     |
| 7 — Release gate | T036, T036a–b, T037 | 4     |
| 8 — Polish       | T038–T039           | 2     |
