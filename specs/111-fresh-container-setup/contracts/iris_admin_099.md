# Contract: iris_admin — New Actions (099-fresh-container-setup)

This document defines the input/output contract for the three new `iris_admin` actions added
in spec 099.

---

## Action: `clear_password_change_flag`

**Write class**: `WriteClass::Write`
**Gate**: `IRIS_WRITE_TOOLS_ENABLED`
**Namespace**: `%SYS`
**IRIS API**: `##class(%SYSTEM.Security).ChangePassword(Username, NewPassword, OldPassword, &Status)`

### Input schema

```json
{
  "tool": "iris_admin",
  "params": {
    "action": "clear_password_change_flag",
    "username": "_SYSTEM",
    "password": "SYS",
    "new_password": "SYS"
  }
}
```

All fields except `action` are optional. Defaults: `username="_SYSTEM"`, `password="SYS"`,
`new_password=password`.

### Success output

```json
{
  "success": true,
  "username": "_SYSTEM",
  "flag_cleared": true
}
```

### Error outputs

```json
{ "success": false, "error_code": "WRITE_TOOLS_DISABLED", "error": "..." }
{ "success": false, "error_code": "IRIS_UNREACHABLE", "error": "..." }
{ "success": false, "error_code": "PASSWORD_CHANGE_FAILED", "error": "ChangePassword returned false for user _SYSTEM: <status text>" }
```

### Idempotency

Calling `clear_password_change_flag` on an account where the flag is already cleared
returns `success: true`. The `ChangePassword` API returns 1 in both cases.

---

## Action: `unlock_user`

**Write class**: `WriteClass::Write`
**Gate**: `IRIS_WRITE_TOOLS_ENABLED`
**Namespace**: `%SYS`
**IRIS API**: `##class(Security.Users).Modify(Username, &Properties)` with `Properties("InvalidLoginAttempts")=0`

> **Implementation note**: Use `##class(Security.Users).Modify(username, .props)` with
> `props("InvalidLoginAttempts")=0`. `Security.Users.UnlockUser()` does not exist on
> IRIS 2026.2 (verified: compiled method lookup returns 0). `Modify` with
> `InvalidLoginAttempts=0` resets the failed login counter and unblocks the account.

### Input schema

```json
{
  "tool": "iris_admin",
  "params": {
    "action": "unlock_user",
    "username": "_SYSTEM"
  }
}
```

`username` is required. Missing `username` returns `INVALID_PARAMS`.

### Success output

```json
{
  "success": true,
  "username": "_SYSTEM",
  "unlocked": true
}
```

### Error outputs

```json
{ "success": false, "error_code": "WRITE_TOOLS_DISABLED", "error": "..." }
{ "success": false, "error_code": "INVALID_PARAMS", "error": "username is required for unlock_user" }
{ "success": false, "error_code": "IRIS_UNREACHABLE", "error": "..." }
{ "success": false, "error_code": "UNLOCK_FAILED", "error": "Modify returned error for user _SYSTEM: <IRIS status text>" }
```

### Idempotency

Resetting `InvalidLoginAttempts=0` on an account that is already unlocked returns success.
Safe to call on any account at any time.

---

## Action: `fresh_container_setup`

**Write class**: `WriteClass::Write`
**Gate**: `IRIS_WRITE_TOOLS_ENABLED`
**Namespace**: `%SYS`
**Steps**: `clear_password_change_flag` then `unlock_user`, in that order.

### Input schema

```json
{
  "tool": "iris_admin",
  "params": {
    "action": "fresh_container_setup",
    "username": "_SYSTEM",
    "password": "SYS",
    "new_password": "SYS"
  }
}
```

All fields except `action` are optional. Same defaults as `clear_password_change_flag`.

### Success output (all steps ok)

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

### Partial failure output (step error — continues)

```json
{
  "success": false,
  "ready": false,
  "steps": [
    { "action": "clear_password_change_flag", "status": "ok", "detail": "" },
    { "action": "unlock_user", "status": "error", "detail": "Modify returned error: ..." }
  ]
}
```

### Error outputs

```json
{ "success": false, "error_code": "WRITE_TOOLS_DISABLED", "error": "..." }
{ "success": false, "error_code": "IRIS_UNREACHABLE", "error": "..." }
```

> `IRIS_UNREACHABLE` before any step runs. Per-step errors are surfaced inside `steps[]`,
> not as top-level `error_code`, so the caller can see which steps succeeded.

### Idempotency

The entire sequence is idempotent. Calling it on an already-configured container is safe.
`ready: true` is returned whenever no step reports `"error"`.

---

## write_gate.rs changes

The `mixed("iris_admin", ...)` table in `write_gate.rs` must gain three new entries:

```rust
("clear_password_change_flag", WriteClass::Write),
("unlock_user", WriteClass::Write),
("fresh_container_setup", WriteClass::Write),
```

These must appear before the closing `WriteClass::Destructive` default. Without them, the
actions fall through to `Destructive`, which requires a separate gate flag.

---

## INVALID_ACTION error update

The `_ =>` arm in `iris_admin`'s match block must be updated to include the three new
action names in its error message.
