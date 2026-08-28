# Contract: Emitted Audit Record

**Feature**: 086-agent-attribution-audit | Covers FR-020 … FR-024

Opt-in, off by default. When on, one `%SYS.Audit` record per tool call.

## Why our own event and not `%System`/`%MCP`

IRIS 2026.3.0 adds `%System/%MCP/ToolCall` and `%System/%MCP/ToolDiscovery` (DP-452957). Those
fire for MCP servers **IRIS hosts**, and only where an audit policy has been applied to the
server or tool set — the enabled event type alone records nothing (DP-445295). iris-agentic-dev is
an external MCP server that reaches IRIS over Atelier REST, so it produces none of them either
way (research R13). We never write to a platform-owned event:
`EventSource='iris-agentic-dev'` keeps the two trails separable in a report, and an operator can
enable either, both or neither.

## Operator prerequisite

The tool never creates or enables the event definition — that is a `%SYS` security write, and an
agent that can enable its own auditing can disable it (FR-024). The operator runs this once, in
`%SYS`:

```objectscript
Set tSC = ##class(Security.Events).Create("iris-agentic-dev","Tool","ToolCall","iris-agentic-dev tool invocation",1)
Write $SYSTEM.Status.GetErrorText(tSC)
```

Verified live: `Create(Source,Type,Name,Description="",Enabled=1,Flags=0)` returns `$$$OK`, and
`Security.Events.Get` then reports `Enabled=1`, `Flags=0`.

Until that entry exists, emission is inert: nothing is written, no tool call fails, and the
tool reports this exact command as the remediation.

## Emission call

```objectscript
Set tSC = $SYSTEM.Security.Audit("iris-agentic-dev","Tool","ToolCall",EventData,Description)
```

Five arguments, confirmed by `%Dictionary` introspection. `%SYSTEM.Security` exposes exactly
two audit methods, `Audit` and `AuditID`. There is no flush API.

## Return-code contract (measured)

| Condition                   | Return | Record | Tool behavior                                  |
| --------------------------- | ------ | ------ | ---------------------------------------------- |
| Event exists, `Enabled = 1` | `1`    | yes    | Nothing to report                              |
| Event exists, `Enabled = 0` | `0`    | no     | Warn once with cause + remediation, then count |
| Event does not exist        | `0`    | no     | Warn once with cause + remediation, then count |

`$SYSTEM.Status.GetErrorText` on the failure path returns `ERROR #00: (no error description)`.
The tool therefore authors its own cause and remediation text and does not forward an IRIS
error string.

## `EventData` format

```text
tool=<tool-name> mode=<mcp|cli> ua=<caller-marker> [client=<name>/<version>]
```

The `ua=` value is byte-identical to the `User-Agent` sent on the same call, which is what makes
FR-021's requirement — that the access log and `%SYS.Audit` agree — checkable rather than
aspirational. Both are derived from the same values at the same choke point (research R10).

`EventData` is ASCII-only. The operator label reaches it through the marker, and DP-446307 (Open)
has `%SYS.Audit::Export()` throwing `<ILLEGAL VALUE>` on a record containing a Unicode character
— a non-ASCII label would make the whole audit log unexportable, which is a worse failure than a
mangled label. The sanitizer enforces this (research R7a).

Parameters are **not** included verbatim. The existing local audit log already scrubs
credentials and PHI global names before recording params (`audit_log.rs::scrub_params`);
`%SYS.Audit` is readable by anyone with `%SYS` access, so this record carries identity and
intent, not payload.

## Fields IRIS fills in

Read back from a live record: `Username=_SYSTEM`, `Roles=%All`, `Authentication=32`,
`ClientIPAddress`, `ClientExecutableName=CSPa24.so`, `CSPSessionID`, `OSUsername=irisowner`,
`Pid`, `Namespace`, `RoutineSpec`, `UTCTimeStamp`, `SystemID`, `Version`.

Self-reported records carry the client fields. Native `RoutineChange` records do **not** —
`ClientIPAddress`, `ClientExecutableName` and `CSPSessionID` come back empty there (measured).
That difference is the whole reason emission is worth having, and the reason it is not a
replacement for the native trail.

## Failure policy

An audit write failure never fails the tool call it describes (FR-023). This follows the
precedent already in the codebase: `audit_log.rs::write` logs a warning and returns `Ok(())`.
The first failure warns with cause and remediation; subsequent failures increment a per
connection counter surfaced in connection status.

## Namespace scope

The write succeeds from a working namespace — verified from `USER`. Only configuration needs
`%SYS`: `%Dictionary.CompiledClass.%ExistsId("Security.Events")` returns `0` in `USER`, so the
tool cannot see the configuration class from where it runs. Emit anywhere, configure only in
`%SYS`.

## Cost

Ten consecutive writes completed within `$ZHOROLOG` resolution (`0.0000s`). The write is
in-process; there is no latency argument against per-call emission.

## Read-back contract (tests and docs)

Read records through the `%SYS.Audit` **List query**, not a `SELECT`. DP-449511 (Open) records
that SQL selects on `%SYS.Audit` can return stale data because the indices are not refreshed
first, while the List query refreshes them. That, not a write delay, is why cross-process
`SELECT COUNT(*)` returned `0` seconds after writes that were present minutes later (research
R7). Live tests read through the List query and still poll with a bounded timeout as a belt.

Any `SELECT`-based recipe published in the guide must carry the staleness warning.

## Negative contract

With `irisAudit` absent or `false`, no `iris-agentic-dev` record is written. A live test asserts
this by counting rows before and after a tool call. Audit records are immutable — deleting the
event definition does not remove records already written — so tests compare a filtered count
before and after, never a total.
