# Implementation Plan: iris_execute Session State

**Branch**: `071-execute-session` | **Date**: 2026-07-30 | **Spec**: [spec.md](spec.md)

## Summary

Add `capture` and `session_state` parameters to `iris_execute`. When `capture` is
provided, the generated ObjectScript class includes a serialization epilogue that
encodes named variables to a JSON string, which is returned as `session_state` in the
tool response. When `session_state` is provided, a deserialization preamble is injected
before user code that restores those variables. No state is written to IRIS — the token
lives in the MCP client between calls.

## Technical Context

**Language/Version**: Rust 2021 edition (workspace version)
**Primary Dependencies**: `serde_json` (workspace), `base64` (already workspace), no new crates needed
**Storage**: N/A — state is a client-held string, nothing written to IRIS
**Testing**: `cargo test` (unit, no IRIS) + `cargo test -- --include-ignored` (integration, live IRIS)
**Target Platform**: macOS arm64/x86_64, Linux x86_64, Windows x86_64 (existing targets)
**Performance Goals**: < 50ms round-trip overhead for serialization/deserialization on local dev container
**Constraints**: No new IRIS classes installed; no globals written; must be backward-compatible (no-arg path unchanged)
**Scale/Scope**: Modifies one tool (`iris_execute`); no schema changes to other tools

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Zero-Install Binary | PASS | State token is client-side; no IRIS class install |
| II. ObjectScript Sanity | NEEDS VERIFICATION | `$classname()`, `%Id()`, `%OpenId()`, `%ToJSON()`, `%FromJSON()` must be verified against live IRIS |
| III. HTTP-First Execution | PASS | All changes inside `execute_via_generator`; no Docker path |
| IV. Test-First, Fixture-Driven | PASS | Unit tests use `None` iris; integration tests `#[ignore]` |
| V. Output Shape Parity | PASS | Additive only — `session_state` and `session_warnings` are new optional fields |
| VI. Environment Guard | PASS | `iris_execute` is already read-capable; no new write surface added |
| VII. Dependency Minimalism | PASS | `serde_json` + `base64` already in workspace; no new crates |
| VIII. 90% Coverage Gate | PASS | Polish phase includes coverage-check task |
| IX. Tool Lift Requirement | PASS | Benchmark tasks defined in research.md; lift ≥ +0.20 required |
| X. ObjectScript Coverage | N/A | No new ObjectScript shipped to IRIS; generated code is ephemeral |

## Phase 0 Research

### API Verification

ObjectScript APIs used in generated preamble/epilogue code — must be verified against live IRIS:

| Expression | Purpose | Verification status |
|------------|---------|-------------------|
| `$classname(obj)` | Detect class of variable | NEEDS VERIFICATION |
| `##class(%Library.Persistent).%IsA(obj)` | Is %Persistent subclass | NEEDS VERIFICATION |
| `obj.%Id()` | Get OID of %Persistent | Known valid |
| `##class(ClassName).%OpenId(id)` | Restore %Persistent | Known valid |
| `obj.%IsA("%DynamicAbstractObject")` | Is %DynamicObject or %DynamicArray | NEEDS VERIFICATION |
| `obj.%ToJSON()` | Serialize dynamic object to string | Known valid |
| `##class(%DynamicObject).%FromJSON(str)` | Deserialize JSON string | Known valid |
| `$isobject(varname)` | Is variable an object | Known valid |
| `$data(varname)` | Is variable set | Known valid |
| `+varname = varname` | Is variable numeric | Known valid |

### API Verification Results (live IRIS 2026.2)

All APIs verified against `iris-dev-iris` (IRIS 2026.2 community):

| Expression | Result |
|------------|--------|
| `$system.Encryption.Base64Encode(str)` | Works, produces standard Base64 |
| `$system.Encryption.Base64Decode(str)` | Works, round-trips correctly |
| `##class(%DynamicObject).%FromJSON(str)` | Works |
| `obj.%ToJSON()` | Works on `%DynamicObject`; fails on `%Persistent` — epilogue must re-stub first |
| `obj.%IsA("%Library.Persistent")` | Works as instance method |
| `obj.%IsA("%Library.DynamicAbstractObject")` | Works |
| `$classmethod(cls, "%OpenId", id)` | Works for dynamic dispatch; `##class(@var)` is NOT valid syntax |
| `$classname(obj)` | Works |
| `obj.%Id()` | Works on `%Persistent` |
| `$L(str, sep)` / `$P(str, sep, n)` | Works; `$LENGTH`/`$PIECE` unabbreviated do NOT compile |
| `@varName` indirection | **Does NOT see local variables set with `Set x = value`** in objectgenerator context — creates/reads a separate symbol table entry. This rules out capture-by-name designs. |
| `%ctx` as carrier variable | Works — preamble sets `Set %ctx = {}`, user writes to `%ctx.key`, epilogue serializes `%ctx` directly (no indirection needed) |

### Serialization Design

**Token format**: the `session_state` token is `Base64Encode(%ctx.%ToJSON())`. `%ctx` is
a `%DynamicObject`, so its JSON serialization is direct. `%Persistent` values are
pre-converted to `{"_cls": classname, "_id": id}` stubs before `%ToJSON()` is called
(the epilogue detects live objects via `%IsA("%Library.Persistent")` and replaces them).

**No metadata envelope needed**: the token IS the JSON of `%ctx`. The Rust layer wraps
it in Base64 for transport safety (quotes, newlines). Version tagging can be added later
by convention (`Set %ctx.zVersion = 1`) without a schema change.

