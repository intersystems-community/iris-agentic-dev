# Data model: SQL storage maps (172)

`model_version` starts at `1`. Any change below that renames, removes or retypes a field bumps it (FR-033).

## Two layers

1. **Element tree** (internal, lossless): `Node { tag, name: Option<String>, value: Option<String>, children:
Option<Vec<Node>> }`. A leaf has a value; a container has children; `<X/>` is a container with no children. The
   parser builds it from the Storage body and the writer emits it in canonical form (research R3). Every edit is
   applied to this tree, so an element the typed layer does not know is never lost.
2. **Storage model** (the JSON contract): a typed view built from the tree. Known elements become named fields;
   anything else lands in `extra`, keeping tag, name and value.

## Storage model (JSON)

```json
{
  "model_version": 1,
  "class": "IadSm172.Person",
  "storage": {
    "name": "SQLStorage",
    "type": "%Storage.SQL",
    "version": "3f9a1c2e",
    "editable": true,
    "not_editable_reason": null,
    "keywords": {
      "SqlIdExpression": "$i(^IadSm172Person)",
      "StreamLocation": "^IadSm172PersonS"
    },
    "description": null,
    "maps": ["…SqlMap…"],
    "default_storage": null,
    "extra": []
  },
  "derived": [
    {
      "path": "/maps/Master/data/City",
      "fact": "no delimiter: node read as $LIST"
    }
  ]
}
```

- `storage.type`: the `<Type>` value. `editable` is true only for `%Storage.SQL`; otherwise `not_editable_reason` is
  `"default storage"`, `"storage type <X> is read-only in 172"`.
- `keywords`: storage-level leaves other than `Type`, `Description` and `SQLMap` (for example `SqlIdExpression`,
  `SqlRowIdName`, `ExtentSize`, `StreamLocation`, `IdLocation`), tag → value.
- `default_storage`: for default storage only, `{ data_location, id_location, index_location, stream_location,
nodes: [ { node, value_name, slots: [ { slot, property } ] } ] }`, read-only.

### SqlMap

| Field                               | Element                           | Notes                                                 |
| ----------------------------------- | --------------------------------- | ----------------------------------------------------- |
| `name`                              | `SQLMap name=`                    | key in paths                                          |
| `version`                           | —                                 | digest of this map's canonical text (R10)             |
| `type`                              | `Type`                            | `data`, `index`, `bitmap`, `bitmapextent`, `columnar` |
| `master`                            | —                                 | true for the first `data` map                         |
| `used`                              | —                                 | false for a second or later `data` map                |
| `global`                            | `Global`                          | as written, e.g. `^IadSm172Person`                    |
| `subscripts`                        | `Subscript name=n`                | ordered by level                                      |
| `data`                              | `Data name=`                      | keyed by field name                                   |
| `row_id`                            | `RowIdSpec name=n`                | ordered by position                                   |
| `condition`                         | `Condition`                       | code                                                  |
| `condition_fields`                  | `ConditionFields`                 |                                                       |
| `conditional_with_host_vars`        | `ConditionalWithHostVars`         |                                                       |
| `population_type`, `population_pct` | `PopulationType`, `PopulationPct` |                                                       |
| `block_count`                       | `BlockCount`                      |                                                       |
| `structure`                         | `Structure`                       |                                                       |
| `row_reference`                     | `RowReference`                    | code                                                  |
| `deferred`                          | `Deferred`                        |                                                       |
| `extra`                             | anything else                     | `[ { tag, name, value } ]`, kept verbatim             |

### Subscript

`level` (integer, from `name=`), `expression`, `access_type` (`Sub` `Piece` `List` `Global` `Other`), `delimiter`,
`data_access`, `next_code`, `start_value`, `stop_value`, `stop_expression`, `loop_init_value`, `null_marker`,
`access_vars: [ { level, code, variable } ]`, `invalid_conditions: [ { level, expression } ]`, `extra`.

### DataField

`name`, `node`, `delimiter`, `piece`, `retrieval_code`, `extra`, and `derived: { format: "$LIST" | "delimited" |
"whole node", computed: bool, filed: bool }` (research R5).

### RowIdPart

`position` (integer, from `name=`), `field`, `expression`.

## Paths

A path names one element of the model, keyed by name where the element has one:

- `/keywords/SqlIdExpression`
- `/maps/Master/global`, `/maps/Master/type`
- `/maps/Master/subscripts/2/expression`, `/maps/Master/subscripts/2/access_vars/1/code`
- `/maps/Master/data/City/piece`
- `/maps/Master/row_id/1/field`
- `/maps/Master/extra/<tag>` (only `remove` is allowed on `extra`)

## Path operations (`edit`)

