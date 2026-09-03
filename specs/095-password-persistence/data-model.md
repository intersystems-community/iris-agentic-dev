# Data Model: 095-password-persistence

## ServerEntry (servers_config.rs)

```rust
pub struct ServerEntry {
    pub host: String,
    pub port: u16,
    pub namespace: String,
    pub username: String,
    pub description: Option<String>,
    pub scheme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,   // NEW — plaintext fallback credential
}
```

### Serde behavior
- Existing entries without `"password"` key → `password: None` (serde default)
- New entry with plaintext fallback → `"password": "<value>"` in servers.json
- `skip_serializing_if = "Option::is_none"` — omits key when None

### servers.json example (plaintext fallback)
```json
{
  "servers": {
    "myserver": {
      "host": "localhost",
      "port": 52773,
      "namespace": "USER",
      "username": "_SYSTEM",
      "password": "SYS"
    }
  }
}
```

## Error codes

| Code | Condition | Changed? |
|------|-----------|---------|
| `KEYCHAIN_FAILED` | keychain failed, non-availability reason | unchanged |
| (success) | `added: true, stored_plaintext: true` | NEW — replaces KEYCHAIN_FAILED for unavailability + password present |

## iris_add_server response shapes

### Keychain success (unchanged)
```json
{ "added": true, "name": "...", "note": "Restart iad for the pool to include this server." }
```

### Plaintext fallback (new)
```json
{
  "added": true,
  "name": "...",
  "stored_plaintext": true,
  "warning": "Password stored in plaintext in servers.json — use Server Manager for production credentials.",
  "note": "Restart iad for the pool to include this server."
}
```

### Keychain error, non-unavailability (unchanged)
```json
{ "error_code": "KEYCHAIN_FAILED", "keychain_unavailable": false, "message": "..." }
```

## iris_servers list entry (additive)

```json
{
  "name": "...",
  "host": "...",
  "port": 1972,
  "namespace": "USER",
  "username": "_SYSTEM",
  "source": "iad-native",
  "reachable": null,
  "has_plaintext_credential": true
}
```

`has_plaintext_credential: false` when `ServerEntry.password` is `None`.
Password value never exposed.
