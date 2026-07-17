---
author: tdyar
benchmark_date: '2026-04-08'
benchmark_iris_version: '2025.1'
benchmark_tasks:
- sql-001
- sql-002
- sql-003
- sql-004
- sql-005
- sql-006
- sql-007
- sql-008
description: Use when writing, debugging, or optimizing SQL queries in IRIS — table
  naming, reserved words, NULL semantics, SQLCODE, IN clause limits, procedures, DDL
  quirks, and date handling. Load explicitly for SQL work; do NOT load globally for
  general ObjectScript repair tasks.
iris_version: '>=2024.1'
metadata:
  baseline_pass_rate: 0.625
  benchmark_note: Negative lift (-27%) on single-function repair benchmark (most tasks
    not SQL). Load explicitly for SQL tasks only.
  lift: 0.125
name: tdyar/iris-sql
pass_rate: 0.75
state: reviewed
tags:
- iris
- sql
- quirks
- ddl
- dbapi
trigger: Use for tdyar/iris-sql
---

# IRIS SQL — Quirks and Deviations from Standard SQL

## HARD GATE

Before writing any IRIS SQL, check these. Every one has caused production bugs.

- [ ] **Table name**: last dot = schema separator (`pkg.isc.Foo` → `pkg_isc.Foo`, NOT `pkg_isc_Foo`)
- [ ] **`SQLCODE = 0`** means success (falsy) — never use `If SQLCODE` as a success check
- [ ] **Column alias `avg`** is a reserved word — use `avgval`, `avgprice`, not `avg`
- [ ] **IN clause** — hard limit ~500 params; chunk at 499
- [ ] **`CALL` not supported** in iris.dbapi — use `SELECT procedure(args)`
- [ ] **NULL ≠ empty string** — `IS NULL` won't find `""` stored by ObjectScript

---

## 1. Table Naming (the #1 source of SQLCODE -30)

```sql
-- Two-level class → schema.table (dot preserved)
Catalog.Item         → SELECT FROM Catalog.Item       ✓
Healthcare.Patient   → SELECT FROM Healthcare.Patient  ✓

-- Three+ levels → underscores except last dot
pkg.isc.genai.Router → SELECT FROM pkg_isc_genai.Router  ✓

-- WRONG assumptions:
SELECT FROM Catalog_Item        -- ✗ table not found
SELECT FROM pkg.isc.genai.Router -- ✗ table not found
```

Verify for any class:
```objectscript
Set cls = ##class(%Dictionary.ClassDefinition).%OpenId("My.Package.Class")
Write cls.SqlSchemaName, ".", cls.SqlTableName
```

---

## 2. SQLCODE Semantics (backwards from intuition)

```objectscript
&sql(SELECT Name INTO :name FROM MyTable WHERE ID = :id)

// WRONG — SQLCODE=0 is success, which is falsy:
If SQLCODE { Write "error" }       // fires when row IS found!
If 'SQLCODE { Write "found" }      // WRONG boolean

// CORRECT:
If SQLCODE = 100 { Write "not found" }
If SQLCODE < 0   { Write "error: ", %msg }
// SQLCODE = 0  →  row found, name is set
```

---

## 3. Reserved Words — Complete Reference

### Full IRIS SQL reserved word list
Check any word: `$SYSTEM.SQL.IsReservedWord("word")` → 1 = reserved.

