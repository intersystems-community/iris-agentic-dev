# SQL storage maps

`iris_storage_map` reads, draws, checks and edits the SQL storage maps of a class: the hand-written
`%Storage.SQL` maps that lay a table over existing globals. It is the tool for classes that Studio's
map editor maintained. Default storage (`%Storage.Persistent`) is read-only here; `iris_doc` still
writes it as it always has.

Every action works on a model of the Storage block, never on its text. iad parses the block into a
tree, edits the tree, and writes it back in the exact form IRIS stores, so an element iad does not
model is carried through untouched and a one-field edit changes one line.

## Actions

| Action    | What it does                                                                               | Writes            |
| --------- | ------------------------------------------------------------------------------------------ | ----------------- |
| `read`    | The class's storage as JSON: maps, subscripts, fields, row ID, and the facts IRIS applies  | no                |
| `diagram` | A Mermaid flowchart, a JSON graph, and a Markdown document of each map                     | no                |
| `uses`    | Every class whose maps use a global, in one or more namespaces                             | no                |
| `check`   | Compiles the class's map, or a proposed `model`, in a scratch copy that is deleted after   | no (scratch only) |
| `compare` | Samples the map's global and lists where the map and the data disagree                     | no                |
| `preview` | The rows a map would return, read through a scratch copy; masked by default; 10 s time cap | no (scratch only) |
| `propose` | Samples a global with no map and drafts one; links index globals and drafts their maps     | no (scratch only) |
| `edit`    | Path operations (`set`, `add`, `remove`, `move`) on the map, then the write path below     | yes               |
| `replace` | A whole map from `model`; asks before removing anything (`confirm_removal`)                | yes               |

`edit` and `replace` carry the `version` that `read` returned for the map, or for the storage when
the change is outside one map. A class that changed since is refused with the current map.

A path names one element by the model's keys: `/maps/Master/data/City/piece`,
`/maps/Master/subscripts/2/next_code`, `/keywords/SqlIdExpression`. An element iad does not model can
only be removed: `/maps/Master/extra/<Tag>`.

## Reading a map that computes a level

A subscript level whose keys come from code (`AccessType Other`, a `NextCode`) is not necessarily a
subscript of the global: a `$BIT` or `$LIST` held in a node's value is a common reason. `read` and
`diagram` mark such a level in `derived` (`level 3 is computed by code (...)`), and `edit` and
`replace` repeat it in their `warnings`. iad does not run the code. A level that only has an
`Invalidcondition` is called a filter, and an access variable that the map sets by code gets its own
line. Check a flagged map against real data with `compare`.

`compare` answers `verified: false` and says `no data found at <global>, the map was not checked` when
the global holds nothing, so an empty answer is never read as a match. When the map has a computed
level and the data sits at another depth, `notes` names both depths. `propose` returns `draft_note`
and `not_inferred`: the list of what the draft leaves to you (datatypes, collations, index maps,
conditions, computed subscripts, the ID counter).

`read` returns a `compiler_added` list when the compiler wrote a keyword the class does not use
(today `StreamLocation`, on a class whose own text declares no stream property and extends only plain
persistent classes). The element stays in `storage` so edits write it back; `diagram` leaves it out.

## `propose`: a global, its index globals, and a class

`propose` samples a global that has no map and drafts one. Since spec 173 it also finds the index
globals that belong to it, says which master field and which collation each index subscript holds,
and drafts the index map. Every draft is a guess that `check` and `compare` test against the data. The
call reads through the execute path as the service account and writes nothing: its scratch copies are
deleted, and the answer holds counts, positions, names and verdicts, never a sampled value (the
exceptions are under "What a sampled value can reach").
Every new key is additive: `model_version` stays 1 and the keys `propose` had keep their meaning.

### Parameters

