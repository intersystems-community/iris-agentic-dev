---
author: tdyar
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
description: 'Worked fix examples for the 6 most common ObjectScript LLM mistakes.
  Uses "Bug Pattern → Root Cause → Fix" structure per SOTA code repair research. Use
  when fixing ObjectScript bugs involving: Return vs Quit in loops, HTML escaping
  order, SQL date filters, list operations, or postfix conditions.

  '
iris_version: '>=2024.1'
name: objectscript-fewshot-fixes
pass_rate: 0.5294117647058824
state: reviewed
tags:
- objectscript
- fewshot
- repair
trigger: 'Use when fixing any ObjectScript bug. These examples demonstrate the exact
  reasoning + fix pattern for the most common failure modes.

  '
---

# ObjectScript Fix Examples

**HOW TO USE THESE**: Before generating any fix, first identify which pattern matches
the bug, state the root cause explicitly, THEN generate the fix. Research shows this
"identify-then-fix" order reduces errors by ~15%.

---

## EXAMPLE 1: Quit-in-Loop Returns Wrong Value

BUGGY CODE:
```objectscript
ClassMethod IsNameUnique(name As %String, existingNames As %ListOfDataTypes) As %Boolean
{
    For i=1:1:existingNames.Count() {
        If (existingNames.GetAt(i) = name) {
            Quit 1
        }
    }
    Quit 0
}
```

BUG PATTERN: `Quit value` inside a `For` loop exits the loop, not the method.
ROOT CAUSE: After `Quit 1`, execution continues AFTER the loop — the method returns
the value of `Quit 0` at the bottom, or empty string if Quit had a value (IRIS-specific).
The return values are also logically swapped (found = not unique = should return 0).

FIXED CODE:
```objectscript
ClassMethod IsNameUnique(name As %String, existingNames As %ListOfDataTypes) As %Boolean
{
    For i=1:1:existingNames.Count() {
        If (existingNames.GetAt(i) = name) {
            Return 0
        }
    }
    Return 1
}
```

KEY RULE: `Return value` always exits the method. `Quit value` inside a loop only exits the loop.

---

## EXAMPLE 2: HTML Escaping Order

BUGGY CODE:
```objectscript
ClassMethod DisplayMessage(message As %String) As %String
{
    Set safe = $REPLACE(message, "<", "&lt;")
    Set safe = $REPLACE(safe, ">", "&gt;")
    Set safe = $REPLACE(safe, "&", "&amp;")
    Return "<div>" _ safe _ "</div>"
}
```

BUG PATTERN: Wrong escaping order causes double-encoding.
ROOT CAUSE: Escaping `<` first produces `&lt;`. Then escaping `&` turns `&lt;` into
`&amp;lt;` — the ampersand in the entity gets escaped again.

FIXED CODE:
```objectscript
ClassMethod DisplayMessage(message As %String) As %String
{
    Set safe = $REPLACE(message, "&", "&amp;")
    Set safe = $REPLACE(safe, "<", "&lt;")
    Set safe = $REPLACE(safe, ">", "&gt;")
    Return "<div>" _ safe _ "</div>"
}
```

KEY RULE: Always escape `&` FIRST, then `<`, then `>`.

---

## EXAMPLE 3: SQL Missing Date Filter

BUGGY CODE:
```objectscript
ClassMethod SearchItems(category As %String) As %ListOfDataTypes
{
    Set results = ##class(%ListOfDataTypes).%New()
    Set stmt = ##class(%SQL.Statement).%New()
    Set sc = stmt.%Prepare("SELECT Name FROM Catalog.Item WHERE Category = ?")
    Set rs = stmt.%Execute(category)
    While rs.%Next() { Do results.Insert(rs.%Get("Name")) }
    Return results
}
```

BUG PATTERN: Query missing filter to exclude expired records.
ROOT CAUSE: No `ActiveUntil` date check — returns both active and expired items.
`ActiveUntil = ""` (NULL) means never expires; `ActiveUntil < today` means expired.

FIXED CODE:
```objectscript
ClassMethod SearchItems(category As %String) As %ListOfDataTypes
{
    Set results = ##class(%ListOfDataTypes).%New()
    Set stmt = ##class(%SQL.Statement).%New()
    Set sc = stmt.%Prepare("SELECT Name FROM Catalog.Item WHERE Category = ? AND (ActiveUntil IS NULL OR ActiveUntil >= ?)")
    Set rs = stmt.%Execute(category, +$HOROLOG)
    While rs.%Next() { Do results.Insert(rs.%Get("Name")) }
    Return results
}
```

