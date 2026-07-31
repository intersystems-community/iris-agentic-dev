# Data Model: Multi-Instance Connection Pool (072)

## Error Code Registry

All error codes introduced by this feature. Standard codes (`IRIS_UNREACHABLE`,
`INVALID_PARAMS`, etc.) are defined in the project constitution and not repeated here.

| Code                      | Tool(s)                                          | Meaning                                                                                                                                       |
| ------------------------- | ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `SERVER_NOT_FOUND`        | All tools with `server` param                    | Named server not registered in the connection pool.                                                                                           |
| `SERVER_UNREACHABLE`      | `iris_test_server`, any tool with `server` param | Server is known but currently unreachable (TCP refused, timeout, or auth failure).                                                            |
| `REMOVE_NOT_ALLOWED`      | `iris_remove_server`                             | Server was sourced from VS Code / Cursor settings or fleet config, not iad-native config. Remove from source (edit settings.json directly).   |
| `IMPORT_NO_SOURCE`        | `iris_import_servers`                            | No VS Code or Cursor `settings.json` found on this system.                                                                                    |
| `SESSION_STALE`           | `iris_ws_exec`, `iris_ws_close`                  | WS session token references a server name no longer in the connection pool (server was removed or pool not refreshed).                        |
| `SESSION_WS_DISCONNECTED` | `iris_ws_exec`                                   | WS connection to IRIS dropped (network interruption, IRIS restart). Auto-reconnect was attempted and failed.                                  |
| `SESSION_WS_UNAVAILABLE`  | `iris_ws_open`                                   | IRIS Atelier API version < 7. WS terminal requires IRIS 2023.2+. Use `iris_execute` with `use_session: true` instead.                         |
| `SESSION_IN_USE`          | `iris_ws_exec`                                   | Concurrent `iris_ws_exec` calls on the same session token. A session may only have one in-flight execution at a time.                         |
| `SESSION_TIMEOUT`         | `iris_ws_exec`                                   | No response from IRIS within `IRIS_WS_TIMEOUT_SECS` seconds (default: 30). The session is left open; retry or close with `iris_ws_close`.     |
| `CONFIRM_REQUIRED`        | `global_kill`                                    | Destructive tool called without a confirmation token. Call `global_preview` first to obtain a token.                                          |
| `CONFIRM_EXPIRED`         | `global_kill`                                    | Confirmation token is older than 5 minutes. Call `global_preview` again.                                                                      |
| `CONFIRM_MISMATCH`        | `global_kill`                                    | Confirmation token was issued for a different global or server.                                                                               |
| `HL7_NOT_AVAILABLE`       | `hl7_schema_list`, `hl7_schema_inspect`          | `EnsLib.HL7.Schema` class does not exist on this IRIS instance. HL7 schema tools require the Interoperability HL7 components to be installed. |

## iad-native Config File

**Path**: `~/.config/iris-agentic-dev/servers.json` (macOS/Linux),
`%APPDATA%\iris-agentic-dev\servers.json` (Windows)

**Version**: `1`

**Schema**:

```json
{
  "version": 1,
  "servers": {
    "<name>": {
      "host": "string — hostname or IP",
      "port": 52780,
      "namespace": "string — default namespace",
      "username": "string",
      "description": "string (optional)",
      "scheme": "http | https (optional, default http)"
    }
  },
  "default": "string — name of default server (optional)"
}
```

Passwords are **never written** to this file. They are stored in the OS keychain
under service `"intersystems-server-credentials"`,
account `"credentialProvider:<server-name>/<username-lowercase>"`.

## WS Session Token Format

```text
ws:<server-name>:<NAMESPACE>:<uuid-v4>
```

- `server-name`: URL-safe string matching a pool entry name
- `NAMESPACE`: uppercase IRIS namespace
- `uuid-v4`: random UUID, no hyphens stripped (standard format: `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`)

Example: `ws:dev:USER:3f2504e0-4f89-11d3-9a0c-0305e82c3301`

Pool key: `(server_name, namespace, uuid)` — triple must match exactly on lookup.

## `iris_servers` Response Shape

```json
[
  {
    "name": "prod",
    "host": "iris-prod.internal",
    "port": 52780,
    "namespace": "USER",
    "username": "admin",
    "description": "Production IRIS",
    "scheme": "http",
    "source": "iad-native | vscode | cursor | fleet | env",
    "reachable": true | false | null
  }
]
```

`reachable: null` = not yet tested (lazy pool). Use `iris_test_server` to probe.

## `iris_import_servers` Response Shape

```json
{
  "imported": 3,
  "skipped": 1,
  "no_keychain": [{ "name": "old-dev", "reason": "no keychain entry found" }]
}
```

`no_keychain` items have `name` (server name) and `reason` (string). Server is still
imported into iad-native config; it will prompt for credentials on first connection.

## `iris_test_server` Response Shape

```json
{
  "name": "dev",
  "reachable": true,
  "auth": true,
  "atelier_version": "v8",
  "iris_version": "IRIS for UNIX (Apple M1 Pro) 2026.2 (Build 0) ...",
  "latency_ms": 14,
  "namespaces": ["USER", "%SYS", "HSLIB"]
}
```

When `reachable: true` but `auth: false` — TCP reachable but credentials rejected.
When `reachable: false` — network or DNS failure; `auth`, `atelier_version`,
`iris_version`, `latency_ms`, `namespaces` are all `null`.

## `global_preview` Response Shape

```json
{
  "global": "TempData",
  "server": "dev",
  "entries": [
    { "subscripts": ["key1"], "value": "..." },
    { "subscripts": ["key2", "sub"], "value": "..." }
  ],
  "total_subscripts": 12345,
  "confirm_token": "3f2504e0-4f89-11d3-9a0c-0305e82c3301",
  "confirm_expires": "2026-07-30T18:23:00Z"
}
```

`count` defaults to 20 if not specified. `total_subscripts` is the `$Order` count
(best-effort — may be slow on large globals; cap at 10,000).

## `compare_document` Response Shape

```json
{
  "document": "Ens.MessageHeader.cls",
  "server_a": "prod",
  "server_b": "staging",
  "same": false,
  "diff": "--- prod/Ens.MessageHeader.cls\n+++ staging/Ens.MessageHeader.cls\n@@ ..."
}
```

When `same: true`, `diff` is an empty string.

## `compare_namespace` Response Shape

```json
{
  "namespace": "USER",
  "server_a": "prod",
  "server_b": "staging",
  "only_in_a": ["Foo.Bar.cls", "Foo.Baz.cls"],
  "only_in_b": ["Test.Scratch.cls"],
  "different": ["Ens.MessageHeader.cls"],
  "same_count": 4201
}
```
