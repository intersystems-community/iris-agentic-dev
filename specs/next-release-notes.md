> Staging file. When the next tag is cut, this becomes the GitHub release body via
> `gh release edit <tag> --notes`. The constitution requires What's new / Notable fixes /
> Breaking changes plus a `v0.9.10...<tag>` compare link.
> There is no `CHANGELOG.md` in this repo; the release body is the changelog.
>
> Written against `v1.0.0` as the next tag.

## What's new

### Multi-instance connection pool

Every execution tool now accepts an optional `server` parameter. Pass a server name and
the call routes to that instance without touching the default connection:

```json
{ "tool": "iris_execute", "server": "prod", "code": "Write $ZV" }
```

iad maintains its own server registry at `~/.config/iris-agentic-dev/servers.json`.
Passwords go to the OS keychain — never to the config file — using the same service name
as VS Code Server Manager, so both tools share credentials automatically.

Five new tools manage the pool: `iris_servers`, `iris_add_server`, `iris_remove_server`,
`iris_test_server`, and `iris_import_servers`.

### Persistent WebSocket terminal sessions

Three new tools give Claude a persistent ObjectScript terminal over WebSocket:
`iris_ws_open`, `iris_ws_exec`, `iris_ws_close`. Variables and process state persist
between calls — no need to bundle a whole sequence into one `iris_execute`. Requires
IRIS 2023.2+ (Atelier V7 API).

### Administration and cross-instance comparison

Twenty-two new tools covering global management, namespace/database admin, observability,
and cross-instance comparison:

- **Global management**: `global_preview` + `global_kill`. Preview returns a confirmation
  token; kill requires it. The token is bound to the specific global and server, expires
  after 5 minutes.
- **Namespace/database**: `iris_namespace_list`, `iris_namespace_create`,
  `iris_database_list`, `iris_database_stats`.
- **Observability**: `journal_search`, `query_audit_log`, `stream_inspect`, `my_access`,
  `capability_matrix`.
- **HL7 schema**: `hl7_schema_list`, `hl7_schema_inspect`. Return `HL7_NOT_AVAILABLE`
  cleanly on Community builds.
- **Visualization**: `mermaid_class`, `mermaid_production`, `resolve_storage`.
- **Cross-instance comparison**: `compare_document` (unified diff of a single document
  across two servers), `compare_namespace` (full namespace diff).

### MCP ToolAnnotations

All 64 tools now expose MCP `ToolAnnotations`. 57 tools carry `read_only_hint = true`.
The 7 destructive tools (`global_kill`, `iris_admin`, `iris_credential_manage`,
`iris_lookup_manage`, `iris_namespace_create`, `iris_remove_server`, `skill_forget`) carry
`destructive_hint = true`. MCP clients that respect these hints can gate confirmation
dialogs or run read-only tools in parallel without prompts.

## Notable fixes

### Production management and skill tools broken on Atelier REST connections

`iris_production`, `iris_production_item`, and all skill tools returned errors or empty
results on connections without `IRIS_CONTAINER` set — VS Code extension, remote servers,
anything not running in a named local container. The docker-exec fallback was incorrectly
used in paths that have working Atelier REST endpoints. Fixed across all affected tools.

## Breaking changes

None. All existing tool calls work identically. The `server` parameter defaults to `None`,
preserving current active-connection behavior including hot-reload.
`iris_select_container` still works.

**Full changelog:**
[`v0.9.10...v1.0.0`](https://github.com/intersystems-community/iris-agentic-dev/compare/v0.9.10...v1.0.0)
