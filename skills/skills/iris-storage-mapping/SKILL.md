---
name: iris-storage-mapping
tier: extra
description: Read, check and edit SQL storage maps (%Storage.SQL, hand-written SQLMap elements that lay a table over existing globals) with iris_storage_map. Use when a class has StorageStrategy pointing at SQL storage, when a table over an old global returns wrong or missing rows, when adding a column or index to a mapped class, or when asked to map an unmapped global. Covers read, diagram, compare, preview, check, edit, replace, propose, seed_counter and build_indexes, and the IRIS behaviour that makes a map edit go wrong.
author: tdyar
version: 0.2.0
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

## A level that is not a subscript

A level with `AccessType Other` or a `NextCode` is computed by code, and the global may hold no node at
that level: a `$BIT` (or `$LIST`) in the value is a common reason, and the table still returns rows
(live test: `computed_172_the_look_alike_really_returns_the_ids_its_bits_name`). `read` and `diagram`
flag such a level (live test: `computed_172_read_flags_level_three_and_the_streamlocation_is_compiler_added`).
Do not describe it as a subscript of the global, and do not add or remove fields from it before you
have `compare`d with real data. `compare` says `verified: false` when the global is empty (live
test: `computed_172_compare_over_an_empty_global_says_it_checked_nothing`), and names the depth the data
sits at when it differs from the map's levels (live test:
`computed_172_compare_over_data_names_the_depth_the_bits_sit_at`).

An `Invalidcondition` on a level is a filter, not a computed level. `read` names `StreamLocation`
under `compiler_added` and leaves it out of `storage`: it is the compiler's own element, and the
class does not use a stream (live test:
`test_storage_map_feedback_173_read_leaves_out_the_compiler_added_stream_location`).

## Maps that compute a level

Three signs in a map say a level may not be a subscript of the global: `AccessType Other`, a
`NextCode`, and an `Invalidcondition` (a filter on keys that do exist). An access variable whose
`Code` sets it is the fourth, smaller sign: the value comes from code, not from the global. iad does
not run any of that code. A `$BIT` or `$LIST` value makes a level that is not a subscript: the
positions of its 1 bits, or the list items, are the keys the map's code produces.

`read` and `diagram` add one `warnings` line per such level and per such variable, naming the map and
the level; `diagram` also puts a `**Warning:**` line at the top of its `markdown` (live test:
`test_storage_map_feedback_173_read_and_diagram_warn_about_each_computed_level`). Run `compare` before
you trust the map: it samples the real global and names the depth the data sits at (live test:
`computed_172_compare_over_data_names_the_depth_the_bits_sit_at`).

## A global with no map

`propose` samples the global and drafts a map in model JSON, listing every guessed element. Rename
the guessed fields to the class's properties, then `check` and `replace`; the draft of a seeded
global answers its known question (live test: `test_storage_map_data_172_propose_drafts_a_working_map`).
Over a global with no data it drafts nothing and answers `no_data: true` with the global's name (live
test: `test_storage_map_feedback_173_propose_over_an_empty_global_drafts_nothing_and_says_why`);
`compare` and `preview` say the same. It is a draft to check against real data. `not_inferred` lists what it left out: datatypes, collations,
index maps, conditions, computed subscripts and the ID counter (live test:
`computed_172_propose_lists_what_it_did_not_infer`).

## What `propose` infers and what it does not

