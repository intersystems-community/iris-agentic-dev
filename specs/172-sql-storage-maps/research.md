# Research: SQL storage maps (172)

Every IRIS fact here was measured on iris-dev-iris (IRIS for UNIX 2026.2.0L build 208U, namespace USER) unless it
says otherwise. Nothing here comes from internal source or internal test material.

## R1. Where `read` gets the map: the class text

- **Decision**: `read` parses the Storage block of the class text that Atelier `GET /doc/<Class>.cls` returns. It
  queries `%Dictionary.CompiledStorageSQLMap` (and `…Sub`, `…Data`, `…RowIdSpec`) only to cross-check and for
  `uses`.
- **Rationale**: the class text is what iad writes back, so reading it is the only way to round-trip byte for byte.
  The compiled dictionary is normalised and does not keep formatting. A prototype parsed and re-wrote the Storage
  block of all 162 classes with SQL maps in `%SYS` with no difference.
- **Alternatives**: building the model from `%Dictionary.StorageSQLMap*Definition` objects. Rejected: an agent's
  `iris_execute` cannot name those classes (the code-edit gate refuses `%Dictionary.*Definition`, measured), and
  saving through them bypasses the document path, source control and the write gates.

## R2. Parser and writer: hand-written, no XML crate

- **Decision**: a small element parser and canonical writer in Rust, ported from the throwaway prototype (about 200
  lines of Python). No new crate dependency.
- **Rationale**: the Storage body is XML-like but not XML. Values are raw text: `&gt;` in map code is stored as the
  four characters `&gt;` (measured: it broke the generated routine), so an XML parser that unescapes entities would
  change code. NextCode can contain lines that are only `}`, so the end of the Storage block is found by parsing
  elements, not by matching braces. Constitution VII: the standard library covers this.
- **Alternatives**: `quick-xml` or `roxmltree`. Rejected for unescaping and for adding a dependency.

## R3. The canonical form IRIS writes

Measured by PUT then GET. The writer reproduces each rule:

1. Children sort by tag name ignoring case, then by the `name` attribute in `$Order` collation: canonical numbers
   numerically first, then strings by code point. Storage-level keywords such as `SqlIdExpression` therefore sort
   before `SQLMap`, and subscript names sort 1, 2, … 10, 11.
2. A value containing `<`, `>` or `&` is written in CDATA on a new line after the opening tag. The CDATA is split at
   every `]]`, not only at `]]>`.
3. The storage-level `<Description>` is always CDATA, and IRIS also writes it as `///` lines above `Storage <name>`.
4. IRIS drops empty leaves, and drops `ConditionalWithHostVars` and `BlockCount` when they are `0`. It keeps
   `Deferred 0`, `PopulationPct 0`, `ExtentSize 0`, `LoopInitValue 0`, `StartValue 0` and `AccessType Sub`.
5. The class text ends with one empty line.
6. An element IRIS does not recognise is dropped on PUT, with no error. A known element with an odd value
   (`<BlockCount>-4</BlockCount>`) survives.
7. Compile adds `<StreamLocation>^<Class>S</StreamLocation>` when it is missing.
8. The canonical form is a fixed point: PUT then GET, and compile then GET, give the same text.

## R4. Values IRIS rejects, reinterprets or loses

| Input                                            | Save                                                                                 | Compile                                                                  |
| ------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `<Piece></Piece>`                                | element dropped                                                                      | ok, meaning changed                                                      |
| `<Subscript name="a">`                           | HTTP 200, `status.errors` empty, `result.status` #16023; GET fails until a valid PUT | n/a                                                                      |
| `<Type>wibble</Type>`                            | kept                                                                                 | #5511                                                                    |
| `<Type>DATA</Type>`                              | kept                                                                                 | #5511 (case-sensitive)                                                   |
| `<AccessType>Wibble</AccessType>`                | kept                                                                                 | ok, behaves as `Sub`                                                     |
| `<AccessType>sub</AccessType>`, `other`, `piece` | kept as written                                                                      | ok, same as `Sub`, `Other`, `Piece`: access types are not case-sensitive |
| multi-line NextCode with a tab                   | byte-identical                                                                       | ok                                                                       |
| `&gt;` or `&amp;` in RetrievalCode or Condition  | stored literally                                                                     | generated routine fails                                                  |

