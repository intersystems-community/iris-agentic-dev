# Spec 066 — irispython-connector skill: external Python DB-API/SQLAlchemy guide

## Summary

New skill `skills/skills/irispython-connector/SKILL.md` covering how to connect
Python to IRIS from _outside_ IRIS (DB-API, SQLAlchemy) — distinct from
`iris-embedded-python` (in-process). Addresses the highest-friction first-mile
blockers found in the CDP Discovery Project replication (July 2026).

## Motivation

CDP Python challenge and independent agent replication both surfaced the same
cluster of blockers that docs.intersystems.com hides behind a 403 wall:

1. Port confusion — developers try the web port (52780); the superserver port
   (default 1972) is required and not mentioned in the PyPI README.
2. Keyword-arg segfault — `dbapi.connect(hostname=..., port=...)` crashes
   irispython ≥5.x outside an IRIS install; positional args work.
3. `FETCH FIRST n ROWS ONLY` causes SIGSEGV; must use `SELECT TOP n`.
4. `pd.read_sql()` crashes with IRIS dialect; raw cursor iteration is stable.
5. Aggregate return types are inconsistent (Decimal / str / float); must cast.

These are not edge cases — every agent without IAD hit at least two of them.
No existing skill covers external Python connectivity.

## Spec

### Skill file

`skills/skills/irispython-connector/SKILL.md`

### Required sections

**When to use this skill vs. iris-embedded-python**

- This skill: Python process external to IRIS, connecting over TCP
- iris-embedded-python: Python running inside IRIS via Embedded Python

**Connection — which port**

```
iris-agentic-dev tool check_config --args '{}'
```

Look for `iris_port` (superserver, default 1972) not `web_port`. Docker mapping
example: `-p 11975:1972` means superserver port is 11975.

DB-API (positional args only — keyword args segfault):

```python
import intersystems_irispython.dbapi as dbapi
conn = dbapi.connect("localhost", 11975, "USER", "_SYSTEM", "SYS")
```

SQLAlchemy:

```python
from sqlalchemy import create_engine
engine = create_engine("iris://_SYSTEM:SYS@localhost:11975/USER")
```

**Crash-triggering patterns — avoid**

| Pattern                                              | Symptom                 | Fix                                       |
| ---------------------------------------------------- | ----------------------- | ----------------------------------------- |
| `dbapi.connect(hostname=..., port=...)` keyword args | SIGSEGV / exit 139      | Use positional args                       |
| `SELECT ... FETCH FIRST n ROWS ONLY`                 | SIGSEGV in C extension  | Use `SELECT TOP n ...`                    |
| `pd.read_sql(query, conn)`                           | SIGSEGV on some queries | Use `cursor.execute(); cursor.fetchall()` |

**Type normalization**

DB-API does not normalize aggregate return types. Cast explicitly:

```python
avg_fare = float(row[0]) if row[0] is not None else 0.0
```

**IRISINSTALLDIR warning**

Cosmetic; suppress with `os.environ.setdefault("IRISINSTALLDIR", "")` before import.

**IRIS SQL dialect differences**

- `COUNT`, `HOUR`, `YEAR`, `DATE`, `TIME` are reserved words — cannot use as aliases
- No `FETCH FIRST` — use `TOP n` in SELECT
- Standard aggregates, COALESCE, CASE, ROUND all work as expected
- `INFORMATION_SCHEMA` available for discovery

**Quick-start (10 lines)**

Minimal working example from connect to first query result.

### Registration

Add `irispython-connector` to `skills.sh.json` under "Core Skills" alongside
`iris-embedded-python`. `sourceType: local`.

## Out of scope

- Embedded Python (covered by iris-embedded-python skill)
- SQLAlchemy ORM / model definitions
- Connection pooling

## Acceptance criteria

- [ ] Skill file exists at correct path
- [ ] All five crash-triggering patterns documented with fix
- [ ] Port disambiguation section references `check_config`
- [ ] Quick-start example is runnable against `iris-dev-iris` container
- [ ] Registered in skills.sh.json
- [ ] markdownlint + prettier clean
