---
author: tdyar
benchmark_date: '2026-04-13'
benchmark_iris_version: '2025.1'
benchmark_tasks:
- prd-001
- prd-004
- prd-005
- prd-006
- prd-007
- prd-008
- prd-009
description: Use when writing any IRIS vector search, embedding, HNSW index, similarity
  search, or AI feature code. Hard gate — IRIS vector syntax is completely different
  from pgvector.
iris_version: '>=2024.1'
name: tdyar/iris-vector-ai
pass_rate: 1.0
state: reviewed
tags:
- iris
- vector
- hnsw
- embedding
- ai
- similarity-search
trigger: ''
---

# IRIS Vector & AI — Hard Gate

**IRIS vector syntax is NOT pgvector. Stop. Read this before writing any vector code.**

## HARD GATE

- [ ] VECTOR column: `VECTOR(DOUBLE, 384)` — type AND dimension required, not just `vector(384)`
- [ ] HNSW index: `AS HNSW(Distance='Cosine')` — NOT `USING hnsw (col vector_cosine_ops)`
- [ ] Distance parameter: `'Cosine'` or `'DotProduct'` only — NOT `'l2'`, `'euclidean'`, `'inner_product'`
- [ ] Similarity function: `VECTOR_COSINE(a, b)` — NOT `<=>` or `<->` operators
- [ ] Parameter binding: `TO_VECTOR(?)` — NOT casting with `::vector`
- [ ] Embedding function: `EMBEDDING('config-name', ?)` — references `%Embedding.Config` table
- [ ] Embedded Python: `%SYS.Python.Import("module")` — NOT `IRIS.Python.New()`
- [ ] **HNSW activation**: query MUST use `TOP N ... ORDER BY score DESC` — no ORDER BY = full table scan, HNSW ignored
- [ ] **ERROR #15806 "Vector Search not permitted"** — see CRITICAL GOTCHA below BEFORE concluding it's a license issue
- [ ] **Vector available in ALL editions** — Community, Enterprise, AI builds — do NOT say "need Enterprise for VECTOR"

---

## CRITICAL GOTCHA: ERROR #15806 Is NOT Always a License Issue

**When you see:** `ERROR #15806: Vector Search not permitted with current license`

**STOP. Do NOT immediately conclude the container lacks a Vector Search license.**

### What ERROR #15806 Actually Means

IRIS Vector Search is available in **ALL** editions — Community, Enterprise, and AI builds. If you see #15806, the real cause is almost certainly one of:

1. **Syntax error in the VECTOR DDL** → IRIS fails to compile the generated table class → surfaces as #15806
2. **Wrong iris.key or no iris.key** → container running as Community without the key (rare — Community has Vector Search too)
3. **Typo in VECTOR type** (e.g., `VECTOR(FLOAT, 384)` instead of `VECTOR(DOUBLE, 384)`)

### Diagnosis Protocol

**Step 1: Test raw VECTOR DDL directly in %SYS namespace:**
```objectscript
// Run this before anything else — bypasses all framework code:
Set stmt = ##class(%SQL.Statement).%New()
Set sc = stmt.%Prepare("CREATE TABLE Test.VecProbe (id INT, v VECTOR(DOUBLE, 3))")
Write "Prepare sc: ", sc, !
If 'sc { Write ##class(%SYSTEM.Status).GetErrorText(sc), ! }

Set rs = stmt.%Execute()
Write "SQLCODE: ", rs.%SQLCODE, !
If rs.%SQLCODE < 0 { Write rs.%Message, ! }
```

If this **succeeds** → your container has Vector Search. The problem is in your table DDL, not the license.

If this **fails with #15806** → check Step 2.

**Step 2: Verify the iris.key:**
```bash
docker exec <container> ls -la /usr/irissys/mgr/iris.key || echo "NO KEY — container is Community Edition"
```
Community Edition has Vector Search without any key. Enterprise edition needs a key.

**Step 3: If no key exists but you need enterprise features:**
```bash
# Copy your iris.key into the running container:
docker cp ~/ws/iris-devtester/iris.key <container>:/usr/irissys/mgr/iris.key
docker exec <container> bash -c 'iris stop IRIS quietly && iris start IRIS quietly'
```

