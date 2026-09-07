# Implementation Plan: Typed input schemas for every MCP tool

**Branch**: `113-typed-tool-schemas` | **Date**: 2026-09-06 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/113-typed-tool-schemas/spec.md`

## Summary

31 tools advertise `inputSchema: {"type": "object"}` with no properties because they take
`Parameters<AnyParams>`, a `serde_json::Value` newtype with a hand-written `JsonSchema` impl.
Give each one a `#[derive(Deserialize, JsonSchema)]` params struct so the schema declares its
real properties, types, optionality, and enums, then delete `AnyParams` — after which
`Parameters<AnyParams>` cannot compile, and the compiler becomes the detector.

Three findings from probing the shipped 1.3.2 binary reshaped the approach:

1. **Typed structs alone do not fix the silent-discard defect.** `iris_query` is already
   typed. Called with `{"query": "SELECT 1", "bogus_unknown_key": 123, "namesapce": "USER"}`
   it returned `success: true` and ran against the _default_ namespace — the misspelled
   `namesapce` was dropped without a word. schemars only emits `additionalProperties: false`
   under `#[serde(deny_unknown_fields)]`, which nothing in the tree uses. So FR-015 is a
   cross-cutting change over all 81 tools, not a side effect of converting 31.
2. **rmcp already rejects before the handler runs, but not in our response shape.**
   `{"query": 42}` returns `isError: true` with the bare text
   `failed to deserialize parameters: invalid type: integer 42, expected a string` — no
   `success`, no `error_code`. FR-016 is therefore satisfied structurally by the router, and
   FR-017 needs one normalization site rather than 31 hand-written checks.
3. **The advertised schema is not the generated schema.** `list_tools` runs
   `normalize_schema_openapi3` over every input schema, rewriting an optional field's nullable
   type array into `anyOf: [{…}, {"type": "null"}]` and relocating `enum` into the non-null
   branch. Tests must assert on the emitted listing, and an enum check that reads the property's
   top level finds nothing — a vacuous pass by default rather than by accident.

Approach: convert the 31 in seven batches grouped by module, add an eighth batch closing the
already-typed 50 (`additionalProperties: false` is absent from all 81 tools, and 8 closed-set
parameters there still live in prose), then flip on a single key-set validation site in
`call_tool` that answers FR-015/FR-016/FR-017 uniformly for every tool using property names read
from the router's own registry. Conversion must precede the flip — a tool advertising no
properties would otherwise have every argument rejected.

## Technical Context

**Language/Version**: Rust 2021, workspace pinned via `rust-toolchain`
**Primary Dependencies**: `rmcp` 3.1.3 (`server`, `macros`, `schemars`), `serde`,
`serde_json`, `schemars`. **No new dependency.** Unknown-key detection is a `BTreeSet`
difference against the advertised property names, not a JSON-Schema validator crate.
**Storage**: N/A
**Testing**: `cargo test` (unit), spawned-binary tests via `testing::clean_mcp_command`
(Layer 2), `#[ignore]` live-IRIS tests against `iris-dev-iris` (localhost:52780), all with
`--test-threads=1`
**Target Platform**: macOS arm64/x86_64, Linux x86_64, Windows x86_64 — unchanged
**Project Type**: single Rust workspace, two crates
**Performance Goals**: no measurable change to `tools/list` or per-call latency. The
validation site is one set difference over ≤15 keys per call.
**Constraints**: `tools/list` output grows — 31 tools gain full property maps. Schema size is
context an agent pays for on every session, so descriptions must not be duplicated into
per-property docs where the property name already says it. The emitted schema is post-processed
by `normalize_schema_openapi3`, so assertions target the listing rather than schemars output.
**Scale/Scope**: 81 tools, 31 converted, 132 parameter slots over 71 distinct names, 6 tools
that legitimately take none (`agent_stats`, `check_config`, `iris_import_servers`, `iris_reload_pool`,
`skill_community_list`, `skill_list`), 39 tasks with the conversion work in 8 batches.

## Constitution Check

_GATE: Must pass before Phase 0 research. Re-check after Phase 1 design._

