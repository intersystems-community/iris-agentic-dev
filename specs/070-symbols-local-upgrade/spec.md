# Feature Specification: iris_symbols_local Upgrade

**Spec Number**: 070
**Feature Branch**: `070-symbols-local-upgrade`
**Created**: 2026-07-27
**Status**: Draft

## Problem Statement

`iris_symbols_local` scans ObjectScript source files on disk (`.cls`, `.mac`, `.inc`) and
extracts symbols using `tree-sitter-objectscript` grammars. It was introduced as a functional
first pass, but investigation reveals twelve gaps that reduce its usefulness for navigation,
code intelligence, and future codebase-memory-mcp (CBM) graph indexing:

1. The grammar crates are pinned to `1.7`, missing the python body fix from `1.9` (PR #52,
   merged 2026-07-27).
2. Symbols carry no line number, making jump-to-definition impossible.
3. Return types are declared `Option<String>` but always `None`.
4. `FormalSpec` is raw text sliced from the arguments node — not structured per-parameter.
5. Eight of twelve `class_statement` member kinds are silently dropped.
6. Routine names come from the file stem, not the `routine_name` grammar node.
7. Glob matching is case-sensitive; ObjectScript identifiers are case-insensitive.
8. The `query` field filters at class level only; callers cannot filter members within a class.
9. There is no `kinds` filter, so callers cannot request only methods or only properties.
10. A dead `tag_with_params` arm in `extract_routine_nodes` can never fire; the correct node
    kind is `tag_statement`.
11. `docs_introspect` returns `FormalSpec` as a raw string instead of structured `ArgSpec`
    objects, giving callers inconsistent output compared with the tree-sitter path.
12. BPL and DTL classes store all their logic in XData blocks — `docs_introspect` returns
    empty methods/properties for them, and `extract_message_map_routing` returns NOT_FOUND.
    The routing targets, transform mappings, and data-flow graph are invisible to every
    current tool.

These gaps must be closed to make `iris_symbols_local` and its companion tools reliable
sources of structural information for both interactive queries and downstream CBM graph
indexing.

---

## User Stories

### US1 — Bump grammar crates to 1.9

A developer uses `iris_symbols_local` on a class that contains Python-body methods. With crates
at `1.7` the python block is mis-parsed and symbols inside it are lost. After the bump to
`1.9.13` the python body fix from PR #52 applies and those symbols are extracted correctly.

**Acceptance Scenarios**:

1. **Given** `Cargo.toml` specifies `tree-sitter-objectscript = "1.9"` and
   `tree-sitter-objectscript-routine = "1.9"`, **When** `cargo build` runs, **Then** the build
   succeeds with no version conflicts.
2. **Given** a `.cls` file containing `Language = python` method bodies, **When**
   `extract_cls_symbols` is called, **Then** the method symbol is returned (not dropped due to
   a parse error from the python block).

---

### US2 — Line numbers on every symbol

An IDE integration calls `iris_symbols_local` to implement jump-to-definition. Without a line
number it cannot navigate to the symbol. After this change every `Symbol` carries a `line`
field (1-based), populated from the tree-sitter node's `start_position().row + 1`.

**Acceptance Scenarios**:

1. **Given** `MyApp/Foo.cls` with `DoSomething` at a known line, **When**
   `extract_cls_symbols` returns the method symbol, **Then** `symbol.line` equals the correct
   1-based line number.
2. **Given** any symbol (class, method, property, parameter, label, macro), **When**
   serialized to JSON, **Then** the `"line"` key is present and its value is `>= 1`.
3. **Given** the class symbol itself, **When** returned, **Then** `line` points to the `Class`
   keyword row.

---

### US3 — Return types extracted from AST

A caller lists properties in `MyApp.Foo` and needs their types to understand the data model.
Currently `Type` is always `null`. After this change, `Type` is populated from the
`return_type` → `typename` subtree for methods, classmethods, properties, and parameters.

**Acceptance Scenarios**:

1. **Given** `Property Value As %String;`, **When** extracted, **Then** `Type = "%String"`.
2. **Given** `Method DoSomething(...) As %Boolean`, **When** extracted, **Then**
   `Type = "%Boolean"`.
