---
name: iris-storage-mapping
tier: extra
description: Read, check and edit SQL storage maps (%Storage.SQL, hand-written SQLMap elements that lay a table over existing globals) with iris_storage_map. Use when a class has StorageStrategy pointing at SQL storage, when a table over an old global returns wrong or missing rows, when adding a column or index to a mapped class, or when asked to map an unmapped global. Covers read, diagram, compare, preview, check, edit, replace, propose, seed_counter and build_indexes, and the IRIS behaviour that makes a map edit go wrong.
author: tdyar
managed_by: iris-agentic-dev
---

# SQL storage maps with iris_storage_map

A SQL storage map tells IRIS how a table's rows sit in a global: which subscripts are the row ID,
which node and piece each column comes from. Edit maps with `iris_storage_map`, not by changing the
Storage block text through `iris_doc`: `iris_storage_map` checks the map against the data and keeps
the class readable. Default storage (`%Storage.Persistent`) is a different thing and stays off-limits.

Every claim below names the live test that shows it on IRIS 2026.2.

## Work in this order

1. `read` the class: the maps as JSON, with the facts IRIS applies but does not store, such as "no
   delimiter means `$LIST`" (live test: `read_member_matches_the_compiled_dictionary`).
2. `diagram` it when you need to explain the layout: its `markdown` shows each map's global layout,
   subscripts and fields (live test: `diagram_returns_mermaid_and_graph`).
3. `compare` the map with the real data before changing anything. It reports a field on a piece the
   data does not have (live test: `test_storage_map_data_172_compare_finds_a_field_on_the_wrong_piece`)
   and nodes no field reads (live test: `test_storage_map_data_172_compare_finds_unread_nodes`).
4. `check` a proposed map: it compiles in a scratch copy and the real class is not touched (live
   test: `test_storage_map_edit_172_check_failure_is_explained_and_leaves_the_class`).
5. `preview` the rows the proposed map would return; values are masked unless you pass
   `include_values` (live test: `test_storage_map_data_172_preview_returns_the_known_rows`).
6. `edit` or `replace` with the `version` from `read`. A stale version is refused with the current
   map (live test: `test_storage_map_edit_172_edit_refusals`).

To add a column: store the class with the new property first, without compiling, then `edit` the
map to add the field (live test: `test_storage_map_edit_172_add_a_field`). A property no data map
files does not compile.

## What goes wrong, and what iad does about it

- A `$i(^G)` ID expression over rows 1 to 3 with no counter node gives the next INSERT ID 1, and
  row 1 is overwritten without an error (live test:
  `test_storage_map_data_172_counterless_insert_overwrites_row_1`). iad refuses such a write as
  `STORAGE_MAP_ID_REUSE`; repeat with `seed_counter: true` and iad sets the counter and reports the old
  value (live test: `test_storage_map_data_172_id_reuse_and_seed_counter`).
- A new index map over existing data finds nothing until the index is built (live test:
  `test_storage_map_data_172_unbuilt_index_finds_nothing`). iad refuses it as
  `STORAGE_MAP_INDEX_UNBUILT`; `build_indexes: true` builds it, and needs the destructive tier
  (live test: `test_storage_map_data_172_unbuilt_index_and_build_indexes`).
- A map that walks a level as row IDs returns a parent's named node, such as `"EV"`, as a row (live
  test: `d11_a_level2_walk_returns_the_literal_node_as_a_row`). iad refuses the write unless the
  map skips those nodes with a `NextCode` (live test:
  `test_storage_map_data_172_a_walk_over_named_nodes_is_refused`).
- A SQL DELETE of a row also deletes rows another map stores under it (live test:
  `d11_delete_of_a_parent_row_kills_its_child_rows`). `read` says so for nested maps.
- The map type is case-sensitive: `DATA` does not compile, `data` does (live test:
  `test_storage_map_scratch_172_compile_scratch_class`).
- If the real compile fails after the scratch copy passed, iad puts the previous class back
  (`STORAGE_MAP_RESTORED`) (live test: `test_storage_map_edit_172_failed_real_compile_restores`).

## A global with no map

`propose` samples the global and drafts a map in model JSON, listing every guessed element. Rename
the guessed fields to the class's properties, then `check` and `replace`; the draft of a seeded
global answers its known question (live test: `test_storage_map_data_172_propose_drafts_a_working_map`).

## Map code

Subscript expressions, `NextCode`, `RetrievalCode`, `Condition` and the other code elements are
ObjectScript that runs whenever the table is queried. iad gates them like `iris_execute` code. Once
saved, they run as whoever queries the table: review them like class code. Details:
`docs/storage-maps.md`.