| Principle                        | Status | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| -------------------------------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install Binary           | PASS   | No install step, no runtime, no IRIS-side import. No new crate — see VII.                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| II. ObjectScript Sanity          | N/A    | No ObjectScript is added or modified. Every handler body is untouched below the parameter-extraction lines.                                                                                                                                                                                                                                                                                                                                                                                                                     |
| III. HTTP-First Execution        | N/A    | No new tools, no transport change. Tier registration unchanged.                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| IV. Test-First, Fixture-Driven   | PASS   | Each batch writes its schema assertions before the struct exists, so the test fails first. No-IRIS coverage comes from Layer 2 spawn tests; live tests are `#[ignore]` with documented commands.                                                                                                                                                                                                                                                                                                                                |
| V. Output Shape Parity           | PASS   | Inputs only. The one new response — parameter rejection — follows `{success: false, error_code, error}` and is specified in `contracts/rejection-response.md`. rmcp's bare-text form is normalized.                                                                                                                                                                                                                                                                                                                             |
| VI. Environment Guard            | PASS   | `gate_check` runs on raw `request.arguments` before dispatch and stays there; a gate refusal must keep winning over a parameter rejection. Asserted, not assumed — see research decision 6.                                                                                                                                                                                                                                                                                                                                     |
| VII. Dependency Minimalism       | PASS   | Zero new crates. Rejected `jsonschema`/`valico`: a set difference over already-parsed property names is ~30 lines and needs no validator.                                                                                                                                                                                                                                                                                                                                                                                       |
| VIII. 90% Coverage Gate          | PASS   | Polish phase runs `scripts/coverage.sh` (with `cargo llvm-cov clean` first, per the `stale-coverage-objects` registry entry) and `scripts/check-coverage-floors.sh`, asserting TOTAL line coverage ≥ 88% with no per-file floor regression. The constitution says ≥ 90% here and ≥ 88% under Release Discipline while the tree measures 87.68%; 88% is the enforceable number and the conflict is filed for amendment.                                                                                                          |
| IX. Tool Lift Requirement        | N/A    | No new tool ships. Not claimed as free, though: schema changes alter agent behavior, so the GEPA eval-harness regression run and the SC-007 schema-only exercise get their own labeled release-gate phase ahead of Polish, per the placement rule this principle sets — see research decision 10.                                                                                                                                                                                                                               |
| X. ObjectScript Coverage         | N/A    | Pure Rust feature.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| XI. No Vacuous Tests             | PASS   | Three named hazards. (1) A loop over `properties` that passes on an empty map. (2) An enum check that finds nothing because `list_tools` moved the enum into `anyOf[0]`. (3) The FR-003 invariant itself: once a tool is typed, its advertised schema and its read set both derive from the same struct, so the comparison is tautological. Fixed by anchoring the comparison to data-model.md's frozen pre-conversion inventory and to `handler_uses_field`, which reads the handler body. All three asserted non-empty first. |
| XII. Hermetic Test Environment   | PASS   | Spawn tests go through `clean_mcp_command`. `IRIS_ADMIN_TOOLS=1` gates admin _write actions_, not listing visibility, so schema tests need no env var; live admin-write tests set it themselves.                                                                                                                                                                                                                                                                                                                                |
| XIII. Single-Source Failure Dete | PASS   | One validation site in `call_tool`; property names read from the router's tool list, never a literal copy. No handler-local unknown-key checks.                                                                                                                                                                                                                                                                                                                                                                                 |

_A plan with any FAIL gate MUST NOT proceed to implementation._

**Post-design re-check (after Phase 1).** No status changed, and the table above is already
written against the design rather than ahead of it — rows V, VI, VII, XI, XII, and XIII were each
rewritten during Phase 1 as the probes landed. Three findings from the design work are what keep
those rows at PASS rather than at risk:

- `list_tools` normalizes every schema through `normalize_schema_openapi3`, which relocates an
  optional parameter's `enum` into `anyOf[0]`. Left undiscovered, the natural enum test would
  have passed vacuously forever — XI would have been a paper PASS.
- rmcp already refuses a mis-typed parameter before the handler, but as bare text with no
  `error_code`. FR-016 is therefore structural and free; only the shape needs work, which is what
  keeps V a real PASS with one normalization site rather than 31.
- `additionalProperties: false` is absent from all 81 tools, not 31, so batch 8 was added instead
  of splitting the spec. That is the only Complexity Tracking entry this design introduces.

