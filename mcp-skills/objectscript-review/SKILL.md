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
description: Reviews ObjectScript code for common LLM mistakes before presenting to
  the user
iris_version: '>=2024.1'
name: objectscript-review
pass_rate: 1.0
state: reviewed
tags:
- objectscript
- review
- quality
trigger: After writing any .cls file or ObjectScript code block
---

## Purpose
Automatically confirm generated ObjectScript follows the critical project rules before showing it to the user.

## HARD GATE
Do not show ObjectScript code to the user until this review passes.

## Review Checklist
For each item, check the generated code and flag any violations:

- [ ] **QUIT/RETURN**: No `Quit <value>` inside TRY/CATCH or loops
- [ ] **Method calls**: Intra-class calls use `..MethodName()` syntax
- [ ] **Error handling**: Uses `$$$ThrowOnError` / `$$$ISERR` macros, not raw status checks
- [ ] **THROW**: Never throws a `%Status` directly — uses `%Exception` objects
- [ ] **Precedence**: Complex arithmetic has explicit parentheses
- [ ] **Transactions**: TRollback checks `$TLevel > 0` first
- [ ] **NEW**: No `New` command inside method/procedure blocks
- [ ] **%TimeStamp**: Uses `YYYY-MM-DD HH:MM:SS` format, not ISO 8601 with `T`
- [ ] **%Status returns**: Methods returning %Status use `$$$OK` and check with `$$$ISOK`/`$$$ISERR`
- [ ] **Globals**: No temporary data stored in globals when locals suffice

## Output Format

If violations found:
> ⚠️ ObjectScript review flagged [N] issues — correcting before showing:
> - [rule]: [what was wrong] → [correct pattern]

Then show the corrected code.

If clean:
> ✅ ObjectScript review passed.

Then show the code.