3. **Given** a member with no return type clause, **When** extracted, **Then** `Type` is absent
   from the serialized JSON (`skip_serializing_if = "Option::is_none"`).
4. **Given** `Parameter VERSION = 1;` with no type keyword, **When** extracted, **Then**
   `Type` is absent.

---

### US4 — Structured FormalSpec

A caller needs to know whether a parameter is passed by reference, its type, and its default
value in order to generate call stubs. Currently `FormalSpec` is raw text. After this change
`FormalSpec` is a JSON array of per-argument objects produced by walking
`arguments → argument → method_arg`.

Each argument object:

```json
{
  "name": "pName",
  "type": "%String",
  "byref": false,
  "output": false,
  "default": "\"hello\""
}
```

Fields `type`, `byref`, `output`, and `default` are omitted when absent/false.

**Acceptance Scenarios**:

1. **Given** `Method Foo(pName As %String = "hello", ByRef pRef As %Integer)`, **When**
   extracted, **Then** `FormalSpec` is an array with two elements: first has `name="pName"`,
   `type="%String"`, `default="\"hello\""` ; second has `name="pRef"`, `type="%Integer"`,
   `byref=true`.
2. **Given** a method with no arguments `()`, **When** extracted, **Then** `FormalSpec` is
   absent or an empty array.
3. **Given** a classmethod with `Output` keyword on a parameter, **When** extracted, **Then**
   the argument object has `output: true`.

---

### US5 — All twelve member kinds extracted

A caller indexes the full structure of an Ensemble production class and needs `query`,
`trigger`, `index`, `xdata`, `storage`, `relationship`, `foreignkey`, and `projection` members
in the symbol list, in addition to `method`, `classmethod`, `property`, and `parameter`.

**Acceptance Scenarios**:

1. `Index ByName On Name;` → symbol with `kind = "index"`
2. `XData DefaultData { ... }` → `kind = "xdata"`
3. `Query AllItems() As %SQLQuery { ... }` → `kind = "query"`
4. `Trigger OnInsert After Insert {}` → `kind = "trigger"`
5. `Relationship Parent As MyApp.Parent [...]` → `kind = "relationship"`
6. `storage`, `foreignkey`, `projection` each produce a symbol from a well-formed fixture.

---

### US6 — Routine name from grammar node

A routine file has `ROUTINE Utils [Type=MAC]` as its first line. Currently the routine name
comes from the file stem (`Utils`), which fails when stem and declared name diverge (renamed
file, versioned include). After this change the name is extracted from
`routine_definition → routine_name`; the file stem is the fallback when that node is absent.

**Acceptance Scenarios**:

1. `Utils.mac` with `ROUTINE Utils` → routine name is `"Utils"` (from grammar).
2. A `.mac` file with no `ROUTINE` header → file stem used as fallback.
3. `MyRoutine_v2.mac` containing `ROUTINE MyRoutine` → label names use `"MyRoutine"`.

---

### US7 — Case-insensitive glob matching

An agent sends `query = "myapp.*"` expecting to match the `MyApp` package. ObjectScript
identifiers are case-insensitive so the query should match regardless of case. After this
change both sides are uppercased before the glob comparison.

**Acceptance Scenarios**:

1. `glob_match("myapp.*", "MyApp.Foo")` → `true`
2. `glob_match("MYAPP.FOO", "MyApp.Foo")` → `true`
3. `glob_match("MyApp.*", "MYAPP.BAR")` → `true`
4. `glob_match("Other.*", "MyApp.Foo")` → `false`

---

### US8 — Member-level glob filter

A caller passes `query = "MyApp.Foo.Do*"` expecting only methods starting with `Do`. Currently
the query is matched against the class name only, returning all members. After this change the
final dot-separated segment is a member glob applied when emitting member symbols.

**Parsing rule**: if the query has more than one dot segment and the final segment contains no
dot, treat everything before the last dot as the class pattern and the last segment as the
member glob.

**Acceptance Scenarios**:

1. `query = "MyApp.Foo.Do*"` on a class with `DoSomething`, `DoOther`, `Helper` → returns
   only `DoSomething` and `DoOther` (not `Helper`, not the class symbol itself).
