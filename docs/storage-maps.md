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
| `propose` | Samples a global with no map and drafts one, listing every element it had to guess         | no                |
| `edit`    | Path operations (`set`, `add`, `remove`, `move`) on the map, then the write path below     | yes               |
| `replace` | A whole map from `model`; asks before removing anything (`confirm_removal`)                | yes               |

`edit` and `replace` carry the `version` that `read` returned for the map, or for the storage when
the change is outside one map. A class that changed since is refused with the current map.

A path names one element by the model's keys: `/maps/Master/data/City/piece`,
`/maps/Master/subscripts/2/next_code`, `/keywords/SqlIdExpression`. An element iad does not model can
only be removed: `/maps/Master/extra/<Tag>`.

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
there ID reuse and an unbuilt index are warnings, not refusals.

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

| What IRIS does                                                                       | Why it matters                                      | Live test                                                            |
| ------------------------------------------------------------------------------------ | --------------------------------------------------- | -------------------------------------------------------------------- |
| With rows 1 to 3 and no counter node, the first INSERT through a `$i` map gets ID 1  | it overwrites row 1, with no error                  | `test_storage_map_data_172_counterless_insert_overwrites_row_1`      |
| A new index map over existing data answers nothing for a value that is there         | queries return no rows until the index is built     | `test_storage_map_data_172_unbuilt_index_finds_nothing`              |
| A new extent-bitmap map over existing rows makes `COUNT(*)` answer 0                 | the table looks empty until the bitmap is built     | `test_storage_map_paths_172_a_new_extent_bitmap_must_be_built`       |
| A map that walks level 2 of a global returns a parent's named node (`"EV"`) as a row | the table shows rows that are not rows              | `d11_a_level2_walk_returns_the_literal_node_as_a_row`                |
| A SQL DELETE of a row deletes the rows another map stores under it                   | a delete reaches a second table                     | `d11_delete_of_a_parent_row_kills_its_child_rows`                    |
| An UPDATE through a map keeps the fields another node of the row holds               | partial maps are safe to update through             | `d11_update_preserves_other_fields`                                  |
| A map type is case-sensitive: `DATA` fails to compile (#5511)                        | the edit looks right and does not compile           | `test_storage_map_scratch_172_compile_scratch_class`                 |
| Compiling and deleting a class over the real globals writes nothing to them          | `check` and `preview` can use a scratch copy safely | `test_storage_map_scratch_172_copy_over_real_globals_writes_nothing` |
| A PUT without `ignoreConflict` that carries a stale timestamp is refused (409)       | iad writes only the version it read                 | `test_storage_map_edit_172_guarded_put_refuses_a_stale_timestamp`    |
| A class whose real compile fails can be put back and compiled                        | a failed edit does not leave the table out of SQL   | `test_storage_map_edit_172_failed_real_compile_restores`             |

## Map code is code

Several map elements hold ObjectScript that IRIS runs when the table is queried or written: subscript
`Expression`, `NextCode`, `DataAccess`, `RetrievalCode`, `Condition`, `RowReference` and the access
variables. iad runs the same code-edit check on them as on `iris_execute` code, and a change to them
needs the execute category as well as compile. That check reads text, so code that builds a name at
run time gets past it. A map, once saved, runs as whoever queries the table, not as the agent that
wrote it. Review map code the way you review class code.
