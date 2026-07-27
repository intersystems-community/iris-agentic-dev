# Tasks: CBM Graph Navigation Tools (071)

**Input**: `specs/071-cbm-graph-tools/spec.md`
**Prerequisites**: spec.md ✅; CBM binary at `~/.local/bin/codebase-memory-mcp` ✅

All Rust paths relative to `crates/iris-agentic-dev-core/`.
Tests are written before implementation — each test must **fail** before its implementation
task begins, then **pass** after.

---

## Phase 1: Core module with unit tests

**Purpose**: Build `graph.rs` with binary discovery, project-name derivation, and the
index gate — all pure logic, no CBM required.

- [ ] T001 [US6] Write unit tests for `find_cbm_binary` in `graph.rs`
      `#[cfg(test)]`: (a) `CODEBASE_MEMORY_PATH` override wins, (b) binary on `PATH` used
      when env var absent, (c) `~/.local/bin/codebase-memory-mcp` fallback, (d) all absent
      → `Err("CBM_NOT_FOUND: ...")`; confirm they **fail** (module doesn't exist yet)
- [ ] T002 [US6] Create `crates/iris-agentic-dev-core/src/tools/graph.rs`; add
      `pub mod graph;` to `tools/mod.rs`; implement `fn find_cbm_binary() -> Result<PathBuf,
String>` with discovery order from US6; confirm T001 tests **pass**
- [ ] T003 [US5] Write unit test for `check_cbm_index_gate`: absent → `CBM_INDEX_GATED`;
      `CBM_ALLOW_INDEX=1` → `Ok(())`; confirm **fail**
- [ ] T004 [US5] Implement `fn check_cbm_index_gate() -> Result<(), McpError>` in
      `graph.rs`; confirm T003 **passes**
- [ ] T005 [US6] Write unit test for `default_project_name`: mock cwd `/Users/tdyar/ws/myproject`
      → `"Users-tdyar-ws-myproject"`; confirm **fail**
- [ ] T006 [US6] Implement `fn default_project_name() -> String` using
      `std::env::current_dir()`; confirm T005 **passes**
- [ ] T007 Write unit test for `run_cbm`: mock a binary that exits 0 with `{"ok":true}`
      → parsed value; exits 1 with stderr `"boom"` → `Err` containing `"boom"`; confirm
      **fail**
- [ ] T008 Implement `fn run_cbm(bin: &Path, verb: &str, args: Value) -> Result<Value,
String>` using `std::process::Command`; confirm T007 **passes**

**Checkpoint**: `cargo test -p iris-agentic-dev-core graph` passes; no CBM call needed.

---

## Phase 2: MCP tool wiring

**Purpose**: Define param structs, implement each tool handler, and register in `mod.rs`.

- [ ] T009 [US1] Write test T071-01 asserting `iris_graph_search` appears in the tool
      list and its JSON schema has `name_pattern` and `label` fields; confirm **fail**
- [ ] T010 [US1] Add `GraphSearchParams` struct, implement `async fn iris_graph_search`
      (calls `run_cbm("search_graph", ...)`, handles `CBM_NOT_FOUND` / `CBM_NOT_INDEXED`);
      add `dispatch!` line to `mod.rs` and add tool name to list; confirm T071-01 **passes**
- [ ] T011 [US2] Write test T071-02 asserting `iris_graph_trace` in tool list with `from`,
      `to`, `mode` fields; confirm **fail**
- [ ] T012 [US2] Add `GraphTraceParams` struct, implement `async fn iris_graph_trace`;
      wire in `mod.rs`; confirm T071-02 **passes**
- [ ] T013 [US3] Write test T071-03 asserting `iris_graph_architecture` in tool list with
      optional `aspects` field; confirm **fail**
- [ ] T014 [US3] Add `GraphArchitectureParams` struct, implement
      `async fn iris_graph_architecture`; wire in `mod.rs`; confirm T071-03 **passes**
- [ ] T015 [US4] Write test T071-04 asserting `iris_graph_snippet` in tool list with
      `qualified_name` field; confirm **fail**
- [ ] T016 [US4] Add `GraphSnippetParams` struct, implement `async fn iris_graph_snippet`;
      wire in `mod.rs`; confirm T071-04 **passes**
- [ ] T017 [US5] Write test T071-05 asserting `iris_graph_index` in tool list with `path`,
      `incremental`, `project` fields; confirm **fail**
- [ ] T018 [US5] Add `GraphIndexParams` struct, implement `async fn iris_graph_index`
      (calls `check_cbm_index_gate` first); wire in `mod.rs`; confirm T071-05 **passes**

**Checkpoint**: `cargo build` clean; all 5 tools in tool list; index gate unit test green.

---

## Phase 3: Integration tests (CBM required, `#[ignore]` by default)

**Purpose**: Verify each tool against a real CBM-indexed project. These run manually with
`cargo test --test graph_tests -- --include-ignored`.

- [ ] T019 [US1] Create `tests/graph_tests.rs`; write `#[ignore] #[test] fn
graph_search_returns_nodes()`: call `iris_graph_search` with `name_pattern="*"` against
      `Users-tdyar-ws-iris-agentic-dev`; assert `count > 0`
- [ ] T020 [US2] Write `#[ignore] #[test] fn graph_trace_no_crash()`: call
      `iris_graph_trace` with two known-existing qualified names; assert `found` field present
- [ ] T021 [US4] Write `#[ignore] #[test] fn graph_snippet_known_symbol()`: call
      `iris_graph_snippet` with `qualified_name` of a known symbol; assert `source` non-empty
      and `start_line >= 1`
- [ ] T022 [US3] Write `#[ignore] #[test] fn graph_architecture_non_empty()`: call
      `iris_graph_architecture`; assert response is non-empty object
- [ ] T023 [US5] Write `#[ignore] #[test] fn graph_index_gated_without_env()`:
      `CBM_ALLOW_INDEX` unset → assert error code `CBM_INDEX_GATED`

---

## Phase 4: Polish and docs

- [ ] T024 Update `docs/tools.md` — add `iris_graph_*` tools to the
      "Search and introspection" table with one-line descriptions; add `CBM_NOT_FOUND`,
      `CBM_NOT_INDEXED`, `CBM_INDEX_GATED` to Common error codes table
- [ ] T025 Run `cargo clippy -p iris-agentic-dev-core -- -D warnings`; confirm clean
- [ ] T026 Run `cargo test -p iris-agentic-dev-core` (unit tests only); confirm all pass
- [ ] T027 Smoke test: start the MCP server (`cargo run -- --dev`) and call
      `check_config`; verify all 5 `iris_graph_*` tool names appear in the list

---

## Dependency graph

```text
T001–T008 (core module — pure unit tests, no CBM)
  └─ T009–T018 (MCP wiring — one pair per tool)
    └─ T019–T023 (integration tests — require live CBM)
      └─ T024–T027 (polish + smoke test)
```

T009/T010, T011/T012, T013/T014, T015/T016, T017/T018 are independent pairs within phase 2
and can be worked in any order once phase 1 is complete.
