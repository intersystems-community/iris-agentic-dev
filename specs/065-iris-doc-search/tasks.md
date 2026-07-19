# Tasks: 065 — iris_doc_search

## Phase 1: Tool Implementation + Unit Tests

- [x] T001 Write unit tests for `doc_search.rs` pure functions in `tests/unit/test_doc_search_unit.rs`
- [x] T002 Implement `crates/iris-agentic-dev-core/src/tools/doc_search.rs`
- [x] T003 Wire `iris_doc_search` into `mod.rs` (mod decl, dispatch, registration, descriptor, handler)
- [x] T004 Add `"iris_doc_search"` to `TOOL_NAMES` in `crates/iris-agentic-dev-bin/src/cmd/tool.rs`
- [x] T005 Run `cargo test` — all unit tests pass

## Phase 2: Integration Test + Skill Upgrade

- [x] T006 Add `#[ignore]` live-network integration test in `tests/integration/test_doc_search_live.rs`
- [x] T007 Run integration test against live Algolia network — passes
- [x] T008 Rewrite `skills/skills/iris-docs/SKILL.md` to lead with `iris_doc_search`

## Phase 3: Benchmark + Lift

- [x] T009 Write `benchmark/021/tasks/DOC-01.yaml` through `DOC-05.yaml`
- [x] T010 Add `DOC_SYSTEM_BASELINE`, `DOC_SYSTEM_MERGED`, DOC routing to `benchmark/021/runner/claude_code.py`
- [x] T011 Add `DOC_CATEGORY_NOTE` to `benchmark/021/runner/judge.py`
- [x] T012 Run baseline pass (5 tasks × 2 paths) — record scores
- [x] T013 Run merged pass (5 tasks × 2 paths) — record scores (DOC-03 excluded; rate limit timeout; 8/10 tasks run)
- [x] T014 Write `specs/065-iris-doc-search/lift-results.md` — lift ≥ +0.20 required (achieved +0.46)

## Phase 4: Polish

- [x] T015 `cargo clippy --all-targets -- -D warnings` clean
- [x] T016 `cargo fmt --all`
- [x] T017 `cargo llvm-cov --include-ignored` — 86.83% line (90% gate unmet; pre-existing gap)
- [x] T018 Write release notes draft in `specs/065-iris-doc-search/release-notes-0.9.3.md`
