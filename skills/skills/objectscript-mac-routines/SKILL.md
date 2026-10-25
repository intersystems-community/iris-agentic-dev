---
name: objectscript-mac-routines
tier: extra
description: MAC routine syntax — label-based structure, #include, $ZTRAP error traps, extrinsic functions, and Quit vs Return. Use when working with .mac files, legacy CHUI applications, or any pre-class ObjectScript code.
author: tdyar
managed_by: iris-agentic-dev
---

# MAC Routine Hard Gate — Check BEFORE Writing Code

Before generating any `.MAC` code, verify all 8 items:

| #   | Check                  | Wrong                  | Right                                          |
| --- | ---------------------- | ---------------------- | ---------------------------------------------- |
| 1   | Routine entry          | `MYROUTINE()` (parens) | `MYROUTINE` (no parens)                        |
| 2   | Label indentation      | `LABEL {` (braces)     | `LABEL` then tab-indented body                 |
| 3   | Include syntax         | `Include %occStatus`   | `#include %occStatus`                          |
| 4   | Extrinsic call         | `##class(X).Method()`  | `$$LABEL(args)` or `$$LABEL^RTN(args)`         |
| 5   | Error trap             | none                   | `Try { } Catch e { }` or `Set $ZTRAP="ERRHAN"` |
| 6   | Cross-routine call     | `.Method()`            | `Do LABEL^ROUTINE` or `$$FUNC^ROUTINE(args)`   |
| 7   | Variable scope         | Method-scoped (class)  | `New var` to create local scope                |
| 8   | Return from subroutine | none                   | `Quit` or `Return`                             |

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
iris-agentic-dev tool iris_doc --args '{"mode":"get","name":"MYROUTINE.mac"}'

# Write (checks the document out of source control first when one is configured)
iris-agentic-dev tool iris_doc --args '{"mode":"put","name":"MYROUTINE.mac","content":"MYROUTINE\n Quit\n"}'

# List MAC routines
iris-agentic-dev tool iris_doc --args '{"mode":"list","pattern":"*.mac"}'
```

Without the CLI, call Atelier with `curl -K -`, which reads the credential from stdin so it stays out
of the process list. `IRIS_SCHEME` is `https` for an HTTPS endpoint:

```bash
printf 'user = "%s:%s"\n' "${IRIS_USERNAME:-_SYSTEM}" "$IRIS_PASSWORD" \
  | curl -s -K - "${IRIS_SCHEME:-http}://${IRIS_HOST:-localhost}:${IRIS_WEB_PORT:-52773}/api/atelier/v1/${IRIS_NAMESPACE:-USER}/doc/MYROUTINE.mac"
```

A PUT to the same URL with `?ignoreConflict=1` overwrites regardless of the server timestamp.

A PUT does not compile — follow it with `POST /action/compile` and a body of
`["MYROUTINE.mac"]`, or use the `iris_compile` MCP tool. Always re-read after writing;
the PUT response body is not a reliable confirmation. See the `compile` skill.

## Common Bugs in Legacy MAC Code

- `result` used after a For loop but never initialized → `<UNDEFINED>` if no rows
- `#include` file not found → check `^%INCLUDE` global or `.inc` file spelling
- `$$FUNC` vs `Do LABEL` confusion: `$$` for functions returning values, `Do` for subroutines
- `Quit value` in a `Do`-called subroutine → exits subroutine cleanly; `Quit` with no value for void