| Param            | Type            | Rule                                                                                                                                                                                                                    |
| ---------------- | --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `global`         | string          | The master: a plain global name. Not with `class`                                                                                                                                                                       |
| `class`          | string          | Class mode: the master is the class's own master map, so nothing is sampled for it. Not with `global`. A class with default storage, a columnar map or no master map is refused with the reason `read` gives            |
| `index_globals`  | array of string | At most 8 plain global names, checked before any IRIS call and put through the same gates as the master. `propose` also finds candidates itself (below)                                                                 |
| `field_names`    | array of string | Global mode only. The master's property names by field position, at least as many as the sampled field count. With them `propose` names the master draft's fields and the index drafts' expressions, and drafts a class |
| `max_nodes`      | integer         | The node cap, applied to each candidate. A 5 second cap covers the whole call                                                                                                                                           |
| `include_values` | boolean         | Releases the two literals described under "What a sampled value can reach", under the data policy and `acknowledgePhi`                                                                                                  |

`propose` with neither `global` nor `class`, or with both, is `INVALID_PARAMS`. So is `index_globals` or
`field_names` on another action, more than 8 index globals, a name that is not a plain global name, and
`field_names` with `class`.

### Where the candidates come from

A candidate is a global that may be an index over the master: a candidate index global. There are three
sources, and the answer says which found each one (`found_by`):

- `parameter`: a name in `index_globals`.
- `name_prefix`: a global in the namespace whose name begins with the master's (`^A.x` for `^A`), from
  the namespace's own global listing. When the service account cannot list globals, `index_candidates`
  says `listing: "unavailable"` with `listing_reason`, and the other two sources still run.
- `sub_node`: a literal first subscript inside the master (`^M("idx", ...)`), looked for when the
  master's row ID is its first subscript. The candidate's `sub_prefix` holds the literal subscripts cut
  off, and its map name comes from the literal. When the master's row ID is not its first subscript,
  `sub_trees_not_looked_for` says so.

At most 8 candidates are examined. The rest are named in `index_candidates.not_examined` and in
`not_inferred_detail`, so a cut-off is never silent. A candidate has a `status`: `examined` (it has an
`indexes` item, whatever its verdict), `already_mapped` (class mode: one of the class's index maps
already uses it, and `reason` names the map), `refused` (a global gate said no, and `reason` says why) or
`not_examined`. A global that shares the master's name prefix and links to nothing is listed with its
best agreement, never dropped.

### What a call returns

| Key                   | When                                                 | Content                                                                                                                                                     |
| --------------------- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `index_candidates`    | always                                               | `listing`, `listing_reason`, `sub_trees_not_looked_for`, `candidates` (each with `global`, `found_by`, `sub_prefix`, `status`, `reason`) and `not_examined` |
| `indexes`             | at least one candidate was examined                  | one link per candidate, below                                                                                                                               |
| `bitmaps`             | global mode, a candidate holds bit strings           | `global`, `chunk_position`, `index_name`, `ids_checked`, `ids_found_in_master`, `verdict`, `assumed`, and `map` and `verified` when drafted                 |
| `chunk_map_draft`     | the master is `^G(const, chunk) = $BIT`              | a data map with the computed third level, returned only when its scratch copy returned the IDs the bits name                                                |
| `class_draft`         | `field_names` or `index_globals` given (global mode) | `source`, `compiled`, `properties` (each with `name`, `type`, `parameters`, `evidence`) and `guesses`                                                       |
| `limits`              | always                                               | `node_cap`, `time_cap_seconds` and `stopped`: each `candidate` a cap stopped, with `cap` as `nodes`, `time` or `count_cap`                                  |
| `not_inferred`        | always                                               | strings: what the draft leaves to you                                                                                                                       |
| `not_inferred_detail` | always                                               | the same items as `what` and `why` pairs: not examined, not explained, no drafted map, held-back values                                                     |

A link (an item of `indexes`) holds:

