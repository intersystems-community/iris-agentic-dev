# Feature Specification: iris_global Tool

**Feature Branch**: `052-iris-global`
**Created**: 2026-06-29
**Status**: Implemented; amended 2026-10-10 (FR-015 to FR-023, gate bypass fix)
**Depends on**: 051 (PHI policy and env gates — merged to master)

## Overview

iris-agentic-dev has no MCP tool for reading or writing IRIS globals. Globals are the
fundamental storage primitive in IRIS — nearly all production IRIS systems store critical
data in globals, including HL7 message bodies, scheduling data, clinical records, and
application state. AI agents working with IRIS need the ability to inspect and manipulate
globals to debug issues, investigate data, and perform guided operations.

This feature adds the `iris_global` MCP tool with four operations: `get` (read a node or
subtree), `set` (write a single node), `kill` (delete a node or subtree), and `list` (list
subscripts at a given node level). All operations pass through `dispatch_gate()` (spec 051)
which enforces the environment template gate, PHI data policy, system blocklist, and PHI
name pattern gate before any IRIS call is made.

**This is the tool that spec 051 was built to protect.**

---

## Clarifications

### Session 2026-06-29

- Q: How should subscript values be sanitized before ObjectScript interpolation? → A: Allowlist — subscripts must match `^[a-zA-Z0-9 _.:\-]+$`; reject with `INVALID_SUBSCRIPT` error before any IRIS call. No escape-and-proceed fallback.
- Q: How should subtree traversal be bounded beyond node count? → A: Count cap (`max_nodes` default 100, max 1000) plus 5-second wall-clock timeout; return `truncated: true` on either limit.
- Q: How should IRIS-side execute errors be reported? → A: Reuse existing codes — HTTP 5xx → `IRIS_UNREACHABLE`; ObjectScript `CATCH` output → `IRIS_EXECUTE_ERROR` with `message` field. Consistent with `iris_execute` tool.

### Session 2026-10-10 — gate bypass (a leading `^` skipped gates [3] and [4])

Found while fact-checking the Part 2 article against a live container: `global_name="^ROUTINE"`
wrote to a code-storage global that `global_name="ROUTINE"` refused. Reproduced read-only the same
day, which also found two more ways past the same gates. All three have one cause: **the gate and
the executor did not look at the same name.** The gate compared the string as typed; the executor
stripped one `^`, interpolated the rest into ObjectScript, and validated nothing but the subscripts.

- Q: Which string do the gates compare? → A: The canonical name: one leading `^` stripped, then
  validated. Gates [3] and [4], the audit scrub and the executor all read that one string, and nothing
  reads the raw input after it is canonicalized.
- Q: What is a valid name? → A: After stripping one `^`, `^[%A-Za-z][A-Za-z0-9.]{0,127}$`. Anything
  else is rejected with `INVALID_GLOBAL_NAME` before any gate or IRIS call: empty, a second `^`,
  parentheses or quotes (an embedded subscript such as `oddDEF("x")`), spaces, `|` or `[` (an extended
  reference), `$`, `,`, `;`, control characters, non-ASCII, and anything over 128 characters.
- Q: Which tools take a global name? → A: `iris_global` (`global_name`), `global_preview` and
  `global_kill` (`global`). The gate read only `global_name`/`globalName`, so the last two skipped
  gates [3] and [4]: `global_preview` of `oddDEF` returned a real entry and `global_kill` takes the
  same parameter. Gate [3] and [4] now read all three keys.
- Q: Does case matter? → A: IRIS global names are case-sensitive and the gates match
  case-insensitively, so `^routine` is refused by the `^ROUTINE` rule. That over-blocks a harmless
  name and is kept; the other direction (a case variant slipping past) cannot happen.
- Q: Cross-namespace access? → A: Through the `namespace` parameter only. An extended reference in the
  name (`^|"%SYS"|ROUTINE`, `^["dir"]ROUTINE`) is rejected, not parsed.
- Q: Operator patterns? → A: `globalBlocklist` and `data_policy_kill_allowlist` entries may be written
  with or without `^`. The matcher strips one `^` from the pattern and from the name, so an entry
  behaves the same either way.
- Q: Is the blocklist itself complete? → A: No. Enumerating `^$GLOBAL` on 2026.2 in USER and %SYS finds
  these code-storage globals that the list did not name: `rMAC`, `rMACSAVE`, `rINC`, `rINCSAVE`,
  `rINDEXCLASS`, `rINDEXEXT`, `rINDEXSQL`, `oddDEP`, `oddEXTR`, `oddMAP`, `oddMETA`, `oddPROC`,
  `oddPROJECT`, `oddPYTHON`, `oddStudioDocument`, `oddStudioMenu`, `oddTSQL`, `oddXML`. `^rMAC` is
  routine source, and two comments in `code_edit_gate.rs` already said it was caught. They are added as
  exact names, not as `^odd*` or `^r*` prefixes, because an over-broad prefix hides application
  globals for good (the reason `^SYS*` was narrowed). A live test enumerates the instance and fails
  when a new family appears.
