> Staging file. When the next tag is cut, this becomes the GitHub release body via
> `gh release edit <tag> --notes`. The constitution requires What's new / Notable fixes /
> Breaking changes plus a `v0.9.6...<tag>` compare link.
> There is no `CHANGELOG.md` in this repo; the release body is the changelog.
>
> Written against `v0.9.7` as the next tag.

## What's new

### `iris_symbols_local` upgrades

The local symbol scanner got a full overhaul.

- **Line numbers** — every symbol now includes a `line` field (1-based) pointing at its
  definition in the source file. Useful for jump-to-definition workflows.
- **Return types** — methods include a `Type` field when the source declares one.
- **Structured `FormalSpec`** — method parameters are now a structured array
  (`{name, type, byref, output, default}`) instead of a raw string. Byref and output
  flags are only present when true; default is only present when set.
- **All 12 member kinds** — the scanner now emits symbols for indexes, queries, XData
  blocks, storage definitions, relationships, foreign keys, projections, and triggers, in
  addition to classes, methods, properties, and parameters.
- **Routine name from grammar** — `.mac` and `.int` routine names are read from the
  `ROUTINE` header, not inferred from the filename.
- **Case-insensitive glob** — `myapp.*` and `MyApp.*` match the same symbols.
- **Member-level glob** — `MyApp.MyClass.Do*` filters to members matching `Do*` inside
  `MyApp.MyClass`. Previously only class-level globs were supported.
- **`kinds` filter** — pass `kinds: ["method", "property"]` to return only those symbol
  kinds. Accepts the full list: `class`, `method`, `property`, `parameter`, `index`,
  `xdata`, `query`, `trigger`, `relationship`, `foreignkey`, `projection`, `storage`,
  `routine`, `label`.

### BPL and DTL support

`docs_introspect` and `extract_message_map_routing` now handle BPL and DTL classes.

**`docs_introspect`** adds an `xdata_flow` field for BPL and DTL classes:

- BPL: `kind=bpl`, `steps` array (each step has `step_kind`, `name`, and `target` for
  Call steps), `has_dynamic_dispatch` flag set when `$classmethod` appears in a Code step.
- DTL: `kind=dtl`, `source_class`, `target_class`, `subtransforms`, `assign_count`.

**`extract_message_map_routing`** now handles three class types:

- MessageMap routers: unchanged — `message_type → method` table at confidence 0.9.
- BPL classes (`Ens.BusinessProcessBPL`): `kind=bpl`, routes derived from Call steps at
  confidence 0.8. Includes a `note` when dynamic dispatch is detected.
- DTL classes (`Ens.DataTransformDTL`): `kind=dtl`, `source_class`, `target_class`, empty
  routes.
- Any other class: returns `NOT_FOUND`.

### `docs_introspect` — structured FormalSpec

`docs_introspect` now returns `FormalSpec` as a structured array (same format as
`iris_symbols_local`) rather than the raw IRIS FormalSpec string. Each element is
`{name, type, byref, output, default}` with optional fields omitted when not set.

## Notable fixes

- **BPL/DTL export required the `.bpl`/`.dtl` suffix.** Exporting a BPL or DTL class by
  bare class name fails with IRIS error 6304. The export now uses the correct item name.
- **quick_xml split nested CDATA into fragments.** IRIS exports BPL XData with
  `<![CDATA[...]]]]><![CDATA[>` escaping for ObjectScript code blocks. The parser
  returned only the first fragment, cutting off the BPL XML mid-element. It now
  accumulates all fragments until `</Data>`.
- **BPL routing hit the MessageMap generator path.** The generator produces empty output
  for BPL classes (output capture fails when `Quit` interacts with `}` in write
  statements). BPL/DTL detection now runs before the generator, not after.

## Breaking changes

`docs_introspect` — `FormalSpec` is now a structured array. Previously it was the raw
IRIS FormalSpec string (e.g. `"pName As %String = """`). Update any code that reads
`methods[n].FormalSpec` as a string.

**Full changelog:**
[`v0.9.6...v0.9.7`](https://github.com/intersystems-community/iris-agentic-dev/compare/v0.9.6...v0.9.7)