**Step 4: If the raw DDL fails even after key check:**
The error is in the SQL syntax. Check:
- Must be `VECTOR(DOUBLE, N)` — both type and dimension required
- `DOUBLE` is the only supported type (not `FLOAT`, `INT`, `REAL`)
- `N` must be a literal integer, not a variable
- HNSW index must be separate DDL statement, NOT inline in CREATE TABLE

### The Exact False Alarm Pattern (from 2026-04-20)

```
// This framework code called VectorStore.Build() which internally calls CreateTable()
// CreateTable() generated invalid SQL class definition
// IRIS class compiler failed → surfaced as #15806 "Vector Search not permitted"
// ACTUAL CAUSE: syntax error in generated DDL, not a license problem

%AI.RAG.VectorStore.IRIS.Build() → #15806
// BUT:
Direct SQL "CREATE TABLE Test.V (v VECTOR(DOUBLE,3))" → SQLCODE: 0  ✅
```

**The container HAD Vector Search. The %AI.RAG.VectorStore framework had a bad DDL template.**

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
SELECT TOP 5 Name, VECTOR_COSINE(Biography, TO_VECTOR(?)) AS score
FROM Company.People
ORDER BY score DESC

-- Embedding() generates vector from text using configured model:
SELECT TOP 5 Name
FROM Company.People
ORDER BY VECTOR_COSINE(Biography, EMBEDDING('myconfig', ?)) DESC

-- WRONG (pgvector operators — don't exist in IRIS):
SELECT * FROM items ORDER BY embedding <=> '[1,2,3]'::vector LIMIT 5;
```

## Inserting Vectors

```sql
-- From a comma-separated string:
INSERT INTO Company.People (Name, Biography)
VALUES ('Alice', TO_VECTOR('[0.1,0.2,...]'))

-- Python iris.dbapi:
cur.execute("INSERT INTO People (Name, Biography) VALUES (?,TO_VECTOR(?))",
            ["Alice", "[0.1,0.2,...]"])   -- pass as string, not list
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

## HNSW Index Activation — ORDER BY is Mandatory

```sql
-- WRONG: Full table scan, HNSW index NOT used
SELECT node_id, VECTOR_COSINE(vec, TO_VECTOR(?)) AS score
FROM MyTable
WHERE visibility = 'global'

-- CORRECT: HNSW index activated by TOP N + ORDER BY score DESC
SELECT TOP 10 node_id, VECTOR_COSINE(vec, TO_VECTOR(?)) AS score
FROM MyTable
WHERE visibility = 'global'
ORDER BY score DESC   -- REQUIRED — without this, HNSW is bypassed
```

Pre-filtering with WHERE before the ORDER BY is correct and efficient — it narrows candidates before the HNSW ANN pass.

---

## %AI.RAG.VectorStore.IRIS — What the API Actually Is

**Do NOT assume methods exist on this class without verifying.** It ships ~5 ObjectScript methods; additional capabilities come from a Rust bridge binary and only exist in specific builds.

```objectscript
// Methods confirmed on %AI.RAG.VectorStore.IRIS (2026.2.0 AI builds):
//   Build(fields)        -- creates table + HNSW index via Rust
//   Cleanup()            -- drops table
//   CreateTable(fields)  -- internal; called by Build()
//   %OnClose             -- destructor
//   %OnNew               -- constructor

// Methods that DO NOT exist directly on VectorStore:
//   AddDocument()        -- lives on %AI.RAG.KnowledgeBase
//   Search()             -- lives on %AI.RAG.KnowledgeBase
//   UpdateMetadata()     -- does NOT exist as a method; update via raw SQL

// %AI.RAG.KnowledgeBase is a RAG document chunking layer — wrong primitive
// for typed memory entries with SEDM fields (confidence, usefulness, expiry).
// Use raw VECTOR SQL tables for typed memory entries.
```

**If Build() returns #15806 and your raw VECTOR DDL works fine** → the Rust bridge template has a DDL bug for your specific field configuration. Bypass VectorStore and manage the table schema yourself with raw SQL.

---

Requires IRIS 2021.2+. Python environment must be configured (see `iris-connectivity` skill).