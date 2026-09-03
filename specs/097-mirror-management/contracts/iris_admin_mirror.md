# Contract: iris_admin mirror actions (097)

Tool: `iris_admin`  
Actions: `mirror_add_async`, `mirror_failover`

---

## mirror_add_async

### Request

```json
{
  "action": "mirror_add_async",
  "mirror_name": "MIRSET1",
  "primary_host": "192.168.1.10",
  "primary_port": 2188,
  "instance_name": "IRIS",
  "async_member_type": 0,
  "ssl_enabled": false
}
```

Required: `action`, `mirror_name`, `primary_host`.  
All other fields optional with documented defaults.

### Response — success

```json
{
  "success": true,
  "mirror_name": "MIRSET1",
  "message": "Joined mirror set MIRSET1 as async DR member."
}
```

### Response — already member

```json
{
  "success": false,
  "error_code": "ALREADY_MEMBER",
  "mirror_name": "EXISTINGSET"
}
```

### Response — version mismatch

```json
{
  "success": false,
  "error_code": "MIRROR_VERSION_MISMATCH",
  "error": "<IRIS error text>"
}
```

### Response — missing param

```json
{
  "success": false,
  "error_code": "INVALID_PARAMS",
  "error": "mirror_name is required"
}
```

### Response — write gate disabled

```json
{
  "success": false,
  "error_code": "WRITE_TOOLS_DISABLED"
}
```

---

## mirror_failover

### Request

```json
{
  "action": "mirror_failover"
}
```

Optional: `"confirm": true` (extra guard, not currently enforced but reserved).

Gate: requires `IRIS_DESTRUCTIVE_TOOLS_ENABLED=1`.

### Response — success

```json
{
  "success": true,
  "new_role": "primary"
}
```

### Response — already primary

```json
{
  "success": false,
  "error_code": "ALREADY_PRIMARY"
}
```

### Response — not a member

```json
{
  "success": false,
  "error_code": "NOT_MIRROR_MEMBER"
}
```

### Response — destructive gate disabled

```json
{
  "success": false,
  "error_code": "DESTRUCTIVE_TOOLS_DISABLED"
}
```

### Response — BecomePrimary returned false

```json
{
  "success": false,
  "error": "BecomePrimary returned false — check mirror agent connectivity"
}
```

---

## Gate Classification

| Action             | WriteClass                | Gate env var                     |
| ------------------ | ------------------------- | -------------------------------- |
| `mirror_add_async` | `WriteClass::Write`       | `IRIS_WRITE_TOOLS_ENABLED`       |
| `mirror_failover`  | `WriteClass::Destructive` | `IRIS_DESTRUCTIVE_TOOLS_ENABLED` |

Both must appear as explicit entries in the `mixed("iris_admin", ...)` table in
`crates/iris-agentic-dev-core/src/tools/write_gate.rs` (~line 524).
