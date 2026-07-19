---
name: iris-pgwire
description: Use when connecting to IRIS via the PostgreSQL wire protocol, using psycopg3 or any Postgres-compatible client
managed_by: "iris-agentic-dev"
---

# iris-pgwire

**Project**: [iris-pgwire](https://github.com/intersystems-community/iris-pgwire)
**Status**: community preview
**Requires**: IRIS 2024.1+; `iris-pgwire` package installed via ZPM

## What it does

`iris-pgwire` exposes IRIS SQL over the PostgreSQL wire protocol. Any
`psycopg3`, `psycopg2`, `asyncpg`, or JDBC/Postgres-compatible client can
connect to IRIS without the `intersystems-irispython` package.

Useful when:

- You're deploying to an environment where the IRIS Python gateway isn't
  available (AWS Lambda, Docker minimal images, etc.)
- You want to use standard Postgres tooling (`psql`, `pgAdmin`, `dbt`)
- Your library requires a Postgres connection and you can't swap drivers

## When to use this vs the alternatives

| Scenario                               | Preferred driver                          |
| -------------------------------------- | ----------------------------------------- |
| Python inside IRIS (Embedded Python)   | `iris` native module                      |
| Python outside IRIS, full IRIS API     | `intersystems-irispython`                 |
| Python outside IRIS, SQL only, no deps | **iris-pgwire + psycopg3**                |
| Java / Spring                          | JDBC (`com.intersystems.jdbc.IRISDriver`) |
| Standard Postgres tooling              | **iris-pgwire**                           |

See the [iris-connectivity skill](../iris-connectivity/SKILL.md) for the full
connection-options table.

## Install

```bash
# On the IRIS instance
zpm "install iris-pgwire"
```

The package starts a listener on port 5432 (configurable via CPF). No IRIS
restart required.

## Connection string

```text
postgresql://username:password@host:5432/NAMESPACE
```

The database name in the connection string maps to the IRIS namespace
(case-insensitive, uppercased internally).

```python
import psycopg

conn = psycopg.connect(
    "postgresql://_SYSTEM:SYS@localhost:5432/USER"
)
```

Or with keyword arguments:

```python
conn = psycopg.connect(
    host="localhost",
    port=5432,
    dbname="USER",        # IRIS namespace
    user="_SYSTEM",
    password="SYS",
)
```

## psycopg3 usage pattern

```python
import psycopg

with psycopg.connect("postgresql://_SYSTEM:SYS@localhost:5432/USER") as conn:
    with conn.cursor() as cur:
        cur.execute("SELECT Name, DOB FROM Sample.Person WHERE Age > %s", (30,))
        rows = cur.fetchall()
        for name, dob in rows:
            print(name, dob)
```

Async variant:

```python
import psycopg

async with await psycopg.AsyncConnection.connect(
    "postgresql://_SYSTEM:SYS@localhost:5432/USER"
) as conn:
    async with conn.cursor() as cur:
        await cur.execute("SELECT Name FROM Sample.Person")
        rows = await cur.fetchall()
```

## Key gotchas

### Parameter style: `%s` only

iris-pgwire speaks the PostgreSQL extended query protocol. Use `%s`
placeholders, not `?` (ODBC style) or `:name` (named style).

```python
# Correct
cur.execute("SELECT * FROM Foo WHERE ID = %s", (42,))

# Wrong — do not use
cur.execute("SELECT * FROM Foo WHERE ID = ?", (42,))
```

### Type mapping

| IRIS type     | Python type returned                  |
| ------------- | ------------------------------------- |
| `%String`     | `str`                                 |
| `%Integer`    | `int`                                 |
| `%Double`     | `float`                               |
| `%Date`       | `datetime.date`                       |
| `%TimeStamp`  | `datetime.datetime`                   |
| `%Boolean`    | `bool`                                |
| `%List`       | `str` (encoded form)                  |
| Stream / clob | `str` (truncated at 64 KB by default) |

IRIS `%List` returns the internal `$LISTBUILD` encoding, not a Python list.
Use `iris_list_to_python()` helper from `intersystems-irispython` if you need
decoded list values, or store lists as JSON strings.

### Transaction semantics

iris-pgwire exposes IRIS SQL transactions via the standard Postgres
autocommit/explicit-transaction model:

- **autocommit off** (psycopg3 default): each statement runs inside a
  transaction; call `conn.commit()` or `conn.rollback()` explicitly.
- **autocommit on**: each statement commits immediately (matches IRIS default
  SQL behavior).

IRIS does not support `SAVEPOINT` — nested transactions will error. Use flat
transactions only.

```python
conn.autocommit = False
try:
    cur.execute("INSERT INTO MyTable (Name) VALUES (%s)", ("Alice",))
    conn.commit()
except Exception:
    conn.rollback()
    raise
```

### Namespace isolation

Each psycopg3 connection is bound to one namespace (the `dbname` in the
connection string). To query multiple namespaces, open separate connections.
`SET SCHEMA` is not supported.

### DDL support

DDL (`CREATE TABLE`, `ALTER TABLE`, `DROP`) is available if the connected user
has `%DB_<namespace>:Write` privilege. DDL runs outside IRIS class compilation
— changes are visible immediately in SQL but not as compiled classes.

## Reference

- [iris-connectivity skill](../iris-connectivity/SKILL.md) — full connection
  options table (JDBC, ODBC, irispython DBAPI, native API)
- [iris-pgwire repo](https://github.com/intersystems-community/iris-pgwire)
- [psycopg3 docs](https://www.psycopg.org/psycopg3/docs/)