```json
{ "op": "set", "path": "/maps/Master/data/City/piece", "value": "3" }
{ "op": "add", "path": "/maps/Master/data/Email", "value": { "node": "\"contact\"", "delimiter": "\"^\"", "piece": "1" } }
{ "op": "remove", "path": "/maps/CityIdx" }
{ "op": "move", "path": "/maps/Master/subscripts/3", "to": "/maps/Master/subscripts/4" }
```

- `set` on a leaf replaces its value; `set` to `null` removes the element.
- `add` creates a map, subscript level, data field, row ID part, access variable or invalid condition; it fails if
  the target exists.
- `move` renumbers a subscript level, row ID part, access variable or invalid condition (integer names only).
- Operations apply in order to one copy of the tree; if any fails, none is written.

## Validation (before any write, FR-014)

| Rule                                                                                                | Error                          |
| --------------------------------------------------------------------------------------------------- | ------------------------------ |
| Subscript, RowIdSpec, Accessvar, Invalidcondition names are integers                                | `STORAGE_MAP_INVALID`          |
| Map `type` in the five values, exact case                                                           | `STORAGE_MAP_INVALID`          |
| `access_type` in the five values, ignoring case (spelling kept as written; research R16)            | `STORAGE_MAP_INVALID`          |
| No unknown element in a write (except one already in `extra`, which is reported as dropped-on-save) | `STORAGE_MAP_INVALID`          |
| No empty leaf set on purpose                                                                        | `STORAGE_MAP_INVALID`          |
| Map is `columnar`, or storage not `%Storage.SQL`                                                    | `STORAGE_MAP_READ_ONLY`        |
| Carried version differs from the stored one                                                         | `STORAGE_MAP_VERSION_CONFLICT` |

## Edit lifecycle

```text
read (GET, parse, versions)
  → validate ops (FR-014) → gates (FR-026..030) → data checks (FR-023)
  → scratch copy: PUT IrisDevTmp.<id>, compile, delete       ── fails → STORAGE_MAP_COMPILE_FAILED, real class untouched
  → real PUT (canonical text) → read-back compare (FR-012)  ── differs → STORAGE_MAP_DROPPED / STORAGE_MAP_WRITER_MISMATCH
  → real compile                                             ── fails → restore previous text, compile → STORAGE_MAP_RESTORED
  → opt-ins: seed_counter, build_indexes (write gate)
  → response: new model, diff, warnings
```

## Error codes (new)

| Code                                 | When                                                                                                         |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `STORAGE_MAP_NOT_FOUND`              | the class has no such map, or no Storage block                                                               |
| `STORAGE_MAP_PARSE_FAILED`           | the Storage block cannot be parsed; the response quotes where                                                |
| `STORAGE_MAP_INVALID`                | a validation rule above failed; names the rule and path                                                      |
| `STORAGE_MAP_READ_ONLY`              | default storage, a storage type other than `%Storage.SQL`, or columnar                                       |
| `STORAGE_MAP_VERSION_CONFLICT`       | stale version; carries the current map                                                                       |
| `STORAGE_MAP_CONFIRM_REMOVAL`        | `replace` would remove elements; lists them; repeat with `confirm_removal: true`                             |
| `STORAGE_MAP_COMPILE_FAILED`         | the scratch copy did not compile; IRIS errors plus explanation                                               |
| `STORAGE_MAP_RESTORED`               | the real compile failed and the previous version was restored                                                |
| `STORAGE_MAP_RESTORE_FAILED`         | the real compile failed and the restore failed too                                                           |
| `STORAGE_MAP_DROPPED`                | read-back lacks an element that was sent                                                                     |
| `STORAGE_MAP_WRITER_MISMATCH`        | read-back differs in a way the drop rules do not explain (iad bug)                                           |
| `STORAGE_MAP_ID_REUSE`               | ID counter below the highest existing ID; names both                                                         |
| `STORAGE_MAP_INDEX_UNBUILT`          | new index map over existing data with no index nodes                                                         |
| `STORAGE_MAP_DYNAMIC_GLOBAL_BLOCKED` | refusal (operator limit): a dynamic global; lifted only by `allow_dynamic_map_globals = true` for the server |
| `STORAGE_MAP_SCAN_CAPPED`            | not an error: a flag in compare/preview/propose responses                                                    |

Gate refusals keep their existing codes and the `refusal {kind, server, lift, set_aside}` shape.

## Shape report (`compare`, `preview`, `propose`)

```json
{
  "global": "^IadSm172Person",
  "nodes_scanned": 200,
  "capped": true,
  "cap": { "nodes": 200, "seconds": 5 },
  "depths": { "1": 120, "2": 80 },
  "formats": [
    {
      "depth": 2,
      "node": "\"addr\"",
      "format": "delimited",
      "delimiter": "^",
      "pieces": { "min": 3, "max": 3 }
    }
  ],
  "values": null
}
```

`values` is filled only with `include_values: true`, after the data policy (FR-021).