Zero FAIL gates. `/speckit.tasks` may proceed.

### Bug Class Registry entry (Clarification Prompt 9)

| Class                    | First shipped instance                                                                                                                                                                                                                                                                | Detector                                                                                                                                                               |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Undeclared parameter     | `stream_inspect` documented `max_chars`; no code read it, so a caller asking for 10 000 characters got the whole stream. Second instance: the `iris_admin` per-action tables documented `type_filter`/`namespace_filter`/`name_filter` for actions reading `type`/`namespace`/`name`. | Deleting `AnyParams` makes the compiler the detector; the cargo-side rule is the zero-empty-properties test.                                                           |
| Silently discarded param | `iris_query` accepted `namesapce` and answered from the wrong namespace with `success: true`.                                                                                                                                                                                         | `undeclared-params` in `scripts/gates/antipatterns.py` — flags a params struct without `deny_unknown_fields` and any `Parameters<serde_json::Value>`. Never baselined. |

The registry rule "where cargo can express a rule, the rule belongs in a cargo test" applies:
the count assertion is a cargo test, and the scanner covers only what cargo cannot see — a
future struct that re-opens the surface via `deny_unknown_fields` omission, a raw `Value`, or a
`#[serde(flatten)]` that satisfies both rules while flattening an open map back in. A second
scanner rule, `prose-only-enum`, flags a description carrying a value list for a parameter whose
property has no `enum`, which is what makes SC-004's "count reaches zero" measurable rather
than assumed.

Both rows and both never-baselined detectors belong in the constitution's own Bug Class
Registry, not only here. The Amendment Procedure requires the updated constitution in the same
PR as the code change that motivated it, so the amendment is a task in Phase 6 alongside the
detector rather than a follow-up.

## Project Structure

### Documentation (this feature)

```text
specs/113-typed-tool-schemas/
├── plan.md              # This file
├── research.md          # Phase 0 — 12 decisions, each with the evidence
├── data-model.md        # Phase 1 — entities, the verified 31-tool inventory, error codes
├── quickstart.md        # Phase 1 — test layers, and how to reproduce every measurement
├── parameter-audit.md   # FR-009 disagreement list, filled in as each batch lands
├── contracts/
│   ├── parameter-contract.md    # What a conforming inputSchema must contain
│   └── rejection-response.md    # The FR-015/016/017 response shape
├── checklists/
│   └── requirements.md  # Spec quality checklist (complete)
└── tasks.md             # Phase 2 output (/speckit.tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/iris-agentic-dev-core/
├── src/
│   ├── tools/
│   │   ├── mod.rs                  # AnyParams (l.73-91, deleted); 31 #[tool] signatures;
│   │   │                           # NoParams (l.937); call_for_test dispatch_any! (l.10990);
│   │   │                           # call_tool override (l.9077) — validation site
│   │   ├── params/                 # NEW — 27 structs in batch2.rs…batch7.rs (batch 1's
│   │   │                           # four already exist; batch 8 adds none)
│   │   ├── admin_tools.rs          # GlobalPreviewParams, GlobalKillParams already here
│   │   ├── comparison_tools.rs     # CompareDocumentParams, CompareNamespaceParams already here
│   │   ├── interop.rs
│   │   └── write_gate.rs           # gate_check — ordering vs validation asserted, not changed
│   └── testing.rs                  # clean_mcp_command; BEHAVIOR_ENV_VARS registry
└── tests/
    ├── unit/test_docs_contract.rs  # l.894 fallback removed; l.990 handler_body retired;
    │                               # l.1290 iris_admin per-action tables
    ├── unit/                       # NEW schema-declaration + zero-count tests
    ├── binary/                     # NEW Layer 2 per-batch tools/list assertions
    └── integration/                # NEW live per-batch round-trips + failure paths

scripts/gates/
├── antipatterns.py                 # NEW undeclared-params detector
└── antipatterns-baseline.txt       # must NOT gain a line for this class
```

**Structure Decision**: The 27 new structs go in a new `src/tools/params/` module tree, one
file per conversion batch, rather than into `mod.rs` — which is already ~11 000 lines and is
the file every batch would otherwise contend on. The four structs that already exist stay
where they are; moving them would inflate the diff for no gain. `#[tool]` signatures stay in
`mod.rs` because the `#[tool_router]` macro requires them in the impl block.

