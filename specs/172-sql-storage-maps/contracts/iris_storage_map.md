# Contract: `iris_storage_map`

One MCP tool. Flat params; `action` is a string with a schema enum (no `$ref`), `deny_unknown_fields`.

## Params

| Param             | Type         | Actions                                             | Default           | Notes                                                                          |
| ----------------- | ------------ | --------------------------------------------------- | ----------------- | ------------------------------------------------------------------------------ |
| `action`          | string, enum | all                                                 | required          | `read` `diagram` `uses` `compare` `preview` `propose` `check` `edit` `replace` |
| `class`           | string       | all but `uses`, `propose`                           | —                 | `Pkg.Class`, validated with `validate_class_name`                              |
| `map`             | string       | `diagram`, `compare`, `preview`, `replace`, `check` | all maps / master | map name                                                                       |
| `global`          | string       | `uses`, `propose`                                   | —                 | `^X` or `X`                                                                    |
| `namespaces`      | string[]     | `uses`                                              | target            | other namespaces to search                                                     |
| `version`         | string       | `edit`, `replace`                                   | required          | map version from `read` (storage version when editing storage keywords)        |
| `ops`             | object[]     | `edit`                                              | required          | path operations (data-model.md)                                                |
| `model`           | object       | `replace`, `check`, `preview`                       | —                 | one SqlMap in model JSON                                                       |
| `confirm_removal` | bool         | `replace`                                           | false             | second call after `STORAGE_MAP_CONFIRM_REMOVAL`                                |
| `seed_counter`    | bool         | `edit`, `replace`                                   | false             | FR-023                                                                         |
| `build_indexes`   | bool         | `edit`, `replace`                                   | false             | FR-023                                                                         |
| `include_values`  | bool         | `compare`, `preview`, `propose`                     | false             | FR-021; `true` is a body read                                                  |
| `acknowledgePhi`  | bool         | with `include_values`                               | false             | data policy                                                                    |
| `max_nodes`       | integer      | `compare`, `propose`                                | 200               | clamped 1..1000                                                                |
| `max_rows`        | integer      | `preview`                                           | 50                | clamped 1..1000                                                                |
| `namespace`       | string       | all                                                 | target            |                                                                                |
| `server`          | string       | all                                                 | default           |                                                                                |
| `confirm`         | bool         | writes                                              | false             | subject-server confirmation, as other tools                                    |

A param given to an action that does not use it is `INVALID_PARAMS` naming the param and the action.

## Answers (success)

Every answer has `success: true`, `model_version`, `action`, `class` (when given) and `namespace`.

| Action    | Extra keys                                                                                      |
| --------- | ----------------------------------------------------------------------------------------------- |
| `read`    | `storage` (StorageModel), `derived`                                                             |
| `diagram` | `mermaid` (text), `graph` `{nodes, edges}`                                                      |
| `uses`    | `global`, `namespaces_searched`, `uses: [{namespace, class, storage, map, type}]`, `count`      |
| `compare` | `map`, `shape` (ShapeReport), `mismatches: [{path, expected, seen, node}]`, `capped`            |
| `preview` | `map`, `columns`, `rows` (masked unless `include_values`), `row_count`, `capped`                |
| `propose` | `global`, `shape`, `draft` (SqlMap with `guess` markers), `capped`                              |
| `check`   | `compiled: true`, `scratch_class`, `warnings`                                                   |
| `edit`    | `storage` (new model), `changed: [path]`, `versions {before, after}`, `warnings`, `opt_ins_run` |
| `replace` | as `edit`, plus `removed: [path]`                                                               |

## Answers (error)

`{success: false, error_code, error, ...}` with codes from data-model.md; gate refusals carry `refusal {kind, server,
lift, set_aside}`. `STORAGE_MAP_VERSION_CONFLICT` carries `current` (the stored map) and `current_version`.
`STORAGE_MAP_COMPILE_FAILED` carries `iris_errors: [text]` and `explanations: [{code, text}]`.

## Stability

`model_version` is `1`. The golden-file test (`test_storage_map_golden_172`) pins the JSON of `read`, `diagram`,
`compare`, `preview` and `propose` on the fixtures. Changing a key or type requires bumping `model_version`, updating
the golden files with `IAD_REGEN_GOLDEN=1`, and a "breaking" line in the release notes.