So FR-014 refuses, before any write: non-integer names on Subscript, RowIdSpec, Accessvar and Invalidcondition; a
map type outside `data`, `index`, `bitmap`, `bitmapextent`, `columnar`; an access type that is not, ignoring case, one of `Sub`, `Piece`,
`List`, `Global`, `Other`; an unknown element; an empty leaf the caller set on purpose.

## R5. Meaning IRIS applies but does not store

Measured on fixture classes `Iad172.Probe` and `Iad172.Lst`:

- A data field with no `<Delimiter>` is read and written as `$LIST`. On `^`-delimited text SELECT raised `<LIST>`.
  UPDATE through such a map wrote `$lb("Ada",37)`.
- A subscript `{ID}` with no ID allocation fails INSERT with `<UNDEFINED>`; an explicit ID is refused with SQLCODE
  -111. A storage-level `SqlIdExpression` supplies one.
- `$i(^G)` as the ID expression over a global with rows and no counter node gave the first INSERT ID 1 and replaced
  row 1. No error.
- A new index map over existing data: `WHERE City='Arlington'` returned no rows until the index was built;
  `%NOINDEX` found the row.
- RetrievalCode may not name another field: `{Age}` in a computed field's code fails with #5524. Reading the global
  through `{L1}` works.
- A compile that fails removes the table: SQLCODE -30 afterwards, although the previous version had compiled.
- Only the first data map (the master map) is used for reading and filing.

## R6. Map code is agent-supplied code

- **Decision**: these elements carry ObjectScript: subscript `Expression`, `DataAccess`, `NextCode`,
  `StopExpression`, `Invalidcondition` expressions, Accessvar `Code`, `RetrievalCode`, RowIdSpec `Expression`,
  `RowReference`, `Condition`, and an extended-reference `Global`. Each goes through the same code-edit check as
  `iris_execute` code (FR-027).
- **Rationale**: generated routines run them at query and filing time. A side-effect Condition (`$i(^Side)`) ran
  once per INSERT and once per UPDATE (measured). Whether the query planner can also run a Condition while it plans
  a query could not be reproduced in four query shapes; it is treated as code either way.

## R7. Scratch copy

- **Decision**: `check` writes the class under `IrisDevTmp.<id>` with the proposed map, `SqlTableName` and any
  `{%%CLASSNAME}`-dependent keywords rewritten so it cannot collide, compiles it, reads the compile errors, then
  deletes it under the existing scratch-class guard. The real class is written only after that compile succeeds.
- **Rationale**: a failed compile of the real class takes its table out of SQL (R5). Compiling a copy reads no data.
  `preview` runs a SELECT against the copy, which reads the real global and runs map code, so it needs execute
  permission (FR-029).
- **Open in the plan's first task**: confirm that a scratch copy pointing at the real global compiles without
  writing to it, and that deleting the copy leaves the global alone (live test).

## R8. Restore after a failed real compile

- **Decision**: before the real PUT, iad keeps the text it read (the same GET that gave the map version). If the
  real compile fails, iad PUTs that text back, compiles it, and reports both the failure and the restore. If the
  restore fails too, the response says so first.

## R9. Read-back check

- **Decision**: after every PUT, GET the class, parse both texts, and compare the models after applying R3's drop
  rules to what was sent. Any other difference is an error naming the element (FR-012); a difference R3 does not
  explain is reported as a writer bug with the diff.

## R10. Map version

- **Decision**: the map version is a short hex digest of the canonical text of the one map (`SQLMap` element), and
  the storage version a digest of the whole Storage block. Use the hashing crate already in the workspace.

## R11. `uses`

- **Decision**: `SELECT parent, Name, Type, _Global FROM %Dictionary.CompiledStorageSQLMap WHERE _Global = ?` over
  Atelier SQL, plus `DataLocation`, `IndexLocation` and `IdLocation` from `%Dictionary.CompiledStorage` for default
  storage. Measured: the first query with `^Iad172Probe` returned exactly the master map of `Iad172.Probe`; the
  namespace held 232 compiled maps. The global is compared after normalising both spellings (`^X`, `X`) and
  extended references (`^|"NS"|X`, `^["NS"]X`).
- **Scope**: the target namespace by default; `namespaces: [...]` lists others. Each namespace searched is named in
  the response.

