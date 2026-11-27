# Research: `propose` reads index globals, collations and datatypes (173)

Every claim below was measured on IRIS 2026.2 (`iris-dev-iris`, local Docker, USER namespace) on 2026-11-26 with
classes written for this repository (`Iad173.*`, removed afterwards). Each one becomes a live test in the corpus (task
list, Phase 0). A claim without its test is an assumption.

## R1. How a hand-written bitmap index map is declared

**Decision**: the draft is a map of `Type bitmap` with one subscript, `{Field}` (through the Link's transform). The
chunk is appended by IRIS as the last subscript.

**Measured** (`fact_10_bitmap_map_layout`), rows 1 and 3 with status `A`, row 2 with `B`:

| Map subscripts                 | What IRIS files                                                                 |
| ------------------------------ | ------------------------------------------------------------------------------- |
| `{Status}`                     | `("A",1)` holding `$bit(2,4)` and `("B",1)` holding `$bit(3)`: `(value, chunk)` |
| `{Status}`, `{ID}`             | `("A",1,1)`, `("A",3,1)`, `("B",2,1)`: one node per row ID, the chunk last      |
| `{Status}`, `{ID}\64000+1`     | `("A",1,1)` holding the bits of IDs 1 and 3: the chunk repeated                 |
| class-compiled `Type = bitmap` | `("<IndexName>"," A",1)` holding `$bit(2,4)`: index name, value, chunk          |

**Consequences**:

1. The chunk is always the last subscript, in every shape. The linker takes it as the last subscript and confirms it by
   decoding: the IDs the bits name must resolve to master rows.
2. A second subscript only multiplies nodes, so the draft carries the field alone. An earlier draft of this note
   (`{ID}\64000+1` as subscript 2) filed a repeated chunk and is dropped. The scratch verification of FR-017 still
   decides whether a drafted bitmap map is called inferred.
3. Decode, spec context fact 7: ID = (chunk − 1) × 64000 + bit position − 1, checked with an ID in chunk 2
   (`A@64000`). Holds for map-compiled and class-compiled shapes.

**Alternatives**: a bitmap map with `{ID}` as a second subscript (rejected: one node per row); drafting from the class-compiled layout
(rejected: that layout belongs to default storage, a different index name prefix).

## R2. How a map reads the row ID from the node value

**Decision**: the index map carries `<Data name="Id"/>` (no node, delimiter or piece, so the whole node value) and a
`RowIdSpec` `1` with expression `{Id}`. A `Field` is named in the RowIdSpec.

**Measured**: with subscript 1 `$$SQLUPPER({Name})` and that Data and RowIdSpec, INSERT filed
`^G(" BOB")=2`, and `SELECT %ID, City … WHERE Name = 'bob'` returned row 2 with the planner line "Read index map
Iad173.PVal.MVal". A query that matches nothing returned no rows. The class had no property named `Id`; the map
compiled.

**Consequence**: US1 scenario 5's "row-ID spec that reads the node value" is this form. The linker reports `form: value`.

## R3. What a `Condition` on an index map does with the rows it skips

**Decision**: draft `Condition` together with `PopulationType conditional` (spec 172's fixture `ActiveLoan` does the
same). A rule is verified in a scratch copy: the same rows come back through the map as through the master.

**Measured** (hand-written index maps, `PopulationType conditional`):

- `Condition {Status}="A"` filed rows with status `A` only; rows with `B` have no node.
- `Condition {City}'=""` skipped a row inserted with `City` NULL, and did **not** skip a row inserted with
  `City = ''`: SQL stores an empty string as `$c(0)`, and `$c(0)'=""` is true. That row has an index node
  `($c(0), id)`.
- Without `PopulationType conditional` and with `{City}'=""`, the empty-string row was filed the same way, so that
  case does not tell us whether the element is needed.

**Consequence**: "field is empty" is two cases. The linker classifies the missing rows by the master's stored piece:
empty (NULL) or `$c(0)` (empty string), and reports the kind. A NULL rule drafts `{F}'=""`; an empty-string rule
drafts `{F}'=$c(0)`. The second form is verified by the scratch compile and query before it is returned (it is an
unmeasured expression until its test exists; if it fails the case goes to `not_inferred`).

## R4. `$$COLLATE` argument forms

**Decision**: `COLLATE` stays recognised and not drafted (spec FR-032). No transform of FR-007 uses it.

**Measured**: `$$COLLATE({Name},"SQLUPPER")` compiles and stores the value unchanged (the second argument is a
translation spec, not a collation name); `$$COLLATE({Name})` compiles and stores the value unchanged;
`$$COLLATE({Name},7)` fails the class compile (ERROR #398: the truncate length should be an integer). Separately, the
expression `" "_{Name}` compiles and stores `" Jones"` and `" Smith, Anna"`, the same strings `$$SQLSTRING({Name})`
stores for those values.

**Consequence**: a data set whose index holds `" "_value` is explained by `$$SQLSTRING` and by `" "_{F}` equally on
short clean values; the linker's candidate is `$$SQLSTRING`, listed with the verdict `inferred` only when a value
separates them (trailing blanks), else `ambiguous` with `" "_{F}` named in `alternatives`.

## R5. What the service account needs to list the namespace's globals

**Decision**: use the class query `%SYS.GlobalQuery:NameSpaceList` with mask `<master name>*`; on any failure the
answer says the listing was not available (FR-004) and the other two candidate sources still run. This is a documented
class query, not an internal routine.

**Measured**: as the instance owner it returned the 22 `Iad173P*` globals of the USER namespace (name only, no data
read). A user holding only `%DB_USER` could not run the check: the session refused with `Access Denied` before the
query ran, which says nothing about the query itself.

**Open, first live task of the candidates phase**: the same query over the Atelier-driven generator path as the
service account the fixtures configure, with a user that has no `%Admin_*` role. The design does not depend on the
result: the failure path is a feature (FR-004, US2 scenario 4), and the test holds both outcomes.

## R6. How a multi-part key is expressed in a draft

**Decision**: a master with an IdKey of k parts keeps spec 172's form: `RowIdSpec n` with `{Ln}` and the property
name as `Field`, and subscripts `{A}`, `{B}`. An index map over it names the parts as subscripts: `$$SQLUPPER({Name})`,
`{A}`, `{B}`, with no RowIdSpec.

**Measured**: class with `Index IDKEY On (A, B) [IdKey, Unique]`, master map with `RowIdSpec` `{L1}`/`{L2}`, and an
index map with subscripts `$$SQLUPPER({Name})`, `{A}`, `{B}`. INSERT filed `(" SMITH","x",1)`, `(" SMITH","x",2)`,
`(" JONES","y",1)`; `SELECT A,B … WHERE Name = 'SMITH' ORDER BY B` returned `x 1` and `x 2` (the default collation
matched `Smith` and `smith`).

**Consequence**: the draft's ID part names are the master's own (class mode) or the draft's placeholders `Key1…Key(k-1)` and `ID`
(global mode). The linker reports k positions.

## R7. The server's own collation functions, and why not the internal call

**Decision**: the linker calls `##class(%SYSTEM.SQL.Functions).<Name>(value[,length])`, the documented SQL function
class. Its methods are `ALPHAUP`, `COLLATE`, `MVR`, `SQLSTRING`, `SQLUPPER`, `STRING`, `TRUNCATE`, `UPPER`: the closed
set of spec context fact 1, with no `EXACT`, `PLUS`, `MINUS` or `SPACE`.

**Measured** on `"  Smith, Anna-2  "`: `SQLUPPER` → `"   SMITH, ANNA-2"` (leading blanks kept, trailing stripped, one
space prefixed); `SQLUPPER(v,4)` → `"   S"` (the length counts the prefix); `SQLSTRING` → `"   Smith, Anna-2"`;
`STRING` → `" SMITH,ANNA2"`; `ALPHAUP` → `"SMITH,ANNA2"`. These equal what a map-compiled index filed for the same
values (R1 to R6 tables).

**Consequence**: spec FR-007 is met by calling the server's documented functions; iad has no collation of its own, and
the generated code needs no internal function number.

## R8. Sampling a spread

**Decision**: per FR-004a, the linker walks the candidate with `$Order` on the first subscript and takes up to three
nodes under each value, hopping to the next, until the node cap; fewer than seven distinct first values means an
ordered walk to the cap. The existing sampler (`sample.rs`) is a prefix walk and stays as is for the master.

**Why**: a prefix walk of an index over `Name` sees the low end of the key range only; a transform that differs for
values further on (`$$ALPHAUP` against `$$UPPER` on punctuation) might never separate. That is reasoning, not a
measurement: the corpus gets a fixture with the separating values at the far end of the range, and a test that a prefix
walk would have called it `ambiguous` and the spread calls it `inferred`. If that test cannot be made to fail on the
prefix walk, FR-004a is amended with the owner.

## R9. Where the work runs, and what returns

**Decision**: the linker is generated ObjectScript, run through `execute_via_generator_prefixed(…, "IrisDevSm")` like
the sampler: the service account, one call per candidate plus one for typing, plus one for the namespace listing.
Only counts, positions, names and verdicts come back, as JSON lines the pure parser reads (as `sample::parse` does).

**Cost shape** (budget test): one IRIS call for the listing, one per candidate (at most 8), one for typing, one for
the class-draft scratch compile and one per drafted index for its verification. A test fails if a `propose` over a
master and two candidates makes more than the table in `plan.md` allows.

## T010 outcomes (2026-11-26, IRIS 2026.2, `test_storage_map_measured_173_live`)

All 17 tests pass (`fact_1` to `fact_12`, `r4_collate_forms`, `r6_multipart_key`, three `open_`). Four findings change
the design, each now written into the spec and tasks:

1. **NULL and empty string are different markers** (fact 5). Map-compiled: `$c(0)` for an empty string; class-compiled:
   `" "_$c(0)` for an empty string and a lone `" "` for a NULL. The linker reports the kind.
2. **Bitmap layout** (R1, fact 10): one subscript files `(value, chunk)`; the chunk is always last; a second subscript
   only adds nodes. The draft carries the field alone. The `{ID}\64000+1` draft this spec first named was wrong.
3. **Spread** (R8, `open_spread_beats_prefix`): a prefix walk cannot separate `$$UPPER` from `$$ALPHAUP` when they
   differ only late, and a stride over the whole key range can, but a stride needs the key list first. FR-004a now has
   a counting pass under its own cap (10 times the node cap).
4. **`{City}'=$c(0)`** (R3, `open_c0_condition`): compiles in a conditional map and skips exactly the empty-string rows.
   The empty-string rule is drafted as `{F}'=$c(0)`.

Still open: the listing as a user with no administrative role (R5). `open_listing_as_the_connection_user` lists the
fixture's globals as the rig user. The failure path is built and tested in the candidates phase (T040) either way.

## Not decided here

- Whether the usability line (US7) can read the planner's text through a documented interface rather than the
  `EXPLAIN` result. `EXPLAIN SELECT …` through `%SQL.Statement` returned the plan as XML text with the line "Read
  index map …" (R2) and the warning forms of spec context fact 6; the plan uses that, and the live test pins the two
  strings.

## iFind real index (T072)

Measured 2026-11-27 on `iris-dev-iris` (IRIS Community 2026.2, USER). Corpus: 60 invented sentences of three words
around an `and` (plus a repeated word every fourth row and a `ZephyrN!` token every tenth). Two real indexes, both built
by IRIS itself: the basic form (`Index Idx On (Txt) As %iFind.Index.Basic(LANGUAGE="en")` on a `%Persistent` class with
default storage) and Benjamin's recipe (`create table ... (str varchar(10000))` then
`create index AnalyticIDX on ... (str) as %iFind.Index.Analytic(IFINDMAPPINGS = 1)`). The class text of
`%iFind.Index.Basic` and the storage map of `%iFind.OriginalWord` were read with `iris_doc get`; this note paraphrases
them. Observations are layouts and counts only. Test: `test_storage_map_ifind_173_live.rs` (pins the numbers below).

### What each index files

| Global                                        | Subscripts (after the global name)                | Value                                                                |
| --------------------------------------------- | ------------------------------------------------- | -------------------------------------------------------------------- |
| `^Pkg.ClassD` (master, default storage)       | root: nothing; `ID`                               | root = ID counter; row = `$LIST` of (class name, Title, Txt)         |
| `^Pkg.ClassI`, `"$Doc",chunk`                 | `"$Doc"`, chunk                                   | bit string; bit n+1 set = row n exists                               |
| `^Pkg.ClassI`, `"Idx","L",lang,chunk`         | language id (1 for `en`), chunk                   | the same bit string                                                  |
| `^Pkg.ClassI`, `"Idx","W",wordID`             | word ID                                           | count of rows holding the word                                       |
| `^Pkg.ClassI`, `"Idx","W",wordID,chunk`       | word ID, chunk                                    | bit string of the rows that hold the word (bit n+1 = row n)          |
| `^Pkg.ClassI`, `"Idx","P",rowID`              | row ID                                            | count of token positions in the row                                  |
| `^Pkg.ClassI`, `"Idx","P",rowID,wordID,chunk` | row ID, word ID, chunk                            | bit string of the word's positions in the row (bit p+1 = position p) |
| `^IRIS.IF.WordD`, `^IRIS.IF.WordI`            | word ID; `" "_word` (a leading blank)             | word; word ID (the namespace's shared dictionary)                    |
| `^IRIS.IF.TransfoB(0,chunk)`                  | `0`, chunk = `wordID\64000+1`                     | bit string; bit n+1 set = word ID n (every word of the dictionary)   |
| `^IRIS.IF.WordRI`, `WordCP`, `EntityD/I/W/WP` | reversed words, code points, entity IDs and spans | scalars, `$LIST`s and bit strings, filled by the same index          |

Facts the layout settles:

1. **No word is a subscript of the class's own index global.** The key is a word ID. The words sit in `^IRIS.IF.WordD`
   and `^IRIS.IF.WordI`, namespace-wide globals that the class's storage does not name. A subscript-to-field linker
   cannot explain a word ID from a master field.
2. **Row IDs are bit positions, not subscripts**, in `"$Doc"`, `"L"` and `"W"`. Only the `"P"` nodes carry a row ID as a
   subscript (position 3 of `("Idx","P",row,word,chunk)`). The chunk subscript is last and is 1 for every ID below 64000.
3. **The first levels are constants of different branches** (`"Idx"`, then `"L"`, `"P"` or `"W"`): one global holds
   several layouts.
4. **The analytic form files the same trees and more**: its `^<ext>.3` index global holds `E`, `L`, `P`, `W`, `c`, `d`,
   `p`, `x` and `xio` (1,066 nodes for 60 rows against 364 for the basic form). The table's globals are hashed
   (`^Cd34.E0li.1` rows, `.2` the extent bitmap, `.3` the index, `.I`, `.S`), so the index shares no name prefix with the
   master beyond the extent. A DDL table's IDs come from `$SEQUENCE`, so they are not 1..60.
5. **`%iFind.OriginalWord` maps `^IRIS.IF.TransfoB`**: level 1 the constant 0, level 2 `{OriginalWord}\64000+1` (an
   access variable `{2D2}` set by code to the chunk's base ID), level 3 `AccessType Other` with a `NextCode` that walks
   the chunk's bit string with `$BITFIND` and an invalid-condition on `$BIT`. The global has **two** subscripts; the
   third level exists only as code. This is the shape Benjamin described, and `propose` already drafts it (below). His
   note gave the chunk as `wordID\64000 - 1`; the measured chunk is `wordID\64000+1` (1 for IDs below 64,000), and `-1`
   appears only in the access variable, where `{2D2}=({L2}-1)*64000-1` is the base, so bit position = ID - base = ID + 1
   in the first chunk.
6. The shared `^IRIS.IF.*` globals exist in USER only once an iFind index has been filled; `%SYS` held none.
   `IFINDMAPPINGS = 1` adds the word-spread and word-position SQL projections; no global named for
   `%iFind.OriginalWord` appears (the class is a map over `TransfoB`).

IRIS quirk met while loading (not iFind): the first INSERT into a DDL table that is re-created over globals killed by
hand can fail with `<ILLEGAL VALUE>` at `%SQLInsert+13`; the retry works. It happened with and without the analytic
index. The test loader retries and asks IRIS for the IDs.

### What `propose`, `read`, `diagram` and `compare` say today

| Call                                                | Answer                                                                                                                                                                                                                                                                                                                |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `propose global=^Pkg.ClassD`                        | Master drawn correctly (`$LIST`, three pieces, 60 IDs). `index_candidates.candidates = []`: the sibling `^Pkg.ClassI` is not offered, and nothing says none was found.                                                                                                                                                |
| same, `index_globals=[^Pkg.ClassI]`                 | Verdict `not_explained`, honest, no map. But `id.positions=[5]` (agreement 1.0; position 5 is the chunk, 1 in every node) and `missing_rows` says the index holds only the rows where `{Field2}` is one value (false: all 60 rows are in it). The node cap (200 of 364) stopped the walk.                             |
| `propose global=^Pkg.ClassI` (index as master)      | Draws a data map with `Key1..Key4`, `{ID}` and four fields split on `$c(1)`: the bytes of the bit strings read as delimiters. No `bitmaps` item.                                                                                                                                                                      |
| analytic, master `^Cd34.E0li.1` alone               | Master drawn; no candidates and no statement (the index shares no name prefix with it).                                                                                                                                                                                                                               |
| analytic, `index_globals=[^Cd34.E0li.3]`            | `too_few_nodes` (25 nodes sampled, 11 usable); `id.positions=[3]` with agreement 1.0 (the chunk again); `missing_rows` 23 of 60.                                                                                                                                                                                      |
| `propose global=^IRIS.IF.TransfoB`                  | `chunk_map_draft` is right: constant 0, `{Key1}\64000+1`, computed level 3; verified compiled, 25 rows, ID sum 325 (1..25). The plain `draft` beside it is the `$c(1)` nonsense again.                                                                                                                                |
| `read class=%iFind.OriginalWord`                    | `derived` carries "level 3 is computed by code ... the level may not exist in the global ... check the map against real data" (second entry, after the access-variable fact). `warnings` is empty.                                                                                                                    |
| `diagram` of it                                     | The fact is the last paragraph of `markdown`. The graph and the mermaid draw `sub 3: {OriginalWord} (Other)` like any subscript: three levels for a two-subscript global.                                                                                                                                             |
| `compare` of it                                     | `mismatches=[]`, `verified=true`, and a note: "the map declares 3 levels, of which level 3 is computed by code; the data sits at depth 2".                                                                                                                                                                            |
| `compare` and `propose` over a global with no nodes | `compare`: note "no data found at ^G, the map was not checked", yet five `no value there` mismatches. `propose`: a successful, empty draft (`data: {}`); only `shape.nodes_scanned = 0` says there was nothing; with `index_globals` the candidate is `not_examined` because "a master draft has at least one field". |
| Benjamin's `^bdb` (3 rows) with `^bdb.x`            | Link `too_few_nodes`, no map, `id.form=value`. The detail names no minimum (20 usable nodes) and does not say what more rows would do. `Sub1` (the `"sub"` node, 99934.1) is typed `%String` with evidence `max_length 0`.                                                                                            |
| the same shape with 40 rows                         | Link `inferred`: `Field1`, `exact`, ID in the value; the draft compiles, finds a row, and the planner uses it. Types: `Field4` `%Integer` (40 of 40).                                                                                                                                                                 |

### Benjamin's description against what iFind files

- "a master `^bdb` with `^bdb.x` holding the index, an IdKey": true of his hand-built shape, and `propose` links it once
  there are 20 rows. A real iFind index is `^<Class>D` plus `^<Class>I` (`D` and `I`, not `.x`), with the words not in
  it at all.
- "NextCode third level on a bitmap-like global": matches `%iFind.OriginalWord` over `^IRIS.IF.TransfoB` exactly. That
  is the one real iFind structure `propose` handles (the chunk map), and its answer is right.

### What the linker would need to learn (T076 to T083 did 1 to 4 and 6 to 8; 5 is not done)

1. Offer the stem sibling: a default-storage class files `^XD` and `^XI`, and the name-prefix rule misses it; a DDL
   table's index is `^<ext>.3` beside `^<ext>.1`. Say when the candidate list is empty.
2. Treat a global whose first levels are constants of different branches (`"Idx","W"` against `"Idx","P"`) tree by
   tree, and report the ID where it really is (a bit position, or position 3 of `"P"`).
3. Never report a position as the row ID when it holds one value across the nodes read; never state a `missing_rows`
   rule from one-value subscripts.
4. Do not read a bit string as `$c(1)`-delimited fields (run the bitmap detector on the master too), and drop the plain
   `draft` when a `chunk_map_draft` explains the global.
5. A word ID has no master field. The honest answer is "the keys are word IDs held in this namespace's
   `^IRIS.IF.WordD`", which `propose` could state when the first subscripts match that dictionary.
6. Say "no data found" in `propose`, and keep `compare` from listing mismatches it did not check.
7. Name the 20-node minimum when a link is `too_few_nodes`, and count the values of `"sub"`-node fields when typing.
8. Mark a computed level in the diagram's graph, not only in the markdown.
