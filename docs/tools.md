# Tools

Most tools work over the Atelier REST API and connect to any IRIS instance — no Docker
required unless noted. Tools marked ✦ require `IRIS_CONTAINER`. Tools marked 🔒 are
write-gated: they run when the target server's write tier is on, which by default it is not on a
Live instance (see [Write protection](#write-protection)). Tools marked ☠ are
destructive-gated — they require `destructive_tools_enabled = true` in addition to the write
tier.

`namespace` defaults to `"USER"` on every tool. It is omitted from parameter tables below
unless there is something non-obvious to say about it.

---

## Quick reference

| Tool                                                                                                                      | Section                   |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| [`iris_servers`](#iris_servers)                                                                                           | Server Management         |
| [`iris_add_server`](#iris_add_server)                                                                                     | Server Management         |
| [`iris_remove_server`](#iris_remove_server-) ☠                                                                            | Server Management         |
| [`iris_reload_pool`](#iris_reload_pool)                                                                                   | Server Management         |
| [`iris_test_server`](#iris_test_server)                                                                                   | Server Management         |
| [`iris_import_servers`](#iris_import_servers)                                                                             | Server Management         |
| [`iris_doc`](#iris_doc)                                                                                                   | Code                      |
| [`iris_compile`](#iris_compile)                                                                                           | Code                      |
| [`iris_check`](#iris_check)                                                                                               | Code                      |
| [`iris_execute`](#iris_execute)                                                                                           | Code                      |
| [`iris_execute_method`](#iris_execute_method)                                                                             | Code                      |
| [`iris_query`](#iris_query)                                                                                               | Code                      |
| [`iris_test`](#iris_test)                                                                                                 | Code                      |
| [`iris_coverage`](#iris_coverage)                                                                                         | Code                      |
| [`iris_global`](#iris_global-) 🔒                                                                                         | Code                      |
| [`iris_source_control`](#iris_source_control-) ✦                                                                          | Code                      |
| [`iris_symbols`](#iris_symbols)                                                                                           | Search and introspection  |
| [`iris_doc_search`](#iris_doc_search)                                                                                     | Search and introspection  |
| [`iris_symbols_local`](#iris_symbols_local)                                                                               | Search and introspection  |
| [`docs_introspect`](#docs_introspect)                                                                                     | Search and introspection  |
| [`iris_search`](#iris_search)                                                                                             | Search and introspection  |
| [`iris_info`](#iris_info)                                                                                                 | Search and introspection  |
| [`iris_macro`](#iris_macro)                                                                                               | Search and introspection  |
| [`iris_table_info`](#iris_table_info)                                                                                     | Search and introspection  |
| [`resolve_dynamic_dispatch`](#resolve_dynamic_dispatch)                                                                   | Search and introspection  |
| [`extract_message_map_routing`](#extract_message_map_routing)                                                             | Search and introspection  |
| [`find_subclass_implementations`](#find_subclass_implementations)                                                         | Search and introspection  |
| [`iris_debug`](#iris_debug)                                                                                               | Debugging                 |
| [`iris_get_log`](#iris_get_log)                                                                                           | Debugging                 |
| [`check_config`](#check_config)                                                                                           | Debugging                 |
| [`iris_generate`](#iris_generate)                                                                                         | Generation                |
| [`iris_generate_class`](#iris_generate_class)                                                                             | Generation                |
| [`iris_generate_test`](#iris_generate_test)                                                                               | Generation                |
| [`iris_production`](#iris_production-) ✦                                                                                  | Interoperability          |
| [`iris_interop_query`](#iris_interop_query-) ✦                                                                            | Interoperability          |
| [`iris_production_item`](#iris_production_item-) 🔒                                                                       | Interoperability          |
| [`iris_production_diff`](#iris_production_diff)                                                                           | Interoperability          |
| [`iris_message_body`](#iris_message_body)                                                                                 | Interoperability          |
| [`iris_business_rule_info`](#iris_business_rule_info)                                                                     | Interoperability          |
| [`iris_credential_list`](#iris_credential_list)                                                                           | Interoperability          |
| [`iris_credential_manage`](#iris_credential_manage--) 🔒 ☠                                                                | Interoperability          |
| [`iris_lookup_manage`](#iris_lookup_manage-) ☠                                                                            | Interoperability          |
| [`iris_lookup_transfer`](#iris_lookup_transfer)                                                                           | Interoperability          |
| [`iris_ws_open`](#iris_ws_open)                                                                                           | WebSocket sessions        |
| [`iris_ws_exec`](#iris_ws_exec)                                                                                           | WebSocket sessions        |
| [`iris_ws_close`](#iris_ws_close)                                                                                         | WebSocket sessions        |
| [`global_preview`](#global_preview)                                                                                       | Administration            |
| [`global_kill`](#global_kill--) 🔒 ☠                                                                                      | Administration            |
| [`iris_namespace_list`](#iris_namespace_list)                                                                             | Administration            |
| [`iris_namespace_create`](#iris_namespace_create--) 🔒 ☠                                                                  | Administration            |
| [`iris_database_list`](#iris_database_list)                                                                               | Administration            |
| [`iris_mirror_status`](#iris_mirror_status)                                                                               | Administration            |
| [`iris_system_performance`](#iris_system_performance)                                                                     | Administration            |
| [`iris_database_stats`](#iris_database_stats)                                                                             | Administration            |
| [`journal_search`](#journal_search)                                                                                       | Administration            |
| [`query_audit_log`](#query_audit_log)                                                                                     | Administration            |
| [`stream_inspect`](#stream_inspect)                                                                                       | Administration            |
| [`my_access`](#my_access)                                                                                                 | Administration            |
| [`capability_matrix`](#capability_matrix)                                                                                 | Administration            |
| [`hl7_schema_list`](#hl7_schema_list)                                                                                     | Administration            |
| [`hl7_schema_inspect`](#hl7_schema_inspect)                                                                               | Administration            |
| [`mermaid_class`](#mermaid_class)                                                                                         | Administration            |
| [`mermaid_production`](#mermaid_production)                                                                               | Administration            |
| [`resolve_storage`](#resolve_storage)                                                                                     | Administration            |
| [`compare_document`](#compare_document)                                                                                   | Administration            |
| [`compare_namespace`](#compare_namespace)                                                                                 | Administration            |
| [`iris_admin`](#iris_admin-) ☠                                                                                            | Administration            |
| [`iris_containers`](#iris_containers-) ✦                                                                                  | Administration            |
| [`skill`](#skill)                                                                                                         | Skills and knowledge base |
| [`skill_list` / `skill_search` / `skill_describe` / `skill_forget`](#skill_list-skill_search-skill_describe-skill_forget) | Skills and knowledge base |
| [`skill_community`](#skill_community)                                                                                     | Skills and knowledge base |
| [`kb` / `kb_index` / `kb_recall`](#kb--kb_index--kb_recall)                                                               | Skills and knowledge base |
| [`agent_history` / `agent_stats`](#agent_history--agent_stats)                                                            | Skills and knowledge base |
| [`telemetry_query`](#telemetry_query)                                                                                     | Skills and knowledge base |
| [`telemetry_export_trace`](#telemetry_export_trace)                                                                       | Skills and knowledge base |

---

## Parameters are declared, and the set is closed

Every tool advertises its parameters in its `inputSchema`: names, JSON types, and — where
the handler branches on a fixed set of values — an `enum`. 28 parameters across 24 tools
carry one, `iris_admin.action` (25 values) being the largest. The parameter tables in this
file describe the same contract in prose; the schema is what a client can read without
parsing English.

Every declared `enum` is the set the handler branches on, checked against the source rather
than against these tables — a test extracts the string literals each dispatcher matches on
and compares them with the schema. Two parameters have no branch literal behind them and
say so in the test with a reason: `iris_add_server.scheme`, which is interpolated into a
URL, and `iris_admin.type`, which filters the webapp list case-insensitively against a name
derived from IRIS. Declaring an enum changes nothing at runtime: a value outside the set
still reaches the handler and still comes back with the handler's own message naming the
values it accepts.

Every tool also sets `additionalProperties: false`. A parameter the tool does not declare
is rejected with `UNKNOWN_PARAMETER` and the error lists the names it does accept. Through
1.3.2, 31 tools advertised an open object with no properties at all, so a misspelled or
invented parameter was dropped in silence — `stream_inspect` was documented with a
character cap that no code read, and a caller asking for 10,000 characters got the whole
stream and no warning. The cap is real now (`max_chars`, spec 141): a handler reads it, the
schema declares it, and a test in each place fails if one of the three goes.

Six tools take no parameters at all and declare an empty property set: `agent_stats`,
`check_config`, `iris_import_servers`, `iris_reload_pool`, `skill_community_list`,
`skill_list`.

---

## Parameters every gated tool takes

Two parameters appear on most tools and are described here once instead of in every table.

| Parameter | Type   | Notes                                                                                                                                                                                                                                                                            |
| --------- | ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `server`  | string | Run the call on a named registered server instead of the default. Every limit on the call (gates, policy, role, service account, namespace) is judged against that server, and a name that is not in the pool is refused before any gate runs. Use `iris_servers` to list names. |
| `confirm` | bool   | Acknowledges a write on a `subject` server (`SUBJECT_CONFIRM_REQUIRED`) or a destructive call that asks for confirmation. It does nothing on other servers. `iris_execute` and `iris_ws_exec` also accept the older spelling `confirmed`.                                        |

`confirm` is an acknowledgement: it lifts a check meant to catch the agent's mistakes, not one
meant to hold against the agent. See [Refusals](#refusals). `force` is not an acknowledgement; a
forced call is judged by what it actually runs.

---

## Tool catalog size

This server exposes 81 to 85 tools depending on toolset (`IRIS_TOOLSET=baseline|nostub|merged`),
with full schemas and descriptions: 94 KB to 107 KB of JSON, roughly 26K to 30K tokens, if a
client loads the whole catalog on every connection. Two independent ways to avoid paying that
cost, and you don't have to pick just one:

- **Client-side, zero server changes needed**: Anthropic's [Tool Search
  Tool](https://docs.claude.com/en/docs/agents-and-tools/tool-use/tool-search-tool)
  (`tool_search_tool_bm25_20251119`/`_regex_20251119`) lets a model discover tools on demand
  instead of front-loading the full catalog. It already works against this server's MCP surface
  unmodified — enabling it on the client is the whole change.
- **Server-side**: `list_tools` supports real cursor-based pagination. A plain, unconfigured
  `tools/list` call still returns the entire effective toolset in one response (the default page
  size is set above every toolset's real tool count, so nothing changes for existing clients).
  Set `IRIS_LIST_TOOLS_PAGE_SIZE` to a smaller value to page the catalog across multiple
  `tools/list` calls instead — each response includes a `nextCursor` when more tools remain;
  omit `cursor` on the first call and pass back whatever `nextCursor` you last received to get
  the next page, until a response with no `nextCursor` signals the end.

---

## Discovery from a shell

`tool --list` prints one line per tool — name and a one-sentence summary. `tool <name>
--schema` prints one tool's full description and `inputSchema`. Both read the same router
`tools/list` reads, so the schema you get from the CLI is byte-identical to the schema an MCP
client receives, and a test compares all 81 for every build.

Neither path connects to IRIS. Discovery works with no container running, no credentials, and
a closed port:

```bash
iris-agentic-dev tool --list                      # 81 names + summaries
iris-agentic-dev tool --list --json               # same, as {"count": 81, "tools": [...]}
iris-agentic-dev tool iris_query --schema         # one tool's contract
iris-agentic-dev tool iris_query --schema --json  # same, as one JSON document
```

`--json` works on both paths. `--list` with a tool name is an error rather than a silent
preference for one or the other.

Why it exists — measured against this tree:

| Reading the surface via              | Bytes   |
| ------------------------------------ | ------- |
| MCP `tools/list` (81 tools, compact) | 106,658 |
| `tool --list`                        | 7,648   |
| `tool <name> --schema`, smallest     | 231     |
| `tool <name> --schema`, median       | 1,357   |
| `tool <name> --schema`, largest      | 10,134  |

An agent with only a shell reads the whole surface for 7.6 KB and then pays for the one or two
schemas it actually needs, instead of 107 KB up front. A wrong name is refused with the nearest
accepted name, the same suggestion `UNKNOWN_PARAMETER` uses for misspelled parameters.

`IRIS_TOOLSET` applies to `--list`, so the CLI lists the tier the server would serve.

---

## Server Management

Tools for registering, testing, and managing IRIS server connections. All other tools
accept an optional `server` parameter that routes to a named instance — see `iris_servers`
to list what's registered.

### `iris_servers`

List all registered IRIS instances from all configuration sources (iad-native config, VS
Code Server Manager settings, workspace fleet config, environment variables). Shows name,
host, port, namespace, source, and reachability status.

Every entry carries `base_url`, the URL iad actually calls. Entries served under a web path
prefix also carry `web_prefix` (the key is absent when there is no prefix). Two instances
behind the same gateway share a host and port, so `base_url` is the field that tells them
apart.

| Parameter | Type | Default | Notes                               |
| --------- | ---- | ------- | ----------------------------------- |
| `probe`   | bool | `false` | Probe every server for reachability |

Default (`probe` omitted or `false`): fast path — `reachable` is `null` per entry, no HTTP
requests made.

Pass `probe: true` to fan out a parallel Atelier REST probe to every server in the pool
(5-second timeout each). Each entry then includes `reachable`, `auth`, `latency_ms`,
`iris_version`, `atelier_version`, and `error`. The probe requests each entry's own
`base_url`; through 1.4.1 it rebuilt a URL from host and port, so a prefixed instance was
reported healthy on the strength of the gateway root answering.

### `iris_add_server`

Register a new IRIS instance. Writes the server's details to
`~/.config/iris-agentic-dev/servers.json` and stores the password in the OS keychain, under
iad's own service (`iris-agentic-dev`). The entry in `servers.json` holds no password when the
keychain took it. The keychain account includes the server's address, so a password saved for
one address cannot be found for another. iad never writes to or deletes from the Server Manager's
keychain service.

| Parameter          | Type   | Default  | Notes                                                                                                                                    |
| ------------------ | ------ | -------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `name`             | string | —        | Referenced by the `server` param of every other tool. A name that is a Server Manager profile is refused with `NAME_CONFLICT`            |
| `host`             | string | —        | Hostname or IP                                                                                                                           |
| `port`             | number | —        | Web port                                                                                                                                 |
| `namespace`        | string | —        | Default namespace for calls to this server                                                                                               |
| `username`         | string | —        | IRIS username                                                                                                                            |
| `password`         | string | —        | Goes to the keychain; on a host with no keychain, to `servers.json` (owner-only), tied to the address                                    |
| `description`      | string | none     | Free text                                                                                                                                |
| `scheme`           | string | `"http"` | `"http"` or `"https"`; anything else is `INVALID_PARAMS`                                                                                 |
| `web_prefix`       | string | none     | Path prefix the web server is served under, e.g. `/hs20261`                                                                              |
| `container`        | string | none     | The Docker container of this server: the only one a `docker exec` fallback may use for it                                                |
| `service_username` | string | none     | The restricted account the agent's code runs as on this server; see [Service account](connecting.md#service-account-for-the-agents-code) |
| `service_password` | string | none     | The service account's password, stored the way `password` is                                                                             |

The response says where the password went: `credential_store` is `keychain`, `file` or `none`,
and `stored_plaintext: true` with a warning marks the headless fallback. Registering a name again
at a different address drops the password the old address had (`password_cleared: true`), so a
moved entry never sends it to a new host; a re-registration at the same address keeps the stored
password. See [Adding servers](connecting.md#adding-servers-via-iris_add_server).

Pass `web_prefix` when the instance sits behind a shared web gateway and Atelier answers at
`/<prefix>/api/atelier/` rather than at `/api/atelier/`. HealthShare and multi-instance
gateway deployments work this way. It takes a path only — a value carrying a scheme or a
host is refused with `INVALID_PARAMS`, since concatenating it onto the base URL would
produce a nonsense address that fails much later as a connection error. Leading and
trailing slashes are optional: `hs20261`, `/hs20261`, `hs20261/`, and `/hs20261/` all
resolve to the same URL. Multi-segment prefixes (`/gw/hs20261`) work.

`webServer.pathPrefix` is accepted as an alias when reading `servers.json` by hand, so a
block copied out of VS Code settings loads without renaming the key.

After adding a server, call `iris_reload_pool` for the new connection to appear in the
running pool. No restart is needed.

### `iris_remove_server` ☠

Remove a server from the iad-native config and iad's own keychain item for it (never the
Server Manager's). Requires `destructive_tools_enabled = true`. Cannot remove servers sourced
from VS Code settings, the environment or a fleet file (`REMOVE_NOT_ALLOWED`) — edit those
sources directly.

| Parameter | Type   | Default | Notes                                      |
| --------- | ------ | ------- | ------------------------------------------ |
| `name`    | string | —       | **Required.** Name of the server to remove |

### `iris_reload_pool`

Hot-reload the connection pool from disk without restarting iad. Re-reads the operator file,
the workspace file, their trust state and the iad-native servers list, then atomically replaces
the in-memory pool.
Returns `{success, servers_loaded, servers, note}`. Parse errors preserve the existing pool
and return `{success: false, error_code: "TOML_PARSE_ERROR", ...}`.

This is a write: it makes every server declared since startup routable, so it needs the write
gate open on the default server (`WRITE_TOOLS_DISABLED` otherwise) and is refused under
`read_only = "strict"`. It is an `admin` call for `allow` lists.

**Background reload**: iad also reloads the pool automatically when the workspace TOML
changes on disk. The change is detected via file mtime on each call to `iris_servers`,
`iris_compile`, `iris_execute`, and similar tools. A parse error during background reload
is silently swallowed — the existing pool is kept.

### `iris_test_server`

Probe an IRIS server for reachability. Returns `reachable`, `auth`, `iris_version`,
`atelier_version`, and `latency_ms`.

| Parameter  | Type   | Default   | Notes                                               |
| ---------- | ------ | --------- | --------------------------------------------------- |
| `name`     | string | —         | A registered server; mutually exclusive with `host` |
| `host`     | string | —         | Ad-hoc probe of a server not in the pool            |
| `web_port` | number | `52773`   | Ad-hoc probe only                                   |
| `username` | string | `_SYSTEM` | Ad-hoc probe only                                   |
| `password` | string | `""`      | Ad-hoc probe only                                   |

Two modes:

- **Named server** (`name`): looks up the server in the connection pool and probes the URL
  that server actually uses, web path prefix included.
- **Ad-hoc** (`host`, optional `web_port`, `username`, `password`): probes a server that is
  not in the pool. Useful for discovery before calling `iris_add_server`. Default port is
  52773; default credentials are `_SYSTEM`/`""`. Returns `reachable: true, auth: false` when
  the server responds with HTTP 401 (reachable but credentials wrong). This mode has no
  prefix parameter and probes the root; register the server first to probe a prefixed path.

### `iris_import_servers`

One-time import of IRIS server definitions from VS Code or Cursor settings into the
iad-native config. It copies no password: each entry is marked `credential = "server-manager"`,
its password stays in the Server Manager's keychain item, and iad reads that item only while the
entry's address equals the profile's. Reports servers imported, skipped (already present), and
those with no keychain entry (`no_keychain`). A profile's `webServer.pathPrefix` is carried
across as `web_prefix`.

---

## Code

### `iris_doc`

Read, write, delete, insert lines, or list IRIS documents.

| Parameter                    | Type     | Default  | Notes                                                                                                         |
| ---------------------------- | -------- | -------- | ------------------------------------------------------------------------------------------------------------- |
| `mode` (alias: `action`)     | string   | `"get"`  | See modes below                                                                                               |
| `name` (alias: `document`)   | string   | —        | Document name, e.g. `"MyApp.MyClass.cls"`                                                                     |
| `names`                      | string[] | `[]`     | Batch get/delete                                                                                              |
| `content`                    | string   | —        | Document content for `put`/`insert`                                                                           |
| `compile`                    | bool     | `false`  | Compile after `put`                                                                                           |
| `start`                      | int      | —        | Start line for `get` (fragment) or `delete_lines`                                                             |
| `end`                        | int      | —        | End line for `get` (fragment) or `delete_lines`                                                               |
| `line`                       | int      | —        | Insert-before line for `insert`                                                                               |
| `expected`                   | string   | —        | CAS guard: current content at `start`–`end`; fails with `STALE_CONTENT` if mismatch                           |
| `pattern`                    | string   | —        | Glob filter for `list`, e.g. `"MyApp.*.cls"`                                                                  |
| `category`                   | string   | —        | `"CLS"` \| `"MAC"` \| `"INT"` \| `"INC"` \| `"ALL"`                                                           |
| `max_results`                | int      | `200`    | Max 1000; for `list`                                                                                          |
| `compiled_type`              | string   | `"INT"`  | `"INT"` is the only value; for `compiled` mode. OBJ returns `INVALID_PARAMS`                                  |
| `allow_storage_regeneration` | bool     | `false`  | Confirms a `put` that drops a class's existing Storage block                                                  |
| `elicitation_id`             | string   | —        | SCM checkout dialog resume ID                                                                                 |
| `elicitation_answer`         | string   | —        | SCM checkout dialog answer                                                                                    |
| `include_scratch`            | bool     | `false`  | `list`: include iad's own `IrisDevTmp.*` scratch classes (hidden unless the pattern starts with `IrisDevTmp`) |
| `confirm`                    | bool     | `false`  | Acknowledges a write on a `subject` server                                                                    |
| `server`                     | string   | —        | Named server; omit for default                                                                                |
| `namespace`                  | string   | `"USER"` |                                                                                                               |

**Modes:**

| Mode           | What it does                                                    |
| -------------- | --------------------------------------------------------------- |
| `get`          | Read document content                                           |
| `put`          | Write document content (SCM-gated; Storage is written as given) |
| `delete`       | Delete document                                                 |
| `head`         | Return metadata only (size, timestamp) without content          |
| `fragment`     | Read lines `start`–`end`                                        |
| `compiled`     | Read compiled INT or OBJ source                                 |
| `list`         | List documents matching `pattern`/`category`                    |
| `insert`       | Insert `content` before `line` (use `expected` for a CAS check) |
| `delete_lines` | Delete lines `start`–`end` (use `expected` for a CAS check)     |

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

**Storage block guard.** `iris_doc` writes a Storage block exactly as given; IRIS 2025.3 and
2026.2 both accept one in a PUT and keep it through the compile. What it refuses, with
`STORAGE_RESET_REQUIRES_CONFIRMATION`, is a `put` whose content has no Storage block for a class
that has one on the server: compiling that makes IRIS regenerate the storage from scratch, which
can misalign existing rows of a `%Persistent` class. Pass `allow_storage_regeneration: true` only
when the reset is intended; the response then lists the properties the old storage held. Use
`mode=insert` or `mode=delete_lines` when the edit does not touch the Storage block.

**Compile-time gate.** A `put` of a `.cls`, `.mac` or `.inc` document is read for constructs IRIS
runs while the document compiles (preprocessor directives, braced class parameters, projections,
class XML). A match is refused with `COMPILE_TIME_EXEC_BLOCKED`, as an error result, before
anything is written; with `compile=true`, the stored `.inc` files the document includes are
read and checked too, and a class whose out-of-date superclass IRIS would compile with it is
refused with `COMPILE_SUPERCLASS_STALE` after the write (see `iris_compile`). The switch is the
target server's; see
[Compile-time gate](connecting.md#compile-time-gate).

**SCM checkout.** On source-controlled instances, `iris_doc` runs the SCM pre-write
check before writing. If checkout is required, the tool returns an elicitation dialog
rather than writing. Resume it with `elicitation_id` + `elicitation_answer`.

---

### `iris_compile`

Compile a class, routine, or wildcard pattern.

| Parameter        | Type   | Default  | Notes                                                                                                             |
| ---------------- | ------ | -------- | ----------------------------------------------------------------------------------------------------------------- |
| `target`         | string | —        | **Required.** Class name, routine, glob like `"MyApp.*.cls"`, or a local source file inside the project directory |
| `flags`          | string | `"cuk"`  | Compile flags                                                                                                     |
| `namespace`      | string | `"USER"` |                                                                                                                   |
| `force_writable` | bool   | `false`  | Override read-only check                                                                                          |
| `inline`         | bool   | `false`  | Return all errors inline (bypass log store)                                                                       |
| `confirm`        | bool   | `false`  | Acknowledges a compile on a `subject` server                                                                      |
| `server`         | string | —        | Named server; omit for default                                                                                    |

A `target` with a path separator is a file on the machine iad runs on: iad uploads it, then
compiles it. It must be a `.cls`, `.mac`, `.inc` or `.int` file inside the
[project directory](#project-directory), and is refused with `HOST_PATH_BLOCKED` before it is read
when it is not. The compile hook passes an absolute path inside the project, which is allowed.

Returns errors with line numbers. Before IRIS compiles a target by name, iad reads its stored
source and the `.inc` files it includes and applies the compile-time gate to them. A stored
document that exists but cannot be read is refused with `STORED_SOURCE_UNREADABLE`; a wildcard
whose members cannot be listed is refused the same way.

`flags` takes the letters `c`, `d`, `k` and `u` (with `-` to turn one off) and the qualifiers
`/checkuptodate`, `/display`, `/displayerror`, `/displaylog`, `/keepsource`, `/quiet`, `/silent` and
`/verbose`. Anything else is refused with `INVALID_PARAMS`. IRIS compiles more than the target when
told to (`b` and `/subclasses` add every subclass, `r` and `/predecessorclasses` the classes it
depends on, `y` and `/relatedclasses` the ones it shares SQL with, `/expand` all three), and iad has
read only the target and its includes; a subclass stored some other way would run its compile-time
code unread. Name the classes you want compiled, or a wildcard, which iad lists and reads.

IRIS also compiles a target's out-of-date superclasses with it, and those would run their
compile-time code unread. Before it compiles, iad walks the `Extends` chain of each class (multiple
inheritance included) on the target server. A superclass outside the library packages that IRIS
would recompile is refused with `COMPILE_SUPERCLASS_STALE`, which names it: compile that
superclass first with `iris_compile`, where it is checked like any other class, then compile the
original target. `iris_doc` with `compile=true` and a local file given as `target` store the class
first, then run the same check, so the class stays written and nothing is compiled. A walk that
cannot run is refused with `SUPERCLASS_CHECK_FAILED`. See the
[compile-time gate](connecting.md#compile-time-gate) for the library packages.

```text
iris_compile(target="MyApp.MyClass.cls")
iris_compile(target="MyApp.*.cls", flags="cukd")
```

---

### `iris_check`

Check an ObjectScript document before compilation, powered by
[intersystems-community/ascot](https://github.com/intersystems-community/ascot), a static analyzer
and flow-sensitive type checker.

| Parameter   | Type   | Default | Notes                                                                                         |
| ----------- | ------ | ------- | --------------------------------------------------------------------------------------------- |
| `name`      | string | —       | **Required.** The document with its extension: `Pkg.Class.cls`, `Routine.mac`, `.int`, `.inc` |
| `source`    | string | —       | The document's text, to check code before saving it; omit to check it as stored on the server |
| `namespace` | string | —       | Namespace that resolves the document's references; defaults to the connection's               |
| `server`    | string | —       | Named server; omit for default                                                                |

Classes, routines and `.inc` files the document refers to but does not define are read from the
target server with read-only Atelier requests; nothing runs on IRIS. Without a connection they are
reported as hints and the response carries a `note` saying so; without a connection and without
`source` there is nothing to check, and the call is refused with `IRIS_UNREACHABLE`. A `name` the
server does not hold answers `DOCUMENT_NOT_FOUND`.

Returns LSP-style diagnostics (zero-based `range`, `severity`, `code`, `code_description`) with
`errors`, `warnings` and `hints` counts, and `iris` saying whether a connection answered the lookups.

```text
iris_check(name="MyApp.Service.cls")
iris_check(name="MyApp.Service.cls", source="Class MyApp.Service ...")
```

---

### `iris_execute`

Run arbitrary ObjectScript and return the output.

| Parameter       | Type   | Default  | Notes                                                                                               |
| --------------- | ------ | -------- | --------------------------------------------------------------------------------------------------- |
| `code`          | string | —        | **Required.** ObjectScript to execute                                                               |
| `namespace`     | string | `"USER"` |                                                                                                     |
| `timeout`       | int    | `120`    | Seconds; overridden by `OBJECTSCRIPT_TEST_TIMEOUT` env var                                          |
| `translate_sql` | bool   | `true`   | Rewrite `&sql(...)` macros to `%SQL.Statement` on the docker exec path; HTTP runs `&sql` as written |
| `use_session`   | bool   | `false`  | Enable `%ctx` session carrier (see below)                                                           |
| `session_state` | string | —        | Token from a prior call; restores `%ctx`                                                            |
| `confirm`       | bool   | `false`  | Acknowledges a write on a `subject` server (`confirmed` is the older spelling)                      |
| `server`        | string | —        | Named server; omit for default                                                                      |

```text
iris_execute(code="Write $ZVersion")
iris_execute(code="Set sc = ##class(MyApp.Util).Run() Write sc", namespace="MYAPP")
```

**Code-edit guard** — see [Code-edit guard](#code-edit-guard) below.

**Who runs the code** — every response carries an `identity` object,
`{user, source, path, verified, server}`: the IRIS account the code actually ran as, where that
account came from (`service_account_env`, `service_account_server`, `primary` or
`os_user_opt_in`), the path it ran on (`atelier`, `docker_exec_local`, `docker_exec_ssh` or
`ws_terminal`), and whether `user` is what IRIS reported (`$USERNAME`) rather than what iad sent.
With `IRIS_SERVICE_USERNAME` set (or a `service_username` on the server named with `server`), the
code runs as that account on every path, or the call is refused with a `SERVICE_ACCOUNT_*`
code; it never quietly runs as the connection account. `auth_user` and `service_account_env`
repeat part of it and go away in 1.8.0. The same `identity` block comes back from `iris_query`
(`write`/`force`), `iris_global` (`set`/`kill`), `iris_execute_method`, `global_kill`,
`iris_test`, `iris_coverage` (`run`/`start`), `iris_ws_open` and `iris_ws_exec`. See
[Service account](connecting.md#service-account-for-the-agents-code) for setup and
[Troubleshooting](troubleshooting.md#service_account_-errors) for each error.

**Destructive-tier gate** — `iris_execute` requires `write_tools_enabled = true` (it is
write-gated). Additionally, if the literal code string contains a `Kill ^<global>` pattern
(any case variant), `destructive_tools_enabled = true` is also required. This check catches
direct global kills written literally in the code. Indirect operations — `Kill @variable`,
`Xecute`-dispatched code, or class method calls that internally kill a global — are not
detected; the service account's IRIS privileges and the `mcpTemplate` environment gate are the
appropriate controls for those.

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

#### Execution paths and terminal-mode constraint

`iris_execute` has two execution paths:

1. **HTTP (primary)** — wraps code in a temporary class method body, compiles it via
   Atelier REST, runs it, deletes the class. Block syntax (`{}`) works here because class
   method bodies support it.

2. **docker exec (fallback)** — used when `IRIS_CONTAINER` is set and HTTP fails, or when
   `docker_only = true` in config. This path pipes code into `iris session` stdin, which is
   a line-by-line terminal interpreter. **Block syntax is not supported in terminal mode.**
   `If cond { Write x }` causes `<SYNTAX>` with no explanation.

Use classic terminal-compatible form for the docker exec path:

```text
// Terminal-compatible: classic form
If cond  Write x

// Terminal-compatible: dotted-DO
If cond  Do
.  Write x

// NOT terminal-compatible: block syntax
If cond {
    Write x        // causes <SYNTAX> on docker exec path
}
```

**Escape hatch for complex scripts on the docker exec path**: write a `.mac` routine with
`iris_doc`, compile it with `iris_compile`, then call `iris_execute Do entry^RoutineName`.
Block syntax works inside compiled routines.

```text
// Step 1: write the routine
iris_doc(mode="put", name="MyScript.mac", content="ROUTINE MyScript\nentry()\n    If x=1 {\n        Write \"yes\"\n    }\n    Quit")

// Step 2: compile it
iris_compile(target="MyScript.mac")

// Step 3: run it
iris_execute(code="Do entry^MyScript")
```

Error code `TERMINAL_SYNTAX_UNSUPPORTED` is returned when block syntax is detected before
the docker exec round-trip, with an actionable message pointing to this pattern.

---

### `iris_execute_method`

Invoke a `ClassMethod` directly by class, method name, and arguments.

| Parameter   | Type     | Default  | Notes                                      |
| ----------- | -------- | -------- | ------------------------------------------ |
| `class`     | string   | —        | **Required.** e.g. `"%Library.Integer"`    |
| `method`    | string   | —        | **Required.** e.g. `"IsValid"`             |
| `args`      | string[] | `[]`     | Positional string arguments                |
| `namespace` | string   | `"USER"` |                                            |
| `confirm`   | bool     | `false`  | Acknowledges a write on a `subject` server |
| `server`    | string   | —        | Named server; omit for default             |

String-returning methods only (v1). A method that returns an error `%Status` answers
`success: false`, `error_code: METHOD_RETURNED_ERROR`, with the status text from
`$System.Status.GetErrorText`.

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
| `confirm`           | bool     | `false`  | Acknowledges a write on a `subject` server           |
| `server`            | string   | —        | Named server; omit for default                       |

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

**What read mode refuses.** `mode="read"` runs one statement that starts with `SELECT`, `WITH` or
`EXPLAIN`, the first word outside comments. It refuses everything else before IRIS sees the query:
`CALL`, `TUNE TABLE`, `BUILD INDEX`, `PURGE`, `SET OPTION`, `FREEZE PLANS`, DML, DDL, `GRANT` and
`REVOKE`, and a second statement after a `;` (`SQL_WRITE_BLOCKED`, a fixed rule: nothing in the call
or the configuration lifts it, and the message points at `mode="write"`, which the write gate
judges and which runs `INSERT`, `UPDATE`, `DELETE`, `CALL` and `TRUNCATE`). A stored procedure that
writes or runs ObjectScript still runs in read mode when a `SELECT` calls it (`SELECT Pkg.Proc()`):
no check on the query text can tell that call from `SELECT LENGTH(x)`, and read mode runs as the
connection account, not the service account. If read mode has to be safe, connect as an IRIS user
without `EXECUTE` on such procedures. `force=true` skips the check only when the target server's
write tier is on, and the live and test templates then treat the call as a write. `mode="count"`
takes one `SELECT` (no `;`) and a `table` that is a table name, under the same rules.

`mode="write"` runs as the service account when one is configured, on the server the call names,
and returns `identity` like the other code-running tools.

---

### `iris_test`

Run `%UnitTest` tests and return structured pass/fail results.

| Parameter             | Type     | Default  | Notes                                                                                                                      |
| --------------------- | -------- | -------- | -------------------------------------------------------------------------------------------------------------------------- |
| `pattern`             | string   | —        | **Required.** Compiled class or package pattern, e.g. `"App.Tests"`. A `..`, a `/` or `\` is refused with `INVALID_PARAMS` |
| `namespace`           | string   | `"USER"` |                                                                                                                            |
| `timeout`             | int      | `60`     | Seconds                                                                                                                    |
| `coverage`            | bool     | —        | Also measure line coverage inline                                                                                          |
| `coverage_classes`    | string[] | —        | Explicit class list for coverage; defaults to all classes in `pattern`                                                     |
| `coverage_target_pct` | float    | —        | Fail if coverage falls below this threshold                                                                                |
| `test_type`           | string   | `"auto"` | `"auto"` \| `"testcase"` \| `"testproduction"`: `auto` detects `%UnitTest.TestProduction` subclasses and uses `.Run()`     |
| `confirm`             | bool     | `false`  | Acknowledges a run on a `subject` server                                                                                   |
| `server`              | string   | —        | Named server; omit for default                                                                                             |

```text
iris_test(pattern="MyApp.Tests")
iris_test(pattern="MyApp.Tests", coverage=true, coverage_target_pct=80)
```

`iris_test` makes no directory on the IRIS host. It used to create `/tmp/httest/<pattern>/` for a
pattern with a separator; any local user on that host could write there, and the pattern was
spliced in without a `..` check. `iris_coverage` takes `test_path` through the same rule.

---

### `iris_coverage`

Standalone line coverage via `%Monitor.System.LineByLine`.

| Parameter        | Type     | Default  | Notes                                                                                       |
| ---------------- | -------- | -------- | ------------------------------------------------------------------------------------------- |
| `mode`           | string   | —        | **Required.** See modes below                                                               |
| `classes`        | string[] | —        | Classes to monitor; used with `start`/`run`                                                 |
| `package`        | string   | —        | Package prefix — alternative to explicit `classes`                                          |
| `test_path`      | string   | —        | `%UnitTest` class pattern to run (same rule as `iris_test`'s `pattern`); required for `run` |
| `target_pct`     | float    | —        | Fail threshold %                                                                            |
| `cobertura_path` | string   | —        | Write Cobertura XML here (requires TestCoverage IPM)                                        |
| `namespace`      | string   | `"USER"` |                                                                                             |
| `confirm`        | bool     | `false`  | Acknowledges a run on a `subject` server                                                    |
| `server`         | string   | —        | Named server; omit for default                                                              |

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

Every mode except `report` is a write and needs the write gate open on the target server,
`check` included: it starts the line monitor to see that it can start, and stops it again.
`report` only reads the counters back.

Requires `gmheap ≥ 256 MB`. Run `mode=check` first. If `BBSIZ_NOT_CONFIGURED` is
returned, increase `gmheap` in Management Portal → System Administration →
Configuration → Additional Settings → Advanced Memory, then restart IRIS.

The line monitor is one per instance and belongs to whoever started it. `check`, `start` and
`run` refuse with `MONITOR_IN_USE` when one is running, and leave it running; `mode=stop` stops
it, for a monitor that is yours. A class counts over every routine it compiled to
(`MyApp.Service.0`, `MyApp.Service.1`, ...), and each entry lists them in `routines`. A class
with no compiled routine is `COVERAGE_ROUTINE_UNKNOWN`, and nothing runs. A class with 0 of 0
lines (never loaded during the run, or no executable lines) has `pct: null`, and a `null`
`total_pct` and `meets_target` when that is all there is: no lines is not 0%. If a line of the
run throws, the monitor is stopped anyway and the result carries `partial: true` and
`run_error`, with the coverage gathered before it; `meets_target` is `null` then. The count is
the executions of each line the monitor lists for the routine.

`run` and `start` run as the service account when one is set. The line monitor's state is
system-wide, so that account needs read-write access to the IRISSYS database's resource; without
it they answer `COVERAGE_PRIVILEGE_DENIED`.

---

### `iris_global` 🔒

Read, write, kill, or list IRIS global nodes. Gated — see [Data safety gates](#data-safety-gates).

| Parameter        | Type     | Default | Notes                                                    |
| ---------------- | -------- | ------- | -------------------------------------------------------- |
| `action`         | string   | —       | **Required.** `"get"` \| `"set"` \| `"kill"` \| `"list"` |
| `global_name`    | string   | —       | **Required.** With or without one leading `^`; see below |
| `subscripts`     | string[] | —       | Subscript path; values validated `[a-zA-Z0-9 _.:\-]+`    |
| `value`          | string   | —       | Required for `action=set`                                |
| `subtree`        | bool     | —       | `get` only: include all descendants                      |
| `max_nodes`      | int      | `100`   | `get` + `subtree`; max 1000                              |
| `max_subscripts` | int      | `50`    | `list` action; max 500                                   |
| `acknowledgePhi` | bool     | —       | Required when global name matches a PHI pattern          |
| `confirm`        | bool     | `false` | Acknowledges a `set` or `kill` on a `subject` server     |
| `server`         | string   | —       | Named server; omit for default                           |
| `namespace`      | string   | —       |                                                          |

```text
iris_global(action="get", global_name="^MyApp.Config", subscripts=["timeout"])
iris_global(action="list", global_name="^MyApp.Config")
iris_global(action="set", global_name="^MyApp.Config", subscripts=["timeout"], value="30")
```

**Global names.** `MyApp.Config` and `^MyApp.Config` are the same global, and every gate sees them
as the same name. After one optional `^`, a name is an optional `%`, a letter, then letters and
digits with single dots between them, up to 128 characters. Anything else is `INVALID_GLOBAL_NAME`, before any gate or IRIS
call: a subscript in the name (`MyApp("x")`), a second `^`, a space, an extended reference
(`^|"%SYS"|ROUTINE`), `$`, or a non-ASCII character. Put subscripts in `subscripts` and a
namespace in `namespace`. The same rules apply to `global_preview` and `global_kill`.

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
| `confirm`        | bool   | `false`  | Has no effect; see below                                            |
| `server`         | string | —        | Named server; omit for default                                      |
| `namespace`      | string | `"USER"` |                                                                     |

CheckIn is opt-in via `IRIS_SCM_ALLOW_CHECKIN=1`. Disconnect and Reconnect are opt-in via
`IRIS_SCM_ALLOW_DISCONNECT=1`, a separate switch: turning one on leaves the other off. Both apply
in `action=execute` and in the `menu` listing, and `check_config` reports each under
`scm_opt_ins`. Source control answers `UserAction` with a code from 0 to 7. 0 means no dialog is needed, 1 a
yes/no dialog and 7 a prompt for text (`action=execute` can take that one). 6 is the provider
declining (`SCM_REJECTED`). 2 to 5 need a window this tool cannot open (a web page, a program on
the client, text in the editor, documents in the editor) and are refused as `SCM_NEEDS_UI`; a
prompt at a call that cannot take text is `SCM_NEEDS_INPUT`. Any other number is
`SCM_ACTION_UNKNOWN` and an answer that is not a whole number, or is missing, is
`SCM_ACTION_UNPARSEABLE`. All four are fixed rules, and `iris_doc` `put` stops on them without
writing: an unreadable answer is never read as "no dialog needed". The reply is read from a
labelled record the snippet writes last, so a hook's own output before it does not change the
answer.

On a `subject` server a source-control write is always refused
(`SUBJECT_SCM_WRITE_BLOCKED`, a fixed rule); `confirm` does not lift it. Source control runs as
the connection account so that edits stay attributed to a real user.

`action=execute` with `action_id` `Diff` (or `%Diff`) is a read: it needs no write gate. Every other
id, including one iad does not know, is a write.

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

| Parameter        | Type     | Default | Notes                                                                                                      |
| ---------------- | -------- | ------- | ---------------------------------------------------------------------------------------------------------- |
| `query`          | string   | —       | **Required.** Search string                                                                                |
| `workspace_path` | string   | —       | Directory to search, inside the [project directory](#project-directory); defaults to the project directory |
| `limit`          | int      | `50`    |                                                                                                            |
| `kinds`          | string[] | —       | Filter by symbol kind                                                                                      |

```text
iris_symbols_local(query="Patient")
iris_symbols_local(query="GetStatus", workspace_path="/home/user/myapp")
```

The crawl has a budget: 200,000 directory entries, 32 levels and 10 seconds. A crawl that runs out
stops and returns `truncated: true` with `truncated_reason` set to `file_budget`, `depth_budget` or
`time_budget`. Stopping at `limit` is not truncation. A path outside the project directory is
refused with `HOST_PATH_BLOCKED`.

---

### `docs_introspect`

Deep class inspection: methods, properties, parameters, XData blocks, superclasses.
Returns `xdata_flow` for BPL and DTL classes showing the step tree.

| Parameter    | Type   | Default  | Notes         |
| ------------ | ------ | -------- | ------------- |
| `class_name` | string | —        | **Required.** |
| `namespace`  | string | `"USER"` |               |

```text
docs_introspect(class_name="MyApp.BP.PatientProcess")
docs_introspect(class_name="Ens.BusinessProcess")
```

---

### `iris_search`

Full-text search within a document scope. Supports regex and category filters.

| Parameter        | Type     | Default  | Notes                                                                        |
| ---------------- | -------- | -------- | ---------------------------------------------------------------------------- |
| `query`          | string   | —        | **Required.** Search string                                                  |
| `documents`      | string[] | —        | **Required.** Scope, e.g. `["MyApp.*.cls"]`; empty triggers `SCOPE_REQUIRED` |
| `regex`          | bool     | `false`  |                                                                              |
| `case_sensitive` | bool     | `false`  |                                                                              |
| `category`       | string   | —        | `"CLS"` \| `"MAC"` \| `"INT"` \| `"INC"` \| `"ALL"`                          |
| `namespace`      | string   | `"USER"` |                                                                              |
| `limit`          | int      | `200`    | Most matches, 1 to 10000; 200 is Atelier's own default                       |
| `inline`         | bool     | `false`  | Bypass log store                                                             |

Namespace-wide grep times out — always provide a `documents` scope.

A `regex` pattern matches anywhere in a line, as in grep; `^` and `$` anchor it to the start or
end. (Atelier itself matches a regex against the whole line, so iad wraps the pattern.)

Atelier stops at `limit` matches. When it does, the response has `limit_reached: true` and a
`warning`: there may be more matches than `results` holds. Narrow `documents` or raise `limit`.
Up to 1.6.1 iad sent no limit and did not report the cut-off, so a search could stop at 200
matches and look complete, and `regex: true` was read by Atelier as off.

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
| `name`      | string | —        | For `what=sa_schema`: an XData namespace URL, e.g. `http://www.intersystems.com/deepsee`. Not a cube name                                  |
| `namespace` | string | `"USER"` |                                                                                                                                            |
| `inline`    | bool   | `false`  |                                                                                                                                            |

```text
iris_info(what="namespace")
iris_info(what="documents", doc_type="CLS")
iris_info(what="jobs")
iris_info(what="sa_schema", name="http://www.intersystems.com/deepsee")
```

`what=sa_schema` returns the Studio Assist grammar for that XData namespace. It does not list or describe BI cubes: run `%DeepSee.Utils` `%GetCubeList` and `%GetDimensionList` through `iris_execute` for those. A name that is not a URL is refused with `INVALID_PARAMS`, and a URL with no grammar returns `SA_SCHEMA_NOT_FOUND`. While IRIS is still building a grammar (fresh instance, or a cube class compiled since the last build), iad waits up to ten seconds, then returns `SA_SCHEMA_PENDING`.

---

### `iris_doc_search`

Search the InterSystems documentation site (docs.intersystems.com) through its Algolia index.
This is the one tool that sends text to a third party: the search text the agent wrote goes to
that service, with no IRIS credentials or data. It belongs to the `docs` category.

| Parameter | Type   | Default | Notes                             |
| --------- | ------ | ------- | --------------------------------- |
| `query`   | string | —       | **Required.** Search text         |
| `version` | string | none    | e.g. `"2025.1"`                   |
| `product` | string | none    | e.g. `"InterSystems IRIS"`        |
| `hits`    | int    | `5`     | Most hits to return; capped at 10 |

Returns `{query, total_hits, hits: [{title, url, excerpt, breadcrumbs, version, product}]}`.

---

### `iris_macro`

Inspect `$$$` macros: list, signature, location, definition, expand.

| Parameter   | Type     | Default  | Notes                                                                                   |
| ----------- | -------- | -------- | --------------------------------------------------------------------------------------- |
| `action`    | string   | —        | **Required.** `"list"` \| `"signature"` \| `"location"` \| `"definition"` \| `"expand"` |
| `name`      | string   | —        | Macro name, with or without `$$$`. Required for every action but `list`                 |
| `args`      | string[] | `[]`     | Arguments for `expand`                                                                  |
| `includes`  | string[] | —        | Include files to look in, e.g. `["EnsConstants"]`. See below                            |
| `namespace` | string   | `"USER"` |                                                                                         |

System macros such as `$$$OK` resolve without an include. Other macros need the include that defines them. If you leave `includes` out, iad searches every include file in the namespace and answers from the one that defines the macro. The answer names that file in `document` and `line`. If you pass `includes`, iad looks only there.

A macro that no include defines returns `MACRO_NOT_FOUND`. It does not come back as a success with an empty result.

`action="list"` with no `includes` lists the namespace's include files. With `includes`, it lists the macros those files define themselves; a trailing `(` marks a macro that takes arguments.

```text
iris_macro(action="list")
iris_macro(action="list", includes=["EnsConstants"])
iris_macro(action="expand", name="eProductionStateRunning")      # → expansion ["1"], document EnsConstants.inc
iris_macro(action="signature", name="ISERR")                     # → signature "(x)"
iris_macro(action="expand", name="ISERR", args=["sc"])
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

| Parameter    | Type   | Default  | Notes         |
| ------------ | ------ | -------- | ------------- |
| `class_name` | string | —        | **Required.** |
| `namespace`  | string | `"USER"` |               |

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

Show the active configuration. No parameters, and it always succeeds, so it works when
everything else does not. It makes no IRIS network call, except one `GET /api/atelier/` with the
service account (sending no code) when `IRIS_SERVICE_USERNAME` is set, to report whether that
account can log in. Run this first if anything seems misconfigured.

| Field                                                              | What it says                                                                                                                                                                                                                                                                                                                                              |
| ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `server_version`, `connected`, `connection_source`, `iris_version` | Which build is running, and how the default server was found: `connection_source` is `config_file`, `env_vars`, `iris_select_container` or `auto_discovered`                                                                                                                                                                                              |
| `host`, `port`, `namespace`, `container`, `capabilities`           | The resolved default connection and what it can do (`atelier_rest`, `compile_path`, `private_web_server`, `webgateway_url`)                                                                                                                                                                                                                               |
| `config_file`, `config_watch_path`, `config_loaded_at`             | The file that chose the connection (a file names one only through `host` or `container`), the path iad watches for a new project file (never the operator file), and when it last loaded. A file that holds neither is `config_file_named_no_target`, with a `config_file_warning` saying so and that `connection_source` names what chose the connection |
| `config_sources`                                                   | `operator_file` (`path`, `loaded`) and `workspace_file` (`path`, `kind`, `named_by`, `trust`, and `changed`/`lift` when trust lapsed or is missing)                                                                                                                                                                                                       |
| `effective_policy`                                                 | One entry per server, `default` first: each part (`mcpTemplate`, `allow`, `dataPolicy`, `globalBlocklist`, `irisAudit`, `writeTools`, `destructiveTools`, `role`, `compileTimeGate`) as `{value, from, file?, set_aside?}`, where `from` is `launch`, `operator_file`, `trusted_file`, `workspace_file`, `system_mode` or `strict_default`                |
| `set_aside`                                                        | Everything iad read and did not act on: `{id, kind, setting, server, file, reason, lift}`; see [Set-aside items](connecting.md#set-aside-items)                                                                                                                                                                                                           |
| `write_tools_enabled`, `write_tools_source`                        | The default server's write tier and what decided it: `operator_env`, `config_file`, `legacy_allow_prod`, `inferred_system_mode`, `inferred_namespace`, `inferred_default` or `fail_closed`                                                                                                                                                                |
| `destructive_tools_enabled`, `destructive_tools_source`            | The same for the destructive tier                                                                                                                                                                                                                                                                                                                         |
| `compile_time_gate`                                                | `{enabled, from, file, set_aside}` for the [compile-time gate](connecting.md#compile-time-gate)                                                                                                                                                                                                                                                           |
| `identity`                                                         | Who the agent's code would run as: `service_account`, `os_user_opt_in` (`enabled`, `source`, `ignored`), `paths` (`http`, `docker_local`, `docker_ssh`, `ws_terminal`), `servers` and the `tools` covered. `http` reports `login` from one `GET /api/atelier/` that sends no code                                                                         |
| `tls_verify`, `tls`                                                | The default connection's certificate check, and every server's with its `source` (`default`, `launch_env`, `config_file`, `servers_json`, `server_manager`)                                                                                                                                                                                               |
| `discovery`                                                        | `skipped`: listeners the localhost scan did not send a credential to, with port and reason                                                                                                                                                                                                                                                                |
| `credentials_withheld`                                             | The environment credentials kept from a host a project file named, and what to do                                                                                                                                                                                                                                                                         |
| `read_only`, `read_only_from`                                      | `strict` or `off`, and what asked for it                                                                                                                                                                                                                                                                                                                  |
| `telemetry_sink`                                                   | `local` (the default) or `iris`: telemetry always goes to the local file, and `iris` adds the IRIS global where the call's server allows it                                                                                                                                                                                                               |
| `audit`                                                            | The local audit log: its `path` and whether it keeps `payloads` as `sizes` or `full`                                                                                                                                                                                                                                                                      |
| `skills`                                                           | Where served skills came from: `override_dir`, `override_kind`, `local_skills`, `ignored_dir` (a project's `skills/skills` left unread)                                                                                                                                                                                                                   |
| `server_manager`                                                   | Whether Server Manager profiles were found and whether each credential resolved                                                                                                                                                                                                                                                                           |
| `config_errors`                                                    | Values iad corrected or did not understand, and strict read-only combined with `irisAudit`                                                                                                                                                                                                                                                                |
| `config_parse_error`                                               | Present when a configuration file does not parse; every IRIS tool then answers `CONFIG_INVALID`                                                                                                                                                                                                                                                           |
| _(failure count)_                                                  | Present when `irisAudit` emission has failed; the field is named in [connecting.md](connecting.md#irisaudit--opt-in-sysaudit-emission)                                                                                                                                                                                                                    |
| `fallback_warning`                                                 | Present when the connection came from discovery with no configuration file, so the agent may be attached to the wrong instance                                                                                                                                                                                                                            |

A call to a project that has a file with loosening settings it has not trusted shows them here
before anything is refused:

```json
{
  "config_sources": {
    "operator_file": {
      "path": "/home/me/.iris-agentic-dev.toml",
      "loaded": true
    },
    "workspace_file": {
      "path": "/work/app/.iris-agentic-dev.toml",
      "kind": "workspace_file",
      "named_by": null,
      "trust": "untrusted",
      "lift": "run `iris-agentic-dev trust /work/app/.iris-agentic-dev.toml` in a terminal"
    }
  },
  "set_aside": [
    {
      "id": "workspace:write_tools_enabled",
      "kind": "untrusted_setting",
      "setting": "write_tools_enabled = true",
      "server": "default",
      "file": "/work/app/.iris-agentic-dev.toml",
      "reason": "the file is not trusted",
      "lift": "run `iris-agentic-dev trust /work/app/.iris-agentic-dev.toml` in a terminal"
    }
  ]
}
```

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

Generate a class from a description, store it and compile it. Requires an LLM API key in the
connection config. The generated text is stored under the name it declares, replacing any class of
that name, and compiled the way `iris_doc` put with `compile` does it: the compile-time gate reads
the text, the stored `.inc` files it includes are checked, and a superclass IRIS would compile with
it must be up to date. A refusal comes back as an error result and nothing is compiled. When IRIS
reports compile errors the model gets them once to fix the class, and the fixed text goes through
the same checks. The response carries `class_name`, `class_text`, `compiled`, `retried` and
`compile_errors`.

| Parameter     | Type   | Default  | Notes                                                                                          |
| ------------- | ------ | -------- | ---------------------------------------------------------------------------------------------- |
| `description` | string | —        | **Required.**                                                                                  |
| `overwrite`   | bool   | `false`  | Accepted and not read: the generated class replaces a stored class of the same name either way |
| `namespace`   | string | `"USER"` |                                                                                                |

```text
iris_generate_class(description="A %Persistent class storing patient visit records with indexes on PatientID and VisitDate")
```

---

### `iris_generate_test`

Generate `%UnitTest` scaffolding for an existing class.

| Parameter    | Type   | Default  | Notes         |
| ------------ | ------ | -------- | ------------- |
| `class_name` | string | —        | **Required.** |
| `namespace`  | string | `"USER"` |               |

```text
iris_generate_test(class_name="MyApp.Service")
```

---

## Interoperability

### `iris_production` ✦

Start, stop, update, check, or recover a production.

| Parameter         | Type   | Default  | Notes                                                                                                                                 |
| ----------------- | ------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `action`          | string | —        | **Required.** `"status"` \| `"start"` \| `"stop"` \| `"update"` \| `"check"` \| `"recover"` \| `"get_autostart"` \| `"set_autostart"` |
| `production_name` | string | —        | Production class name for `start` and `stop`; defaults to the currently running production                                            |
| `production`      | string | —        | `set_autostart` only — the class name to autostart. **No other action reads this key**                                                |
| `timeout`         | int    | `30`     | Seconds; `stop` only                                                                                                                  |
| `force`           | bool   | `false`  | `stop` only                                                                                                                           |
| `full`            | bool   | `false`  | `status` only: include per-item state                                                                                                 |
| `enabled`         | bool   | —        | `set_autostart` only                                                                                                                  |
| `namespace`       | string | `"USER"` |                                                                                                                                       |

`production_name` and `production` are two distinct keys, and each action reads exactly
one of them. Passing `production` to `start` is silently ignored — the production name
comes out empty and the call starts nothing.

Actions that modify production state require `IRIS_CONTAINER`.

```text
iris_production(action="status")
iris_production(action="stop", timeout=60, force=true)
iris_production(action="start", production_name="MyApp.Production")
iris_production(action="set_autostart", production="MyApp.Production", enabled=true)
```

---

### `iris_interop_query` ✦

Query production logs, queue depths, or message archive.

| Parameter       | Type            | Default           | Notes                                                       |
| --------------- | --------------- | ----------------- | ----------------------------------------------------------- |
| `what`          | string          | `"logs"`          | Enum: `"logs"` \| `"queues"` \| `"messages"`                |
| `component`     | string          | —                 | `logs`: filter by business host name                        |
| `log_type`      | string          | `"error,warning"` | `logs` only; comma-separated, see below                     |
| `limit`         | int             | `50`              | Applies to `logs` and `messages`                            |
| `source`        | string          | —                 | `messages`: filter by source                                |
| `target`        | string          | —                 | `messages`: filter by target                                |
| `message_class` | string          | —                 | `messages`: filter by message class                         |
| `session_id`    | int \| string   | —                 | `messages`: one session's messages; decimal string accepted |
| `since_id`      | int \| string   | —                 | `messages`: tail after this header ID                       |
| `body_class`    | string          | —                 | `messages`: body class to join, e.g. `Ens.StringContainer`  |
| `body_where`    | string          | —                 | `messages`: SQL predicate on the joined body table          |
| `body_select`   | array\<string\> | —                 | `messages`: body-table columns to add to each row           |
| `search_table`  | object          | —                 | `messages`: indexed Search Table search — see below         |
| `namespace`     | string          | `"USER"`          |                                                             |
| `server`        | string          | —                 | Named server; omit for default                              |

`log_type` takes any comma-separated subset of `assert`, `error`, `warning`, `info`, `trace`,
`alert` — the six values of `Ens.Util.Log.Type`, matched case-insensitively. A name that is not one
of those six contributes no filter, and the response reports it under `unknown_log_types` rather
than quietly widening the query to every severity.

`search_table` takes `prop` (required), one of `value` or `value_like`, and optional
`class` and `extent` (default `EnsLib.HL7.SearchTable`).

A call that names `body_class`, `body_where` or `body_select` reads message bodies, so it is a
bulk-data call: it is refused with `DATA_POLICY_BLOCKED` unless the target server's `dataPolicy`
is `allow` (`redact` does not admit it, because the rows are not HL7 text iad could redact).
Without those arguments the tool lists headers, logs and queues under any policy.

`body_where` is one condition, joined to the query as `(…)`. It is read the way IRIS reads SQL:
quotes, comments and parentheses count only where IRIS counts them. It is refused when it holds a
comment or `;` outside a quote, a quote that never closes, an unbalanced parenthesis, `UNION`,
`INTERSECT`, `EXCEPT`, `SELECT`, `INTO`, or a write keyword. A quoted string can hold any of
those: `Name = 'a;b -- c'` is an ordinary filter.

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
| `server`    | string               | —        | Named server; omit for default                                                  |

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

| Parameter    | Type   | Default  | Notes                                    |
| ------------ | ------ | -------- | ---------------------------------------- |
| `production` | string | —        | Defaults to currently running production |
| `namespace`  | string | `"USER"` |                                          |
| `server`     | string | —        | Named server; omit for default           |

```text
iris_production_diff()
iris_production_diff(production="MyApp.Production")
```

---

### `iris_message_body`

Read a message body by ID. A bulk-data tool: what it returns depends on the target server's
`dataPolicy`, see [Data safety gates](#data-safety-gates).

| Parameter        | Type   | Default  | Notes                                                                 |
| ---------------- | ------ | -------- | --------------------------------------------------------------------- |
| `message_id`     | string | —        | **Required.** `Ens.MessageHeader` ID                                  |
| `max_bytes`      | int    | `65536`  | Clamped to 1 MB (1048576)                                             |
| `acknowledgePhi` | bool   | `false`  | Required with `dataPolicy = "allow"`                                  |
| `dataPolicy`     | string | none     | `block` \| `redact` \| `allow`; omitted means the server's own policy |
| `namespace`      | string | `"USER"` |                                                                       |
| `server`         | string | —        | Named server; omit for default                                        |

The policy that applies is the stricter of the server's `dataPolicy` and the call's, so passing
`allow` here cannot widen what a configuration file restricted, and a call that passes none gets
the server's own (a server set to `redact` answers a call that names no policy with a redacted
body). The value is read trimmed and in any case (`Allow` is `allow`); any other value is
`INVALID_PARAMS`. `acknowledge_phi` and `data_policy` are accepted for `acknowledgePhi` and
`dataPolicy`. The result of the policy:

| Policy   | Result                                                                                                                                                                                                 |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `block`  | `PHI_POLICY_BLOCKED`; nothing of the body is returned                                                                                                                                                  |
| `redact` | An HL7 v2 body with the identifying fields of PID, NK1, GT1, IN1, IN2 and MSH-3 replaced by `[REDACTED]` (list below); any other content (XML, JSON, plain text) is refused with `BODY_NOT_REDACTABLE` |
| `allow`  | The body, once the call carries `acknowledgePhi: true` (`PHI_ACK_REQUIRED` otherwise)                                                                                                                  |

`redact` is a field mask, not de-identification. `redacted: true` on a result means the mask ran. It
replaces these HL7 v2.5 fields with `[REDACTED]` and leaves every other field, segment and line of
free text (PV1, OBX, NTE and the rest) as stored; an empty field stays empty:

| Segment | Masked fields                                                                                                                                                                            |
| ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| MSH     | MSH-3 sending application                                                                                                                                                                |
| PID     | PID-2, 3, 4 (ids), 5 name, 6 mother's maiden name, 7 birth date, 8 sex, 9 alias, 11 address, 12 county, 13 and 14 phones, 18 account number, 19 SSN, 20 driver's licence                 |
| NK1     | NK1-2 name, 4 address, 5 and 6 phones, 12 employee number, 16 birth date, 30 contact name, 31 contact phone, 32 contact address, 33 identifiers, 37 contact SSN                          |
| GT1     | GT1-2 guarantor number, 3 name, 4 spouse name, 5 address, 6 and 7 phones, 8 birth date, 12 SSN, 16 employer name, 17 employer address, 18 employer phone, 19 employee id, 29 employer id |
| IN1     | IN1-10 group employee id, 16 insured's name, 18 birth date, 19 address, 36 policy number, 43 sex, 44 employer address, 49 insured's id                                                   |
| IN2     | IN2-1 insured's employee id, 2 insured's SSN                                                                                                                                             |

A segment is recognised after blanks, `\x0b` (MLLP start) or `\x1c` in front of its name are set
aside, and a stray `\x0b` or `\x1c` between segments counts as a segment break.

```text
iris_message_body(message_id="123456")
iris_message_body(message_id="123456", dataPolicy="allow", acknowledgePhi=true)
```

---

### `iris_business_rule_info`

List or inspect business rules. It reads rule classes, which extend `Ens.Rule.Definition` and hold an
`XData RuleDefinition` block (the kind the Rule Editor opens), and the legacy `Ens.Rule.RuleSet` table
from before 2012. Each result says which with `kind: "class"` or `kind: "legacy"`.

`get` on a rule class returns its rule type (`Ens.Rule.GeneralBusinessRuleAssist`,
`EnsLib.MsgRouter.RuleAssist`, ...), its context class, and each rule set's rules with their `when`
conditions and actions. On a legacy rule set it returns condition and action counts only. Earlier versions read the legacy table alone, so it answered `RULE_NOT_FOUND` for every rule class.

| Parameter   | Type   | Default  | Notes                             |
| ----------- | ------ | -------- | --------------------------------- |
| `action`    | string | —        | **Required.** `"list"` \| `"get"` |
| `rule_name` | string | —        |                                   |
| `namespace` | string | `"USER"` |                                   |
| `server`    | string | —        | Named server; omit for default    |

```text
iris_business_rule_info(action="list")
iris_business_rule_info(action="get", rule_name="MyApp.RoutingRule")   # the rule class name
```

---

### `iris_credential_list`

List Ensemble credentials. Passwords are never returned.

| Parameter   | Type   | Default  |
| ----------- | ------ | -------- |
| `namespace` | string | `"USER"` |

No other parameters.

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

A key set to an empty value exists: `get` returns `value: ""`. `get` or `delete` of a key that was
never set answers `KEY_NOT_FOUND`, and either action on a missing table answers `TABLE_NOT_FOUND`.

```text
iris_lookup_manage(action="list_tables")
iris_lookup_manage(action="list_keys", table="FacilityMap")
iris_lookup_manage(action="get", table="FacilityMap", key="MGH")
```

---

### `iris_lookup_transfer`

Export or import an Ensemble lookup table as XML. Import replaces the whole table, so it is in
the destructive tier (☠); export is a read.

| Parameter   | Type   | Default  | Notes                                  |
| ----------- | ------ | -------- | -------------------------------------- |
| `action`    | string | —        | **Required.** `"export"` \| `"import"` |
| `table`     | string | —        | **Required.**                          |
| `xml`       | string | —        | For `action=import`                    |
| `namespace` | string | `"USER"` |                                        |

---

## Administration

Most tools in this section are ported from Pierre Abdelsayed's Server Manager MCP work.
The global confirmation pattern, namespace/database admin, observability tools, HL7
schema tools, Mermaid diagrams, and `resolve_storage` all originate from his design.

### `global_preview`

Preview the top N subscripts of an IRIS global and mint a confirmation token for a
subsequent `global_kill`. Returns up to 100 entries, the total subscript count, and a
`confirm_token` that expires in 5 minutes. The system blocklist and the PHI name gate apply
before anything is read; a refused preview returns no entries and no token.

| Parameter | Type   | Default | Notes                                          |
| --------- | ------ | ------- | ---------------------------------------------- |
| `global`  | string | —       | **Required.** Global name, with or without `^` |
| `count`   | number | `20`    | Max entries to preview (1–100)                 |
| `server`  | string | —       | Named server; omit for default                 |

### `global_kill` 🔒 ☠

Kill an IRIS global after confirming with a token from `global_preview`. The token
validates the global name and server — a token issued for `^Foo` cannot be used to kill
`^Bar`. Tokens expire after 5 minutes. Write- and destructive-gated, and the system blocklist and PHI
name gate apply. On a connection whose `mcpTemplate` is `live` or `test` it is refused, like
`iris_global` with `action=kill`.

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

List databases, their directory paths, and free space. Each entry includes
`size_mb`, `free_space_mb`, `free_pct`, and `max_size_mb` (null when unlimited).

| Parameter | Type   | Default | Notes                          |
| --------- | ------ | ------- | ------------------------------ |
| `server`  | string | —       | Named server; omit for default |

### `iris_mirror_status`

Report mirror membership and role for the connected IRIS instance. Non-mirror
instances return `{is_member: false}`. Useful as a pre-flight check before
operations that require a primary.

| Parameter | Type   | Default | Notes                          |
| --------- | ------ | ------- | ------------------------------ |
| `server`  | string | —       | Named server; omit for default |

**Response fields:** `is_member` (bool), `mirror_name` (string or null),
`member_type` (string or null — primary/backup/async), `is_primary` (bool).

### `iris_system_performance`

Manage IRIS SystemPerformance (pbuttons) profiles, start and poll runs, and locate the report a
finished run wrote.

| Parameter          | Type    | Default | Notes                                                                                                                         |
| ------------------ | ------- | ------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `mode`             | string  | —       | **Required.** `start` / `status` / `last_runid` / `list_profiles` / `add_profile` / `delete_profile` / `list_runs` / `report` |
| `profile`          | string  | `test`  | `mode=start`: which profile to run. `mode=add_profile` / `delete_profile`: which profile to create or remove                  |
| `run_id`           | string  | —       | Required for `mode=status`. Optional for `mode=report` — omit for the newest completed run                                    |
| `description`      | string  | —       | Required for `mode=add_profile`                                                                                               |
| `interval_seconds` | integer | —       | Required for `mode=add_profile`. Seconds between samples; the shipped profiles use 1 to 60                                    |
| `sample_count`     | integer | —       | Required for `mode=add_profile`. `interval_seconds × sample_count` is the run length                                          |
| `server`           | string  | —       | Named server; omit for default                                                                                                |

**Response fields:** `success` (bool), `mode` (string).

- `mode=start` returns `profile` and `run_id`. The run ID comes straight from
  `$$run^SystemPerformance(profile)`.
- `mode=last_runid` returns `run_id` and `in_progress` (bool). It reads
  `^IRIS.SystemPerformance("run")` before `("history")`, so a run that is still collecting is
  reported with `in_progress: true` — a run gets no history node until it finishes.
- `mode=status` returns `wait_time` (string from `$$waittime^SystemPerformance`, e.g.
  `"7 minutes"`).
- `mode=list_profiles` returns `profiles` and `count`. Each profile has `name`,
  `interval_seconds`, `sample_count`, `duration_minutes`, and `description`.
- `mode=add_profile` returns the stored `profile`, `interval_seconds`, `sample_count`, and
  `duration_minutes`. A duplicate name comes back as `success: false` with IRIS's own
  `profile name exists already`.
- `mode=delete_profile` returns the deleted `profile`. A deleted profile is one `add_profile`
  away from being back, so this is a write, not a destructive operation.
- `mode=list_runs` returns `runs` (newest first, capped at 100) and `count`. Each run has
  `run_id`, `completed_at`, `output_dir`, and `profile`.
- `mode=report` returns `run_id`, `completed_at`, `output_dir`, `report_path`, `size_bytes`, and
  `exists`. A run still collecting has no report yet and comes back with `exists: false` and a
  `note` saying so.

Profile names accept letters, digits and underscore only. That is stricter than IRIS itself:
`addprofile` accepts `"bad name"`, returns success, and stores the profile as `badname` — so the
name you asked for does not exist and nothing tells you. Names outside the safe set are rejected
before the call.

The completed report lands in the instance's mgr directory as
`<host>_<instance>_<run_id>.html`; `mode=report` resolves that path and falls back to scanning
the run's output directory if the host name has changed since. There is no cancel entry point —
a started profile runs to completion, so prefer `test` unless you need a longer window.

### `iris_database_stats`

Show size, free space, and block stats for a specific database directory.

| Parameter | Type   | Default | Notes                                 |
| --------- | ------ | ------- | ------------------------------------- |
| `db`      | string | —       | **Required.** Database directory path |
| `server`  | string | —       | Named server; omit for default        |

### `journal_search`

Search the IRIS journal for global set/kill records. A bulk-data tool: it needs
`dataPolicy = "allow"` on the target server (`DATA_POLICY_BLOCKED` otherwise; `redact` does not
admit it, since a journal record's value is not a message there is a rule to redact). It returns
which globals changed and when, never the values.

| Parameter        | Type   | Default | Notes                                       |
| ---------------- | ------ | ------- | ------------------------------------------- |
| `start`          | string | —       | ISO 8601 lower bound on the record time     |
| `end`            | string | —       | ISO 8601 upper bound on the record time     |
| `global_pattern` | string | —       | Substring the global reference must contain |
| `max_entries`    | number | `100`   | Cap on returned records; clamped to 1–500   |
| `server`         | string | —       | Named server; omit for default              |

The answer is `{entries: [{timestamp, type, job_id, global}], returned, truncated,
truncated_reason, scanned}`. The search walks the current journal file from its newest record
back, so records come newest first and a search for something recent ends early; a record before
`start` ends the walk. It reads at most 200,000 records and runs at most 20 seconds, so the answer says how far it got:
`scanned` is the records read, `truncated` is `true` when the walk stopped before the start of the
journal or `start`, and `truncated_reason` is `max_records`, `scan_cap` or `time_cap` (`null` when
the walk finished). **A truncated answer has not seen the rest of the file.** Narrow `start`/`end`
or `global_pattern`, and read `scanned`. The walk reads records one by one in iad's own code; it
does not call `%SYS.Journal.File:Search`, which writes one `messages.log` line per record it
visits and can fill the log of a busy instance. A request that outruns the HTTP timeout anyway
answers `JOURNAL_SEARCH_TIMEOUT`, naming the journal file and its size.

### `query_audit_log`

Query the `%SYS.Audit` table for recent events.

Both filters are equality comparisons (`EventType = ?`, `Username = ?`), not substring
matches — `event_type="Login"` will not find `LoginFailure`.

| Parameter    | Type   | Default | Notes                                              |
| ------------ | ------ | ------- | -------------------------------------------------- |
| `event_type` | string | —       | Exact match on the `EventType` column              |
| `user`       | string | —       | Exact match on the `Username` column               |
| `start`      | string | —       | Lower bound on `UTCTimeStamp`, inclusive; ISO 8601 |
| `end`        | string | —       | Upper bound on `UTCTimeStamp`, inclusive; ISO 8601 |
| `limit`      | number | `100`   | Max rows; clamped to 1–500                         |
| `server`     | string | —       | Named server; omit for default                     |

### `stream_inspect`

Read the start of a stream by OID. Returns the stream's `class`, its whole `size`, the content,
`truncated`, the `max_chars` cap the read used, and the line count of what came back. `class` is the
class the read went through (`%Stream.GlobalCharacter`); character and binary streams share storage,
so it says nothing about what the stream holds, and the answer makes no text or binary claim. An id
that holds no stream is `STREAM_NOT_FOUND`, which is not the same as a stream of size 0. The target
server's `dataPolicy` decides what comes back: `allow` returns the content, `redact` returns HL7 v2
with the identifying fields masked (not de-identified; see the redact row above) and refuses other
content (`BODY_NOT_REDACTABLE`), and `block` refuses the content (`DATA_POLICY_BLOCKED`). A refused
stream still answers with `class`, `size` and `line_count`.

| Parameter   | Type    | Default  | Notes                                                               |
| ----------- | ------- | -------- | ------------------------------------------------------------------- |
| `oid`       | string  | —        | **Required.** Stream OID                                            |
| `max_chars` | integer | `20000`  | Most characters to return from the start, at most `1000000` (min 1) |
| `namespace` | string  | `"USER"` | Namespace that contains the stream                                  |
| `server`    | string  | —        | Named server; omit for default                                      |

### `my_access`

Show current user, roles, and privileges for the connected session.

| Parameter | Type   | Default | Notes                          |
| --------- | ------ | ------- | ------------------------------ |
| `server`  | string | —       | Named server; omit for default |

### `capability_matrix`

Show the roles assigned to one user. Returns `{user, full_name, roles}` from
`Security.Users` — one user, not a namespace-by-privilege matrix.

| Parameter | Type   | Default     | Notes                          |
| --------- | ------ | ----------- | ------------------------------ |
| `user`    | string | `$USERNAME` | Username to look up            |
| `server`  | string | —           | Named server; omit for default |

### `hl7_schema_list`

List available HL7 2.x schema versions. Returns `HL7_NOT_AVAILABLE` if
`EnsLib.HL7.Schema` is absent. Requires HealthShare or IRIS for Health — not present on
plain IRIS regardless of edition.

| Parameter   | Type   | Default | Notes                          |
| ----------- | ------ | ------- | ------------------------------ |
| `namespace` | string | —       | Namespace to list from         |
| `server`    | string | —       | Named server; omit for default |

### `hl7_schema_inspect`

Show segment definitions, field names, and data types for a specific HL7 schema version
and optional segment filter.

| Parameter   | Type   | Default | Notes                                        |
| ----------- | ------ | ------- | -------------------------------------------- |
| `schema`    | string | —       | **Required.** e.g. `"2.6"`                   |
| `segment`   | string | —       | Segment name filter, e.g. `"PID"` (optional) |
| `namespace` | string | —       | Namespace to read the schema from            |
| `server`    | string | —       | Named server; omit for default               |

### `mermaid_class`

Generate a Mermaid class diagram showing inheritance for one class. Walks the `Super`
hierarchy and strips `%`-prefixed system class names.

| Parameter   | Type   | Default  | Notes                          |
| ----------- | ------ | -------- | ------------------------------ |
| `class`     | string | —        | **Required.** Starting class   |
| `depth`     | number | `3`      | Levels of `Super` to walk      |
| `namespace` | string | `"USER"` | Namespace to query             |
| `server`    | string | —        | Named server; omit for default |

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

### System observability

Five read-only `iris_admin` actions give a real-time view into IRIS internals. All run
against `%SYS` directly — no external monitoring stack required.

**`view_locks`** — lists all active IRIS locks. Each row shows the lock name, lock type
(shared/exclusive), the owning process ID, and the caller's job ID. Useful for debugging
contention during long-running transactions or BPL routes that hold locks across
`Await` calls.

```text
iris_admin(action="view_locks")
```

**`view_processes`** — shows running IRIS processes. Columns include process ID,
namespace, routine+label (i.e. where the process is executing), CPU time, and whether
the process is suspended. The `namespace` parameter narrows results to a single
namespace. Process user names are redacted in the response (PHI precaution) unless the
call passes `dataPolicy = "allow"`; the default is `block`, which refuses the call
outright.

```text
iris_admin(action="view_processes", dataPolicy="redact")
iris_admin(action="view_processes", dataPolicy="redact", namespace="MYAPP")
```

**`namespace_mappings`** — returns the global, package, and routine mappings for a
namespace: which packages and globals resolve to which databases. Equivalent to the
Namespace Mappings page in Management Portal, useful when debugging `<CLASS DOES NOT
EXIST>` errors that turn out to be mapping gaps.

```text
iris_admin(action="namespace_mappings", namespace="MYAPP")
```

**`database_status`** — calls `SYS.Database:FreeSpace` and returns size, free space, and
journal state for every database. Pass `name` to narrow to a substring match on
the database path.

```text
iris_admin(action="database_status")
iris_admin(action="database_status", name="MYAPP")
```

**`journal_search`** — scans the IRIS journal for global set/kill records, with their values.
Gated: requires `dataPolicy = "allow"` on the target server because journal records can contain
PHI, and at least one of `global_pattern` or `time_range`. The `global_pattern` substring filter
keeps results manageable; `max_records` caps at 1000. A value is returned only if the global is not
on the system blocklist or the server's `globalBlocklist` (never, `redacted: true`), and, for a
PHI-named global, only with `acknowledgePhi: true`.

```text
iris_admin(action="journal_search",
           global_pattern="^MyApp.Order",
           time_range={"from": "2026-01-15T00:00:00Z",
                       "to":   "2026-01-15T23:59:59Z"})
```

For standalone journal access there is also a `journal_search` tool (Administration
section below) that does not require `iris_admin`. It is **not** an alias — the two take
different parameter names for the same concepts, and passing one tool's names to the
other is rejected with `UNKNOWN_PARAMETER`:

| Concept       | `iris_admin(action="journal_search")` | standalone `journal_search` |
| ------------- | ------------------------------------- | --------------------------- |
| Time window   | `time_range={"from": …, "to": …}`     | `start` / `end`             |
| Result cap    | `max_records` (default 100, max 1000) | `max_entries` (1–500)       |
| PHI gate      | connection `dataPolicy`               | connection `dataPolicy`     |
| Global filter | `global_pattern`                      | `global_pattern`            |

`global_pattern` is the only name they share.

`iris_admin(action="journal_search")` answers with `records` (`global_ref`, `transaction_id`,
`operation`, `timestamp`, `value`, `redacted`), `count`, `scanned`, `truncated` and
`truncated_reason` (`max_records`, `scan_cap` or `time_cap`; `null` when the walk reached the end
of the journal or passed the end of the time range). It reads at most 200,000 journal records
and runs at most 20 seconds, so a search of a large journal comes back with `truncated: true`
and the records found so far; a truncated answer has not seen the rest of the file.
`global_pattern` is matched as a literal substring of the global reference, not of the value, and
`*` and `?` in it are stripped.

---

### `iris_admin` ☠

List namespaces, databases, users, roles, and web apps. Read actions have no tier gate.
Write actions need the target server's write tier on. The user, namespace and web application
actions, `clear_password_change_flag`, `unlock_user` and `fresh_container_setup` also need
`IRIS_ADMIN_TOOLS=1` in the launch settings, which the handler checks after the tier gate
(`ADMIN_WRITE_DISABLED`).

**Read actions** (no env gate):

| Action               | Parameters                                                                                                 |
| -------------------- | ---------------------------------------------------------------------------------------------------------- |
| `list_namespaces`    | —                                                                                                          |
| `list_databases`     | —                                                                                                          |
| `list_users`         | —                                                                                                          |
| `list_roles`         | —                                                                                                          |
| `list_webapps`       | `type` (string, optional)                                                                                  |
| `get_webapp`         | `path` (string, required)                                                                                  |
| `check_permission`   | `resource` (string, required), `permission` (string, required)                                             |
| `view_locks`         | —                                                                                                          |
| `view_processes`     | `namespace` (string, optional)                                                                             |
| `namespace_mappings` | `namespace` (string, optional)                                                                             |
| `database_status`    | `name` (string, optional)                                                                                  |
| `list_user_roles`    | `username` (string, required)                                                                              |
| `journal_search`     | `global_pattern` (string), `time_range` (`{from, to}` ISO8601), `max_records` (int, default 100, max 1000) |

`action` is the only required parameter, and its 25 values are advertised as an enum, so a
client can complete them and a value outside the list comes back as `INVALID_ACTION`.
`server` routes the call to a named registered instance; `acknowledgePhi` applies to
`journal_search` only. Every other parameter is optional
at the schema level and required by the action that reads it, per the tables here.

The parameter set is closed: a filter typed as `type_filter` or `name_filter` is rejected
with `UNKNOWN_PARAMETER` naming what the tool does accept. It used to be dropped silently,
and the unfiltered result set came back looking like an answer.

`view_processes` and `journal_search` redact PHI according to `dataPolicy` in
`[policy.<server>]`, which defaults to `block`. It used to be a call parameter, which meant
a caller could authorize its own bulk journal read by passing `dataPolicy: "allow"`. That
key is no longer part of the schema, so passing it is now an `UNKNOWN_PARAMETER` error
rather than a no-op. `journal_search` needs `dataPolicy = "allow"` on the connection.

**Write actions** (require the write tier):

| Action                       | Parameters                                                                                                                                                                |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `create_user`                | `username`, `password` (required); `full_name`, `roles` (optional)                                                                                                        |
| `update_user`                | `username` (required); `password`, `enabled`, `roles` (optional)                                                                                                          |
| `delete_user`                | `username` (required)                                                                                                                                                     |
| `create_namespace`           | `name`, `code_database`, `data_database` (all required)                                                                                                                   |
| `delete_namespace`           | `name` (required)                                                                                                                                                         |
| `create_webapp`              | `path`, `namespace`, `enabled` (required); `dispatch_class` (optional)                                                                                                    |
| `delete_webapp`              | `path` (required)                                                                                                                                                         |
| `clear_password_change_flag` | `username` (default `_SYSTEM`), `password` (default `SYS`), `new_password` (optional, default same as password)                                                           |
| `unlock_user`                | `username` (required)                                                                                                                                                     |
| `fresh_container_setup`      | `username` (default `_SYSTEM`), `password` (default `SYS`), `new_password` (optional)                                                                                     |
| `mirror_add_async`           | `mirror_name`, `primary_host` (required); `primary_port` (default 2188), `instance_name` (default `IRIS`), `async_member_type` (0=DR, 1=ReadOnly, 2=ReadWrite; default 0) |

**Destructive actions** (require the destructive tier):

| Action            | Parameters                                                                 |
| ----------------- | -------------------------------------------------------------------------- |
| `mirror_failover` | `confirm` (bool, required — must be `true`; prevents accidental promotion) |

**Fresh container setup actions** clear the first-boot blockers on a newly-started IRIS
community container:

- `clear_password_change_flag` — clears the forced-password-change flag on an account by
  calling `%SYSTEM.Security.ChangePassword`. When `new_password` is omitted, the same
  password is used for both old and new — clears the flag without changing the credential.
- `unlock_user` — resets `InvalidLoginAttempts` to 0 via `Security.Users.Modify`, unblocking
  a locked account. Idempotent — safe to call on an account that is not locked.
- `fresh_container_setup` — runs `clear_password_change_flag` then `unlock_user` in sequence.
  Returns `{success, ready, steps[]}`. Continues on per-step errors so callers see which
  steps succeeded. Call this once on a fresh container before any other iad tool.

**Mirror management**:

- `mirror_add_async` — joins this IRIS instance to an existing mirror set as an async
  disaster-recovery member by calling `SYS.Mirror.JoinMirrorAsAsyncMember` in `%SYS`.
  Performs a pre-flight `IsMember()` check; returns `ALREADY_MEMBER` if already in a
  mirror. Version mismatches between primary and candidate surface as `MIRROR_VERSION_MISMATCH`.
- `mirror_failover` — promotes this backup member to primary via `SYS.Mirror.BecomePrimary`.
  Irreversible without manual recovery. Requires `confirm: true` to prevent accidental
  promotion. Pre-flights: returns `NOT_MIRROR_MEMBER` if not in a mirror, `ALREADY_PRIMARY`
  if this instance is already the primary.

```text
iris_admin(action="list_namespaces")
iris_admin(action="list_users")
iris_admin(action="journal_search", global_pattern="^MyApp.*",
           time_range={"from": "2025-01-01T00:00:00Z", "to": "2025-01-02T00:00:00Z"})
iris_admin(action="fresh_container_setup")
iris_admin(action="clear_password_change_flag", username="_SYSTEM", password="SYS")
iris_admin(action="unlock_user", username="_SYSTEM")
iris_admin(action="mirror_add_async", mirror_name="DR_SET", primary_host="10.0.1.5")
iris_admin(action="mirror_failover", confirm=true)
```

---

### `iris_containers` ✦

List, select, or start IRIS Docker containers.

| Parameter | Type   | Default | Notes                                                       |
| --------- | ------ | ------- | ----------------------------------------------------------- |
| `action`  | string | —       | **Required.** `"list"` \| `"select"` \| `"start"`           |
| `name`    | string | `""`    | Container name; required for `select`, optional for `start` |

Those two are the only parameters this tool reads. `list` scans
`$OBJECTSCRIPT_WORKSPACE` (falling back to the process working directory) — there is no
`workspace_root` override. `select` and `start` connect as `_SYSTEM`/`SYS` on the
`community` edition and take the namespace from the container, so passing `namespace`,
`username`, `password`, or `edition` has no effect.

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

The token is a handle into a pool held in the memory of the process that opened the session, so it
only resolves for the process that minted it. An MCP client gets that for free — one server process
serves the whole conversation. On the CLI, `iris-agentic-dev tool` dispatches one call and exits, so
the three session tools are refused there and belong in a `batch` script instead:

```json
[
  { "tool": "iris_ws_open", "args": { "namespace": "USER" } },
  {
    "tool": "iris_ws_exec",
    "args": { "session": "{{0.session}}", "code": "Set x=42" }
  },
  { "tool": "iris_ws_close", "args": { "session": "{{0.session}}" } }
]
```

```bash
iris-agentic-dev batch --file ws-session.json
```

`{{0.session}}` is step 0's `session` field, resolved at run time. Details and the failure it
replaces:
[SESSION_STALE after `iris_ws_open` in the CLI](troubleshooting.md#session_stale-after-iris_ws_open-in-the-cli).

### `iris_ws_open`

Open a new WebSocket terminal session. Returns a `session` token to pass to subsequent
calls.

| Parameter   | Type   | Default  | Notes                          |
| ----------- | ------ | -------- | ------------------------------ |
| `namespace` | string | `"USER"` | Namespace for the session      |
| `server`    | string | —        | Named server; omit for default |

The session logs in with the service account's credentials when one is set, and asks IRIS for
`$USERNAME` once after the prompt; the response carries `identity`. A session whose identity later
differs from the configured one is refused (`SESSION_IDENTITY_CHANGED`).

### `iris_ws_exec`

Execute ObjectScript in an existing session. The session context (variables, open
devices) persists across calls.

| Parameter | Type    | Default | Notes                                                                           |
| --------- | ------- | ------- | ------------------------------------------------------------------------------- |
| `session` | string  | —       | **Required.** Token from `iris_ws_open`                                         |
| `code`    | string  | —       | **Required.** ObjectScript to run                                               |
| `confirm` | boolean | `false` | Acknowledges the call on a `subject` server (`confirmed` is the older spelling) |

Each line of `code` goes to the terminal as its own input, as a pasted block would; blank
lines are skipped. A block in braces must sit on one line, as it must at a terminal prompt.

The per-frame timeout is a fixed 30 seconds, not a parameter. On timeout the session
stays open; call `iris_ws_close` to release it.

`code` goes through the same gates as `iris_execute`: the code-edit hard-block
(`CODE_EDIT_BLOCKED` — `$system.OBJ`, `%Dictionary.*Definition`, `%RoutineMgr`, direct writes
to `^oddDEF` and the other code-storage globals), the `mcpTemplate` environment gate, the PHI
gates, and the destructive tier for a literal `Kill ^<global>`. A refused call sends nothing to
IRIS, so the session's variables are exactly as they were. The gates judge the server the session
was opened on.

Through v1.4.1 it ran none of those, so a WebSocket session was a way around all of them
([#137](https://github.com/intersystems-community/iris-agentic-dev/issues/137)).

### `iris_ws_close`

Close a WebSocket session and free its resources. Returns `{closed: true}`. Closing is not
idempotent: a token that is already closed, or that came from another process, returns
`SESSION_STALE`, because by then there is no session to look up. Earlier text here promised
`already_closed: true`, which the handler has never returned.

| Parameter | Type   | Default | Notes                                   |
| --------- | ------ | ------- | --------------------------------------- |
| `session` | string | —       | **Required.** Token from `iris_ws_open` |

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

### `skill_list`, `skill_search`, `skill_describe`, `skill_forget`

The same registry as `skill`, one tool per action. `skill_list` and `skill_search` cover the
skills bundled with the binary (no IRIS needed) and any synthesized ones in `^SKILLS`; each result
carries `source` (`bundled` or `synthesized`) and `tier`, and the response says how many skills
each source held, so a zero never means only one place was checked. The bundled copies are what
the binary embeds. A directory on disk replaces an embedded copy only when it comes from the
operator; see [skills.md](skills.md#serving-skills-from-a-directory). `skill_forget` is ☠.

| Tool             | Parameter | Type   | Default | Notes                                    |
| ---------------- | --------- | ------ | ------- | ---------------------------------------- |
| `skill_list`     | —         | —      | —       | No parameters                            |
| `skill_search`   | `query`   | string | —       | **Required.** Name, description and tags |
| `skill_search`   | `top_k`   | int    | `20`    |                                          |
| `skill_describe` | `name`    | string | —       | **Required.**                            |
| `skill_forget`   | `name`    | string | —       | **Required.**                            |

### `skill_community`

Browse or install community skills from subscribed GitHub repos. `skill_community_list` lists the
skills loaded from `--subscribe` packages and takes no parameters.

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

`index` reads the `.md` and `.txt` files directly in one directory, at most 1,000 of them. The
directory must be inside the [project directory](#project-directory) (the default is the project
directory itself), or the call is refused with `HOST_PATH_BLOCKED`. A file that is a link out of the
project is skipped and counted in `skipped_outside`. The reply has `truncated` and, when it is true,
`truncated_reason: "file_budget"`.

---

### `agent_history` / `agent_stats`

Recent tool-call history, and learning agent status. `agent_stats` takes no parameters.

| Tool            | Parameter | Type | Default |
| --------------- | --------- | ---- | ------- |
| `agent_history` | `limit`   | int  | `20`    |

```text
agent_history(limit=50)
agent_stats()
```

---

### `telemetry_query`

Query the durable telemetry record. It reads the local file, which holds this machine's records.
When the operator turned on the IRIS sink (`telemetry_sink = "iris"` or `IAD_TELEMETRY_SINK=iris`;
see [Telemetry](connecting.md#telemetry)) it reads the IRIS global too, on the server the call
targets where that server's writes are on, read-only mode is not strict and its role is not
`subject`, and returns a call that both hold once. `check_config.telemetry_sink` says which is
set.

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
| `read_only_hint`   | 53 tools — all query, inspect, list, history, and comparison tools                                                                                   | The tool makes no changes to IRIS state            |
| `destructive_hint` | 7 tools — `global_kill`, `iris_admin`, `iris_credential_manage`, `iris_lookup_manage`, `iris_namespace_create`, `iris_remove_server`, `skill_forget` | The tool can irreversibly delete or overwrite data |

MCP clients that respect `read_only_hint` can run read-only tools in parallel or in
background without approval prompts. Clients that respect `destructive_hint` can surface
an extra confirmation step before calling the destructive tools.

These are hints, not enforcement. Enforcement comes from the config gates described below.

---

## Write protection

Two config keys control which tools can write. They form a stack — the destructive tier can
only further restrict the write tier, never expand it.

### `write_tools_enabled`

Every write-capable tool returns `WRITE_TOOLS_DISABLED` when the target server's write tier is
off. Declare it in a configuration file:

```toml
write_tools_enabled = true      # the default server
```

```toml
[policy.prod]
writeTools = false              # one server
```

Editing a file changes the gate on the next tool call, in both directions — no restart. What
decides the answer, strongest first:

1. The launch settings: `IRIS_WRITE_TOOLS_ENABLED=1` or `0` (and the legacy `IRIS_ALLOW_PROD=1`).
   They outrank every file.
2. The operator file, then the workspace file, combined part by part. A file you have not trusted
   can turn writes **off** but cannot turn them **on**; its `write_tools_enabled = true` is set
   aside until you run `iris-agentic-dev trust`. See
   [Effective policy](connecting.md#effective-policy).
3. With nothing declared, the server's IRIS system mode: off on Live, on on Development and Test,
   and for an unset or unreadable mode, off when the namespace looks like production.

`check_config` reports which decided, per server in `effective_policy` and for the default
server in `write_tools_source`, so an unexpected answer names its own cause. Strict
read-only mode (`read_only = "strict"`) goes further and refuses every tool that can write a
scratch class; see [Strict read-only](connecting.md#strict-read-only-no-scratch-classes).

### `destructive_tools_enabled`

The tools marked ☠ require an additional opt-in. Even with the write tier on, they return
`DESTRUCTIVE_TOOLS_DISABLED` unless you also set:

```toml
destructive_tools_enabled = true      # or [policy.<server>] destructiveTools = true
```

Default: `false`, and never inferred — the destructive tier stays off until you declare it, from
the operator file, a trusted file or `IRIS_DESTRUCTIVE_TOOLS_ENABLED=1`. Declaring it with the
write tier off is a contradiction, so iad logs `DESTRUCTIVE_REQUIRES_WRITES` and exits 2 rather
than serving requests with one of the two answers silently discarded.

**Why a separate flag?** A compile-test workflow needs the write tier, but there's no reason for
that same session to be able to kill globals or delete namespaces. Enabling the two tiers
independently means an agent that can compile can't accidentally wipe data even if it constructs
a destructive call.

The tiers are per server. A write allowed on the default server is not allowed on `prod` unless
`prod`'s own policy and system mode allow it, and each call is judged against the server it runs
on. (A `compare_document` call that names two servers must pass on both.)

### Check order for a tool call

```text
1. configuration valid        — CONFIG_INVALID if a file does not parse
2. target server resolved     — a server name not in the pool is refused here
3. write_tools_enabled        — if off, WRITE_TOOLS_DISABLED
4. destructive_tools_enabled  — if off and the tool is ☠, DESTRUCTIVE_TOOLS_DISABLED
5. read_only = "strict"       — READ_ONLY_STRICT for a scratch-class tool
6. parameters and namespace   — UNKNOWN_PARAMETER, INVALID_PARAMS
7. data safety gates          — environment template, bulk-data tools, system globals,
                                sensitive-named globals, policy.<server>.allow, server role
8. execute
```

Steps 1 to 5 run once, in `call_tool`, before the request reaches the tool — so a tool cannot be
added without passing through them. Read-only tools skip steps 3 and 4. Every refusal in this
list says which kind it is; see [Refusals](#refusals).

---

## Data safety gates

Gate design by Pierre Abdelsayed (InterSystems Server Manager MCP), ported to Rust.

PHI is Protected Health Information — the patient-identifying data HIPAA governs.

Some tools can reach PHI, and some can reach the globals IRIS uses to store its own code
and configuration. Those calls are checked before they run and refused by default. Four
checks run in order; the first one that refuses wins. Each is judged against the **target
server** of the call, using its [effective policy](connecting.md#effective-policy).

**1. Environment template.** A server's `mcpTemplate` says what kind of instance it is: `dev`
permits everything, `test` blocks code execution and compiles, and `live` blocks those plus source
control. When no file declares one, the server's IRIS system mode chooses it (Live and Failover:
`live`; Test: `test`; Development: `dev`), so a Live instance is protected without any
configuration. Writes count as execution here: `iris_global` with `action=set` or `kill`, and
`iris_query` with `mode=write`, are treated as execution even though reads from the same
tools are not. Error code: `ENV_GATE_BLOCKED`.

**2. Bulk-data tools.** `journal_search`, `iris_message_body` and `stream_inspect` return stored
message, stream or journal content, so there is no field to inspect before the call. The server's
`dataPolicy` decides, strictest first: `block` (the default) returns at most the content's
metadata, `redact` returns the content with sensitive fields replaced or refuses it if iad cannot
redact it, and `allow` returns it whole. `journal_search` is refused outright unless the policy is
`allow`, with no per-call override. `iris_message_body` and `stream_inspect` are refused under
`block`, return HL7 v2 redacted under `redact` and refuse any other content
(`BODY_NOT_REDACTABLE`), and return the content under `allow`; `stream_inspect` answers a refused
stream with its class, size and line count. `iris_message_body` also takes a per-call
`dataPolicy`, but only to tighten. Error codes: `DATA_POLICY_BLOCKED`, `PHI_POLICY_BLOCKED`,
`BODY_NOT_REDACTABLE`.

**3. System globals.** IRIS keeps compiled classes, routines, roles, users, and
interoperability config in globals such as `^oddDEF`, `^ROUTINE`, `^%Dictionary*`,
`^ROLE`, and `^Ens.Config*`. Writing to them can leave the instance unable to compile or
start. The list names the code-storage globals an instance holds, as exact names (`^rMAC` for
routine source, `^rINC` for include source, and the `^odd*` class-dictionary globals), so an
application global that merely starts the same way, such as `^oddments`, is not blocked. This
blocklist is hardcoded and cannot be switched off — `globalBlocklist` in your
config adds to it, never replaces it. The one exception is `dataPolicyKillAllowlist`,
which exempts named patterns from this check on kill operations only. Error code:
`SYSTEM_BLOCKLIST`.

**4. Globals whose names suggest PHI.** Reading `^PAPMI*`, `^PAADM*`, `^MRADM*`,
`^ORDER*`, and similar requires `acknowledgePhi: true` on the call. This is a speed bump,
not a lock — it exists so nobody pulls a patient record into a chat transcript by
accident. Error code: `PHI_GATE_BLOCKED`.

`iris_message_body` is gated by the stricter of the server's `dataPolicy` and the call's
`dataPolicy` alone: `block` refuses the call (`PHI_POLICY_BLOCKED`), `allow` requires
`acknowledgePhi: true` (`PHI_ACK_REQUIRED`), and `redact` returns the body with the standard HL7 v2
identifying fields replaced by `[REDACTED]`, whatever mix of `\r`, `\n` and `\r\n` ends its segments
(and whatever blanks, `\x0b` or `\x1c` precede a segment name). Redaction only recognizes HL7 v2, so under `redact` anything
else (XML, JSON, plain text, custom bodies) is refused with `BODY_NOT_REDACTABLE` rather than
returned as it is.

---

## Project directory

`iris_compile` (a local file), `kb` index and `iris_symbols_local` read the disk of the machine iad
runs on. They are held to one directory, the project directory: `OBJECTSCRIPT_WORKSPACE`, else
`--workspace`, else the directory of the project file (`.iris-agentic-dev.toml`) found by walking up
from the working directory, else the working directory. It is made canonical.

- A path is allowed when it is inside that directory with symbolic links followed. A link that leads
  out is refused through its target.
- A path with a `..` segment is refused even when it lands inside. A relative path is taken from the
  working directory.
- When no project directory is known, every host path is refused (`reason: "no_workspace"`). The
  filesystem root, the home directory and a directory that does not exist do not count as one; set
  `OBJECTSCRIPT_WORKSPACE` to the project.
- The refusal is `HOST_PATH_BLOCKED`, a fixed rule, with `reason` set to `outside_workspace`,
  `parent_reference`, `extension` or `no_workspace`. It comes before the file is read. For a file,
  the extension is judged first, so the reply does not say whether a file outside the project exists.

## Refusals

A **refusal** is iad declining a call before IRIS runs anything. Every refusal is an error result
(`isError`) whose body keeps `success: false`, its `error_code` and `error`, and adds a
`refusal` object that says which kind it is, so the agent knows who can lift it:

```json
{
  "success": false,
  "error_code": "WRITE_TOOLS_DISABLED",
  "error": "...",
  "refusal": {
    "kind": "untrusted_setting",
    "server": "default",
    "lift": { "setting": "write_tools_enabled" },
    "set_aside": "workspace:write_tools_enabled"
  }
}
```

| `refusal.kind`             | Meaning                                                                                                                                                         | Who lifts it, and `lift`                                                         |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `acknowledgement_required` | The call needs the agent to repeat it with an acknowledgement                                                                                                   | The agent: `lift.argument` is `confirm` or `acknowledgePhi`                      |
| `operator_limit`           | A setting in the operator file, launch settings or a trusted file stands in the way                                                                             | The operator: `lift` is `{setting}`                                              |
| `untrusted_setting`        | A loosening setting would have lifted it, but the file holding it is not trusted                                                                                | The operator, by trusting the file: `refusal.set_aside` names the set-aside item |
| `fixed_rule`               | Nothing in configuration or in the call lifts it (the system blocklist, `SQL_WRITE_BLOCKED`, a source-control write on a `subject` server, the code-edit guard) | Nobody; `lift` is `null`                                                         |

A refusal names the setting and never the operator file's path or the edit that lifts it. An agent
with a file tool that is told "put `write_tools_enabled = true` in `~/.iris-agentic-dev.toml`"
does so. iad writes that remedy to its own stderr, once per setting per process, and
`iris-agentic-dev trust --show` lists it for the operator.

`refusal.server` is the target server whose policy refused. Input errors (`UNKNOWN_PARAMETER`,
`INVALID_PARAMS`, `SERVER_NOT_FOUND`, `CONFIG_INVALID`) and every **IRIS error** (a missing
privilege, a SQL error) are not refusals and carry no `refusal`. IRIS declining a call is never
called a refusal.

---

## Common error codes

| Code                                  | Meaning                                                                                                                                                                                                                                                                                 |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `POLICY_GATE`                         | Call blocked by per-connection policy — see `allow` in `.iris-agentic-dev.toml`                                                                                                                                                                                                         |
| `ENV_GATE_BLOCKED`                    | Tool not permitted by this connection's `mcpTemplate` — see [gates](#data-safety-gates)                                                                                                                                                                                                 |
| `DATA_POLICY_BLOCKED`                 | Bulk-PHI tool called without `dataPolicy = "allow"`                                                                                                                                                                                                                                     |
| `SYSTEM_BLOCKLIST`                    | Global is on the system blocklist — not bypassable                                                                                                                                                                                                                                      |
| `INVALID_GLOBAL_NAME`                 | `global_name` / `global` is not a global name — no subscripts, extended references, or spaces in the name                                                                                                                                                                               |
| `PHI_GATE_BLOCKED`                    | Global name matches a PHI pattern — pass `acknowledgePhi: true`                                                                                                                                                                                                                         |
| `SCOPE_REQUIRED`                      | `iris_search` called without a document scope — pass a `documents` wildcard list                                                                                                                                                                                                        |
| `STALE_CONTENT`                       | `iris_doc` insert/delete_lines `expected` field didn't match stored content                                                                                                                                                                                                             |
| `STORAGE_RESET_REQUIRES_CONFIRMATION` | `iris_doc mode=put` would reset an existing Storage block — pass `allow_storage_regeneration: true` to proceed                                                                                                                                                                          |
| `CODE_EDIT_BLOCKED`                   | `iris_execute` call matched a code-editing pattern — use `iris_doc` + `iris_compile`                                                                                                                                                                                                    |
| `CHECKIN_BLOCKED`                     | SCM CheckIn called without `IRIS_SCM_ALLOW_CHECKIN=1`                                                                                                                                                                                                                                   |
| `SCM_ADMIN_BLOCKED`                   | SCM Disconnect or Reconnect called without `IRIS_SCM_ALLOW_DISCONNECT=1`                                                                                                                                                                                                                |
| `SCM_ACTION_UNPARSEABLE`              | `UserAction` answered with something that is not a whole-number action code, or no answer was written                                                                                                                                                                                   |
| `SCM_ACTION_UNKNOWN`                  | `UserAction` answered with a number outside the documented codes 0 to 7                                                                                                                                                                                                                 |
| `SCM_NEEDS_UI`                        | `UserAction` asked for a web page, a client program, editor text or editor documents (codes 2 to 5) — do that step in your own source-control client                                                                                                                                    |
| `SCM_NEEDS_INPUT`                     | `UserAction` asked for typed text (code 7) at a call that cannot take it                                                                                                                                                                                                                |
| `HTTP_EXECUTION_FAILED`               | Atelier HTTP call failed — check host, port, credentials                                                                                                                                                                                                                                |
| `CONTAINER_UNREACHABLE`               | `docker exec` never reached IRIS — the container is missing or stopped; the message names which                                                                                                                                                                                         |
| `IRIS_UNREACHABLE`                    | No IRIS connection discoverable — run `check_config`                                                                                                                                                                                                                                    |
| `INTEROP_ERROR`                       | Ensemble/interop HTTP call failed — check production state and container access                                                                                                                                                                                                         |
| `SESSION_WS_UNAVAILABLE`              | The instance does not serve the WebSocket terminal endpoint — requires IRIS 2023.2+                                                                                                                                                                                                     |
| `SESSION_WS_DISCONNECTED`             | Session token is invalid or already closed — call `iris_ws_open` to get a new token                                                                                                                                                                                                     |
| `SESSION_STALE`                       | Session outlived its idle window and was reaped — open a new one                                                                                                                                                                                                                        |
| `SESSION_TIMEOUT`                     | IRIS sent no frame within 30 seconds — the statement is still running on the server                                                                                                                                                                                                     |
| `CONFIRM_REQUIRED`                    | `global_kill` requires a `confirm_token` from `global_preview`                                                                                                                                                                                                                          |
| `CONFIRM_EXPIRED`                     | Confirmation token is older than 5 minutes — call `global_preview` again                                                                                                                                                                                                                |
| `CONFIRM_MISMATCH`                    | Token was issued for a different global or server                                                                                                                                                                                                                                       |
| `WRITE_TOOLS_DISABLED`                | Write tool called on a server whose write tier is off — see [Write protection](#write-protection)                                                                                                                                                                                       |
| `DESTRUCTIVE_TOOLS_DISABLED`          | Destructive tool (☠) called on a server whose destructive tier is off                                                                                                                                                                                                                   |
| `DESTRUCTIVE_REQUIRES_WRITES`         | `destructive_tools_enabled = true` set while `write_tools_enabled = false` — invalid config                                                                                                                                                                                             |
| `FETCH_FAILED`                        | `compare_document` could not fetch source from one or both servers                                                                                                                                                                                                                      |
| `HL7_NOT_AVAILABLE`                   | `EnsLib.HL7.Schema` not installed on this instance                                                                                                                                                                                                                                      |
| `SQL_ERROR`                           | `iris_query` statement failed; IRIS's message is in `error`                                                                                                                                                                                                                             |
| `IRIS_RUNTIME_ERROR`                  | `iris_execute` code raised an ObjectScript error; IRIS's text is in `output`                                                                                                                                                                                                            |
| `CONFIG_INVALID`                      | A configuration file or entry does not parse or validate; the message names the file, line and column                                                                                                                                                                                   |
| `PHI_POLICY_BLOCKED`                  | `iris_message_body` under `dataPolicy = "block"`                                                                                                                                                                                                                                        |
| `PHI_ACK_REQUIRED`                    | `iris_message_body` under `allow` without `acknowledgePhi: true`                                                                                                                                                                                                                        |
| `BODY_NOT_REDACTABLE`                 | Content under `dataPolicy = "redact"` that iad cannot redact (anything but HL7 v2)                                                                                                                                                                                                      |
| `COMPILE_TIME_EXEC_BLOCKED`           | A document would run code while it compiles — see the [compile-time gate](connecting.md#compile-time-gate)                                                                                                                                                                              |
| `STORED_SOURCE_UNREADABLE`            | `iris_compile` target exists but its stored source cannot be read                                                                                                                                                                                                                       |
| `COMPILE_SUPERCLASS_STALE`            | A compile would also build an out-of-date superclass outside the library packages; the message names it. Compile it first. A fixed rule                                                                                                                                                 |
| `HOST_PATH_BLOCKED`                   | `iris_compile`, `kb` index or `iris_symbols_local` was given a host path outside the project directory, with a `..` segment, or (compile) not a `.cls`, `.mac`, `.inc` or `.int` file. `reason` is `outside_workspace`, `parent_reference`, `extension` or `no_workspace`. A fixed rule |
| `SUPERCLASS_CHECK_FAILED`             | The superclass walk before a compile could not run or did not finish, so nothing was compiled                                                                                                                                                                                           |
| `SUBJECT_CONFIRM_REQUIRED`            | A write on a `subject` server without `confirm: true`                                                                                                                                                                                                                                   |
| `SUBJECT_SCM_WRITE_BLOCKED`           | A source-control write on a `subject` server; `confirm` does not lift it                                                                                                                                                                                                                |
| `READ_ONLY_STRICT`                    | A scratch-class tool called under `read_only = "strict"`                                                                                                                                                                                                                                |
| `NAME_CONFLICT`                       | `iris_add_server` named an existing Server Manager profile                                                                                                                                                                                                                              |
| `SERVICE_ACCOUNT_*`                   | The agent's code could not run as the service account: `_UNCONFIGURED_FOR_SERVER`, `_WITHHELD`, `_HTTP_DENIED`, `_TERMINAL_DENIED`, `_LOGIN_FAILED`, `_IDENTITY_MISMATCH`, `_INVALID` — see [Troubleshooting](troubleshooting.md#service_account_-errors)                               |
| `SESSION_IDENTITY_CHANGED`            | A WebSocket session's identity no longer matches the configured service account                                                                                                                                                                                                         |
| `DOCKER_TERMINAL_LOGIN_REQUIRED`      | The container's terminal asks for a password, which iad does not send                                                                                                                                                                                                                   |
| `NO_DOCKER_ROUTE_FOR_SERVER`          | Named in the `no_docker_route` field of an `HTTP_EXECUTION_FAILED` response: a named server's HTTP path failed and it has no `container` of its own                                                                                                                                     |
| `SSH_HOST_KEY_MISMATCH`               | The SSH host's key changed since it was recorded                                                                                                                                                                                                                                        |
| `JOURNAL_SEARCH_TIMEOUT`              | `journal_search` outran the HTTP timeout; names the journal file and size                                                                                                                                                                                                               |

`SQL_ERROR` and `IRIS_RUNTIME_ERROR` responses carry a `hint` when iad recognises the error. Seven
rules match, each reproduced on live IRIS first:

| Rule                        | Error                                                                                   |
| --------------------------- | --------------------------------------------------------------------------------------- |
| `sql_objectscript_function` | SQLCODE -12 at a `$`: an ObjectScript function such as `$ZDATETIME` in SQL              |
| `sys_only_class`            | `<CLASS DOES NOT EXIST>` for a `Security.*`, `Config.*` or `SYS.*` class outside `%SYS` |
| `sys_only_table`            | SQLCODE -30 for a `Security.*` or `Config.*` table outside `%SYS`                       |
| `deep_package_table`        | SQLCODE -30 for a class name with three or more parts used as a table                   |
| `double_quoted_string`      | SQLCODE -29 for a string value in double quotes                                         |
| `reserved_word`             | SQLCODE -1 for a reserved word used as a name, such as `AS count`                       |
| `nonstandard_insert`        | `INSERT OR IGNORE`, `INSERT OR REPLACE`, `INSERT IGNORE` or `ON CONFLICT`               |

Any other error comes back without a hint. A hinted response also carries `hint_ref`:

```json
{
  "hint": "Security.Users is a %SYS table, so USER cannot see it. Rerun with namespace: \"%SYS\".",
  "hint_ref": {
    "skill": "iris-agentic-dev",
    "section": "`<CLASS DOES NOT EXIST>` for a system class",
    "why": "Security and Config tables belong to the same %SYS-only classes the section lists."
  }
}
```

`hint` never names a skill; `hint_ref` says which skill section has the fix, for an agent that can
load it. Set `IAD_CODING_PACK=off` (or `0`, `false`, `no`) to drop `hint_ref` and keep `hint`, for a
client that has no iad skills loaded. It is read on every call. The wording lives in
`crates/iris-agentic-dev-core/src/tools/hints.toml`.
