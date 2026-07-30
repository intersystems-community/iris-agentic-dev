# What's New in v0.9.11

## `iris_execute` session state

`iris_execute` now carries state between calls without touching IRIS. Set `use_session: true`
and the tool injects `%ctx` — a `%DynamicObject` — before your code runs. Store whatever you
need in `%ctx.key`. The response includes a `session_state` token; pass it back on the next
call and `%ctx` is restored exactly where you left it.

```text
# Call 1 — run a query and stash the result
iris_execute(
  use_session=true,
  code="Set %ctx.count = 1247"
)
# → { "output": "", "session_state": "eyJjb3VudCI6MTI0N..." }

# Call 2 — use it without rerunning the query
iris_execute(
  use_session=true,
  session_state="eyJjb3VudCI6MTI0N...",
  code="Write %ctx.count * 0.05"
)
# → { "output": "62.35" }
```

`%Persistent` objects stored in `%ctx` serialize to OID stubs (`{"_cls": ..., "_id": ...}`)
and are re-opened by `%OpenId` on restore. Nested `%DynamicObject` values survive unchanged.

Nothing is written to IRIS. The token is an opaque Base64 string held by the client.

Three new error codes cover failure cases: `SESSION_INVALID` (bad token),
`SESSION_RESTORE_FAILED` (class missing or bad OID), `SESSION_SERIALIZE_FAILED`.

Closes [#32](https://github.com/intersystems-community/iris-agentic-dev/issues/32).
