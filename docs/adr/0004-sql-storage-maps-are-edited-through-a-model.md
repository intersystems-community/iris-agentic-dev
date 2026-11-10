# 0004: SQL storage maps are edited through a model, written in IRIS's own form

Date: 2026-11-09. Status: accepted for spec 172; nothing is built yet.

## Context

A `%Storage.SQL` map says how a table's rows sit in a global. A developer writes it by hand inside the class's
Storage block, in an XML-like text. IRIS rewrites that text when it saves the class:

- It sorts the elements by tag name, ignoring case, then by name, with numeric names sorted as numbers.
- It drops empty values and two defaults (`ConditionalWithHostVars 0`, `BlockCount 0`).
- It wraps code containing `<`, `>` or `&` in CDATA, split at every `]]`.
- It drops an element it does not recognise, with no error or warning.
- It keeps entities such as `&gt;` as literal characters, so code written that way changes meaning.

An agent or GUI that edits the text has to know all of this, and a mistake usually compiles. Measured on IRIS 2026.2:
a field without `<Delimiter>` is read as a `$LIST`; an empty `<Piece/>` disappears; a non-integer subscript name
makes the class unreadable, while the save still reports success.

## Decision

`iris_storage_map` reads a map into a structured model, applies edits to the model as path operations (set, add,
remove, move) or as a whole-map replace, and writes the result in the canonical form IRIS itself produces. It never
splices text. Each edit carries the map version it was based on. After every save, iad reads the class back and
compares it with what it sent, so an element IRIS dropped is reported, not lost.

A throwaway prototype parsed and re-emitted all 162 maps that ship in IRIS's own `%SYS` byte for byte, and 30
single-field edits changed exactly the edited element.

## Considered options

- **Text edits, checked afterwards.** These need nothing new, but every ordering, escaping and dropping rule
  above becomes the caller's problem, and a community GUI would have to re-implement them.
- **Whole-map replace only.** This fits a GUI's Save button, but a caller can drop elements it never meant to touch.
  It is kept as one action, and it reports what it would remove before writing.

## Consequences

- The canonical writer has to track IRIS. A new storage keyword, such as `Deferred` in 2025.2, needs a model update
  and a `model_version` bump when the JSON shape changes. The round-trip tests over `%SYS` and over the public
  corpus are what catch a release where IRIS changed its writing rules.
- `iris_doc` still accepts map text, because developers push `.cls` files they edited locally. When the map
  changed, it runs the same safeguards and warns that `iris_storage_map` exists.