```
ABSOLUTE ADD ALL ALTER AND ANY AS ASC AT AUTHORIZATION AVG BEGIN BETWEEN BY
CASCADE CASE CAST CHAR CHARACTER CHECK CLOSE COALESCE COLLATE COMMIT CONNECT
CONSTRAINT CONTINUE CONVERT COUNT CREATE CROSS CURRENT CURSOR DATE DEALLOCATE
DECIMAL DECLARE DEFAULT DELETE DESC DISTINCT DOMAIN DOUBLE DROP ELSE END
ESCAPE EXCEPT EXEC EXISTS EXTRACT FALSE FETCH FIRST FLOAT FOR FOREIGN FROM
FULL GLOBAL GOTO GRANT GROUP HAVING HOUR IDENTITY IMMEDIATE IN INNER INPUT
INSERT INT INTEGER INTERSECT INTO IS JOIN LANGUAGE LAST LEFT LEVEL LIKE
LOCAL LOWER MATCH MAX MIN MINUTE MODULE NAMES NATIONAL NATURAL NEXT NO NOT
NULL NULLIF NUMERIC OF ON ONLY OPEN OPTION OR ORDER OUTER OUTPUT PAD PARTIAL
PREPARE PRIMARY PRIOR PROCEDURE PUBLIC READ REAL REFERENCES RELATIVE RESTRICT
REVOKE RIGHT ROLE ROLLBACK ROWS SCHEMA SCROLL SECOND SELECT SESSION_USER SET
SHARD SOME SPACE SQLERROR SQLSTATE STATISTICS SUBSTRING SUM SYSDATE TABLE
TEMPORARY THEN TIME TO TOP TRANSACTION TRIM TRUE UNION UNIQUE UPDATE UPPER
USER USING VALUES VARCHAR WHEN WHERE WITH WORK WRITE
```

**IRIS-specific reserved words** (% prefix — never use as identifiers):
```
%ID %ROWCOUNT %TABLENAME %CLASSNAME %STARTSWITH %INLIST %MATCHES %EXACT
%DLIST %UPPER %SQLUPPER %SQLSTRING %VALUE %KEY %VID %PARALLEL %NOLOCK
```

### Most dangerous as table/class names

```objectscript
// WRONG — class short name is reserved:
Class SQL.User Extends %Persistent      // SQL.USER → reserved → can't SELECT
Class App.Order Extends %Persistent     // App.ORDER → reserved
Class My.Group Extends %Persistent      // My.GROUP → reserved

// FIX option 1: rename the class
Class SQL.AppUser Extends %Persistent   // safe

// FIX option 2: SqlTableName keyword
Class SQL.User Extends %Persistent [ SqlTableName = AppUser ] {}
Class App.Order Extends %Persistent [ SqlTableName = SalesOrder ] {}
```

### Most dangerous as property/column names

`NAME, TYPE, VALUE, TEXT, KEY, DATE, TIME, LEVEL, STATUS, STATE, CODE, DATA, LIST, SET`

```objectscript
// WRONG:
Property Name As %String;      // SELECT Name — may error
Property Type As %String;      // "type" is reserved

// FIX option 1: SqlFieldName keyword (SQL field name can differ from property name)
Property Name As %String [ SqlFieldName = FullName ];
Property Type As %String [ SqlFieldName = ItemType ];

// FIX option 2: double-quote in SQL
// SELECT "Name", "Type" FROM MyTable   ← double-quotes escape reserved words
```

### Aggregates as aliases: avg, count, sum, min, max

```sql
-- WRONG: reserved word as alias
SELECT AVG(Price) avg, COUNT(*) count FROM Products

-- CORRECT: non-reserved aliases
SELECT AVG(Price) avgprice, COUNT(*) totalcount FROM Products
```

### No Underscores in ObjectScript Member Names (IRIS 2025.1+)

```objectscript
// WRONG — causes #5559 parse error:
Property first_name As %String;
ClassMethod get_value() As %String {}
Parameter MAX_RETRY = 3;   // reads as MAX concatenated with _RETRY string

// CORRECT — camelCase:
Property FirstName As %String;
ClassMethod GetValue() As %String {}
Parameter MaxRetry = 3;

// Exception: SqlFieldName CAN use underscores (it's an SQL identifier, not ObjectScript):
Property FirstName As %String [ SqlFieldName = first_name ];   // OK
```

Check actual projected column names:
```objectscript
Set rs = ##class(%SQL.Statement).%ExecDirect(,"SELECT * FROM MyTable WHERE 1=0")
Set meta = rs.%GetMetadata()
For i=1:1:meta.columns.Count() { Write i, ": ", meta.columns.GetAt(i).colName, ! }
```

---

## 4. NULL vs Empty String

IRIS stores ObjectScript `""` as a non-NULL empty string. `IS NULL` won't find it:

