# Data Model: Agent Attribution and Audit

**Feature**: 086-agent-attribution-audit | **Date**: 2026-08-27
**Source**: [spec.md](./spec.md) Key Entities, [research.md](./research.md)

There is no database and no persisted schema of our own. The entities below are: a header
value, a task-scoped value, a config field, and a row IRIS writes into `%SYS.Audit`.

---

## 1. Caller Marker

The `User-Agent` header value sent on every IRIS-bound request.

| Field          | Type                     | Source                                  | Rules                                                                                                                                       |
| -------------- | ------------------------ | --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| product        | literal                  | `iris-agentic-dev`                      | Fixed. FR-002.                                                                                                                              |
| version        | string                   | `env!("CARGO_PKG_VERSION")`             | Exact workspace version, not a range. FR-002.                                                                                               |
| caller mode    | `mcp` \| `cli`           | `CallerMode`, set once from `main.rs`   | Determined by process invocation, never by config or env. FR-003/004.                                                                       |
| operator label | string, optional         | `IRIS_AGENT_LABEL`                      | ASCII only, control chars → space, whitespace runs collapsed, trimmed, capped at 64 chars, dropped if empty after cleaning. FR-005/006/007. |
| MCP client     | name + version, optional | peer `Implementation` from `initialize` | Present only for an MCP session that identified itself. FR-012.                                                                             |

**Relationships**: one marker per outgoing request. Assembled fresh at client-construction
time — never cached — so it always reflects the caller mode and the peer in scope.

**Validation**: the assembled value MUST satisfy `HeaderValue::from_str` and MUST NOT contain
empty parentheses or a dangling separator when optional parts are absent (FR-007/008).

**State transitions**: none. Caller mode is set once per process (first write wins). The label
is read from the environment on each assembly.

**Why ASCII**: the label travels into `%SYS.Audit` `EventData`, and DP-446307 (Open) has
`%SYS.Audit::Export()` throwing `<ILLEGAL VALUE>` on any record containing a Unicode character.
A non-ASCII label would make the entire audit log unexportable (research R7a).

## 2. MCP Peer Identity (task-scoped)

| Field   | Type   | Source                   | Lifetime                              |
| ------- | ------ | ------------------------ | ------------------------------------- |
| name    | string | `Implementation.name`    | The `call_tool` task and its children |
| version | string | `Implementation.version` | Same                                  |

Carried in a `tokio::task_local!` set inside the same `scope(…)` that already carries
`CALL_START` (`tools/mod.rs:60`). Absent outside a tool call and absent when the peer sent no
`clientInfo` — in both cases the marker simply omits the MCP client part.

**Why task-scoped and not global**: the HTTP transport clones one `IrisTools` across every
session (`mcp.rs`), so a process-global value would be overwritten by concurrent clients. See
research R3.

## 3. Policy Audit Setting

One new field on the existing per-connection policy block, `[policy.<server-name>]`.

| Field               | TOML key    | Type | Default | Rules                                  |
| ------------------- | ----------- | ---- | ------- | -------------------------------------- |
| IRIS audit emission | `irisAudit` | bool | `false` | Off unless explicitly set. FR-020/022. |

Added to `ConnectionPolicyRaw` (`workspace_config.rs:213`) with `#[serde(rename = "irisAudit",
default)]`, matching the existing camelCase convention (`mcpTemplate`, `dataPolicy`,
`globalBlocklist`, `dataPolicyKillAllowlist`), and surfaced on `ConnectionPolicy`.

**Validation**: parsed from a TOML string, not a struct literal, so a missing `rename` fails a
test rather than silently dropping the key.

**Relationships**: scoped to one connection, which is what makes "audit in production, silent
in development" expressible.

## 4. Emitted Audit Record

What IRIS stores when emission is on. All columns below were read back from a live
`%SYS.Audit` row (research R6).

| `%SYS.Audit` column                                                                             | Written by | Value                                                                                      |
| ----------------------------------------------------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------ |
| `EventSource`                                                                                   | us         | `iris-agentic-dev`                                                                         |
| `EventType`                                                                                     | us         | `Tool`                                                                                     |
| `Event`                                                                                         | us         | `ToolCall`                                                                                 |
| `EventData`                                                                                     | us         | `tool=<name> mode=<mcp\|cli> ua=<marker>` plus MCP client name/version when known. FR-021. |
| `Description`                                                                                   | us         | Short human-readable summary of the call                                                   |
| `Username`, `Roles`, `Authentication`                                                           | IRIS       | Credential the connection authenticated with                                               |
| `ClientIPAddress`, `ClientExecutableName`, `CSPSessionID`                                       | IRIS       | Populated for self-reported records (unlike `RoutineChange`)                               |
| `Namespace`, `RoutineSpec`, `Pid`, `JobId`, `OSUsername`, `UTCTimeStamp`, `SystemID`, `Version` | IRIS       | Ambient                                                                                    |

**Single event definition**: one `iris-agentic-dev` / `Tool` / `ToolCall` entry, with the tool
name in `EventData`. Per-tool event names would force the operator to create ~100 definitions
(research R6).

**Distinct from the platform's own MCP events**: IRIS 2026.3.0 adds `%System/%MCP/ToolCall` and
`%System/%MCP/ToolDiscovery` for MCP servers IRIS hosts (DP-452957; the event type ships enabled
but recording also needs an audit policy on the server or tool set, per DP-445295). We
never write under `%System`/`%MCP`; `EventSource` is what separates the two trails in a report
(research R13).

**Read-back**: through the `%SYS.Audit` List query. A `SELECT` can return stale data because it
does not refresh the indices first (DP-449511, research R7).

**Preconditions**: the event definition must exist and be enabled. `$SYSTEM.Security.Audit`
returns `1` only then; it returns `0` both when the event is missing and when it is disabled,
with an empty IRIS error text (research R6). The tool authors its own cause and remediation.

**Failure handling**: never fails the tool call. First failure warns with cause and
remediation; later failures increment a counter reported in connection status (FR-023).

**Lifecycle note**: audit records are immutable history. Deleting the event definition does
not remove records already written (measured, research R11) — so no test may assert on a total
row count.

## 5. Attribution Warning State

| Field               | Type  | Scope          | Rules                                                                   |
| ------------------- | ----- | -------------- | ----------------------------------------------------------------------- |
| docker-exec warned  | bool  | per connection | Warn once that attribution is unavailable on this transport. FR-011.    |
| audit failure count | usize | per connection | Incremented after the first warned failure; reported in status. FR-023. |

Both are per connection, not per process, so one degraded connection does not silence the
warning for another.
