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
description: 'ObjectScript For/While loop patterns, $Order iteration, postfix Quit,
  Return vs Quit. Use when writing loops, iterating globals/collections, or handling
  early exits.

  '
iris_version: '>=2024.1'
name: objectscript-loop-patterns
pass_rate: 0.5294117647058824
state: reviewed
tags:
- objectscript
- loops
trigger: Any ObjectScript code with For, While, $Order, Quit, or loop iteration patterns.
---

# ObjectScript Loop Patterns

## 1. THE GOLDEN RULE: Return vs Quit in Loops

```objectscript
// Quit with value inside a For/While loop → EXITS THE LOOP, not the method
// The method continues after the loop and returns whatever Quit returned... actually ""

// WRONG — Quit 0 inside For exits the loop, method returns "":
ClassMethod IsUnique(name As %String, lst As %ListOfDataTypes) As %Boolean
{
    For i=1:1:lst.Count() {
        If (lst.GetAt(i) = name) {
            Quit 0    // exits loop, NOT method — method returns ""!
        }
    }
    Quit 1            // only reached after loop completes
}

// CORRECT — Return always exits the method:
ClassMethod IsUnique(name As %String, lst As %ListOfDataTypes) As %Boolean
{
    For i=1:1:lst.Count() {
        If (lst.GetAt(i) = name) {
            Return 0   // exits method immediately with 0
        }
    }
    Return 1
}
```

## 2. $Order Loop — Correct Pattern

```objectscript
// Iterate all keys of a local array:
Set key = ""
For {
    Set key = $Order(arr(key))
    Quit:key=""          // ← postfix Quit, ALONE on its own line, NO SPACES around =
    // process arr(key)
}

// WRONG — spaces in postfix condition:
Quit:key = ""            // ← #5559 parse error!

// WRONG — postfix Quit on same line as anything else:
Set key = $Order(arr(key))  Quit:key=""   // ← #5559 parse error!
Return value  Quit:key=""                  // ← #5559 parse error!
```

## 3. Backwards Loop for Safe Removal

```objectscript
// Remove items during iteration — always go backwards:
For i=items.Count():-1:1 {
    If (items.GetAt(i) [ "DELETE") {
        Do items.RemoveAt(i)
    }
}
// items.Count():-1:1 means start=Count(), step=-1, end=1
```

## 4. $Order on Globals

```objectscript
// Forward iteration:
Set key = $Order(^MyGlobal(""))    // "" seed gives FIRST key
While key '= "" {
    // process ^MyGlobal(key)
    Set key = $Order(^MyGlobal(key))
}

// WRONG — no subscript:
Set key = $Order(^MyGlobal)        // ← <FUNCTION> error!

// Reverse iteration:
Set key = $Order(^MyGlobal(""), -1)   // last key first
While key '= "" {
    Set key = $Order(^MyGlobal(key), -1)
}
```

## 5. For Loop Variants

```objectscript
// Count up:
For i=1:1:10 { ... }           // 1,2,3,...,10

// Count down:
For i=10:-1:1 { ... }          // 10,9,8,...,1

// Infinite with explicit Quit:
For {
    // ...
    Quit:condition
}

// Over a comma-separated list (not common but valid):
For i=1,3,7 { write i,! }      // 1, 3, 7
```

## 6. HTML Escaping — Order Matters

When escaping HTML, always escape `&` FIRST:

```objectscript
// WRONG — & escaped last causes double-escaping:
// "<b>A & B</b>" → "&lt;b&gt;A & B&lt;/b&gt;" → "&lt;b&gt;A &amp; B&lt;/b&gt;" ← wrong

// CORRECT — & first:
Set safe = $REPLACE(message, "&", "&amp;")   // 1. ampersands first
Set safe = $REPLACE(safe,    "<", "&lt;")    // 2. then less-than
Set safe = $REPLACE(safe,    ">", "&gt;")    // 3. then greater-than
// optional: Set safe = $REPLACE(safe, """", "&quot;")
```

## 7. For Loop Counter — Never Modify the Loop Variable

```objectscript
// WRONG — modifying i inside the loop causes skipped/repeated iterations:
For i=1:1:items.Count() {
    If condition { Set i = i + 1 }   // skips next item — DON'T DO THIS
}

// CORRECT — use a separate flag or backwards iteration:
Set i = 1
While i <= items.Count() {
    If condition {
        Do items.RemoveAt(i)         // don't increment — list shrunk
    } Else {
        Set i = i + 1
    }
}

// OR — backwards loop (simpler):
For i=items.Count():-1:1 {
    If condition { Do items.RemoveAt(i) }
}

## 8. Global Negative-Key Trick — Descending Sort Without a Sort Step

IRIS globals sort subscripts ascending by default. To iterate in descending order
by a numeric score, store the **negated** value as the subscript key. `$Order` then
visits higher scores first.

```objectscript
// Store: negate the score so high scores sort first
Set ^||TagScores(-score, tag) = ""

// Iterate descending — $Order walks negative numbers most-negative first,
// so -100, -90, -80 ... means scores 100, 90, 80 ...
Set key = ""
For {
    Set key = $Order(^||TagScores(key))
    Quit:key=""
    Set tag = $Order(^||TagScores(key, ""))
    Write "Score: ", (-key), " Tag: ", tag, !
}

// Two-level subscript: ^||Name(-score, tiebreaker) = value
// Lets you sort by score descending, then alphabetically within the same score:
Set ^||Results(-score, name) = data
```

**Why it works**: IRIS collates numeric subscripts in numeric order. Negative numbers
sort before zero, so `-100 < -90 < 0`. Negating the score inverts the order.
No `$SortBegin` / array copy / post-sort needed — the global IS the sorted structure.

**Common uses**: leaderboards, top-N queries, priority queues, ranked results.
```