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

| Field                   | Type    | Notes                                                                                                                 |
| ----------------------- | ------- | --------------------------------------------------------------------------------------------------------------------- |
| `global`                | string  |                                                                                                                       |
| `found_by`              | enum    | as Candidate                                                                                                          |
| `nodes_sampled`         | integer | nodes read                                                                                                            |
| `distinct_first_values` | integer | FR-004a                                                                                                               |
| `usable_nodes`          | integer | nodes with a master row                                                                                               |
| `id`                    | object  | `{form: "subscript"\|"value", positions: [int], agreement: number}`; `positions` empty for `value`                    |
| `subscripts`            | array   | one per non-ID position: `{position, verdict, field?, transform?, length?, agreement?, null_marker?, alternatives[]}` |
| `data_in_value`         | array   | `{field, element}` for covering `$LIST` data                                                                          |
| `missing_rows`          | object? | `{share, rule: {kind, field, empty_kind?}?}`; present when the index lacks master rows (FR-019)                       |
| `verdict`               | enum    | the link's overall verdict (below)                                                                                    |
| `map`                   | object? | the drafted index map in the model's JSON form; present when the verdict is `inferred` or `ambiguous`                 |
| `verified`              | object? | `{compiled: bool, row_found: bool, detail?}` from the scratch verification of FR-014/SC-002                           |
| `guesses`               | array   | element paths the draft guessed                                                                                       |
| `not_explained`         | array   | `{position, best_agreement}`                                                                                          |

`subscripts[].verdict` and `verdict` share one vocabulary, decided by `link::verdict`:

| Verdict         | Rule                                                                                       |
| --------------- | ------------------------------------------------------------------------------------------ |
| `inferred`      | ≥ 20 usable nodes and agreement ≥ 0.95, one best pair                                      |
| `ambiguous`     | two or more (field, transform) pairs within 0.01 of the best; all listed in `alternatives` |
| `too_few_nodes` | fewer than 20 usable nodes                                                                 |
| `not_linked`    | no ID position reaches 0.9; `agreement` is the best seen                                   |
| `not_explained` | ID found, no transform of the closed set reaches 0.95 at this position                     |
| `constant`      | the position holds one value across the sample (subscripts only)                           |

`null_marker`: `{kind: "c0"\|"minus1e14"\|"space"\|"space_c0"\|"discovered", length?, literal?, expression?}`. `literal`
and the drafted `NullMarker` expression appear only for `discovered` markers under `include_values` and the data
policy; otherwise they are absent and the draft's `guesses` and `not_inferred` say the marker is missing.

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

`{global, chunk_position, ids_checked, ids_found_in_master, verdict, map?, verified?}`. The
drafted map is `Type bitmap`, the one subscript `{Field}` through the Link's transform (research
R1). A decoded ID set not found in the master is `not_linked`. A chunk size or offset the data does not confirm is
`ambiguous` and `assumed` names it.

### Chunk data-map draft (`chunk_map_draft`)

For a master of shape `^G(const, chunk) = $BIT`: a data map in the model's JSON form with the computed third level
(`AccessType Other`, one access variable, `NextCode`, one `Invalidcondition`) of `IadSm172.Bits`. Present only when
the scratch verification returned the IDs the bits name (`verified.row_found`).

### Class draft (`class_draft`)

`{source: string, compiled: bool, properties: [{name, type, parameters, evidence}], guesses: [path…]}`. `evidence` is
counts only (`{"integer": 200, "of": 200}`, `{"date_fit": 198, "of": 200}`, `{"max_length": 31}`). `source` holds no
sampled value. Every property path is in `guesses`.

### Candidate listing (`index_candidates`)

`{listing: "ok"\|"unavailable", listing_reason?, candidates: [Candidate], not_examined: [string]}`.

### `limits`

`{node_cap, time_cap_seconds, stopped: [{candidate, cap: "nodes"|"time"}]}`.

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
