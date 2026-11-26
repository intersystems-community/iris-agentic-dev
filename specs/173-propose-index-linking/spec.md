# Feature Specification: `propose` reads index globals, collations and datatypes

**Feature Branch**: `172-sql-storage-maps` (173 builds on 172's unmerged work and ships with it; no separate branch)
**Created**: 2026-11-26
**Status**: Draft
**Input**: Benjamin De Boe (InterSystems) tried the 172 pre-release on a real iFind class and found `propose` the
weaker half: it samples one global and drafts a delimited data map with placeholder names, it does not recognise a
global such as `^bdb.x` as an index over the master's IdKey, and it says nothing about collations, datatypes or what it
did not infer. Make the reverse direction (from globals to maps) as strong as the forward one. Parent spec:
`specs/172-sql-storage-maps/spec.md` (User Story 7, Q9).

## Context

A developer who owns a global (`^bdb`) and an index global beside it (`^bdb.x`) wants SQL over both. The map for the
master is a draft away; the index map is the part that takes knowledge, because three things are invisible in the
data:

1. which subscript of the index is the row ID (it can be one subscript, several parts of a composite IdKey, or the
   value of the node),
2. which master field each other subscript holds, and
3. how that field was transformed on the way in: upper-cased, space-prefixed, truncated, stripped, or left as written.

The third is the trap. IRIS compares a query's value with the index through the collation named in the map's
subscript expression. A map written as `{Name}` over an index that holds upper-cased names compiles, returns the right
rows, and is never used by the optimiser: a performance bug no error reports. Today `propose` returns no index map at
all, so a developer writes that expression by hand.

Measured on IRIS 2026.2 (iris-dev-iris), before any code, each a fact the design stands on. Each line gets a live test
in the corpus (User Story 9); a line without one is an assumption until that test exists.

1. A map expression can name these collation functions, and no others: `$$SQLUPPER`, `$$SQLSTRING`, `$$STRING`,
   `$$ALPHAUP`, `$$UPPER`, `$$MVR`, `$$TRUNCATE`. `$$EXACT`, `$$PLUS`, `$$MINUS` and `$$SPACE` do not compile in a
   map expression (an undefined function call fails the class compile), so a plain `{Field}` is the exact collation.
2. `$$SQLUPPER({Name})` stores `" SMITH, ANNA"` (a space, then the value upper-cased with trailing blanks stripped,
   leading blanks kept: `"  bob  "` becomes `"   BOB"`); `$$SQLSTRING` keeps case (`" Smith, Anna"`); `$$STRING`
   upper-cases and removes spaces; `$$ALPHAUP` keeps letters and digits only, with no leading space; `$$UPPER`
   upper-cases and keeps blanks; `$$MVR` stores a numeric-sortable form; `$$TRUNCATE({F},3)` cuts to three characters.
3. A length variant, `$$SQLUPPER({Name},4)`, stores `" SMI"`: the length counts the leading space.
4. `$p($$SQLUPPER({Name}),",",1)` compiles and stores `" SMITH"` (the surname of a `Smith, Anna` name).
5. A map-compiled index stores `$c(0)` for an empty string and `-1E14` for an empty number, and `" "_$c(0)` under
   `$$SQLUPPER`. A class-compiled index stores `" "` for an empty string under the default collation. A subscript can
   also declare its own `NullMarker`.
6. The SQL planner tells you when an index map cannot serve a query: "cannot be used to satisfy the equal condition on
   %SQLUPPER(Name) because there is no index subscript collation", or "...because the index subscript collation is
   %SQLUPPER(4)". With a matching map it reads the index.
7. A bitmap index global holds `(value, chunk)` subscripts with a `$BIT` string as the node value: row ID = (chunk - 1)
   x 64000 + bit position - 1 (bit 1 is ID 0).
8. A table over a `^G(const, chunk) = $BIT` global is a data map whose third level is `AccessType Other` with an access
   variable, a `NextCode` and an `Invalidcondition`, and returns the IDs its bits name (`IadSm172.Bits`, spec 172).
9. The server's documented SQL function class (`%SYSTEM.SQL.Functions`) has exactly the collation functions of fact 1
   (plus `COLLATE`), and its results equal what a map-compiled index files for the same values.
10. A hand-written bitmap map chunks IDs only when subscript 2 is `{ID}\64000+1`, and then files `(value, chunk, 1)`;
    with `{ID}` alone it files one node per row ID, which is not a chunked bitmap.
11. A map can read the row ID from the node value: `<Data name="Id"/>` with a `RowIdSpec` of `{Id}`; the planner reads
    the index map and returns the row.
12. A conditional index map (`PopulationType conditional`) skips rows at filing time. SQL stores an empty string as
    `$c(0)`, so the condition `{F}'=""` skips NULL and does not skip an empty string.

## Clarifications

### Session 2026-11-26

Settled before this spec was written (Tom):

- Q1: Everything in one spec and one delivery: index linking, collations, bitmaps, sparse and conditional indexes,
  datatypes and the usability line. No staged v1 and v2.
- Q2: Spec 173 is its own spec, built on the 172 branch and released with it.
- Q3: The class draft is text in the answer. It writes nothing; saving it is the caller's act (`iris_doc`).
- Q4: Linking and typing return counts, positions, names and verdicts only. No value leaves IRIS, so no
  `acknowledgePhi` is needed (the one exception is a discovered null marker's literal, below).
- Q5: Every collation is computed by IRIS with its own function. iad never re-implements one.

Session 2026-11-26, `/speckit.clarify`:

- Q: What happens when one of the six unmeasured behaviours cannot be expressed in a map? → A: Measure all six first.
  A piece that cannot be expressed stops there, goes to the owner (Tom), and the spec is amended with his decision;
  meanwhile the tool returns that case under `not_inferred` and the other pieces proceed. Nothing is descoped
  silently.

## User Scenarios & Testing

### User Story 1 - Link an index global to its master and draft the index map (Priority: P1)

A developer has `^bdb` (master) and `^bdb.x` beside it. They ask `propose` for the master; the answer also lists
`^bdb.x`, says which of its subscripts is the row ID, which master field the other subscripts hold, through which
collation, and drafts the index map with the exact subscript expressions. Each draft is a guess and is marked as one.

**Why this priority**: it is Benjamin's request and the half of "global to table" that `propose` could not do.

**Independent Test**: on invented fixtures with known answers (one per transform, null marker, ID position), `propose`
returns the known field, transform, length and ID position, and each drafted index map compiles in a scratch copy and
returns the fixture's known row through an indexed query.

**Acceptance Scenarios**:

1. **Given** a master of delimited rows and an index global `(CityUpper, ID)` built through `$$SQLUPPER`, **When**
   `propose` runs, **Then** the index draft has subscript 1 `$$SQLUPPER({City})` and subscript 2 `{ID}`, with the city
   field named by the draft's field name for that piece.
2. **Given** an index whose first subscript is the raw value, **Then** the draft expression is `{Field}`.
3. **Given** one subscript produced by each of `SQLSTRING`, `STRING`, `ALPHAUP`, `UPPER`, `MVR`, `TRUNCATE(n)` and
   `SQLUPPER(n)`, **Then** each draft names that function and, where it applies, that length.
4. **Given** an index over a composite master key `^M(a,b)`, **Then** the ID is reported as the two subscript
   positions that hold `a` and `b` and the draft ends in `{Key1}`, `{IdKey}` (or the names the master draft uses).
5. **Given** an index whose value holds the row ID and whose subscripts hold only the field, **Then** the ID is
   reported as `value`, and the draft uses a row-ID spec that reads the node value (measured when built).
6. **Given** an index whose node value is a `$LIST` of master fields, **Then** the covered fields and their element
   positions are reported.
7. **Given** a subscript whose value is the piece of a name field (`Smith` of `Smith, Anna`), **Then** the draft uses
   the `$p($$SQLUPPER({F}),",",n)` form.
8. **Given** empty fields, **Then** the null marker kind found in the index is reported and, when it differs from the
   compiler's own for that type, its length; the literal and the drafted `NullMarker` come only under
   `include_values` and the data policy.

---

### User Story 1a - Add the missing index maps to a class that already has its master (Priority: P1)

A developer has a class whose master map is written and a global beside it that is an index nobody mapped. They ask
`propose` with `class` and get the index maps the class lacks, drafted in the class's own field names and row-ID
parts, ready to pass to `check` and `edit` or `replace`. An index global the class already maps is reported as
already mapped, not drafted again.

**Why this priority**: this is the common real case (Benjamin's), and it needs no renaming by hand.

**Independent Test**: a fixture class with a master map and an unmapped index global; the draft's expressions use the
class's property names and `check` compiles the class with the draft added.

**Acceptance Scenarios**:

1. **Given** a class with a `%Storage.SQL` master map and an unmapped index global, **When** `propose` runs with
   `class`, **Then** the index draft names the class's fields, not placeholders, and the row ID subscripts match the
   class's row-ID parts.
2. **Given** an index global that one of the class's index maps already uses, **Then** it is reported
   `already_mapped` with that map's name, and nothing is drafted for it.
3. **Given** a class with default storage, a columnar map, or no master map, **Then** the call is refused with the
   reason (the same read-only reasons `read` gives), since its drafts could not be saved with iad.
4. **Given** both `class` and `global`, **Then** the call is refused as `INVALID_PARAMS` (one master per call).

---

### User Story 2 - Find the index globals without being told (Priority: P1)

A developer names only the master. `propose` finds candidate index globals three ways: an explicit `index_globals`
list; other globals in the namespace whose name starts with the master's (`^bdb` finds `^bdb.x`, `^bdbI`); and
literal first subscripts inside the master (`^M("idx",...)`, `^MI("NameIdx",...)`). Each candidate is reported with
how it was found, and a candidate that does not link is still listed, with the reason.

**Why this priority**: nobody lists their index globals to a tool that is meant to find them.

**Independent Test**: fixtures with each of the three layouts; `propose` with no `index_globals` finds all of them.

**Acceptance Scenarios**:

1. **Given** `^IadSm173A` and `^IadSm173A.x`, **When** `propose` runs on the first, **Then** the second is a candidate
   found by name prefix.
2. **Given** a global that shares only a prefix and links to nothing (`^IadSm173Alpha`), **Then** it is listed as
   `not_linked` with the best agreement it reached, never silently dropped.
3. **Given** index sub-trees inside the master global itself, **Then** each distinct first subscript is a candidate.
4. **Given** the service account cannot list the namespace's globals, **Then** the answer says so and still processes
   the explicit and in-master candidates.
5. **Given** more candidates than the cap (8), **Then** the answer says which were not examined.

---

### User Story 3 - Say how sure, and what was not inferred (Priority: P1)

Every link carries a verdict, an agreement figure and the alternatives that tied; every position that no transform
explains is listed as not explained; the answer always carries `not_inferred`, naming what the draft leaves to the
caller. A reader can tell a link at 200 of 200 nodes from a guess.

**Why this priority**: the failure Benjamin named was an answer that reads as complete when it is not.

**Independent Test**: fixtures for each verdict: `inferred`, `ambiguous` (two identical columns), `too_few_nodes`
(fewer than 20), `not_linked`, `not_explained` (a computed subscript).

**Acceptance Scenarios**:

1. **Given** at least 20 usable nodes and at least 95% agreement, **Then** the verdict is `inferred`.
2. **Given** two fields that explain a position equally, **Then** the verdict is `ambiguous`, both are listed as
   alternatives, and the draft marks its pick as a guess.
3. **Given** fewer than 20 usable nodes, **Then** the verdict is `too_few_nodes` and the candidates are listed
   without a draft.
4. **Given** a subscript position that has one value across the sample, **Then** it explains no field and is reported
   `constant`.
5. **Given** a subscript produced by code (a date format, a computed key), **Then** it is `not_explained` with the
   best agreement seen, and no code is guessed.
6. **Given** any `propose`, **Then** `not_inferred` is present and names what it did not look at.

---

### User Story 4 - Bitmap indexes and bitmap-chunk data globals (Priority: P2)

A global of `(value, chunk)` subscripts holding `$BIT` strings is recognised as a bitmap index: the chunk subscript is
named, the IDs the bits hold are decoded and checked against the master, and an index map of the bitmap kind is
drafted. A data global of the shape `^G(const, chunk) = $BIT` is recognised too, and its data map is drafted with the
computed third level (`AccessType Other`, access variable, `NextCode`, `Invalidcondition`) that `IadSm172.Bits` shows.

**Why this priority**: Benjamin's own example is a bitmap global; without it `propose` misreads exactly the case
that started this.

**Independent Test**: `IadSm173` fixtures built with `%BuildIndices` (bitmap index) and seeded with `$BIT` strings
(chunk data map); the drafted maps compile and return the known IDs.

**Acceptance Scenarios**:

1. **Given** a bitmap index over a field, **Then** the answer gives `ids_checked` and `ids_found_in_master` and the
   drafted map compiles in a scratch copy.
2. **Given** a `^G(0, chunk) = $BIT` global, **Then** the drafted data map returns the IDs its bits name when the
   table is queried.
3. **Given** a bit string whose decoded IDs are not in the master, **Then** the link is `not_linked`.
4. **Given** the decode needs a chunk size or a bit offset the data does not confirm, **Then** the verdict is
   `ambiguous` and says what it assumed.

---

### User Story 5 - Sparse and conditional indexes (Priority: P2)

An index with fewer nodes than the master has rows is a sparse (conditional) index. `propose` reports the rows the
index lacks and, when a simple rule explains every missing row (the field is empty; the field equals one value), drafts
a `Condition` for the index map. When no rule fits, it says so and drafts the index without a condition, marked as
covering fewer rows than the master.

**Independent Test**: fixtures with an index that skips empty values and one that skips a fixed status; the drafted
`Condition` makes the index map return the same rows the master does.

**Acceptance Scenarios**:

1. **Given** an index that skips rows whose field is empty, **Then** the draft carries a condition naming that field
   as non-empty.
2. **Given** an index that skips rows whose status equals one value, **Then** the draft carries the matching condition.
3. **Given** missing rows that no single rule explains, **Then** no condition is drafted and the answer names the
   missing share.

---

### User Story 6 - A draft class with typed properties (Priority: P2)

Alongside the map drafts, `propose` returns class source: one property per field, a datatype guessed from the sampled
values, and a `COLLATION` parameter taken from the linking evidence. Every type is a guess and says so. The tool writes
nothing.

**Why this priority**: datatypes live on the class, not the map, so a map draft alone leaves the developer to guess
them; Benjamin raised it directly.

**Independent Test**: fixtures with integer, decimal, date-like and text columns; the answer's types and collations
are the fixture's known ones, and the drafted class compiles in a scratch copy.

**Acceptance Scenarios**:

1. **Given** a column whose every sampled value is an integer, **Then** the property is `%Integer`.
2. **Given** decimals, **Then** `%Numeric` with a scale from the longest fraction.
3. **Given** a column of whole numbers in a plausible date range that an index stores as numbers, **Then** the
   property is a date guess, reported with how many of the sampled values fit.
4. **Given** text, **Then** `%String` with a `MAXLEN` rounded up from the longest sampled value.
5. **Given** linking evidence that an index stores a field upper-cased, **Then** the property has no `COLLATION`
   parameter (the default); given raw, `COLLATION = "EXACT"`; given `SQLSTRING`, `COLLATION = "SQLSTRING"`.
6. **Given** any answer, **Then** no value from the sample appears in it.

---

### User Story 7 - `check` says whether an index map would be used (Priority: P2)

`check` compiles a proposed map in a scratch copy. It now also asks the SQL planner, in the copy, whether each index map
serves an equality query on its field, and returns the planner's own words: "used" or the warning. The line is
advisory and never refuses a write.

**Independent Test**: `{Name}` over a default-collation class gives the planner's warning; `$$SQLUPPER({Name})` gives
"used".

**Acceptance Scenarios**:

1. **Given** an index map whose expression has no collation function over a default-collated field, **Then** `check`
   returns the planner's "no index subscript collation" text for that map.
2. **Given** a matching map, **Then** it returns "used".
3. **Given** a map the planner cannot explain, **Then** the line says it could not be asked, and `check` still succeeds.

---

### User Story 8 - The same safety as the sampler (Priority: P1)

Everything above runs under the controls `propose`, `compare` and `preview` already have: it is an execute call as the
service account, refused under strict read-only, held to the node cap and a 5 s total time cap, and built from global
names that iad validated and quoted. It returns no sampled value.

**Independent Test**: unit and live tests for each control (see FRs); a hostile-name suite on `index_globals`.

**Acceptance Scenarios**:

1. **Given** `index_globals` containing a name with a quote, a closing parenthesis or a `$ZF` call, **Then** it is
   refused as `INVALID_PARAMS` before any IRIS call.
2. **Given** strict read-only, **Then** `propose` is refused as before.
3. **Given** a sensitive-named candidate global, **Then** the existing global gate judges it like the master.
4. **Given** a time cap that stops a link, **Then** the answer says which candidate and that the cap stopped it.

---

### User Story 9 - A public corpus with known answers, and honest docs (Priority: P2)

Every fixture is invented (`IadSm173...`), seeded with known rows, and carries the known answer: the ID position, the
field, the transform, the null marker, the verdict. A live test per measured fact above. `docs/storage-maps.md`, the
bundled skill and the response keys are updated and held to the code by the existing doc test.

**Acceptance Scenarios**:

1. **Given** the corpus, **Then** a test changes if any transform, marker or verdict is silently changed.
2. **Given** the docs, **Then** every key `propose` and `check` add is listed, and every measured behaviour names a
   live test that exists.

## Edge Cases

- A master with a `$LIST` row, a delimited row, or sub-nodes: fields come from the shape report, as in 172.
- An index over a field that is itself a piece of a longer value (`$p(...)`): the position is `not_explained` unless
  one of the name forms fits.
- Numeric subscripts that look like strings (`"34"` and `34`): compared as IRIS stores them, not as text.
- Non-ASCII text: the server's collation functions decide; iad never upper-cases.
- Several indexes on the same field: each is drafted with its own name.
- An index global that is also another class's master: reported by `uses` as before; `propose` does not merge them.
- A master with fewer than 20 rows: `too_few_nodes` for every candidate.
- Index globals with more than six subscripts: examined up to six, the rest reported as not examined.
- Deleted rows leave index nodes with no master row: below a 5% share they do not lower the verdict; above it they
  are reported.
- A candidate in another namespace (an extended global reference): refused as in 172.

## Requirements

### Functional Requirements

#### Finding candidates

- **FR-001**: `propose` MUST accept `index_globals` (a list of plain global names, at most 8) and MUST examine each.
- **FR-001a**: `propose` MUST accept `class` instead of `global`. With `class`, the master is the class's master map
  (its global, subscripts, fields, delimiters and pieces, read by the same reader as `read`); the master is not
  re-sampled for a draft; only the index drafts, the class-draft additions and the `not_inferred` list are returned.
  A class whose storage is not an editable `%Storage.SQL` with a master map MUST be refused as in `read`'s reasons.
  Giving both `class` and `global`, or neither, MUST be `INVALID_PARAMS`. A candidate global that one of the class's
  index maps already uses MUST be reported `already_mapped` with the map's name. The names in drafts MUST be the
  class's own, and `field_names` MUST be refused with `class`.
- **FR-002**: `propose` MUST list, as candidates, globals in the namespace whose name begins with the master's name
  (the master's own name excluded), and literal first subscripts of the master global that hold sub-trees. The answer
  MUST say how each was found (`parameter`, `name_prefix`, `sub_node`).
- **FR-003**: A candidate that does not link MUST still be listed, with the verdict `not_linked` and the best agreement
  reached. More than 8 candidates MUST be capped, and the answer MUST name those not examined.
- **FR-004**: When the namespace's globals cannot be listed, the answer MUST say so and continue with the other
  sources.

#### Linking

- **FR-004a**: Each candidate MUST be sampled as a spread: up to 3 nodes per distinct value of the first subscript,
  hopping from one value to the next in order, until the node cap. A candidate whose first subscript has fewer
  than 7 distinct values MUST be sampled in order to the cap instead. The report MUST say how many distinct first
  values were seen, and the sample MUST NOT be used for any claim about uniqueness or cardinality.
- **FR-005**: For each candidate, `propose` MUST find the row-ID position or positions: a run of consecutive
  subscripts as long as the master's key (one or more), or the node value. It MUST count the nodes whose ID resolves
  to a master row and report that share.
- **FR-006**: For every other subscript position and every master field, `propose` MUST test, on the sampled nodes,
  whether the field's value, transformed by one of the forms in FR-007, equals the subscript, and MUST report the best
  field and transform and its agreement.
- **FR-007**: The transforms MUST be exactly: the value as written; `SQLUPPER`, `SQLSTRING`, `STRING`, `ALPHAUP`,
  `UPPER`, `MVR`; each of `SQLUPPER`, `SQLSTRING`, `STRING` with a length; `TRUNCATE` with a length; and the name-piece
  forms of `SQLUPPER`, `SQLSTRING`, `STRING`, `ALPHAUP`, `UPPER`, `MVR`, `TRUNCATE` (`$p(...,",",1)` and `,2`).
  `COLLATE` is recognised, not drafted (FR-032). A transform MUST be applied by the IRIS server through its documented SQL function class (fact 9); no
  Rust or ObjectScript text of iad's may re-implement one.
- **FR-008**: A length variant's length MUST be found from the data (the longest subscript seen at that position) and
  MUST be confirmed on every node before it is reported.
- **FR-009**: Null markers MUST be matched per field: `$c(0)`, `-1E14`, `" "`, `" "_$c(0)`, and any single constant
  that every empty-field node shares, which MUST be reported as a discovered marker: its length always, its literal
  only under `include_values` and the data policy of FR-021 of spec 172 (refused under `block` and `redact`,
  `acknowledgePhi` under `allow`). Without the literal the draft leaves `NullMarker` unset and says what is missing.
- **FR-010**: An index node whose value is a `$LIST` MUST be tested element by element against the master's fields
  (covering data), and each covered field and position reported.
- **FR-011**: A node is _usable_ when its row ID resolves to a master row. A link MUST be called `inferred` only with at
  least 20 usable nodes and agreement of at least 0.95. Two or more (field, transform) pairs within 0.01 of the best
  score are a tie, and ties MUST give `ambiguous` with every tied field and transform listed; `<20` nodes MUST give `too_few_nodes`; no ID
  position at 0.9 MUST give `not_linked`; a position no form explains MUST be `not_explained`; a position with one
  value MUST be `constant`.
- **FR-012**: Linking MUST NOT return any sampled value, except a discovered null marker under FR-009's rule. It
  returns counts, positions, field and transform names, lengths, marker kinds and verdicts only.

#### Drafting

- **FR-013**: For each linked candidate `propose` MUST return an index map draft in the model's JSON form, using only
  the expression spellings of FR-007, `{ID}` or the master draft's key names, the global, and every element it
  guessed in `guesses`.
- **FR-014**: Every drafted map MUST pass the model validator and the write path's data checks that need no IRIS call
  before it is returned; one that fails MUST be returned as an item of `not_inferred` and `not_inferred_detail` with the reason.
- **FR-015**: Index map names MUST come from the global's last name segment, made unique when two collide.
- **FR-015a**: `propose` MUST accept `field_names`, a list of names by field position (the order of the master draft's
  placeholders). The master draft, every index draft, the class draft and `not_explained` MUST use them; a list shorter
  than the field count MUST be refused as `INVALID_PARAMS` naming the count the sample found, a name that is not a
  valid property name MUST be refused, and without the list the placeholders of the master draft are used.

#### Bitmaps

- **FR-016**: A candidate whose nodes hold bit strings MUST be reported as a bitmap index. The chunk subscript is the
  numeric position whose decoded IDs resolve to master rows, tried as the last subscript and as the one before a
  constant last subscript (fact 10). The decoded IDs are checked against the master (up to 1000 IDs in 5 chunks),
  `ids_checked` and `ids_found_in_master` reported, and the subscripts before the chunk explained as in FR-006.
- **FR-017**: A bitmap index map MUST be drafted when the link is `inferred`, as `Type bitmap` with the field's
  expression and `{ID}\64000+1` (fact 10), and MUST be verified in a scratch copy before it is called inferred.
- **FR-018**: A master global of the shape `^G(const, chunk) = $BIT` MUST be recognised and its data map drafted with
  the computed level (access variable, `NextCode`, `Invalidcondition`) of spec 172's `IadSm172.Bits`; the draft MUST
  be checked, in a scratch copy, to return the IDs the bits name before it is called inferred.

#### Sparse indexes

- **FR-019**: When the index lacks master rows (more than 5% of the sample), `propose` MUST report the share and try
  these rules, in order, on the missing rows: field NULL; field an empty string; field equals one value (fact 12: NULL
  and an empty string are different rules). A rule MUST explain every missing row and no present row to be drafted as a
  `Condition` with `PopulationType conditional`, and MUST be verified in a scratch copy (same rows through the map as
  through the master); otherwise the answer MUST say no rule fit.

#### Datatypes and the class draft

- **FR-020**: `propose` MUST return `class_draft`: class source with one property per field, a datatype from server-side
  verdicts over the sampled values (integer, numeric with a scale, date-like with a fit count, string with `MAXLEN`),
  and `COLLATION` taken from the linking evidence. Every property MUST be in `guesses`.
- **FR-021**: The verdicts MUST be computed on the server and return counts and names only (FR-012). The class draft MUST
  NOT contain a sampled value.
- **FR-022**: `class_draft` MUST be compiled in a scratch copy with the drafted maps before it is returned, and the
  answer MUST say whether it compiled.

#### `check`

- **FR-023**: `check` MUST, for each index map of the proposed class and each subscript of it that names a field, ask
  the planner in the scratch copy whether an equality query on that field and the fields before it uses the map, and
  return the planner's own text or "used". The line is advisory (`warnings`); it MUST NOT make `check` fail, and an
  unanswerable question MUST say so.

#### Safety and shape

- **FR-024**: `propose` MUST remain an execute call as the service account, refused under strict read-only, held to
  the node cap (default 200, max 1000) per candidate and 5 s in total, with the response saying which cap stopped what.
- **FR-025**: Every global name in generated code MUST come from `validate_global_name` and `os_str_expr`; a name in
  `index_globals` that fails validation MUST be `INVALID_PARAMS` before any IRIS call. Every candidate MUST pass the
  existing global gates as the master does.
- **FR-026**: The response shape MUST be additive: existing keys stay; `MODEL_VERSION` stays 1; new keys appear only on
  `propose` and `check`. Golden files for 172 MUST NOT change except where a new key is added.
- **FR-027**: `propose` MUST write nothing: no class, no global, no scratch left behind (scratch copies are removed as
  in FR-018 of 172).
- **FR-028**: `not_inferred` (an array of strings, as in 172) and `not_inferred_detail` (the same items as
  `{what, why}`) MUST always be returned and MUST name, at least, what was not examined or not explained in
  this call.

#### Measured claims

- **FR-029**: Each numbered fact in Context MUST have a live test, and the docs and skill MUST cite it. A fact that no
  test holds MUST NOT appear in the docs.
- **FR-030**: `docs/storage-maps.md`, the `iris-storage-mapping` skill and the glossary (`CONTEXT.md`) MUST describe the
  new keys and terms, and the existing doc test MUST cover them.
- **FR-031**: The fixtures MUST be invented for this repository: own class names under `IadSm173`, own globals, own
  values. Nothing may be copied from an InterSystems class or product source.
- **FR-032**: Anything the data could mean but the tool does not draft (`%COLLATE`, numeric collations, computed
  subscripts such as date formats) MUST be named in `not_inferred` for that call, never guessed.

### Key Entities

- **Candidate index global**: a global that might be an index over the master, with how it was found.
- **Link**: the result of testing a candidate against the master: ID position(s), per-position field and transform,
  agreement, verdict, alternatives.
- **Transform**: one of the collation forms of FR-007 as applied to a field to give a subscript.
- **Index map draft**: a map in the model's JSON form, a guess.
- **Class draft**: text of a class with typed properties, a guess.
- **Usability line**: the planner's statement of whether an index map serves a query.

## Success Criteria

### Measurable Outcomes

- **SC-001**: On the corpus, every fixture's known field, transform, length, null marker, ID position and verdict is
  returned (100%; the corpus holds at least 30 fixtures).
- **SC-002**: Every drafted index map compiles in a scratch copy and returns the fixture's known row through an indexed
  query (100% of `inferred` drafts).
- **SC-003**: No fixture in the false-link set (age equal to ID, two identical columns, a constant column, a dense ID
  run, an unrelated global with a shared prefix) is called `inferred` for the wrong field.
- **SC-004**: No sampled value appears in any `propose` or `check` answer (a test scans every corpus answer for the
  seeded values).
- **SC-005**: A `propose` over a master and two index globals of 1000 sampled nodes each finishes inside the 5 s cap
  on the local container.
- **SC-006**: A developer given a master and index globals gets index maps whose collation matches the data without
  writing an expression, and the planner reports "used" for each (live test over the corpus).

## Assumptions

- The corpus and every measured claim are on IRIS 2026.2 (`iris-dev-iris`); another build is not claimed.
- Facts 1 to 12 in Context are measured (`research.md`, R1 to R7). Three behaviours are still unmeasured and are the
  first live tasks of their phases: whether a low-privilege service account can list the namespace's globals (the
  answer has a failure path either way), whether the condition `{F}'=$c(0)` works in a map (an empty-string rule is
  verified in a scratch copy and falls to `not_inferred` if it does not), and whether a spread sample separates what a
  prefix walk cannot (FR-004a; amended with the owner if it does not).
- A requirement whose research task fails stops that piece, is reported to the owner, and is changed in this spec
  only with the owner's decision (Clarifications, 2026-11-26). Until then the tool returns that case under
  `not_inferred`; the other pieces are not held.
- The sample is a spread across the index (FR-004a), so it supports linking by lookup and telling collations apart, not
  claims about uniqueness or cardinality; none are made. Typing reads the master rows the linker reached through the
  sampled IDs, and the first nodes of the master up to the cap.
- Date-like detection is a guess by design, reported with its fit count.
- `%COLLATE` (encoded custom collations) and computed subscripts such as date formats are recognised and named in
  `not_inferred`, not drafted.
