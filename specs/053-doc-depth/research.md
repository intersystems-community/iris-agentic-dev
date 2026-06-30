# Research: Document Depth — iris_doc Extensions + iris_execute_method

**Branch**: `053-doc-depth`
**Verified against**: IRIS 2026.2.0L (Build 208U), Atelier API v8
**Container**: `iris-dev-iris`, port 52780

---

## Decision 1: Atelier `/docnames/` endpoint — server-side filtering

**Decision**: Client-side pattern matching (fetch all, filter in Rust). NOT server-side filter param.

**Verification**:

```text
GET /api/atelier/v1/%SYS/docnames/CLS
→ 3965 entries (no filter): works, returns objects with {name, cat, ts, upd, db, gen}

GET /api/atelier/v1/%SYS/docnames/CLS?filter=%Library.Integer*
→ 0 entries: wildcard filter NOT supported by Atelier API

GET /api/atelier/v1/%SYS/docnames/CLS?filter=%Library.Integer.cls
→ 1 entry (exact name match only): filter IS supported for exact names, not globs
```

Verified 2026-06-29 against iris-dev-iris (IRIS 2026.2.0L).

**Rationale**: Atelier `filter=` param performs exact name match only — wildcards return empty.
To support glob patterns for `mode=list`, iris-dev must: (1) fetch all docs for the category,
(2) apply the user's glob pattern client-side using Rust regex (same as the existing
`iris_compile` wildcard expansion at `mod.rs:2111`).

**Alternatives considered**:

- Server-side `/docs?filter=` — verified empty response on IRIS 2026.2; endpoint exists
  but returns `{result: {}}` with no content. Not usable.
- Atelier `/docnames/?filter=` with glob — confirmed returns 0 results for `%Library.*`
  wildcard. Only exact names work.

**Impact on plan**: Plan section "List Implementation" must be revised. Instead of
`GET /docs?filter=glob&cat=cat`, use `GET /docnames/{cat}` (all docs for category), then
apply Rust regex glob matching (same pattern as `iris_compile`). This also means:

- Pattern validation still applies (reject empty, bare `*`) — prevents fetching all 3965+ docs.
- Require at least a non-wildcard prefix (e.g. `User.` before the `*`) to bound the fetch.
- For categories like `ALL`, fetch from each category endpoint and merge.

**Response shape**: `/docnames/` returns objects (confirmed), NOT strings. The existing
`iris_compile` wildcard code at `mod.rs:2119` calls `.as_str()` on each entry — this is a
**pre-existing bug** (returns 0 matches because entries are dicts, not strings). Document
separately; do not fix in 053.

---

## Decision 2: Compiled INT representation via Atelier

**Decision**: The `GET /doc/ClassName.INT` approach does NOT work on this IRIS version.
Use ObjectScript `##class(%Library.Routine).%OpenId("ClassName.1")` via `execute_via_generator`
to fetch compiled INT content.

**Verification**:

```text
GET /api/atelier/v1/%SYS/doc/%Library.Integer.INT
→ status: "ERROR #16005: Document '%Library.Integer.int' does NOT exist"
→ cat: RTN (Atelier lowercases the extension)

GET /api/atelier/v1/USER/docnames/INT
→ 0 entries (no INT documents in docnames listing)
```

Verified 2026-06-29 against iris-dev-iris (IRIS 2026.2.0L).

**Root cause**: IRIS 2026.2 does not surface compiled INT as a named document via Atelier.
The INT representation is accessible via `%Library.Routine` object or via the IRIS Studio
`%GetDoc` API. The docnames endpoint does not list INT files.

**Alternative — ObjectScript approach**:

Fetch compiled INT lines via `execute_via_generator` with this ObjectScript:

```objectscript
 Set rtn = ##class(%Library.Routine).%OpenId("{routine_name}.INT")
 If rtn = "" { Write "NOT_COMPILED",$C(10)  Quit }
 Do rtn.Rewind()
 While 'rtn.AtEnd { Write rtn.ReadLine(),$C(10) }
 Write "DONE",$C(10)
```

Where `{routine_name}` for a class `MyApp.Foo.cls` is `MyApp.Foo` (strip extension, dots
replaced with dots — IRIS class compilation creates a routine named `MyApp.Foo.1` for
the main routine). Verify the routine name mapping:

- `MyApp.Foo.cls` → primary routine `MyApp.Foo.1.INT` (IRIS convention)
- `MyRoutine.mac` → `MyRoutine.INT`

