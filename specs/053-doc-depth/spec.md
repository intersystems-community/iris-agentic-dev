# Feature Specification: Document Depth — iris_doc Extensions and iris_execute_method

**Feature Branch**: `053-doc-depth`
**Created**: 2026-06-29
**Status**: Draft
**Depends on**: 051 (env gates — merged to master)

## Overview

Agents exploring large IRIS codebases face three gaps with the current `iris_doc` tool
(get/put/delete/head): they must fetch entire files when they only need a region, they
cannot see the compiled output of macros and generated code, and they have no efficient
way to enumerate documents in a package without issuing a full search. A fourth gap
exists separately: calling a single ClassMethod on IRIS requires writing and executing
full ObjectScript boilerplate, which is error-prone and verbose.

This feature closes all four gaps by adding three new read-only modes to `iris_doc`
(fragment, compiled, list) and a new execute-gated `iris_execute_method` tool.

---

## User Scenarios & Testing

### User Story 1 — Read a Specific Code Region (Priority: P1)

A developer debugging a large ObjectScript class wants to inspect 20 lines around a
method without downloading thousands of lines of source. They specify the document name,
start line, and end line and receive only that region.

**Why this priority**: Large classes are common in IRIS enterprise systems. Fetching the
full source on every exploration step wastes context and time. Fragment read is the
highest-leverage efficiency improvement for agent-driven code exploration.

**Independent Test**: Call `iris_doc` with `mode=fragment`, a known document name,
`start=10`, `end=30`. Verify exactly 21 lines are returned. Call with an out-of-range
end line; verify the response clamps to the last line without error.

**Acceptance Scenarios**:

1. **Given** a document with 200 lines, **When** `mode=fragment` is called with `start=50, end=70`, **Then** lines 50–70 (inclusive) are returned with their original line numbers preserved.
2. **Given** `mode=fragment` with `end` greater than the document length, **When** the call is made, **Then** the response returns up to the last line with `clamped: true`.
3. **Given** `mode=fragment` with `start > end`, **When** the call is made, **Then** a structured `INVALID_PARAMS` error is returned.
4. **Given** a document that does not exist, **When** `mode=fragment` is called, **Then** a `NOT_FOUND` error is returned.
5. **Given** `mcpTemplate=live`, **When** `mode=fragment` is called, **Then** it succeeds — fragment is read-only.

---

### User Story 2 — Inspect Compiled Code (Priority: P1)

A developer tracing a macro expansion or correlating a stack trace to source needs to
see the INT intermediate representation that IRIS actually compiled — not the source
.CLS file. They call `iris_doc` with `mode=compiled` and receive the INT lines.

**Why this priority**: Macro expansion is invisible in source. Stack traces reference INT
line numbers. Without compiled view, agents cannot reliably resolve runtime errors to
source locations.

**Independent Test**: Call `iris_doc` with `mode=compiled` on a class that uses macros.
Verify the response contains INT-format output. Call on a class with no compiled
representation; verify a `NOT_COMPILED` error.

**Acceptance Scenarios**:

1. **Given** a compiled class, **When** `mode=compiled` is called, **Then** the INT representation is returned with `category: INT`.
2. **Given** a class that has never been compiled, **When** `mode=compiled` is called, **Then** a `NOT_COMPILED` error is returned.
3. **Given** `mode=compiled` with `type=OBJ`, **When** the call is made, **Then** the OBJ representation is returned if available.
4. **Given** a `.MAC` routine, **When** `mode=compiled` is called, **Then** the compiled INT output for the routine is returned.
5. **Given** an include file (`.INC`), **When** `mode=compiled` is called, **Then** a `NOT_COMPILED` error with a descriptive message is returned — includes do not compile to INT.
6. **Given** `mcpTemplate=live`, **When** `mode=compiled` is called, **Then** it succeeds — compiled read is read-only.

---

### User Story 3 — Enumerate Documents in a Package (Priority: P2)

A developer exploring an unfamiliar package wants to see all documents it contains —
classes, routines, includes — with size and last-modified date, without running a
full-text search. They call `iris_doc` with `mode=list` and a pattern like `MyApp.*.cls`.