### Task decomposition rule (Clarification Prompt 3: ≤40 tasks)

31 tools × (struct + Layer 2 test + live test) is well over 40 tasks one-per-tool, and the
constitution says split the spec at that point. Splitting is the wrong cut here — the guards
in US4 only become assertable once the last tool is converted, so a split would leave one
half unable to close. Batching by module keeps one spec and lands 39 tasks:

| Batch | Tools                                                                                                                  | Note                       |
| ----- | ---------------------------------------------------------------------------------------------------------------------- | -------------------------- |
| 1     | `compare_document`, `compare_namespace`, `global_preview`, `global_kill`                                               | structs exist; wiring only |
| 2     | `iris_namespace_list`, `iris_namespace_create`, `iris_database_list`, `iris_database_stats`, `iris_containers`         | simple, mostly `server`    |
| 3     | `iris_admin`                                                                                                           | 26 keys, action-dependent  |
| 4     | `iris_system_performance`, `iris_mirror_status`, `journal_search`, `query_audit_log`, `my_access`, `capability_matrix` | two enums                  |
| 5     | `iris_interop_query`, `iris_production_item`, `iris_production_diff`, `iris_message_body`, `iris_business_rule_info`   | 15-key tool, two enums     |
| 6     | `iris_credential_list`, `iris_credential_manage`, `iris_lookup_manage`, `iris_lookup_transfer`                         | write-gated; three enums   |
| 7     | `hl7_schema_list`, `hl7_schema_inspect`, `mermaid_class`, `mermaid_production`, `resolve_storage`, `stream_inspect`    | `max_chars` lives here     |
| 8     | the already-typed 50: `deny_unknown_fields` everywhere, plus 8 missing enums on 6 tools                                | see below                  |

Batch 1 is the pattern-setting phase (US1's independent test runs against it alone). Batch 3
is isolated because `iris_admin`'s conditional requirements are the one genuinely hard shape.
`AnyParams` deletion, the validation site, and the guards come after batch 8.

Batch 8 exists because two probe findings are cross-cutting, not `AnyParams`-specific.
`additionalProperties: false` appears on **0 of 81** tools today, so FR-002 reaches every params
struct in the tree. And eight closed-set parameters on already-typed tools carry their values
only in prose — `iris_query.mode`, `iris_coverage.mode`, `iris_doc.category`,
`iris_doc.compiled_type`, `iris_doc.elicitation_answer`, `iris_test.test_type`,
`iris_generate.gen_type`, `iris_add_server.scheme`. SC-004 requires that count reach zero and
US2 was never scoped to the 31, so omitting them would ship a feature that fails its own success
criterion. See research decision 12.

## Complexity Tracking

> No Constitution Check gate is FAIL. Three items are recorded because they are decisions a
> reviewer would otherwise have to reverse-engineer.

| Item                                       | Why needed                                                                                                                                                                                     | Simpler alternative rejected because                                                                                                                                                                                                                                                        |
| ------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Centralized validation site in `call_tool` | FR-015 must hold for all 81 tools, including the 50 already typed — `iris_query` swallowing `namesapce` is the proof. One site keeps detection single-source (XIII) and needs no new crate.    | `#[serde(deny_unknown_fields)]` alone: it fixes the _schema_ (emits `additionalProperties: false`) but its runtime error arrives as rmcp's bare `failed to deserialize parameters: …` with no `error_code`. Both are needed — the attribute for the declaration, the site for the response. |
| Batching 31 tools into 7 task groups       | One-per-tool decomposition exceeds the 40-task cap that Clarification Prompt 3 sets as a split trigger.                                                                                        | Splitting into 113a/113b: the US4 guards cannot be asserted until every tool is converted, so the second spec would own the only gate that proves the first one worked.                                                                                                                     |
| Batch 8 touching the already-typed 50      | `additionalProperties: false` is on 0 of 81 tools and 8 closed-set params on typed tools live in prose. FR-002 and SC-004 are unmeetable without it — the defect was never confined to the 31. | Narrowing SC-004 to the converted tools: that is moving a success criterion to fit the batch plan, and it leaves `iris_query.mode` — one of the most-called parameters here — guessable from prose only.                                                                                    |