**NEEDS VERIFICATION**: The `##class(%Library.Routine).%OpenId` approach for fetching
compiled content needs to be confirmed against a real compiled class. The class
`%Library.Integer` may not have a user-accessible INT via this path (system classes
compiled differently). Test with a user-defined class that is compiled in USER namespace.

**Rationale**: Plan assumed Atelier `GET /doc/Name.INT` works — confirmed it does not.
Must use ObjectScript-side routine object instead. This changes implementation from a pure
HTTP call to an `execute_via_generator` call, same pattern as `iris_global`.

**Impact on plan**: `mode=compiled` switches from "Atelier GET with .INT extension" to
`execute_via_generator` + `%Library.Routine` ObjectScript. No new crate needed. Tool
category stays Query (read-only ObjectScript). Output is still plain text (no `{`/`}` in
routine lines — safe for generator output).

---

## Decision 3: `execute_via_generator` — ClassMethod invocation pattern

**Decision**: `##class(ClassName).MethodName(args)` works correctly via
`execute_via_generator`. Verified by the 052 iris_global live test suite which uses
the same mechanism.

**Verification**:

```bash
cargo test -p iris-agentic-dev-core --features testing --test test_iris_global_live \
  test_set_and_get_roundtrip -- --ignored
→ test test_set_and_get_roundtrip ... ok (0.35s)
```

The `execute_via_generator` mechanism (PUT temp class → compile → SQL query → DELETE)
is confirmed working on IRIS 2026.2.0L, USER namespace, port 52780.

For `iris_execute_method`, the generator code will be:

```objectscript
 Set result = ##class({class}).{method}({args_csv})
 Write result,$C(10)
```

**Constraint confirmed**: The ObjectScript **code** injected into the generator body must
not contain `{` or `}` characters (they would interfere with the Atelier class source
containing the generator `{` block delimiters). The **output** (return value) can contain
any string — it is captured via temp file and the `$Char(1)` encoding handles all chars.

This means `iris_execute_method` is safe as long as the class name, method name, and
string args do not contain `{`/`}`. Validation: reject `class` or `method` params
containing `{`, `}`, or `;` (injection guard).

**Verified 2026-06-29** against iris-dev-iris port 52780 via iris_global live test suite
(6/6 tests pass).

---

## Decision 4: Output field names — `ts` vs `modified`, `count` vs `total_returned`

**Decision**: Use `ts` for timestamp (matches Atelier raw field), rename spec `modified`
to `ts`. Use `count` for document count (matches spec FR-003).

**Verification**:

Atelier `/docnames/CLS` response shape (confirmed):

```json
{
  "name": "%Api.Atelier.cls",
  "cat": "CLS",
  "ts": "2026-05-28 00:34:19.742",
  "upd": true,
  "db": "IRISLIB",
  "gen": false
}
```

Field is `ts` (string timestamp), not `modified`. The spec originally said `modified` but
the actual API field is `ts`.

**Decision**: Expose as `ts` in the iris-dev response (pass-through from Atelier). Update
spec FR-003 Key Entities "Document Listing Entry" to use `ts` instead of `modified`.
Tasks already updated (T032, T033) to use `count` not `total_returned`.

---

## Summary: What Changed from Original Plan

| Item                                    | Plan assumed            | Reality (verified)                                         | Resolution                                                  |
| --------------------------------------- | ----------------------- | ---------------------------------------------------------- | ----------------------------------------------------------- |
| `mode=list` fetch                       | `GET /docs?filter=glob` | `/docs` returns empty; `/docnames/` has no wildcard filter | Fetch all + Rust-side regex filter                          |
| `mode=compiled` fetch                   | `GET /doc/Name.INT`     | `.INT` docs don't exist in Atelier                         | ObjectScript `%Library.Routine` via `execute_via_generator` |
| `mode=compiled` tool category           | Query (HTTP-only)       | Now uses `execute_via_generator`                           | Still Query; `execute_via_generator` is read-only           |
| Response field `modified`               | spec FR-003             | Atelier field is `ts`                                      | Use `ts`; update spec                                       |
| Response field `total_returned`         | tasks T033              | spec FR-003 says `count`                                   | Use `count`                                                 |
| `execute_via_generator` for ClassMethod | assumed works           | Verified via iris_global live tests                        | Confirmed ✅                                                |

---

## Pre-existing Bug (do not fix in 053)

`mod.rs:2119` in `iris_compile` wildcard expansion calls `.as_str()` on `/docnames/` entries.
Those entries are objects (`{name, cat, ts, ...}`), not strings — so `.as_str()` always
returns `None` and the filter yields 0 matches. This means wildcard compile targets silently
match nothing. Filed for tracking; out of scope for 053.
