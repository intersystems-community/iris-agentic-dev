# Tools

Most tools work over the Atelier REST API and connect to any IRIS instance — no Docker
required unless noted. Tools marked ✦ require `IRIS_CONTAINER`. Tools marked 🔒 are
write-gated (suppressed on Live instances unless `IRIS_ALLOW_PROD=1`). Tools marked ☠
are destructive-gated — they require `destructive_tools_enabled = true` in addition to
`write_tools_enabled = true`.

`namespace` defaults to `"USER"` on every tool. It is omitted from parameter tables below
unless there is something non-obvious to say about it.

---

## Quick reference

| Tool                                                              | Section                   |
| ----------------------------------------------------------------- | ------------------------- |
| [`iris_servers`](#iris_servers)                                   | Server Management         |
| [`iris_add_server`](#iris_add_server)                             | Server Management         |
| [`iris_remove_server`](#iris_remove_server-) ☠                    | Server Management         |
| [`iris_test_server`](#iris_test_server)                           | Server Management         |
| [`iris_import_servers`](#iris_import_servers)                     | Server Management         |
| [`iris_doc`](#iris_doc)                                           | Code                      |
| [`iris_compile`](#iris_compile)                                   | Code                      |
| [`iris_execute`](#iris_execute)                                   | Code                      |
| [`iris_execute_method`](#iris_execute_method)                     | Code                      |
| [`iris_query`](#iris_query)                                       | Code                      |
| [`iris_test`](#iris_test)                                         | Code                      |
| [`iris_coverage`](#iris_coverage)                                 | Code                      |
| [`iris_global`](#iris_global-) 🔒                                 | Code                      |
| [`iris_source_control`](#iris_source_control-) ✦                  | Code                      |
| [`iris_symbols`](#iris_symbols)                                   | Search and introspection  |
| [`iris_symbols_local`](#iris_symbols_local)                       | Search and introspection  |
| [`docs_introspect`](#docs_introspect)                             | Search and introspection  |
| [`iris_search`](#iris_search)                                     | Search and introspection  |
| [`iris_info`](#iris_info)                                         | Search and introspection  |
| [`iris_macro`](#iris_macro)                                       | Search and introspection  |
| [`iris_table_info`](#iris_table_info)                             | Search and introspection  |
| [`resolve_dynamic_dispatch`](#resolve_dynamic_dispatch)           | Search and introspection  |
| [`extract_message_map_routing`](#extract_message_map_routing)     | Search and introspection  |
| [`find_subclass_implementations`](#find_subclass_implementations) | Search and introspection  |
| [`iris_debug`](#iris_debug)                                       | Debugging                 |
| [`iris_get_log`](#iris_get_log)                                   | Debugging                 |
| [`check_config`](#check_config)                                   | Debugging                 |
| [`iris_generate`](#iris_generate)                                 | Generation                |
| [`iris_generate_class`](#iris_generate_class)                     | Generation                |
| [`iris_generate_test`](#iris_generate_test)                       | Generation                |
| [`iris_production`](#iris_production-) ✦                          | Interoperability          |
| [`iris_interop_query`](#iris_interop_query-) ✦                    | Interoperability          |
| [`iris_production_item`](#iris_production_item-) 🔒               | Interoperability          |
| [`iris_production_diff`](#iris_production_diff)                   | Interoperability          |
| [`iris_message_body`](#iris_message_body)                         | Interoperability          |
| [`iris_business_rule_info`](#iris_business_rule_info)             | Interoperability          |
| [`iris_credential_list`](#iris_credential_list)                   | Interoperability          |
| [`iris_credential_manage`](#iris_credential_manage--) 🔒 ☠        | Interoperability          |
| [`iris_lookup_manage`](#iris_lookup_manage-) ☠                    | Interoperability          |
| [`iris_lookup_transfer`](#iris_lookup_transfer)                   | Interoperability          |
| [`iris_ws_open`](#iris_ws_open)                                   | WebSocket sessions        |
| [`iris_ws_exec`](#iris_ws_exec)                                   | WebSocket sessions        |
| [`iris_ws_close`](#iris_ws_close)                                 | WebSocket sessions        |
| [`global_preview`](#global_preview)                               | Administration            |
| [`global_kill`](#global_kill--) 🔒 ☠                              | Administration            |
| [`iris_namespace_list`](#iris_namespace_list)                     | Administration            |
| [`iris_namespace_create`](#iris_namespace_create--) 🔒 ☠          | Administration            |
| [`iris_database_list`](#iris_database_list)                       | Administration            |
| [`iris_database_stats`](#iris_database_stats)                     | Administration            |
| [`journal_search`](#journal_search)                               | Administration            |
| [`query_audit_log`](#query_audit_log)                             | Administration            |
| [`stream_inspect`](#stream_inspect)                               | Administration            |
| [`my_access`](#my_access)                                         | Administration            |
| [`capability_matrix`](#capability_matrix)                         | Administration            |
| [`hl7_schema_list`](#hl7_schema_list)                             | Administration            |
| [`hl7_schema_inspect`](#hl7_schema_inspect)                       | Administration            |
| [`mermaid_class`](#mermaid_class)                                 | Administration            |
| [`mermaid_production`](#mermaid_production)                       | Administration            |
| [`resolve_storage`](#resolve_storage)                             | Administration            |
| [`compare_document`](#compare_document)                           | Administration            |
| [`compare_namespace`](#compare_namespace)                         | Administration            |
| [`iris_admin`](#iris_admin-) ☠                                    | Administration            |
| [`iris_containers`](#iris_containers-) ✦                          | Administration            |
| [`skill`](#skill)                                                 | Skills and knowledge base |
| [`skill_community`](#skill_community)                             | Skills and knowledge base |
| [`kb` / `kb_index` / `kb_recall`](#kb--kb_index--kb_recall)       | Skills and knowledge base |
| [`agent_history` / `agent_stats`](#agent_history--agent_stats)    | Skills and knowledge base |
| [`telemetry_query`](#telemetry_query)                             | Skills and knowledge base |
| [`telemetry_export_trace`](#telemetry_export_trace)               | Skills and knowledge base |

---

## Server Management

Tools for registering, testing, and managing IRIS server connections. All other tools
accept an optional `server` parameter that routes to a named instance — see `iris_servers`
to list what's registered.

### `iris_servers`

List all registered IRIS instances from all configuration sources (iad-native config, VS
Code Server Manager settings, workspace fleet config, environment variables). Shows name,
host, port, namespace, source, and reachability status (`null` = not yet tested).

### `iris_add_server`

Register a new IRIS instance. Writes server details to
`~/.config/iris-agentic-dev/servers.json` and stores the password in the OS keychain —
the password never appears in any config file. Uses the same keychain format as VS Code
Server Manager, so credentials are shared automatically if both tools are installed.

Parameters: `name`, `host`, `port`, `namespace`, `username`, `password`, `description`
(optional), `scheme` (optional, default `"http"`).

After adding a server, restart iad for the new connection to appear in the pool.

### `iris_remove_server` ☠

Remove a server from the iad-native config and its keychain entry. Requires
`destructive_tools_enabled = true`. Cannot remove servers sourced from VS Code settings —
edit `settings.json` directly for those.

### `iris_test_server`

Test connectivity to a named server without changing the active connection. Returns
Atelier API version, IRIS version string, and round-trip latency.

### `iris_import_servers`

One-time import of IRIS server definitions from VS Code or Cursor settings into the
iad-native config. Reads passwords from the existing OS keychain — no re-entry required.
Reports servers imported, skipped (already present), and those with no keychain entry.

---

## Code

### `iris_doc`

Read, write, delete, insert lines, or list IRIS documents.

| Parameter                    | Type     | Default  | Notes                                                                               |
| ---------------------------- | -------- | -------- | ----------------------------------------------------------------------------------- |
| `mode` (alias: `action`)     | string   | `"get"`  | See modes below                                                                     |
| `name` (alias: `document`)   | string   | —        | Document name, e.g. `"MyApp.MyClass.cls"`                                           |
| `names`                      | string[] | `[]`     | Batch get/delete                                                                    |
| `content`                    | string   | —        | Document content for `put`/`insert`                                                 |
| `compile`                    | bool     | `false`  | Compile after `put`                                                                 |
| `start`                      | int      | —        | Start line for `get` (fragment) or `delete_lines`                                   |
| `end`                        | int      | —        | End line for `get` (fragment) or `delete_lines`                                     |
| `line`                       | int      | —        | Insert-before line for `insert`                                                     |
| `expected`                   | string   | —        | CAS guard: current content at `start`–`end`; fails with `STALE_CONTENT` if mismatch |
| `pattern`                    | string   | —        | Glob filter for `list`, e.g. `"MyApp.*.cls"`                                        |
| `category`                   | string   | —        | `"CLS"` \| `"MAC"` \| `"INT"` \| `"INC"` \| `"ALL"`                                 |
| `max_results`                | int      | `200`    | Max 1000; for `list`                                                                |
| `compiled_type`              | string   | `"INT"`  | `"INT"` \| `"OBJ"`; for `compiled` mode                                             |
| `allow_storage_regeneration` | bool     | `false`  | Required to proceed when IRIS strips Storage blocks on PUT                          |
| `elicitation_id`             | string   | —        | SCM checkout dialog resume ID                                                       |
| `elicitation_answer`         | string   | —        | SCM checkout dialog answer                                                          |
| `namespace`                  | string   | `"USER"` |                                                                                     |

**Modes:**

| Mode           | What it does                                                              |
| -------------- | ------------------------------------------------------------------------- |
| `get`          | Read document content                                                     |
| `put`          | Write document content (SCM-gated, strips Storage blocks on IRIS 2025.1+) |
| `delete`       | Delete document                                                           |
| `head`         | Return metadata only (size, timestamp) without content                    |
| `fragment`     | Read lines `start`–`end`                                                  |
| `compiled`     | Read compiled INT or OBJ source                                           |
| `list`         | List documents matching `pattern`/`category`                              |
| `insert`       | Insert `content` before `line` (use `expected` for a CAS check)           |
| `delete_lines` | Delete lines `start`–`end` (use `expected` for a CAS check)               |

**Examples:**

```text
# Read a class
iris_doc(mode="get", name="MyApp.MyClass.cls")

# Write and compile
iris_doc(mode="put", name="MyApp.MyClass.cls", content="...", compile=true)

# Patch line 42 — fail if content changed since last read
iris_doc(mode="insert", name="MyApp.MyClass.cls", line=42,
         content="  Set x = 1\n", expected="  // placeholder\n")

# List all classes in a package
iris_doc(mode="list", pattern="MyApp.*.cls")
```

**Storage block guard.** IRIS 2025.1+ rejects Storage XML in a PUT request (upstream
bug). `iris_doc` strips it before writing and refuses by default with
`STORAGE_STRIP_BLOCKED`. Pass `allow_storage_regeneration: true` to proceed — but
understand that recompiling without Storage forces IRIS to regenerate global layout,
which can change the extent for `%Persistent` classes. Use `mode=insert` or
`mode=delete_lines` when the edit does not touch the Storage block.

**SCM checkout.** On source-controlled instances, `iris_doc` runs the SCM pre-write
check before writing. If checkout is required, the tool returns an elicitation dialog
rather than writing. Resume it with `elicitation_id` + `elicitation_answer`.

---

### `iris_compile`

Compile a class, routine, or wildcard pattern.

| Parameter        | Type   | Default  | Notes                                                           |
| ---------------- | ------ | -------- | --------------------------------------------------------------- |
| `target`         | string | —        | **Required.** Class name, routine, or glob like `"MyApp.*.cls"` |
| `flags`          | string | `"cuk"`  | Compile flags                                                   |
| `namespace`      | string | `"USER"` |                                                                 |
| `force_writable` | bool   | `false`  | Override read-only check                                        |
| `inline`         | bool   | `false`  | Return all errors inline (bypass log store)                     |

Returns errors with line numbers.

```text
iris_compile(target="MyApp.MyClass.cls")
iris_compile(target="MyApp.*.cls", flags="cukd")
```

---

### `iris_execute`

Run arbitrary ObjectScript and return the output.

| Parameter       | Type   | Default  | Notes                                                      |
| --------------- | ------ | -------- | ---------------------------------------------------------- |
| `code`          | string | —        | **Required.** ObjectScript to execute                      |
| `namespace`     | string | `"USER"` |                                                            |
| `timeout`       | int    | `120`    | Seconds; overridden by `OBJECTSCRIPT_TEST_TIMEOUT` env var |
| `translate_sql` | bool   | `true`   | Rewrite `&sql(...)` macros to `%SQL.Statement`             |
| `use_session`   | bool   | `false`  | Enable `%ctx` session carrier (see below)                  |
| `session_state` | string | —        | Token from a prior call; restores `%ctx`                   |

```text
iris_execute(code="Write $ZVersion")
iris_execute(code="Set sc = ##class(MyApp.Util).Run() Write sc", namespace="MYAPP")
```

**Code-edit guard** — see [Code-edit guard](#code-edit-guard) below.

#### Session state

Set `use_session: true` to get a `%ctx` variable (`%DynamicObject`) injected before your code
and serialized into a `session_state` token in the response. Pass that token back as
`session_state` on the next call to restore `%ctx`. Nothing is written to IRIS — the token
lives entirely in the client.

```text
# Call 1 — compute something and stash it
iris_execute(
  use_session=true,
  code="Set %ctx.count = 1247  Set %ctx.label = \"patients\""
)
# → response includes session_state: "eyJjb3VudCI6MTI0N..."

# Call 2 — pick up where you left off
iris_execute(
  use_session=true,
  session_state="eyJjb3VudCI6MTI0N...",
  code="Write %ctx.count * 0.05"
)
# → output: 62.35
```

`%Persistent` objects are automatically stubbed on save (`{"_cls": "...", "_id": "..."}`)
and re-opened on restore. `%DynamicObject` and scalar values survive round-trips unchanged.
Values that cannot serialize (open file handles, result sets, device references) must be
removed from `%ctx` before the epilogue runs.

Session error codes:

| Code                       | Meaning                                                                         |
| -------------------------- | ------------------------------------------------------------------------------- |
| `SESSION_INVALID`          | Token is malformed or `%FromJSON` failed                                        |
| `SESSION_RESTORE_FAILED`   | A stubbed `%Persistent` object could not be re-opened (class missing or bad ID) |
| `SESSION_SERIALIZE_FAILED` | `%ctx` could not be serialized at end of call                                   |

**`server`** (optional): route this call to a named registered IRIS instance. If omitted,
uses the default connection. Use `iris_servers` to list available instances.

---

### `iris_execute_method`

Invoke a `ClassMethod` directly by class, method name, and arguments.

| Parameter   | Type     | Default  | Notes                                   |
| ----------- | -------- | -------- | --------------------------------------- |
| `class`     | string   | —        | **Required.** e.g. `"%Library.Integer"` |
| `method`    | string   | —        | **Required.** e.g. `"IsValid"`          |
| `args`      | string[] | `[]`     | Positional string arguments             |
| `namespace` | string   | `"USER"` |                                         |

String-returning methods only (v1).

```text
iris_execute_method(class="MyApp.Util", method="GetVersion")
iris_execute_method(class="%Library.Integer", method="IsValid", args=["42"])
```

---

### `iris_query`

Execute SQL and return rows as JSON.

| Parameter           | Type     | Default  | Notes                                                |
| ------------------- | -------- | -------- | ---------------------------------------------------- |
| `query`             | string   | `""`     | SQL statement; required for `read`/`explain`/`write` |
| `parameters`        | string[] | `[]`     | Bind parameters (positional `?`)                     |
| `mode`              | string   | `"read"` | `"read"` \| `"explain"` \| `"count"` \| `"write"`    |
| `table`             | string   | —        | For `mode=count` without a `query`                   |
| `max_rows_affected` | int      | `1000`   | Write mode only; clamped to [1, 10000]               |
| `namespace`         | string   | `"USER"` |                                                      |
| `force`             | bool     | `false`  | Bypass SQL safety validation                         |

```text
# Read
iris_query(query="SELECT ID, Name FROM MyApp.Patient WHERE Status = ?", parameters=["Active"])

# Query plan
iris_query(query="SELECT * FROM MyApp.Patient", mode="explain")

# Row count without fetching data
iris_query(table="MyApp.Patient", mode="count")

# DML (gated)
iris_query(query="UPDATE MyApp.Patient SET Status = 'Archived' WHERE ID = ?",
           parameters=["123"], mode="write")
```

**`server`** (optional): route this call to a named registered IRIS instance. If omitted,
uses the default connection. Use `iris_servers` to list available instances.

---

### `iris_test`

Run `%UnitTest` tests and return structured pass/fail results.

| Parameter             | Type     | Default  | Notes                                                                  |
| --------------------- | -------- | -------- | ---------------------------------------------------------------------- |
| `pattern`             | string   | —        | **Required.** Package name, e.g. `"App.Tests"`                         |
| `namespace`           | string   | `"USER"` |                                                                        |
| `timeout`             | int      | `60`     | Seconds                                                                |
| `coverage`            | bool     | —        | Also measure line coverage inline                                      |
| `coverage_classes`    | string[] | —        | Explicit class list for coverage; defaults to all classes in `pattern` |
| `coverage_target_pct` | float    | —        | Fail if coverage falls below this threshold                            |

```text
iris_test(pattern="MyApp.Tests")
iris_test(pattern="MyApp.Tests", coverage=true, coverage_target_pct=80)
```

---

### `iris_coverage`

Standalone line coverage via `%Monitor.System.LineByLine`.

| Parameter        | Type     | Default  | Notes                                                |
| ---------------- | -------- | -------- | ---------------------------------------------------- |
| `mode`           | string   | —        | **Required.** See modes below                        |
| `classes`        | string[] | —        | Classes to monitor; used with `start`/`run`          |
| `package`        | string   | —        | Package prefix — alternative to explicit `classes`   |
| `test_path`      | string   | —        | `%UnitTest` package to run; required for `run`       |
| `target_pct`     | float    | —        | Fail threshold %                                     |
| `cobertura_path` | string   | —        | Write Cobertura XML here (requires TestCoverage IPM) |
| `namespace`      | string   | `"USER"` |                                                      |

| Mode     | What it does                                                           |
| -------- | ---------------------------------------------------------------------- |
| `check`  | Pre-flight: verify `gmheap ≥ 256 MB`; returns `testcoverage_available` |
| `run`    | All-in-one: start → RunTest → stop → report                            |
| `start`  | Start monitoring the given classes                                     |
| `stop`   | Stop monitoring                                                        |
| `report` | Collect results from a previously stopped run                          |

```text
iris_coverage(mode="check")
iris_coverage(mode="run", package="MyApp", test_path="MyApp.Tests", target_pct=80)
iris_coverage(mode="run", classes=["MyApp.Service", "MyApp.Util"], test_path="MyApp.Tests")
```

Requires `gmheap ≥ 256 MB`. Run `mode=check` first. If `BBSIZ_NOT_CONFIGURED` is
returned, increase `gmheap` in Management Portal → System Administration →
Configuration → Additional Settings → Advanced Memory, then restart IRIS.

---

### `iris_global` 🔒

Read, write, kill, or list IRIS global nodes. Gated — see [Data safety gates](#data-safety-gates).

| Parameter        | Type     | Default | Notes                                                    |
| ---------------- | -------- | ------- | -------------------------------------------------------- |
| `action`         | string   | —       | **Required.** `"get"` \| `"set"` \| `"kill"` \| `"list"` |
| `global_name`    | string   | —       | **Required.** With or without leading `^`                |
| `subscripts`     | string[] | —       | Subscript path; values validated `[a-zA-Z0-9 _.:\-]+`    |
| `value`          | string   | —       | Required for `action=set`                                |
| `subtree`        | bool     | —       | `get` only: include all descendants                      |
| `max_nodes`      | int      | `100`   | `get` + `subtree`; max 1000                              |
| `max_subscripts` | int      | `50`    | `list` action; max 500                                   |
| `acknowledgePhi` | bool     | —       | Required when global name matches a PHI pattern          |
| `namespace`      | string   | —       |                                                          |

```text
iris_global(action="get", global_name="^MyApp.Config", subscripts=["timeout"])
iris_global(action="list", global_name="^MyApp.Config")
iris_global(action="set", global_name="^MyApp.Config", subscripts=["timeout"], value="30")
```

---

### `iris_source_control` ✦

Check lock status, checkout, or execute SCM actions.

| Parameter        | Type   | Default  | Notes                                                               |
| ---------------- | ------ | -------- | ------------------------------------------------------------------- |
| `action`         | string | —        | **Required.** `"status"` \| `"menu"` \| `"checkout"` \| `"execute"` |
| `document`       | string | —        | Document name                                                       |
| `action_id`      | string | —        | For `action=execute`: the menu action ID                            |
| `elicitation_id` | string | —        | Elicitation dialog resume ID                                        |
| `answer`         | string | —        | Elicitation dialog answer                                           |
| `namespace`      | string | `"USER"` |                                                                     |

CheckIn is opt-in via `IRIS_SCM_ALLOW_CHECKIN=1`.

```text
iris_source_control(action="status", document="MyApp.MyClass.cls")
iris_source_control(action="checkout", document="MyApp.MyClass.cls")
iris_source_control(action="menu")   # list available actions
```

---

### Code-edit guard

`iris_execute` rejects any code matching class- or routine-editing patterns. The check
runs before execution — a compound line mixing innocent data work with one blocked token
is rejected entirely; nothing executes.

Blocked patterns: `%Dictionary.*Definition`, `$system.OBJ` (Load, Compile, Delete, and
variants), `%RoutineMgr`, and direct writes to code-storage globals (`^rOBJ`, `^rINDEX`,
`^%occRoutine`, etc.).

The error response includes a `matched` field naming the specific token and a
`remediation` field pointing to the correct tools:

- To write or delete a class/routine: `iris_doc` with `mode=put` or `mode=delete`.
- To compile: `iris_compile`.

The guard is non-configurable and applies to all connections. Error code:
`CODE_EDIT_BLOCKED`.

---

## Search and introspection

### `iris_symbols`

Search classes and methods via `%Dictionary`.

| Parameter   | Type   | Default  | Notes                     |
| ----------- | ------ | -------- | ------------------------- |
| `query`     | string | —        | **Required.** Search term |
| `limit`     | int    | `20`     |                           |
| `namespace` | string | `"USER"` |                           |

```text
iris_symbols(query="IRISDemo.*")
iris_symbols(query="Patient", limit=50)
```

---

### `iris_symbols_local`

Search `.cls`/`.mac`/`.inc` files on disk by glob pattern. No IRIS connection required.

| Parameter        | Type     | Default | Notes                                              |
| ---------------- | -------- | ------- | -------------------------------------------------- |
| `query`          | string   | —       | **Required.** Search string                        |
| `workspace_path` | string   | —       | Root path to search; defaults to current workspace |
| `limit`          | int      | `50`    |                                                    |
| `kinds`          | string[] | —       | Filter by symbol kind                              |

```text
iris_symbols_local(query="Patient")
iris_symbols_local(query="GetStatus", workspace_path="/home/user/myapp")
```

---

### `docs_introspect`

Deep class inspection: methods, properties, parameters, XData blocks, superclasses.
Returns `xdata_flow` for BPL and DTL classes showing the step tree.

| Parameter    | Type   | Default  |
| ------------ | ------ | -------- | ------------- |
| `class_name` | string | —        | **Required.** |
| `namespace`  | string | `"USER"` |

```text
docs_introspect(class_name="MyApp.BP.PatientProcess")
docs_introspect(class_name="Ens.BusinessProcess")
```

---

### `iris_search`

Full-text search across the namespace. Supports regex and category filters.

| Parameter        | Type     | Default  | Notes                                                                        |
| ---------------- | -------- | -------- | ---------------------------------------------------------------------------- |
| `query`          | string   | —        | **Required.** Search string                                                  |
| `documents`      | string[] | —        | **Required.** Scope, e.g. `["MyApp.*.cls"]`; empty triggers `SCOPE_REQUIRED` |
| `regex`          | bool     | `false`  |                                                                              |
| `case_sensitive` | bool     | `false`  |                                                                              |
| `category`       | string   | —        | `"CLS"` \| `"MAC"` \| `"INT"` \| `"INC"` \| `"ALL"`                          |
| `namespace`      | string   | `"USER"` |                                                                              |
| `inline`         | bool     | `false`  | Bypass log store                                                             |

Namespace-wide grep times out — always provide a `documents` scope.

```text
iris_search(query="GetPatient", documents=["MyApp.*.cls"])
iris_search(query="##class\(.*Patient", documents=["MyApp.*.cls"], regex=true)
```

---

### `iris_info`

Namespace discovery: documents, jobs, CSP apps, metadata.

| Parameter   | Type   | Default  | Notes                                                                                                                                      |
| ----------- | ------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `what`      | string | —        | **Required.** `"documents"` \| `"modified"` \| `"namespace"` \| `"metadata"` \| `"jobs"` \| `"csp_apps"` \| `"csp_debug"` \| `"sa_schema"` |
| `doc_type`  | string | —        | `"CLS"` \| `"MAC"` \| `"INT"` \| `"INC"` \| `"CSP"` \| `"ALL"`                                                                             |
| `name`      | string | —        | For `what=sa_schema`                                                                                                                       |
| `namespace` | string | `"USER"` |                                                                                                                                            |
| `inline`    | bool   | `false`  |                                                                                                                                            |

```text
iris_info(what="namespace")
iris_info(what="documents", doc_type="CLS")
iris_info(what="jobs")
```

---

### `iris_macro`

Inspect `$$$` macros: list, signature, location, definition, expand.

| Parameter   | Type     | Default  | Notes                                                                                   |
| ----------- | -------- | -------- | --------------------------------------------------------------------------------------- |
| `action`    | string   | —        | **Required.** `"list"` \| `"signature"` \| `"location"` \| `"definition"` \| `"expand"` |
| `name`      | string   | —        | Macro name                                                                              |
| `args`      | string[] | `[]`     | Arguments for `expand`                                                                  |
| `namespace` | string   | `"USER"` |                                                                                         |

```text
iris_macro(action="list")
iris_macro(action="signature", name="ThrowOnError")
iris_macro(action="expand", name="ThrowOnError", args=["sc"])
```

---

### `iris_table_info`

Inspect a SQL table: storage type, backing globals, optional row count.

| Parameter           | Type   | Default  | Notes                                 |
| ------------------- | ------ | -------- | ------------------------------------- |
| `table`             | string | —        | **Required.** `"Schema.Table"` format |
| `include_row_count` | bool   | `false`  |                                       |
| `namespace`         | string | `"USER"` |                                       |

```text
iris_table_info(table="MyApp.Patient")
iris_table_info(table="MyApp.Patient", include_row_count=true)
```

---

### `resolve_dynamic_dispatch`

Resolve `$classmethod`/`##class({var})` polymorphic dispatch to concrete candidate
classes, with confidence scores.

| Parameter        | Type   | Default  | Notes                                            |
| ---------------- | ------ | -------- | ------------------------------------------------ |
| `method_name`    | string | —        | **Required.**                                    |
| `package_prefix` | string | —        | Restrict candidates to a package, e.g. `"MyApp"` |
| `limit`          | int    | `50`     |                                                  |
| `namespace`      | string | `"USER"` |                                                  |

```text
resolve_dynamic_dispatch(method_name="ProcessRequest", package_prefix="MyApp")
```

---

### `extract_message_map_routing`

Extract a compiled Ensemble `MessageMap` routing table from a BusinessProcess or Router.

| Parameter    | Type   | Default  |
| ------------ | ------ | -------- | ------------- |
| `class_name` | string | —        | **Required.** |
| `namespace`  | string | `"USER"` |

```text
extract_message_map_routing(class_name="MyApp.BP.PatientProcess")
```

---

### `find_subclass_implementations`

Find all concrete subclass implementations of a method across the inheritance hierarchy.

| Parameter      | Type     | Default  | Notes                        |
| -------------- | -------- | -------- | ---------------------------- |
| `method_name`  | string   | —        | **Required.**                |
| `base_classes` | string[] | —        | **Required.** Non-empty list |
| `limit`        | int      | `100`    |                              |
| `namespace`    | string   | `"USER"` |                              |

```text
find_subclass_implementations(method_name="OnProcessInput",
                              base_classes=["Ens.BusinessProcess"])
```

---

## Debugging

### `iris_debug`

Map INT offsets to source lines, fetch error logs, capture error state.

| Parameter      | Type   | Default  | Notes                                                                        |
| -------------- | ------ | -------- | ---------------------------------------------------------------------------- |
| `action`       | string | —        | **Required.** `"map_int"` \| `"error_logs"` \| `"capture"` \| `"source_map"` |
| `error_string` | string | —        | For `action=map_int`, e.g. `"^MyApp.Patient.1+42"`                           |
| `class_name`   | string | —        | For `action=source_map`                                                      |
| `limit`        | int    | `20`     |                                                                              |
| `namespace`    | string | `"USER"` |                                                                              |

ObjectScript errors report INT line numbers. `map_int` resolves them to source
locations.

```text
iris_debug(action="map_int", error_string="^MyApp.Patient.1+42^MyApp.Patient")
iris_debug(action="error_logs", limit=50)
iris_debug(action="capture")
```

---

### `iris_get_log`

Retrieve a full result when a tool returns `truncated: true`.

| Parameter | Type   | Default | Notes                                           |
| --------- | ------ | ------- | ----------------------------------------------- |
| `id`      | string | —       | Log entry UUID; omit to list all stored entries |
| `limit`   | int    | —       | Max entries; must be > 0 if provided            |
| `offset`  | int    | `0`     | Start index into stored results                 |

```text
iris_get_log(id="a1b2c3d4-...")
iris_get_log()   # list all stored entries
```

---

### `check_config`

Show active connection state. No parameters.

Returns host, port, namespace, discovery source, container name, config file path, and
write tool status. Run this first if anything seems misconfigured.

---

## Generation

### `iris_generate`

Build a context-rich prompt for generating ObjectScript. Pulls relevant class
definitions and coding conventions into a structured prompt. No API key required.

| Parameter     | Type   | Default   | Notes                          |
| ------------- | ------ | --------- | ------------------------------ |
| `description` | string | —         | **Required.** What to generate |
| `gen_type`    | string | `"class"` | `"class"` \| `"test"`          |
| `class_name`  | string | —         | Source class for context       |
| `namespace`   | string | `"USER"`  |                                |

```text
iris_generate(description="A REST API handler that validates patient demographics",
              class_name="MyApp.Patient")
```

---

### `iris_generate_class`

Generate and compile a class from a description. Requires an LLM API key in the
connection config.

| Parameter     | Type   | Default  | Notes                             |
| ------------- | ------ | -------- | --------------------------------- |
| `description` | string | —        | **Required.**                     |
| `overwrite`   | bool   | `false`  | Overwrite if class already exists |
| `namespace`   | string | `"USER"` |                                   |

```text
iris_generate_class(description="A %Persistent class storing patient visit records with indexes on PatientID and VisitDate")
```

---

### `iris_generate_test`

Generate `%UnitTest` scaffolding for an existing class.

| Parameter    | Type   | Default  |
| ------------ | ------ | -------- | ------------- |
| `class_name` | string | —        | **Required.** |
| `namespace`  | string | `"USER"` |

```text
iris_generate_test(class_name="MyApp.Service")
```

---

## Interoperability

### `iris_production` ✦

Start, stop, update, check, or recover a production.

| Parameter     | Type   | Default  | Notes                                                                                                                                                     |
| ------------- | ------ | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action`      | string | —        | **Required.** `"status"` \| `"start"` \| `"stop"` \| `"update"` \| `"check"` \| `"recover"` \| `"get_autostart"` \| `"set_autostart"` \| `"needs_update"` |
| `production`  | string | —        | Production class name; defaults to the currently running production                                                                                       |
| `timeout`     | int    | `30`     | Seconds; for `stop`/`update`                                                                                                                              |
| `force`       | bool   | `false`  | For `stop`/`update`                                                                                                                                       |
| `full_status` | bool   | `false`  | `status` only: include per-item state                                                                                                                     |
| `enabled`     | bool   | —        | `set_autostart` only                                                                                                                                      |
| `namespace`   | string | `"USER"` |                                                                                                                                                           |

Actions that modify production state require `IRIS_CONTAINER`.

```text
iris_production(action="status")
iris_production(action="stop", timeout=60, force=true)
iris_production(action="start", production="MyApp.Production")
```

---

### `iris_interop_query` ✦

Query production logs, queue depths, or message archive.

| Parameter    | Type   | Default           | Notes                                                |
| ------------ | ------ | ----------------- | ---------------------------------------------------- |
| `what`       | string | —                 | **Required.** `"logs"` \| `"queues"` \| `"messages"` |
| `item_name`  | string | —                 | `logs`: filter by business host name                 |
| `log_type`   | string | `"error,warning"` | `logs` only                                          |
| `limit`      | int    | `10`/`20`         | 10 for logs, 20 for messages                         |
| `source`     | string | —                 | `messages`: filter by source                         |
| `target`     | string | —                 | `messages`: filter by target                         |
| `class_name` | string | —                 | `messages`: filter by message class                  |
| `namespace`  | string | `"USER"`          |                                                      |

```text
iris_interop_query(what="logs", log_type="error", limit=50)
iris_interop_query(what="queues")
iris_interop_query(what="messages", source="MyApp.BS.HL7Listener", limit=20)
```

---

### `iris_production_item` 🔒

Enable, disable, or get/set settings on an individual production config item.

| Parameter   | Type                 | Default  | Notes                                                                           |
| ----------- | -------------------- | -------- | ------------------------------------------------------------------------------- |
| `action`    | string               | —        | **Required.** `"enable"` \| `"disable"` \| `"get_settings"` \| `"set_settings"` |
| `item`      | string               | —        | **Required.** Production item name                                              |
| `settings`  | map\<string,string\> | `{}`     | For `set_settings`                                                              |
| `namespace` | string               | `"USER"` |                                                                                 |

Works via Atelier HTTP — no Docker required.

```text
iris_production_item(action="get_settings", item="MyApp.BO.LabResults")
iris_production_item(action="set_settings", item="MyApp.BO.LabResults",
                     settings={"ReplyCodeActions": "E=R,D=C,~=C"})
iris_production_item(action="disable", item="MyApp.BS.HL7Listener")
```

---

### `iris_production_diff`

Diff the running production config against the last source-controlled version.

| Parameter    | Type   | Default  |
| ------------ | ------ | -------- | ---------------------------------------- |
| `production` | string | —        | Defaults to currently running production |
| `namespace`  | string | `"USER"` |

```text
iris_production_diff()
iris_production_diff(production="MyApp.Production")
```

---

### `iris_message_body`

Read a message body by ID. Gated — see [Data safety gates](#data-safety-gates).

| Parameter         | Type   | Default  | Notes                                |
| ----------------- | ------ | -------- | ------------------------------------ |
| `message_id`      | string | —        | **Required.**                        |
| `max_bytes`       | int    | `65536`  | Max 1 MB (1048576)                   |
| `acknowledge_phi` | bool   | `false`  | Required when `dataPolicy = "allow"` |
| `namespace`       | string | `"USER"` |                                      |

```text
iris_message_body(message_id="123456")
iris_message_body(message_id="123456", acknowledge_phi=true)
```

---

### `iris_business_rule_info`

List or inspect Ensemble business rules.

| Parameter   | Type   | Default  |
| ----------- | ------ | -------- | --------------------------------- |
| `action`    | string | —        | **Required.** `"list"` \| `"get"` |
| `rule_name` | string | —        |
| `namespace` | string | `"USER"` |

```text
iris_business_rule_info(action="list")
iris_business_rule_info(action="get", rule_name="MyApp.RoutingRule")
```

---

### `iris_credential_list`

List Ensemble credentials. Passwords are never returned.

| Parameter   | Type   | Default  |
| ----------- | ------ | -------- |
| `namespace` | string | `"USER"` |

---

### `iris_credential_manage` 🔒 ☠

Create, update, or delete an Ensemble credential.

| Parameter   | Type   | Default  | Notes                                                |
| ----------- | ------ | -------- | ---------------------------------------------------- |
| `action`    | string | —        | **Required.** `"create"` \| `"update"` \| `"delete"` |
| `id`        | string | —        | **Required.** Credential ID                          |
| `username`  | string | —        |                                                      |
| `password`  | string | —        |                                                      |
| `namespace` | string | `"USER"` |                                                      |

---

### `iris_lookup_manage` ☠

Read, write, delete, or list Ensemble lookup table entries. Write and delete actions are
🔒 ☠ gated. Read actions (`get`, `list_keys`, `list_tables`) are unrestricted.

| Parameter   | Type   | Default  | Notes                                                                              |
| ----------- | ------ | -------- | ---------------------------------------------------------------------------------- |
| `action`    | string | —        | **Required.** `"get"` \| `"set"` \| `"delete"` \| `"list_keys"` \| `"list_tables"` |
| `table`     | string | —        | Table name                                                                         |
| `key`       | string | —        |                                                                                    |
| `value`     | string | —        | For `action=set`                                                                   |
| `namespace` | string | `"USER"` |                                                                                    |

```text
iris_lookup_manage(action="list_tables")
iris_lookup_manage(action="list_keys", table="FacilityMap")
iris_lookup_manage(action="get", table="FacilityMap", key="MGH")
```

---

### `iris_lookup_transfer`

Export or import an Ensemble lookup table as XML. Import is 🔒 gated.

| Parameter   | Type   | Default  | Notes                                  |
| ----------- | ------ | -------- | -------------------------------------- |
| `action`    | string | —        | **Required.** `"export"` \| `"import"` |
| `table`     | string | —        | **Required.**                          |
| `xml`       | string | —        | For `action=import`                    |
| `namespace` | string | `"USER"` |                                        |

---

## Administration

### `global_preview`

Preview the top N subscripts of an IRIS global and mint a confirmation token for a
subsequent `global_kill`. Returns up to 100 entries, the total subscript count, and a
`confirm_token` that expires in 5 minutes.

| Parameter | Type   | Default | Notes                                          |
| --------- | ------ | ------- | ---------------------------------------------- |
| `global`  | string | —       | **Required.** Global name, with or without `^` |
| `count`   | number | `20`    | Max entries to preview (1–100)                 |
| `server`  | string | —       | Named server; omit for default                 |

### `global_kill` 🔒 ☠

Kill an IRIS global after confirming with a token from `global_preview`. The token
validates the global name and server — a token issued for `^Foo` cannot be used to kill
`^Bar`. Tokens expire after 5 minutes. Write-gated.

| Parameter       | Type   | Default | Notes                                            |
| --------------- | ------ | ------- | ------------------------------------------------ |
| `global`        | string | —       | **Required.** Must match the global in the token |
| `confirm_token` | string | —       | **Required.** Token from `global_preview`        |
| `server`        | string | —       | Named server; omit for default                   |

### `iris_namespace_list`

List all namespaces on the connected IRIS instance.

| Parameter | Type   | Default | Notes                          |
| --------- | ------ | ------- | ------------------------------ |
| `server`  | string | —       | Named server; omit for default |

### `iris_namespace_create` 🔒 ☠

Create a new namespace and its backing database. Write-gated and destructive-gated.

| Parameter | Type   | Default | Notes                                        |
| --------- | ------ | ------- | -------------------------------------------- |
| `name`    | string | —       | **Required.** Namespace name (A–Z, 0–9, `-`) |
| `db_path` | string | —       | Database directory path; defaults to `name`  |
| `server`  | string | —       | Named server; omit for default               |

### `iris_database_list`

List databases and their directory paths.

| Parameter | Type   | Default | Notes                          |
| --------- | ------ | ------- | ------------------------------ |
| `server`  | string | —       | Named server; omit for default |

### `iris_database_stats`

Show size, free space, and block stats for a specific database directory.

| Parameter | Type   | Default | Notes                                 |
| --------- | ------ | ------- | ------------------------------------- |
| `db_path` | string | —       | **Required.** Database directory path |
| `server`  | string | —       | Named server; omit for default        |

### `journal_search`

Search the IRIS journal for global set/kill records in a time range. Bulk-PHI gated —
requires `dataPolicy = "allow"` on the connection.

| Parameter        | Type   | Default | Notes                                |
| ---------------- | ------ | ------- | ------------------------------------ |
| `start`          | string | —       | ISO 8601 start timestamp (inclusive) |
| `end`            | string | —       | ISO 8601 end timestamp (inclusive)   |
| `global_pattern` | string | —       | Substring to filter global names     |
| `max_entries`    | number | `100`   | Cap results (1–500)                  |
| `server`         | string | —       | Named server; omit for default       |

### `query_audit_log`

Query the `%SYS_Audit.Log` table for recent events.

| Parameter    | Type   | Default | Notes                              |
| ------------ | ------ | ------- | ---------------------------------- |
| `event_type` | string | —       | Filter by `Event` column substring |
| `username`   | string | —       | Filter by `SystemID` substring     |
| `limit`      | number | `50`    | Max rows (1–200)                   |
| `server`     | string | —       | Named server; omit for default     |

### `stream_inspect`

Inspect a `%Stream.GlobalBinary` or `%Stream.GlobalCharacter` object by OID.
Returns the first N characters and the total size.

| Parameter   | Type   | Default  | Notes                               |
| ----------- | ------ | -------- | ----------------------------------- |
| `oid`       | string | —        | **Required.** Stream OID            |
| `namespace` | string | `"USER"` | Namespace that contains the stream  |
| `max_chars` | number | `2000`   | Max characters to return (1–10 000) |
| `server`    | string | —        | Named server; omit for default      |

### `my_access`

Show current user, roles, and privileges for the connected session.

| Parameter | Type   | Default | Notes                          |
| --------- | ------ | ------- | ------------------------------ |
| `server`  | string | —       | Named server; omit for default |

### `capability_matrix`

Show which role grants which privilege across a list of namespaces. Useful for auditing
access before a release.

| Parameter    | Type     | Default | Notes                              |
| ------------ | -------- | ------- | ---------------------------------- |
| `namespaces` | string[] | `[]`    | Namespaces to include; empty = all |
| `server`     | string   | —       | Named server; omit for default     |

### `hl7_schema_list`

List available HL7 2.x schema versions. Returns `HL7_NOT_AVAILABLE` if
`EnsLib.HL7.Schema` is not installed (e.g. Community edition without Ensemble).

| Parameter | Type   | Default | Notes                          |
| --------- | ------ | ------- | ------------------------------ |
| `server`  | string | —       | Named server; omit for default |

### `hl7_schema_inspect`

Show segment definitions, field names, and data types for a specific HL7 schema version
and optional segment filter.

| Parameter | Type   | Default | Notes                                        |
| --------- | ------ | ------- | -------------------------------------------- |
| `version` | string | —       | **Required.** e.g. `"2.6"`                   |
| `segment` | string | —       | Segment name filter, e.g. `"PID"` (optional) |
| `server`  | string | —       | Named server; omit for default               |

### `mermaid_class`

Generate a Mermaid class diagram showing inheritance for one or more classes. Walks the
`Super` hierarchy up to 3 levels deep and strips `%`-prefixed system class names.

| Parameter   | Type     | Default  | Notes                              |
| ----------- | -------- | -------- | ---------------------------------- |
| `classes`   | string[] | —        | **Required.** Starting class names |
| `namespace` | string   | `"USER"` | Namespace to query                 |
| `server`    | string   | —        | Named server; omit for default     |

### `mermaid_production`

Generate a Mermaid flowchart of an Ensemble/IRIS Interoperability production — hosts,
connections, and enabled/disabled state.

| Parameter    | Type   | Default  | Notes                               |
| ------------ | ------ | -------- | ----------------------------------- |
| `production` | string | —        | **Required.** Production class name |
| `namespace`  | string | `"USER"` | Namespace that hosts the production |
| `server`     | string | —        | Named server; omit for default      |

### `resolve_storage`

Show the storage definition (`^oddDEF` structure) for a persistent class. Helps diagnose
global layout, extents, and index locations.

| Parameter   | Type   | Default  | Notes                          |
| ----------- | ------ | -------- | ------------------------------ |
| `class`     | string | —        | **Required.** Class name       |
| `namespace` | string | `"USER"` | Namespace to query             |
| `server`    | string | —        | Named server; omit for default |

### `compare_document`

Compare a single document (class, routine, or include file) between two IRIS servers.
Returns `same: true/false` and a unified diff when different.

| Parameter   | Type   | Default  | Notes                                         |
| ----------- | ------ | -------- | --------------------------------------------- |
| `document`  | string | —        | **Required.** Document name, e.g. `MyApp.cls` |
| `server_a`  | string | —        | **Required.** First server name               |
| `server_b`  | string | —        | **Required.** Second server name              |
| `namespace` | string | `"USER"` | Namespace on both servers                     |

### `compare_namespace`

Compare all classes in a namespace between two IRIS servers. Lists classes only in A,
only in B, and classes present in both that differ. Caps comparison at 200 classes to
avoid overload — `unchecked_count` reports how many were skipped.

| Parameter   | Type   | Default  | Notes                            |
| ----------- | ------ | -------- | -------------------------------- |
| `namespace` | string | `"USER"` | Namespace to compare             |
| `server_a`  | string | —        | **Required.** First server name  |
| `server_b`  | string | —        | **Required.** Second server name |

### `iris_admin` ☠

List namespaces, databases, users, roles, and web apps. Read actions have no gate.
Write actions require both `destructive_tools_enabled = true` and `IRIS_ADMIN_TOOLS=1`.

**Read actions** (no env gate):

| Action               | Parameters                                                                                                 |
| -------------------- | ---------------------------------------------------------------------------------------------------------- |
| `list_namespaces`    | —                                                                                                          |
| `list_databases`     | —                                                                                                          |
| `list_users`         | —                                                                                                          |
| `list_roles`         | —                                                                                                          |
| `list_webapps`       | `type_filter` (string, optional)                                                                           |
| `get_webapp`         | `path` (string, required)                                                                                  |
| `check_permission`   | `resource` (string, required), `permission` (string, required)                                             |
| `view_locks`         | —                                                                                                          |
| `view_processes`     | `namespace_filter` (string, optional)                                                                      |
| `namespace_mappings` | `namespace` (string, optional)                                                                             |
| `database_status`    | `name_filter` (string, optional)                                                                           |
| `list_user_roles`    | `username` (string, required)                                                                              |
| `journal_search`     | `global_pattern` (string), `time_range` (`{from, to}` ISO8601), `max_records` (int, default 100, max 1000) |

**Write actions** (require `IRIS_ADMIN_TOOLS=1`):

| Action             | Parameters                                                             |
| ------------------ | ---------------------------------------------------------------------- |
| `create_user`      | `username`, `password` (required); `full_name`, `roles` (optional)     |
| `update_user`      | `username` (required); `password`, `enabled`, `roles` (optional)       |
| `delete_user`      | `username` (required)                                                  |
| `create_namespace` | `name`, `code_database`, `data_database` (all required)                |
| `delete_namespace` | `name` (required)                                                      |
| `create_webapp`    | `path`, `namespace`, `enabled` (required); `dispatch_class` (optional) |
| `delete_webapp`    | `path` (required)                                                      |

```text
iris_admin(action="list_namespaces")
iris_admin(action="list_users")
iris_admin(action="journal_search", global_pattern="^MyApp.*",
           time_range={"from": "2025-01-01T00:00:00Z", "to": "2025-01-02T00:00:00Z"})
```

---

### `iris_containers` ✦

List, select, or start IRIS Docker containers.

**`action=list`**

| Parameter        | Type   | Default | Notes                             |
| ---------------- | ------ | ------- | --------------------------------- |
| `workspace_root` | string | —       | Root path for container discovery |

**`action=select`**

| Parameter   | Type   | Default     | Notes                        |
| ----------- | ------ | ----------- | ---------------------------- |
| `name`      | string | —           | **Required.** Container name |
| `namespace` | string | `"USER"`    |                              |
| `username`  | string | `"_SYSTEM"` |                              |
| `password`  | string | `"SYS"`     |                              |

**`action=start`**

| Parameter | Type   | Default       | Notes          |
| --------- | ------ | ------------- | -------------- |
| `name`    | string | `""`          | Container name |
| `edition` | string | `"community"` |                |

```text
iris_containers(action="list")
iris_containers(action="select", name="my-iris-container")
```

---

## WebSocket sessions

Persistent IRIS terminal sessions over WebSocket. Requires IRIS 2023.2+ (Atelier V7 API).
Each session keeps a live ObjectScript context between calls — variables set in one
`iris_ws_exec` call are visible in the next.

Session tokens have the form `ws:{server}:{NAMESPACE}:{uuid}`.

### `iris_ws_open`

Open a new WebSocket terminal session. Returns a `session_token` to pass to subsequent
calls.

| Parameter   | Type   | Default  | Notes                          |
| ----------- | ------ | -------- | ------------------------------ |
| `namespace` | string | `"USER"` | Namespace for the session      |
| `server`    | string | —        | Named server; omit for default |

### `iris_ws_exec`

Execute ObjectScript in an existing session. The session context (variables, open
devices) persists across calls.

| Parameter       | Type   | Default | Notes                                   |
| --------------- | ------ | ------- | --------------------------------------- |
| `session_token` | string | —       | **Required.** Token from `iris_ws_open` |
| `code`          | string | —       | **Required.** ObjectScript to run       |
| `timeout_secs`  | number | `30`    | Per-call execution timeout              |

### `iris_ws_close`

Close a WebSocket session and free its resources. Passing an already-closed or expired
token returns `already_closed: true` rather than an error.

| Parameter       | Type   | Default | Notes                                   |
| --------------- | ------ | ------- | --------------------------------------- |
| `session_token` | string | —       | **Required.** Token from `iris_ws_open` |

---

## Learning agent, skills, and knowledge base

### `skill`

Manage the learning agent skill registry.

| Parameter | Type   | Default | Notes                                                                             |
| --------- | ------ | ------- | --------------------------------------------------------------------------------- |
| `action`  | string | —       | **Required.** `"list"` \| `"describe"` \| `"search"` \| `"forget"` \| `"propose"` |
| `name`    | string | —       | For `describe`/`forget`                                                           |
| `query`   | string | —       | For `search`                                                                      |

`action=forget` is ☠ destructive-gated — requires `destructive_tools_enabled = true`.

```text
skill(action="list")
skill(action="describe", name="objectscript-review")
skill(action="search", query="status handling")
skill(action="propose")   # mine recent tool calls into a new skill
skill(action="forget", name="outdated-skill")
```

---

### `skill_community`

Browse or install community skills from subscribed GitHub repos.

| Parameter | Type   | Default | Notes                                 |
| --------- | ------ | ------- | ------------------------------------- |
| `action`  | string | —       | **Required.** `"list"` \| `"install"` |
| `package` | string | —       | For `action=install`                  |

---

### `kb` / `kb_index` / `kb_recall`

Index markdown/text into the IRIS knowledge base, or recall content by keyword.

**kb** (unified):

| Parameter | Type   | Default | Notes                                 |
| --------- | ------ | ------- | ------------------------------------- |
| `action`  | string | —       | **Required.** `"index"` \| `"recall"` |
| `path`    | string | —       | For `action=index`                    |
| `query`   | string | —       | For `action=recall`                   |
| `top_k`   | int    | `5`     |                                       |

**kb_index**:

| Parameter        | Type   | Default |
| ---------------- | ------ | ------- |
| `workspace_path` | string | —       |

**kb_recall**:

| Parameter | Type   | Default | Notes         |
| --------- | ------ | ------- | ------------- |
| `query`   | string | —       | **Required.** |
| `top_k`   | int    | `20`    |               |

```text
kb(action="index", path="/home/user/myapp/docs")
kb(action="recall", query="how to handle %Status errors", top_k=5)
```

---

### `agent_history` / `agent_stats`

Recent tool-call history and learning agent status.

| Parameter | Type | Default |
| --------- | ---- | ------- |
| `limit`   | int  | `20`    |

```text
agent_history(limit=50)
agent_stats()
```

---

### `telemetry_query`

Query the durable telemetry record.

| Parameter    | Type   | Default | Notes          |
| ------------ | ------ | ------- | -------------- |
| `tool_name`  | string | —       | Filter by tool |
| `session_id` | string | —       |                |
| `since`      | string | —       | ISO8601        |
| `until`      | string | —       | ISO8601        |
| `limit`      | int    | `500`   |                |

```text
telemetry_query(tool_name="iris_doc", since="2025-01-01T00:00:00Z")
```

---

### `telemetry_export_trace`

Export tool calls as `{from, to, via, count, ts}` dispatch-trace records.

| Parameter    | Type   | Default | Notes   |
| ------------ | ------ | ------- | ------- |
| `session_id` | string | —       |         |
| `since`      | string | —       | ISO8601 |

---

## Coverage

See [`iris_coverage`](#iris_coverage) and [`iris_test`](#iris_test) above for the full
parameter reference.

**Quick reference:**

```text
iris_coverage(mode="check")
iris_coverage(mode="run", package="MyApp", test_path="MyApp.Tests", target_pct=80)
iris_test(pattern="MyApp.Tests", coverage=true, coverage_target_pct=80)
```

Every response includes `testcoverage_available`. When the
[TestCoverage](https://github.com/intersystems/TestCoverage) IPM package is installed,
`cobertura_path` writes Cobertura XML output.

**VS Code:** The [InterSystems Testing Manager](https://marketplace.visualstudio.com/items?itemName=intersystems-community.intersystems-testingmanager)
extension surfaces the same `%UnitTest` classes in the Test Explorer view. Use
`iris_test` to run and fix tests from Copilot or Claude; use Testing Manager to browse
results in the IDE. They share the same server connection.

---

## Tool annotations

Every tool in iad exposes MCP `ToolAnnotations` — machine-readable hints that MCP clients
can act on before calling the tool.

| Annotation         | Set on                                                                                                                                               | What it means                                      |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| `read_only_hint`   | 57 tools — all query, inspect, list, history, and comparison tools                                                                                   | The tool makes no changes to IRIS state            |
| `destructive_hint` | 7 tools — `global_kill`, `iris_admin`, `iris_credential_manage`, `iris_lookup_manage`, `iris_namespace_create`, `iris_remove_server`, `skill_forget` | The tool can irreversibly delete or overwrite data |

MCP clients that respect `read_only_hint` can run read-only tools in parallel or in
background without approval prompts. Clients that respect `destructive_hint` can surface
an extra confirmation step before calling the destructive tools.

These are hints, not enforcement. Enforcement comes from the config gates described below.

---

## Write protection

Three config keys control which servers and which tools can perform writes. They form a
stack — each layer can only further restrict, never expand.

### `write_tools_enabled`

All write tools return `WRITE_TOOLS_DISABLED` when this is `false` (the default for
connections detected as Live). Set it in `.iris-agentic-dev.toml`:

```toml
write_tools_enabled = true
```

### `destructive_tools_enabled`

The 7 tools marked ☠ require an additional opt-in. Even with `write_tools_enabled = true`,
they return `DESTRUCTIVE_TOOLS_DISABLED` unless you also set:

```toml
destructive_tools_enabled = true
```

Default: `false`. Setting `destructive_tools_enabled = true` with `write_tools_enabled = false`
is an error — iad refuses to start with `DESTRUCTIVE_REQUIRES_WRITES`.

**Why a separate flag?** A compile-test workflow needs `write_tools_enabled = true`, but
there's no reason for that same session to be able to kill globals or delete namespaces.
Enabling the two tiers independently means an agent that can compile can't accidentally wipe
data even if it constructs a destructive call.

Environment variable: `IRIS_DESTRUCTIVE_TOOLS_ENABLED=1`

### `write_allowed_servers`

With a multi-instance pool, writes can be directed to any registered server by name.
`write_allowed_servers` restricts which names are valid write targets:

```toml
write_allowed_servers = ["dev", "staging"]
```

Any write-capable tool call with `server: "prod"` (or any other name not in the list)
returns `WRITE_SERVER_NOT_ALLOWED`. Read-only tools (`read_only_hint = true`) are
unaffected — they work against any server regardless of this setting.

When `server` is omitted, the active (default) connection is checked. If the default
connection has no registered name (env-var or bare toml), the allowlist check is skipped.

An empty list `write_allowed_servers = []` blocks writes to every named server. Omitting
the key entirely disables the filter.

Environment variable: `IRIS_WRITE_ALLOWED_SERVERS=dev,staging` (comma-separated)

### Check order for write tool calls

```text
1. write_tools_enabled        — if false, WRITE_TOOLS_DISABLED
2. write_allowed_servers      — if set and server not listed, WRITE_SERVER_NOT_ALLOWED
3. destructive_tools_enabled  — if false and tool is ☠, DESTRUCTIVE_TOOLS_DISABLED
4. policy.<server>.allow      — category gate (POLICY_GATE)
5. data safety gates          — PHI, system globals, env template
6. Execute
```

Read-only tools skip steps 1–3.

---

## Data safety gates

PHI is Protected Health Information — the patient-identifying data HIPAA governs.

Some tools can reach PHI, and some can reach the globals IRIS uses to store its own code
and configuration. Those calls are checked before they run and refused by default. Four
checks run in order; the first one that refuses wins.

**1. Environment template.** A connection declares what kind of instance it points at via
`mcpTemplate` in `.iris-agentic-dev.toml` — `dev` (the default) permits everything,
`test` blocks code execution and compiles, and `live` blocks those plus source control.
Writes count as execution here: `iris_global` with `action=set` or `kill`, and
`iris_query` with `mode=write`, are treated as execution even though reads from the same
tools are not. Error code: `ENV_GATE_BLOCKED`.

**2. Bulk-PHI tools.** `journal_search` and `iris_message_body` return whole records, so
there is no field to inspect and no safe subset to return. They are refused outright
unless the connection sets `dataPolicy = "allow"`, with no per-call override. Error code:
`DATA_POLICY_BLOCKED`.

**3. System globals.** IRIS keeps compiled classes, routines, roles, users, and
interoperability config in globals such as `^oddDEF`, `^ROUTINE`, `^%Dictionary*`,
`^ROLE`, and `^Ens.Config*`. Writing to them can leave the instance unable to compile or
start. This blocklist is hardcoded and cannot be switched off — `globalBlocklist` in your
config adds to it, never replaces it. The one exception is `dataPolicyKillAllowlist`,
which exempts named patterns from this check on kill operations only. Error code:
`SYSTEM_BLOCKLIST`.

**4. Globals whose names suggest PHI.** Reading `^PAPMI*`, `^PAADM*`, `^MRADM*`,
`^ORDER*`, and similar requires `acknowledgePhi: true` on the call. This is a speed bump,
not a lock — it exists so nobody pulls a patient record into a chat transcript by
accident. Error code: `PHI_GATE_BLOCKED`.

`iris_message_body` is gated separately by `dataPolicy` alone: `block` refuses the call
(`PHI_POLICY_BLOCKED`), `allow` requires `acknowledgePhi: true` (`PHI_ACK_REQUIRED`), and
`redact` returns the body with the standard HL7 v2 patient fields replaced by `[REDACTED]`
(PID-3, 5, 7, 8, 11, 18 and MSH-3). Redaction only recognizes HL7 v2 — anything else
comes back as-is, so `redact` is not a safe default for XML or custom message bodies.

---

## Common error codes

| Code                          | Meaning                                                                                              |
| ----------------------------- | ---------------------------------------------------------------------------------------------------- |
| `POLICY_GATE`                 | Call blocked by per-connection policy — see `allow` in `.iris-agentic-dev.toml`                      |
| `ENV_GATE_BLOCKED`            | Tool not permitted by this connection's `mcpTemplate` — see [gates](#data-safety-gates)              |
| `DATA_POLICY_BLOCKED`         | Bulk-PHI tool called without `dataPolicy = "allow"`                                                  |
| `SYSTEM_BLOCKLIST`            | Global is on the system blocklist — not bypassable                                                   |
| `PHI_GATE_BLOCKED`            | Global name matches a PHI pattern — pass `acknowledgePhi: true`                                      |
| `SCOPE_REQUIRED`              | `iris_search` called without a document scope — pass a `documents` wildcard list                     |
| `STALE_CONTENT`               | `iris_doc` insert/delete_lines `expected` field didn't match stored content                          |
| `STORAGE_STRIP_BLOCKED`       | `iris_doc mode=put` would strip a Storage block — pass `allow_storage_regeneration: true` to proceed |
| `CODE_EDIT_BLOCKED`           | `iris_execute` call matched a code-editing pattern — use `iris_doc` + `iris_compile`                 |
| `CHECKIN_BLOCKED`             | SCM CheckIn called without `IRIS_SCM_ALLOW_CHECKIN=1`                                                |
| `HTTP_EXECUTION_FAILED`       | Atelier HTTP call failed — check host, port, credentials                                             |
| `IRIS_UNREACHABLE`            | No IRIS connection discoverable — run `check_config`                                                 |
| `INTEROP_ERROR`               | Ensemble/interop HTTP call failed — check production state and container access                      |
| `WS_TERMINAL_NOT_SUPPORTED`   | Atelier API version is below V7 — WebSocket terminal requires IRIS 2023.2+                           |
| `WS_SESSION_NOT_FOUND`        | Session token is invalid or already closed — call `iris_ws_open` to get a new token                  |
| `CONFIRM_REQUIRED`            | `global_kill` requires a `confirm_token` from `global_preview`                                       |
| `CONFIRM_EXPIRED`             | Confirmation token is older than 5 minutes — call `global_preview` again                             |
| `CONFIRM_MISMATCH`            | Token was issued for a different global or server                                                    |
| `WRITE_TOOLS_DISABLED`        | Write tool called without `write_tools_enabled = true` in `.iris-agentic-dev.toml`                   |
| `DESTRUCTIVE_TOOLS_DISABLED`  | Destructive tool (☠) called without `destructive_tools_enabled = true`                               |
| `DESTRUCTIVE_REQUIRES_WRITES` | `destructive_tools_enabled = true` set while `write_tools_enabled = false` — invalid config          |
| `WRITE_SERVER_NOT_ALLOWED`    | Write directed to a server not in `write_allowed_servers`                                            |
| `FETCH_FAILED`                | `compare_document` could not fetch source from one or both servers                                   |
| `HL7_NOT_AVAILABLE`           | `EnsLib.HL7.Schema` not installed on this instance                                                   |
