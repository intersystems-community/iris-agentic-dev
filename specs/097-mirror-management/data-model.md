# Data Model: Mirror Management Tools (097)

---

## MirrorAddAsyncParams

Input to `iris_admin action=mirror_add_async`.

| Field               | Type             | Required | Default  | Notes                                   |
| ------------------- | ---------------- | -------- | -------- | --------------------------------------- |
| `mirror_name`       | `String`         | yes      | —        | Mirror set name; uppercase alphanumeric |
| `primary_host`      | `String`         | yes      | —        | AgentAddress of primary failover member |
| `primary_port`      | `u16`            | no       | `2188`   | ISCAgent port                           |
| `instance_name`     | `String`         | no       | `"IRIS"` | IRIS instance name on the primary       |
| `async_member_type` | `u8`             | no       | `0`      | 0=DR, 1=ReadOnly, 2=ReadWrite           |
| `ssl_enabled`       | `bool`           | no       | `false`  | Enable SSL for mirror replication       |
| `ssl_cert_file`     | `Option<String>` | no       | `None`   | Path to SSL CA cert file                |

---

## MirrorAddAsyncResult

```json
{
  "success": true,
  "mirror_name": "MIRSET1",
  "message": "Joined mirror set MIRSET1 as async DR member."
}
```

Error variants:

```json
{ "success": false, "error_code": "ALREADY_MEMBER", "mirror_name": "MIRSET1" }
{ "success": false, "error_code": "MIRROR_VERSION_MISMATCH", "error": "..." }
{ "success": false, "error_code": "INVALID_PARAMS", "error": "mirror_name is required" }
{ "success": false, "error_code": "WRITE_TOOLS_DISABLED" }
```

---

## MirrorFailoverParams

Input to `iris_admin action=mirror_failover`.

| Field     | Type   | Required | Default | Notes                                                                 |
| --------- | ------ | -------- | ------- | --------------------------------------------------------------------- |
| `confirm` | `bool` | yes      | —       | Must be `true`; if `false` or missing returns `CONFIRMATION_REQUIRED` |

---

## MirrorFailoverResult

```json
{ "success": true, "new_role": "primary" }
```

Error variants:

```json
{ "success": false, "error_code": "CONFIRMATION_REQUIRED", "error": "confirm must be true to execute failover" }
{ "success": false, "error_code": "ALREADY_PRIMARY" }
{ "success": false, "error_code": "NOT_MIRROR_MEMBER" }
{ "success": false, "error_code": "DESTRUCTIVE_TOOLS_DISABLED" }
{ "success": false, "error_code": "MIRROR_FAILOVER_FAILED", "error": "BecomePrimary returned false" }
```

---

## Error Codes

| Code                         | Trigger                                                                                                                                                               |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `INVALID_PARAMS`             | Missing required field or bad type                                                                                                                                    |
| `ALREADY_MEMBER`             | `%SYSTEM.Mirror.IsMember()` != 0 before add; `mirror_name` in response is the value from `GetMirrorNames()`, which returns a comma-separated list — parse accordingly |
| `CONFIRMATION_REQUIRED`      | `mirror_failover` called with `confirm` missing or `false`                                                                                                            |
| `MIRROR_VERSION_MISMATCH`    | ObjectScript error string contains `"version"` or `"incompatible"` (case-insensitive match)                                                                           |
| `ALREADY_PRIMARY`            | `%SYSTEM.Mirror.IsPrimary()` = true before failover                                                                                                                   |
| `NOT_MIRROR_MEMBER`          | `%SYSTEM.Mirror.IsMember()` = 0 before failover                                                                                                                       |
| `MIRROR_JOIN_FAILED`         | `JoinMirrorAsAsyncMember` returned error not matching version/SSL patterns                                                                                             |
| `MIRROR_FAILOVER_FAILED`     | `SYS.Mirror.BecomePrimary` returned false or error                                                                                                                     |
| `WRITE_TOOLS_DISABLED`       | `IRIS_WRITE_TOOLS_ENABLED` not set                                                                                                                                    |
| `DESTRUCTIVE_TOOLS_DISABLED` | `IRIS_DESTRUCTIVE_TOOLS_ENABLED` not set                                                                                                                              |

### Version-Mismatch Detection

The `MIRROR_VERSION_MISMATCH` error is detected by pattern-matching the ObjectScript error
string returned by `JoinMirrorAsAsyncMember`. Match strings containing `"version"` or
`"incompatible"` (case-insensitive). These patterns come from the IRIS error text surfaced
by `$System.Status.GetErrorText()` when the mirror primary rejects a member with an
incompatible version. If neither pattern matches, surface as `MIRROR_JOIN_FAILED`.

---

## Rust Structs (sketch)

```rust
// in admin_tools.rs or a params module

#[derive(Debug, serde::Deserialize)]
struct MirrorAddAsyncParams {
    mirror_name: String,
    primary_host: String,
    #[serde(default = "default_agent_port")]
    primary_port: u16,
    #[serde(default = "default_instance_name")]
    instance_name: String,
    #[serde(default)]
    async_member_type: u8,
    #[serde(default)]
    ssl_enabled: bool,
    ssl_cert_file: Option<String>,
}

fn default_agent_port() -> u16 { 2188 }
fn default_instance_name() -> String { "IRIS".to_string() }

#[derive(Debug, serde::Deserialize)]
struct MirrorFailoverParams {
    confirm: bool,  // required; must be true or return CONFIRMATION_REQUIRED
}
```
