# Tasks: CLI tool discovery and a detector that sees every value set

**Feature**: `114-tool-surface-discovery` | **Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)

Tests come first inside every phase. Each phase ends on a gate that must pass before the next
phase starts.

## Phase 1: Setup

- [x] T001 Capture the baseline payload, per-tool byte split, toolset tiers, CLI call latency, and the
      eleven undeclared value sets in `specs/114-tool-surface-discovery/research.md` — every later
      claim is measured against these numbers, so they are recorded before any code changes
- [x] T002 Verify `iris-dev-iris` is up (`docker ps --filter name=iris-dev-iris`) and build the
      binary so the spawn tests have something to spawn: `cargo build --features testing`

## Phase 2: Foundational — one catalogue, read off the router

- [x] T003 [P] Unit test in `crates/iris-agentic-dev-core/tests/unit/test_tool_catalogue.rs`: for every
      name in `registered_tool_names()`, `tool_catalogue()` has exactly one entry whose description
      equals `tool_description(name)` and whose schema equals `tool_input_schema(name)` put through
      the same `normalize_schema_openapi3` rewrite `list_tools` applies, and the
      catalogue has no entry for a name that is not registered. This is the XIII guard — one source,
      so the CLI cannot drift from what `tools/list` serves
- [x] T004 [P] Unit test in the same file for `summarize_description`: the first sentence is kept; a
      description longer than 100 characters is cut on a word boundary with no mid-word truncation;
      a description with no sentence break is cut at 100; an empty description yields an empty
      summary rather than a panic
- [x] T005 Implement `tool_catalogue()` and `summarize_description()` in
      `crates/iris-agentic-dev-core/src/tools/mod.rs`, both reading `tool_router.list_all()` once.
      `summarize_description` lives in core rather than in the bin crate so it is unit-testable and
      counted by the coverage gate

**Phase gate**: `cargo test --features testing --test '*' -- --test-threads=1` green for the new unit
file.

## Phase 3: User Story 1 — a bash-only agent finds and calls a tool (P1)

**Goal**: discover the whole surface for under 8 KB and one tool's contract for under 2 KB, with no
IRIS connection.

**Independent test**: point the binary at an unreachable port and confirm both discovery paths still
exit 0 with correct output.

- [x] T006 [P] [US1] Binary test `crates/iris-agentic-dev-core/tests/binary/cli_discovery.rs`:
      `tool --list` with `IRIS_WEB_PORT` set to a closed port exits 0, prints one line per name in
      `TOOL_NAMES`, and the whole stdout is under 8 KB (FR-001, FR-002, FR-005)
- [x] T007 [P] [US1] Binary test: `tool iris_query --schema` with the same unreachable port exits 0
      and prints the tool's description and `inputSchema` (FR-003)
- [x] T008 [P] [US1] Binary test: `--json` on both paths produces exactly one JSON document on
      stdout and nothing else, parsed with `serde_json::from_str` on the whole capture (FR-004)
- [x] T009 [P] [US1] Binary test: `tool iris_quer --schema` exits non-zero and names `iris_query`,
      reusing the suggestion helper in `tools/param_check.rs` (FR-006)
- [x] T010 [P] [US1] Binary test: `tool iris_query --list` is rejected rather than silently listing,
      so an agent that asked for one schema never gets a catalogue instead
- [x] T011 [US1] Binary test comparing `--schema` output against an MCP `tools/list` response from
      the same binary for all 82 tools; the two schemas must be equal for each (SC-003). This is the
      test that makes the CLI arm's contract identical to the MCP arm's
- [x] T012 [US1] Binary test: `IRIS_TOOLSET=baseline tool --list` lists the baseline set, not the
      merged set, and `--list` never prints a name absent from `TOOL_NAMES` (FR-007, edge case)
- [x] T013 [US1] Implement in `crates/iris-agentic-dev-bin/src/cmd/tool.rs`: `name` becomes
      `Option<String>`, add `--list`, `--schema`, `--json`; both discovery paths construct
      `IrisTools::new_with_toolset(None, toolset)` and never call `self.conn.resolve()`

**Phase gate**: `cargo test --features testing --test cli_discovery -- --include-ignored
--test-threads=1` green, and `IRIS_WEB_PORT=1 ./target/debug/iris-agentic-dev tool --list | wc -c`
under 8192.

## Phase 4: User Story 2 — every fixed value set is advertised (P2)

**Goal**: the detector fires on all three unseen prose shapes and on handler match arms, then the
eleven parameters advertise their sets with no runtime change.

**Independent test**: the canaries fail before the detector change and pass after; the tree reports
≥ 11 findings before the declarations and 0 after.

- [x] T014 [P] [US2] Canary in `scripts/gates/test_antipatterns.py` for the repeated-assignment
      shape: a fixture description written `what=documents lists all docs, what=modified lists
recently changed` with no declared enum yields exactly one `prose-only-enum` finding (FR-008)
- [x] T015 [P] [US2] Canary for the bare comma list: `mode: get (fetch source), put, delete` with no
      "one of" lead-in yields one finding, and parentheticals do not become values (FR-008)
