---
author: tleavitt
benchmark_date: '2026-04-02'
benchmark_iris_version: '2025.1'
benchmark_tasks:
- jira-001
- jira-002
- jira-003
- jira-004
- jira-005
- jira-006
- jira-007
- jira-008
- jira-009
- jira-010
- jira-011
- jira-012
- jira-013
- jira-014
- jira-015
- jira-016
- jira-017
- jira-018
- jira-019
- jira-020
- jira-021
- jira-056
description: Generates %UnitTest.TestCase subclasses for ObjectScript classes using
  live IRIS introspection. Use when asked to write tests for any ObjectScript class
  or method.
iris_version: '>=2024.1'
name: objectscript-unit-test
pass_rate: 0.8636363636363636
state: reviewed
tags:
- objectscript
- unit-test
- iris
trigger: Use for tleavitt/objectscript-unit-test
---

## Purpose
Generate idiomatic IRIS `%UnitTest.TestCase` test scaffolds grounded in live class introspection — not guessed from code. Each generated test compiles and runs against a real IRIS instance.

## When to Use
- Writing tests for a new or existing ObjectScript class
- Adding coverage for a specific method before refactoring
- Validating that a bug fix doesn't regress

## Workflow

### Step 1 — Generate Test Class
```
Tool: objectscript_iris_generate_test
Inputs: class_name (e.g. "MyApp.Utils"), method_name (optional, generates all if omitted), test_directory (default "tests")
Returns: test class code + suggested output path
```

The tool uses `objectscript_docs_introspect` to read the live class definition — method signatures, return types, class vs instance methods — and generates appropriate test scaffolds.

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
```
Tool: objectscript_iris_compile
Inputs: target (path to generated .cls file)
```
```
Tool: objectscript_iris_test
Inputs: pattern (e.g. "Test.MyApp.*")
```

Fix any compile errors, then confirm all generated tests pass before adding assertions.

### Step 4 — Add Real Assertions
The scaffold generates placeholder assertions. Replace them with meaningful expected values based on the method's documented behavior or known inputs/outputs.

## Naming Conventions

| Element | Convention |
|---|---|
| Test class | `Test.<OriginalPackage>.<ClassName>` |
| Test method | `Test<OriginalMethodName>` |
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
> Next: compile and run with `objectscript_iris_test("Test.MyApp.*")`