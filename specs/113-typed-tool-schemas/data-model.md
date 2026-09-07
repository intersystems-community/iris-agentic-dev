# Phase 1 Data Model: Typed input schemas for every MCP tool

This feature creates no persistent data. Its entities are the tool surface itself: the
parameter contract each tool advertises, and the one new response shape a rejected call
produces.

## Entities

### Parameter contract

One per tool. Materialized as a Rust struct deriving `Deserialize` and `JsonSchema`, whose
generated schema becomes the tool's advertised `inputSchema`.

| Field                    | Meaning                                                     | Source                                |
| ------------------------ | ----------------------------------------------------------- | ------------------------------------- |
| `properties`             | One entry per parameter the handler reads                   | Struct fields                         |
| `required`               | Parameters with no default                                  | Fields without `#[serde(default)]`    |
| `additionalProperties`   | `false` — nothing beyond the declared set                   | `#[serde(deny_unknown_fields)]`       |
| `description` (per prop) | One line, only where the name is not self-explanatory       | Rust doc comment on the field         |
| `enum` (per prop)        | Permitted values where the handler branches on a closed set | `#[schemars(extend("enum" = [...]))]` |

**Invariant (FR-003)**: `properties.keys()` equals the set of names the handler reads via
`p.get("…")`. Both directions are asserted.

**Invariant (FR-007)**: a parameter whose necessity depends on another parameter's value is
never in `required`. `iris_admin` therefore requires `action` alone.

### Parameter

| Attribute      | Values                                                                            |
| -------------- | --------------------------------------------------------------------------------- |
| name           | snake_case, unchanged from today (renaming is out of scope)                       |
| type           | `string`, `integer`, `boolean`, `object`, or `["integer", "string"]` (two params) |
| optionality    | optional with a default, or required                                              |
| value set      | closed (declared as `enum`) or open                                               |
| conditionality | unconditional, or meaningful only for certain values of a sibling parameter       |

### Parameter audit

Per tool, the three-way comparison of what `docs/tools.md` promises, what the schema
advertises, and what the handler reads. Not a runtime artifact — the disagreement list FR-009
requires be emptied, recorded in `parameter-audit.md` as each batch lands.

Rows look like: tool, parameter, documented?, advertised?, read?, resolution, which source was
wrong. `stream_inspect` / `max_chars` is the known first row: documented yes, read no.

## Verified inventory — the 31 tools

Extracted from `crates/iris-agentic-dev-core/src/tools/mod.rs` by pulling every
`p.get("…")` out of each `Parameters<AnyParams>` handler body. **132 parameter slots, 71
distinct names.** `server` appears in 26 of the 31.

