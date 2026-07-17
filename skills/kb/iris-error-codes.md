# IRIS Error Codes Reference

Quick reference for IRIS runtime errors encountered during ObjectScript development.
Use alongside `debug_map_int_to_cls` and `debug_get_error_logs` tools.

---

## Runtime Errors (`<CODE>` format)

| Code | Meaning | Common Cause | Fix |
|------|---------|--------------|-----|
| `<UNDEFINED>` | Variable used before being set | Missing `Set var = ""` init; typo in variable name | Initialize before use; use `$Get(var, default)` for optional reads |
| `<PROTECT>` | No privilege on global/resource | Wrong namespace; missing role; database is read-only | Check `%Admin_Secure`, namespace mapping, database permissions |
| `<SUBSCRIPT>` | Global subscript is empty string | Passing `""` as a subscript key | Validate: `If key = "" { ... }` before global access |
| `<MAXSTRING>` | String exceeds 3,641,144 bytes | Accumulating large strings in a local | Use `%Stream.GlobalCharacter` for large content |
| `<STORE>` | Out of local or global storage | Too many local variables; namespace DB full | Kill unused locals; check DB free blocks |
| `<NOLINE>` | Label or line does not exist | Stale `.INT` after failed compile; syntax error above line | Recompile; look at line BEFORE reported number |
| `<THROW>` | Unhandled thrown exception | `$$$ThrowStatus` or `Throw` with no enclosing `Try/Catch` | Add `Try { ... } Catch ex { Set sc = ex.AsStatus() }` |
| `<NOROUTINE>` | Routine (INT/MAC) not found | Class not compiled; wrong namespace | Compile the class; check namespace |
| `<FRAMESTACK>` | Call stack too deep | Infinite recursion | Find the recursive loop; add a base-case guard |
| `<ILLEGAL VALUE>` | Argument out of range | `$Extract(str, 0)` — 0-indexed call; `$Piece` with separator "" | Use 1-based indexing; validate inputs |
| `<DIVIDE>` | Division by zero | Divisor not checked | `If divisor '= 0 { Set result = value / divisor }` |
| `<NETWORK>` | TCP/network failure | Remote host unreachable; wrong port | Check host, port, firewall; use `$System.TCP.IsAvailable()` |
| `<READ>` | Read from disconnected device | Reading `$PRINCIPAL` after client disconnect | Check `$PRINCIPAL` before reading in long-running processes |
| `<INTERRUPT>` | Process interrupted by IRIS | Watchdog timeout; `HALT` from another process | Check watchdog timeout; look for `ZSTOP` in audit log |

---

## Compile Errors (ERROR #XXXX format)

| Error # | Meaning | Fix |
|---------|---------|-----|
| `5002` | Generic ObjectScript runtime error in method | Use `debug_map_int_to_cls` to find exact `.cls` line |
| `5659` | `Return` type mismatch | Method signature says `%Status` but code returns a value, or vice versa |
| `5001` | Invalid identifier | Check for reserved word used as variable name |
| `5540` | Duplicate method name | Two methods with the same name; check superclass too |
| `414` | Class not found | Class doesn't exist in this namespace; wrong package name |
| `5628` | Expression error — invalid syntax | Missing parenthesis, operator, or command argument |
| `5578` | Parameter does not exist | `PARAMETER` referenced in code but not declared |
| `5918` | Superclass not found | `Extends` targets a class not in this namespace |

---

## %Status Error Patterns

IRIS %Status is an encoded integer or string. Always use macros — never inspect the value directly.

```objectscript
// Correct: check with macros
If $$$ISERR(sc) { Return sc }
If $$$ISOK(sc) { ... }

// Wrong: never do this
If sc = 0 { ... }       // 0 is SUCCESS in %Status, but fragile
If sc '= 1 { ... }       // 1 is $$$OK but not all errors are non-1
```

### Extracting the error text from a %Status
```objectscript
Do $System.Status.DisplayError(sc)        // prints to current device
Set text = $System.Status.GetErrorText(sc) // returns string
Set errList = $System.Status.DecomposeStatus(sc, .errors)  // structured
```

### Throwing from %Status
```objectscript
$$$ThrowOnError(sc)          // throw if error — stops execution
$$$ThrowStatus(sc)           // unconditional throw
```

---

## Interoperability / Ensemble Errors

| Pattern | Meaning | Fix |
|---------|---------|-----|
| `ERROR <EnsEDI...>` | HL7 or EDI parsing failure | Check message format against schema; use `debug_map_int_to_cls` |
| `ERROR #5540: Duplicate` | Duplicate message rule/routing | Check business rules for duplicate conditions |
| `Queue full` in logs | Input queue at capacity | Increase queue size; scale business process; fix downstream bottleneck |
| `Adapter connection failed` | Outbound adapter can't reach target | Check host/port/credentials in production settings |
| `Session timeout` | Message session exceeded limit | Increase session timeout; check for stuck processes |

---

## Reading INT Offsets

IRIS error messages include an INT offset like `+3^MyApp.Foo.1`:
- `MyApp.Foo.1` = compiled INT routine for class `MyApp.Foo`
- `+3` = line 3 of that INT routine

**Always use `debug_map_int_to_cls` to map this back to the `.cls` source line.**
Never try to read the INT directly — it is generated output and may not match the source line numbers.

```
debug_map_int_to_cls(error_string="<UNDEFINED>x+3^MyApp.Foo.1")
→ MyApp/Foo.cls line 47, method ProcessOrder
```
