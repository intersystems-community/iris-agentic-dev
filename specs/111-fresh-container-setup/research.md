# Phase 0 Research: 099-fresh-container-setup

**Date**: 2026-09-02
**IRIS version**: 2026.2 Community (iris-dev-iris, localhost:52780)

## Constitution Gate II: ObjectScript Sanity

All API calls verified against live `iris-dev-iris` before writing any code.

---

## API 1: `%SYSTEM.Security.ChangePassword`

**Method**: `##class(%SYSTEM.Security).ChangePassword(Username, NewPassword, OldPassword, &Status)`

**Verification**:

```objectscript
ZN "%SYS"
Write ##class(%Dictionary.CompiledMethod).%ExistsId("%SYSTEM.Security||ChangePassword"),!
// Output: 1  (method exists)

Set m=##class(%Dictionary.CompiledMethod).%OpenId("%SYSTEM.Security||ChangePassword")
Write m.FormalSpec,!
// Output: Username:%String,NewPassword:%String,OldPassword:%String,&Status:%Status

Write m.ReturnType,!
// Output: %Library.Boolean
```

**Key finding**: Argument order is `(Username, NewPassword, OldPassword, &Status)` — NOT `(u, p, np)` as suggested in spec FR-002. The spec's description "ChangePassword(u, p, np)" maps to `(Username=u, NewPassword=np_or_p, OldPassword=p)`.

**For fresh container (no-op password change to clear the flag)**:

```objectscript
ZN "%SYS"
Set result=##class(%SYSTEM.Security).ChangePassword("_SYSTEM","SYS","SYS",.sc)
// Output: Result:1   SC:1
```

**Idempotent confirmation**: Calling `ChangePassword` when the flag is already cleared returns 1 (success).

**Flag clearing verified**:

```objectscript
// Set flag to 1
Set modProps("ChangePassword")=1
Set sc=##class(Security.Users).Modify("_SYSTEM",.modProps)
// Output: Set flag sc:1

// Confirm flag is set
Set sc2=##class(Security.Users).Get("_SYSTEM",.p2)
Write p2("ChangePassword"),!
// Output: 1

// Clear it via ChangePassword
Set result=##class(%SYSTEM.Security).ChangePassword("_SYSTEM","SYS","SYS",.sc3)
// Output: ChangePassword result:1

// Confirm flag is cleared
Set sc4=##class(Security.Users).Get("_SYSTEM",.p3)
Write p3("ChangePassword"),!
// Output: 0
```

---

## API 2: `Security.Users.UnlockUser` — Does NOT Exist

**Spec claim**: `##class(Security.Users).UnlockUser(username)` — **WRONG**. Verified against live IRIS:

```objectscript
ZN "%SYS"
Write ##class(%Dictionary.CompiledMethod).%ExistsId("Security.Users||UnlockUser"),!
// Output: 0  (method does NOT exist)
```

**Correct approach**: Use `##class(Security.Users).Modify(username, .props)` with `InvalidLoginAttempts=0`.

**Verification**:

```objectscript
ZN "%SYS"
// Unlock via Modify: reset failed login counter
Set props("InvalidLoginAttempts")=0
Set sc=##class(Security.Users).Modify("_SYSTEM",.props)
Write "Modify sc:",sc,!
// Output: Modify sc:1
```

**Method signature confirmed**:

```objectscript
Set m=##class(%Dictionary.CompiledMethod).%OpenId("Security.Users||Modify")
Write m.FormalSpec,!
// Output: Username:%String,&Properties:%String
```

**Idempotent**: Resetting `InvalidLoginAttempts` to 0 on an already-unlocked account is safe and returns success.

---

## Security.Users Properties Relevant to This Feature

Verified property names from `%Dictionary.CompiledProperty:Summary` on `Security.Users`:

| Property               | Purpose                                                           |
| ---------------------- | ----------------------------------------------------------------- |
| `ChangePassword`       | 1 = user is forced to change password at next login               |
| `InvalidLoginAttempts` | Count of consecutive failed logins; non-zero means account locked |
| `Enabled`              | 1 = account active                                                |
| `InvalidLoginStatus`   | Text status of invalid login state                                |

---

## Existing `admin::admin_*_impl` Pattern

All admin write functions in `admin.rs` follow this structure:

1. Accept `iris: Option<&IrisConnection>` as first arg
2. Return `Result<CallToolResult, McpError>`
3. Create HTTP client: `IrisConnection::http_client()`
4. Execute via `iris.execute_via_generator(code, "%SYS", &client)`
5. Return `ok_json(...)` or `err_json(code, msg)`

**Connection switching**: Functions use `execute_via_generator(code, "%SYS", client)` — the namespace is passed directly. No explicit `ZN "%SYS"` needed in the code string because the executor already targets `%SYS`. However, for `execute_via_generator` the namespace parameter sets the execution namespace, so it is equivalent to switching namespaces.

**Write gate**: Not enforced inside `admin.rs` — `call_tool` in `mod.rs` enforces it via the `write_gate.rs` table before dispatching to any impl function.

---

## write_gate.rs Classification

Current `mixed("iris_admin", ...)` table (line ~524) has the default `WriteClass::Destructive`. The three new actions must be added explicitly as `WriteClass::Write` to prevent over-classification.

Gate env var: `IRIS_WRITE_TOOLS_ENABLED` (not `IRIS_ADMIN_TOOLS` — that is stale documentation).

---

## No New Crate Dependencies

No new Rust crates required. All implementation uses existing dependencies:
`serde_json`, `tokio`, `rmcp` — all already in workspace.

---

## Summary of Verified Calls

| Action                       | ObjectScript                                                                         | Namespace | Verified                |
| ---------------------------- | ------------------------------------------------------------------------------------ | --------- | ----------------------- |
| `clear_password_change_flag` | `##class(%SYSTEM.Security).ChangePassword(user,newpwd,oldpwd,.sc)`                   | `%SYS`    | Yes                     |
| `unlock_user`                | `##class(Security.Users).Modify(user,.props)` with `props("InvalidLoginAttempts")=0` | `%SYS`    | Yes                     |
| `fresh_container_setup`      | Calls both in sequence                                                               | `%SYS`    | Yes (both individually) |

**Spec correction required**: FR-003 and spec assumption reference `##class(Security.Users).UnlockUser` — this method does not exist. Implementation will use `Security.Users.Modify` with `InvalidLoginAttempts=0` instead. This is functionally equivalent and achieves the same result.