- Q: Do `global_preview` and `global_kill` run the policy gates at all? → A: No. Each tool handler calls
  `dispatch_gate` itself with a hand-built params object, and these two never did, so reading the
  `global` key in the gate (FR-018) was not enough: the call has to be made. Both handlers now call
  it. `call_tool` still applies the write and destructive tier checks to `global_kill`; this adds
  the environment template, the system blocklist and the PHI name gate.
- Q: Should `mcpTemplate = "live"` block `global_kill`? → A: Yes. `iris_global` with `action=kill` is
  Execute-category and blocked on `live` and `test`; `global_kill` is the same operation and was
  categorized Admin, which those templates allow. The environment gate now treats `global_kill` as
  Execute, as it does `iris_global` set and kill. `global_preview` stays a read.

---

## User Scenarios & Testing

### User Story 1 — Read a global node or subtree (Priority: P1)

A developer debugging an IRIS application wants to inspect the value of a global node or
browse a subtree to understand data structure.

**Independent Test**: Call `iris_global` with `action=get`, `global_name="MyApp.Config"`,
`subscripts=["DatabaseVersion"]`. Expect the node value or "undefined" if not set. Call with
no subscripts to get the top-level value.

**Acceptance Scenarios**:

1. **Given** `action=get` and a valid global name, **When** the node exists, **Then** the
   response includes `value` with the node's string value.
2. **Given** `action=get` and a node that does not exist, **Then** the response includes
   `value: null` and `defined: false` (not an error).
3. **Given** `action=get` with `subtree: true`, **Then** the response includes a `nodes`
   array of `{path, value}` objects for all descendant nodes, capped at `max_nodes`
   (default 100). `path` is the full global reference string as returned by `$Query`
   (e.g. `^MyApp("a","b")`).
4. **Given** `global_name` matches `SYSTEM_BLOCKLIST`, **Then** `SYSTEM_BLOCKLIST` error
   is returned before any IRIS call.
5. **Given** `global_name` matches a PHI pattern and no `acknowledgePhi`, **Then**
   `PHI_GATE_BLOCKED` error is returned.
6. **Given** `global_name` matches a PHI pattern and `acknowledgePhi: true`, **Then** the
   get proceeds normally.

---

### User Story 2 — Set a global node (Priority: P1)

A developer wants to set a specific global node value during debugging or test setup.

**Independent Test**: Call `iris_global` with `action=set`, `global_name="IrisDevTest.Temp"`,
`subscripts=["testKey"]`, `value="hello"`. Then `action=get` to verify the value was written.

**Acceptance Scenarios**:

1. **Given** `action=set`, a valid global name, and a `value`, **When** the operation
   succeeds, **Then** `success: true` is returned.
2. **Given** `action=set` on a `live` template connection, **Then** `ENV_GATE_BLOCKED`
   is returned (set is a write operation).
3. **Given** `action=set` with no `value` param, **Then** a structured error is returned
   explaining the missing parameter.

---

### User Story 3 — Kill a global node or subtree (Priority: P2)

A developer wants to delete a global node or entire subtree for cleanup or test teardown.

**Independent Test**: Call `iris_global` with `action=kill`, `global_name="IrisDevTest.Temp"`.
All nodes under that global are deleted.

**Acceptance Scenarios**:

1. **Given** `action=kill` and a valid non-blocklisted global, **Then** the node (and
   subtree if present) is deleted and `success: true` returned.
2. **Given** `action=kill` on a `live` template, **Then** `ENV_GATE_BLOCKED` (kill is
   a write operation).
3. **Given** `action=kill` on a system-blocklisted global, **Then** `SYSTEM_BLOCKLIST`
   error — unless the global is in `dataPolicyKillAllowlist`, in which case it proceeds.
4. **Given** `action=kill` on a global that doesn't exist, **Then** `success: true` (kill
   of non-existent node is a no-op in ObjectScript, not an error).

---

### User Story 4 — List subscripts at a node level (Priority: P2)

A developer wants to enumerate what subscripts exist under a global node without reading
all values — useful for navigating large global trees.

**Independent Test**: Call `iris_global` with `action=list`, `global_name="MyApp.Data"`.
Expect an array of first-level subscripts.

**Acceptance Scenarios**:

1. **Given** `action=list` and a valid global, **Then** the response includes `subscripts`
   array of string subscript values at that level, up to `max_subscripts` (default 50).
