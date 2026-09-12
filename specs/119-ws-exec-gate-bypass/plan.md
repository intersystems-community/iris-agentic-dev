# Implementation Plan: iris_ws_exec gate bypass

**Branch**: `119-ws-exec-gate-bypass` | **Date**: 2026-09-12 | **Spec**: [spec.md](./spec.md)
**Issue**: [#137](https://github.com/intersystems-community/iris-agentic-dev/issues/137)

**Written retrospectively.** The fix was implemented before this file existed; it records the
approach that shipped in commit `ccd9c58`, not a forward-looking proposal.

## Summary

`iris_ws_exec` executed arbitrary ObjectScript with no gate but the write tier. The fix is not
"add gate calls to this handler" — that is what produced the bug. It is to make the gate's own
tool set a declared constant, and to collapse the ten-site per-handler gate preamble into one
shared function that both arbitrary-execution tools call.

## Technical Context

**Language/Version**: Rust 2021
**Primary Dependencies**: none added
**Storage**: N/A
**Testing**: `cargo test --features testing`; live container `iris-dev-iris` (localhost:52780)
**Target Platform**: macOS/Linux/Windows, same as the binary
**Project Type**: single workspace, two crates
**Constraints**: the code-edit hard-block at `dispatch_gate` step `[0]` is non-configurable and
must stay that way — no config field may switch it off
**Scale/Scope**: 2 source files, 1 params struct, 3 test files

## Root cause

`dispatch_gate` step `[0]` branched on three literal tool names, and `iris_ws_exec` was not one
of them. Separately, the gate is invoked from ten per-handler sites rather than once in the
dispatcher, so `iris_execute` carried an ~85-line preamble (`dispatch_gate`, per-server policy
gate, audit, role gate, destructive tier for `Kill ^global`) that a newly added handler had no
reason to know about. Both facts are needed to explain the bug: the first made the tool
invisible to the gate, the second made it easy to add a handler that never calls it.

`iris_ws_exec` is the fourth tool to ship past this gate, after `iris_global` set/kill,
`iris_lookup_manage` set/delete, and `iris_execute_method`. That is a bug class, not an
incident.

## Approach

1. **Declare the tool set.** `pub const CODE_EXEC_TOOLS: &[&str] = &["iris_execute",
"iris_ws_exec"]` in `policy/gate.rs`; step `[0]`'s first branch tests membership.
   `iris_execute_method` and `iris_query` keep their own branches — their gate conditions differ.
2. **Share the preamble.** One `arbitrary_exec_gate(tool_name, params_json, code, confirmed)`
   in `tools/mod.rs`, returning `Option<Value>` — `Some(refusal)` short-circuits the handler.
   Both `iris_execute` and `iris_ws_exec` call it and nothing else duplicates it.
3. **Gate before the session pool.** `iris_ws_exec` calls it before `WsSessionPool::exec`, so a
   refused call never reaches IRIS. Namespace comes from `WsSessionPool::parse_token`.
4. **Add `confirmed`.** `WsExecParams` gains `#[serde(default)] pub confirmed: bool` so the
   destructive tier behaves as it does on `iris_execute`.
5. **Detector.** A test walks the tool router: any tool taking a `code` parameter and absent from
   `CODE_EXEC_TOOLS` fails it. This is what closes the class rather than the instance.

## Alternatives considered

**Gate centrally in `call_tool`.** The right end state, and where 085 already put the write and
destructive gates. Rejected for this fix: the ten handler sites pass different arguments
(namespace resolution, per-tool params shapes), so moving them all is a refactor with its own
risk, on a branch whose job is to close a reported hole. `arbitrary_exec_gate` is the first two
of those ten collapsed; the rest is follow-on work.

**Name `iris_ws_exec` in step `[0]`'s existing if/else.** One line, fixes the instance, leaves
the class open — the next tool ships past the gate the same way. Rejected.

## Constitution Check

| Principle                      | Status | Notes                                                        |
| ------------------------------ | ------ | ------------------------------------------------------------ |
| I. Zero-Install Binary         | PASS   | No new install step                                          |
| II. ObjectScript Sanity        | N/A    | No new ObjectScript APIs called                              |
| III. HTTP-First Execution      | PASS   | No new Docker-required path                                  |
| IV. Test-First, Fixture-Driven | PASS   | Unit tests written before the gate change                    |
| V. Output Shape Parity         | PASS   | Refusal shape identical to `iris_execute`'s                  |
| VI. Environment Guard          | PASS   | This is the guard; tier behaviour now matches `iris_execute` |
| VII. Dependency Minimalism     | PASS   | No new crate                                                 |
| VIII. 90% Coverage Gate        | PASS   | Every new branch has a test at one of the three layers       |
| IX. Tool Lift Requirement      | N/A    | No new tool and no description change                        |
| X. ObjectScript Coverage       | N/A    | Pure Rust feature                                            |

## Project Structure

```text
specs/119-ws-exec-gate-bypass/
├── spec.md
├── plan.md   # this file
└── tasks.md
```

No `research.md`: no ObjectScript API needed verification. No `data-model.md` or `contracts/`:
one constant and one bool parameter, both described above.

### Source changes

```text
crates/iris-agentic-dev-core/
├── src/policy/gate.rs                          # CODE_EXEC_TOOLS; step [0] membership test
├── src/tools/mod.rs                            # arbitrary_exec_gate; both handlers call it
├── src/tools/ws_tools.rs                       # WsExecParams.confirmed
├── tests/unit/test_ws_exec_gate.rs             # 7 — gate decisions + the router detector
├── tests/binary/ws_exec_gate.rs                # 4 — stdio JSON-RPC, no IRIS
└── tests/integration/test_ws_exec_gate_live.rs  # 3 — live iris-dev-iris
```

## Known limit

The block matches the code string, so `Kill ^X` is caught and `Set g="^X" Kill @g` is not. That
was already true of `iris_execute`. Stated in the refusal message and in `docs/tools.md`;
closing it needs a parser and is out of scope here.