| Tool                      | Line | Parameters read                                                                                                                                                                                                                                                                                           | n   |
| ------------------------- | ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --- |
| `iris_interop_query`      | 6851 | body_class, body_select, body_where, component, limit, log_type, message_class, namespace, search_table, server, session_id, since_id, source, target, what                                                                                                                                               | 15  |
| `iris_containers`         | 6949 | action, name                                                                                                                                                                                                                                                                                              | 2   |
| `iris_production_item`    | 6996 | action, item, namespace, server, settings                                                                                                                                                                                                                                                                 | 5   |
| `iris_message_body`       | 7056 | max_bytes, message_id, namespace, server                                                                                                                                                                                                                                                                  | 4   |
| `iris_business_rule_info` | 7123 | action, namespace, rule_name, server                                                                                                                                                                                                                                                                      | 4   |
| `iris_production_diff`    | 7175 | namespace, production, server                                                                                                                                                                                                                                                                             | 3   |
| `iris_credential_list`    | 7223 | namespace                                                                                                                                                                                                                                                                                                 | 1   |
| `iris_credential_manage`  | 7248 | action, id, namespace, password, username                                                                                                                                                                                                                                                                 | 5   |
| `iris_lookup_manage`      | 7294 | action, key, namespace, table, value                                                                                                                                                                                                                                                                      | 5   |
| `iris_lookup_transfer`    | 7333 | action, namespace, table, xml                                                                                                                                                                                                                                                                             | 4   |
| `iris_admin`              | 7394 | action, async_member_type, code_database, confirm, data_database, dispatch_class, enabled, full_name, global_pattern, instance_name, max_records, mirror_name, name, namespace, new_password, password, path, permission, primary_host, primary_port, resource, roles, server, time_range, type, username | 26  |
| `compare_document`        | 8439 | document, namespace, server_a, server_b                                                                                                                                                                                                                                                                   | 4   |
| `compare_namespace`       | 8486 | namespace, server_a, server_b                                                                                                                                                                                                                                                                             | 3   |
| `global_preview`          | 8529 | count, global, server                                                                                                                                                                                                                                                                                     | 3   |
| `global_kill`             | 8566 | confirm_token, global, server                                                                                                                                                                                                                                                                             | 3   |
| `iris_namespace_list`     | 8609 | server                                                                                                                                                                                                                                                                                                    | 1   |
| `iris_database_list`      | 8628 | server                                                                                                                                                                                                                                                                                                    | 1   |
| `iris_system_performance` | 8646 | mode, profile, run_id, server                                                                                                                                                                                                                                                                             | 4   |
| `iris_mirror_status`      | 8685 | server                                                                                                                                                                                                                                                                                                    | 1   |
| `iris_namespace_create`   | 8704 | db_path, name, server                                                                                                                                                                                                                                                                                     | 3   |
| `iris_database_stats`     | 8734 | db, server                                                                                                                                                                                                                                                                                                | 2   |
| `journal_search`          | 8757 | end, global_pattern, max_entries, server, start                                                                                                                                                                                                                                                           | 5   |
| `query_audit_log`         | 8794 | end, event_type, limit, server, start, user                                                                                                                                                                                                                                                               | 6   |
| `stream_inspect`          | 8835 | namespace, oid, server                                                                                                                                                                                                                                                                                    | 3   |
| `my_access`               | 8866 | server                                                                                                                                                                                                                                                                                                    | 1   |
| `capability_matrix`       | 8885 | server, user                                                                                                                                                                                                                                                                                              | 2   |
| `hl7_schema_list`         | 8911 | namespace, server                                                                                                                                                                                                                                                                                         | 2   |
| `hl7_schema_inspect`      | 8935 | namespace, schema, segment, server                                                                                                                                                                                                                                                                        | 4   |
| `mermaid_class`           | 8977 | class, depth, namespace, server                                                                                                                                                                                                                                                                           | 4   |
| `mermaid_production`      | 9008 | namespace, production, server                                                                                                                                                                                                                                                                             | 3   |
| `resolve_storage`         | 9039 | class, namespace, server                                                                                                                                                                                                                                                                                  | 3   |

Line numbers are `async fn` positions as of commit `a720d2f` and will drift; the tests locate
handlers by name, not by line.

### Non-string parameters

Everything not listed here is read with `.as_str()`. These are the ones where the declared type
is not `string`, each confirmed by reading its accessor:

| Tool                   | Parameter           | Declared type           | Handler accessor                                    |
| ---------------------- | ------------------- | ----------------------- | --------------------------------------------------- |
| `iris_interop_query`   | `limit`             | `integer`               | `as_u64().unwrap_or(50) as u32`                     |
| `iris_interop_query`   | `session_id`        | `["integer", "string"]` | `as_i64().or_else(as_str → parse::<i64>)`           |
| `iris_interop_query`   | `since_id`          | `["integer", "string"]` | `as_i64().or_else(as_str → parse::<i64>)`           |
| `iris_interop_query`   | `search_table`      | `object`                | `.cloned()` — passed through untyped                |
| `iris_production_item` | `settings`          | `object`                | `as_object()` then iterated                         |
| `iris_message_body`    | `max_bytes`         | `integer`               | `as_u64().map(as u32).unwrap_or(65536)`             |
| `iris_admin`           | `confirm`           | `boolean`               | `as_bool().unwrap_or(false)`                        |
| `iris_admin`           | `enabled`           | `boolean`               | `as_bool()` (and `.unwrap_or(true)` for one action) |
| `iris_admin`           | `primary_port`      | `integer`               | `as_u64().unwrap_or(2188) as u16`                   |
| `iris_admin`           | `async_member_type` | `integer`               | `as_u64().unwrap_or(0) as u8`                       |
| `iris_admin`           | `max_records`       | `integer`               | `as_u64()`                                          |
| `global_preview`       | `count`             | `integer`               | `as_u64().unwrap_or(20) as u32`                     |
| `journal_search`       | `max_entries`       | `integer`               | `as_u64().unwrap_or(100) as u32`                    |
| `query_audit_log`      | `limit`             | `integer`               | `as_u64().unwrap_or(100) as u32`                    |
| `mermaid_class`        | `depth`             | `integer`               | `as_u64().unwrap_or(3) as u32`                      |

