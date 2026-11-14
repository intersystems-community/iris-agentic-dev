# Feature Specification: SQL storage maps an agent can read, draw, check and edit

**Feature Branch**: `172-sql-storage-maps`
**Created**: 2026-11-09
**Status**: Draft
**Input**: vscode-objectscript issue #1701 asks for Studio's global-to-table mapping assistant in VS Code. Developers
who keep data in globals still edit SQL storage maps for new columns, computed columns, new tables over old globals
and Row Reference tables, and Studio is their only tool. Give an agent driving iad tools strong enough that the same
work is safe, and strong enough that someone could build a GUI with Studio's functionality on top of them. Design
decisions: `docs/adr/0004-sql-storage-maps-are-edited-through-a-model.md`.

## Context

A SQL storage map (glossary: **SQL storage map**) is hand-written text inside a class's Storage block. Storage built
from these maps on `%Storage.SQL` is officially called SQL storage, or custom SQL storage; it is not custom storage,
which is `%Library.CustomStorage` with hand-written access code (glossary: **SQL storage**, **Custom storage**). It says which
global holds the table's rows, what each subscript level means, where each column sits in a node, and how the row ID
is formed. Map elements can carry ObjectScript (computed columns, `NextCode`, conditions) that runs at query and filing
time.

Today iad has no way to work with one:

- `resolve_storage` returns the data, ID and index locations, which are empty for a `%Storage.SQL` class. The map
  itself is in the class dictionary and iad never reads it.
- `iris_doc` says Storage blocks are off-limits. That is right for default storage (glossary: **Default storage**)
  and wrong for a map the developer owns.
- An agent that edits the map as text has to follow IRIS's save rules without knowing them, and most mistakes
  compile.

Measured on IRIS 2026.2 (iris-dev-iris), each a reason this needs tools rather than a skill alone:

