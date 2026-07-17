---
name: "tdyar/objectscript-mac-routines"
description: "MAC routine syntax, label-based structure, #include, $ZTRAP, extrinsic functions. Use when working with .mac files, legacy CHUI apps, or any pre-class IRIS code."
trigger: "when the user mentions .mac, .MAC, CHUI, legacy routine, or label-based ObjectScript"
iris_version: ">=2020.1"
tags: [objectscript, mac, routine, legacy, chui]
author: "tdyar"
state: "draft"
---

# MAC Routine Hard Gate — Check BEFORE Writing Code

Before generating any `.MAC` code, verify all 8 items:

| # | Check | Wrong | Right |
|---|-------|-------|-------|
| 1 | Routine entry | `MYROUTINE()` (parens) | `MYROUTINE` (no parens) |
| 2 | Label indentation | `LABEL {` (braces) | `LABEL` then tab-indented body |
| 3 | Include syntax | `Include %occStatus` | `#include %occStatus` |
| 4 | Extrinsic call | `##class(X).Method()` | `$$LABEL(args)` or `$$LABEL^RTN(args)` |
| 5 | Error trap | `Try { } Catch e { }` | `Set $ZTRAP="ERRHAN" ... ERRHAN Set err=$ZE` |
| 6 | Cross-routine call | `.Method()` | `Do LABEL^ROUTINE` or `$$FUNC^ROUTINE(args)` |
| 7 | Variable scope | Method-scoped (class) | `New var` to create local scope |
| 8 | Return from subroutine | `Return` | `Quit` (no value) in a DO label |

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

## Reading .MAC via Atelier (bypasses isfs:// limitation)

```
iris_read_document("MYROUTINE.mac")   → returns full source
iris_write_document("MYROUTINE.mac", fixedCode)  → writes + compiles
iris_list_documents(filter="PATH*", category="MAC")  → lists routines
```

## Common Bugs in Legacy MAC Code

- `result` used after a For loop but never initialized → `<UNDEFINED>` if no rows
- `#include` file not found → check `^%INCLUDE` global or `.inc` file spelling
- `$$FUNC` vs `Do LABEL` confusion: `$$` for functions returning values, `Do` for subroutines
- `Quit value` in a `Do`-called subroutine → exits subroutine cleanly; `Quit` with no value for void