- [x] T016 [P] [US2] Canary for the match-arm source: a fixture handler branching on a declared
      parameter with three string literal arms and an error default yields one finding even when its
      description says nothing about values (FR-008) — this is the `iris_admin` shape
- [x] T017 [P] [US2] Canary for both silences: a parameter carrying
      `#[schemars(extend("enum" = [...]))]` and a parameter whose doc comment says `not an enum`
      each yield zero findings, so the exemption is proven to work rather than assumed
- [x] T018 [US2] Run the canaries and confirm every one of T014–T016 fails against the current
      detector. A canary that passes before the fix is testing nothing (Principle XI)
- [x] T019 [US2] Implement the three new sources in `scripts/gates/antipatterns.py`:
      `ASSIGNED_VALUES` (three or more `param=value` mentions), `BARE_COMMA_VALUES`
      (`param: a, b, c`, parentheticals stripped), and a match-arm reader that finds
      `match <param>` inside the handler body and collects string literal arms
- [x] T020 [US2] Run `python3 scripts/gates/antipatterns.py` on the tree and record the finding list
      in `specs/114-tool-surface-discovery/research.md`; it must name at least the eleven parameters
      from T001 (SC-004, first half)
- [x] T021 [US2] Extend `crates/iris-agentic-dev-core/tests/unit/test_enum_contract.rs`: for every
      parameter that declares an `enum`, the declared values equal the string literals its handler
      branches on — schema against handler, never schema against prose (FR-011)
- [x] T022 [US2] Live IRIS test in `crates/iris-agentic-dev-core/tests/integration/enum_rejection.rs`:
      for each of the eleven parameters, an out-of-set value returns the same `error_code` and
      message text as it does today, recorded before the declarations land (FR-010, SC-005)
- [x] T023 [US2] Declare the twelve value sets, reading each handler's match arms for the values:
      `iris_info.what`, `iris_doc.mode`, `iris_admin.type`, `iris_debug.action`,
      `iris_global.action`, `iris_macro.action`, `iris_production.action`,
      `iris_source_control.action`, `kb.action`, `skill.action`, `skill_community.action`,
      `agent_info.what`. Any one whose set is not closed gets a doc comment saying why it is
      `not an enum`, with the reason (FR-009). Twelve, not eleven: `iris_admin.action` was already
      declared by 113 and only looked like a finding, and `agent_info.what` is new — it is pruned
      from the default Merged tier, so the T001 census did not see it (recorded in research.md)
- [x] T024 [US2] Re-run the detector: zero findings. Re-run T021 and T022: green. Zero is now
      evidence, because T018 showed the canaries failing first

**Phase gate**: `python3 scripts/gates/antipatterns.py` clean, `python3
scripts/gates/test_antipatterns.py` green, and the eleven runtime answers unchanged.

## Phase 5: Polish

- [x] T025 [P] Document both flags in `docs/tools.md` — the discovery cost table from research.md
      belongs here, since the number is the reason the flags exist
- [x] T026 [P] Add a `tool_surface` note to `docs/connecting.md` explaining that discovery needs no
      connection, so a harness can list tools before it has a container
- [x] T027 Register any new source file in `coverage-floors.toml` at its measured unit-only value
      minus 2 points, and re-record drifted line numbers in `scripts/gates/antipatterns-baseline.txt`
      rather than regenerating the baseline. No new `crates/*/src` file was added — 114 only edited
      existing ones — so `coverage-floors.toml` is untouched and the whole task is the baseline
      re-record: nine `tool-name-refs` lines moved to their drifted line numbers (eight in
      `crates/*/src`, one in `docs/tools.md`), and no line was added. The one genuinely new finding,
      `param_check.rs:111`, was fixed in the prose instead of baselined
- [x] T028 `cargo fmt --all -- --check`, `cargo clippy --all-targets --features testing -- -D
warnings`, and `./scripts/coverage.sh` at or above the 88 floor with zero failing targets. fmt and
      clippy clean. Coverage 88.56% against the 88 floor, and all 66 files with coverage data meet
      their own floors. Not zero failing targets: `test_dispatch_find_subclass_implementations_v2`
      and `..._cache_hit` failed 322 s into the serial `test_handlers_live` run with
      `error sending request for url .../action/query` — a dropped Atelier connection, not an
      assertion. Both pass on their own, and 114 touches no hierarchy code, so this is the container
      dropping a request under a long single-threaded run. Recorded rather than papered over
- [x] T029 Update the Recent Changes block in `CLAUDE.md` with what 114 changed

**Phase gate**: every gate in T028 clean.

## Dependencies

- Phase 2 blocks Phase 3 (the CLI reads the catalogue).
- Phase 3 and Phase 4 are independent; either can ship alone.
- Phase 5 needs both.

## Suggested MVP

Phase 2 + Phase 3. That alone unblocks the third eval arm, which is the reason this feature exists.
