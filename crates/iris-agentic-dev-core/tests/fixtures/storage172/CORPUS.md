# Storage-map corpus: patterns and their fixtures

The fixtures in `classes/` cover the common shapes. These patterns occur in real SQL storage maps. Each group was one
task (T101a, T101b, …), invented from scratch with its own seed and known answer, like the others. A pattern with no
fixture says why.

## T101a: delimiters and piece forms (`Ledger`)

- a field delimited by a printable character other than `^` (`|`)
- a field delimited by a control character written as `$C(n)` (`$C(1)`)
- one map whose fields use different delimiters
- a field with no `Piece`: the whole node is the value (`Memo`)
- a `Piece` given as an expression rather than an integer (`1+1`)

## T101b: access types and subscript code (`Route`)

- `AccessType` written in lower case (`sub`), which IRIS reads as `Sub`
- a subscript with an access variable (`Accessvar` with `Code`), read by the next level: an Accessvar is not visible
  at its own level (#5524)
- a subscript with `StopExpression`
- a subscript with an `Invalidcondition`
- not yet: `AccessType Piece` and `AccessType Other`, and a subscript with `DataAccess`, whose semantics are not
  measured

## T101c: map options (`Shipment`)

- `BlockCount`, `PopulationPct` and `Structure` on a map
- a conditional map with `Condition`, `ConditionFields` and `ConditionalWithHostVars` (IRIS stores
  `ConditionalWithHostVars` first)

## T101d: node layout

- not yet: a field whose node is a subscript level of the row, not a constant
- not yet: a parent table with a child table stored under the parent's row node through a parent-child
  `Relationship`. It is two classes that must load together, and the rig loads one class per fixture. (`Loan` and
  `LoanEvent` cover a child stored under a parent's row node that holds a value, with a composite key.)

## T101e: map types and storage types (`Badge`)

- a `bitmapextent` map. It has no subscripts (IRIS adds the chunk), and until it is built `COUNT(*)` answers 0
- the storage type written with the older class name (`%CacheSQLStorage`): IRIS stores it as `%Storage.SQL`, so there
  is no fixture text to keep; `test_storage_map_paths_172_the_old_storage_type_name_is_accepted` puts it

## T101f: global forms (read side only)

- not yet: a map whose `Global` is an extended reference (`^|"NS"|G`), and one whose `Global` is an expression. The
  `iris_doc` map check refuses both, so the rig cannot load them; they need a raw load, as
  `test_storage_map_d11_172_live` does
