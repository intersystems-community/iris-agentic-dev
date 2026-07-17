# AGENTS for ObjectScript AI Coding

Drop this file in your repo root (or `.claude/AGENTS.md`) so AI coding agents understand
ObjectScript semantics before writing a single line of code.

> **Primary workflow**: Most ISC SEs and developers use VS Code with the [ObjectScript MCP extension](https://github.com/intersystems-community/vscode-objectscript-mcp) — it wires `iris_compile`, `iris_test`, `docs_introspect`, and `interop_*` tools directly into Copilot agent mode using your existing `objectscript.conn` settings. This AGENTS.md covers the ObjectScript rules those tools operate under.

> **Benchmark result**: Claude Sonnet 4.6 with this file scores **86% on a 22-task ObjectScript repair suite** (+14% lift over no context). The `objectscript-review` skill below raises that to **100%** (+29% lift).

---

## HARD GATE — Run before showing any ObjectScript code

Load `skills/objectscript-review/SKILL.md` if available. Otherwise apply this checklist mentally:

- [ ] No `Quit <value>` inside For/While loops — use `Return <value>`
- [ ] Postfix condition has no spaces: `Quit:key=""` not `Quit:key = ""`
- [ ] `$IsObject(obj)` checked after every `%OpenId` before accessing properties
- [ ] SQL table name: `MF.Catalog.Product` → `MF_Catalog.Product` (last dot = schema separator)
- [ ] `SQLCODE = 0` is success (falsy) — check `SQLCODE = 0`, not just `SQLCODE`
- [ ] HTML escaping: `&` FIRST, then `<`, then `>`
- [ ] Arithmetic is left-to-right: use `1.8` not `9/5`, parenthesize everything
- [ ] `$ListBuild()` creates a list of length 1, not 0 — use `""` for empty list
- [ ] `%Status`: use `$$$ISERR(sc)` — never return `$$$OK` after catching an error
- [ ] Transactions: `If $TLevel > 0 { TRollback }` — never `Return` inside `TStart` without rollback

---

## 1. ObjectScript Language Rules (LLM Gotchas)

### Control Flow

1. **`Quit` vs `Return`** — `Quit <value>` is **illegal** inside `TRY/CATCH` or loops. Use `Return <value>` to exit a method with a value; use bare `Quit` only to exit the current loop or `FOR` block. When in doubt, use `Return`.
2. **No operator precedence** — ObjectScript evaluates **strictly left-to-right**. `3+3*2 = 12`, not `9`. Always use parentheses for compound expressions.
3. **No `finally`, single `Catch`** — `TRY/CATCH` has exactly one `Catch` block and no `finally`. Cleanup goes after the block. Differentiate exception types with `ex.%IsA("...")`.

### Methods & Variables

1. **Intra-class method calls require `..`** — Inside a method, call `Do ..MyMethod()` not `Do MyMethod()`. `##class(Same.Class).MyMethod()` also works but `..` is idiomatic for same-class calls.
2. **`NEW` is illegal inside methods** — Never use `New varname` inside a method body; method/procedure blocks are already isolated in scope.
3. **Instance variables** — `i%PropertyName` accesses the raw slot directly; `..PropertyName` goes through the accessor. Prefer `..PropertyName` unless you have a specific reason not to.

### Error Handling

1. **`%Status` return convention** — Methods that can fail return `%Status`. Check with `$$$ISOK(sc)` / `$$$ISERR(sc)`. Return `$$$OK` on success. Never compare `If sc=0`; always use the macros.
2. **Throwing and catching** — Use `$$$ThrowOnError(sc)` to throw on failure. Never `Throw sc` directly — `THROW` expects a `%Exception.AbstractException`. Correct pattern:

   ```objectscript
   Try {
       $$$ThrowOnError(..DoSomething())
   } Catch ex {
       Set sc = ex.AsStatus()
   }
   ```

3. **Transaction discipline** — Always check `$TLEVEL` before rolling back. Standard pattern:

   ```objectscript
   TStart
   Try {
       // work
       TCommit
   } Catch ex {
       If $TLevel > 0 TRollback
       Set sc = ex.AsStatus()
   }
   ```

### Types & Formats

1. **`%TimeStamp` format** — `%TimeStamp` uses `YYYY-MM-DD HH:MM:SS` (a space, not `T`). **Not** ISO 8601 with `T`. This is the most common AI mistake. Always use space-separated format for any IRIS date/time literal.
2. **String concatenation** — Use `_` to concatenate strings: `"Hello" _ " " _ name`. There is no `+` for strings.
3. **Globals vs locals** — `^GlobalName` is database-persistent and shared across processes. Local variables (`var`) are process-scoped and temporary. Never use globals as temporary variables.
4. **`$LISTNEXT` for list iteration** — To iterate a `%List`, use `$LISTNEXT(list, ptr, value)` with `Set ptr=0` before the loop. Do not use `FOR i=1:1:$LISTLENGTH(list)` — it is slower and error-prone for embedded lists.

---

## 2. Compile & Test Loop

**If the objectscript MCP server is connected (check `/mcp`), always use MCP tools first. Fall back to bash only if the MCP is unavailable.**

### Via MCP tools (preferred)

```
# Find classes by name pattern — live IRIS namespace
iris_symbols(query="MyPackage.*")
iris_symbols(query="%ASQ*")          # system classes
iris_symbols_local()                 # parse .cls files on disk, no IRIS needed

# Read a full class definition from IRIS (methods, parameters, inheritance)
docs_introspect(class_name="MyPackage.MyClass")
docs_introspect(class_name="%ASQ.Engine")   # works on system classes too

# Compile a .cls file
iris_compile(target="MyPackage/MyClass.cls", namespace="USER")

# Run %UnitTest tests
iris_test(pattern="MyPackage.Tests.*")

# If IRIS is unreachable — list containers and pick the right one
iris_list_containers()
iris_select_container(name="arno_iris_test")   # reconnects without restart

# Read / write / delete IRIS global variables (Merged tier only, requires HTTP connection)
iris_global(action="get", global_name="MyApp", subscripts=["key1"])
iris_global(action="get", global_name="MyApp", subtree=true, max_nodes=200)
iris_global(action="set", global_name="MyApp", subscripts=["key1"], value="hello")
iris_global(action="kill", global_name="MyApp", subscripts=["key1"])
iris_global(action="list", global_name="MyApp", max_subscripts=50)
# PHI globals require explicit acknowledgement:
iris_global(action="get", global_name="PAPMI", subscripts=["123"], acknowledgePhi=true)

# Read a specific line range from a large class (mode=fragment — no full fetch needed)
iris_doc(mode="fragment", name="MyApp.BigClass.cls", start=50, end=70)

# Fetch the compiled INT representation (for macro expansion / stack trace correlation)
iris_doc(mode="compiled", name="MyApp.Patient.cls")
iris_doc(mode="compiled", name="MyRoutine.mac")

# Enumerate documents in a package (mode=list — metadata only, no source fetch)
iris_doc(mode="list", pattern="MyApp.*", category="CLS", max_results=50)
iris_doc(mode="list", pattern="MyApp.*")          # category defaults to ALL
iris_doc(mode="list", pattern="%Library.*", category="CLS", max_results=5)

# Invoke a ClassMethod directly (Merged tier only, Execute-gated: blocked on live/test)
iris_execute_method(class="%SYSTEM.Version", method="GetVersion")
iris_execute_method(class="MyApp.Utils", method="FormatDate", args=["2026-01-01"])
iris_execute_method(class="%Library.Integer", method="IsValid", args=["42"])
```

**Do NOT use `docker exec` / `docker cp` / `iris session` bash commands when the MCP is connected.** The MCP handles container targeting automatically after `iris_select_container`.

### iris_admin — IRIS administration dispatcher

All administration actions go through a single `iris_admin(action=..., ...)` dispatcher. Runs in `%SYS` namespace.

**Read-only (always available, all mcpTemplates):**

```
iris_admin(action="list_namespaces")
iris_admin(action="list_databases")
iris_admin(action="list_users")
iris_admin(action="list_roles")
iris_admin(action="list_user_roles", username="jdoe")
iris_admin(action="check_permission", resource="MyApp.Service", permission="USE")
iris_admin(action="list_webapps")
iris_admin(action="get_webapp", name="/myapp")

# Observability (055-system-observability) — read-only, always permitted
iris_admin(action="view_locks")                                  # active lock table
iris_admin(action="view_processes", dataPolicy="allow")          # running PIDs (PHI-capable; block/redact/allow)
iris_admin(action="view_processes", dataPolicy="redact")         # username/client redacted
iris_admin(action="journal_search",                              # journal records; dataPolicy=allow required
    dataPolicy="allow", global_pattern="IrisDevTest.*", max_records=100)
iris_admin(action="journal_search",
    dataPolicy="allow", time_range={"from": "2026-06-01T00:00:00Z", "to": "2026-06-30T00:00:00Z"})
iris_admin(action="namespace_mappings", namespace="USER")        # glob/package/routine mappings for a namespace
iris_admin(action="database_status")                             # all mounted databases + free space
iris_admin(action="database_status", name="USER")                # single database
```

**Write (blocked on `mcpTemplate=live/test`):**

```
iris_admin(action="create_user", username="bob", password="...", roles="Developer")
iris_admin(action="update_user", username="bob", enabled=false)
iris_admin(action="delete_user", username="bob")
iris_admin(action="create_namespace", name="MYNS", code_database="MYNS", data_database="MYNS")
iris_admin(action="delete_namespace", name="MYNS")
iris_admin(action="create_webapp", name="/myapp", namespace="USER")
iris_admin(action="delete_webapp", name="/myapp")
```

**`dataPolicy` for view_processes / journal_search:**

- `"block"` (default) — returns `DATA_POLICY_BLOCKED` immediately
- `"redact"` — returns rows with `username`, `client_node_name`, `client_ip` replaced by `"[REDACTED]"`
- `"allow"` — returns full data; `journal_search` requires this value; set in connection policy

### Interoperability depth tools (056-interop-depth) — Merged tier, Query category

```
# Read a message body by ID — handles plain-text and stream-backed bodies
iris_message_body(message_id="12345", dataPolicy="allow", acknowledgePhi=true)
iris_message_body(message_id="12345", dataPolicy="redact")   # HL7 PID/MSH fields scrubbed
iris_message_body(message_id="12345", max_bytes=1048576)     # clamped to 1MB max

# List or inspect Ensemble business rules (Ens.Rule.RuleSet)
iris_business_rule_info(action="list")
iris_business_rule_info(action="get", rule_name="MyApp.RoutingRule")

# Diff the running production against the last source-controlled version
iris_production_diff()                              # uses the currently running production
iris_production_diff(production="MyApp.Production")
```

**`dataPolicy` for `iris_message_body`** (message bodies may contain PHI):

- `"block"` (default) — returns `PHI_POLICY_BLOCKED` immediately, regardless of content
- `"allow"` — returns full body; requires `acknowledgePhi=true` or returns `PHI_ACK_REQUIRED`
- `"redact"` — HL7 v2 PID-3/5/7/8/11/18 and MSH-3 fields replaced with `"[REDACTED]"`

`iris_business_rule_info` and `iris_production_diff` are not PHI-gated (rule/config metadata only).

### iris_query modes (057-sql-power)

`iris_query` accepts a `mode` param: `"read"` (default), `"explain"`, `"count"`, `"write"`.

```
# explain — query plan inspection, read-only, permitted on all mcpTemplate values
iris_query(mode="explain", query="SELECT * FROM Sample.Person WHERE Age > 30")
# -> {success, plan_text, query_hash}

# count — row count without transferring rows
iris_query(mode="count", table="Sample.Person")
iris_query(mode="count", query="SELECT * FROM Sample.Person WHERE Age > 30")
# -> {success, count}

# write — DML execution (Execute-gated: blocked on mcpTemplate=live/test)
iris_query(mode="write", query="INSERT INTO MyApp.Log (Msg) VALUES ('hello')")
iris_query(mode="write", query="UPDATE MyApp.Log SET Msg='x' WHERE Id=1")
iris_query(mode="write", query="TRUNCATE TABLE MyApp.Scratch")
iris_query(mode="write", query="UPDATE MyApp.Log SET Msg='x'", max_rows_affected=5000)
# -> {success, rows_affected} or {error_code: "ROWS_LIMIT_EXCEEDED", actual_count, limit}
```

`mode="write"` allows INSERT/UPDATE/DELETE/CALL/TRUNCATE; DDL (CREATE/DROP/ALTER/GRANT/REVOKE)
and SELECT are rejected before any IRIS call. UPDATE/DELETE are pre-checked against
`max_rows_affected` (default 1000, clamp range 1–10000) to prevent accidental mass updates.

### Reading class source from IRIS (INT / system classes)

To read the source of any class — including system classes like `%ASQ.Engine` that have no `.cls` on disk:

```
# Option 1 (preferred): docs_introspect — returns parsed method signatures
docs_introspect(class_name="%ASQ.Engine")

# Option 2: iris_doc fragment — read a specific line range without full fetch
iris_doc(mode="fragment", name="%ASQ.Engine.cls", start=1, end=50)

# Option 3: iris_doc compiled — fetch the compiled INT (for macro expansion / stack traces)
iris_doc(mode="compiled", name="%ASQ.Engine.cls")

# Option 4: export via OBJ then read — use when you need full source with macros
# Run in bash:
docker exec <container> iris session IRIS -U USER \
  "set sc = \$system.OBJ.ExportUDL(\"%ASQ.Engine.cls\",\"/tmp/ASQEngine.cls\") halt"
docker cp <container>:/tmp/ASQEngine.cls /tmp/ASQEngine.cls
# Then use Read tool on /tmp/ASQEngine.cls
```

### Via bash (fallback only — when MCP is unavailable)

```bash
# Compile a single class
iris session IRIS -U USER "Do \$System.OBJ.Load(\"MyPackage/MyClass.cls\",\"ck\")"

# Run %UnitTest tests
iris session IRIS -U USER "Do ##class(%UnitTest.Manager).RunTest(\"MyPackage.Tests\",,\"/nodelete\")"
```

### Reading compile errors

IRIS compiler errors look like:

```
ERROR #5659: Method 'Foo' in class 'My.Class' has a 'Return' that does not match the return type
ERROR #5002: ObjectScript error in method 'Bar' in class 'My.Class'  <UNDEFINED>var+3^My.Class.1
```

- The `+3^My.Class.1` suffix means line 3 of the compiled `.INT` routine — map back to your `.cls` source.
- `<UNDEFINED>` means a variable was used before being set.
- `<NOLINE>` at compile time usually means a syntax error above the reported line.

---

## 3. Class Structure Templates

### Standard class with %Status error handling

```objectscript
Class MyPackage.MyClass Extends %RegisteredObject
{

/// Brief description of what this method does.
ClassMethod MyMethod(pArg As %String) As %Status
{
    Set sc = $$$OK
    Try {
        // implementation
        $$$ThrowOnError(..HelperMethod(pArg))
    } Catch ex {
        Set sc = ex.AsStatus()
    }
    Return sc
}

/// Returns a value or throws.
ClassMethod GetValue(pKey As %String) As %String
{
    Set val = $Get(^MyGlobal(pKey))
    If val = "" $$$ThrowStatus($$$ERROR($$$GeneralError, "Key not found: " _ pKey))
    Return val
}

}
```

### Persistent class

```objectscript
Class MyPackage.MyRecord Extends %Persistent
{

Property Name As %String(MAXLEN = 255);
Property CreatedAt As %TimeStamp;  // stored as YYYY-MM-DD HH:MM:SS

Index NameIdx On Name;

ClassMethod FindByName(pName As %String) As MyPackage.MyRecord
{
    Return ##class(MyPackage.MyRecord).NameIndexOpen(pName)
}

}
```

### %UnitTest test class

```objectscript
Class MyPackage.Tests.MyClassTest Extends %UnitTest.TestCase
{

Method TestBasicCase()
{
    Set result = ##class(MyPackage.MyClass).MyMethod("input")
    Do $$$AssertStatusOK(result)
}

Method TestEdgeCase()
{
    // Test that invalid input returns an error status
    Set result = ##class(MyPackage.MyClass).MyMethod("")
    Do $$$AssertStatusNotOK(result)
}

}
```

---

## 4. Legacy .MAC Routines

**Most IRIS codebases — especially CHUI apps, integrations, and anything pre-2000 — are `.MAC` routines, not classes.** AI models default to class syntax. If you're working with `.MAC`, tell your agent explicitly and use these rules.

### Structure

`.MAC` routines use label-based structure, not class methods:

```objectscript
MYROUTINE
    ; Entry point — no parentheses, no braces
    Set x = 1
    Do HELPER
    Quit

HELPER
    ; Subroutine — called with DO HELPER or DO HELPER^MYROUTINE
    Write "hello", !
    Quit

CALC(a, b)
    ; Extrinsic function — called with $$CALC(1,2) or $$CALC^MYROUTINE(1,2)
    Quit a + b
```

### `#include` vs `Include` — different syntax than classes

```objectscript
; .MAC uses preprocessor directive — NOT the class keyword
#include %occStatus
#include myMacros

; In a class you'd write:
; Include %occStatus   ← class keyword, no #
; But in .MAC it MUST be:
; #include %occStatus  ← preprocessor directive, with #
```

### Calling conventions

```objectscript
; Call a subroutine in same routine:
Do LABEL

; Call a subroutine in another routine:
Do LABEL^OTHERROUTINE

; Call an extrinsic function (returns a value):
Set result = $$LABEL
Set result = $$LABEL^OTHERROUTINE(arg1, arg2)

; WRONG in .MAC — no class method syntax:
Set result = ..MyMethod()          ; ERROR — no object context in .MAC
Set result = ##class(X).Method()   ; Valid but uncommon in legacy .MAC
```

### Error handling — `$ZTRAP` not `Try/Catch`

Legacy `.MAC` uses `$ZTRAP` label-based error handling, not `Try/Catch`:

```objectscript
MYROUTINE
    Set $ZTRAP = "ERRHANDLER"
    ; ... code that might error ...
    Set $ZTRAP = ""
    Quit

ERRHANDLER
    Set $ZTRAP = ""        ; Clear trap to avoid recursion
    Write "Error: ", $ZE, !
    Quit
```

Modern `.MAC` can use `Try/Catch` — prefer it for new code even in `.MAC` files. Do not introduce `$ZTRAP` in new code.

### Variable scope — routines have no automatic isolation

```objectscript
; WRONG: variable bleeds across DO calls unless you NEW it
CALLER
    Set x = "original"
    Do CALLEE
    Write x, !    ; Might print "modified" — x was changed in CALLEE!

CALLEE
    Set x = "modified"
    Quit

; CORRECT: NEW isolates the variable
CALLEE
    New x
    Set x = "modified"
    Quit
```

### Key differences from classes

| .MAC routines              | ObjectScript classes          |
| -------------------------- | ----------------------------- |
| `#include file.inc`        | `Include ClassName`           |
| `Do LABEL^ROUTINE`         | `Do ##class(X).Method()`      |
| `$$FUNC^ROUTINE(args)`     | `##class(X).Method(args)`     |
| `$ZTRAP = "LABEL"`         | `Try { } Catch e { }`         |
| `New var` for scope        | Method variables auto-scoped  |
| No `..` for self-reference | `..Property`, `..Method()`    |
| Globals for shared state   | Properties for instance state |

---

## 5. Namespace & Environment Awareness

- **Always ask which namespace**
- **`%SYS` is privileged** — system-level operations (user management, license info) require `%SYS`. Don't put application code there.
- **IRIS web port ≠ superserver port** — The Atelier/REST web server listens on `52773` by default (or a Docker-mapped port). The superserver (JDBC/DBAPI) is on `1972`. These are different. **VSCode ObjectScript extension requires port 52773.**
- **Enterprise images have no web server** — `containers.intersystems.com/intersystems/iris:*` (enterprise) and `irishealth:2026.2.0AI.*` ship with `WebServer=0`. Port 52773 does not exist. A webgateway cannot substitute. For VSCode/Atelier, use `intersystemsdc/iris-community:*` (same ObjectScript/SQL/globals, same version). Load `iris-vscode-objectscript` skill for setup.
- **Check namespace before class search** — `Do $System.Status.DisplayError(##class(%Dictionary.ClassDefinition).%OpenId("My.Class"))` returning an error likely means you're in the wrong namespace, not that the class doesn't exist.

---

## 6. MCP Tool Error Code Reference

Error codes returned in the `error_code` field of tool responses. All follow `SCREAMING_SNAKE_CASE`.

| Error Code                    | Meaning                                                                                          | Key Fields                                                   |
| ----------------------------- | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------ |
| `IRIS_UNREACHABLE`            | Cannot reach IRIS — network error, wrong port, container down                                    | `attempted_url`                                              |
| `AUTH_ERROR`                  | HTTP 401/403 — wrong credentials or insufficient privilege                                       | —                                                            |
| `COMPILE_ERROR`               | Class/routine failed to compile                                                                  | `errors[]`, `open_uri`                                       |
| `TIMEOUT`                     | Operation exceeded configured timeout                                                            | —                                                            |
| `DOCKER_REQUIRED`             | Operation needs docker exec but `IRIS_CONTAINER` not set                                         | —                                                            |
| `IRIS_NAMESPACE_NOT_FOUND`    | Namespace does not exist on this IRIS instance                                                   | `namespace`                                                  |
| `NO_TESTS_FOUND`              | Test pattern matched no test classes                                                             | `pattern`                                                    |
| `POLICY_GATE`                 | Tool not in the connection's `allow` category list                                               | `server_name`, `allowed_categories`                          |
| `ENV_GATE_BLOCKED`            | `mcpTemplate` for this server blocks the tool's category                                         | `server_name`, `template`, `blocked_category`, `remediation` |
| `DATA_POLICY_BLOCKED`         | `dataPolicy=block` (default) prevents bulk-PHI tool                                              | `tool`, `policy`, `remediation`                              |
| `SYSTEM_BLOCKLIST`            | Global name matches system or per-connection blocklist                                           | `global_name`, `matched_pattern`                             |
| `PHI_GATE_BLOCKED`            | Global name matches PHI name pattern; pass `acknowledgePhi=true` to proceed                      | `global_name`, `matched_pattern`, `remediation`              |
| `INVALID_SUBSCRIPT`           | `iris_global` subscript contains disallowed characters (allowed: `a-z A-Z 0-9 space . _ : -`)    | `subscript`, `pattern`                                       |
| `INVALID_ACTION`              | `iris_global` action not one of `get`, `set`, `kill`, `list`                                     | `action`                                                     |
| `INVALID_PARAMS`              | Required parameter missing (e.g. `action=set` without `value`)                                   | —                                                            |
| `READ_ERROR`                  | Could not read local source file                                                                 | `path`                                                       |
| `UPLOAD_FAILED`               | Atelier PUT rejected the document                                                                | `document`, `http_status`                                    |
| `CONTAINER_NOT_FOUND`         | Named container not running in Docker                                                            | `container`                                                  |
| `CONTAINER_UNREACHABLE`       | Container found but Atelier HTTP probe failed                                                    | `container`, `port`                                          |
| `MISSING_PARAMS`              | Required parameter absent — e.g. `journal_search` with neither `global_pattern` nor `time_range` | —                                                            |
| `NAMESPACE_NOT_FOUND`         | Requested namespace does not exist on this IRIS instance                                         | `namespace`                                                  |
| `DATABASE_NOT_FOUND`          | Requested database name not found                                                                | `name`                                                       |
| `MESSAGE_NOT_FOUND`           | `iris_message_body` — no body record for the given `message_id`                                  | `message_id`                                                 |
| `PHI_ACK_REQUIRED`            | `iris_message_body` with `dataPolicy=allow` and no `acknowledgePhi=true`                         | —                                                            |
| `INVALID_MESSAGE_ID`          | `iris_message_body` — `message_id` is not a valid integer                                        | `message_id`                                                 |
| `STREAM_READ_ERROR`           | `iris_message_body` — stream-backed body could not be read                                       | —                                                            |
| `UNSUPPORTED_BODY_CLASS`      | `iris_message_body` — body class is not a recognized stream/text type                            | `body_class`                                                 |
| `RULE_NOT_FOUND`              | `iris_business_rule_info action=get` — no rule set with the given `rule_name`                    | `rule_name`                                                  |
| `INTEROP_NOT_AVAILABLE`       | Ensemble/Interoperability is not installed in this namespace                                     | —                                                            |
| `NO_SCM`                      | `iris_production_diff` — no source control configured for this production                        | —                                                            |
| `NO_SCM_VERSION`              | `iris_production_diff` — SCM configured but no committed version yet                             | —                                                            |
| `PRODUCTION_NOT_FOUND`        | `iris_production_diff` — named production does not exist                                         | `production`                                                 |
| `NO_PRODUCTION`               | `iris_production_diff` — no production currently running and none named                          | —                                                            |
| `EXPLAIN_REQUIRES_SELECT`     | `iris_query mode=explain` — query is not a SELECT/WITH statement                                 | —                                                            |
| `EXPLAIN_NOT_SUPPORTED`       | `iris_query mode=explain` — IRIS returned no plan text for this query/version                    | —                                                            |
| `MISSING_TARGET`              | `iris_query mode=count` — neither `table` nor `query` provided                                   | —                                                            |
| `DDL_NOT_ALLOWED`             | `iris_query mode=write` — statement is DDL (CREATE/DROP/ALTER/GRANT/REVOKE) or unrecognized      | `blocked_keyword`                                            |
| `SELECT_NOT_ALLOWED_IN_WRITE` | `iris_query mode=write` — statement is a SELECT; write mode is DML-only                          | —                                                            |
| `ROWS_LIMIT_EXCEEDED`         | `iris_query mode=write` — UPDATE/DELETE would affect more rows than `max_rows_affected`          | `actual_count`, `limit`                                      |
| `ROWS_CHECK_FAILED`           | `iris_query mode=write` — the pre-execution row-count check itself failed                        | —                                                            |

**PHI gate bypass** — for `PHI_GATE_BLOCKED`, add `"acknowledgePhi": true` to the tool call params. This only works for per-global name checks; `DATA_POLICY_BLOCKED` (bulk-PHI tools like `journal_search`) cannot be bypassed with `acknowledgePhi`.

---

## 7. Using AI Skills (no MCP server required)

The `skills/` directory contains standalone skills you can use with Claude Code,
opencode, or any agent that supports markdown skill files:

| Skill           | What it does                                                                                                                                                  |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `introspect.md` | Fetches a class definition from IRIS via Atelier REST — gives the AI full method signatures, parameters, and return types for any class in your IRIS instance |
| `compile.md`    | Compiles a class via Atelier REST and returns structured error output for the AI to fix                                                                       |

Copy them to `.claude/skills/` or `.opencode/skills/` in your repo. Then invoke:

- `/introspect MyPackage.MyClass` — before editing any class you haven't written
- `/compile MyPackage.MyClass` — after every edit, before declaring done

**These skills require only `curl` and a running IRIS web server** — no Python, no pip installs.

Set these env vars (or substitute directly):

```bash
export IRIS_HOST=localhost
export IRIS_WEB_PORT=52773
export IRIS_USER=_SYSTEM
export IRIS_PASS=SYS
export IRIS_NS=USER
```
