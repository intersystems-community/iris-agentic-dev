# Quickstart: Fresh Container Setup (099)

Three new `iris_admin` actions that handle the standard IRIS first-boot sequence.
All three require `IRIS_WRITE_TOOLS_ENABLED=1`.

---

## Prerequisites

```bash
export IRIS_WRITE_TOOLS_ENABLED=1
export IRIS_HOST=localhost
export IRIS_PORT=52780
```

---

## Scenario 1: Full first-boot sequence (recommended)

Call a single action that clears the forced-change flag and unlocks the account:

```json
{
  "tool": "iris_admin",
  "params": {
    "action": "fresh_container_setup"
  }
}
```

Default behavior: runs against `_SYSTEM` with password `SYS`. Response:

```json
{
  "success": true,
  "ready": true,
  "steps": [
    { "action": "clear_password_change_flag", "status": "ok", "detail": "" },
    { "action": "unlock_user", "status": "ok", "detail": "" }
  ]
}
```

When `ready: true`, subsequent `iris_execute` and `iris_query` calls will succeed.

---

## Scenario 2: Custom password or user

```json
{
  "tool": "iris_admin",
  "params": {
    "action": "fresh_container_setup",
    "username": "_SYSTEM",
    "password": "SYS",
    "new_password": "myNewPassword"
  }
}
```

When `new_password` differs from `password`, `clear_password_change_flag` changes the
password while clearing the flag.

---

## Scenario 3: Clear the forced-change flag only

```json
{
  "tool": "iris_admin",
  "params": {
    "action": "clear_password_change_flag"
  }
}
```

Defaults to `_SYSTEM` / `SYS`. Returns:

```json
{
  "success": true,
  "username": "_SYSTEM",
  "flag_cleared": true
}
```

---

## Scenario 4: Unlock a specific account

```json
{
  "tool": "iris_admin",
  "params": {
    "action": "unlock_user",
    "username": "Admin"
  }
}
```

Returns:

```json
{
  "success": true,
  "username": "Admin",
  "unlocked": true
}
```

---

## Error: write gate not enabled

Without `IRIS_WRITE_TOOLS_ENABLED=1`:

```json
{
  "success": false,
  "error_code": "WRITE_TOOLS_DISABLED",
  "error": "Set IRIS_WRITE_TOOLS_ENABLED=1 to enable write operations."
}
```

---

## Idempotency

All three actions are safe to call on containers that are already configured. Calling
`fresh_container_setup` on a running, configured IRIS instance returns `ready: true`
with each step reporting `"ok"`.