1. A data field with no delimiter is read as a `$LIST`; on `^`-delimited data the query fails at run time.
2. An empty `<Piece/>` disappears on save, and the column's meaning changes with no error.
3. An element IRIS does not recognise is dropped on save with no error or warning.
4. An entity such as `&gt;` in map code is stored literally and breaks the generated routine.
5. `<Type>DATA</Type>` fails to compile (#5511): map types are case-sensitive. An unknown `AccessType` compiles and
   silently means `Sub`.
6. A non-integer subscript name makes the class unreadable by GET until a valid save replaces it, while the save
   answers HTTP 200 with the error only in `result.status`.
7. A map whose IDs come from `$INCREMENT` on a global that already holds rows but no counter gave the first INSERT ID
   1, and it overwrote row 1 with no error.
8. A new index map over existing data returns no rows for a value that is there, with no error, until the index is
   built.
9. A map that fails to compile removes the table from SQL (SQLCODE -30) until it is fixed, even though the previous
   version worked.

A throwaway prototype read and re-wrote every map in the container's own `%SYS` namespace (162 classes) byte for byte,
and 30 single-field edits each changed exactly the edited element. The canonical writer is feasible.

## Clarifications

### Session 2026-11-09

Settled by grilling before this spec was written (ADR 0004; Q-numbers refer to that session):

- Q1: One release covers read, diagram, verify and edit together.
- Q2: One new dispatcher tool, `iris_storage_map`, with typed actions. `resolve_storage` keeps its name and uses the
  same reader.
- Q3: `check` compiles a scratch copy first; the real class is written only if the copy compiles; a failed real
  compile restores the previous version.
- Q4: `edit` takes path operations on the model; `replace` takes a whole map. Both carry the map version.
- Q5: `iris_doc` still accepts map text, runs the same safeguards when the map changed, and warns that
  `iris_storage_map` exists. A map edit is an ordinary write: same write gates, no extra acknowledgement.
- Q6: Map globals and map code go through the existing global and code checks; dynamic globals need an
  acknowledgement (superseded: an operator limit, FR-028); `preview` of a proposed map needs execute permission.
- Q7: Declared contract: `model_version`, golden-file test, public `docs/storage-maps.md`.
- Q8: `edit` refuses ID reuse and an unbuilt index by default; `seed_counter` and `build_indexes` opt in.
- Q9: `propose` drafts a map from a sampled global and writes nothing.
- Q10: Verify actions return shape by default; values only on request, through the data policy; capped.
- Q11: Two changes land before 172, each on its own branch: `iris_doc` put reads `result.status`, and a separate
  security check.
- Q12: Fixtures and a public synthetic corpus with known answers; a local-only coverage test.
- Q13: Roadmap target December 2026, block "Interoperability and mapped globals".

### Session 2026-11-09 (security review of this design)

An adversarial review checked this design against iad's recent security work (specs 052, 087, 133, 137, 141, 171).
Tom chose to report nested tables and the DELETE hazard rather than block them (FR-003a, 2026-11-10), and to
exercise bitmap maps for real: fixtures build them with `%BuildIndices`, which needs a class `Index` named like the
map (research R17).
Tom chose an operator limit for dynamic globals (FR-028; was acknowledgement required in Q6), and chose to treat
map code like class code, with code-bearing edits also in the Execute category (FR-027), over a pattern allowlist. Other changes:
FR-005 (diagram escaping), FR-010a (no blind overwrite), FR-012a (re-gate what IRIS stored; refuse
what iad cannot parse), FR-014a (parser caps), FR-015a (scratch copy built from the model), FR-021 (values refused
under block and redact), FR-023 and FR-023a (opt-ins judged by the pipeline; `build_indexes` is destructive),
FR-026 (global checks on every action, extended references refused for writes), FR-029 (`preview`, `compare` and
`propose` are execute calls run as the service account), FR-030a to FR-030c (one action reader, no caller text in
generated code, audit), FR-031 (the `iris_doc` hand-off sits at its single write point).

## User Scenarios & Testing

### User Story 1 - Read a map as structured data (Priority: P1)

An agent asks for a class's storage and gets every SQL storage map as structured data: each map's name, type and
global; each subscript level with its expression, access type and the code that walks it; each data field with its
node, delimiter, piece and retrieval code; the row ID parts; conditions and Row Reference. For default storage it gets
which property sits in which `$LIST` slot of which node, read-only.

**Why this priority**: every other story starts from the model. An agent that can read a map correctly can already
explain one, which is most of what the issue's reporter asked a colleague to do.

**Independent Test**: read the fixture classes and the container's `%SYS` maps; each field in the model matches the
compiled dictionary for that class.

**Acceptance Scenarios**:

1. **Given** a `%Storage.SQL` fixture class with a `^`-delimited data map, **When** `iris_storage_map action=read`
   runs, **Then** the model lists the global, each subscript's expression in order, and each field's node, delimiter
   and piece, and they match `%Dictionary.CompiledStorageSQLMap*` for that class.
2. **Given** a field with no delimiter, **Then** the model says the node is read as a `$LIST`, not "no delimiter".
3. **Given** a class with several data maps, **Then** the model marks the master map (glossary: **Master map**) and
   says the others are not used.
4. **Given** a field with retrieval code, **Then** the model marks it computed and not filed.
5. **Given** a default-storage class, **Then** the model lists each property's node and slot and says default storage
   is not edited.
6. **Given** any read, **Then** the response carries `model_version` and the map version (glossary: **Map version**).
7. **Given** `resolve_storage` on a `%Storage.SQL` class, **Then** it returns the same maps instead of empty locations.

---

### User Story 2 - Draw a map, and find who uses a global (Priority: P1)

An agent draws a map as a diagram it can show the user: global, subscript levels, nodes and the columns in each, and
the row ID. It can also start from a global and get every class and map that reads or writes it.

**Why this priority**: the reporter's first need is seeing an old global's layout. Finding every table over one
global is the check a developer makes before changing it.

**Independent Test**: diagram a fixture and render the Mermaid text with the Mermaid CLI with no error; `uses` on a
fixture's global returns exactly the fixture classes that map it.

**Acceptance Scenarios**:

1. **Given** a fixture class, **When** `action=diagram` runs, **Then** the response has Mermaid flowchart text and the
   same content as JSON, and the text renders.
2. **Given** a global used by a data map in one class and an index map in another, **When** `action=uses` runs,
   **Then** both classes and maps are listed with the map type.
3. **Given** a global no class maps, **Then** `uses` returns an empty list and says which namespaces it searched.

---

### User Story 3 - Edit a map without losing anything (Priority: P1)

An agent changes one element of a map (adds a column, changes a piece, adds a subscript level) with a path operation,
or saves a whole map at once as a GUI's Save button would. The saved class differs from the original only in what was
changed. Anything IRIS dropped on save is reported, never lost silently.

**Why this priority**: this is the round-trip requirement. Without it every edit risks the silent changes listed in
the Context.

**Independent Test**: on each fixture, read then write with no change and compare the stored class byte for byte;
apply a single-field edit and diff the stored class against the original.

**Acceptance Scenarios**:

1. **Given** any fixture or `%SYS` map, **When** it is read and written back with no change, **Then** the class text
   IRIS stores is byte-identical to before.
2. **Given** a `set` of one field's piece, **When** `edit` runs, **Then** the diff of the stored class is exactly that
   element.
3. **Given** `add`, `remove` and `move` operations, **Then** each produces the element order and escaping IRIS itself
   would write (ordering by tag and name, CDATA for `<`, `>` or `&`, split at every `]]`).
4. **Given** map code containing `<` or `&`, **Then** it is stored as raw characters and the generated routine runs.
5. **Given** an edit or replace carrying an older map version, **Then** it is refused and the response includes the
   current map.
6. **Given** a `replace` that would remove elements, **Then** the response lists what it would remove before anything
   is written, and the write needs a second call that confirms it.
7. **Given** a model element IRIS drops on save (an empty value, an unknown element), **Then** the read-back after the
   save finds the difference and the response reports it as an error naming the element.
8. **Given** an edit to a columnar map, **Then** it is refused; columnar maps are read-only in 172.

---

### User Story 4 - Check a change before the real class is touched (Priority: P1)

Before any map change reaches the real class, iad compiles a scratch copy (glossary: **Scratch copy**) with the new
map. Compile errors come back explained. Only a copy that compiles leads to the real write, and a real compile that
fails puts the previous version back.

**Why this priority**: a failed compile removes the table from SQL. The scratch copy keeps a working table working.

**Independent Test**: submit a map that fails to compile (`<Type>DATA</Type>`); the real class and its table are
unchanged and still queryable, and the response explains #5511.

**Acceptance Scenarios**:

1. **Given** an edit that fails to compile, **When** `edit` or `check` runs, **Then** the real class is unchanged, its
   table still answers a query, and the response gives the IRIS error with an explanation in plain words.
2. **Given** an edit whose scratch copy compiles but whose real compile fails, **Then** the previous version is
   restored and recompiled, and the response says so.
3. **Given** any check, **Then** no scratch class is left behind; one that cannot be removed is reported.
4. **Given** strict read-only, **Then** `check`, `edit`, `replace`, `preview`, `compare` and `propose` are refused with the
   same refusal as any scratch compile.
5. **Given** a non-integer subscript name, **Then** the edit is refused before any write, and the real class stays
   readable.

---

### User Story 5 - Check a map against the real data (Priority: P2)

An agent compares a map with the global it describes (subscript depth, node format, piece counts) and previews the
rows a proposed map would return, before anything is saved. By default it sees shapes and counts, not values.

**Why this priority**: a map that compiles can still be wrong for the data. This is the check Studio never did.

**Independent Test**: on a seeded fixture with a known mismatch (a field mapped to the wrong piece), `compare` reports
the mismatch, and `preview` returns the known row for the known question.

**Acceptance Scenarios**:

1. **Given** a map that says `^`-delimited and a global that holds `$LIST` nodes, **When** `compare` runs, **Then** the
   mismatch is reported with the node it was seen in.
2. **Given** a seeded fixture and a proposed map, **When** `preview` runs, **Then** it returns the fixture's known
   answer row for the known question.
3. **Given** no `include_values`, **Then** values are masked and only shape, types and counts are returned.
4. **Given** `include_values: true`, **Then** the server's data policy applies as for any bulk-data tool, and a
   sensitive-named global under allow needs `acknowledgePhi`.
5. **Given** a global larger than the cap, **Then** the scan stops at the cap (50 rows for preview, 200 nodes for
   compare and propose by default, hard maximum about 1,000, plus a time cap) and the response says a cap stopped it.

---

### User Story 6 - Refuse edits that damage existing data (Priority: P2)

An edit whose row IDs would reuse existing IDs, or that adds an index map over data without building the index, is
refused by default with what would happen. The agent can opt in to fixing either as part of the edit.

**Why this priority**: both hazards corrupt or hide data with no error from IRIS.

**Independent Test**: on a seeded global with rows and no counter, an edit with `$i(^G)` as the ID expression is
refused; with `seed_counter: true` the counter is set to the highest existing ID and an INSERT gets the next ID.

**Acceptance Scenarios**:

1. **Given** an ID expression `$i(^G…)` over a global whose counter is below the highest existing ID, **Then** the
   edit is refused and the response names both numbers.
2. **Given** `seed_counter: true`, **Then** the counter is set to the highest existing ID through the write gate, and
   a following INSERT does not overwrite a row.
3. **Given** a new index map over existing data, **Then** the edit is refused unless `build_indexes: true`, which
   builds that map's index after compile through the write gate.
4. **Given** any other ID expression, **Then** the response says iad cannot verify it and says what the expression
   does.

---

### User Story 7 - Draft a map for an unmapped global (Priority: P3)

An agent samples a global that has no table and gets a draft map: subscript depth, node format, piece counts, and
suggested field names and types, each marked as a guess. Nothing is written; the draft is saved through `replace`
with every safeguard.

**Why this priority**: "new tables over old globals" is one of the reporter's cases, but a developer can write the
first draft by hand once the read and check tools exist.

**Independent Test**: on a seeded `^`-delimited fixture global, `propose` returns the right depth and piece count,
and the draft saved through `replace` returns the fixture's known row.

**Acceptance Scenarios**:

1. **Given** a seeded global, **When** `propose` runs, **Then** the draft's depth, delimiter and piece count match the
   seed, and every suggested name and type is marked as a guess.
2. **Given** `propose`, **Then** nothing is written to IRIS.

---

### User Story 8 - Map globals and map code go through iad's gates (Priority: P1)

A map can name any global and carry ObjectScript that runs at query time. A map is therefore agent-supplied code
(glossary: **Agent-supplied code**), and it gets the same checks as other agent code, from both entry points
(`iris_storage_map` and `iris_doc`).

**Why this priority**: without it a map edit is a way around the code and global checks.

**Independent Test**: for each refusal, a gate test with both spellings of the global, and a test that each handler
calls the gate.

**Acceptance Scenarios**:

1. **Given** a map whose global is a code-storage global or a global on the system blocklist, **Then** it is refused
   as a fixed rule from both entry points.
2. **Given** map code (retrieval code, `NextCode`, a condition, an invalid condition, access variable code) that the
   code-edit check refuses in `iris_execute`, **Then** the map is refused with the same refusal.
3. **Given** a dynamic global (`AccessType Global`, indirection in data access or Row Reference), **Then** the edit is
   refused as an operator limit that names the global expression; only the server setting
   `allowDynamicMapGlobals` lifts it.
4. **Given** `preview`, `compare` or `propose`, **Then** it is judged as an execute call on the target server and runs
   as the service account when one is configured.
5. **Given** a map edit, **Then** it passes the same write gates as any document write, with no extra
   acknowledgement.

---

### User Story 9 - `iris_doc` and the off-limits rule (Priority: P2)

Developers edit `.cls` files locally and push them with `iris_doc`. That keeps working for a class with a SQL storage
map. When the map changed, `iris_doc` runs the same safeguards as `iris_storage_map` and says the tool exists. Default
storage stays off-limits.

**Why this priority**: `iris_doc` is how most agents write classes today; a second path without the safeguards would
undo stories 3, 4 and 8.

**Independent Test**: push a fixture class through `iris_doc` with a map change that fails to compile; the real class
is unchanged and the response carries the warning.

**Acceptance Scenarios**:

1. **Given** an `iris_doc` put of a class whose SQL storage map changed, **Then** the scratch compile, the read-back
   drop check and the restore all run, and the response warns that `iris_storage_map` exists for map edits.
2. **Given** an `iris_doc` put whose map did not change, **Then** no warning is added (the safeguards still run,
   FR-031).
3. **Given** an edit to default storage, **Then** today's behaviour is unchanged.
4. **Given** `tools/list`, **Then** `iris_doc`'s description says default storage is off-limits and SQL storage maps
   are edited with `iris_storage_map`.

---

### User Story 10 - A contract a GUI can build on (Priority: P2)

Someone building a GUI or another tool on iad can rely on the model's JSON shape. It is versioned, pinned by a test,
and documented publicly. An agent has a skill that explains maps and the traps, each claim backed by a live test.

**Why this priority**: the Studio-parity goal needs a stable interface and a public reference; there is no full one
today.

**Independent Test**: the golden-file test fails when the JSON shape changes without a `model_version` bump; each
skill claim maps to a passing live test.

**Acceptance Scenarios**:

1. **Given** the fixtures, **Then** a golden-file test pins the JSON shape of `read`, `diagram`, `compare`, `preview`
   and `propose`.
2. **Given** a change to the shape, **Then** the test fails until `model_version` is bumped, and the release notes
   list it as breaking.
3. **Given** `docs/storage-maps.md`, **Then** it covers the model, each action, each error code and the measured traps,
   and every fact in it comes from a live test in this repository.
4. **Given** the skill `iris-storage-mapping` (extra tier), **Then** each factual claim has a live test, and the skill
   tier guards pass.

---

### User Story 11 - A public corpus with known answers (Priority: P2)

A public synthetic corpus of map classes covers every map pattern in use: piece and `$LIST` fields, multi-node rows,
computed fields, `NextCode`, index and bitmap maps, conditional maps, composite row IDs, parent-child, Row Reference.
Each class has seeded data and a known question with a known answer.

**Why this priority**: round-trip and preview claims need many real shapes to mean anything, and the corpus has to be
public so CI and contributors can run it.

**Independent Test**: CI loads each corpus class, round-trips it byte-identically, compiles it, and gets its known
answer.

**Acceptance Scenarios**:

1. **Given** each corpus class, **Then** it round-trips byte-identically, compiles, and returns its known answer row.
2. **Given** the container's own `%SYS` maps, **Then** a read-only live test round-trips every one byte-identically.
3. **Given** `IAD_STORAGE_CORPUS=<dir>` set locally, **Then** an ignored test reports which pattern combinations in
   that directory have no example in the public corpus; with it unset the test is skipped.

### Edge Cases

- A row node that holds a value and has child nodes (`$Data` 11, "D11"): some fields sit on the row node itself and
  others on named sub-nodes, or another table's rows are stored under the same node (`^G(id)` is the parent row,
  `^G(id,"EV",n)` the child rows). `read` names it, sampling counts it per level, `propose` drafts both parts, and an
  edit that would make a level walk the parent's literal sub-nodes as if they were rows is refused (FR-003a, FR-020a,
  FR-023b).