**Why this priority**: Navigation before reading. An agent that can cheaply enumerate a
package can decide which files to fetch next without wasting context on full search
results. Unblocks autonomous exploration workflows.

**Independent Test**: Call `iris_doc` with `mode=list`, `pattern=User.*.cls`. Verify
the response contains a `documents` array with `name`, `size`, `modified`, and `category`
fields. Verify a pattern matching nothing returns an empty array, not an error.

**Acceptance Scenarios**:

1. **Given** a namespace with classes matching `MyApp.*.cls`, **When** `mode=list` is called with that pattern, **Then** the response contains a `documents` array with metadata for each match.
2. **Given** a pattern that matches no documents, **When** `mode=list` is called, **Then** the response returns `{success: true, documents: [], count: 0}`.
3. **Given** `mode=list` with no pattern or a wildcard-only pattern, **When** the call is made, **Then** a `MISSING_PARAMS` error is returned.
4. **Given** a pattern matching more than `max_results` documents (default 200), **When** the call is made, **Then** the first `max_results` results are returned with `truncated: true`.
5. **Given** a `category=CLS` filter, **When** `mode=list` is called, **Then** only class documents are returned.
6. **Given** `mcpTemplate=live`, **When** `mode=list` is called, **Then** it succeeds — list is read-only.

---

### User Story 4 — Invoke a ClassMethod Directly (Priority: P2)

A developer wants to call `##class(MyApp.Utils).FormatDate("2026-01-01")` without
writing full ObjectScript. They call `iris_execute_method` with `class`, `method`, and
`args` and receive the return value directly.

**Why this priority**: Single-shot ClassMethod calls are extremely common in IRIS
debugging and test workflows. The current workaround — hand-writing `iris_execute`
boilerplate — is error-prone and verbose. A dedicated tool eliminates that friction.

**Independent Test**: Call `iris_execute_method` with `class=%SYSTEM.Version`,
`method=GetVersion`, `args=[]`. Verify the response includes `return_value` containing
the IRIS version string.

**Acceptance Scenarios**:

1. **Given** a valid class and method, **When** `iris_execute_method` is called, **Then** the response includes `return_value` with the method's string return value.
2. **Given** a method that takes arguments, **When** called with `args=["arg1","arg2"]`, **Then** the arguments are passed positionally and the return value is correct.
3. **Given** a class or method that does not exist, **When** the call is made, **Then** a `NOT_FOUND` error is returned naming the missing class/method.
4. **Given** a method that throws an ObjectScript exception, **When** the call is made, **Then** an `IRIS_EXECUTE_ERROR` is returned with the IRIS error string.
5. **Given** `mcpTemplate=live`, **When** `iris_execute_method` is called, **Then** `ENV_GATE_BLOCKED` is returned before any IRIS call.
6. **Given** `mcpTemplate=test`, **When** `iris_execute_method` is called, **Then** `ENV_GATE_BLOCKED` is returned.
7. **Given** `mcpTemplate=dev` or no template, **When** `iris_execute_method` is called, **Then** the method executes normally.
8. **Given** a system-blocklisted class (e.g., a `%SYS.*` class), **When** called, **Then** `SYSTEM_BLOCKLIST` error is returned.

---

### Edge Cases

