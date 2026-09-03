# Quickstart: Mirror Management Tools (097)

## Prerequisites

- iad configured with a connection to the IRIS instance to be managed
- `IRIS_WRITE_TOOLS_ENABLED=1` for `mirror_add_async`
- `IRIS_DESTRUCTIVE_TOOLS_ENABLED=1` for `mirror_failover`
- The target IRIS instance must be in `%SYS` — tools run in that namespace automatically

---

## Add an async DR member

Check current mirror status first:

```json
{ "action": "mirror_status" }
// → { "is_member": false, ... }  ← good, proceed
```

Join an existing mirror set as DR async:

```json
{
  "action": "mirror_add_async",
  "mirror_name": "MIRSET1",
  "primary_host": "primary.example.com",
  "primary_port": 2188
}
```

Expected success response:

```json
{
  "success": true,
  "mirror_name": "MIRSET1",
  "message": "Joined mirror set MIRSET1 as async DR member."
}
```

---

## Fail over to backup (destructive)

Verify the instance is a backup and not already primary:

```json
{ "action": "mirror_status" }
// → { "is_member": true, "is_primary": false, "member_type": "Backup" }
```

Promote to primary (requires destructive gate):

```json
{ "action": "mirror_failover" }
```

Expected success:

```json
{ "success": true, "new_role": "primary" }
```

---

## Error handling

| Error code                   | Meaning                      | Fix                                    |
| ---------------------------- | ---------------------------- | -------------------------------------- |
| `ALREADY_MEMBER`             | Instance already in a mirror | Remove from current mirror first       |
| `MIRROR_VERSION_MISMATCH`    | IRIS versions incompatible   | Match versions between members         |
| `ALREADY_PRIMARY`            | Already the primary          | No failover needed                     |
| `NOT_MIRROR_MEMBER`          | Not in any mirror            | Cannot fail over non-member            |
| `WRITE_TOOLS_DISABLED`       | Write gate off               | Set `IRIS_WRITE_TOOLS_ENABLED=1`       |
| `DESTRUCTIVE_TOOLS_DISABLED` | Destructive gate off         | Set `IRIS_DESTRUCTIVE_TOOLS_ENABLED=1` |
