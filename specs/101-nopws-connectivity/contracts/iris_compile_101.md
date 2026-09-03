# Contract: iris_compile Response Shape (101-nopws-connectivity)

## Summary of change (FR-016)

Every `iris_compile` response from the docker exec path gains the `execution_path` field,
matching the vocabulary introduced for `iris_execute`. The existing `method: "docker_exec"`
field is kept for backward compatibility — `execution_path` provides finer-grained routing
information distinguishing local from SSH-remote docker exec.

Additive change only — no existing fields removed or renamed (Constitution V compliant).

---

## Response shapes

### Atelier REST path (unchanged behavior, no new field required)

`iris_compile` via Atelier REST does not gain `execution_path` in this spec — the Atelier
path was not modified. `execution_path` is added only to the docker exec branch.

### Docker exec local path (new field)

```json
{
  "success": true,
  "compiled": ["MyPackage.MyClass"],
  "errors": [],
  "method": "docker_exec",
  "execution_path": "docker_exec_local"
}
```

### Docker exec SSH path (new field)

```json
{
  "success": true,
  "compiled": ["MyPackage.MyClass"],
  "errors": [],
  "method": "docker_exec",
  "execution_path": "docker_exec_ssh"
}
```

---

## Routing trigger

The docker exec branch in `iris_compile` fires when `docker_only || no_pws` is true,
mirroring the `iris_execute` early-branch pattern (plan.md §2.4). The `execution_path`
value is chosen by the same SSH-host check:

```
docker_only || no_pws?
  YES → ssh_host set? → execution_path = "docker_exec_ssh"
        else          → execution_path = "docker_exec_local"
```

---

## Backward compatibility

`method: "docker_exec"` is **kept** unchanged. Callers that already switch on `method`
see no change. The new `execution_path` field is additive.