- `mode=fragment` on an empty document: return `{lines: [], count: 0}`, no error.
- `mode=compiled` on a document modified since last compile: return the stale compiled output; include `stale: true` if the IRIS API surfaces a compile timestamp.
- `mode=list` with `max_results=0`: clamp to 1 (consistent with spec 052 clamping pattern).
- Fragment `start=1, end=1`: returns exactly one line.
- `iris_execute_method` with ByRef parameters: return `INVALID_PARAMS` — ByRef is out of scope for v1.
- `iris_execute_method` with a void method (no return value): return `{success: true, return_value: ""}`.
- `mode=fragment` with `start=0`: treat as `start=1` (1-based lines; clamp, don't error).

---

## Requirements

### Functional Requirements

- **FR-001**: `iris_doc` MUST support `mode=fragment` accepting `name`, `start` (integer, 1-based), and `end` (integer, inclusive). Returns lines `start`–`end` with original line numbers. Clamps `end` to document length; returns `clamped: true` when clamped.
- **FR-002**: `iris_doc` MUST support `mode=compiled` accepting `name` and optional `type` (`INT` default, `OBJ`). Returns the compiled representation. Returns `NOT_COMPILED` if no compiled form exists. `.INC` files always return `NOT_COMPILED`.
- **FR-003**: `iris_doc` MUST support `mode=list` accepting `pattern` (required glob), optional `category` filter (default `"ALL"`), and `max_results` (default 200, max 1000). Returns `{documents: [{name, ts, category}], count, truncated}`. Note: Atelier `/docnames/` does not expose `size`; omitted from v1 response.
- **FR-004**: `mode=list` MUST reject a missing or wildcard-only pattern (`*`, `**`, `*.cls` with nothing before the wildcard) with `MISSING_PARAMS`.
- **FR-005**: A new `iris_execute_method` tool MUST accept `class`, `method`, `args` (string array, optional), and `namespace`. Invokes the named ClassMethod and returns `{success: true, return_value: string}`.
- **FR-006**: `iris_execute_method` MUST be `ToolCategory::Execute` — blocked by `mcpTemplate=live` and `mcpTemplate=test`.
- **FR-007**: `mode=fragment`, `mode=compiled`, and `mode=list` MUST be `ToolCategory::Query` — permitted by all `mcpTemplate` values.
- **FR-008**: `iris_execute_method` MUST pass through `dispatch_gate()` — system blocklist and PHI name gate apply to the `class` parameter.
- **FR-009**: `mode=compiled` MUST support `.CLS`, `.MAC`, and `.INT` source document types.
- **FR-010**: New error codes (`NOT_COMPILED`, `MISSING_PARAMS` where list requires pattern) MUST follow `SCREAMING_SNAKE_CASE`. Existing codes (`NOT_FOUND`, `INVALID_PARAMS`, `IRIS_EXECUTE_ERROR`, `ENV_GATE_BLOCKED`, `SYSTEM_BLOCKLIST`) are reused where applicable.
- **FR-011**: `iris_execute_method` MUST be registered in `registered_tool_names()` in the `Toolset::Merged` tier.

### Key Entities

- **Document Fragment**: A contiguous line range from a named IRIS document. Identified by `(name, start, end)`. Read-only; does not persist independently.
- **Compiled Document**: The intermediate compiled form of a source document (`INT` or `OBJ` category). May be stale relative to source.
- **Document Listing Entry**: Metadata record containing `name`, `size` (bytes), `ts` (Atelier timestamp string), and `category`.
- **ClassMethod Invocation**: A stateless single-shot call identified by `(class, method, args[], namespace)`. No object or session is retained between calls.

---

## Success Criteria

### Measurable Outcomes

- **SC-001**: An agent reads a 20-line fragment from a 2,000-line class with no more context transferred than those 20 lines, in under 500ms.
- **SC-002**: `mode=compiled` returns the INT representation of a compiled class in under 1 second.
- **SC-003**: `mode=list` on a 100-document package returns all metadata in under 1 second.
- **SC-004**: `iris_execute_method` on a simple no-arg ClassMethod completes in under 500ms.
- **SC-005**: 100% of `ENV_GATE_BLOCKED` cases for `iris_execute_method` on live/test templates are caught before any IRIS call is made.
- **SC-006**: An agent can enumerate a package and read a target method body in 2 tool calls (list → fragment) instead of requiring a full-source fetch of a large file.

---

## Assumptions

- The Atelier REST API supports fetching compiled (INT) documents via the document endpoint with the `.INT` extension.
- Document metadata (size, modified date) is available from the Atelier document listing endpoint.
- `mode=list` uses the existing Atelier document listing endpoint with glob pattern filtering.
- `iris_execute_method` is implemented using `execute_via_generator` — same mechanism as `iris_global` and `iris_execute` — no IRIS-side install required.
- ByRef and Output parameters are out of scope for `iris_execute_method` v1.
- `max_results` default of 200 for `mode=list` is consistent with `iris_symbols` behavior.
- The `stale` flag for compiled documents is best-effort; omitted if the Atelier API does not expose a compile timestamp.
- `start=0` in fragment mode is treated as `start=1` (clamped, not an error).