2. **Given** the node has no subscripts, **Then** `subscripts: []` (not an error).
3. **Given** `action=list` on a system-blocklisted global, **Then** `SYSTEM_BLOCKLIST`
   error returned.

---

### Edge Cases

- **Leading `^` optional**: `global_name = "MyApp"` and `global_name = "^MyApp"` are both
  valid and are the same global. Both are gated identically (FR-015).
- **Names that are not names**: `ROUTINE("x")`, `^^ROUTINE`, `ROUTINE` (trailing space), `ROUTINE;`,
  `^|"%SYS"|ROUTINE`, `$ROUTINE`, an empty string, and a 129-character name are all
  `INVALID_GLOBAL_NAME`. They never reach a gate or IRIS.
- **Case**: `^routine` is refused by the `^ROUTINE` rule (matching is case-insensitive; IRIS is
  case-sensitive, so this over-blocks and never under-blocks).
- **`global_preview` and `global_kill`**: the same name rules and the same two gates apply. A blocked
  global gets no preview and no confirm token.
- **Subscripts as array or dot-path**: Accept subscripts as a JSON array `["a", "b"]` — do
  not support dot-path strings (too ambiguous for numeric subscripts).
- **Numeric subscripts**: Pass as strings in the array; IRIS coerces `"1"` to the numeric
  subscript correctly.
- **Subscript validation**: Each subscript must match `^[a-zA-Z0-9 _.:\-]+$`. Subscripts
  containing `"`, `^`, `)`, `(`, or other special characters are rejected with
  `INVALID_SUBSCRIPT` before any IRIS call — no escape-and-proceed fallback.
- **Empty subscript array**: Refers to the root node of the global (e.g., `^MyApp` itself).
- **Subtree cap**: Large globals can have millions of nodes. `max_nodes` default 100, max
  allowed 1000. A 5-second wall-clock traversal timeout also applies. Return
  `truncated: true` when either limit is hit; include `node_count` in the response.
- **Namespace**: Defaults to the connection's default namespace. Explicit `namespace` param
  overrides.
- **Non-string values**: IRIS globals store strings. Numbers stored as strings; binary data
  is base64-encoded in the response.
- **IRIS execute failures**: HTTP 5xx → `IRIS_UNREACHABLE`. ObjectScript runtime error
  (e.g., `<PROTECT>` on a protected global, `<SUBSCRIPT>` on bad subscript type) →
  `IRIS_EXECUTE_ERROR` with `message` from IRIS. Never surface raw stack traces.

---

## Requirements

### Functional Requirements

- **FR-001**: `iris_global` MUST support four `action` values: `get`, `set`, `kill`, `list`.
- **FR-002**: All four actions MUST pass through `dispatch_gate()` before any IRIS call,
  passing `global_name` and `action` as params so gate [3] (system blocklist) and gate [4]
  (PHI name gate) fire correctly.
- **FR-003**: `get` with `subtree: false` (default) returns the single node value.
  `get` with `subtree: true` returns all descendant nodes up to `max_nodes` (default 100,
  max 1000) OR a 5-second wall-clock traversal timeout, whichever triggers first. Either
  limit sets `truncated: true` in the response.
- **FR-004**: `set` requires a `value` string param; sets the node at the specified
  `global_name` + `subscripts` path.
- **FR-005**: `kill` deletes the node and all descendants at `global_name` + `subscripts`.
  Kill of a non-existent node is success.
- **FR-006**: `list` returns an array of subscripts at the specified node level, up to
  `max_subscripts` (default 50, max 500).
- **FR-007**: `global_name` MUST accept names with or without a leading `^`; both forms are the
  same global and MUST be treated identically by every gate (see FR-015).
- **FR-008**: `subscripts` param is an optional JSON array of strings. Absent or empty
  means the root global node. Each subscript string MUST match `^[a-zA-Z0-9 _.:\-]+$`;
  any subscript failing this check MUST be rejected with error code `INVALID_SUBSCRIPT`
  before any IRIS call is made.
- **FR-009**: `namespace` param is optional; defaults to the connection's active namespace.
- **FR-010**: `acknowledgePhi: true` param enables the PHI bypass per spec 051 gate [4].
- **FR-011**: When `subtree: true` returns a truncated result, the response MUST include
  `truncated: true` and the count of returned nodes.
- **FR-012**: Tool MUST be registered in `registered_tool_names()` in `Toolset::Merged`
  tier. For env-gate classification: `get` and `list` actions map to `ToolCategory::Query`
  (permitted by all templates); `set` and `kill` actions map to `ToolCategory::Execute`
  (blocked by live and test templates). No new `ToolCategory` variant is needed.
