# Contract: `iris_storage_map` `propose` and `check` (173 additions)

The tool, its other actions and the 172 params are unchanged (`specs/172-sql-storage-maps/contracts/iris_storage_map.md`).
This file lists only what 173 adds. Shapes are in [data-model.md](../data-model.md).

## Params

| Param           | Type     | Actions   | Default | Notes                                                                                 |
| --------------- | -------- | --------- | ------- | ------------------------------------------------------------------------------------- |
| `index_globals` | string[] | `propose` | none    | at most 8 plain global names; each validated before any IRIS call                     |
| `class`         | string   | `propose` | none    | the master is the class's master map; not with `global`; refuses non-editable storage |
| `field_names`   | string[] | `propose` | none    | global mode only; by field position; valid property names; at least the sampled count |

`propose` with neither `global` nor `class`, or with both, is `INVALID_PARAMS`. `max_nodes` applies per candidate.
`include_values` additionally releases a discovered null-marker literal under the data policy (spec FR-009).

## `propose` answer: new keys

| Key                   | When                                                 | Content                                                 |
| --------------------- | ---------------------------------------------------- | ------------------------------------------------------- |
| `index_candidates`    | always                                               | listing status and every candidate with `found_by`      |
| `indexes`             | at least one candidate examined                      | one Link per candidate (verdict, ID, subscripts, draft) |
| `bitmaps`             | global mode, a candidate holds bit strings           | Bitmap items                                            |
| `chunk_map_draft`     | master is `^G(const, chunk) = $BIT`                  | data-map draft with the computed third level            |
| `class_draft`         | `field_names` or `index_globals` given (global mode) | typed class source, evidence counts, `compiled`         |
| `limits`              | always                                               | caps and which stopped what                             |
| `not_inferred_detail` | always                                               | `{what, why}` items matching `not_inferred`             |

Class mode returns `class`, `global`, `indexes`, `index_candidates`, `limits`, `not_inferred` and
`not_inferred_detail`, and omits `draft`, `shape`, `guesses`, `chunk_map_draft` and `class_draft` (the master is not
re-sampled and the class already has its properties). A candidate that holds bit strings appears in `indexes` with its
`bitmap` key; there is no separate `bitmaps` list.

## `check` answer: new key

`usability`: `[{map, subscript_level, field, status, text}]`; strings copied into `warnings`. `check`'s `success` and
its refusals do not depend on it.

## Feedback answers (T073 to T075)

- `read`: `storage.keywords` leaves out a compiler-added `StreamLocation` (named in `compiler_added`); `warnings` gains one
  string per computed level and per access variable set by code. `diagram`: a `warnings` key only when there are any, and
  one `**Warning:**` line each at the top of `markdown`.
- `compare`, `preview`, `propose` over a global with no node: `no_data: true`, `sampled_nodes: 0`, `message`. `propose`
  then has `draft: null`, `guesses: []` and a `not_inferred` line, and links nothing. With data none of the three keys is
  present.

## Unchanged guarantees

Execute call as the service account; refused under strict read-only; node cap per candidate, 5 s in total; writes
nothing and leaves no scratch class or global behind; no sampled value in any answer except under `include_values`.
