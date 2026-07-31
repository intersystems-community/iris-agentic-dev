# Feature Specification: iris_execute Session State

**Feature Branch**: `071-execute-session`  
**Created**: 2026-07-30  
**Status**: Draft  
**Input**: iris_execute session_state parameter — stateful execution across tool calls without installing anything on IRIS

## Background

`iris_execute` runs ObjectScript via the Atelier write-compile-call cycle. Each call
creates a fresh IRIS process, executes, and exits. No local variables, object references,
or in-process state survive between calls.

Issue #32 (cwennerh): wants to create an object, set properties, call methods, then
continue working with it across multiple `iris_execute` calls. Current behavior forces
the AI to re-open or re-construct the object in every call.

**Design constraint**: no new code may be installed on the IRIS instance. The solution
must work entirely within the existing `execute_via_generator` pattern, using only what
runs inside the generated ObjectScript class.

**Key implementation finding**: `@varName` indirection does not resolve local variables
in the objectgenerator `Execute()` method context — a `Set @"x"` creates a separate
symbol-table entry that is invisible to a plain `Set x`. This rules out any design where
the epilogue captures user-named variables by indirection. The solution uses a single
injected carrier variable `%ctx` (`%DynamicObject`) that the user stores state into
explicitly. The preamble injects `Set %ctx = {}` (or restores from `session_state`);
the epilogue serializes `%ctx` directly. No indirection needed.

## User Scenarios & Testing

### User Story 1 — Carry primitive values across calls (Priority: P1)

An AI is debugging a value computed in one call and wants to build on it in the next
call, without having to recompute or hardcode it.

**Why this priority**: Covers the simplest and most common case. Requires no
serialization beyond strings and numbers. Proves the round-trip plumbing works.

**Independent Test**: Call `iris_execute` with code that sets `%ctx.x = 42`, receive
back `session_state`. Call again passing that `session_state` — confirm `%ctx.x` is 42.

**Acceptance Scenarios**:

1. **Given** `iris_execute` with `Set %ctx.x = 42` and `use_session: true`,
   **When** the tool returns,
   **Then** the response includes a non-empty `session_state` string.

2. **Given** a prior `session_state`,
   **When** `iris_execute` is called with that `session_state` and code `Write %ctx.x`,
   **Then** the output is `42`.

3. **Given** a `session_state` containing `%ctx.x = "hello"`,
   **When** `iris_execute` appends `Set %ctx.x = %ctx.x _ " world"`,
   **Then** the new `session_state` contains the updated value.

4. **Given** no `session_state` passed,
   **When** `iris_execute` runs normally,
   **Then** behavior is identical to today — no preamble injected, no `%ctx` variable, no overhead.

---

### User Story 2 — Carry %Persistent object identity across calls (Priority: P2)

An AI opens a `%Persistent` object in one call, inspects some properties, then wants
to read or modify different properties in a follow-up call without knowing the ID in advance.

**Why this priority**: `%Persistent` objects are the most common thing an AI would want
to "hold" across calls — patients, messages, production config items.

**Independent Test**: Open a `%Persistent` instance, store it in `%ctx`, pass
`session_state` to the next call — confirm the object is restored and readable.

**Acceptance Scenarios**:

1. **Given** code that sets `%ctx.hdr = ##class(Ens.MessageHeader).%OpenId(1)` with
   `use_session: true`,
   **When** the tool returns,
   **Then** `session_state` encodes the class name and `%Id()` of `%ctx.hdr`.

2. **Given** `session_state` from the prior call,
   **When** `iris_execute` runs with `Write %ctx.hdr.SourceConfigName`,
   **Then** `%ctx.hdr` is a live `Ens.MessageHeader` instance and the correct name is written.

3. **Given** `session_state` containing an OID for a class that doesn't exist in
   the target namespace,
   **When** `iris_execute` runs,
   **Then** the tool returns `SESSION_RESTORE_FAILED` with a clear message naming
   the missing class.

---

### User Story 3 — Accumulate a %DynamicObject across calls (Priority: P3)

An AI is building up a complex data structure incrementally — adding keys across
multiple calls — and wants the accumulation to survive between them.

**Why this priority**: `%ctx` itself is a `%DynamicObject`, so nested objects stored
in it are serialized naturally. This covers reporting, payload assembly, and multi-step
data collection workflows.

**Independent Test**: Add a key to `%ctx` in call 1, add a different key in call 2,
confirm both keys are present in call 2's output.

**Acceptance Scenarios**:

1. **Given** code `Set %ctx.step1 = "done"` with `use_session: true`,
   **When** the tool returns,
   **Then** `session_state` encodes `%ctx.step1`.