2. `query = "MyApp.Foo.*"` → all members plus the class symbol.
3. `query = "MyApp.*"` → full class + all members (backward-compatible).

---

### US9 — `kinds` filter parameter

A caller building a method-completion list only wants `method` and `classmethod` symbols.
Receiving properties, parameters, indices, etc. wastes tokens. After this change an optional
`kinds: Option<Vec<String>>` field on `SymbolsLocalParams` drops any symbol whose `kind` is
not in the list.

**Acceptance Scenarios**:

1. `kinds = ["method", "classmethod"]` on a class with one method, one property, one
   parameter → only the method symbol returned.
2. `kinds` absent or empty → all kinds returned (backward-compatible).
3. `kinds = ["index"]` on a methods-only class → empty result.

---

### US11 — Structured FormalSpec in `docs_introspect`

`docs_introspect` returns `FormalSpec` as a raw string from `%Dictionary.CompiledMethod`
(e.g. `pName:%String="hello",ByRef pRef:%Integer`). After this change, `docs_introspect`
parses that string using the same `ArgSpec` struct defined in US4, so callers get identical
structured output regardless of whether they used `iris_symbols_local` (AST path) or
`docs_introspect` (compiled dictionary path).

The parser lives in `symbols_local.rs` as `pub fn parse_formalspec_string(s: &str) ->
Vec<ArgSpec>` and handles the `%Dictionary` wire format: comma-separated args,
`ByRef`/`Output` prefixes, `type` after `:`, default after `=`.

**Before implementing**: check `hkimura-intersys/objectscript-lsp` (Rust, `tower-lsp` +
tree-sitter) — it indexes `.cls`/`.mac`/`.inc` and almost certainly parses FormalSpec
strings already. If a compatible parser exists there, align on the same format or extract
shared logic rather than reimplementing. The `ArgSpec` field names and serde shape defined
in US4 are the target contract; the implementation may come from upstream.

**Acceptance Scenarios**:

1. **Given** `FormalSpec = "pName:%String=\"hello\",ByRef pRef:%Integer"`, **When** parsed,
   **Then** result matches the US4 scenario 1 `ArgSpec` array exactly.
2. **Given** `FormalSpec = ""` (no args), **When** parsed, **Then** result is an empty array.
3. **Given** `docs_introspect` called on a class with typed methods, **When** response
   returned, **Then** `FormalSpec` field is a JSON array of `ArgSpec` objects, not a raw
   string.
4. **Given** a `FormalSpec` string with an `Output` prefix, **When** parsed, **Then**
   `output: true` appears on that arg.

---

### US10 — Remove dead `tag_with_params` arm

The `"tag_with_params"` match arm in `extract_routine_nodes` (~lines 469–486) can never fire:
`tag_with_params` is always a child of `tag_statement`, never a direct sibling. Removing it
eliminates a misleading code path and any associated lint suppression.

**Acceptance Scenarios**:

1. Labels still extracted from `Utils.mac` after arm removal.
2. `cargo clippy` shows no new `dead_code` warnings in `symbols_local.rs`.

---

### US12 — XData content in `docs_introspect` for BPL and DTL classes

`docs_introspect` currently returns empty `methods` and `properties` arrays for BPL and DTL
classes because all logic lives in XData blocks, not compiled methods. After this change,
`docs_introspect` detects BPL/DTL classes and returns a structured `xdata_flow` field
alongside the existing output.

**BPL output** — parsed from the `<process>` XML in the `BPL` XData block:

```json
{
  "xdata_flow": {
    "kind": "bpl",
    "request": "IRISDemo.BS.AppTrigger.TriggerEventReq",
    "response": "Ens.Response",
    "context_properties": [{ "name": "HL7Message", "type": "EnsLib.HL7.Message" }],
    "steps": [
      { "step": "code", "name": "Transform Obj to HL7" },
      {
        "step": "call",
        "name": "Send HL7 to Readmission Srv",
        "target": "Readmission Risk HL7 Service",
        "async": true,
        "request_type": "EnsLib.HL7.Message",
        "response_type": "Ens.Response"
      }
    ]
  }
}
```

