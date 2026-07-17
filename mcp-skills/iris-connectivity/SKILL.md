---
author: tdyar
benchmark_date: '2026-04-11'
benchmark_iris_version: '2025.1'
benchmark_tasks:
- prd-001
- prd-002
- prd-003
- prd-004
- prd-005
- prd-006
- prd-007
compatibility: python, java, objectscript, iris, sql
description: Use when connecting to IRIS from Python, Java, JDBC, ODBC, or any external
  language. IRIS connection APIs have specific package names, port numbers, and syntax
  that differ from every other database.
iris_version: '>=2022.1'
license: MIT
metadata:
  baseline_pass_rate: 1.0
  benchmark_note: Source inspection suite. Negative lift when loaded globally (-14%).
    Load on-demand for Python/JDBC/connection tasks only.
  lift: -0.143
  red_phase: Model uses wrong Python package, wrong JDBC prefix, wrong proc-call syntax
    in 100% of cases without this skill
  version: 1.0.0
name: tdyar/iris-connectivity
pass_rate: 0.857
state: reviewed
tags:
- iris
- python
- jdbc
- odbc
- connection
- dbapi
- native-api
---

# IRIS Connectivity — Hard Gate

**IRIS connection syntax is unique. Every other database pattern is wrong.**

## HARD GATE

- [ ] Python package: `intersystems-irispython` → **always `import iris`** (both DBAPI and Native API) — NOT `pyodbc`, `iris-python`, `intersystems_iris`
- [ ] Python procedures: `cursor.execute("SELECT MyProc(?)")` — NOT `cursor.callproc()` or `EXEC`
- [ ] JDBC URL: `jdbc:IRIS://host:1972/NAMESPACE` — NOT `jdbc:Cache://` (old Caché) or `jdbc:intersystems:iris://`
- [ ] JDBC driver class: `com.intersystems.jdbc.IRISDriver` — NOT `com.intersystems.jdbc.CacheDriver`
- [ ] Superserver port: **1972** (DBAPI/JDBC/Native API) vs web port **52773** (REST/Atelier/MCP) — these are different
- [ ] pgwire TEXT columns are streams in IRIS — `LENGTH()` fails with SQLCODE -37 on stream fields

---

## Python — DBAPI (SQL queries)

```python
# Install: pip install intersystems-irispython
import iris.dbapi as iris_dbapi

conn = iris_dbapi.connect(
    hostname="localhost",
    port=1972,           # superserver port — NOT 52773
    namespace="USER",
    username="_SYSTEM",
    password="SYS"
)
cur = conn.cursor()
cur.execute("SELECT Name FROM Sample.Person WHERE Age > ?", [30])
rows = cur.fetchall()

# Stored procedures — use SELECT not CALL or EXEC:
cur.execute("SELECT MyPackage.MyProc(?, ?)", [arg1, arg2])
result = cur.fetchone()[0]  # read BEFORE cur.close()!
```

## Python — Native API (globals, ClassMethods)

```python
import iris

conn = iris.connect(
    hostname="localhost",
    port=1972,
    namespace="USER",
    username="_SYSTEM",
    password="SYS"
)

# Call a ClassMethod:
result = conn.classMethodValue("MyPackage.MyClass", "MyMethod", arg1, arg2)

# Read/write globals:
gref = conn.createGlobal("MyGlobal")
gref.set("value", "subscript1", "subscript2")
```

## JDBC

```java
// Correct driver JAR: intersystems-jdbc-3.x.x.jar
// Maven: com.intersystems:intersystems-jdbc:3.8.4

String url = "jdbc:IRIS://localhost:1972/USER";  // uppercase IRIS, port 1972
Properties props = new Properties();
props.setProperty("user", "_SYSTEM");
props.setProperty("password", "SYS");

Connection conn = DriverManager.getConnection(url, props);
// Driver class: com.intersystems.jdbc.IRISDriver

// WRONG — legacy Caché driver (deprecated):
// "jdbc:Cache://localhost:1972/USER"
// "jdbc:intersystems:iris://localhost:1972/USER"
```

## Port Reference

| Port | Protocol | Use for |
|------|----------|---------|
| **1972** | SuperServer (TCP) | DBAPI, JDBC, ODBC, Native API |
| **52773** | HTTP/WebSocket | REST, Atelier, MCP, IRIS web apps |
| **1972** | pgwire | PostgreSQL wire protocol (if enabled) |

> **Docker**: `docker port <container> 1972/tcp` for DBAPI port, `docker port <container> 52773/tcp` for web port. These are different.

## pgwire Gotcha — TEXT columns are streams

```python
# pgwire (psycopg2) + IRIS: TEXT/LONGVARCHAR columns are %Stream objects
# WRONG — will return garbled binary or error:
cur.execute("SELECT description FROM MyTable")
desc = cur.fetchone()[0]   # might be stream object, not string
length = len(desc)          # SQLCODE -37: function not supported on stream

# CORRECT — cast to VARCHAR in the query:
cur.execute("SELECT CAST(description AS VARCHAR(4000)) FROM MyTable")
desc = cur.fetchone()[0]   # now a real Python string
```

## When to use which interface

| Task | Use |
|------|-----|
| SQL queries from Python | `iris.dbapi` |
| ObjectScript ClassMethods from Python | `iris` Native API |
| Java/JVM applications | JDBC (`jdbc:IRIS://`) |
| AI agent tools (Claude, GPT) | IRIS MCP (`%AI.MCP.Service`) |
| REST APIs | IRIS Web Gateway port 52773 |
| Legacy Caché apps (migrate) | Update from `jdbc:Cache://` → `jdbc:IRIS://` |

## iris-devtester CLI — Container Testing Workflow

When using `iris-devtester` (PyPI: `pip install iris-devtester`) for spinning up IRIS test containers:

```bash
# Start container — with exact port or auto-assign
idt container up --port 11972          # maps 1972→11972
idt container up --auto-port           # auto-assigns free port from 1972-2000

# Password change required? (common on fresh community containers)
idt test-connection --auto-fix         # detects "Unexpected error: 1", auto-remediates
idt container reset-password <name> --timeout 10  # manual with timeout

# Run ObjectScript from CLI
idt container exec <name> --objectscript "Write \$ZVERSION"
idt container exec <name> --namespace MYAPP --objectscript "Do MyMethod()"

# Check connectivity (shows host, port, namespace, masked password)
idt test-connection --container <name>
idt test-connection -v                 # verbose: shows full password
```

**Python API:**
```python
from iris_devtester import IRISContainer

with IRISContainer.community() as iris:
    conn = iris.get_connection()   # CallIn + password reset auto-handled
    cur = conn.cursor()
    cur.execute("SELECT $ZVERSION")
    print(cur.fetchone()[0])

# Specific port:
with IRISContainer.community().with_name("mydb") as iris:
    pass  # port auto-mapped, use iris.get_exposed_port(1972)

# Get credentials:
password = iris.get_password()   # public accessor (v1.15.0+)
username = iris.get_username()
```