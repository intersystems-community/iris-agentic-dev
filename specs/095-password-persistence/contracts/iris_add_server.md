# Contract: iris_add_server (095 changes)

## Tool: `iris_add_server`

### Input (unchanged)
```json
{
  "name": "string",
  "host": "string",
  "port": "number",
  "namespace": "string",
  "username": "string",
  "password": "string"
}
```

### Output — keychain success (unchanged)
```json
{ "added": true, "name": "string", "note": "string" }
```

### Output — plaintext fallback (NEW — keychain unavailable, password provided)
```json
{
  "added": true,
  "name": "string",
  "stored_plaintext": true,
  "warning": "Password stored in plaintext in servers.json — use Server Manager for production credentials.",
  "note": "Restart iad for the pool to include this server."
}
```

### Output — keychain error, non-unavailability (unchanged)
```json
{ "error_code": "KEYCHAIN_FAILED", "keychain_unavailable": false, "message": "string" }
```

### Output — no password in keychain-unavailable context (NEW)
When keychain unavailable AND `password` is empty string:
```json
{
  "added": true,
  "name": "string",
  "note": "Restart iad for the pool to include this server."
}
```
No `stored_plaintext` field — nothing to store.

---

## Tool: `iris_servers`

### List entry shape (additive change)
```json
{
  "name": "string",
  "host": "string",
  "port": "number",
  "namespace": "string",
  "username": "string",
  "source": "iad-native | vscode | fleet | env",
  "reachable": null,
  "has_plaintext_credential": "boolean"
}
```

`has_plaintext_credential` is `false` for entries without a plaintext password.
Password value is never included in any response.

---

## Invariants

1. `stored_plaintext` field appears ONLY when a password was actually written to servers.json.
2. `password` key in servers.json is NEVER returned in any tool response.
3. `KEYCHAIN_FAILED` error is returned ONLY for non-unavailability keychain failures.
4. Credential resolution priority: keychain → entry.password → empty string.