**DTL output** — parsed from the `<transform>` XML in the `DTL` XData block:

```json
{
  "xdata_flow": {
    "kind": "dtl",
    "source_class": "IRISDemo.Data.Encounter",
    "target_class": "EnsLib.HL7.Message",
    "target_doc_type": "2.5:ADT_A03",
    "subtransforms": [
      { "class": "IRISDemo.DTL.HL7AppTrigger.Sub.MSH", "target_segment": "MSH" },
      { "class": "IRISDemo.DTL.HL7AppTrigger.Sub.PID", "target_segment": "PID" },
      { "class": "IRISDemo.DTL.HL7AppTrigger.Sub.PV1", "target_segment": "PV1" }
    ],
    "assign_count": 4
  }
}
```

**Detection logic**: a class is BPL if its superclass chain includes `Ens.BusinessProcessBPL`
(check `%Dictionary.CompiledClass.Super`); DTL if it includes `Ens.DataTransformDTL`. Both
are detectable without reading source — the compiled superclass is available via the
Dictionary API already used by `docs_introspect`.

**XData retrieval**: use `$system.OBJ.ExportToStream` to get the full class XML export, then
extract the XData CDATA block. Parse the XML in Rust using the `quick-xml` crate (already a
transitive dep via other tooling; add directly if not present).

**Acceptance Scenarios**:

1. **Given** `docs_introspect` called on a BPL class, **When** response returned, **Then**
   `xdata_flow.kind = "bpl"`, `xdata_flow.request` is the request class, and `xdata_flow.steps`
   lists every `<call>` and `<code>` element in order, with `target` populated for `<call>`
   steps.
2. **Given** a BPL with a `<call>` that has `async='1'`, **When** parsed, **Then**
   `xdata_flow.steps[n].async = true`.
3. **Given** `docs_introspect` called on a DTL class, **When** response returned, **Then**
   `xdata_flow.kind = "dtl"`, `source_class` and `target_class` are correct, and
   `subtransforms` lists every `<subtransform>` with `class` and `target_segment`.
4. **Given** a non-BPL, non-DTL class, **When** `docs_introspect` called, **Then** no
   `xdata_flow` key appears in the response (backward-compatible).
5. **Given** a BPL class with `<sequence>` containing nested `<if>` or `<foreach>` elements,
   **When** parsed, **Then** steps includes those elements with `step` = `"if"` / `"foreach"`
   and their children flattened one level (nested sequences are not recursed further in v1).

---

### US13 — BPL routing in `extract_message_map_routing`

`extract_message_map_routing` currently returns `NOT_FOUND` for BPL classes because it only
looks for a `MessageMap` XData block (present in router/service classes, absent in BPL).
After this change, BPL classes return a routing summary derived from their `<call>` steps —
consistent with what US12 puts in `docs_introspect`, but shaped as routing targets the way
the tool already returns them for router classes.

**Output shape** (mirrors existing routing output format):

```json
{
  "class": "IRISDemo.BP.HL7AppTrigger.Process",
  "kind": "bpl",
  "routes": [
    {
      "target": "Readmission Risk HL7 Service",
      "request_type": "EnsLib.HL7.Message",
      "async": true,
      "step_name": "Send HL7 to Readmission Srv"
    }
  ],
  "note": "BPL routing is derived from <call> steps; dynamic $classmethod dispatch in <code> blocks is not statically resolvable."
}
```

The `note` field is included whenever the BPL contains `<code>` blocks that invoke
`$classmethod` or other dynamic dispatch — because those targets cannot be statically
extracted. This prevents callers from assuming the routes list is exhaustive.

**Acceptance Scenarios**:

1. **Given** `extract_message_map_routing` called on a BPL class with one `<call>` step,
   **When** response returned, **Then** `kind = "bpl"` and `routes` contains one entry with
   the correct `target`.
2. **Given** a BPL with multiple `<call>` steps (including async), **When** parsed, **Then**
   `routes` has one entry per `<call>`, each with correct `target`, `request_type`, and
   `async` flag.
3. **Given** a BPL whose `<code>` blocks contain `$classmethod` calls, **When** parsed,
   **Then** response includes a `note` field warning about dynamic dispatch.