```sql
-- ObjectScript Set obj.Field = "" → stored as "" (empty string), not NULL

-- WRONG: misses empty strings stored by ObjectScript
WHERE Field IS NULL

-- CORRECT for "absent or empty":
WHERE Field IS NULL OR Field = ''

-- CORRECT for "has a value":
WHERE Field IS NOT NULL AND Field <> ''
```

---

## 5. IN Clause Limit (~500 params)

IRIS SQL has a hard parameter limit around 500-999 per IN clause. Silent failure (0 rows) above the limit:

```objectscript
// WRONG for large id lists:
Set sql = "SELECT ID FROM Table WHERE ID IN ("
For i=1:1:ids.Count() { Set sql = sql _ "?," }
Set sql = $Extract(sql, 1, *-1) _ ")"
// If ids.Count() > ~500 → 0 rows, no error

// CORRECT: chunk at 499
Set chunkSize = 499
Set start = 1
While start <= ids.Count() {
    Set end = $Select((start + chunkSize - 1) <= ids.Count(): start + chunkSize - 1, 1: ids.Count())
    Set placeholders = ""
    For i=start:1:end {
        Set placeholders = placeholders _ $Select(i=start:"", 1:",") _ "?"
    }
    Set sql = "SELECT ID FROM Table WHERE ID IN (" _ placeholders _ ")"
    // ... execute with ids start..end
    Set start = start + chunkSize
}
```

For Python iris.dbapi, same limit applies:
```python
CHUNK = 499
for i in range(0, len(ids), CHUNK):
    chunk = ids[i:i+CHUNK]
    placeholders = ",".join(["?"] * len(chunk))
    cur.execute(f"SELECT id FROM MyTable WHERE id IN ({placeholders})", chunk)
```

---

## 6. Calling Procedures from Python iris.dbapi

`CALL` is valid ODBC/JDBC but NOT in the Python iris.dbapi driver:

```python
# WRONG — raises SQLCODE -51 "SQL statement expected, IDENTIFIER found":
cur.execute("CALL pkg.MyProc(?, ?)", [arg1, arg2])

# CORRECT:
cur.execute("SELECT pkg.MyProc(?, ?)", [arg1, arg2])
row = cur.fetchone()
result = row[0]   # read BEFORE cur.close()!
```

Also: **read row values before closing the cursor** — they become inaccessible after `cur.close()`.

---

## 7. DDL: What IRIS Doesn't Support

```sql
-- ✗ NOT SUPPORTED:
id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY  -- no IDENTITY
id SERIAL                                            -- no SERIAL
col VARCHAR DEFAULT NEWID()                          -- no NEWID()
WITH cte AS (SELECT ?) SELECT FROM cte               -- no ? params in CTEs

-- ✓ CORRECT alternatives:
id VARCHAR(72) PRIMARY KEY   -- use UUID from application code
-- Generate in Python: str(uuid.uuid4())
-- Generate in ObjectScript: $System.Util.CreateGUID()

-- ✓ CTEs work but parameters must be literals, not ?:
WITH ranked AS (SELECT Name, Age FROM Sample.Person WHERE Age > 30)
SELECT * FROM ranked
```

---

## 8. %INLIST / FOR SOME — Searching %List Columns

`LIKE '%value%'` on a `%List` column returns nothing. Use these instead:

```sql
-- Exact element match in %List column:
SELECT Title FROM Items WHERE ? %INLIST Tags

-- Any element matches condition:
SELECT Title FROM Items WHERE FOR SOME %ELEMENT(Tags) (%VALUE = ?)

-- Contains substring in any element:
SELECT Title FROM Items WHERE FOR SOME %ELEMENT(Tags) (%VALUE LIKE ?)
```

---

## 9. Date and Time Arithmetic

IRIS dates are stored as integer offsets from Dec 31, 1840 (`$HOROLOG` format):

