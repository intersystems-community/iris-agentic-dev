---
name: objectscript-repair
description: Perform coordinated fixes across multiple ObjectScript files. Use when a change in one class requires updates in dependent classes (e.g., method signature changes, renames).
---

# Multi-File Repair Skill

This skill provides a procedure for handling ObjectScript repairs that span multiple files.

## When to use
- Compilation errors in files other than the one being edited.
- Method signature updates where callers need to be updated.
- Renaming properties or classes that are referenced elsewhere.
- Implementing an interface that requires updates to several implementations.

## Procedure

### 1. Dependency Discovery
When a change is made to a class member, identify all call sites before touching any file.
- **Tool**: `docs_introspect(class_name)` — confirms the current method signature and shows the full class definition from `%Dictionary`
- **Tool**: `iris_symbols(query="ClassName.*")` — lists all members of the class
- **Tool**: `Grep` (regex search across `.cls` files) — finds every reference to the old symbol name
- **Requirement**: You MUST perform this check for every public `ClassMethod`, `Method`, or `Property` change before writing any edits.

### 2. Multi-File Planning
Update the repair plan to include all affected files.
- The `target_files` list should contain the root cause file first, followed by all dependent files.
- **Strategy**: Map the change from "Definition" → "Reference" → "Integration".
- Document the full list of files to touch before editing any of them.

### 3. Coordinated Patching
Apply edits to all target files in a single pass.
- Use the `Edit` tool for each file in dependency order (definitions before callers).
- **Validation**: After all edits are written, compile the root file first, then each dependent file.

### 4. Atomic Verification
Always verify the workspace as a whole after the full patch is applied.
- **Compile**: `iris_compile(target="Package/SubPackage/ClassName.cls")` — compile each changed file by path; fix any errors before proceeding to the next file
- **Test**: Run the full test suite related to the change via `iris_test`
- **Revert**: If any file fails to compile or tests regress, revert ALL changes in the iteration using `git checkout -- <file>` (via Bash) before starting the next attempt

## Common Scenarios

### 1. Method Signature Update
**Context**: A method signature is modified (parameters added, removed, or types changed).
**Discovery**:
1. Call `docs_introspect` on the class to capture the current signature.
2. Use `Grep` to find all callers of the method across `.cls` files.
3. List all call sites in the repair plan.
**Transformation**:
- Update all identified call sites to match the new signature.
- Provide default values for newly added parameters if possible.

### 2. Class/Member Rename
**Context**: A class name or a public property/method is renamed.
**Discovery**:
1. Use `Grep` to find all occurrences of the old symbol name across the workspace.
2. Use `iris_symbols` to confirm the old name no longer appears in the live IRIS namespace after renaming.
**Transformation**:
- Rename the definition.
- Rename all occurrences in referencing files.
- Update `##class(OldName)` to `##class(NewName)` everywhere.

### 3. Interface Implementation Sync
**Context**: An abstract class or interface is updated, requiring all subclasses to be updated.
**Discovery**:
1. Use `Grep` to find all classes that `Extends` the modified class.
2. Call `docs_introspect` on each subclass to see which methods they currently implement.
**Transformation**:
- Implement or update the required methods in all subclasses to maintain contract compliance.

## Common Pitfalls
- Forgetting to update a caller in a different package.
- Mismatching parameter counts after a signature change.
- Circular dependencies causing compilation loops — compile in dependency order (base class first).
- Using `iris_compile` without a file path — always pass the `.cls` file path, not a wildcard.
