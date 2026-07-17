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
description: Use when writing or reviewing any ObjectScript code. Hard gate — 10-item
  checklist catches the most common AI mistakes before showing code to the user.
iris_version: '>=2024.1'
name: tdyar/objectscript-guardrails
pass_rate: 0.8636363636363636
state: reviewed
tags:
- objectscript
- review
- repair
- core
trigger: Use for tdyar/iris-light-slim
---

# ObjectScript — Hard Gate

**Do not show ObjectScript code until this checklist passes.**

## HARD GATE Checklist

- [ ] **Quit/Return**: No `Quit value` inside For/While/Try — use `Return value`
- [ ] **Postfix syntax**: `Quit:key=""` — NO spaces in condition, alone on its own line
- [ ] **$IsObject**: Check `'$IsObject(obj)` after every `%OpenId` before touching properties
- [ ] **SQL table name**: Last dot = schema separator. `Catalog.Item` → SQL `Catalog.Item` (not `Catalog_Item`)
- [ ] **SQLCODE**: `0` = success (falsy). Check `SQLCODE = 0` not just `SQLCODE`
- [ ] **HTML escaping**: `&` FIRST, then `<`, then `>`
- [ ] **Arithmetic**: Left-to-right, no precedence. Use `1.8` not `9/5`. Parenthesize everything
- [ ] **$ListBuild()**: Empty list is `""` not `$ListBuild()` — `$ListLength($ListBuild()) = 1`
- [ ] **%Status**: Use `$$$ISERR(sc)` / `$$$ThrowOnError(sc)`. Never return `$$$OK` after catching an error
- [ ] **Transactions**: `If $TLevel > 0 { TROLLBACK }` — never `Return` inside TSTART without rollback
- [ ] **Storage blocks**: NEVER write `Storage Default { ... }` in UDL — omit entirely. IRIS auto-generates storage. Writing one causes ERROR #5559 in IRIS 2025.1+.
- [ ] **%INLIST in ObjectScript**: `%INLIST` is SQL-only. In ObjectScript method code use `$ListFind(list, value) > 0`. Writing `Return (x %INLIST list)` causes ERROR #1010.
- [ ] **`'=` in SQL strings**: `'=` is the ObjectScript not-equal operator. Inside SQL string literals, use `<>`. `"WHERE Tags '= ''"` → parser sees `'` as start of SQL string.

## Output Format

If violations found:
> ⚠️ ObjectScript review flagged [N] issues — correcting:
> - [rule]: [wrong] → [correct]

Then show corrected code.

If clean:
> ✅ ObjectScript review passed.

Then show code.

## Quick Reference

```objectscript
// WRONG → CORRECT
Quit 0               (inside For)  →  Return 0
Quit:key = ""                      →  Quit:key=""
..%OpenId(id).Prop                 →  Set obj=..%OpenId(id)  If '$IsObject(obj){...}
SELECT FROM Catalog_Item           →  SELECT FROM Catalog.Item
If SQLCODE { "not found" }         →  If SQLCODE = 100 { "not found" }
celsius * 9 / 5 + 32               →  (celsius * 1.8) + 32
Set lst = $ListBuild()             →  Set lst = ""

// Storage / Operators:
Storage Default { <Type>...</Type> }   →  (omit entirely — IRIS auto-generates)
Return (tag %INLIST myList)            →  Return ($ListFind(myList, tag) > 0)
"WHERE Tags '= ''"                     →  "WHERE Tags <> ''"
```