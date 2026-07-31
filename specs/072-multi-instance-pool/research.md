# Research: Multi-Instance Connection Pool (072)

**Branch**: `072-multi-instance-pool` | **Date**: 2026-07-30

## Dependency Justification (Constitution §VII)

### `tokio-tungstenite`

**Crate**: `tokio-tungstenite = { version = "0.26", features = ["native-tls"] }`

**Justification**: WebSocket client support for the Atelier v7 terminal endpoint
(`/api/atelier/v7/{namespace}/terminal`). This endpoint is the only mechanism for a
persistent IRIS process — it is not reachable via HTTP REST or `execute_via_generator`.

**Standard library alternative**: None. `std::net::TcpStream` does not speak WebSocket
protocol (RFC 6455 framing, upgrade handshake, ping/pong, binary/text frame distinction).

**Existing workspace check**: No WebSocket crate is currently in the workspace.
`reqwest` handles HTTP but does not support WebSocket upgrades.

**Scope**: 072-b only. 072-a (connection pool, server registry) adds no new deps.

**Binary size impact**: `tokio-tungstenite` + `tungstenite` add approximately 200–400 KB
to the release binary. Acceptable given the capability added.

**Precedent**: The 025 tree-sitter crates (three deps for a specific parsing capability)
are the model. This is one dep for a specific protocol capability unavailable otherwise.

### `uuid` (workspace — already present)

`uuid = { version = "1", features = ["v4", "serde"] }` is already in
`crates/iris-agentic-dev-core/Cargo.toml`. Used by `execute_via_generator` for temp
class name generation. 072-b reuses it for WS session token UUIDs — no new dep.

### `similar` (072-c — pending verification)

Before adding `similar` for diff output in `compare_document`, run:

```bash
cargo tree -p iris-agentic-dev-core | grep similar
```

If absent: add `similar = { version = "2" }`. Justification: `compare_document`'s entire
user value is the unified diff output. Hand-rolling Myers diff in ≤30 lines is not
realistic — the algorithm is O(ND) and requires edit-script reconstruction. `similar` is
a well-maintained, zero-unsafe crate with no transitive deps beyond `bstr`.

## `IrisConnection` Construction Safety

`IrisConnection::new()` (`connection.rs:98`) sets struct fields only — no network calls,
no HTTP, no async. Verified by reading the implementation: fields are `String`, `u16`,
`AtelierVersion`, `SystemMode`. Safe to construct lazily per pool entry at startup
without triggering any IRIS connection.

**Decision confirmed**: Use lazy construction (Option 1 from spec). `ConnectionPool`
builds `IrisConnection` instances at startup; actual HTTP only happens on first tool call.

## ObjectScript API Verification (Constitution §II)

All APIs below MUST be verified against a live IRIS 2026.2 instance (`iris-dev-iris`,
port 52780) before the implementing task begins. Mark each row VERIFIED / FAILED /
NEEDS VERIFICATION.

### 072-a: Server Pool

No new ObjectScript APIs. `execute_via_generator` and `GET /api/atelier/` are verified
from prior work.

### 072-b: WebSocket Terminal

| API / Behavior                                                           | Purpose              | Status                                                                             |
| ------------------------------------------------------------------------ | -------------------- | ---------------------------------------------------------------------------------- |
| `GET /api/atelier/v7/%25SYS/terminal` WebSocket upgrade                  | WS terminal endpoint | VERIFIED — IRIS 2026.2, HTTP 101, server sends `init` frame immediately            |
| `CSPSESSIONID-SP-{port}-UP-api-atelier-` cookie from `GET /api/atelier/` | WS auth mechanism    | VERIFIED — path-scoped cookie; Basic auth on WS upgrade is accepted but not needed |
| WS JSON frame protocol (`init`, `config`, `prompt`, `output`)            | Message sequence     | VERIFIED — see protocol notes below                                                |

**CRITICAL CORRECTIONS from live verification (2026-07-30):**

1. **URL is always `%25SYS/terminal`**, not `/:namespace/terminal`. The v7 UrlMap route is
   `/%SYS/terminal` — the namespace is `%SYS`, not the target namespace. The target
   namespace is sent in the `config` message.
2. **Output frame key is `"text"`, not `"data"`**. Frame: `{"type":"output","text":"42"}`.
3. **Prompt frame**: `{"type":"prompt","text":"\e[1mUSER>\e[0m","ns":"USER"}` —
   includes ANSI codes in `text` and current namespace in `ns`.
4. **Cookie name is path-scoped**: `CSPSESSIONID-SP-52780-UP-api-atelier-` — use
   `response.cookies` dict, not `response.cookies.get("CSPSESSIONID")`.
5. **Server sends `init` first** — no client-first message needed.
   `{"type":"init","protocol":1,"version":"IRIS for UNIX ..."}`.

**Verified protocol sequence:**

```text
Client  →  WS upgrade (with CSPSESSIONID cookie)  →  Server
Server  →  {"type":"init","protocol":1,"version":"..."}
Client  →  {"type":"config","namespace":"USER","rawMode":false}
Server  →  {"type":"prompt","text":"\e[1mUSER>\e[0m","ns":"USER"}
Client  →  {"type":"prompt","input":"Write 42"}
Server  →  {"type":"output","text":"42"}
Server  →  {"type":"prompt","text":"\e[1mUSER>\e[0m","ns":"USER"}
```

Additional client message types: `read` (for READ input), `interrupt`, `color` (syntax highlight request).