```sql
-- Today's date as HOROLOG integer:
SELECT CURRENT_DATE          -- returns YYYY-MM-DD string
-- In ObjectScript: +$HOROLOG = today as integer

-- Date arithmetic:
SELECT DATEADD('day', 30, GETDATE())       -- 30 days from now
SELECT DATEDIFF('day', StartDate, GETDATE()) -- days elapsed

-- Convert between formats:
-- ObjectScript: $ZDATE(horolog_int, 3) = "YYYY-MM-DD"
-- ObjectScript: $ZDATEH("2026-01-15", 3) = horolog integer

-- Stored as HOROLOG integer, filter by integer comparison:
SELECT Name FROM Orders WHERE OrderDate >= ?   -- pass +$HOROLOG value
```

---

## 10. Stream Columns in SQL Results

Stream properties (`%Stream.GlobalCharacter`, `%CacheStream`) behave differently in SQL:

```objectscript
// WRONG: stream OREF from SQL result set may be invalid:
Set rs = stmt.%Execute()
While rs.%Next() {
    Set stream = rs.description   // may be invalid OREF
    Write stream.Read()           // <INVALID OREF>
}

// CORRECT: use %ID to open the object directly:
// SELECT %ID AS rowId, Name FROM MyTable WHERE ...
While rs.%Next() {
    Set obj = ##class(MyTable).%OpenId(rs.rowId)
    If '$IsObject(obj) { Continue }
    Set stream = obj.description
    Do stream.Rewind()
    Write stream.Read(32000)
}
```

SQL function restrictions on stream columns:
```sql
-- ✗ SQLCODE -37: SQL scalar/aggregate not supported for stream fields
SELECT LENGTH(description) FROM Articles
SELECT UPPER(body) FROM Posts

-- ✓ Open object and use ObjectScript stream methods instead
```

---

## EXAMPLE: FOR SOME %ELEMENT — Both %INLIST and LIKE break on %List columns

Neither `LIKE` nor `? %INLIST col` reliably searches `%List` columns in all contexts.
**Always use `FOR SOME %ELEMENT` for both exact and substring matching.**

```sql
-- WRONG: LIKE does substring match on binary encoding of %List — returns wrong/no results
WHERE Tags LIKE '%iris%'

-- CORRECT with ELEMENTS index (most reliable):
WHERE FOR SOME %ELEMENT(Tags) (%VALUE = ?)    -- exact match
WHERE FOR SOME %ELEMENT(Tags) (%VALUE LIKE ?) -- substring match

-- CORRECT without ELEMENTS index (when you know the value is an exact list element):
WHERE ? %INLIST Tags

-- Check if ELEMENTS index exists first:
-- Index TagsIdx On Tags(ELEMENTS);   ← required for FOR SOME %ELEMENT
-- Without this index, FOR SOME %ELEMENT returns SQLCODE -400 (unsupported)
```

**Rule**: If a `%List` column has `Index X On Col(ELEMENTS)`, use `FOR SOME %ELEMENT`.
If no ELEMENTS index, use `? %INLIST Col` for exact membership testing.
`LIKE` on a `%List` column never works correctly — never use it.

**CSV string columns** (tags stored as "iris,sql,python"): use `$LISTFROMSTRING`:
```sql
-- Tags is VARCHAR: "iris,sql,objectscript"
-- WRONG: LIKE '%iris%' matches 'iris2' via substring
WHERE Tags LIKE '%iris%'

-- CORRECT: exact element membership via $LISTFROMSTRING
WHERE ? %INLIST $LISTFROMSTRING(Tags, ',')
-- Pass the bare tag value (no % wildcards): stmt.%Execute("iris")
```

---

## EXAMPLE: IN clause chunking — %Execute does NOT accept arrays

When chunking IN clause queries, you CANNOT pass an array to `%Execute`.
Pass individual positional arguments using a dynamically built call:

