# 072-b: WebSocket Terminal Sessions — Release Notes

## What's new

Three new tools give Claude a persistent ObjectScript terminal session over WebSocket:

**`iris_ws_open`** opens a session and returns a token. The session lives on the IRIS
side — variables, open devices, and process state persist between calls.

**`iris_ws_exec`** runs ObjectScript code in the open session. A variable set in the
first call is readable in the second. No need to rebuild state on every execute.

**`iris_ws_close`** shuts the session down and frees its resources.

Sessions are keyed by `ws:{server}:{NAMESPACE}:{uuid}`. You can open multiple sessions
in different namespaces or against different named servers at the same time.

Requires IRIS 2026.2+ with Atelier V7 API. The toolset checks the API version at
connection time — `iris_ws_open` returns `WS_TERMINAL_NOT_SUPPORTED` on older instances
rather than connecting and failing mid-handshake.

## Why it matters

Before this, every `iris_execute` call was a fresh context. A sequence like "set X, do
some work, read X back" required bundling all three steps into one call or storing
intermediate state in a global. Now each step can be a separate call. Debugging a loop,
testing an incremental build, or walking through a complex routine step-by-step all work
as natural back-and-forth.

## Compatibility

No changes to existing tools. The `server` routing from 072-a applies to WS sessions —
`iris_ws_open(server: "prod")` opens a session against the `prod` instance in the pool.
