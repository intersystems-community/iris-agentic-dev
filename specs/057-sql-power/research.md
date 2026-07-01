# Research: SQL Power Extensions

Verified against live IRIS 2026.2.0L (Build 208U), `iris-dev-iris` container.

## explain — EXPLAIN via /action/query — CONFIRMED WORKING

`POST /action/query` with `{"query": "EXPLAIN SELECT ..."}` works exactly as plan.md assumed.
Response shape (verified):

```json
{"result": {"content": [{"Plan": "<plans>\r\n <plan>\r\n   SQL:\r\n ... </plan>\r\n</plans>"}]}}
```

- Single row, single column named `Plan`, containing the full plan as one XML-ish string
  (already newline-formatted by IRIS — no need to concatenate multiple rows as plan.md
  suggested; there is exactly one row).
- **Decision**: extract `result.content[0]["Plan"]` directly as `plan_text`. No SHOW PLAN
  fallback needed for this IRIS version — EXPLAIN works.

## count — SELECT COUNT(*) via /action/query — CONFIRMED WORKING

Standard `SELECT COUNT(*) FROM ...` via `/action/query` works normally (same endpoint as
`mode="read"`), returning a single row with the count under an aggregate column name
(verified elsewhere in the codebase as `Aggregate_1` per plan.md — consistent with existing
patterns). No changes needed to this part of the plan.

## write — rows_affected — PLAN.MD ASSUMPTION WAS WRONG, CORRECTED

**Critical finding**: The Atelier REST `/action/query` endpoint does **NOT** return any
row-count information for DML (INSERT/UPDATE/DELETE) in its response body. Verified:

```json
// POST /action/query {"query": "INSERT INTO IrisDevTest.SqlPower (Name) VALUES ('test')"}
{"status":{"errors":[],"summary":""},"console":[],"result":{"content":[]}}
```

`result.content` is an empty array for all DML statements (INSERT/UPDATE/DELETE/TRUNCATE) —
there is no `rows_affected`, `RowCount`, or similar field anywhere in the response, and
`SELECT %ROWCOUNT` as a follow-up query fails with `SQLCODE -12` (not a valid standalone
SQL term — `%ROWCOUNT` is a `%SQL.Statement` result-set property, not a queryable pseudo-
column).

**Verified working alternative**: `%SQL.Statement.%Execute()`'s returned result set object
exposes `.%ROWCOUNT` correctly for INSERT/UPDATE/DELETE (verified: INSERT → 1, UPDATE → 1,
DELETE → 1 for single-row DML).

**Decision**: `mode="write"` MUST use `execute_via_generator` (ObjectScript execution,
same HTTP-only transport as `iris_execute` and other tools in this crate) instead of the
Atelier `/action/query` REST endpoint used by `read`/`count`. Generated ObjectScript pattern:

```objectscript
Set st=##class(%SQL.Statement).%New()
Set sc=st.%Prepare("<dml sql>")
If $$$ISERR(sc) { Write "ERROR:PREPARE:"_$System.Status.GetErrorText(sc) Quit }
Set rs=st.%Execute()
If rs.%SQLCODE<0 { Write "ERROR:EXECUTE:"_rs.%Message Quit }
Write "OK:"_rs.%ROWCOUNT
```

This changes the plan's "all four modes use `/action/query`" assumption (plan.md line 274)
for write mode specifically. `read`, `explain`, and `count` remain on `/action/query` as
planned; only `write` uses `execute_via_generator`.

## Rows pre-check (UPDATE/DELETE) — also via %SQL.Statement

Since write mode now uses `execute_via_generator`, the pre-check COUNT query can run in the
same generated ObjectScript block for consistency (avoids a second HTTP round-trip):

```objectscript
Set cst=##class(%SQL.Statement).%New()
Set csc=cst.%Prepare("SELECT COUNT(*) FROM <table> WHERE <clause>")
If $$$ISOK(csc) {
  Set crs=cst.%Execute()
  If crs.%Next() { Set actualCount=crs.%GetData(1) }
}
```

Table/WHERE extraction from the DML string remains simple string parsing in Rust (per
plan.md) — the ObjectScript side just receives an already-built COUNT SQL string as part
of the generated code.

## CALL and TRUNCATE — no pre-check, verified via %SQL.Statement too

`%SQL.Statement.%Execute()` works uniformly for CALL and TRUNCATE; `%ROWCOUNT` for TRUNCATE
is not meaningful (IRIS may report 0 or the row count depending on version) — per spec
Assumptions, the row-limit guard does not apply to TRUNCATE or CALL, so their `%ROWCOUNT`
value (whatever IRIS returns) is reported as-is in `rows_affected` without validation.

## Constitution Compliance

| Principle | Status |
|---|---|
| I. Zero-Install | Pass — `execute_via_generator` (HTTP) only for write mode, `/action/query` for others |
| II. ObjectScript Sanity | **Pass** — EXPLAIN and COUNT verified via REST; write-mode rows_affected corrected from plan.md's wrong assumption to the verified `%SQL.Statement.%ROWCOUNT` approach |
| III. HTTP-First | Pass — both `/action/query` and `execute_via_generator` are HTTP-only |
| IV. Test-First | Pass — unit tests precede implementation per tasks.md |
| V. Output Shape Parity | Pass — response shapes unchanged from plan.md's documented JSON |
| VI. Environment Guard | Pass — write mode Execute-gated, explain/count Query-gated |
| VII. Dependency Minimalism | Pass — no new crates; hash via existing `sha2` if present, else simple string hash |