| Key                     | Meaning                                                                                                                                       |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `global`, `found_by`    | the candidate and how it was found                                                                                                            |
| `nodes_sampled`         | nodes read, in a sample that spreads over the whole key range and not only the first nodes in order                                           |
| `distinct_first_values` | how many different first subscripts the sample saw                                                                                            |
| `usable_nodes`          | nodes whose row ID found a master row                                                                                                         |
| `id`                    | where the row ID sits: `form` (`subscript`, `value` or `bitmap`), `positions` (more than one for a composite key) and `agreement`             |
| `subscripts`            | one item per other position: `position`, `verdict`, `field`, `transform`, `length`, `agreement`, `null_marker` and `alternatives`             |
| `data_in_value`         | master fields a covering `$LIST` node value holds, each with its `field` and `element`                                                        |
| `verdict`               | the link's verdict, from the table below                                                                                                      |
| `not_explained`         | each position no transform reaches, with its `best_agreement`                                                                                 |
| `unexamined_positions`  | positions past the sixth non-ID subscript, which are not scored; also named in `not_inferred_detail`                                          |
| `orphan_nodes`          | `count` and `share` of index nodes with no master row, as rows deleted from the master leave them; reported only above 5%, for a linked index |
| `missing_rows`          | the index lacks master rows: `share`, `rows`, `missing` and the `rule` that explains them (below)                                             |
| `verified`              | what the scratch copy showed: `compiled`, `row_found`, `detail`, `usability` and, for a condition, `rows_kept`                                |
| `map`, `guesses`        | the drafted index map and the paths of what it guessed; present only when the draft verified                                                  |
| `bitmap`                | for a candidate that holds bit strings: `chunk_position`, `ids_checked`, `ids_found_in_master`                                                |

A global-mode answer's `shape.sub_nodes` lists the literal subscripts the sample saw at each depth. Below the first subscript, a depth that holds three or more distinct names with no repeat is data, not structure (an index inside the master files its keys there), so the answer withholds it and says how many it held in `shape.withheld_sub_nodes` (`{"2": 39}`). `include_values` under the data policy shows them.

`null_marker` on a subscript says what the index files for an empty field: `kind` is `c0`, `minus1e14`,
`space`, `space_c0` or `discovered`, `empty` says whether it stands for an `empty_string` or a `null`,
and `count` is how many nodes hold it. A `discovered` marker also has its `length`.

### Verdicts

One vocabulary serves a link and each of its subscripts. The rule is in `link::verdict`.

| Verdict         | Meaning                                                                                                                    |
| --------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `inferred`      | at least 20 usable nodes and agreement of 0.95 or more, with one best (field, transform) pair                              |
| `ambiguous`     | two or more pairs within 0.01 of the best; each is listed in `alternatives`                                                |
| `too_few_nodes` | fewer than 20 usable nodes: the sample cannot decide                                                                       |
| `not_linked`    | no position reaches 0.9 as the row ID: the global is not an index of this master                                           |
| `not_explained` | the row ID is found and no transform of the closed set reaches 0.95 at this position (a computed subscript, a date format) |
| `constant`      | the position holds one value across the sample (a subscript only)                                                          |

Agreement is a share of sampled nodes, not a proof: `inferred` means no other pair in the set did as well
on the sample. No answer claims that an index is unique or gives a cardinality.

### The closed set of transforms

A subscript is explained by a master field and one of these, drafted with the spelling in the second
column. Anything else is `not_explained`. `COLLATE` is recognised and never drafted. `EXACT`, `PLUS`,
`MINUS` and `SPACE` are not map-expression functions.

| `transform`                                      | Draft expression                         |
| ------------------------------------------------ | ---------------------------------------- |
| `exact`                                          | `{F}`                                    |
| `sqlupper`                                       | `$$SQLUPPER({F})`, `$$SQLUPPER({F},n)`   |
| `sqlstring`                                      | `$$SQLSTRING({F})`, `$$SQLSTRING({F},n)` |
| `string`                                         | `$$STRING({F})`, `$$STRING({F},n)`       |
| `alphaup`                                        | `$$ALPHAUP({F})`                         |
| `upper`                                          | `$$UPPER({F})`                           |
| `mvr`                                            | `$$MVR({F})`                             |
| `truncate`                                       | `$$TRUNCATE({F},n)`                      |
| `piece1`, `piece2` of any of the seven functions | `$p($$SQLUPPER({F}),",",1)` and `,2`     |

