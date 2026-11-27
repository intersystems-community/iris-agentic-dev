# Data model: `propose` and `check` additions (173)

All keys are additive. `MODEL_VERSION` stays 1. Existing `propose` keys (`success`, `model_version`, `action`,
`namespace`, `global`, `capped`, `shape`, `draft`, `guesses`, `draft_note`, `not_inferred`) keep their meaning; the
172 golden files change only where a key below is added. Nothing here carries a sampled value, except a discovered
null-marker literal under FR-009's rule.

## New parameters (`IrisStorageMapParams`)

| Param           | Type            | Applies to              | Rule                                                                                                                 |
| --------------- | --------------- | ----------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `index_globals` | array of string | `propose`               | At most 8; each a plain global name (`validate_global_name`); anything else `INVALID_PARAMS`                         |
| `class`         | string          | `propose`               | Class whose master map is the master. With `global`, or neither: `INVALID_PARAMS`                                    |
| `field_names`   | array of string | `propose` (global mode) | By field position; shorter than the sampled field count, an invalid property name, or with `class`: `INVALID_PARAMS` |

## Entities

### Candidate

| Field        | Type   | Notes                                                                                                                              |
| ------------ | ------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| `global`     | string | `^name`                                                                                                                            |
| `found_by`   | enum   | `parameter`, `name_prefix`, `sub_node`                                                                                             |
| `sub_prefix` | array  | for `sub_node`: the literal first subscripts cut off, as JSON strings                                                              |
| `status`     | enum   | `examined` (an `indexes` item exists, whatever its verdict), `already_mapped` (class mode), `refused` (gate), `not_examined` (cap) |
| `reason`     | string | for `refused`, `not_examined`, `already_mapped` (the map's name)                                                                   |

### Link (an item of `indexes`)

| Field                   | Type    | Notes                                                                                                                                                                                                                                                                       |
| ----------------------- | ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `global`                | string  |                                                                                                                                                                                                                                                                             |
| `found_by`              | enum    | as Candidate                                                                                                                                                                                                                                                                |
| `nodes_sampled`         | integer | nodes read                                                                                                                                                                                                                                                                  |
| `distinct_first_values` | integer | FR-004a                                                                                                                                                                                                                                                                     |
| `usable_nodes`          | integer | nodes with a master row                                                                                                                                                                                                                                                     |
| `id`                    | object  | `{form: "subscript"\|"value", positions: [int], agreement: number}`; `positions` empty for `value`                                                                                                                                                                          |
| `subscripts`            | array   | one per non-ID position: `{position, verdict, field?, transform?, length?, agreement?, null_marker?, alternatives[]}`                                                                                                                                                       |
| `data_in_value`         | array   | `{field, element}` for covering `$LIST` data                                                                                                                                                                                                                                |
| `missing_rows`          | object? | `{share, rows, missing, rule: {kind, field, literal?}?}`; `kind` is `null`, `empty_string` or `fixed`; present when the index lacks master rows (FR-019). `literal` only for `fixed`, under `include_values` and the data policy                                            |
| `orphan_nodes`          | object? | `{count, share}`: index nodes that found no master row; present only for a linked index and above 5% (FR-019a)                                                                                                                                                              |
| `unexamined_positions`  | array?  | subscript positions past the first six non-ID positions, which are not scored; present when there are any (FR-005)                                                                                                                                                          |
| `bitmap`                | object? | `{chunk_position, ids_checked, ids_found_in_master}`; present for a candidate that holds bit strings (`id.form` is `bitmap`)                                                                                                                                                |
| `verdict`               | enum    | the link's overall verdict (below)                                                                                                                                                                                                                                          |
| `map`                   | object? | the drafted index map in the model's JSON form; present only when the verdict is `inferred` or `ambiguous` **and** the draft verified (below)                                                                                                                               |
| `verified`              | object? | `{compiled: bool, row_found: bool, detail?, usability?, rows_kept?}` from the scratch verification of FR-014/SC-002; `usability` is the planner's line for the field the draft links (the `check` line shape), `rows_kept` counts the master rows a drafted condition keeps |
| `guesses`               | array   | element paths the draft guessed                                                                                                                                                                                                                                             |
| `not_explained`         | array   | `{position, best_agreement}`                                                                                                                                                                                                                                                |

`subscripts[].verdict` and `verdict` share one vocabulary, decided by `link::verdict`:

| Verdict         | Rule                                                                                       |
| --------------- | ------------------------------------------------------------------------------------------ |
| `inferred`      | ≥ 20 usable nodes and agreement ≥ 0.95, one best pair                                      |
| `ambiguous`     | two or more (field, transform) pairs within 0.01 of the best; all listed in `alternatives` |
| `too_few_nodes` | fewer than 20 usable nodes                                                                 |
| `not_linked`    | no ID position reaches 0.9; `agreement` is the best seen                                   |
| `not_explained` | ID found, no transform of the closed set reaches 0.95 at this position                     |
| `constant`      | the position holds one value across the sample (subscripts only)                           |

A draft is kept in `map` only when it verified: it compiled in a scratch copy, a query by the indexed field found the
row, and, where the planner can be asked, its usability line for that field is `used` (SC-006). A piece form
(`$p($$SQLUPPER({F}),",",1)`) has no property collation to ask with, so it carries no `usability` line and verifies on
the first two alone. A draft that does not verify leaves `verified` in place and no `map`; `not_inferred_detail` says why.

`null_marker`: `{kind: "c0"\|"minus1e14"\|"space"\|"space_c0"\|"discovered", empty: "empty_string"\|"null", count, length?, literal?}`.
`literal` and the drafted `NullMarker` expression (in `map`) appear only for `discovered` markers under
`include_values` and the data policy; otherwise they are absent and the draft's `guesses` and `not_inferred` say the
marker is missing.

A digit-only column cannot be told apart by its stored form: `exact`, `alphaup` and `upper` file the same string, so
its position is `ambiguous` with the other two in `alternatives` (F11 and F16: Age).

### Transform (closed set; spelling in the draft)

| `transform`                                                                                    | Draft expression                       | Server function (`%SYSTEM.SQL.Functions`) |
| ---------------------------------------------------------------------------------------------- | -------------------------------------- | ----------------------------------------- |
| `exact`                                                                                        | `{F}`                                  | none                                      |
| `sqlupper`                                                                                     | `$$SQLUPPER({F})`, `$$SQLUPPER({F},n)` | `SQLUPPER(v[,n])`                         |
| `sqlstring`                                                                                    | `$$SQLSTRING({F})`, `…,n)`             | `SQLSTRING(v[,n])`                        |
| `string`                                                                                       | `$$STRING({F})`, `…,n)`                | `STRING(v[,n])`                           |
| `alphaup`                                                                                      | `$$ALPHAUP({F})`                       | `ALPHAUP(v)`                              |
| `upper`                                                                                        | `$$UPPER({F})`                         | `UPPER(v)`                                |
| `mvr`                                                                                          | `$$MVR({F})`                           | `MVR(v)`                                  |
| `truncate`                                                                                     | `$$TRUNCATE({F},n)`                    | `TRUNCATE(v,n)`                           |
| `piece1`, `piece2` of `sqlupper`, `sqlstring`, `string`, `alphaup`, `upper`, `mvr`, `truncate` | `$p($$SQLUPPER({F}),",",1)` and `,2`   | the base function, then `$Piece`          |

`COLLATE` is recognised and never drafted; `EXACT`, `PLUS`, `MINUS`, `SPACE` are not map-expression functions
(research R4, spec fact 1).

### Bitmap item (an item of `bitmaps`)

`{global, chunk_position, index_name?, ids_checked, ids_found_in_master, verdict, assumed: [string], map?, verified?}`.
`index_name` is the constant first subscript of a class-compiled bitmap. The drafted map is `Type bitmap`, the one
subscript `{Field}` through the Link's transform (research R1). A decoded ID set not found in the master is
`not_linked`. The chunk size of 64000 is confirmed only when IDs from a later chunk than the first were sampled and
found their rows; with only chunk 1 sampled it is `assumed`, and with later IDs that find nothing the verdict is
`ambiguous`. `assumed` names what the decode assumed.

### Chunk data-map draft (`chunk_map_draft`)

For a master of shape `^G(const, chunk) = $BIT`: a data map in the model's JSON form with the computed third level
(`AccessType Other`, one access variable, `NextCode`, one `Invalidcondition`) of `IadSm172.Bits`. Present only when
the scratch verification returned the IDs the bits name (`verified.row_found`).

### Class draft (`class_draft`)

`{source: string, compiled: bool, properties: [{name, type, parameters, evidence}], guesses: [path…]}`. `evidence` is
counts only (`{"integer": 200, "of": 200}`, `{"date_fit": 198, "of": 200}`, `{"max_length": 31}`). `source` holds no
sampled value. Every property path is in `guesses`. It is returned only when `field_names` or `index_globals` is
given (global mode), and its properties are typed from counts: `%Date`, `%Integer`, `%Numeric` with `SCALE`, else
`%String` with `MAXLEN`. A `COLLATION` parameter goes only on a `%String` property (IRIS refuses it on the others), and
only for a field whose link is `inferred` and whole (not a piece, not a tie).

### Candidate listing (`index_candidates`)

`{listing: "ok"\|"unavailable", listing_reason?, sub_trees_not_looked_for?, candidates: [Candidate], not_examined: [string]}`.
`sub_trees_not_looked_for` says why the master's own sub-trees were not scanned (a master whose row ID is not its first
subscript has no literal first subscripts to read).