Cookie extraction (use all cookies, not just CSPSESSIONID):

```python
s = requests.Session()
s.get("http://host/api/atelier/", auth=(user, pw))
cookies = "; ".join(f"{k}={v}" for k, v in s.cookies.items())
# Use cookies string as Cookie header on WS upgrade
```

### 072-c: Ported Tools

| API                                                                                                    | Tool                      | Status / Correction                                                                                                                                                                                                                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------ | ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `##class(%SYS.Namespace).ListAll(&array)`                                                              | `iris_namespace_list`     | VERIFIED — returns `%SYS`, `USER`. Use `$O(array(key))` loop. Note: `For` syntax fails in session; use `Set key=$O(...)` loop.                                                                                                                                                                                                                           |
| `##class(Config.Namespaces).CreateOne(&props)`                                                         | `iris_namespace_create`   | VERIFIED (method exists). `%SYS.Namespace` has NO `Create()` method. Use `Config.Namespaces.CreateOne(&props)` in `%SYS` namespace. Requires `props("Globals")`, `props("Routines")`.                                                                                                                                                                    |
| `##class(Config.Databases).List()` → does NOT exist                                                    | `iris_database_list`      | CORRECTED — `Config.Databases.List()` does not exist. Use `##class(%SYS.Namespace).ListAll(&ns)` for namespace→db mapping. For size, use `SYS.Database.GetFreeSpace(dir, &free, &blocks)`. VERIFIED.                                                                                                                                                     |
| `%SYS.Journal.File` / `%SYS.Journal.SetKillRecord`                                                     | `journal_search`          | VERIFIED — `##class(%SYS.Journal.File).%OpenId(##class(%SYS.Journal.System).GetCurrentFileName())`. Iterate via `rec = jf.FirstRecord` / `rec = rec.Next`. Record properties: `TypeName`, `TimeStamp`, `JobID`, `Next`. SetKillRecord adds: `GlobalReference`, `DatabaseName`, `NewValue`, `OldValue`. Note: `FileSize` property does NOT exist on File. |
| `SELECT Event, EventType, Username, UTCTimeStamp FROM %SYS.Audit`                                      | `query_audit_log`         | VERIFIED — SQL `SQLCODE=0`, returns rows. Use `%SYS.Audit` table via SQL query in `%SYS` namespace.                                                                                                                                                                                                                                                      |
| `##class(%Stream.GlobalCharacter).%OpenId(id)`                                                         | `stream_inspect`          | VERIFIED — id format is just the integer id (not full OID). Works in USER namespace. Binary: `%Stream.GlobalBinary.%OpenId(id)`.                                                                                                                                                                                                                         |
| `##class(%Stream.GlobalBinary).%OpenId(id)`                                                            | `stream_inspect` (binary) | VERIFIED — same pattern as GlobalCharacter.                                                                                                                                                                                                                                                                                                              |
| `$USERNAME` + `SELECT Name, FullName, Roles FROM Security.Users WHERE Name=?`                          | `my_access`               | VERIFIED — use `%SYS` namespace, parameter binding with variable (not literal `$USERNAME` in SQL). Returns `irisowner`, roles `%All`.                                                                                                                                                                                                                    |
| `##class(EnsLib.HL7.Schema).GetSchemaList()`                                                           | `hl7_schema_list`         | NOT AVAILABLE on this 2026.2 Community instance. `EnsLib.HL7.Schema` class does not exist. Return `HL7_NOT_AVAILABLE` error. Check with `##class(%Dictionary.CompiledClass).%ExistsId("EnsLib.HL7.Schema")` before calling.                                                                                                                              |
| `##class(EnsLib.HL7.Schema).GetSegmentStructure(schema, seg)`                                          | `hl7_schema_inspect`      | NOT AVAILABLE — same as above.                                                                                                                                                                                                                                                                                                                           |
| `SELECT Name, Super FROM %Dictionary.CompiledClass WHERE Name=?`                                       | `mermaid_class`           | VERIFIED — `Super` is a comma-separated list. Walk recursively. VERIFIED on `%Library.Persistent` → `%Library.SwizzleObject`.                                                                                                                                                                                                                            |
| `SELECT Name, DataLocation, IdLocation, IndexLocation FROM %Dictionary.CompiledStorage WHERE parent=?` | `resolve_storage`         | VERIFIED — SQLCODE=0, returns Default storage with `DataLocation=^Ens.MessageHeaderD`. VERIFIED on `Ens.MessageHeader`.                                                                                                                                                                                                                                  |
| `##class(SYS.Database).GetFreeSpace(dir, &free, &blocks)`                                              | `iris_database_stats`     | VERIFIED — returns MB free (float) and block count. Run in `%SYS`. Use `##class(%SYS.Namespace).ListAll()` to get ns→db mapping, then call per-database.                                                                                                                                                                                                 |

**Verification rule**: Each row must be changed to VERIFIED (with IRIS version and output
confirmed) before the implementing task begins. If a method does not exist on 2026.2,
update the implementation approach and note the alternative.

### Known-good APIs (from prior work)

| API                                         | Verified in               |
| ------------------------------------------- | ------------------------- |
| `execute_via_generator` pattern             | 071, 070, all prior specs |
| `%Dictionary.CompiledClass` — basic queries | 070 (iris_symbols_local)  |
| `Ens.Config.Production` — item queries      | 056 (iris_production)     |
| `%DynamicObject.%ToJSON` / `%FromJSON`      | 071                       |
| `$system.Encryption.Base64Encode/Decode`    | 071                       |
