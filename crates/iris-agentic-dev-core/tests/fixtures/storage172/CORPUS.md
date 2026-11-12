# Storage-map corpus: patterns still without a fixture

The fixtures in `classes/` cover the common shapes. These patterns occur in real SQL storage maps
and have no public fixture yet. Each line is a pattern, in words; each group is one fixture to
write (T101a, T101b, …), invented from scratch with its own seed and known answer, like the others.

## T101a: delimiters and piece forms

- a field delimited by a printable character other than `^` (for example `|` or `,`)
- a field delimited by a control character written as `$C(n)`
- one map whose fields use different delimiters
- a field with no `Piece`: the whole node is the value
- a `Piece` given as an expression rather than an integer

## T101b: access types and subscript code

- `AccessType` written in lower case (`sub`, `other`), which IRIS reads as `Sub` and `Other`
- `AccessType Piece` and `AccessType Other`
- a subscript with an access variable (`Accessvar` with `Code`)
- a subscript with `DataAccess`
- a subscript with `StopExpression`
- a subscript with an `Invalidcondition`

## T101c: map options

- `BlockCount`, `PopulationPct` and `Structure` on a map
- a conditional map with `ConditionFields` and `ConditionalWithHostVars`

## T101d: node layout

- a field whose node is a subscript level of the row, not a constant
- a parent table with a child table stored under the parent's row node (parent-child), where the
  parent's row node also holds a value

## T101e: map types and storage types

- a `bitmapextent` map
- the storage type written with the older class name (`%CacheSQLStorage`)

## T101f: global forms (read side only)

- a map whose `Global` is an extended reference (`^|"NS"|G`): iad reads it and refuses to write it
- a map whose `Global` is an expression: iad reads it and refuses it unless the operator allows
  dynamic globals
