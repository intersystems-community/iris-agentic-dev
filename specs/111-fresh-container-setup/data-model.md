# Data Model: 099-fresh-container-setup

## Rust Structs

### `FreshSetupResult`

Returned by `iris_admin action="fresh_container_setup"`.

```rust
pub struct FreshSetupResult {
    pub success: bool,
    pub ready: bool,
    pub steps: Vec<SetupStep>,
}

pub struct SetupStep {
    pub action: String,
    pub status: SetupStepStatus,
    pub detail: String,
}

pub enum SetupStepStatus {
    Ok,
    Skipped,
    Error,
}
```

### JSON representation

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

`ready` is `true` when all steps return `"ok"` or `"skipped"`. It is `false` if any step returns `"error"`.

---

## Parameter Structs (inline, derived from `AnyParams`)

### `clear_password_change_flag` params

| Parameter      | Type   | Default            | Required |
| -------------- | ------ | ------------------ | -------- |
| `username`     | string | `"_SYSTEM"`        | No       |
| `password`     | string | `"SYS"`            | No       |
| `new_password` | string | same as `password` | No       |

When `new_password` is omitted, the call sets `NewPassword = password`, effectively a no-op that only clears the forced-change flag.

### `unlock_user` params

| Parameter  | Type   | Default | Required |
| ---------- | ------ | ------- | -------- |
| `username` | string | —       | Yes      |

Missing `username` → `INVALID_PARAMS` before any IRIS call.

### `fresh_container_setup` params

| Parameter      | Type   | Default            | Required |
| -------------- | ------ | ------------------ | -------- |
| `username`     | string | `"_SYSTEM"`        | No       |
| `password`     | string | `"SYS"`            | No       |
| `new_password` | string | same as `password` | No       |

---

## Error Code Registry

New error codes introduced by this feature (SCREAMING_SNAKE_CASE):

| Code                     | Meaning                                                           |
| ------------------------ | ----------------------------------------------------------------- |
| `PASSWORD_CHANGE_FAILED` | `%SYSTEM.Security.ChangePassword` returned false or non-OK status |
| `UNLOCK_FAILED`          | `Security.Users.Modify` returned non-OK status code               |

Standard codes reused (do not redefine):

| Code                   | Usage here                                                                |
| ---------------------- | ------------------------------------------------------------------------- |
| `IRIS_UNREACHABLE`     | No connection or HTTP failure                                             |
| `WRITE_TOOLS_DISABLED` | `IRIS_WRITE_TOOLS_ENABLED` not set (enforced in `call_tool`, not in impl) |
| `INVALID_PARAMS`       | `username` missing for `unlock_user`                                      |

---

## Response Shape: `clear_password_change_flag` (success)

```json
{
  "success": true,
  "username": "_SYSTEM",
  "flag_cleared": true
}
```

## Response Shape: `clear_password_change_flag` (error)

```json
{
  "success": false,
  "error_code": "PASSWORD_CHANGE_FAILED",
  "error": "ChangePassword returned false for user _SYSTEM: <status text>"
}
```

## Response Shape: `unlock_user` (success)

```json
{
  "success": true,
  "username": "_SYSTEM",
  "unlocked": true
}
```

## Response Shape: `unlock_user` (error)

```json
{
  "success": false,
  "error_code": "UNLOCK_FAILED",
  "error": "Modify returned error for user _SYSTEM: <IRIS status text>"
}
```

## Response Shape: `fresh_container_setup` (success)

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

## Response Shape: `fresh_container_setup` (partial failure)

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