Each function is the server's own, in `%SYSTEM.SQL.Functions`. The linker asks the server what each
would file for the sampled value and compares that with the subscript, so no collation is
reimplemented here.

### Rules that wrong guesses came from

- A column of digits only is `ambiguous`: `exact`, `alphaup` and `upper` file the same string for it, so
  no sample can tell them apart. The others are listed in `alternatives`.
- A row ID can sit in the node's value instead of a subscript (`id.form` is `value`), and a composite
  key fills more than one position. A draft for a composite key ends in the master draft's part names
  (`{Key1}`, `{ID}` in global mode, the master's own in class mode).
- An index that lacks some master rows is a conditional map. The empty string and NULL are different
  rules: SQL stores an empty string as `$c(0)`, so a condition `{F}'=""` skips NULL only, and the
  empty-string rule is `{F}'=$c(0)`. A rule that keeps the rows where the field equals one value is
  `fixed`, and its literal is a sampled value that appears only under `include_values`. When no single
  rule fits (every seventh row is missing, say), `rule` is null, no condition is drafted, and the answer
  says the map covers fewer rows than the master.
- A bitmap index files `(value, chunk)` with a `$BIT` string as the node value, the chunk last. Its draft
  is `Type bitmap` with the field as its only subscript, and the engine appends the chunk. A row ID is
  `(chunk - 1) * 64000 + position - 1`. The chunk size of 64000 is confirmed only when IDs from a later
  chunk than the first were sampled and found their rows. With only chunk 1 sampled, `assumed` says so;
  with later IDs that find no row, the verdict is `ambiguous`.
- A draft is `verified` only when its scratch copy compiled, a query by the indexed field found the row,
  and the planner's usability line for the field is `used`. A draft that does not verify has `verified`
  and no `map`, and `not_inferred_detail` says why. A piece form has no collation to ask the planner
  with, so it has no usability line and verifies on the first two alone.
- Orphans: a few index nodes with no master row are what deleted rows leave behind. Up to 5% go
  unmentioned; above that `orphan_nodes` says how many; past 10% the ID is `not_linked`.

### What a sampled value can reach

Nothing a sample read leaves IRIS except two literals: a `discovered` null marker's `literal` (and the
`NullMarker` expression in the drafted map) and a `fixed` rule's `literal` (and its condition). They
appear only with `include_values: true` under `dataPolicy = "allow"` and `acknowledgePhi`. Under `block`
and `redact` the call is refused. Without the flag the answer says that the marker or rule exists and
holds the value back. The `class_draft` source and its `evidence` hold counts only
(`{"integer": 200, "of": 200}`).

### Class mode

`propose` with `class` takes the master from the class's own master map and links the candidates to it.
An index global that one of the class's index maps uses is `already_mapped` and drafts nothing. Every
other draft is spelled in the class's property names and row-ID part names, under a map name the class
does not use. The answer has `class`, `global`, `index_candidates`, `indexes`, `limits`, `not_inferred`
and `not_inferred_detail`. It has no `draft`, `shape`, `chunk_map_draft` or `class_draft`.

### Property types in `class_draft`

Each property gets the type its sampled values support: `%Date` when at least 95% are whole numbers in
a plausible date range (`evidence` says how many), else `%Integer`, else `%Numeric` with the longest
fraction as its `SCALE`, else `%String` with a `MAXLEN` rounded up from the longest value. A
`COLLATION` parameter goes only on a `%String` property, for a field whose link is `inferred` and whole.
Every property is a guess and is listed in `guesses`. The draft is compiled in a scratch copy, and
`compiled` says whether it passed.

### `check` and the usability line

`check` adds `usability` to its answer and copies each line into `warnings`. A line has `map`,
`subscript_level`, `field`, `status` and `text`. `status` is `used`, `unused` or `unknown`. `text` is the
planner's own wording: for `unused`, its warning that the map cannot satisfy an equality on the field and
why (no index subscript collation, a different length). `unknown` says why the planner could not be
asked. `check`'s `success` and its refusals do not depend on the line, and a class with no index map has
none.

