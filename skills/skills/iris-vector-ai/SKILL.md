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
compatibility: objectscript, iris, sql, python
description: Use when writing any IRIS vector search, embedding, HNSW index, similarity
  search, or AI feature code. Hard gate — IRIS vector syntax is completely different
  from pgvector.
iris_version: '>=2024.1'
license: MIT
metadata:
  baseline_pass_rate: 1.0
  benchmark_note: 'Source inspection suite. Negative lift on unrelated tasks when
    loaded globally. Load on-demand for vector/AI tasks. RED phase: model plagiarizes
    pgvector syntax 100% without this skill.'
  lift: 0.0
  red_phase: 12 prompts tested — model plagiarizes pgvector syntax 100% of the time
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
- [ ] Similarity function: `VECTOR_COSINE(a, b)` — NOT `<=>` or `<->` operators
- [ ] Parameter binding: `TO_VECTOR(?, DOUBLE, 384)` — 3 args: value, type, dimension. NOT casting with `::vector`. The type and dimension MUST match the column definition.
- [ ] **NEVER SELECT a VECTOR column directly** — the DB-API driver returns it as a plain `str` (comma-separated floats). Use `VECTOR_COSINE()` or `VECTOR_DOT_PRODUCT()` in the SQL; never try to deserialize the raw column value as a list or array.
- [ ] **TO_VECTOR type mismatch causes SQLCODE -259** — `TO_VECTOR(?)` with no type arg creates a different internal datatype than `TO_VECTOR(?, DOUBLE, 384)`. Always pass all 3 args. Mismatched type (e.g., DOUBLE vs FLOAT) or dimension causes "different datatypes" or "different lengths" errors even when the content looks the same.
- [ ] Embedding function: `EMBEDDING('config-name', ?)` — references `%Embedding.Config` table
- [ ] Embedded Python: `%SYS.Python.Import("module")` — NOT `IRIS.Python.New()`

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

-- With tuning params:
CREATE INDEX HNSWIdx ON TABLE Company.People (Biography)
  AS HNSW(M=24, efConstruct=100, Distance='DotProduct')

-- WRONG (pgvector syntax — does NOT work in IRIS):
CREATE INDEX ON embeddings USING hnsw (embedding vector_cosine_ops);
CREATE INDEX ON t USING hnsw (col) WITH (m=16, ef_construction=64);
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

| Feature | Min IRIS version | Notes |
|---------|-----------------|-------|
| `VECTOR` datatype | **2024.1** | Works in Community Edition |
| `VECTOR_COSINE()`, `VECTOR_DOT_PRODUCT()` | **2024.1** | SIMD-accelerated |
| HNSW index (`AS HNSW(...)`) | **2025.1** | ANN search |
| `EMBEDDING()` SQL function | **2025.1** | Requires `%Embedding.Config` |
| `%Library.Embedding` class | **2025.1** | |
| `$VECTOROP` global operation | **2025.3** | Batch operations |
| Sharded HNSW | **2026.2** | Compute/data separation |

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

Requires IRIS 2021.2+. Python environment must be configured (see `iris-connectivity` skill).