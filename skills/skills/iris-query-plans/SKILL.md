---
name: iris-query-plans
tier: extra
description: Read an IRIS SQL query plan and fix index problems. Use when a query is slow, when a count or search returns fewer rows than the table holds, after adding an Index to a persistent class, or when iris_query(mode="explain") shows "Read master map" where you expected an index. Covers master map vs index map, Cost, %BuildIndices, INSERT %NOINDEX, WHERE %NOINDEX as a check, CREATE INDEX, TUNE TABLE, fixed vs collected statistics and outlier selectivity.
author: tdyar
managed_by: iris-agentic-dev
---

# IRIS query plans and indexes

## Read the plan

`iris_query(mode="explain", query="SELECT ... WHERE ...")` returns the plan IRIS will run. Two lines matter most:

| Plan line                                                        | Meaning                                  |
| ---------------------------------------------------------------- | ---------------------------------------- |
| `Read master map Pkg.Table.IDKEY, looping on the subrange of ID` | full scan of the data rows               |
| `Read index map Pkg.Table.StatusIdx`                             | the query uses the index `StatusIdx`     |
| `Cost`                                                           | relative estimate; compare plans with it |

On a 5,000-row table, `WHERE Status = 'OPEN'` with no index costs 33000 and reads the master map. After `CREATE INDEX StatusIdx ON Pkg.Orders (Status)` the same query reads the index map at cost 13720.

## An index added in the class is empty until you build it

Adding `Index CustIdx On Customer;` to a persistent class and compiling it defines the index but does not fill it. Queries that the planner sends through the index then return short, with `SQLCODE = 0` and no error. On a table with 10 rows for customer `C1`:

```sql
SELECT COUNT(*) FROM Pkg.Orders WHERE Customer = 'C1'            -- 0
SELECT COUNT(*) FROM Pkg.Orders WHERE %NOINDEX Customer = 'C1'   -- 10
```

`%NOINDEX` in the WHERE clause tells the planner not to use an index for that condition, so it reads the data. When the two counts differ, the index is stale. Build it:

```objectscript
Set tSC = ##class(Pkg.Orders).%BuildIndices($ListBuild("CustIdx"))
If $$$ISERR(tSC) { Return tSC }
```

After the build, both counts are 10.

DDL is different. `CREATE INDEX` on a table that already has rows builds the index as part of the statement.

## `INSERT %NOINDEX` leaves the index behind

`INSERT %NOINDEX INTO Pkg.Orders ...` writes the row but skips index maintenance. A count through the index then misses the new row (0), and the same count with `WHERE %NOINDEX` finds it (1). So the method that does the `INSERT %NOINDEX` has to call `%BuildIndices` when its inserts are done:

```objectscript
For i = 1:1:tCount {
    Set tRs = tStmt.%Execute(...)
    If tRs.%SQLCODE < 0 { Return $$$ERROR($$$SQLError, tRs.%SQLCODE, tRs.%Message) }
}
Quit ##class(Pkg.Orders).%BuildIndices($ListBuild("CustIdx"))
```

A rebuild by hand lasts until the next load. If the loader clears the extent and inserts with `%NOINDEX` again, the index is empty again, and a fix that only ran `%BuildIndices` once is gone. Change the loader, or drop `%NOINDEX` from it.

## `TUNE TABLE`

`TUNE TABLE Pkg.Orders` measures extent size (row count), selectivity per field, outlier values, average field size and block counts. Run it on data shaped like production, after a large load or when the distribution changes. `iris_query` refuses `TUNE TABLE` in both modes, so run it through `iris_execute`: `Set r=$SYSTEM.SQL.Execute("TUNE TABLE Pkg.Orders") Write r.%SQLCODE` (0 when it ran). On the test table the `CustIdx` plan stayed on the index map and its cost went from 3272 to 2672.

## Statistics: fixed and collected