4. **Given** a non-BPL class that already has a `MessageMap`, **When** called, **Then**
   existing behavior is unchanged.
5. **Given** `extract_message_map_routing` called on a DTL class, **When** response returned,
   **Then** `kind = "dtl"`, `source_class` and `target_class` populated, `routes` empty
   (DTLs transform, they do not route).

---

## Output Schema

The upgraded `Symbol` struct serializes as follows. Optional fields are omitted when absent.

```json
{
  "Name": "MyApp.Foo.DoSomething",
  "kind": "classmethod",
  "file": "MyApp/Foo.cls",
  "line": 8,
  "Type": "%Boolean",
  "FormalSpec": [{ "name": "name", "type": "%String" }]
}
```

### Fields

| JSON key     | Rust field    | Type                   | Notes                              |
| ------------ | ------------- | ---------------------- | ---------------------------------- |
| `Name`       | `name`        | `String`               | Qualified name (see CBM Alignment) |
| `kind`       | `kind`        | `String`               | One of 15 kind labels below        |
| `file`       | `file`        | `String`               | Relative path from workspace root  |
| `line`       | `line`        | `u32`                  | 1-based; always present            |
| `Type`       | `type_name`   | `Option<String>`       | Omitted when absent                |
| `FormalSpec` | `formal_spec` | `Option<Vec<ArgSpec>>` | Omitted when absent/empty          |

### Kind labels (15 total)

```text
class
method
classmethod
property
parameter
query
trigger
index
xdata
storage
relationship
foreignkey
projection
label
macro
```

### `ArgSpec` sub-object

```json
{ "name": "pRef", "type": "%Integer", "byref": true, "default": "42" }
```

| Field     | Type             | Omit when |
| --------- | ---------------- | --------- |
| `name`    | `String`         | never     |
| `type`    | `Option<String>` | absent    |
| `byref`   | `bool`           | `false`   |
| `output`  | `bool`           | `false`   |
| `default` | `Option<String>` | absent    |

---

## CBM Alignment Note

Spec 071 builds codebase-memory-mcp graph tools that index ObjectScript symbols. To ensure
structural consistency, `iris_symbols_local` output uses the same qualified name format as CBM:

**Qualified name format**:

- Class: `Package.ClassName` (e.g. `MyApp.Foo`)
- Class member: `Package.ClassName.MemberName` (e.g. `MyApp.Foo.DoSomething`)
- Routine label: `RoutineName:LabelName` (e.g. `Utils:Start`)
- Macro: unqualified identifier (e.g. `VERSION`)

**Kind label → CBM node label mapping**:

| `iris_symbols_local` kind | CBM node label                |
| ------------------------- | ----------------------------- |
| `class`                   | `ObjClass`                    |
| `method`                  | `ObjMethod`                   |
| `classmethod`             | `ObjMethod` (`isClass: true`) |
| `property`                | `ObjProperty`                 |
| `parameter`               | `ObjParameter`                |
| `query`                   | `ObjQuery`                    |
| `trigger`                 | `ObjTrigger`                  |
| `index`                   | `ObjIndex`                    |
| `xdata`                   | `ObjXData`                    |
| `storage`                 | `ObjStorage`                  |
| `relationship`            | `ObjRelationship`             |
| `foreignkey`              | `ObjForeignKey`               |
| `projection`              | `ObjProjection`               |
| `label`                   | `RoutineLabel`                |
| `macro`                   | `IncMacro`                    |

This spec defines the output schema. CBM integration is out of scope here — see spec 071.

---

## Implementation Notes

### US1 — Bump crates

In `crates/iris-agentic-dev-core/Cargo.toml`:

```toml
tree-sitter-objectscript = "1.9"
tree-sitter-objectscript-routine = "1.9"
```

Run `cargo update` and verify `Cargo.lock` pins `1.9.13`. No source changes needed.

### US2 — Line numbers

Add `pub line: u32` with `#[serde(rename = "line")]` to `Symbol`. Populate from
`node.start_position().row as u32 + 1` at each symbol construction site.

### US3 — Return types

