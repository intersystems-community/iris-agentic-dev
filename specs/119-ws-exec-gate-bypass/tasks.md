# 119 — iris_ws_exec gate bypass: tasks

Written retrospectively alongside `plan.md`; every box reflects work that shipped in commit
`ccd9c58`, verified by the runs recorded under Done criteria.

## Phase 1: Unit tests (written first)

- [x] T001 `tests/unit/test_ws_exec_gate.rs` — `ws_exec_is_blocked_on_the_code_edit_surface`:
      `dispatch_gate` refuses `iris_ws_exec` on the class-delete / `^oddDEF` / `Kill ^global`
      surface. Failed before T005, which is the point of writing it first.
- [x] T002 `ws_exec_and_execute_return_the_same_verdict` — the same code string through both tool
      names must produce the same verdict. This is the assertion that would have caught the bug
      when `iris_ws_exec` was added.
- [x] T003 `ws_exec_permits_ordinary_objectscript` and `ws_exec_is_gated_without_a_valid_session`
      — the gate does not become a blanket refusal.
- [x] T004 Detector, three tests: `code_exec_tools_declares_both_arbitrary_execution_tools`,
      `every_tool_with_a_code_parameter_is_declared` (walks the router; fails for any tool taking
      `code` that is absent from the constant), `code_exec_tools_names_only_registered_tools`
      (catches a stale name after a rename).

## Phase 2: Implementation

- [x] T005 `src/policy/gate.rs` — `pub const CODE_EXEC_TOOLS: &[&str] = &["iris_execute",
  "iris_ws_exec"]` with a doc comment naming #137; step `[0]`'s first branch becomes
      `if CODE_EXEC_TOOLS.contains(&tool_name)`. `iris_execute_method` and `iris_query` keep their
      own branches — different conditions.
- [x] T006 `src/tools/mod.rs` — new `arbitrary_exec_gate(tool_name, params_json, code, confirmed)
  -> Option<serde_json::Value>`: `dispatch_gate` → `server_manager::policy_gate` → audit →
      role gate → `contains_global_kill` destructive tier. `iris_execute`'s ~85-line inline
      preamble collapses to one call, unchanged in behaviour.
- [x] T007 `iris_ws_exec` calls `arbitrary_exec_gate` **before** `WsSessionPool::exec`, so a
      refused call never reaches IRIS. Namespace via `WsSessionPool::parse_token`.
- [x] T008 `src/tools/ws_tools.rs` — `WsExecParams` gains `#[serde(default)] pub confirmed: bool`
      for the destructive tier, matching `iris_execute`.

## Phase 3: Binary invocation tests (no live IRIS)

- [x] T009 `tests/binary/ws_exec_gate.rs`, 4 `#[ignore]` tests over stdio JSON-RPC:
      `ws_exec_refuses_a_class_delete`, `ws_exec_refuses_a_write_to_a_code_storage_global`,
      `ws_exec_refuses_a_literal_global_kill_when_the_destructive_tier_is_off`,
      `the_write_gate_still_answers_first_when_writes_are_off` (tier ordering — the write gate
      must answer before the code-edit block, or the error code misleads).

## Phase 4: Live IRIS integration tests

- [x] T010 `tests/integration/test_ws_exec_gate_live.rs`, 3 `#[ignore]` tests against
      `iris-dev-iris`: a refused class delete and a refused `Kill ^global` leave the session
      usable, and ordinary ObjectScript still runs. A refusal that also breaks the session would
      pass a unit test and be a regression in practice.

## Phase 5: Docs and polish

- [x] T011 `docs/tools.md` — `iris_ws_exec` gate behaviour, `confirmed`, and the indirection
      limit stated in the same words as `iris_execute`'s entry.
- [x] T012 `specs/next-release-notes.md` — entry under Notable fixes crediting @devecchijr and
      naming #137.
- [x] T013 `cargo fmt --all -- --check` and `cargo clippy --features testing --all-targets --
  -D warnings` clean; `.specify/gates/verify.sh` `failed=0 warnings=0`.

## Done criteria

- Unit: 7 new tests green; `unit` target 2380 passed / 0 failed ✓
- Binary: 4 new tests green with `--include-ignored --test-threads=1` ✓
- Live IRIS: 3 new tests green against `iris-dev-iris` ✓
- Non-integration sweep across all six targets, 0 failed ✓
- fmt, clippy, gates clean ✓
- `docs/tools.md` and release notes updated ✓

## Not done here

- The remaining eight per-handler gate sites still each call `dispatch_gate` themselves.
  `arbitrary_exec_gate` collapses two of ten; central enforcement in `call_tool` is follow-on
  work. Recorded in `plan.md` under Alternatives considered.
- Indirection (`Set g="^X" Kill @g`) is still not caught. Needs a parser; unchanged from
  `iris_execute`'s pre-existing behaviour and documented as a limit rather than fixed silently.
