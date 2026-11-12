# Release notes draft: SQL storage maps (spec 172)

## What's new

`iris_storage_map` edits SQL storage maps without breaking the table. Classes that lay a table
over existing globals with hand-written `%Storage.SQL` maps used to need Studio's map editor, which
never looks at the data. The new tool reads a class's maps as JSON, draws them as Mermaid or a
Markdown page, finds every class that uses a global, and checks a map against the real data before
anything is saved.

- `compare` samples the global and lists where the map and the data disagree: a field on a piece
  the data does not have, `$LIST` against `^`-delimited text, nodes no field reads.
- `check` compiles a proposed map in a temporary copy of the class and deletes it. Compiling the copy
  writes nothing to the real globals; I measured that.
- `preview` shows the rows a proposed map would return, with values masked unless you ask for them
  and the data policy allows it.
- `edit` and `replace` change a map only if nobody else changed the class since you read it, read
  it back to make sure IRIS kept what was sent, and put the old version back if the compile fails.
- `propose` drafts a map for a global that has none.

Before a write, iad refuses three mistakes that IRIS accepts without an error: an ID counter behind
the highest stored ID (the next INSERT would overwrite row 1), a new index over existing rows that
was never built (queries would find nothing), and a map that would read a parent's named node as a
row. `seed_counter` and `build_indexes` fix the first two in the same call.

A new skill, `iris-storage-mapping`, walks an agent through the order that keeps a table working:
read, compare, check, then edit. `docs/storage-maps.md` has the full reference.

## Changed

- An `iris_doc` put of a class with an SQL storage map gets the same checks, and its answer says to
  use `iris_storage_map`. Default storage is unchanged.
- Refusals from `iris_doc` now carry the `refusal` object, like the other tools.
