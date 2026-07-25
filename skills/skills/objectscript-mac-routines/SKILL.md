---
name: objectscript-mac-routines
description: MAC routine syntax — label-based structure, #include, $ZTRAP error traps, extrinsic functions, and Quit vs Return. Use when working with .mac files, legacy CHUI applications, or any pre-class ObjectScript code.
author: tdyar
managed_by: iris-agentic-dev
---

# MAC Routine Hard Gate — Check BEFORE Writing Code

Before generating any `.MAC` code, verify all 8 items:

| #   | Check                  | Wrong                  | Right                                        |
| --- | ---------------------- | ---------------------- | -------------------------------------------- |
| 1   | Routine entry          | `MYROUTINE()` (parens) | `MYROUTINE` (no parens)                      |
| 2   | Label indentation      | `LABEL {` (braces)     | `LABEL` then tab-indented body               |
| 3   | Include syntax         | `Include %occStatus`   | `#include %occStatus`                        |
| 4   | Extrinsic call         | `##class(X).Method()`  | `$$LABEL(args)` or `$$LABEL^RTN(args)`       |
| 5   | Error trap             | `Try { } Catch e { }`  | `Set $ZTRAP="ERRHAN" ... ERRHAN Set err=$ZE` |
| 6   | Cross-routine call     | `.Method()`            | `Do LABEL^ROUTINE` or `$$FUNC^ROUTINE(args)` |
| 7   | Variable scope         | Method-scoped (class)  | `New var` to create local scope              |
| 8   | Return from subroutine | `Return`               | `Quit` (no value) in a DO label              |

## Correct MAC Structure

```objectscript
MYROUTINE
    ;Entry point - no parens, no braces
    Set x = $$CALC(3,4)
    Quit
    ;
CALC(a,b)   ;Extrinsic function - called with $$CALC(a,b)
    Quit a+b
    ;
HELPER  ;Subroutine - called with Do HELPER
    Write "done",!
    Quit
```

## $ZTRAP Error Handling (legacy pattern)

```objectscript
MYROUTINE
    Set $ZTRAP = "ERRHAN"
    ; ... code that might error ...
    Quit
    ;
ERRHAN
    Set err = $ZE
    Set $ZTRAP = ""
    Write "Error: ",err,!
    Quit
```

## Reading and writing .MAC via Atelier

VS Code's `isfs://` provider does not always expose routines, so go at the Atelier REST
API directly. The document name includes the `.mac` extension.

```bash
# Read
curl -s -u "${IRIS_USERNAME:-_SYSTEM}:${IRIS_PASSWORD:-SYS}" \
  "http://localhost:52773/api/atelier/v1/${IRIS_NAMESPACE:-USER}/doc/MYROUTINE.mac"

# Write — ignoreConflict=1 overwrites regardless of server timestamp
curl -s -X PUT -u "${IRIS_USERNAME:-_SYSTEM}:${IRIS_PASSWORD:-SYS}" \
  -H "Content-Type: application/json" \
  "http://localhost:52773/api/atelier/v1/${IRIS_NAMESPACE:-USER}/doc/MYROUTINE.mac?ignoreConflict=1" \
  -d '{"enc":false,"content":["MYROUTINE"," Quit"]}'

# List MAC routines
curl -s -u "${IRIS_USERNAME:-_SYSTEM}:${IRIS_PASSWORD:-SYS}" \
  "http://localhost:52773/api/atelier/v1/${IRIS_NAMESPACE:-USER}/docnames/MAC"
```

A PUT does not compile — follow it with `POST /action/compile` and a body of
`["MYROUTINE.mac"]`, or use the `iris_compile` MCP tool. Always re-read after writing;
the PUT response body is not a reliable confirmation. See the `compile` skill.

## Common Bugs in Legacy MAC Code

- `result` used after a For loop but never initialized → `<UNDEFINED>` if no rows
- `#include` file not found → check `^%INCLUDE` global or `.inc` file spelling
- `$$FUNC` vs `Do LABEL` confusion: `$$` for functions returning values, `Do` for subroutines
- `Quit value` in a `Do`-called subroutine → exits subroutine cleanly; `Quit` with no value for void
