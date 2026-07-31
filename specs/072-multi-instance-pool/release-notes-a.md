# What's New: Multi-Instance Connection Pool

## Named server routing

Every execution tool now accepts an optional `server` parameter. Pass a server name and
the call routes to that instance, leaving the active connection alone:

```json
{ "tool": "iris_execute", "server": "prod", "code": "Write $ZV" }
```

No restart, no config edit, no side effects on other calls. Omit `server` and behavior is
identical to before.

## Server registry

iad now maintains its own server registry at `~/.config/iris-agentic-dev/servers.json`.
Passwords go to the OS keychain — never to the config file — using the same service name
as VS Code Server Manager. If VS Code Server Manager is also installed, both tools read
from the same keychain entries; no password re-entry required.

The pool loads from all sources at startup:

1. `~/.config/iris-agentic-dev/servers.json` (iad-native, highest priority)
2. VS Code `settings.json` → `intersystems.servers`
3. Cursor settings
4. `.iris-agentic-dev.toml` `[instance.*]` blocks
5. `IRIS_HOST` / `IRIS_WEB_PORT` env vars

## Five new tools

**`iris_servers`** — list every known server with host, port, namespace, source, and
reachability.

**`iris_add_server`** — register a server from the CLI. Works without VS Code, without
any editor, on headless servers and in CI.

**`iris_remove_server`** — remove a server from the iad-native registry and keychain.

**`iris_test_server`** — probe a server and get back Atelier API version, IRIS version,
and round-trip latency. Does not change the active connection.

**`iris_import_servers`** — pull all servers from VS Code / Cursor Server Manager into
the iad-native registry in one call. Reads credentials from the existing OS keychain
automatically, so no passwords to re-enter.

## Backward compatibility

All existing tool calls work identically. The `server` parameter defaults to `None`, which
preserves the current active connection behavior including hot-reload.
`iris_select_container` still works.
