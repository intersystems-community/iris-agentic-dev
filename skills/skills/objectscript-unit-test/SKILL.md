---
name: objectscript-unit-test
tier: core
description: Generates %UnitTest.TestCase subclasses for ObjectScript classes using live IRIS introspection. Use when asked to write tests for any ObjectScript class or method.
license: MIT
metadata:
  version: "1.0.0"
  author: InterSystems Developer Community
  compatibility: objectscript, iris, healthconnect
  spec: 009-unit-test-skill
---

## Purpose

Generate idiomatic IRIS `%UnitTest.TestCase` test scaffolds grounded in live class introspection — not guessed from code. Each generated test compiles and runs against a real IRIS instance.

## When to Use

- Writing tests for a new or existing ObjectScript class
- Adding coverage for a specific method before refactoring
- Validating that a bug fix doesn't regress

## Workflow

### Step 1 — Generate Test Class

```text
Tool: iris_generate_test
Inputs: class_name (e.g. "MyApp.Utils"), method_name (optional, generates all if omitted), test_directory (default "tests")
Returns: test class code + suggested output path
```

The tool uses `docs_introspect` to read the live class definition — method signatures, return types, class vs instance methods — and generates appropriate test scaffolds.

### Step 2 — Review Generated Scaffold

Each generated test method follows this pattern:

```objectscript
Method TestMyMethod()
{
    // Arrange
    Set tObj = ##class(MyApp.Utils).%New()

    // Act
    Set tResult = tObj.MyMethod()

    // Assert
    Do $$$AssertEquals(tResult, expectedValue, "MyMethod should return expected value")
}
```

For `%Status`-returning methods, assertions use `$$$AssertStatusOK`:

```objectscript
Do $$$AssertStatusOK(tSC, "MyMethod should succeed")
```

### Step 3 — Compile and Run

Put the class on the server and compile it there. `iris_compile` only recompiles source IRIS already holds; it does not read local files.

```text
iris_doc(mode="put", name="Test.MyApp.Utils.cls", content="...", compile=true)
iris_test(pattern=":Test.MyApp.Utils")
```

The leading colon runs one class. Only methods whose names start with `Test` run. A method named `CheckSum` or `VerifySub` is skipped without a word. If no method runs, `%UnitTest` still prints `All PASSED`; iad returns NO_TESTS_FOUND instead. Treat NO_TESTS_FOUND as a failure, not a pass.

Fix any compile errors, then confirm all generated tests pass before adding assertions.

Run tests through `iris_test`, not by calling `%UnitTest.Manager` in `iris_execute`. The manager checks `messages.log` after each test class and when it closes; if the log grew, it opens it, reads it, closes it, and leaves `$IO` on `$PRINCIPAL`. Under `iris_execute` that loses the output, or corrupts the whole answer if it happens mid-test, and any process writing to `messages.log` sets it off. `iris_test` runs the manager in a background job, which it cannot harm. If you must call the manager yourself, do it in a `Job` and read `^UnitTest.Result` afterwards.

### Step 4 — Add Real Assertions

The scaffold generates placeholder assertions. Replace them with meaningful expected values based on the method's documented behavior or known inputs/outputs.

## Naming Conventions

| Element     | Convention                             |
| ----------- | -------------------------------------- |
| Test class  | `Test.<OriginalPackage>.<ClassName>`   |
| Test method | `Test<OriginalMethodName>`             |
| Output file | `tests/Test/<Package>/<ClassName>.cls` |

## What Gets Generated

- One test method per public, non-private method in the class
- Instance methods: `%New()` + method call
- Class methods: direct `##class()` call
- `%Status` returns: `$$$AssertStatusOK` assertion
- All other returns: `$$$AssertEquals` placeholder assertion
- Private methods: skipped (prefix `%` or marked `Private`)

## Output Format

> ✅ Generated test class `Test.MyApp.Utils` — `N` test methods
> Output path: `tests/Test/MyApp/Utils.cls`
> Next: put and compile with `iris_doc(mode="put", ..., compile=true)`, then run `iris_test(pattern=":Test.MyApp.Utils")`

## Related skills

- **objectscript-tdd** — the compile-test-fix loop that uses the tests you write here
- **objectscript-coverage** — measure how well your test suite exercises the code
- **objectscript-debugging** — when a test fails with a runtime error and you need to trace it
