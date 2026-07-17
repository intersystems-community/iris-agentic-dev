---
name: iris-devtester
description: >
  Use when writing tests that need a live IRIS container, using IRISContainer
  factory methods, debugging connection failures, or understanding the password
  reset story. Covers iris-devtester 1.18.0+ patterns: CPF-first password
  strategy, factory methods, gotchas, and the full connection flow.
tags: [iris, devtester, testing, docker, python, password, connection]
---

# iris-devtester — Container Testing Toolkit

**PyPI**: `pip install iris-devtester>=1.18.0`
**Repo**: `github.com/intersystems-community/iris-devtester`

---

## HARD GATE — Common Mistakes

- **DO NOT** call `unexpire_all_passwords()` proactively in `get_connection()` — 1.18.0 is CPF-first; docker exec only fires as fallback
- **DO NOT** connect as `localhost` on macOS — use `127.0.0.1` (IPv6 resolution bug)
- **DO NOT** use `IRISContainer(); iris.start()` — always `with IRISContainer.community() as iris:`
- **DO NOT** hardcode ports — use `iris.get_mapped_port(1972)`
- **DO NOT** bind-mount host directories on Linux without fixing uid 51773 — see `iris-linux-docker` skill

---

## Factory Methods

```python
from iris_devtester import IRISContainer

# Community — default, auto-detects ARM64 vs x86
with IRISContainer.community() as iris:
    conn = iris.get_connection()

# Light — CI/CD, 580MB, no web server
with IRISContainer.light() as iris: ...

# Enterprise — needs IRIS_LICENSE_KEY env var or license_key= kwarg
with IRISContainer.enterprise(license_key="~/iris.key") as iris: ...

# Health — irishealth-community + FHIR R4 pre-installed (see irishealth-container skill)
with IRISContainer.health() as iris:
    h = iris.fhir_health_check()

# AI Hub — %AI.Agent, VECTOR SQL, ISC internal registry (see irishealth-container skill)
with IRISContainer.ai_hub(build="159") as iris: ...

# Attach — reconnect to existing container (e.g. started by idt container up)
iris = IRISContainer.attach("container-name")
conn = iris.get_connection()
```

---

## Password Strategy (1.18.0+) — CPF-First

**How it works:**

1. `start()` injects `CPFPreset.SECURE_DEFAULTS` via `ISC_CPF_MERGE_FILE` — sets `ChangePassword=0` for `_SYSTEM` and `SuperUser` **before IRIS opens connections**. No `docker exec` needed.
2. `get_connection()` tries `iris.connect()` directly (optimistic).
3. If that fails with a password-change error AND `_password_handled` is False → calls `unexpire_all_passwords()` once (docker exec fallback) → retries.
4. `_password_handled = True` prevents double-remediation.

**What this means:**
- Happy path: zero `docker exec` calls. Works in restricted CI environments.
- Fallback path: one `docker exec` on `attach()` containers or existing containers.

**Manual escape hatches:**
```bash
idt test-connection --auto-fix          # detect + reset + retry
idt container reset-password <name> --timeout 30
```

```python
from iris_devtester.utils import reset_password_if_needed
reset_password_if_needed(e, container_name="iris_db", username="_SYSTEM")
```

---

## Connection Flow

```
IRISContainer.community().__enter__()
  → start()
    → with_cpf_merge(SECURE_DEFAULTS)  # ChangePassword=0 via ISC_CPF_MERGE_FILE
    → super().start()                  # IRIS reads CPF, opens port 1972
    → _password_handled = True

iris.get_connection()
  → enable_callin_service()            # docker exec once
  → get_connection(config)             # iris.connect() — optimistic
  → on ChangePassword error:
      unexpire_all_passwords()         # fallback, once
      retry iris.connect()
```

---

## Diagnostic Tools (1.16.0+)

```python
from iris_devtester import probe_connection, ContainerHealth, ConnectionProbe

# Inspect connection state (schemas, IRIS version, latency)
result = probe_connection(conn)
print(result.report())

# Health check from container (includes schema visibility)
health = iris.health_check()
print(health.tables_visible)   # False if schema not seeded
print(health.report())
```

`ConnectionDiagnosticError` wraps SQLCODE -30 (table not found) and -23 (CTE scoping) with schema visibility context and suggested fix. Raised automatically — no call-site changes needed.

---

## Public Exports

```python
from iris_devtester import (
    IRISContainer,
    get_connection,
    probe_connection,
    ContainerHealth,
    ConnectionProbe,
    ConnectionDiagnosticError,
    IRISConfig,
)
```

---

## pytest Fixtures

```python
def test_example(iris_db):           # function-scoped, fresh container
def test_example(iris_db_shared):    # module-scoped, shared container
def test_example(iris_container):    # raw IRISContainer access
```

---

## CLI (idt)

```bash
idt container up --port 11972          # exact port mapping
idt container up --auto-port           # auto-assign from 1972-2000
idt container exec iris_db --objectscript "Write $ZVERSION"
idt container reset-password iris_db --timeout 30
idt test-connection --container iris_db --auto-fix -v
idt fixture load --name my-data
```

---

## Gotchas

**Ryuk kills containers on process exit** — use `idt container up` + `IRISContainer.attach()` for persistent containers. See `containers/AGENTS.md`.

**No web server on enterprise irishealth** — `irishealth:2026.2.0AI.*` and `iris:XXXX` (enterprise) have `WebServer=0`. Port 1972 only. A webgateway CANNOT substitute — it proxies CSP protocol to the superserver, not Atelier REST. Use `irishealth-community` or `iris-community` for VSCode/Atelier access. See `irishealth-container` skill.

**`%AI.*` classes in read-only irislib** — cannot transplant to community image. Two-container split is correct.

**DinD port mapping** — `get_mapped_port(52773)` raises `ConnectionError` inside CI containers. Fixed in 1.15.1: falls back to internal port. Set `TC_HOST=<ip>` to override.