## The write path

A write goes through these steps in order, and stops at the first that fails:

1. The map is checked against the rules IRIS rejects or misreads: integer subscript names, the map
   type in lower case, a known access type, no element iad does not know, no extended global
   reference, no empty element.
2. The global and map-code gates: no code-storage global, nothing on the server's blocklist, the
   code-edit check on every element that holds ObjectScript, no global known only at run time unless
   the operator allows it.
3. The data checks, on the real globals (see below).
4. The class is compiled in a scratch copy, which carries every property of the class, so a map the
   real compile would refuse fails here first.
5. The class is written only if IRIS still holds the version iad read.
6. iad reads the class back, checks that IRIS kept what was sent, and runs the gates again on it.
7. The real compile. If it fails, iad puts the previous text back and compiles it.
8. The opt-ins, if asked: `seed_counter` sets the ID counter, `build_indexes` builds a new index.

The data checks and the opt-ins run as the service account when one is set, as `preview`,
`compare` and `propose` do, and the answer carries the `identity` block; the PUT and the compiles
run as the connection's user. A server whose service account cannot be used refuses the edit
before anything is read.

The data checks refuse three kinds of damage:

- A map that walks a level as row IDs while that level holds named nodes that are not IDs: a
  parent's `"EV"` node under rows that hold values, or `"reserved"` among numeric IDs. A `NextCode`,
  `StartValue` or `Invalidcondition` that skips them lets the write through.
- An ID counter behind the highest ID already stored, when the write sets the ID expression or the
  master map (`STORAGE_MAP_ID_REUSE`). `seed_counter: true` sets the counter and reports the value it
  replaced.
- A new index, bitmap or extent-bitmap map over a table with rows while it is empty
  (`STORAGE_MAP_INDEX_UNBUILT`). An unbuilt extent bitmap makes `COUNT(*)` answer 0.
  `build_indexes: true` builds it; it is a destructive write, refused when the index range already
  holds nodes or another map uses it.

An `iris_doc` put of a class with an SQL map goes through steps 1 to 4 (step 4 only when the put
compiles), 6 and 7, and answers with a warning that names `iris_storage_map`. It has no opt-ins, so
there ID reuse and an unbuilt index are warnings, not refusals. Its data checks run as the
connection's user, the account its write runs as.

## Error codes

| Code                           | Meaning                                                                                                                            |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| `STORAGE_MAP_NOT_FOUND`        | no such class, map or Storage block                                                                                                |
| `STORAGE_MAP_PARSE_FAILED`     | the Storage block cannot be parsed; the answer quotes where                                                                        |
| `STORAGE_MAP_INVALID`          | a rule in step 1 or a data check failed; the answer names the rule and path                                                        |
| `STORAGE_MAP_READ_ONLY`        | default storage, a storage type other than `%Storage.SQL`, or a columnar map                                                       |
| `STORAGE_MAP_VERSION_CONFLICT` | the version given is not the stored one, or the class changed during the write                                                     |
| `STORAGE_MAP_CONFIRM_REMOVAL`  | `replace` would remove the elements listed; repeat with `confirm_removal: true`                                                    |
| `STORAGE_MAP_COMPILE_FAILED`   | the scratch copy did not compile; IRIS's errors with a plain explanation                                                           |
| `STORAGE_MAP_RESTORED`         | the real compile failed and the previous version was put back                                                                      |
| `STORAGE_MAP_RESTORE_FAILED`   | the real compile failed and putting the previous version back failed too                                                           |
| `STORAGE_MAP_DROPPED`          | IRIS did not keep an element iad sent; the previous version was put back                                                           |
| `STORAGE_MAP_WRITER_MISMATCH`  | IRIS stored something iad did not send (an iad bug); put back                                                                      |
| `STORAGE_MAP_ID_REUSE`         | the ID counter is behind the highest stored ID                                                                                     |
| `STORAGE_MAP_INDEX_UNBUILT`    | a new index map over rows with an empty index                                                                                      |
| `STORAGE_MAP_OPT_IN_FAILED`    | the map was written and compiled, but an opt-in (`seed_counter`, `build_indexes`) failed; run it again before the table takes rows |
| `STORAGE_MAP_TIME_CAP`         | `preview` stopped at its time cap (10 s); lower `max_rows` or narrow the map                                                       |