KEY RULE: `+$HOROLOG` = today as integer. Pattern: `AND (field IS NULL OR field >= ?)`.

---

## EXAMPLE 4: Forward Iteration With Removal Skips Items

BUGGY CODE:
```objectscript
ClassMethod ProcessItems(items As %ListOfDataTypes) As %Integer
{
    For i=1:1:items.Count() {
        If (items.GetAt(i) [ "DELETE") { Do items.RemoveAt(i) }
    }
    Return items.Count()
}
```

BUG PATTERN: Removing items during forward iteration causes index skipping.
ROOT CAUSE: After `RemoveAt(2)`, the old item 3 becomes item 2 — but `i` advances to 3,
skipping the newly-shifted item.

FIXED CODE:
```objectscript
ClassMethod ProcessItems(items As %ListOfDataTypes) As %Integer
{
    For i=items.Count():-1:1 {
        If (items.GetAt(i) [ "DELETE") { Do items.RemoveAt(i) }
    }
    Return items.Count()
}
```

KEY RULE: Iterate backwards (`Count():-1:1`) when removing items in-place.

---

## EXAMPLE 5: Missing $IsObject Check After %OpenId

BUGGY CODE:
```objectscript
ClassMethod GetPatientAge(id As %String) As %Integer
{
    Set patient = ##class(Patient).%OpenId(id)
    Set birthYear = $PIECE(patient.DOB, "-", 1)
    Return +$EXTRACT($ZDATE($HOROLOG, 3), 1, 4) - birthYear
}
```

BUG PATTERN: No null check after `%OpenId` — crashes when record not found.
ROOT CAUSE: `%OpenId` returns `""` when ID doesn't exist. Accessing `.DOB` on `""`
causes `<INVALID OREF>` runtime error.

FIXED CODE:
```objectscript
ClassMethod GetPatientAge(id As %String) As %Integer
{
    Set patient = ##class(Patient).%OpenId(id)
    If '$IsObject(patient) { Return -1 }
    Set birthYear = $PIECE(patient.DOB, "-", 1)
    Return +$EXTRACT($ZDATE($HOROLOG, 3), 1, 4) - birthYear
}
```

KEY RULE: Always `If '$IsObject(obj) { Return <default> }` immediately after `%OpenId`.

---

## EXAMPLE 6: Postfix Quit Syntax Error

BUGGY CODE (causes #5559 parse error):
```objectscript
For {
    Set key = $ORDER(users(key))
    Quit:key = ""
    If ($ZCONVERT(key, "U") = searchName) { Return users(key) }
}
```

BUG PATTERN: Spaces in postfix condition cause parse error.
ROOT CAUSE: `Quit:key = ""` — the space before `=` causes IRIS UDL parser error #5559.
Postfix conditions must be written without any spaces.

FIXED CODE:
```objectscript
For {
    Set key = $ORDER(users(key))
    Quit:key=""
    If ($ZCONVERT(key, "U") = searchName) { Return users(key) }
}
```

KEY RULE: Postfix conditions: `Quit:condition` — NO spaces anywhere in the condition.
`Quit:key=""` not `Quit:key = ""`. Also never append postfix to other statements.

---

## EXAMPLE 7: CSV Building — O(n²) String Concat

BUGGY CODE:
```objectscript
ClassMethod BuildCSV(values As %ListOfDataTypes) As %String
{
    Set result = ""
    For i=1:1:values.Count() {
        If (i > 1) { Set result = result _ "," }
        Set result = result _ values.GetAt(i)
    }
    Return result
}
```

BUG PATTERN: O(n²) string concatenation — fails performance test for large lists.
ROOT CAUSE: Each `result _ value` creates a new string copying all previous data.
For 1000 items this copies ~500,000 total characters.

FIXED CODE:
```objectscript
ClassMethod BuildCSV(values As %ListOfDataTypes) As %String
{
    Set lst = ""
    For i=1:1:values.Count() {
        Set lst = lst _ $LISTBUILD(values.GetAt(i))
    }
    Return $LISTTOSTRING(lst, ",")
}
```

KEY RULE: Accumulate into `$LIST` then convert once with `$LISTTOSTRING(lst, ",")`.

---

## HOW TO APPLY: Structured Fix Protocol

For every bug fix request:
1. **State the bug pattern** from the examples above (or identify a new one)
2. **State the root cause** in one sentence
3. **Generate the fix** based on the pattern

This structure is mandatory — stating the pattern before generating code
prevents the most common errors.