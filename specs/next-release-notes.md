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
between calls. Before this, every `iris_execute` call was a fresh context — a sequence
like "set X, do some work, read X back" had to be one call or use a global to carry
state. Now each step can be separate. Requires IRIS 2023.2+ (Atelier V7 API).

### Administration and cross-instance comparison

Twenty-two new tools across global management, namespace/database admin, observability,
and cross-instance comparison. Most of this section is ported from Pierre Abdelsayed's
Server Manager MCP work — the global confirmation pattern, namespace/database admin,
observability tools, HL7 schema tools, Mermaid diagrams, and `resolve_storage` all
originate from his design. The data safety gates (PHI policy, system globals blocklist,
environment template) are also his, ported to Rust in an earlier release.

- **Global management**: `global_preview` + `global_kill`. Preview returns a confirmation
  token; kill requires it. The token is bound to the specific global and server, expires
  after 5 minutes.
- **Namespace/database**: `iris_namespace_list`, `iris_namespace_create`,
  `iris_database_list`, `iris_database_stats`.
- **Observability**: `journal_search`, `query_audit_log`, `stream_inspect`, `my_access`,
  `capability_matrix`.
- **HL7 schema**: `hl7_schema_list`, `hl7_schema_inspect`. Return `HL7_NOT_AVAILABLE`
  cleanly on IRIS builds without `EnsLib.HL7.Schema` (requires HealthShare or IRIS for Health).
- **Visualization**: `mermaid_class`, `mermaid_production`, `resolve_storage`.
- **Cross-instance comparison**: `compare_document` (unified diff of a single document
  across two servers), `compare_namespace` (full namespace diff).

### MCP ToolAnnotations

All 64 tools now carry MCP `ToolAnnotations`. 57 are tagged `read_only_hint = true`
(queries, introspection, list tools). The 7 destructive tools — `global_kill`,
`iris_admin`, `iris_credential_manage`, `iris_lookup_manage`, `iris_namespace_create`,
`iris_remove_server`, `skill_forget` — are tagged `destructive_hint = true`. Claude Code
and other MCP clients that read these hints can show a confirmation step before
destructive calls and run read-only tools without prompting.

## Notable fixes

### Production management and skill tools broken on Atelier REST connections

`iris_production`, `iris_production_item`, and all skill tools returned errors or empty
results on connections without `IRIS_CONTAINER` set: VS Code extension, remote servers,
anything not running in a named local container. These tools have working Atelier REST
paths; the docker-exec fallback was routing around them. All affected tools now use
Atelier REST.

## Breaking changes

None. All existing tool calls work identically. The `server` parameter defaults to
omitted, so hot-reload and active-connection behavior are unchanged.
`iris_select_container` still works.

**Full changelog:**
[`v0.9.10...v1.0.0`](https://github.com/intersystems-community/iris-agentic-dev/compare/v0.9.10...v1.0.0)