Write `fn extract_typename(node: Node, source: &[u8]) -> Option<String>` that walks a node's
children for a `return_type` child, then descends into `typename` for the text. Call from
`extract_method_symbol`, `extract_property_symbol`, and `extract_parameter_symbol`.

### US4 — Structured FormalSpec

Replace `formal_spec: Option<String>` with `formal_spec: Option<Vec<ArgSpec>>`. Add:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ArgSpec {
    pub name: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub byref: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub output: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}
```

Write `fn parse_arguments(node: Node, source: &[u8]) -> Vec<ArgSpec>` walking
`arguments → argument` children, extracting name, type, byref, output, default.

### US5 — All member kinds

Extend `match member.kind()` in `extract_cls_members` to cover all twelve kinds. Add a
helper `fn extract_member_name(node: Node, source: &[u8]) -> Option<String>` that looks for
a `<kind>_name` child (e.g. `query_name`, `trigger_name`, `index_name`).

### US6 — Routine name from grammar node

In `extract_routine_symbols`, after parsing walk `root` for
`routine_definition → routine_name`. Use `node_text` on that leaf as the routine name; fall
back to the file stem when absent.

### US7 — Case-insensitive glob

At the top of `glob_match`:

```rust
let query = query.to_uppercase();
let name = name.to_uppercase();
```

No signature change.

### US8 — Member-level glob filter

Add `fn split_member_query(query: &str) -> (String, Option<String>)` that splits on the last
dot when the query has 2+ segments and the final segment contains no dot. Apply the member
glob as a secondary filter in `extract_cls_members`.

### US9 — `kinds` filter

Add `kinds: Option<Vec<String>>` to `SymbolsLocalParams` (with `#[serde(default)]`). Update
`scan_workspace` signature to accept `kinds: Option<&[String]>`. Filter after each symbol
construction. Update all call-sites to pass `None` or the new field.

### US11 — Structured FormalSpec in `docs_introspect`

Add `pub fn parse_formalspec_string(s: &str) -> Vec<ArgSpec>` to `symbols_local.rs`. The
`%Dictionary` wire format is comma-separated args of the form
`[ByRef|Output ]name[:type][=default]`. Handle quoted defaults (`"hello"`) and empty
strings. Call from `docs_introspect` in `mod.rs` after fetching `FormalSpec` from
`%Dictionary.CompiledMethod`: replace the raw string value with the parsed array in the
JSON response.

### US10 — Remove dead arm

Delete the `"tag_with_params"` match arm (~lines 469–486 in `symbols_local.rs`) and any
associated `allow` annotation. Run `cargo clippy` to confirm clean.

### US12 — XData content in `docs_introspect`

**New module**: `crates/iris-agentic-dev-core/src/tools/xdata_flow.rs`

```rust
pub enum XDataFlow {
    Bpl(BplFlow),
    Dtl(DtlFlow),
}

pub struct BplFlow {
    pub request: String,
    pub response: String,
    pub context_properties: Vec<ContextProperty>,
    pub steps: Vec<BplStep>,
}

pub enum BplStep {
    Code { name: String },
    Call { name: String, target: String, async_: bool,
           request_type: Option<String>, response_type: Option<String> },
    If { name: String, condition: String, steps: Vec<BplStep> },
    ForEach { name: String, property: String, steps: Vec<BplStep> },
    Other { kind: String, name: String },
}

pub struct DtlFlow {
    pub source_class: String,
    pub target_class: String,
    pub target_doc_type: Option<String>,
    pub subtransforms: Vec<Subtransform>,
    pub assign_count: usize,
}
```

**Detection in `mod.rs`** (`docs_introspect` handler): after fetching the class from
`%Dictionary.CompiledClass`, check `Super` for `Ens.BusinessProcessBPL` or
`Ens.DataTransformDTL`. If detected, call
`iris_execute("$system.OBJ.ExportToStream(...)")` to get the XML, extract the CDATA from
the matching `<XData name="BPL">` or `<XData name="DTL">` block, parse with `quick-xml`,
and serialize to `xdata_flow` in the JSON response.