Since 2025.2 ([release notes](https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=GCRN_new20252#GCRN_new20252_sql)) a table can hold two sets of the same statistics ([GSOD_opttable](https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=GSOD_opttable#GSOD_opttable_colvfix)):

- **Collected**: stored with the data, written by `TUNE TABLE` and by the system task `%SYS.Task.AutoStatsCollection`.
- **Fixed**: stored in the class's Storage definition (`ExtentSize`, `Selectivity`), written by `ALTER TABLE ... FIX STATISTICS`, `$SYSTEM.SQL.Stats.Table.SetExtentSize`/`SetFieldSelectivity`, or `Import`.

What 2026.3 does, each point measured:

- **A fixed set wins, whole.** With collected extent 2000 and a fixed extent of 5,000,000, the plan for `WHERE Flag = 'Y'` cost 20,707,000 and ran a parallel scan; on collected alone it cost 12,200. The fixed set also hid the collected outlier selectivity, so the plan's "Using the outlier selectivity" note went away.
- **`TUNE TABLE` writes only the collected set.** It leaves the Storage definition alone. While a fixed set exists, TUNE changes nothing the planner reads. The [TUNE TABLE page](https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=RSQL_tunetable) still describes the older behaviour, where TUNE wrote the class.
- **A cached plan changes at the next prepare, not during TUNE.** `INFORMATION_SCHEMA.STATEMENTS.Timestamp` stays the same when TUNE runs, and a new prepare of the same text after TUNE gets a new timestamp. If you prepare again without a TUNE in between, the cached plan is reused.
- A table made with `CREATE TABLE` starts with no fixed set. `Export` with type 1 still reports `<extentsize>100000</extentsize>` for it, which is the default and not a fixed value.
- `ClearTableStats("Pkg.T")` (and `ClearSchemaStats`) clears the fixed set only. The collected set survives.
- 2026.3 ships with Adaptive Mode on, and with `AutoStatsForFixedStatsTable=1`, so collection still runs on tables that have a fixed set ([RACS_AutoStatsForFixedStatsTable](https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=RACS_AutoStatsForFixedStatsTable)).

Not tested here: the docs say collecting statistics updates the extent index in the namespace's default globals database, so collection fails when that database is read-only ([GSOD_opttable](https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=GSOD_opttable)).

So when a plan's cost ignores the real row count, look for a fixed set before running TUNE again:

```sql
SELECT ExtentSize FROM %Dictionary.StorageDefinition WHERE parent = 'Pkg.Orders'
```

An empty `ExtentSize` means the planner uses collected statistics.

### Change statistics with a way back

1. Save what is there now. Type 1 is the fixed set, 2 the latest collected set, 3 both:

   ```objectscript
   Set tSC = $SYSTEM.SQL.Stats.Table.Export("/tmp/orders-stats.xml", "Pkg", "Orders", 0, 3)
   If $$$ISERR(tSC) { Return tSC }
   ```

2. Check for a fixed set (the query above).
3. To let collected statistics through, run `ALTER TABLE Pkg.Orders DROP FIXED STATISTICS` ([ALTER TABLE STATS](https://docs.intersystems.com/iris20262/csp/docbook/Doc.View.cls?KEY=RSQL_altertable#RSQL_altertable_desc_stats)), then `TUNE TABLE Pkg.Orders`.
4. Explain the query again and compare its `Cost`.
5. To roll back, run `$SYSTEM.SQL.Stats.Table.Import("/tmp/orders-stats.xml", 0)`. Importing a type 1 export restores the fixed set.
6. To pin plans that are good now, run `ALTER TABLE Pkg.Orders FIX STATISTICS`. It copies the latest collected set into the Storage definition, so later TUNE runs no longer move the plan.

## An outlier value reads the master map on purpose

If one value covers most of the rows, the planner skips the index for that value. With `Status = 'DONE'` on 98% of rows and `StatusIdx` present, the plan for `WHERE Status = 'DONE'` reads the master map at cost 33000, with the note "Using the outlier selectivity". That plan is correct. Reading nearly every row through an index costs more than a scan. A plan without the index is not always a missing index. Check which value the query uses before you add one.

## Checklist

1. Explain the query and read which map it uses.
2. If a count looks low, run it again with `WHERE %NOINDEX`. Different counts mean a stale index.
3. After adding an `Index` to a class that already has data, run `%BuildIndices`. A loader that uses `INSERT %NOINDEX` calls it itself, after its last insert.
4. After a large load, run `TUNE TABLE`. If the plan's cost does not move, look for a fixed set in the Storage definition.
5. A master-map plan for a common value is expected (outlier selectivity).

## Related skills

- `iris-sql`: SQL dialect, table naming, `SQLCODE`
- `objectscript-sql-patterns`: `%SQL.Statement` error checks
