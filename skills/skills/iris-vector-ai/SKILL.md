---
author: tdyar
benchmark_date: "2026-04-11"
benchmark_iris_version: "2025.1"
benchmark_tasks:
  - prd-001
  - prd-002
  - prd-003
  - prd-004
  - prd-005
  - prd-006
  - prd-007
compatibility: objectscript, iris, sql, python
description:
  Use when writing any IRIS vector search, embedding, HNSW index, similarity
  search, or AI feature code. Hard gate — IRIS vector syntax is completely different
  from pgvector.
iris_version: ">=2024.1"
license: MIT
metadata:
  baseline_pass_rate: 1.0
  benchmark_note:
    "Source inspection suite. Negative lift on unrelated tasks when
    loaded globally. Load on-demand for vector/AI tasks. RED phase: model plagiarizes
    pgvector syntax 100% without this skill."
  lift: 0.0
  red_phase:
    12 prompts tested — model plagiarizes pgvector syntax 100% of the time
    without this skill
  version: 1.0.0
name: iris-vector-ai
pass_rate: 1.0
state: reviewed
tags:
  - iris
  - vector
  - hnsw
  - embedding
  - ai
  - similarity-search
---

# IRIS Vector & AI — Hard Gate

**IRIS vector syntax is NOT pgvector. Stop. Read this before writing any vector code.**

## HARD GATE

- [ ] VECTOR column: `VECTOR(DOUBLE, 384)` — type AND dimension required, not just `vector(384)`
- [ ] HNSW index: `AS HNSW(Distance='Cosine')` — NOT `USING hnsw (col vector_cosine_ops)`
- [ ] Distance parameter: `'Cosine'` or `'DotProduct'` only — NOT `'l2'`, `'euclidean'`, `'inner_product'`
- [ ] HNSW tuning params are exactly `M` and `efConstruction` — spelled `efConstruction`, NOT `efConstruct`, NOT `ef_construction`
- [ ] **`efSearch` is NOT a `AS HNSW(...)` parameter** — it is a runtime property on `%SQL.VectorIndex.HNSWIndexer`, not settable in DDL. Passing it to `AS HNSW(...)` fails the CREATE INDEX.
- [ ] Similarity function: `VECTOR_COSINE(a, b)` — NOT `<=>` or `<->` operators
- [ ] Parameter binding: `TO_VECTOR(?, DOUBLE, 384)` — 3 args: value, type, dimension. NOT casting with `::vector`. The type and dimension MUST match the column definition.
- [ ] **NEVER SELECT a VECTOR column directly** — the DB-API driver returns it as a plain `str` (comma-separated floats). Use `VECTOR_COSINE()` or `VECTOR_DOT_PRODUCT()` in the SQL; never try to deserialize the raw column value as a list or array.
- [ ] **TO_VECTOR type mismatch causes SQLCODE -259** — `TO_VECTOR(?)` with no type arg creates a different internal datatype than `TO_VECTOR(?, DOUBLE, 384)`. Always pass all 3 args. Mismatched type (e.g., DOUBLE vs FLOAT) or dimension causes "different datatypes" or "different lengths" errors even when the content looks the same.
- [ ] Embedding function: `EMBEDDING('config-name', ?)` — references `%Embedding.Config` table
- [ ] Embedded Python: `%SYS.Python.Import("module")` — NOT `IRIS.Python.New()`
- [ ] **HNSW activation**: the query MUST use `TOP N ... ORDER BY <score> DESC` — no `ORDER BY` means the HNSW index is ignored and the query full-scans the table
- [ ] **ERROR #15806 "Vector Search not permitted"** — read the CRITICAL GOTCHA below BEFORE concluding it is a license issue
- [ ] **Vector search ships in ALL editions** — Community, Enterprise, AI builds — do NOT say "you need Enterprise for VECTOR"

---

## CRITICAL GOTCHA: ERROR #15806 Is NOT Always a License Issue

**When you see:** `ERROR #15806: Vector Search not permitted with current license`

**STOP. Do NOT immediately conclude the container lacks a Vector Search license.**

### What ERROR #15806 Actually Means