- A class whose Storage block has no SQL map: `read` returns default storage or an empty map list, and says which.
- A storage class other than `%Storage.SQL` that carries SQL maps: `read` handles it; editing is refused in 172.
- `NextCode` containing `}` lines: the Storage block's end is found by parsing elements, never by matching braces.
- A storage `<Description>`: IRIS writes it as CDATA and as `///` lines; the writer produces both, and the round-trip
  test covers it.
- Default values IRIS drops (`ConditionalWithHostVars 0`, `BlockCount 0`) and empty values: the writer drops them the
  same way, so a round-trip is still byte-identical.
- Two callers edit the same map: the second carries an old map version and is refused with the current map.
- Map type in the wrong case, or an unknown access type: refused at edit time with the reason, before the scratch
  compile.
- A class that IRIS cannot read back after a save: the restore runs, and the response says the class was restored.
- The read-back finds a difference that was not a drop (IRIS reformatted something the writer did not predict): it is
  reported as a writer bug, with the diff, and the edit is not reported as clean.
- A target server with no Atelier REST (NoPWS): every action that needs a document write or GET is refused as
  unavailable, with the reason.

## Requirements

### Functional Requirements

#### Reading and drawing

- **FR-001**: iad MUST provide one new tool, `iris_storage_map`, with the actions `read`, `diagram`, `uses`,
  `compare`, `preview`, `propose`, `check`, `edit` and `replace`. No other new tool.