### `limits`

`{node_cap, time_cap_seconds, stopped: [{candidate, cap: "nodes"|"time"|"count_cap"}]}`; `count_cap` is the counting
pass of the spread sample reaching ten times the node cap.

### `usability` (an addition to `check`)

`[{map, subscript_level, field, status: "used"|"unused"|"unknown", text}]`. `text` is the planner's own wording for
`unused` (the warning line) and the line that names the map for `used`; `unknown` says why the planner could not be
asked. Also copied as strings into `warnings`; `check`'s pass or fail is unchanged.

## `not_inferred`

Always present, and still an array of strings (the 172 shape and golden keep it). 173 adds `not_inferred_detail`, an
array of `{what, why}` objects carrying the same items with the reason apart. Between them they name at least: what was
not examined (cap, refusal, listing unavailable), what was not explained (`not_explained` positions, `COLLATE`, numeric
collations, computed subscripts), datatypes beyond the four verdicts, index conditions no rule explained, and the ID
counter.

## Errors

No new error code. `INVALID_PARAMS` (bad `index_globals`, `class`/`global` conflict, `field_names` length or name),
`STORAGE_MAP_TIME_CAP` (reused for a candidate stopped by the 5 s cap), `IRIS_EXECUTE_ERROR` (generator failure),
`DATA_POLICY_BLOCKED` and `PHI_ACK_REQUIRED` (marker literal), and the existing global-gate refusals.

`shape.withheld_sub_nodes` (global mode): `{depth: count}` for each depth of `shape.sub_nodes` below the first that holds three
or more distinct literal names with none repeated, which the answer omits unless `include_values` is on under the data
policy (an index inside the master files its keys there, so they are data).