IRIS Vector Search is available in **all** editions — Community, Enterprise, and AI builds.
If you see #15806, the real cause is almost certainly one of:

1. **Syntax error in the VECTOR DDL** → IRIS fails to compile the generated table class → surfaces as #15806
2. **Wrong `iris.key` or no `iris.key`** → container running as Community without the key (rare — Community has vector search too)
3. **Typo in the VECTOR type** (e.g. `VECTOR(FLOAT, 384)` where the rest of the code assumes `VECTOR(DOUBLE, 384)`)

### Diagnosis Protocol

**Step 1 — test raw VECTOR DDL directly, bypassing all framework code:**

```objectscript
Set stmt = ##class(%SQL.Statement).%New()
Set sc = stmt.%Prepare("CREATE TABLE Test.VecProbe (id INT, v VECTOR(DOUBLE, 3))")
Write "Prepare sc: ", sc, !
If 'sc { Write ##class(%SYSTEM.Status).GetErrorText(sc), ! }

Set rs = stmt.%Execute()
Write "SQLCODE: ", rs.%SQLCODE, !
If rs.%SQLCODE < 0 { Write rs.%Message, ! }
```

If this **succeeds**, the container has vector search — the problem is in your table DDL, not the
license. If it **fails with #15806**, go to Step 2.

**Step 2 — verify the `iris.key`:**

```bash
docker exec <container> ls -la /usr/irissys/mgr/iris.key || echo "NO KEY — container is Community Edition"
```

Community Edition has vector search without any key. Enterprise needs a key.

**Step 3 — if no key exists but you need enterprise features:**

```bash
docker cp ~/ws/iris-devtester/iris.key <container>:/usr/irissys/mgr/iris.key
docker exec <container> bash -c 'iris stop IRIS quietly && iris start IRIS quietly'
```

**Step 4 — if the raw DDL fails even after the key check**, the error is in the SQL syntax:

- Both type and dimension are required: `VECTOR(DOUBLE, N)`
- `N` must be a literal integer, not a variable
- The type must match everywhere it is referenced — column DDL, `TO_VECTOR()`, index
- The HNSW index must be a separate DDL statement, NOT inline in `CREATE TABLE`

### The exact false-alarm pattern

```objectscript
// Framework code called VectorStore.Build(), which internally calls CreateTable().
// CreateTable() generated an invalid SQL class definition, the IRIS class compiler
// failed, and the failure surfaced as #15806 "Vector Search not permitted".
// ACTUAL CAUSE: syntax error in the generated DDL, not a license problem.

%AI.RAG.VectorStore.IRIS.Build()                       // -> #15806
// but, at the same time, on the same instance:
CREATE TABLE Test.V (v VECTOR(DOUBLE,3))               // -> SQLCODE: 0
```

The container had vector search. The `%AI.RAG.VectorStore` framework had a bad DDL template.

---

## VECTOR Column and Index

```sql
-- CORRECT IRIS syntax (NOT pgvector):
CREATE TABLE Company.People (
    Name VARCHAR(100),
    Biography VECTOR(DOUBLE, 384)   -- type + dimension required
)

-- CORRECT HNSW index:
CREATE INDEX HNSWIdx ON TABLE Company.People (Biography)
  AS HNSW(Distance='Cosine')

-- With tuning params (defaults on %SQL.Index.HNSW: Distance=Cosine, M=16, efConstruction=64):
CREATE INDEX HNSWIdx ON TABLE Company.People (Biography)
  AS HNSW(M=32, efConstruction=200, Distance='DotProduct')

-- WRONG (misspelled param — SQLCODE -400, ERROR #5480 Index parameter not declared):
CREATE INDEX HNSWIdx ON TABLE Company.People (Biography)
  AS HNSW(M=32, efConstruct=200)

-- WRONG (efSearch is a runtime indexer property, not a DDL parameter):
CREATE INDEX HNSWIdx ON TABLE Company.People (Biography)
  AS HNSW(efSearch=100)

-- WRONG (pgvector syntax — does NOT work in IRIS):
CREATE INDEX ON embeddings USING hnsw (embedding vector_cosine_ops);
CREATE INDEX ON t USING hnsw (col) WITH (m=16, ef_construction=64);
```