`STORAGE_MAP_DYNAMIC_GLOBAL_BLOCKED` is a refusal: a map names a global known only at run time, and
the operator has not set `allowDynamicMapGlobals`. Values in `compare`, `preview` and `propose`
answers (`include_values: true`) need `acknowledgePhi` under `dataPolicy = "allow"` and are refused
under `block` and `redact`.

## Measured behaviour

Each line is something IRIS does that a map edit can get wrong, measured on IRIS 2026.2, with the
live test that shows it.

| What IRIS does                                                                                                      | Why it matters                                          | Live test                                                            |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- | -------------------------------------------------------------------- |
| With rows 1 to 3 and no counter node, the first INSERT through a `$i` map gets ID 1                                 | it overwrites row 1, with no error                      | `test_storage_map_data_172_counterless_insert_overwrites_row_1`      |
| A new index map over existing data answers nothing for a value that is there                                        | queries return no rows until the index is built         | `test_storage_map_data_172_unbuilt_index_finds_nothing`              |
| A new extent-bitmap map over existing rows makes `COUNT(*)` answer 0                                                | the table looks empty until the bitmap is built         | `test_storage_map_paths_172_a_new_extent_bitmap_must_be_built`       |
| A map that walks level 2 of a global returns a parent's named node (`"EV"`) as a row                                | the table shows rows that are not rows                  | `d11_a_level2_walk_returns_the_literal_node_as_a_row`                |
| A SQL DELETE of a row deletes the rows another map stores under it                                                  | a delete reaches a second table                         | `d11_delete_of_a_parent_row_kills_its_child_rows`                    |
| An UPDATE through a map keeps the fields another node of the row holds                                              | partial maps are safe to update through                 | `d11_update_preserves_other_fields`                                  |
| A map type is case-sensitive: `DATA` fails to compile (#5511)                                                       | the edit looks right and does not compile               | `test_storage_map_scratch_172_compile_scratch_class`                 |
| Compiling and deleting a class over the real globals writes nothing to them                                         | `check` and `preview` can use a scratch copy safely     | `test_storage_map_scratch_172_copy_over_real_globals_writes_nothing` |
| A PUT without `ignoreConflict` that carries a stale timestamp is refused (409)                                      | iad writes only the version it read                     | `test_storage_map_edit_172_guarded_put_refuses_a_stale_timestamp`    |
| A class whose real compile fails can be put back and compiled                                                       | a failed edit does not leave the table out of SQL       | `test_storage_map_edit_172_failed_real_compile_restores`             |
| A map with `AccessType Other` and a `NextCode` over a global with a `$BIT` at depth 2 returns the IDs its bits name | a third level that no node holds is still a real column | `computed_172_the_look_alike_really_returns_the_ids_its_bits_name`   |
| `compare` over a map whose global is empty                                                                          | "no mismatches" would mean nothing was checked          | `computed_172_compare_over_an_empty_global_says_it_checked_nothing`  |

## What `propose` measured

Each line is a fact the index-linking design rests on, measured on IRIS 2026.2, with the live test that
holds it. The tests are in `test_storage_map_measured_173_live.rs`.

| What IRIS does                                                                                                                                                                                                         | What `propose` does with it                                        | Live test                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | -------------------------------------------------- |
| A map expression can name `$$SQLUPPER`, `$$SQLSTRING`, `$$STRING`, `$$ALPHAUP`, `$$UPPER`, `$$MVR` and `$$TRUNCATE`. `$$EXACT`, `$$PLUS`, `$$MINUS` and `$$SPACE` do not compile                                       | the closed set of transforms; a plain `{F}` is the exact collation | `fact_1_closed_set_of_collation_functions`         |
| Each function stores its own form: a space then the upper-cased value for `$$SQLUPPER`, the case kept for `$$SQLSTRING`, spaces removed for `$$STRING`, letters and digits only for `$$ALPHAUP`                        | the linker compares each form with the stored subscript            | `fact_2_stored_forms`                              |
| A length variant counts the leading space: `$$SQLUPPER({Name},4)` stores `" SMI"`                                                                                                                                      | `length` on the pair, drafted as `$$SQLUPPER({F},4)`               | `fact_3_length_variant`                            |
| `$p($$SQLUPPER({Name}),",",1)` compiles and stores `" SMITH"`                                                                                                                                                          | the `piece1` and `piece2` forms                                    | `fact_4_name_piece`                                |
| A map-compiled index stores `$c(0)` for an empty string, `-1E14` for an empty number and `" "_$c(0)` under `$$SQLUPPER`. A class-compiled one stores a space and `$c(0)` for an empty string and a lone space for NULL | the `null_marker` kinds                                            | `fact_5_null_markers`                              |
| The planner says why a map cannot serve an equality (no index subscript collation, "the index subscript collation is %SQLUPPER(4)") and says when it reads one ("using the given")                                     | the usability line: `used`, `unused` or `unknown`                  | `fact_6_planner_words`                             |
| A bitmap index global holds `(value, chunk)` with a `$BIT` string, and the ID is `(chunk - 1) * 64000 + position - 1`                                                                                                  | the bitmap decode                                                  | `fact_7_bitmap_decode`                             |
| A table over `^G(const, chunk) = $BIT` is a data map whose third level is computed, and it returns the IDs its bits name                                                                                               | `chunk_map_draft`                                                  | `fact_8_chunk_data_map`                            |
| Each of the eight `%SYSTEM.SQL.Functions` methods that matches a map function returns what a map-compiled index files, length variants and the empty string included                                                   | the server's own function does the comparing                       | `fact_9_function_class_equals_the_filed_subscript` |
| A `Type bitmap` map with one subscript files `(value, chunk)`, and a second subscript is appended before the chunk                                                                                                     | a bitmap draft carries the field alone                             | `fact_10_bitmap_map_layout`                        |
| A map can read the row ID from the node value (`<Data name="Id"/>` with a row-ID spec of `{Id}`), and the planner reads the index map                                                                                  | `id.form` is `value`                                               | `fact_11_id_in_the_node_value`                     |
| A conditional map skips rows at filing time, and `{City}'=""` skips NULL and keeps the empty string                                                                                                                    | the NULL and the empty-string rules                                | `fact_12_conditional_filing`                       |
| `$$COLLATE({F},"SQLUPPER")` and `$$COLLATE({F})` store the value unchanged, `$$COLLATE({F},7)` does not compile, and `" "_{F}` files what `$$SQLSTRING` does on clean values                                           | `COLLATE` is never drafted                                         | `r4_collate_forms`                                 |
| A composite row ID `(A,B)` works through a master whose row-ID spec is `{L1}` and `{L2}`                                                                                                                               | composite-key links and drafts                                     | `r6_multipart_key`                                 |
| The namespace's global listing runs as the connection's user                                                                                                                                                           | `name_prefix` candidates; a failure is said, not hidden            | `open_listing_as_the_connection_user`              |
| `{City}'=$c(0)` compiles in a conditional map and skips exactly the empty-string rows                                                                                                                                  | the empty-string condition                                         | `open_c0_condition`                                |
| A sample spread over the key range separates `$$UPPER` from `$$ALPHAUP`, where a walk of the first 200 nodes in order cannot                                                                                           | the spread sample                                                  | `open_spread_beats_prefix`                         |

## Map code is code

Several map elements hold ObjectScript that IRIS runs when the table is queried or written: subscript
`Expression`, `NextCode`, `DataAccess`, `RetrievalCode`, `Condition`, `RowReference` and the access
variables. iad runs the same code-edit check on them as on `iris_execute` code, and a change to them
needs the execute category as well as compile. That check reads text, so code that builds a name at
run time gets past it. A map, once saved, runs as whoever queries the table, not as the agent that
wrote it. Review map code the way you review class code.