- **FR-002**: `read` MUST return every SQL storage map of a class as structured data with all map, subscript, data,
  row ID, access variable and invalid-condition elements IRIS stores, and default storage's property slots read-only.
- **FR-003**: `read` MUST state derived facts IRIS applies but does not store: no delimiter means `$LIST`, the master
  map is the first data map, a field with retrieval code is not filed, a table is writable only when the master map's
  global reference starts with `^`, `$` or `@`.
- **FR-003a**: `read` MUST report a map whose row node holds fields and also has sub-nodes, and a data map stored
  inside another map's row node (same global, its subscripts extend the other's with a literal level); both maps name
  each other. On the parent, a derived fact MUST say that a SQL DELETE of one of its rows also deletes the rows of the
  nested map stored under it (measured, research R17). iad does not change or block this behaviour; `diagram` shows the
  nesting, and `docs/storage-maps.md` and the skill state the hazard with the test that measured it.
- **FR-004**: `resolve_storage` MUST use the same reader and return the maps for a `%Storage.SQL` class.
- **FR-005**: `diagram` MUST return Mermaid flowchart text and the same content as JSON, and a Markdown document of
  each map (added 2026-11-12 at Tom's request, a cheap step toward a GUI). Labels MUST be built
  from a fixed set of characters, with everything else escaped; no Mermaid directive, link or click handler may come
  from class or map text.
- **FR-006**: `uses` MUST return every class and map in the searched namespaces whose global is the given one, with the
  map type, and name the namespaces searched.

#### Writing

- **FR-007**: `edit` MUST accept a list of path operations (`set`, `add`, `remove`, `move`) on the model; `replace`
  MUST accept a whole map.
- **FR-008**: Every write MUST produce the canonical text IRIS itself writes, and MUST NOT splice text into the class
  source.
- **FR-009**: A read followed by a write with no change MUST leave the stored class byte-identical.
- **FR-009a**: A seeded set of random grammar-valid Storage trees MUST each survive emit, save and read-back with
  no difference beyond the documented drop rules.
- **FR-010**: Every edit and replace MUST carry the map version it was based on, and MUST be refused with the current
  map when the stored map has changed.
- **FR-010a**: The real PUT and any restore MUST detect a change to the class since iad read it, and refuse rather
  than overwrite it.
- **FR-011**: `replace` MUST report what it would remove and write only on a confirming call.
- **FR-012**: After every save, iad MUST read the class back, compare it with what it sent, and report any difference
  as an error naming the element.
- **FR-012a**: After the PUT and before the real compile, iad MUST run the gates (FR-026 to FR-028) again on the map
  as IRIS stored it, and restore the previous version when they refuse. A Storage block iad cannot parse MUST be
  refused, never written as plain text. Every Storage block in a class MUST be gated, not only the first. Class XML
  that carries a SQL map MUST be refused in 172.
- **FR-013**: Edits to columnar maps and to storage classes other than `%Storage.SQL` MUST be refused in 172.
- **FR-014**: Values that IRIS rejects or reinterprets (non-integer subscript, access variable or invalid-condition
  names; map type not one of `data`, `index`, `bitmap`, `bitmapextent`, `columnar`, exact case; an access type that
  is not one of the five ignoring case; unknown elements) MUST be refused before any write, with the reason. An
  access type's spelling (`sub`, `Sub`) is kept as written.
- **FR-014a**: The parser MUST refuse, not crash on, a Storage body over a size cap, nesting deeper than a fixed
  limit, or an unterminated element or CDATA, and MUST run in time linear in the input.

#### Checking

- **FR-015**: Every map change MUST compile a scratch copy first; the real class MUST be written only if the copy
  compiles.
- **FR-015a**: The scratch copy MUST be built from the model: the class header, properties, indices, the keywords the
  map needs and the Storage block, nothing else from the caller's text. It MUST pass the same compile-time gate as an
  `iris_doc` put, MUST hold no credentials, and compiling and deleting it MUST leave every global unchanged (a live
  checksum test proves it).
- **FR-016**: When the real compile fails, iad MUST restore and recompile the previous version and say so.
- **FR-017**: Compile errors from map elements MUST come back with a plain explanation of the error number.
- **FR-018**: Scratch copies MUST be removed after use, and any that cannot be removed MUST be reported.
- **FR-019**: Strict read-only MUST refuse `check`, `edit`, `replace`, `preview`, `compare` and `propose`: each
  compiles a scratch class or runs generated code on the server.

#### Data

- **FR-020**: `compare`, `preview` and `propose` MUST return shape, types and counts by default, with values masked.
- **FR-020a**: The shape report MUST count, per subscript level, nodes with a value only (`$Data` 1), children only
  (10) and both (11), and list the literal sub-node names seen under value-holding nodes (masked as `node1`, `node2`
  unless `include_values`). `compare` MUST flag a mapped level whose nodes are 11 when the map reads only the value,
  or only the children.
- **FR-021**: `include_values: true` MUST go through the server's data policy as a bulk-data read: refused under block
  and redact, and needing `acknowledgePhi` under allow. `include_values: false` or absent is not a body read. The
  shared body-read predicate other tools use MUST NOT change.
- **FR-022**: Scans MUST stop at the caps in User Story 5, and the response MUST say when a cap stopped one.
  `preview`'s time cap is 10 s on its query through the scratch copy (`STORAGE_MAP_TIME_CAP`).
- **FR-023**: `edit` MUST refuse by default an ID expression that would reuse existing IDs, and an index map over
  existing data whose index is empty; `seed_counter` and `build_indexes` opt in to the fix. Each opt-in is judged by
  the normal gate pipeline: `seed_counter` as a write, `build_indexes` as a destructive write (an index build purges
  the index first).
- **FR-023a**: `build_indexes` MUST be refused when the index map's global and subscript range is used by another map
  or already holds nodes; `seed_counter` MUST be refused when another class uses the same counter, and MUST report the
  value it replaced.
- **FR-023b**: Before an edit is written, iad MUST check every level the map walks: when sampling shows literal
  sub-node names at a level the map walks as row IDs (so a parent's `"EV"` node would read as a row), the edit is
  refused unless the map's walk skips them (a `NextCode`, a start value or an invalid condition), and the refusal
  names the level and the masked node. A delete or update through a map whose row node has children MUST be checked
  live in 172 for what it removes, and the result written into `docs/storage-maps.md`.
- **FR-024**: An ID expression other than plain `$i(^G…)` MUST be reported as not verifiable, with what it does.
- **FR-025**: `propose` MUST write nothing and mark every suggestion as a guess.

#### Gates

- **FR-026**: Every global a map or storage keyword names MUST pass the system blocklist, the server's own
  blocklist and the sensitive-name check, on every action that reaches IRIS (`compare`, `preview`, `propose`,
  `check`, `edit`, `replace`, the opt-ins and the `iris_doc` hand-off), not only on writes. A code-storage or
  blocklisted global is a fixed rule. An extended reference to another namespace or database MUST be refused for
  writes and sampling in 172 (`STORAGE_MAP_INVALID`, rule `extended_reference`); `read`, `diagram` and `uses` may show it.
- **FR-027**: Map code is treated like class code written through `iris_doc`. Every code-bearing element MUST pass
  the same lexical code-edit check as `iris_execute` code, from both entry points, along with source control and the
  compile-time gate. An edit or `iris_doc` put that adds or changes a code-bearing element is judged in the Execute
  category as well as Compile, so a server whose `allow` list has Compile but not Execute refuses it. For
  `iris_doc` the hand-off decides this when it reads what the class held, by comparing code elements (amended
  2026-11-14: the call's static category cannot see the previous version). The audit
  record marks `code_changed`. `docs/storage-maps.md` MUST say that saved map code later runs as whoever queries or
  files rows, and that the service account does not cover it.
- **FR-028**: A dynamic global (`AccessType Global`, `@` indirection, or a `$Name`-built reference in a map's
  global, data access or Row Reference) MUST be refused as an operator limit, naming the expression. Only the
  per-server setting `allowDynamicMapGlobals = true`, in the operator file or a trusted file, lifts it; it is a
  loosening setting, and no call argument lifts it.
- **FR-029**: `preview`, `compare` and `propose` run code on the server built from a map or for one. They MUST be
  judged as execute calls (write tier, execute category), and MUST run as the service account when one is configured,
  through the same identity path as `iris_execute`, refusing when they cannot. Their answers carry the identity block.
- **FR-030**: Map writes MUST pass the same write gates as other document writes, with no extra acknowledgement.
- **FR-030a**: One function MUST read the action and opt-ins of an `iris_storage_map` call for every consumer:
  write classification, strict read-only, the service-account decision and the handler. An action it cannot read is
  judged as the strictest one.
- **FR-030b**: Generated ObjectScript for sampling MUST take only a canonical global name, literal subscripts quoted
  by iad, an identifier map name and integer limits; any caller value goes through the `X-IAD-Args` header, and no map
  expression is evaluated by an iad snippet.
- **FR-030c**: Every map write MUST leave an audit record: class, map, versions before and after, the changed paths,
  and whether a code-bearing element changed. Payloads go through the existing scrub.

#### `iris_doc`

- **FR-031**: Every `iris_doc` write path that stores a class (put, the line-edit modes, and the resume after a
  confirmation) MUST hand a class whose content holds a SQL map, of any storage type, to the same safeguards: FR-012,
  FR-012a, FR-014 to FR-018, FR-023 and FR-026 to FR-028. The hand-off sits at the single point where `iris_doc`
  writes. FR-010 and FR-011 do not apply to `iris_doc`. The answer carries a warning that names `iris_storage_map`,
  and a `storage_map_write` record that is also audited (FR-030c); a failed compile carries FR-017's explanations.
  Amended 2026-11-14 to match what ships: `iris_doc` has no opt-ins, so FR-023's ID-reuse and unbuilt-index checks
  are warnings there, not refusals (the walk check still refuses), and the scratch compile runs only when the put
  compiles. The safeguards run whenever the content holds a SQL map, whether or not the map changed.
- **FR-032**: `iris_doc`'s handling of default storage MUST NOT change, and its description MUST distinguish default
  storage from SQL storage maps.

#### Contract, docs, skill, corpus

- **FR-033**: Every response MUST carry `model_version`; a golden-file test MUST pin the JSON shape on the fixtures.
- **FR-034**: `docs/storage-maps.md` MUST document the model, actions, error codes and measured traps, from this
  repository's live tests only.
- **FR-035**: A skill `iris-storage-mapping` (extra tier) MUST explain maps and their traps, with a live test per
  factual claim.
- **FR-036**: A public synthetic corpus MUST cover the patterns in User Story 11, each class with seeded data and a
  known answer, with invented names and globals and code written for this repository.
- **FR-037**: A local-only test MUST compare a corpus directory named by `IAD_STORAGE_CORPUS` with the public corpus,
  and MUST be skipped when the variable is unset.
- **FR-037a**: Beyond the hand-written fixtures, a generated corpus MUST cover every allowed pair of dimension values
  (map type, depth, delimiter class, node layout, row ID parts, each code element, conditional, parent-child, access
  type, global form). Live IRIS decides which pairs are allowed: a pair that fails to compile is recorded with its
  error number and becomes a validation case. Each generated class carries its own row model, and its known answer
  is that row model, never a value read back from iad or IRIS.
- **FR-038**: The new tool MUST be declared wherever `docs/invariants.md` requires (tool category, gate class, guard
  tests, live sweep).

### Key Entities

- **Storage model**: one class's storage as data: the storage type, its SQL storage maps, default storage slots, the
  map versions and `model_version`.
- **SQL storage map**: name, type, global, subscripts, data fields, row ID parts, conditions, Row Reference, and any
  other element IRIS stores.
- **Subscript level**: position, expression, access type and the code that walks it (`NextCode`, start and stop
  values, access variables, invalid conditions, data access).
- **Data field**: column name, node, delimiter, piece, retrieval code.
- **Path operation**: `set`, `add`, `remove` or `move`, with a path into the model and a value.
- **Map version**: a fingerprint of a map as read.
- **Scratch copy**: a temporary class with a proposed map, compiled and queried, then removed.
- **Shape report**: subscript depth, node formats, piece counts, inferred types and row counts for a global, with
  values masked unless asked for.

## Success Criteria

### Measurable Outcomes

- **SC-001**: Every SQL storage map in the container's `%SYS` namespace (162 classes on IRIS 2026.2, re-counted on each image) and every class in
  the public corpus round-trips byte-identically through read and write.
- **SC-002**: Every single-field edit in the test suite changes exactly the edited element in the stored class.
- **SC-003**: Each of the nine measured traps in the Context has a test, and none of them reaches the real class
  without being refused, reported or fixed by an opt-in.
- **SC-004**: No save in the test suite loses an element without a reported error.
- **SC-005**: Every public corpus class compiles and returns its known answer row on live IRIS in CI.
- **SC-006**: An agent with only the tool descriptions and the skill can add a column to a fixture map and get the
  known answer for it, without editing class text.
- **SC-007**: The tool count grows by one, and every guard in `docs/invariants.md` passes.

## Assumptions

- IRIS 2026.2 on iris-dev-iris is the reference. CI's e2e and throwaway containers run 2026.1; a writing rule that differs
  between them is a test failure to investigate, not a reason to skip.
- `Deferred` (added in 2025.2) is part of the model; older servers are out of scope.
- Editing is limited to `%Storage.SQL` in 172. Custom storage (`%Library.CustomStorage`) and other storage classes that carry SQL maps are read-only
  until someone needs to edit them.
- Columnar maps are read-only in 172.
- A GUI is not part of 172; the contract and docs are what a community GUI would build on.
- Before 172: `iris_doc` put reads `result.status` (its own branch, committed locally), and a separate security
  check, tracked privately, is settled before 172's implementation starts. 172 builds on both.
- No comment on vscode-objectscript #1701 is part of this spec; any reply is drafted and waits for Tom.
