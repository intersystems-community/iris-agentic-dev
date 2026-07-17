---
name: objectscript-tdd
description: Compile-test-fix loop for ObjectScript development. Use when writing or modifying ObjectScript classes.
license: MIT
metadata:
  version: "1.0.0"
  author: InterSystems Developer Community
  compatibility: objectscript, iris, healthconnect
---

## Purpose
Close the feedback loop: write → compile → fix errors → run tests → fix failures → done.

## Process Flow

1. **Write** the class
2. **Review** — run objectscript-review skill first
3. **Compile** — run the compile command for this project
4. **If compile errors**: read the error, identify the line, fix it. Return to step 3.
5. **Run tests** — run the unit test suite
6. **If test failures**: read the failure message, fix the code or the test. Return to step 3.
7. **Done** when: compiles clean + all tests pass

## Compile Command

Use the objectscript MCP tool when available (check `/mcp`):
```
iris_compile(target="MyPackage/MyClass.cls", namespace="USER")
```

Fallback when MCP is unavailable:
```
iris session IRIS -U USER "Do $System.OBJ.Load(\"<ClassName>.cls\",\"ck\")"
```

## Key Principle

Never present code to the user that hasn't compiled. If you can't compile (no IRIS access), flag this explicitly:
> ⚠️ No IRIS connection available — code is unverified. Recommend compiling before use.

## Common Compile Errors and Fixes

| Error | Cause | Fix |
|-------|-------|-----|
| `QUIT argument not allowed` | `Quit <val>` inside TRY/CATCH or loop | Change to `Return <val>` |
| `Expected a compilable class` | Missing `Class` keyword or malformed header | Check class declaration line |
| `Method does not exist` | Intra-class call without `..` | Add `..` prefix |
| `<UNDEFINED>` at runtime | Variable used before SET | Initialize variable before use |
| `Expected white space` | Missing space after command keyword | Add space: `Set x=1` not `Setx=1` |