### Generated Code Shape (verified working)

**Preamble — no incoming state** (fresh session):
```objectscript
Set %ctx = {}
```

**Preamble — with incoming `session_state`** (Rust substitutes the literal token):
```objectscript
Set zToken = "BASE64TOKEN"
Try {
    Set %ctx = ##class(%DynamicObject).%FromJSON($system.Encryption.Base64Decode(zToken))
} Catch zEx { Write "__SESSION_INVALID__:", zEx.DisplayString(), ! Quit }
Kill zToken
// Scan for _cls/_id stubs and restore to live objects (two-pass to avoid mutate-while-iterate):
Set zToRestore = []
Set zIter = %ctx.%GetIterator()
While zIter.%GetNext(.zK, .zV) {
    If $isobject(zV) && zV.%IsDefined("_cls") { Do zToRestore.%Push(zK) }
}
Kill zIter, zK, zV
Set zI = 0
While zI < zToRestore.%Size() {
    Set zK = zToRestore.%Get(zI)
    Set zStub = %ctx.%Get(zK)
    Set zCls = zStub."_cls"  Set zId = zStub."_id"
    Set zObj = $classmethod(zCls, "%OpenId", zId)
    If '$isobject(zObj) { Write "__SESSION_RESTORE_FAILED__:", zK, ":", zCls, ! Quit }
    Do %ctx.%Set(zK, zObj)
    Set zI = zI + 1
}
Kill zToRestore, zI, zK, zStub, zCls, zId, zObj
```

**Epilogue** (appended after user code when `use_session: true`):
```objectscript
// Scan for any live %Persistent objects and re-stub before serializing (two-pass):
Set zToStub = []
Set zIter = %ctx.%GetIterator()
While zIter.%GetNext(.zK, .zV) {
    If $isobject(zV) && zV.%IsA("%Library.Persistent") { Do zToStub.%Push(zK) }
}
Kill zIter, zK, zV
Set zI = 0
While zI < zToStub.%Size() {
    Set zK = zToStub.%Get(zI)
    Set zV = %ctx.%Get(zK)
    Do %ctx.%Set(zK, {"_cls": ($classname(zV)), "_id": (zV.%Id())})
    Set zI = zI + 1
}
Kill zToStub, zI, zK, zV
Try {
    Write "__SESSION_STATE__:", $system.Encryption.Base64Encode(%ctx.%ToJSON()), !
} Catch zEx { Write "__SESSION_SERIALIZE_FAILED__:", zEx.DisplayString(), ! }
```

**Note on epilogue key walk**: `%ctx.%Set(zK, value)` is the correct dynamic-key setter
on `%DynamicObject` — `Set %ctx.zK = value` would set a literal property named `"zK"`.
Modifying the object while iterating causes silent empty output, so two passes are used:
first pass collects keys needing re-stub into a `%DynamicArray`, second pass replaces them.
Both approaches verified working against IRIS 2026.2.

The Rust output parser strips sentinel lines (`__SESSION_STATE__:`, `__SESSION_INVALID__:`,
`__SESSION_RESTORE_FAILED__:`, `__SESSION_SERIALIZE_FAILED__:`) from visible output before
returning to the MCP client, and maps them to `session_state` / error codes in the response.

### Benchmark Tasks

Two tasks defined for lift measurement:

**task: session-001** — Carry computed value across calls
- Challenge: "The patient count query returned 1247 earlier. Without rerunning it, use that value to calculate what 5% of the patient population is."
- Baseline: agent must rerun the count query or hardcode the number
- With feature: agent passes `session_state` containing the count variable

**task: session-002** — Multi-step object inspection
- Challenge: "Open patient ID 1 and tell me their name, then in a second call tell me their date of birth."
- Baseline: agent reopens the object in the second call (works but is redundant)
- With feature: agent captures the object OID and restores it in the second call without knowing the ID in advance

## Project Structure

### Documentation (this feature)

```text
specs/071-execute-session/
├── plan.md              # This file
├── research.md          # Phase 0 output (API verification results)
├── data-model.md        # SessionState schema + new error codes
├── quickstart.md        # Usage examples for the feature
├── lift-results.md      # Benchmark results (required before merge)
├── checklists/
│   └── requirements.md  # Spec quality checklist
└── tasks.md             # Phase 2 output
```

### Source Code

```text
crates/iris-agentic-dev-core/src/tools/
└── execute.rs           # iris_execute handler — primary change

crates/iris-agentic-dev-core/src/tools/
└── execute_session.rs   # New: session state serialization/deserialization + code generation

crates/iris-agentic-dev-core/tests/
├── integration/
│   └── test_e2e.rs      # New e2e tests for session round-trips (3 tests, #[ignore])
└── unit/
    └── test_execute_session.rs  # New: unit tests for state encoding/decoding without IRIS
```

## Implementation Strategy

1. **Unit layer first**: `execute_session.rs` — pure Rust encoding/decoding of
   `SessionState` struct. No IRIS, no HTTP. Full unit test coverage.
2. **Code generation**: functions that produce preamble/epilogue ObjectScript strings
   given a `SessionState` value or a capture list. Unit-testable by comparing string output.
3. **Integration into `execute.rs`**: wire `capture` and `session_state` parameters,
   call the generators, parse sentinel lines from output, return `session_state` in response.
4. **E2E tests**: three `#[ignore]` tests against live IRIS — scalar round-trip, persistent
   OID round-trip, corrupted state returns `SESSION_INVALID`.
5. **Lift measurement**: run benchmark tasks before marking feature complete.