```objectscript
// WRONG: %Execute does not accept local arrays
Set args = chunkCount
For i=1:1:chunkCount { Set args(i) = ids.GetAt(start + i - 1) }
Set rs = stmt.%Execute(args...)   // <STACK> error!

// CORRECT: use %ExecDirect with positional params OR build the call dynamically
// For small fixed chunks, pass args directly:
Set stmt = ##class(%SQL.Statement).%New()

// Option A: %ExecDirect with string SQL (simpler for chunked IN):
Set chunkSize = 499
Set start = 1
While start <= ids.Count() {
    Set end = $Select((start+chunkSize-1)<=ids.Count(): start+chunkSize-1, 1: ids.Count())
    Set placeholders = ""
    Set execArgs = ""
    For i=start:1:end {
        Set placeholders = placeholders _ $Select(i=start:"", 1:",") _ "?"
        // Build comma-separated list for positional args — use %ExecDirect with array
    }
    // Use %ExecDirect which accepts a variable-length arg list
    Set rs = ##class(%SQL.Statement).%ExecDirect(
        , "SELECT ID FROM Table WHERE ID IN (" _ placeholders _ ")"
    )
    // BUT %ExecDirect also doesn't take array! Solution: use dynamic method call
    // BEST APPROACH for variable params: build quoted IN list directly
    Set idList = ""
    For i=start:1:end {
        Set idList = idList _ $Select(i=start:"", 1:",") _ "'" _ ids.GetAt(i) _ "'"
    }
    Set rs = ##class(%SQL.Statement).%ExecDirect(
        , "SELECT RefId, Status FROM SQL.Record WHERE RefId IN (" _ idList _ ")"
    )
    While rs.%Next() {
        Do results.Insert(rs.%Get("RefId") _ ":" _ rs.%Get("Status"))
    }
    Set start = end + 1
}
```

**Key rule**: For variable-count IN clause params, build the quoted values directly into the SQL string. Don't try to pass arrays to `%Execute` or `%ExecDirect` — they take positional args only, not arrays.

---

## %ROWCOUNT and %ROWID After DML

After `&sql(UPDATE/INSERT/DELETE ...)`, two implicit variables are set:

```objectscript
&sql(UPDATE SQL.User SET Active = 0 WHERE UserID = :userId)
If SQLCODE < 0 { Return $$$ERROR($$$GeneralError, "SQL error") }

// %ROWCOUNT = number of rows affected (0 = no match)
If %ROWCOUNT = 0 {
    Return $$$ERROR($$$GeneralError, "User not found: " _ userId)
}
Return $$$OK

// BulkDeactivate: return count of affected rows
&sql(UPDATE SQL.User SET Active = 0 WHERE Department = :dept AND Active = 1)
If SQLCODE < 0 { Return -1 }
Return %ROWCOUNT    // NOT SQLCODE (which is always 0 on success)
```

**Never return `SQLCODE` as a row count** — `SQLCODE = 0` means success, not "0 rows affected". Use `%ROWCOUNT` for affected row counts.

---

## %MATCHES — IRIS SQL Regex Operator

`%MATCHES` supports character classes and quantifiers. `LIKE` treats `[A-Z]` as a literal string.

```sql
-- WRONG: LIKE treats [A-Z] as literal text, not a character class
WHERE Code LIKE '[A-Z][A-Z][A-Z]-[0-9][0-9][0-9]'   -- matches nothing!

-- CORRECT: %MATCHES supports character class ranges
WHERE Code %MATCHES '[A-Z][A-Z][A-Z]-[0-9][0-9][0-9]'  -- exactly ABC-123 format
WHERE Name %MATCHES 'pkg\\.isc\\..*'               -- dots must be escaped: \\.
WHERE Status %MATCHES '(OPEN|CLOSED|PENDING)'          -- alternation
```

**%MATCHES is full-string anchored** — the pattern must match the entire string, not just a substring. `'[A-Z].*'` matches strings that start with a letter (the `.*` covers the rest).

**Common patterns:**
```sql
-- Package prefix (dots escaped):
WHERE Name %MATCHES 'pkg\\.isc\\..*'

-- Exactly N digits:
WHERE Code %MATCHES '[A-Z][A-Z][A-Z]-[0-9][0-9][0-9]'

-- Version suffix (3 digits):
WHERE Name %MATCHES '.*[0-9][0-9][0-9]'

-- %STARTSWITH equivalent (but %STARTSWITH is faster):
WHERE Name %MATCHES 'pkg\\.isc\\..*'   -- use %STARTSWITH 'pkg.isc.' instead
```

