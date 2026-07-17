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
- jira-018
- jira-019
- jira-020
- jira-021
- jira-056
description: 'ObjectScript %List, %ListOfDataTypes, $LISTBUILD, $LISTTOSTRING, $LISTNEXT
  patterns. Use when building, iterating, merging, or converting lists in ObjectScript.

  '
iris_version: '>=2024.1'
name: objectscript-list-patterns
pass_rate: 0.9090909090909091
state: reviewed
tags:
- objectscript
- list
trigger: Any ObjectScript code involving $LIST, $LISTBUILD, %ListOfDataTypes, CSV
  building, or list accumulation.
---

# ObjectScript List Patterns

## 1. Building a List Incrementally

```objectscript
// WRONG — $LISTBUILD creates a new list each iteration, O(n²):
Set lst = ""
For i=1:1:count {
    Set lst = lst _ $LISTBUILD(values.GetAt(i))
}

// CORRECT — accumulate into a %ListOfDataTypes, convert at end:
Set result = ##class(%ListOfDataTypes).%New()
For i=1:1:values.Count() {
    Do result.Insert(values.GetAt(i))
}

// CORRECT — for CSV output specifically, use $LISTTOSTRING:
Set lst = ""
For i=1:1:values.Count() {
    Set lst = lst _ $LISTBUILD(values.GetAt(i))
}
Set csv = $LISTTOSTRING(lst, ",")
```

## 2. CSV Building — O(n) Pattern

The O(n²) string concat is always wrong for >10 items. Use `$LISTBUILD` accumulation then `$LISTTOSTRING`:

```objectscript
// O(n²) — WRONG for large lists:
Set result = ""
For i=1:1:values.Count() {
    If (i > 1) { Set result = result _ "," }
    Set result = result _ values.GetAt(i)
}

// O(n) — CORRECT:
Set lst = ""
For i=1:1:values.Count() {
    Set lst = lst _ $LISTBUILD(values.GetAt(i))
}
Return $LISTTOSTRING(lst, ",")
```

## 3. Iterating a %List With $LISTNEXT

```objectscript
// Preferred — no index arithmetic, handles empty list correctly:
Set ptr = 0
While $LISTNEXT(lst, ptr, val) {
    // process val
}

// Alternative — $List is 1-based:
For i=1:1:$LISTLENGTH(lst) {
    Set val = $List(lst, i)
    // process val
}
// Note: $LISTLENGTH("") = 0 (truly empty), $LISTLENGTH($LISTBUILD()) = 1 (one empty element!)
// Use "" not $LISTBUILD() to represent an empty list
```

## 4. Deduplication With Local Array (O(1) Lookup)

```objectscript
// WRONG — O(n²), calls Find() for every item:
For i=1:1:list2.Count() {
    Set item = list2.GetAt(i)
    If (result.Find(item) = "") {
        Do result.Insert(item)
    }
}

// CORRECT — O(n) with temp array as hash set:
Set seen = ""
For i=1:1:list1.Count() {
    Set item = list1.GetAt(i)
    Set seen(item) = ""
    Do result.Insert(item)
}
For i=1:1:list2.Count() {
    Set item = list2.GetAt(i)
    If '$Data(seen(item)) {
        Set seen(item) = ""
        Do result.Insert(item)
    }
}
Kill seen
```

## 5. Backwards Iteration for Safe Removal

```objectscript
// WRONG — removing during forward iteration skips items:
For i=1:1:items.Count() {
    If (items.GetAt(i) [ "DELETE") { Do items.RemoveAt(i) }
}

// CORRECT — iterate backwards so indices stay valid:
For i=items.Count():-1:1 {
    If (items.GetAt(i) [ "DELETE") { Do items.RemoveAt(i) }
}
```

## 6. $LISTFROMSTRING vs $LISTBUILD

```objectscript
Set csv = "a,b,c"
Set lst = $LISTFROMSTRING(csv, ",")    // creates %List from delimited string
Set back = $LISTTOSTRING(lst, ",")     // "a,b,c"

Set lst2 = $LISTBUILD("a", "b", "c")  // creates %List from literal values
```

## 7. %ListOfDataTypes vs %List

```objectscript
// %ListOfDataTypes: object wrapper, use Insert/GetAt/Count/Find/RemoveAt
Set lst = ##class(%ListOfDataTypes).%New()
Do lst.Insert("value")
Set val = lst.GetAt(1)            // 1-based
Set n = lst.Count()
Set pos = lst.Find("value")       // returns "" if not found, else position

// %List (raw): use $LISTBUILD/$LIST/$LISTLENGTH/$LISTNEXT
Set lst = $LISTBUILD("a","b")
Set val = $LIST(lst, 1)           // 1-based
Set n = $LISTLENGTH(lst)
```