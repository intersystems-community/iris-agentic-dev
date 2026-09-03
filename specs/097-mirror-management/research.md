# Research: Mirror Management Tools (097)

**Date**: 2026-09-02  
**Branch**: `097-mirror-management`  
**Verified against**: iris-dev-iris (localhost:52780, iris-community:2026.2)

---

## SYS.Mirror Write Classmethods (verified live)

### JoinMirrorAsAsyncMember

```objectscript
ClassMethod JoinMirrorAsAsyncMember(
    MirrorSetName   As %String,
    SystemName      As %String = "",
    InstanceName    As %String,
    AgentAddress    As %String,
    AgentPort       As %Integer = 2188,
    AsyncMemberType As %Integer,
    ByRef LocalInfo As %String,
    ByRef SSLInfo   As %String
) As %Status
```

**AsyncMemberType values** (from source docs, L724):

- `0` — Disaster Recovery (DR) — **default**
- `1` — Read-Only Reporting
- `2` — Read-Write Reporting

**Notes**:

- `SystemName` defaults to `$SYSTEM` value if omitted. If `$SYSTEM` > 32 chars, omitting
  it causes failure — call `DefaultSystemName()` first.
- `InstanceName`: the IRIS instance name of the _failover member_ to join to (e.g. `IRIS`).
- `AgentAddress`: ISCAgent address of the failover primary (hostname/IP).
- `AgentPort`: ISCAgent port, default 2188.
- `LocalInfo` and `SSLInfo` are pass-by-reference output parameters. Pass empty variables;
  IRIS populates them. They do not need to be pre-set for basic usage.
- Returns `%Status` — check with `$$$ISERR`.

**For iad implementation**: the tool accepts `mirror_name`, `primary_host`, `primary_port`
(default 2188), and exposes them as the three required/defaulted params. `InstanceName`
is mapped from the primary IRIS instance name — default to `"IRIS"` unless provided.
`LocalInfo` and `SSLInfo` are left as empty variables (framework defaults). SSL configuration
via `ssl_enabled`/`ssl_cert_file` maps to `SSLInfo` array entries if enabled.

### BecomePrimary (failover action)

```objectscript
ClassMethod BecomePrimary() As %Boolean  // on SYS.Mirror
```

- Returns `TRUE` = node is now primary (or was already primary)
- Returns `FALSE` = failed to become primary
- Designed for normal takeover when the other node is down or the user explicitly wants
  this node to become primary
- **Overrides** `cstop nofailover` state
- Works by forcing the other node down (destructive — irreversible without manual recovery)

**Note**: `SYS.Mirror.Promote()` is NOT the failover action. It promotes a DR async
member to a failover member (planned cutover scenario). `BecomePrimary()` is the correct
method for promoting a backup failover member to primary.

### Promote (async-to-failover — NOT used by this spec)

```objectscript
ClassMethod Promote(ByRef UnavailableMembers As %String) As %Status
```

- Promotes a DR async member to failover member
- **Out of scope** for this feature — noted here to prevent confusion with failover

---

## %SYSTEM.Mirror Read Classmethods (existing, for pre-flight checks)

Used by `iris_mirror_status_impl` — available for pre-flight in new write actions:

| Method             | Signature | Returns                                            |
| ------------------ | --------- | -------------------------------------------------- |
| `IsMember()`       | `()`      | `%Integer` (0=no, 1=failover, 2=async)             |
| `IsPrimary()`      | `()`      | `%Boolean`                                         |
| `IsBackup()`       | `()`      | `%Boolean`                                         |
| `GetMemberType()`  | `()`      | `%String` ("Primary", "Backup", "AsyncMember", "") |
| `GetMirrorNames()` | `()`      | `%String` (comma list)                             |

---

## Class Distinction

| Class            | Purpose                                   | Namespace |
| ---------------- | ----------------------------------------- | --------- |
| `%SYSTEM.Mirror` | Read-only status queries                  | `%SYS`    |
| `SYS.Mirror`     | Write operations (join, failover, create) | `%SYS`    |

Both must be called in `%SYS` namespace via `ZN "%SYS"` prefix.

---

## Implementation Pattern

From `iris_mirror_status_impl` (admin_tools.rs L573):

```rust
let code = r#"ZN "%SYS"
Set tMember=##class(%SYSTEM.Mirror).IsMember()
...
Write tMember,"|",..."#;
match iris.execute_via_generator(code, "%SYS", client).await {
    Ok(out) => { /* parse pipe-delimited output */ }
    Err(e) => { /* return error json */ }
}
```

New write actions follow same pattern. For `mirror_add_async`:

```objectscript
ZN "%SYS"
// pre-flight check
Set tMember=##class(%SYSTEM.Mirror).IsMember()
If tMember'=0 {
  Write "ALREADY_MEMBER|",##class(%SYSTEM.Mirror).GetMirrorNames(),!
  Quit
}
// join async
Set tSC=##class(SYS.Mirror).JoinMirrorAsAsyncMember(mirrorName,"",instanceName,agentAddr,agentPort,0,.tLocalInfo,.tSSLInfo)
If $$$ISERR(tSC) {
  Write "ERROR:",$System.Status.GetErrorText(tSC),!
} Else {
  Write "OK",!
}
```

For `mirror_failover`:

```objectscript
ZN "%SYS"
Set tMember=##class(%SYSTEM.Mirror).IsMember()
If tMember=0 {
  Write "NOT_MEMBER",!
  Quit
}
Set tPrimary=##class(%SYSTEM.Mirror).IsPrimary()
If tPrimary {
  Write "ALREADY_PRIMARY",!
  Quit
}
Set tResult=##class(SYS.Mirror).BecomePrimary()
If tResult {
  Write "OK",!
} Else {
  Write "ERROR:BecomePrimary returned false",!
}
```

---

## Constraints from Community IRIS

- Community IRIS (iris-dev-iris) is not configured as a mirror member.
- `IsMember()` returns `0` on the dev container.
- Live round-trip tests for add/failover require a live mirror set (`IRIS_MIRROR_PRIMARY`
  env var gates these tests).
- The binary/unit tests are sufficient for gate verification without a live mirror.

---

## Gate Classification

| Action             | WriteClass                | Env var gate                     |
| ------------------ | ------------------------- | -------------------------------- |
| `mirror_add_async` | `WriteClass::Write`       | `IRIS_WRITE_TOOLS_ENABLED`       |
| `mirror_failover`  | `WriteClass::Destructive` | `IRIS_DESTRUCTIVE_TOOLS_ENABLED` |

Both must be added to the `mixed("iris_admin", ...)` table in `write_gate.rs` (~line 524).

---

## Alternatives Considered

- **Using `SYS.Mirror.AddFailoverMember`** for `mirror_add_async`: rejected — that adds
  a failover member to an existing set, not an async member. `JoinMirrorAsAsyncMember`
  is the correct API.
- **Using `SYS.Mirror.Promote`** for failover: rejected — `Promote` converts DR async to
  failover member, not backup-to-primary failover. `BecomePrimary` is correct.
- **Exposing `ssl_cert_file` in v1**: included as optional. If `ssl_enabled=false`, SSL
  params are ignored — no behavior change.
