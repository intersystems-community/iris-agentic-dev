# Release notes draft: SQL storage maps (spec 172)

## What's new

`iris_storage_map` edits SQL storage maps without breaking the table. Classes that lay a table
over existing globals with hand-written `%Storage.SQL` maps used to need Studio's map editor, which
never looks at the data. The new tool reads a class's maps as JSON, draws them as Mermaid or a
Markdown page, finds every class that uses a global, and checks a map against the real data before
anything is saved.

- `compare` samples the global and lists where the map and the data disagree: a field on a piece
  the data does not have, `$LIST` against `^`-delimited text, nodes no field reads.
- `check` compiles a proposed map in a temporary copy of the class and deletes it. Compiling the copy
  writes nothing to the real globals; I measured that.
- `preview` shows the rows a proposed map would return, with values masked unless you ask for them
  and the data policy allows it.
- `edit` and `replace` change a map only if nobody else changed the class since you read it, read
  it back to make sure IRIS kept what was sent, and put the old version back if the compile fails.
- `propose` drafts a map for a global that has none, and reads the index globals beside it (below).

Before a write, iad refuses three mistakes that IRIS accepts without an error: an ID counter behind
the highest stored ID (the next INSERT would overwrite row 1), a new index over existing rows that
was never built (queries would find nothing), and a map that would read a parent's named node as a
row. `seed_counter` and `build_indexes` fix the first two in the same call.

A new skill, `iris-storage-mapping`, walks an agent through the order that keeps a table working:
read, compare, check, then edit. `docs/storage-maps.md` has the full reference.

### `propose` reads index globals

Until now `propose` sampled one global and drafted one data map with placeholder field names. An index
global such as `^bdb.x` beside a master `^bdb` was invisible. A user trying the pre-release on
`%iFind.OriginalWord` asked for exactly this.

`propose` now finds the index globals on its own: the ones you name in `index_globals`, the globals in the
namespace whose names start with the master's, and literal first subscripts inside the master. It reads at most
eight per call and lists the rest as not examined. `propose` with `class` does the same for a class that already
has maps, and says which candidates the class already maps.

For each candidate it works out where the row ID sits (one part of the key, several, or the node's value) and which
master field, under which collation, explains every other subscript. The collations are the ones a map expression
can name: plain, `$$SQLUPPER`, `$$SQLSTRING`, `$$STRING`, `$$ALPHAUP`, `$$UPPER`, `$$MVR`, `$$TRUNCATE`, the length
variants and the two name pieces. IRIS runs every one of them. iad never re-implements a collation, and no sampled
value leaves IRIS, so a call needs no PHI acknowledgement.

Each link carries a verdict instead of a guess:

- `inferred`: at least 20 usable nodes and 0.95 agreement.
- `ambiguous`: two answers fit within 0.01, and both are listed. A column of digits is ambiguous, because
  EXACT, ALPHAUP and UPPER file it the same way.
- `too_few_nodes`, `not_linked`, `not_explained` and `constant` say why there is no map. A subscript nothing
  explains is named in `not_inferred`, and no code is drafted for it.

A draft counts as verified only when a scratch copy of the class compiles it, finds a row you can check, and the SQL
planner's own `EXPLAIN` says it would use the map. `check` shows the same line for any index map: "used", or the
planner's reason, such as "there is no index subscript collation".

It also covers what real index globals hold:

- NULL and empty-string markers. A map-compiled index files an empty string as `$c(0)` and an empty number as
  -1E14; a class-compiled one files `" "` followed by `$c(0)` and a lone `" "`. So the condition for a sparse index
  that skips empty strings is `{F}'=$c(0)`, and `{F}'=""` skips NULL only.
- Covering data held in the index node's value.
- Bitmap indexes: the chunk is the last subscript and the row ID is (chunk - 1) \* 64000 + position - 1. `propose`
  drafts the index map, and the data map for a global whose nodes are `^G(const, chunk)` bit strings.
- Sparse and conditional indexes: it reports the share of master rows the index lacks and drafts a `Condition` when
  one rule explains every missing row and no present one.
- A draft class with typed properties (`%Integer`, `%Numeric`, `%Date`, `%String` with a `MAXLEN`) and the
  collation the linking showed, compiled in a scratch copy and returned, never written.

The answer only adds keys, so `MODEL_VERSION` stays 1. A call over two candidates makes at most 11 IRIS calls and
took 0.76 s on the test container.

While testing this I found that `propose`'s `shape.sub_nodes` printed the data names of an index that sits inside
the master (39 of them for one fixture). It now withholds a depth that holds data names and reports how many
(`withheld_sub_nodes`). `include_values` under the data policy still shows them.

Limits: the 5 s cap applies to each candidate's run, not to the whole call, and a one- or two-key depth in
`shape.sub_nodes` cannot be told from structure and is not withheld.

## Changed

- An `iris_doc` put of a class with an SQL storage map gets the same checks, and its answer says to
  use `iris_storage_map`. Default storage is unchanged.
- Refusals from `iris_doc` now carry the `refusal` object, like the other tools.
