# 119 — iris_ws_exec gate bypass

## Status

Implemented — merged to master (`ccd9c58`). Not yet released; ships in the next release.
Issue #137 stays open until the reporter confirms.

Reported as [#137](https://github.com/intersystems-community/iris-agentic-dev/issues/137) by
`devecchijr` on 2026-09-08. Reproducible on v1.3.0 and v1.4.1.

## Problem

`iris_ws_exec` runs arbitrary ObjectScript in a persistent WebSocket terminal session and
consults no gate except the write tier. The handler is eleven lines:

```rust
async fn iris_ws_exec(&self, Parameters(p): Parameters<ws_tools::WsExecParams>) -> ... {
    let output = WsSessionPool::exec(&self.ws_pool, &p.session, &p.code).await?;
    ok_json(...)
}
```

No `dispatch_gate`, no `policy_gate`, no audit entry, no role gate, no destructive check.
So everything `iris_execute` refuses, `iris_ws_exec` accepts:

```text
iris_ws_open(namespace="USER")                                  -> session token
iris_ws_exec(session=…, code="Do $system.OBJ.Delete(\"MyApp.Foo\")")   # CODE_EDIT_BLOCKED on iris_execute
iris_ws_exec(session=…, code="Set ^oddDEF(\"X\")=1")                   # CODE_EDIT_BLOCKED on iris_execute
iris_ws_exec(session=…, code="Kill ^SomeGlobal")                       # DESTRUCTIVE_GATE on iris_execute
```

Two independent gaps produce this:

1. `dispatch_gate()` step `[0]` branches on the tool name — `iris_execute`,
   `iris_execute_method`, `iris_query`. `iris_ws_exec` is not one of them, so
   `check_objectscript_code_edit` is never reached even if the handler called the gate.
2. The `iris_ws_exec` handler calls `dispatch_gate` nowhere, so steps `[1]`–`[4]` (env
   template, bulk PHI, system blocklist, PHI patterns) do not run either.

The write tier does cover it (`wr("iris_ws_exec")` in `write_gate.rs`), enforced centrally in
`call_tool`. That is the only reason a read-only caller cannot use this today. With
`write_tools_enabled = true` — the setting anyone doing development work has on — the whole
policy surface is reachable through a WebSocket session.

Spec 087 deferred this explicitly ("`iris_ws_exec` (WebSocket terminal) — separate analysis
needed; same class of problem"). 1.4.x then extended step `[0]` to `iris_execute_method` and
did not pick it up.

## Root cause

`dispatch_gate` is called from ten handlers individually. `call_tool`'s own comment already
names the failure mode this produces:

> a per-handler guard is a guard a new tool can silently miss, which is exactly how
> `iris_ws_exec`, `iris_global` set/kill, `iris_lookup_manage` set/delete and
> `iris_execute_method` shipped ungated while `check_config` reported the gate as active.

`iris_ws_exec` is the last of those four still ungated. Fixing only the tool name leaves the
mechanism that produced the bug intact: the next arbitrary-execution tool misses it the same
way.

## Scope

1. **Gate `iris_ws_exec` on the same terms as `iris_execute`.** Code-edit hard-block, env
   template, bulk PHI, system blocklist, PHI patterns, server-manager policy gate, role gate,
   destructive `Kill ^` check, and an audit entry on every outcome.
2. **Make step `[0]`'s tool set data, not control flow.** A declared list of the tools whose
   `code` parameter carries arbitrary ObjectScript, so adding a tool to it is a one-line
   change and omitting one is a test failure.
3. **A guard test that fails when a new tool ships ungated.** Read the `dispatch!` table for
   the tool → params-struct mapping, find each struct's fields, and assert every tool with a
   `code: String` field is declared in the step-`[0]` list.
4. **Share the preamble.** `iris_execute` and `iris_ws_exec` run the identical gate sequence;
   it exists once and both call it.

## Out of scope

- Moving `dispatch_gate` into `call_tool` for all 81 tools. It is the right end state and the
  reason this bug exists, but turning gates `[1]`–`[4]` on for 71 tools that have never seen
  them is a behaviour change for every existing `mcpTemplate` and `dataPolicy` setting. It
  needs its own spec and its own release note.
- Indirection. `Kill @var`, `Xecute`, `$ZF`, and method calls that kill a global internally
  are not detected, exactly as spec 087 documented for `iris_execute`. The error message says
  so. IRIS-side credentials and the env template are the controls for those.
- `iris_ws_open` / `iris_ws_close`. Opening a session executes no caller-supplied code and
  closing one executes none either.

## Functional requirements

- **FR-001** `dispatch_gate("iris_ws_exec", …)` returns `CODE_EDIT_BLOCKED` for a `code`
  parameter that reaches the editable-code surface, on the same inputs that block
  `iris_execute`.
- **FR-002** `dispatch_gate("iris_ws_exec", …)` returns `Ok(())` for ObjectScript that does
  not touch that surface (`write $zversion,!`, `Set x=1`).
- **FR-003** The tool names step `[0]` scans for a `code` parameter are a declared, public
  constant. `iris_execute` and `iris_ws_exec` are both in it.
- **FR-004** A test fails if any tool in the `dispatch!` table has a `code: String` params
  field and is absent from that constant.
- **FR-005** `iris_ws_exec` refuses a literal `Kill ^<global>` with
  `DESTRUCTIVE_GATE_DISABLED` when the destructive tier is off, with the same
  indirection caveat `iris_execute` states.
- **FR-006** `iris_ws_exec` writes an audit entry — `allowed` or `blocked` — on every call,
  so a blocked WebSocket exec is as visible as a blocked `iris_execute`.
- **FR-007** `iris_ws_exec` honours the role gate on a `subject` instance, bypassable with
  `confirmed: true`, matching `iris_execute`.
- **FR-008** The gate runs before `WsSessionPool::exec`, so a blocked call sends nothing to
  IRIS and does not disturb session state.

## Testing

Three layers, per the project's coverage policy.

| Layer     | File                                          | Covers                                                                        |
| --------- | --------------------------------------------- | ----------------------------------------------------------------------------- |
| Unit      | `tests/unit/test_ws_exec_gate.rs`             | FR-001 – FR-005: `dispatch_gate` verdicts, the declared list, the guard test  |
| Binary    | `tests/binary/test_ws_exec_gate_binary.rs`    | FR-005, FR-008: refusal over stdio JSON-RPC with a bogus session token        |
| Live IRIS | `tests/integration/test_ws_exec_gate_live.rs` | FR-001, FR-008: real session; blocked call, then a benign call still succeeds |

The binary layer needs no IRIS: the gate fires before the session pool is consulted, so a
made-up session token still produces the gate's error rather than `SESSION_INVALID`. That is
FR-008 stated as a test.

## Notes

`WsExecParams` gains `confirmed: bool` (defaulted, `#[serde(default)]`) for FR-007. Adding an
optional field to a `deny_unknown_fields` struct is backward compatible — existing callers
that omit it get `false`, which is the gated-by-default direction.
