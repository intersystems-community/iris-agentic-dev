---
name: irispython-connector
description: Connect Python to IRIS from outside IRIS over TCP — DB-API, SQLAlchemy, and pandas. Covers the port confusion, keyword-arg segfault, FETCH FIRST crash, and type normalization blockers that trip up every first-time user.
trigger: When Python code outside IRIS needs to query IRIS via DB-API or SQLAlchemy, or when a segfault / SIGSEGV / exit 139 occurs connecting Python to IRIS
---

## This skill vs. iris-embedded-python

|                 | This skill                                          | iris-embedded-python                                  |
| --------------- | --------------------------------------------------- | ----------------------------------------------------- |
| Python location | External process, connects over TCP                 | Inside IRIS (Embedded Python)                         |
| Import          | `intersystems_irispython.dbapi`                     | `import iris`                                         |
| Connection      | Superserver port (default 1972)                     | In-process, no network                                |
| Use when        | Running `.py` scripts, notebooks, external services | ObjectScript calls Python, or Python calls `iris.sql` |

If you see `import iris` in the code, use the `iris-embedded-python` skill instead.

---

## Connection — which port

**Always use the superserver port, not the web port.**

Run `check_config` to see both ports:

```text
iris-agentic-dev tool check_config --args '{}'
```

Look for `iris_port` (superserver, default 1972), not `web_port` (default 52773/52780).

Docker example: `-p 11975:1972` → superserver port on the host is **11975**.

### Install

```bash
pip install intersystems-irispython
```

### DB-API — positional args only

```python
import intersystems_irispython.dbapi as dbapi

# CORRECT — positional args
conn = dbapi.connect("localhost", 11975, "USER", "_SYSTEM", "SYS")

# WRONG — keyword args cause SIGSEGV on irispython >= 5.x outside an IRIS install
# conn = dbapi.connect(hostname="localhost", port=11975, ...)  # DO NOT USE
```

### SQLAlchemy

```python
from sqlalchemy import create_engine

engine = create_engine("iris://_SYSTEM:SYS@localhost:11975/USER")
```

---

## Crash-triggering patterns — avoid all three

| Pattern                                              | Symptom                 | Fix                                                                |
| ---------------------------------------------------- | ----------------------- | ------------------------------------------------------------------ |
| `dbapi.connect(hostname=..., port=...)` keyword args | SIGSEGV / exit 139      | Use positional args: `connect("host", port, "ns", "user", "pass")` |
| `SELECT ... FETCH FIRST n ROWS ONLY`                 | SIGSEGV in C extension  | Use `SELECT TOP n ...` instead                                     |
| `pd.read_sql(query, conn)`                           | SIGSEGV on some queries | Use `cursor.execute(); cursor.fetchall()`                          |

These are not edge cases — all three crash hard with no Python traceback. If you see exit 139 or SIGSEGV, check this table first.

---

## Type normalization

DB-API does not normalize aggregate return types. `COUNT`, `AVG`, `SUM` may return `Decimal`, `str`, or `float` depending on the query. Cast explicitly:

```python
cursor.execute("SELECT COUNT(*), AVG(Fare) FROM MyTable")
row = cursor.fetchone()
count = int(row[0]) if row[0] is not None else 0
avg_fare = float(row[1]) if row[1] is not None else 0.0
```

---

## IRISINSTALLDIR warning suppression

If you see a warning about `IRISINSTALLDIR` not being set, it is cosmetic. Suppress it before the import:

```python
import os
os.environ.setdefault("IRISINSTALLDIR", "")
import intersystems_irispython.dbapi as dbapi
```

---

## IRIS SQL dialect differences

| SQL feature        | IRIS behavior                                                            |
| ------------------ | ------------------------------------------------------------------------ |
| Row limit          | `SELECT TOP n ...` — `FETCH FIRST n ROWS ONLY` causes SIGSEGV            |
| Reserved words     | `COUNT`, `HOUR`, `YEAR`, `DATE`, `TIME` cannot be used as column aliases |
| Schema discovery   | `INFORMATION_SCHEMA` available and standard                              |
| Aggregates         | `COUNT`, `SUM`, `AVG`, `MIN`, `MAX` work                                 |
| Standard functions | `COALESCE`, `CASE`, `ROUND`, `NULLIF` all work                           |

---

## Quick-start (10 lines)

Minimal working example against the local dev container (`iris-dev-iris`, superserver port 11975):

```python
import os
os.environ.setdefault("IRISINSTALLDIR", "")
import intersystems_irispython.dbapi as dbapi

conn = dbapi.connect("localhost", 11975, "USER", "_SYSTEM", "SYS")
cursor = conn.cursor()
cursor.execute("SELECT TOP 5 ID, Name FROM %Dictionary.ClassDefinition")
for row in cursor.fetchall():
    print(row[0], row[1])
cursor.close()
conn.close()
```

Swap `11975` for whatever `iris_port` shows in `check_config` output for your environment.