`ProjectMap` and `SUPPORTSPARTITIONING` also exist on `%SQL.Index.HNSW` but are framework
internals — not tuning knobs.

### Failed CREATE INDEX looks exactly like an inert index

A rejected `CREATE INDEX` leaves the table with **no** vector index. Downstream that is
indistinguishable from an index that exists but has an empty HNSW graph: every query does a
full scan, results are still correct, only slow. This ambiguity burned ~9.5 hours in one real
debugging session. Check both sides:

```sql
-- 1. Never ignore the SQLCODE from CREATE INDEX itself.
--    SQLCODE=-400 / ERROR #5480 "Index parameter not declared: <Class>:<Idx>:EFCONSTRUCT"
--    means the index was never created.

-- 2. Confirm the index actually exists:
SELECT Name, TypeClass, Parameters FROM %Dictionary.CompiledIndex
WHERE parent = 'Company.People'
```

## Similarity Search

```sql
-- CORRECT: TOP N nearest neighbors
SELECT TOP 5 Name, VECTOR_COSINE(Biography, TO_VECTOR(?, DOUBLE, 384)) AS score
FROM Company.People
ORDER BY score DESC

-- Embedding() generates vector from text using configured model:
SELECT TOP 5 Name
FROM Company.People
ORDER BY VECTOR_COSINE(Biography, EMBEDDING('myconfig', ?)) DESC

-- WRONG (pgvector operators — don't exist in IRIS):
SELECT * FROM items ORDER BY embedding <=> '[1,2,3]'::vector LIMIT 5;
```

### `TOP N` + `ORDER BY` is what activates the HNSW index

An HNSW index that exists is not an HNSW index that runs. The query shape decides. Without
`ORDER BY <score> DESC` the optimizer has nothing to feed the ANN pass, so it full-scans and
computes `VECTOR_COSINE` for every row — correct results, index bypassed.

```sql
-- WRONG: full table scan, HNSW index NOT used
SELECT node_id, VECTOR_COSINE(vec, TO_VECTOR(?, DOUBLE, 384)) AS score
FROM MyTable
WHERE visibility = 'global'

-- CORRECT: HNSW activated by TOP N + ORDER BY score DESC
SELECT TOP 10 node_id, VECTOR_COSINE(vec, TO_VECTOR(?, DOUBLE, 384)) AS score
FROM MyTable
WHERE visibility = 'global'
ORDER BY score DESC   -- REQUIRED — without this, HNSW is bypassed
```

Pre-filtering with `WHERE` ahead of the `ORDER BY` is correct and efficient — it narrows the
candidate set before the HNSW ANN pass.

## Driver Behavior — What the DB-API Returns

**VECTOR columns come back as plain `str` through the iris.dbapi driver. Always. No exceptions.**

```python
cur.execute("SELECT embedding FROM MyTable WHERE id = 1")
row = cur.fetchone()
# row[0] is a str: "0.1,0.2,0.3,..." — NOT a list, NOT a numpy array, NOT bytes
# type(row[0]) == str   ← this is permanent, not a bug, not fixable
```

**Never do this:**

```python
# WRONG — will fail or silently corrupt
vec = list(row[0])          # gives list of chars, not floats
vec = np.array(row[0])      # gives array of one string
vec = json.loads(row[0])    # JSON parse error (no brackets)
```

**Correct pattern — never SELECT the raw vector; compute similarity in SQL:**

```python
# RIGHT — always use VECTOR_COSINE / VECTOR_DOT_PRODUCT in SQL
cur.execute("""
    SELECT TOP 5 id, VECTOR_COSINE(embedding, TO_VECTOR(?, DOUBLE, 384)) AS score
    FROM MyTable
    ORDER BY score DESC
""", ["0.1,0.2,..."])  # pass query vector as comma-separated string
```

**If you must retrieve a stored vector as Python floats:**

```python
# Convert in SQL, not in Python
cur.execute("SELECT VECTOR_TOARRAY(embedding) FROM MyTable WHERE id=1")
# Returns a comma-separated string you can then parse:
floats = [float(x) for x in row[0].split(",")]
```

