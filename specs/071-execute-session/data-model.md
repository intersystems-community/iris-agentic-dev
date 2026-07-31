# Data Model: iris_execute Session State (071)

## Error Codes

| Code | When | Detail field |
|------|------|--------------|
| `SESSION_INVALID` | Token fails Base64 validation or `%FromJSON` throws | Error message from `%DynamicObject.%FromJSON` |
| `SESSION_RESTORE_FAILED` | `$classmethod(cls, "%OpenId", id)` throws `<CLASS DOES NOT EXIST>` or returns a non-object | `key:ClassName` — the `%ctx` key and class that failed |
| `SESSION_SERIALIZE_FAILED` | `%ctx.%ToJSON()` or `$system.Encryption.Base64Encode` throws | Error message from IRIS |

## Session Token Format

The `session_state` value returned in the response and accepted as input is:

```
Base64Encode(%ctx.%ToJSON())
```

Where `%ctx` is a `%DynamicObject`. The Base64 alphabet is standard (A-Za-z0-9+/=),
whitespace-stripped by Rust before embedding in generated ObjectScript.

## OID Stub Format

`%Persistent` objects stored in `%ctx` are serialized as:

```json
{"_cls": "Ens.MessageHeader", "_id": "42"}
```

The epilogue scans all top-level `%ctx` keys. Any value that is a live `%Persistent`
object gets replaced with this stub before serialization. On restore the preamble
detects keys whose value has `_cls` defined and re-opens them via
`$classmethod(cls, "%OpenId", id)`.

## Sentinel Lines

Lines written by generated ObjectScript, stripped from visible output by Rust:

| Sentinel prefix | Meaning |
|-----------------|---------|
| `__SESSION_STATE__:` | Followed by the Base64 token |
| `__SESSION_INVALID__:` | Token restore failed; detail follows |
| `__SESSION_RESTORE_FAILED__:` | OID open failed; `key:ClassName` follows |
| `__SESSION_SERIALIZE_FAILED__:` | Epilogue serialization failed; detail follows |
