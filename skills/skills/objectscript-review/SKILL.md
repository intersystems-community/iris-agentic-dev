---
name: objectscript-review
description: Reviews ObjectScript code for common LLM mistakes before presenting to the user
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
