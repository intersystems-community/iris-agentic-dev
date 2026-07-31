# 072-c: Administration and Cross-Instance Comparison — Release Notes

## What's new

Twenty-two new tools across four areas: global management, namespace/database admin,
observability, and cross-instance comparison.

### Global management

**`global_preview`** shows the top N subscripts of a global and returns a confirmation
token. **`global_kill`** deletes the global, but only after you present the token. The
token binds to the specific global name and server, expires after 5 minutes, and is
consumed on use. No token, no kill. This replaces the single-step kill in `iris_global`
for operations where a forced pause makes sense.

### Namespace and database admin

**`iris_namespace_list`**, **`iris_namespace_create`** (write-gated), **`iris_database_list`**,
and **`iris_database_stats`** let you list what exists, check sizes,
and create a workspace namespace for a new project. `iris_namespace_create` uses
`Config.Namespaces.CreateOne` — the correct API on 2026.2.

### Observability

**`journal_search`** scans the IRIS journal for set/kill records in a time window.
**`query_audit_log`** queries `%SYS_Audit.Log` for login, privilege, and resource events.
**`stream_inspect`** reads a stream object by OID — useful when a message body reference
points to a `%Stream.GlobalCharacter` rather than inline text.

**`my_access`** and **`capability_matrix`** report current session privileges and
cross-namespace role coverage. Good for checking what a service account can reach before
running a migration.

### HL7 schema

**`hl7_schema_list`** and **`hl7_schema_inspect`** cover the HL7 2.x schema installed on
the instance. Both return `HL7_NOT_AVAILABLE` cleanly on Community builds that ship
without Ensemble.

### Visualization

**`mermaid_class`** walks the `Super` hierarchy for one or more classes and emits a
Mermaid class diagram. **`mermaid_production`** diagrams a running production — hosts,
connections, and enabled/disabled state. **`resolve_storage`** shows the `^oddDEF`
storage layout for a persistent class.

### Cross-instance comparison

**`compare_document`** fetches a class or routine from two named servers and returns a
unified diff. **`compare_namespace`** compares all classes in a namespace across two
servers — lists what's only in A, only in B, and what differs.

## Why it matters

The multi-instance pool from 072-a made it possible to talk to several IRIS instances in
one session. This phase makes that useful for real migration work: compare dev against
prod before a deploy, check namespace contents after a copy, inspect globals before
killing them, trace a HL7 message from inbound to its stored stream object.

## Compatibility

All new tools follow the same `server` routing from 072-a. Passing `server: "prod"` on
any 072-c tool routes to the `prod` instance in the pool. No changes to existing tools.
Write-gated tools (`global_kill`, `iris_namespace_create`) respect `write_tools_enabled`
from config — they do nothing on connections where writes are off.