Given a master global, `propose` also finds its index globals (the names you pass in `index_globals`, the
globals whose name starts with the master's, and literal sub-trees inside the master), and says which
master field and which collation each index subscript holds. Pass `class` instead of `global` to link
against a class's own master map. The claims below are the ones a test holds; the full answer shape is in
`docs/storage-maps.md`.

- A subscript is `inferred` only with at least 20 usable nodes and agreement of 0.95 or more, and
  `ambiguous` when two pairs tie, with both listed in `alternatives` (live test:
  `f26_identical_columns_are_ambiguous_with_both_fields_listed`). A column of digits only is always
  `ambiguous`: exact, ALPHAUP and UPPER file the same string for it (live test:
  `f11_to_f18_link_to_the_known_answer`).
- With fewer than 20 usable nodes the verdict is `too_few_nodes` and no map is drafted (live test:
  `f27_five_rows_are_too_few_nodes_and_draft_no_map`). A position with one value is `constant` (live
  test: `f28_a_constant_column_is_constant`).
- A global that shares the master's name prefix and is unrelated is `not_linked`, listed with its best
  agreement and not dropped (live test:
  `f31_an_unrelated_global_is_not_linked_with_its_best_agreement`). A column that happens to equal the
  ID is never taken for the ID (live test: `f29_age_equal_to_id_is_never_the_id`).
- A subscript made by code, such as a date format, is `not_explained` and no code is drafted for it (live
  test: `f33_a_computed_subscript_is_not_explained_and_names_no_code`). The closed set of transforms is
  the seven collation functions a map expression can name and `{F}` itself; `$$EXACT`, `$$PLUS`,
  `$$MINUS` and `$$SPACE` do not compile there (live test: `fact_1_closed_set_of_collation_functions`).
- The sample spreads over the key range, so two collations that differ only late in the keys are told
  apart, which a walk of the first nodes in order cannot do (live test:
  `f34_the_spread_sample_separates_what_a_prefix_walk_cannot`).
- A bitmap index is drafted as `Type bitmap` with the field alone as its subscript, because the engine
  appends the chunk (live test: `f19_map_compiled_bitmap_is_decoded_drafted_and_verified`). The chunk
  size is confirmed only when a later chunk was sampled; otherwise the verdict is `ambiguous` and
  `assumed` names the size (live test:
  `a_decode_that_needs_an_unconfirmed_chunk_size_is_ambiguous_and_says_so`).
- An index that lacks master rows is drafted as a conditional map when one rule explains the gap. NULL
  and an empty string are different rules: the empty-string condition is `{F}'=$c(0)`, because
  `{F}'=""` skips NULL only (live test:
  `f23_an_empty_string_rule_drafts_the_c0_condition_not_the_null_one`). A rule that keeps one fixed
  value shows its literal only with `include_values` under the data policy (live test:
  `f24_a_fixed_value_rule_needs_include_values_for_its_literal`), and when no rule fits there is no
  condition (live test: `f25_no_rule_fits_every_seventh_row`).
- A few index nodes with no master row, as deleted rows leave behind, are not mentioned up to 5%; above
  that `orphan_nodes` counts them, and past 10% the index is `not_linked` (live test:
  `f44_f45_orphan_nodes_are_reported_only_above_five_percent`).
- Candidates are capped at 8: the rest are named in `index_candidates` and `not_inferred_detail` (live
  test: `ten_candidates_link_eight_and_name_the_other_two`), and a listing that failed is said, with
  the other sources still examined (live test:
  `a_failed_listing_is_said_and_the_other_two_sources_still_link`).
- A draft is a guess to `check`, not an answer. It carries `verified` only when it compiled in a scratch
  copy, found its row, and the planner's usability line for the field is `used` (live test:
  `a_verified_draft_carries_a_used_line_for_the_field_it_links`); a draft that does not verify has
  no `map` (live test: `no_drafted_map_is_one_the_planner_would_not_use`). Run `check` on the map after
  you rename its fields: it adds the same `usability` line (live test:
  `usability_follows_the_planners_words`).
- With `class`, the drafts use the class's own property names, and an index the class already maps is
  `already_mapped` and drafts nothing (live test:
  `f35_drafts_the_missing_index_in_the_classs_own_names`, and
  `f36_an_index_global_the_class_maps_is_already_mapped_and_drafts_nothing`).
- `class_draft` types each property from counts of what the values look like: integer, numeric with its
  scale, a date only when at least 95% fit, else a string with a `MAXLEN` (live test:
  `f37_each_column_gets_the_type_its_values_support`).
- A `COLLATION` goes on a property only where the link is `inferred` and whole, and only on a string
  (live test: `f37_the_collation_follows_how_the_index_files_the_field`).
- No sampled value is in an answer, except a discovered null marker's literal and a fixed rule's literal
  under `include_values` and `dataPolicy = "allow"` (live test:
  `f18_names_the_missing_marker_and_returns_no_literal`). Under the `block` policy `include_values` is
  refused (live test: `include_values_is_refused_under_the_block_policy`).
- No answer says an index is unique or gives its cardinality (live test:
  `no_answer_key_claims_uniqueness_or_a_cardinality`). What `propose` leaves to you is in
  `not_inferred` and `not_inferred_detail`: datatypes beyond the four it types, collations it cannot
  draft, computed subscripts and the ID counter.

## Map code

Subscript expressions, `NextCode`, `RetrievalCode`, `Condition` and the other code elements are
ObjectScript that runs whenever the table is queried. iad gates them like `iris_execute` code. Once
saved, they run as whoever queries the table: review them like class code. Details:
`docs/storage-maps.md`.