## TO_VECTOR Type Contract

`TO_VECTOR` creates a typed vector. The type and dimension **must exactly match** the column definition or SQLCODE -259 fires.

```python
# Column defined as VECTOR(DOUBLE, 384)
# CORRECT:
TO_VECTOR(?, DOUBLE, 384)   # type=DOUBLE, dim=384 — matches column

# WRONG — causes SQLCODE -259 "different datatypes":
TO_VECTOR(?)                # no type/dim — different internal type
TO_VECTOR(?, FLOAT, 384)    # FLOAT ≠ DOUBLE — different datatype
TO_VECTOR(?, DOUBLE, 128)   # 128 ≠ 384 — different lengths
```

**In tests/fixtures**: if a test table was created with dimension 128 and the query uses 768, the test is broken by design — no amount of driver magic fixes a dimension mismatch. Fix the test fixture to match the query dimension, or make dimension a configurable constant.

## Inserting Vectors

```sql
-- From a comma-separated string:
INSERT INTO Company.People (Name, Biography)
VALUES ('Alice', TO_VECTOR('[0.1,0.2,...]', DOUBLE, 384))

-- Python iris.dbapi:
cur.execute("INSERT INTO People (Name, Biography) VALUES (?,TO_VECTOR(?,DOUBLE,384))",
            ["Alice", "0.1,0.2,..."])   -- pass as string without brackets, not list
```

## Version Matrix

| Feature                                   | Min IRIS version | Notes                        |
| ----------------------------------------- | ---------------- | ---------------------------- |
| `VECTOR` datatype                         | **2024.1**       | Works in Community Edition   |
| `VECTOR_COSINE()`, `VECTOR_DOT_PRODUCT()` | **2024.1**       | SIMD-accelerated             |
| HNSW index (`AS HNSW(...)`)               | **2025.1**       | ANN search                   |
| `EMBEDDING()` SQL function                | **2025.1**       | Requires `%Embedding.Config` |
| `%Library.Embedding` class                | **2025.1**       |                              |
| `$VECTOROP` global operation              | **2025.3**       | Batch operations             |
| Sharded HNSW                              | **2026.2**       | Compute/data separation      |

## Embedded Python (`%SYS.Python`)

```objectscript
// CORRECT:
Set pd = ##class(%SYS.Python).Import("pandas")
Set df = pd.DataFrame(data)

// Method written in Python:
Method Analyze() [ Language = python ]
{
    import iris
    return iris.cls("MyClass").GetData()
}

// WRONG (these don't exist):
Set py = ##class(IRIS.Python).New()
Do py.Execute("import pandas")
```

## `%AI.RAG.VectorStore.IRIS` — What the API Actually Is

**Do NOT assume methods exist on this class without verifying.** It ships roughly five
ObjectScript methods; the rest of its advertised capability comes from a Rust bridge binary and
only exists in specific builds.

```objectscript
// Methods confirmed on %AI.RAG.VectorStore.IRIS (2026.2.0 AI builds):
//   Build(fields)        -- creates table + HNSW index via the Rust bridge
//   Cleanup()            -- drops the table
//   CreateTable(fields)  -- internal; called by Build()
//   %OnClose             -- destructor
//   %OnNew               -- constructor

// Methods that do NOT exist on VectorStore:
//   AddDocument()        -- lives on %AI.RAG.KnowledgeBase
//   Search()             -- lives on %AI.RAG.KnowledgeBase
//   UpdateMetadata()     -- does not exist at all; update via raw SQL
```

`%AI.RAG.KnowledgeBase` is a RAG document-chunking layer. It is the wrong primitive for typed
records with their own fields (confidence, usefulness, expiry, and so on) — use raw VECTOR SQL
tables for those.

If `Build()` returns #15806 and your raw VECTOR DDL works fine, the Rust bridge template has a DDL
bug for your specific field configuration. Bypass VectorStore and manage the table schema yourself
with raw SQL.

---

Requires IRIS 2021.2+. Python environment must be configured (see `iris-connectivity` skill).