## R12. Sampling a global

- **Decision**: one generated ObjectScript walk (`$Order` with a node cap and a time cap) returns, per sampled node,
  its subscript depth and a format class: `$LIST` when `$LISTVALID` is true and the value is not empty, `^`-delimited
  when it contains `^`, otherwise plain; plus piece counts. Values are returned only when `include_values` is set.
- **Measured**: `$LISTVALID("a^b")` is 0, `$LISTVALID($lb("a",1))` is 1, `$LISTVALID("")` is 1, so an empty value
  is classed as plain, not `$LIST`.

## R13. ID reuse and unbuilt indexes

- **Decision**: for an ID expression of the form `$i(^G)` or `$i(^G(subs))`, compare `$Get(^G…)` with
  `$Order(^G…(""),-1)` of the master map's first ID level; refuse when the counter is lower (FR-023). For a new index
  map, refuse when its global has no node under the map's first subscript and the master map has rows.
- **`build_indexes`**: `##class(<Class>).%BuildIndices($lb("<MapName>"))` returned OK for the index map `CityIdx` on
  the `%Storage.SQL` class `Iad172.Probe` (measured). The plan's task verifies the index answers afterwards.
- **`seed_counter`**: `Set ^G=<highest existing ID>` through the write gate, then an INSERT is checked not to
  overwrite.

## R14. Diagram

- **Decision**: Mermaid `flowchart LR`: one node for the global, one per subscript level (expression and access type),
  one per data node with its fields and pieces, and the row ID parts; index maps as separate subgraphs. The JSON
  carries the same nodes and edges. Text must render with the Mermaid CLI in a test.

## R15. Corpus and coverage

- **Decision**: fixture classes under `tests/fixtures/storage172/`, package `IadSm172.*`, globals `^IadSm172*`,
  invented domain, code written for this repository. Each has a seed routine and a known question with its known
  answer row. A local-only test reads `IAD_STORAGE_CORPUS` and reports pattern combinations with no public example.

## R16. Access types are not case-sensitive (measured 2026-11-10)

`sub`, `other` and `piece` compile exactly as `Sub`, `Other` and `Piece` (same generated code, same compile errors for
a `Piece` level with no delimiter), and the dictionary keeps the spelling as written. Lower-case `sub` is common in real
maps, so the validator compares access types ignoring case and the writer keeps the original spelling. Map `Type`
stays case-sensitive (`DATA` fails with #5511).

## R17. D11 and fixture facts (measured 2026-11-10, fixtures in `tests/fixtures/storage172/`)

- **A SQL DELETE kills the row node's whole subtree.** `DELETE FROM IadSm172.Loan WHERE ID = 1` took
  `$Data(^IadSm172Loan(1))` from 11 to 0 and removed `^IadSm172Loan(1,"EV",1)`, a row of the other table
  (`LoanEvent`) stored inside it, with no error or warning (`test_storage_map_d11_172_live`).
- **A level walked as a plain subscript returns literal sub-nodes as rows.** A map over `^IadSm172Loan` with
  subscripts `{LoanNo}`, `{Part}` returned `(1,"EV")` and `(2,"EV")`.
- **An UPDATE of a row-node field keeps the sub-node fields** (`MemberD11`: Email and Phone on the `"contact"` node
  were unchanged).
- A parent's map is not confused by child rows under its row nodes: `Loan` returns only real loans although
  `^IadSm172Loan(id,"EV",n)` exists.
- A master map's subscripts must be IDKEY fields (#5547). A composite row ID needs `Index IDKEY On (...) [ IdKey ]`;
  RowIdSpec entries are optional for it (`Copy` has them, `LoanEvent` does not).
- `NextCode` refers to its level as `{Ln}`, sets it to the next subscript or to `""` to stop, and every line starts
  with a space or a tab (column 0 is a label).
- `RowReference` reads the row from another global while the map walks its own (`LoanUnion`).
- A code value with `]]` and no `<`, `>` or `&` stays raw; with one of them, IRIS writes CDATA split as
  `]]]]><![CDATA[` (`Notes`). `BlockCount 0` and `ConditionalWithHostVars 0` are dropped on save (`Notes`).
- Not exercised: the bitmap map in `Genre` compiles, but the seed format sets only strings and `$LIST`s, so its
  question goes through the master map.