`session_id` and `since_id` are the only two parameters in the whole set that accept a numeric
string. They keep that tolerance via a `StringOrI64` helper (`#[serde(untagged)]`); the rest
already discard strings, so declaring them `integer` turns a silent discard into a rejection
rather than breaking a working call. See research decision 3.

### Closed value sets

Each of these becomes an `enum` in the schema, asserted against the literals the handler
matches on:

| Tool                      | Parameter | Values (from the handler's match arms) |
| ------------------------- | --------- | -------------------------------------- |
| `iris_system_performance` | `mode`    | `start`, `status`, `last_runid`        |
| `iris_interop_query`      | `what`    | per handler dispatch; default `logs`   |
| `iris_containers`         | `action`  | per handler dispatch; default `list`   |
| `iris_production_item`    | `action`  | per handler dispatch                   |
| `iris_business_rule_info` | `action`  | per handler dispatch                   |
| `iris_credential_manage`  | `action`  | per handler dispatch                   |
| `iris_lookup_manage`      | `action`  | per handler dispatch                   |
| `iris_lookup_transfer`    | `action`  | per handler dispatch                   |
| `iris_admin`              | `action`  | per handler dispatch — the largest set |

The exact arm lists are enumerated per batch during implementation rather than transcribed
here, because the test reads them from the handler and a copy in this document would be a
second source of truth. That is the point of FR-005's "asserted against the values the handler
branches on, not against the prose."

## Error codes

Per the constitution's Error Code Registry, every code is `SCREAMING_SNAKE_CASE` and documented
before use.

| Code                | New? | Meaning                                                                                       | Emitted by                      |
| ------------------- | ---- | --------------------------------------------------------------------------------------------- | ------------------------------- |
| `UNKNOWN_PARAMETER` | yes  | The call named a parameter this tool does not accept (FR-015)                                 | the `call_tool` validation site |
| `INVALID_PARAMS`    | no   | A parameter was present but its value is not permitted, or a required one was absent (FR-006) | handlers, unchanged             |
| `INVALID_ACTION`    | no   | The `action` parameter named a verb the tool does not have                                    | handlers, unchanged             |

`UNKNOWN_PARAMETER` versus `INVALID_PARAMS` is what satisfies FR-017: a caller can tell "I sent
a name you do not know" from "I sent a value you do not permit" by the code alone, without
parsing the message. `INVALID_ACTION` is the same distinction one level in, and it matters
because `iris_admin.action` is the largest enum on the surface. `MISSING_PARAMS` appears nowhere
in the constitution's registry and is not used here; a missing required parameter is
`INVALID_PARAMS`.

Gate codes (`WRITE_TOOLS_DISABLED`, `DESTRUCTIVE_TOOLS_DISABLED`,
`DESTRUCTIVE_REQUIRES_WRITES`) are unchanged and continue to take precedence — a gate refusal
is returned before parameter validation runs.

## State transitions

One, per call, in `call_tool`:

```text
request → gate check ──refused──→ gate error (WRITE_TOOLS_DISABLED, …)
             │ passed
             ▼
        key validation ──unknown key──→ UNKNOWN_PARAMETER, no work done
             │ all keys advertised
             ▼
        router dispatch ──deserialize fail──→ INVALID_PARAMS, no work done
             │ deserialized
             ▼
          handler ──bad value──→ INVALID_PARAMS
                  ──bad action──→ INVALID_ACTION
             │
             ▼
           success
```

The first two arrows are the FR-016 guarantee: neither rejection reaches the handler, so a
destructive tool cannot half-apply a mis-specified call. The third arrow already behaves this
way — rmcp deserializes before invoking the handler, verified by probe.
