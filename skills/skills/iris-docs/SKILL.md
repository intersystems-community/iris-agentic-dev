---
name: iris-docs
description: Fetch InterSystems IRIS class reference documentation for a specific class and version. Use when implementing any ObjectScript API to verify method signatures, return types, and parameters exist before writing code. Eliminates hallucinated methods. Also covers SQL tables, CSP APIs, and the InterSystems docbook.
tags:
  - iris
  - objectscript
  - documentation
  - verification
author: tdyar
state: reviewed
---

# iris-docs — IRIS Class Reference Lookup

Fetch live IRIS documentation to verify class method signatures before writing ObjectScript code.
**Prevents hallucinated APIs.** Always use this before implementing any IRIS class method you
haven't personally verified in this session.

## When to use

- Before implementing any ObjectScript class method you're unsure about
- When research gives you a method name but not the exact signature
- When you get `<METHOD DOES NOT EXIST>` and need to find the real method name
- When you need to know what SQL tables/views exist in a namespace
- When checking which IRIS version introduced a feature

---

## URL Pattern

```
https://docs.intersystems.com/iris{VERSION}/csp/documatic/%25CSP.Documatic.cls?LIBRARY={LIBRARY}&CLASSNAME={CLASS}
```

### Version codes

| IRIS version | URL code |
|---|---|
| 2026.1 | `iris20261` |
| 2025.1 | `iris20251` |
| 2024.1 | `iris20241` |
| 2023.1 | `iris20231` |
| 2022.1 | `iris20221` |
| Latest stable | `irislatest` |

**Default to `iris20251` unless you know the target version.**

### Library codes

| Library | URL code | Contains |
|---|---|---|
| %SYS (system) | `%25SYS` | Security.*, Config.*, SYS.*, %SYSTEM.*, %SYSTEM.Security |
| USER (any namespace) | `USER` | Application classes |
| ENSLIB | `ENSLIB` | Ens.*, Ensemble/Interoperability classes |
| %SYS (standard) | `%SYS` | (sometimes works; use %25SYS if not) |

**Important**: Ensemble classes (Ens.*) live in `ENSLIB`, NOT `%SYS`. Use `LIBRARY=ENSLIB` for `Ens.Director`, `Ens.Util.LookupTable`, `Ens.Config.Production`, etc.

---

## Usage examples

### Look up a specific class
```
URL: https://docs.intersystems.com/iris20251/csp/documatic/%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=SYS.Database

Prompt: What methods does SYS.Database have for listing databases?
        List all method signatures (name, parameters, return type).
```

### Look up Security.Users
```
URL: https://docs.intersystems.com/iris20251/csp/documatic/%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=Security.Users

Prompt: List all class methods with exact signatures. Which method creates a user?
        Which method deletes a user? Show parameters and return types.
```

### Look up %SYSTEM.Security
```
URL: https://docs.intersystems.com/iris20251/csp/documatic/%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=%25SYSTEM.Security

Prompt: List all methods related to permission checking. Show exact signatures.
```

### Check SQL table availability
```
URL: https://docs.intersystems.com/iris20251/csp/documatic/%25CSP.Documatic.cls?LIBRARY=%25SYS&CLASSNAME=Config.Namespaces

Prompt: Is this class projected as a SQL table? What are the SQL column names?
```

### Docbook for broader topics (not a specific class)
```
URL: https://docs.intersystems.com/iris20251/csp/docbook/DocBook.UI.Page.cls?KEY=GSQL_tables

Prompt: What system tables are available in IRIS SQL?
```

---

## Fetch procedure (use WebFetch tool)

```python
# Step 1: Build the URL
class_name = "SYS.Database"
version = "iris20251"
library = "%25SYS"  # URL-encoded %SYS
url = f"https://docs.intersystems.com/{version}/csp/documatic/%25CSP.Documatic.cls?LIBRARY={library}&CLASSNAME={class_name}"

# Step 2: Fetch with specific prompt
prompt = f"""List ALL class methods for {class_name} with exact signatures:
- Method name
- Parameters (name, type, default if any)
- Return type
- Brief description if shown
Flag any methods related to: listing, creating, deleting, modifying instances."""
```

---

## Interpreting results

- **ClassMethod** = static method, call as `##class(ClassName).MethodName()`
- **Method** = instance method, call on an object reference
- **%Status return** = check with `$$$ISERR(sc)` or `$$$ISOK(sc)`
- **Query** = use `##class(%ResultSet).%New("ClassName:QueryName")` or `%SQL.Statement`
- **Property** = accessed on object instances, not via class methods

---

## Known gotchas (verified against IRIS 2025.1)

- `%SYSTEM.Security.Check(resource, permission)` — checks CURRENT user. NOT `CheckPermission()` (doesn't exist)
- `%SYSTEM.Security.CheckUserPermission(username, resource, permission)` — checks ANOTHER user; requires `%Admin_Secure:USE`
- `SYS.Database` has a `List()` Query — use `##class(%ResultSet).%New("SYS.Database:List")`; fields include Directory, Size, MaxSize, Status, Mounted
- `SYS.Database` SQL table does NOT exist; use the ResultSet API
- `Security.Users.Delete(name As %String)` — NOT `%DeleteId()`; takes the username string directly
- `Config.Namespaces.Create(name, .props)` — props array with `Globals`, `Routines`, `Database` keys
- `Ens.Director.GetAutoStart()` — does NOT exist; use `$GET(^Ens.AutoStart)` directly
- `Ens.*` classes are in `ENSLIB` library, NOT `%SYS` — use `LIBRARY=ENSLIB` in URLs
- `Ens.Util.LookupTable.%Export(pFileName, pTableName)` — takes a **file path**, NOT a stream object
- `Ens.Util.LookupTable.%Import(pFileName, pForceTableName, .pCount)` — takes a **file path**, NOT a stream
- Namespace `%SYS` in documatic URLs must be `%25SYS` (URL-encoded percent sign)
- `versioned_ns_url()` in Rust must URL-encode namespace: `urlencoding::encode(namespace)` — `%SYS` → `%25SYS`

---

## Quick reference — most-used admin classes

```
Security.Users      → user CRUD
Security.Roles      → role management  
Security.Applications → web app management
Config.Namespaces   → namespace management
SYS.Database        → database info (use List() query, not SQL)
%SYSTEM.Security    → permission checking (Check method)
Ens.Director        → production lifecycle
Ens.Config.Credentials → Ensemble credentials
Ens.Util.LookupTable → lookup table CRUD
```