---

## INSERT Syntax — No SQLite/MySQL Shortcuts

IRIS SQL uses standard SQL INSERT. Several shorthand forms from other databases do NOT work:

```python
# WRONG — IRIS does not support INSERT OR IGNORE (SQLite syntax):
cur.execute("INSERT OR IGNORE INTO Graph_KG.docs (id, text) VALUES (?, ?)", (doc_id, text))
# Error: SQLCODE -1 "UPDATE expected, IDENTIFIER (IGNORE) found"

# WRONG — IRIS does not support INSERT IGNORE (MySQL syntax):
cur.execute("INSERT IGNORE INTO MyTable (col) VALUES (?)", (val,))

# WRONG — IRIS does not support ON CONFLICT (SQLite/PostgreSQL syntax):
cur.execute("INSERT INTO MyTable (id, val) VALUES (?,?) ON CONFLICT(id) DO NOTHING", ...)

# CORRECT — Standard INSERT (will raise on duplicate PK):
cur.execute("INSERT INTO Graph_KG.docs (id, text) VALUES (?, ?)", (doc_id, text))
```

### Handling duplicates in IRIS

**Option 1: Pre-filter with a Python set (best for bulk loads)**
```python
# Load existing IDs first, then skip before inserting
cur.execute("SELECT id FROM MyTable")
existing = set(r[0] for r in cur.fetchall())

for row in data:
    if row["id"] in existing:
        continue  # skip duplicate
    cur.execute("INSERT INTO MyTable (id, val) VALUES (?, ?)", (row["id"], row["val"]))
    existing.add(row["id"])
```

**Option 2: Catch the duplicate key error**
```python
for row in data:
    try:
        cur.execute("INSERT INTO MyTable (id, val) VALUES (?, ?)", (row["id"], row["val"]))
    except Exception as e:
        if "SQLCODE: <-119>" in str(e):  # -119 = unique constraint violation
            continue  # duplicate, skip
        raise  # re-raise unexpected errors
```

**Option 3: DELETE + INSERT (upsert)**
```python
# For true upsert behavior — delete first, then insert
cur.execute("DELETE FROM MyTable WHERE id = ?", (row["id"],))
cur.execute("INSERT INTO MyTable (id, val) VALUES (?, ?)", (row["id"], row["val"]))
```

**Option 4: ObjectScript embedded SQL UPSERT**
```objectscript
// IRIS ObjectScript supports %Save() which handles insert/update automatically
Set obj = ##class(MyTable).%OpenId(id)
If '$IsObject(obj) { Set obj = ##class(MyTable).%New() }
Set obj.id = id
Set obj.val = val
Do obj.%Save()
```

### Key IRIS INSERT constraints
- No `INSERT OR IGNORE` (SQLite)
- No `INSERT IGNORE` (MySQL)
- No `ON CONFLICT ... DO NOTHING/UPDATE` (PostgreSQL/SQLite)
- No `MERGE` statement (standard SQL, not supported in IRIS as of 2026.2)
- Duplicate primary key → `SQLCODE -119` ("Unique constraint violation")
- `iris.dbapi` `executemany()` is supported but each row is still individual — no batch optimization

---

## %STARTSWITH vs LIKE for Prefix Search

`%STARTSWITH` treats the search term literally. `LIKE 'prefix%'` uses `_` as a wildcard.

```sql
-- WRONG: if prefix contains _ (SQL wildcard), LIKE misbehaves
WHERE Name LIKE '_mith%'    -- _ matches ANY character, finds Smith, Xmith, etc.

-- CORRECT: %STARTSWITH is literal, no special characters
WHERE Name %STARTSWITH ?    -- pass "Smith", matches only names starting with "Smith"
WHERE Name %STARTSWITH '_mith'  -- matches only names literally starting with "_mith"
```

For package/class prefix search, always use `%STARTSWITH`:
```sql
WHERE Name %STARTSWITH 'pkg.isc.'    -- faster than LIKE, no escaping needed
WHERE Name %STARTSWITH ?             -- safe with any user input
```