- **FR-013**: `check_config` response MUST list `iris_global` in the tool inventory.
- **FR-014**: HTTP 5xx from the IRIS execute endpoint MUST return `IRIS_UNREACHABLE`. An
  ObjectScript `CATCH` block triggered during the operation MUST return `IRIS_EXECUTE_ERROR`
  with a `message` field containing the IRIS error string (e.g., `<UNDEFINED>` or
  `<PROTECT>`). No new error codes are introduced for IRIS-side failures.
- **FR-015**: Every tool that takes a global name MUST reduce it to one canonical name (exactly one
  leading `^` stripped) before any gate runs, and gates [3] and [4], the audit scrub and the executor
  MUST all read that canonical name. A gate comparing the raw input is a defect.
- **FR-016**: The canonical name MUST match `^[%A-Za-z][A-Za-z0-9.]{0,127}$`. Any other input MUST
  return `INVALID_GLOBAL_NAME` before any gate or IRIS call, with the offending value and the rule
  in the message. The name is interpolated into generated ObjectScript, so validation is also the
  injection guard, as the subscript allowlist (FR-008) is for subscripts.
- **FR-017**: `matches_pattern` MUST strip one leading `^` from the name as well as from the pattern, so
  `matches_pattern("^ROUTINE", "^ROUTINE")` and `matches_pattern("ROUTINE", "ROUTINE")` are both
  true. Operator-authored `globalBlocklist` and `data_policy_kill_allowlist` entries behave the same
  with or without `^`.
- **FR-018**: `dispatch_gate` MUST read the global name from `global_name`, `globalName` and `global`,
  so `global_preview` and `global_kill` pass through gates [3] and [4].
- **FR-019**: Each handler MUST validate the name itself (defense in depth), so a caller that reaches
  a handler without the gate still cannot interpolate an invalid name.
- **FR-020**: The audit scrub MUST treat `global` like `global_name`: a PHI-pattern name is logged as
  `[REDACTED-PHI]`.
- **FR-021**: A blocked or invalid `global_preview` MUST NOT mint a confirm token.
- **FR-022**: `SYSTEM_BLOCKLIST` MUST name every code-storage global family that a live instance holds,
  as exact names. A live test enumerates `^$GLOBAL` in USER and %SYS and fails on any `^odd*` or
  `^r<Upper>*` global that the list does not block in both spellings.
- **FR-023**: `global_preview` and `global_kill` MUST call `dispatch_gate` before any IRIS call, and
  the environment gate MUST treat `global_kill` as Execute-category, so `mcpTemplate = "live"` and
  `"test"` refuse it exactly as they refuse `iris_global` with `action=kill`.

### Implementation Approach

Implement via IRIS `%Object.Execute` with inline ObjectScript:

```objectscript
// get single node
Set val = $Get(^GlobalName(sub1, sub2))
Set defined = $Data(^GlobalName(sub1, sub2)) > 0

// subtree get (using $Query)
Set ref = $Name(^GlobalName(sub1))
Set node = ref
For {
    Set node = $Query(@node)
    Quit:node=""
    Quit:$Extract(node,1,$Length(ref))'=ref
    // collect node and value
}

// set
Set ^GlobalName(sub1) = value

// kill
Kill ^GlobalName(sub1)

// list subscripts
Set sub = ""
For {
    Set sub = $Order(^GlobalName(sub))
    Quit:sub=""
    // collect sub
}
```

All executed via `iris_execute` HTTP endpoint (same path as `iris_execute` tool).

---

## Success Criteria

- **SC-001**: `iris_global get` on an existing node returns the correct value in < 500ms.
- **SC-002**: `iris_global get` with `subtree: true` on a 100-node tree returns all 100
  nodes correctly.
- **SC-003**: `iris_global set` followed by `iris_global get` round-trips a value correctly.
- **SC-004**: `iris_global kill` removes all descendant nodes.
- **SC-005**: All four actions on a PHI-named global without `acknowledgePhi` return
  `PHI_GATE_BLOCKED`.
- **SC-006**: All four actions on a system-blocklisted global return `SYSTEM_BLOCKLIST`.
- **SC-007**: `iris_global` with `mcpTemplate=live` returns `ENV_GATE_BLOCKED` for set and
  kill; permits get and list.

---

## Assumptions

- IRIS execute endpoint (`/api/atelier/v1/{namespace}/action/execute`) is used for all
  four operations — same transport as `iris_execute` tool.
- Global values are always strings in IRIS; no special binary handling needed for P1/P2.
- Subscripts are always strings in the JSON API; numeric coercion is IRIS's responsibility.
- The `Global` tool category is a new category to be added to `env_gate.rs`; `get` and
  `list` are read-only (not blocked by live template); `set` and `kill` are write operations
  (blocked by live template).
- `iris_global` does not need a separate `iris_global.rs` handler file — it can live in
  `tools/global.rs` as a new module.