**XML parsing**: use `quick-xml` reader in pull-mode. BPL: start at `<process>`, walk
`<sequence>` children recursively up to one level of nesting. DTL: start at `<transform>`,
collect `<subtransform>` and count `<assign>` elements.

**`$classmethod` detection**: after parsing `<code>` CDATA text, check for the string
`$classmethod` — if found, set `has_dynamic_dispatch: true` on `BplFlow`.

**Crate dependency**: add `quick-xml = "0.37"` to `Cargo.toml` if not already present.

### US13 — BPL routing in `extract_message_map_routing`

In `mod.rs`, in the `extract_message_map_routing` handler, after the existing
`MessageMap`-not-found path: detect BPL/DTL via superclass (same check as US12). For BPL,
call the same `XDataFlow::parse_bpl(...)` function from `xdata_flow.rs` (shared with US12),
collect `BplStep::Call` entries into `routes`, set `kind = "bpl"`, and include `note` if
`has_dynamic_dispatch`. For DTL, return `kind = "dtl"` with source/target classes and empty
routes.

All XML parsing logic lives in `xdata_flow.rs` — `docs_introspect` and
`extract_message_map_routing` both call into it. No duplication.

---

## Test Strategy

Tests are written before implementation (test-first). Each test must **fail** before the
corresponding implementation begins and **pass** after.

**Test files**:

- `crates/iris-agentic-dev-core/tests/symbols_local_tests.rs` — integration tests against
  fixtures
- Inline `#[cfg(test)]` in `symbols_local.rs` — unit tests for `glob_match`,
  `parse_arguments`, `split_member_query`, `parse_formalspec_string`

**Required new fixtures** (`tests/fixtures/`):

- `MyApp/PythonBody.cls` — `Language = python` method body (US1)
- `MyApp/TypedMembers.cls` — methods/properties with explicit type annotations (US3)
- `MyApp/FormalSpec.cls` — method with ByRef, Output, typed, and defaulted parameters (US4)
- `MyApp/AllMembers.cls` — one definition of each of the eight new member kinds (US5)
- `NamedRoutine.mac` — `ROUTINE NamedRoutine` header for stem-mismatch test (US6)

**Additional new test file**:

- `crates/iris-agentic-dev-core/tests/xdata_flow_tests.rs` — unit tests for BPL/DTL XML
  parsing against fixture XML strings; `#[ignore]` integration tests against live container

**Required new fixtures for US12/US13** (`tests/fixtures/xdata/`):

- `bpl_simple.xml` — minimal BPL `<process>` with one `<code>` and one `<call>` step
- `bpl_dynamic.xml` — BPL `<code>` block containing `$classmethod` dispatch
- `bpl_nested.xml` — BPL with `<if>` and `<foreach>` wrapping inner `<call>` steps
- `dtl_simple.xml` — minimal DTL `<transform>` with two `<subtransform>` and three
  `<assign>` elements

**Test coverage target**: all new tests (T070-01 through T070-52) plus zero regressions on
the existing 65 tests. T070-36 through T070-39 cover `parse_formalspec_string` (US11) and
T070-43 through T070-52 cover US12/US13; the `#[ignore]` integration tests in those ranges
require a live IRIS container.

---

## Non-Goals

- No CBM integration — this spec does not implement any graph writes or CBM tool calls.
- No new MCP tools — `iris_symbols_local`, `docs_introspect`, and
  `extract_message_map_routing` tool names and transport are unchanged.
- Live IRIS required only for US11/US12/US13 integration tests — all other tests run against
  files on disk.
- No `.int` file support — compiled intermediate files remain excluded.
- No cross-file type resolution — `Type` is verbatim text from source, not a catalog lookup.
- No streaming output — tools continue to return a single JSON response.
- No deep `<code>` block analysis — ObjectScript inside `<code>` CDATA is not parsed for
  dynamic dispatch targets beyond detecting that `$classmethod` is present.
- No recursive BPL nesting beyond one level — deeply nested sequences (`<if>` inside
  `<foreach>` inside `<sequence>`) are reported at the outer level with `kind = "other"`.
- No `<rule>` or `<switch>` routing rule evaluation — BPL conditional routing that depends
  on runtime values is not statically resolved; the `note` field covers this gap.