2. **Given** `session_state` from the prior call,
   **When** code sets `Set %ctx.step2 = "also done"`,
   **Then** the new `session_state` contains both `step1` and `step2`.

---

### Edge Cases

- What if `%ctx` is not assigned by the user (they never write to it)? The epilogue
  serializes an empty object; next call restores `%ctx = {}`. No error.
- What if a `%ctx` key holds an unsupported type (open `%ResultSet`, device handle)?
  The epilogue's `%ToJSON()` call will fail. The epilogue catches this with a `Try/Catch`
  and returns `SESSION_SERIALIZE_FAILED` with the key name.
- What if `session_state` is corrupted or tampered? `Base64Decode` or `%FromJSON` will
  error; the preamble catches this and returns `SESSION_INVALID` without executing user code.
- What if the AI passes `session_state` from namespace A to a call targeting namespace B?
  OID restore (`$classmethod(cls, "%OpenId", id)`) will return null if the class or row
  doesn't exist — the preamble catches null returns and returns `SESSION_RESTORE_FAILED`
  with the key and class name.
- What if `use_session: false` (default)? No preamble, no epilogue, no `%ctx` injected.
  Behavior is byte-for-byte identical to the current implementation.
- What if `use_session: true` but no `session_state` provided? Preamble injects
  `Set %ctx = {}` and epilogue emits a fresh `session_state`. This is how session 1 starts.

## Requirements

### Functional Requirements

- **FR-001**: `iris_execute` MUST accept an optional boolean `use_session` parameter
  (default `false`). When `true`, preamble and epilogue are injected.
- **FR-002**: `iris_execute` MUST accept an optional string `session_state` parameter:
  a token produced by a prior `iris_execute` call with `use_session: true`.
- **FR-003**: When `use_session: true`, the preamble MUST inject `Set %ctx = {}` if no
  `session_state` is provided, or restore `%ctx` from the token if one is provided.
- **FR-004**: When `use_session: true`, the epilogue MUST serialize `%ctx` to a
  Base64-encoded JSON string and return it as `session_state` in the tool response.
- **FR-005**: The `session_state` string MUST be self-contained — no state written
  to IRIS globals, no background jobs, nothing persisted on the server between calls.
- **FR-006**: `%ctx` keys holding scalar values (strings, numbers) MUST round-trip
  correctly through `session_state`.
- **FR-007**: `%ctx` keys holding `%Persistent` instances MUST be serialized as
  `{"_cls": classname, "_id": id}` stubs. The preamble MUST restore them to live
  objects via `$classmethod(cls, "%OpenId", id)`.
- **FR-008**: `%ctx` keys holding `%DynamicObject` or `%DynamicArray` values MUST
  round-trip via `%ToJSON()` / `%FromJSON()` automatically (they are JSON-native).
- **FR-009**: If epilogue serialization fails (unsupported object type in `%ctx`),
  the tool MUST return error code `SESSION_SERIALIZE_FAILED` naming the failing key.
- **FR-010**: An invalid or unparseable `session_state` MUST return error code
  `SESSION_INVALID` without executing user code.
- **FR-011**: A failed OID restore (`$classmethod` returns null) MUST return error
  code `SESSION_RESTORE_FAILED` naming the key and class.
- **FR-012**: When `use_session: false` (default), `iris_execute` MUST behave
  identically to the current implementation — no preamble, no epilogue, no overhead.
- **FR-013**: The code-edit guard and all existing gates MUST apply before any
  session preamble/epilogue injection.

### Key Entities

- **SessionState**: Base64-encoded JSON string held by the MCP client (never written to
  IRIS). The decoded JSON is the serialized `%ctx` `%DynamicObject`. Persistent object
  values are stored as `{"_cls": classname, "_id": id}` stubs.
- **%ctx**: The injected carrier variable — a `%DynamicObject` available in every
  session-enabled `iris_execute` call. The user stores what they want to persist as
  properties of `%ctx`. It is never visible in non-session calls.

## Success Criteria

### Measurable Outcomes

- **SC-001**: A two-call sequence where call 1 sets a variable and call 2 reads it
  produces the correct value, with no new ObjectScript classes installed on IRIS.
- **SC-002**: A two-call sequence with a `%Persistent` OID restores the object and
  reads a property correctly in call 2.
- **SC-003**: An `iris_execute` call with no `session_state` / `capture` produces
  output byte-for-byte identical to the current implementation.
- **SC-004**: A corrupted `session_state` returns `SESSION_INVALID` without executing
  any user code.
- **SC-005**: The serialization/deserialization round-trip adds less than 50ms
  overhead on a local dev container compared to a plain `iris_execute` call.
