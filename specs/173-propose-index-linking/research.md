# Research: `propose` reads index globals, collations and datatypes (173)

Every claim below was measured on IRIS 2026.2 (`iris-dev-iris`, local Docker, USER namespace) on 2026-11-26 with
classes written for this repository (`Iad173.*`, removed afterwards). Each one becomes a live test in the corpus (task
list, Phase 0). A claim without its test is an assumption.

## R1. How a hand-written bitmap index map is declared

**Decision**: the draft is a map of `Type bitmap` with subscript 1 `{Field}` and subscript 2 `{ID}\64000+1`.

**Measured**:

| Map subscript 2                                      | Class declares an `Index … [Type = bitmap]` | What IRIS files for rows 1 and 3 with status `A`                                  |
| ---------------------------------------------------- | ------------------------------------------- | --------------------------------------------------------------------------------- |
| `{ID}`                                               | no and yes (same result)                    | `("A",1,1)` and `("A",3,1)`, one node per row ID, each with one bit               |
| `{ID}\64000+1`                                       | yes                                         | `("A",1,1)` holding the bits for IDs 1 and 3 (`$bit(2,4)`)                        |
| (class-compiled, `Type = bitmap` in default storage) | n/a                                         | `("<IndexName>"," A",1)` holding `$bit(2,4)`: index name, value, chunk, no suffix |

`%BuildIndices` on the class gave the same nodes as INSERT for both map shapes.

**Consequences**:

1. A map whose subscript 2 is `{ID}` compiles and files, but it is not a chunked bitmap: each node is one row. A draft
   that used it would be silently wrong. The draft uses `{ID}\64000+1`.
2. A hand-written bitmap map files `(value, chunk, 1)`: the chunk is the second-to-last subscript and the last
   subscript is a constant `1`. The linker finds the chunk subscript as the numeric position whose decoded IDs resolve
   to master rows, and tries both the last subscript and the one before a constant last subscript. A class-compiled
   bitmap has no trailing constant. Both shapes are fixtures.
3. Decode, spec context fact 7: ID = (chunk − 1) × 64000 + bit position − 1. Holds for both.

**Alternatives**: a bitmap map with `{ID}` alone (rejected: not chunked); drafting from the class-compiled layout
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

**Consequence**: the draft's ID part names are the master's own (class mode) or the draft's placeholders `Key1…Keyk`
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

## Not decided here

- Whether the usability line (US7) can read the planner's text through a documented interface rather than the
  `EXPLAIN` result. `EXPLAIN SELECT …` through `%SQL.Statement` returned the plan as XML text with the line "Read
  index map …" (R2) and the warning forms of spec context fact 6; the plan uses that, and the live test pins the two
  strings.
