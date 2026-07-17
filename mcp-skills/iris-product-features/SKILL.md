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
compatibility: objectscript, iris, sql
description: Use when asked about IRIS capabilities, products, or features — especially
  MCP, full-text search, HL7/Interoperability, mirroring, IRIS for Health vs HealthShare.
  AI models confidently describe features that don't exist or confuse products.
iris_version: '>=2024.1'
license: MIT
metadata:
  baseline_pass_rate: 1.0
  benchmark_note: Source inspection suite. Negative lift when loaded globally (-29%).
    Load on-demand when asked about IRIS capabilities/products. Features ARE in IRIS
    — this skill prevents denial of existence.
  lift: -0.286
  red_phase: Model denies MCP exists, invents Python HL7 APIs, confuses IRIS/HealthShare,
    uses PostgreSQL FTS syntax
  version: 1.0.0
name: tdyar/iris-product-features
pass_rate: 0.714
state: reviewed
tags:
- iris
- features
- mcp
- interoperability
- fts
- health
- capabilities
---

# IRIS Product Features — Hard Gate

**AI models deny features that exist and invent features that don't. Check here first.**

## HARD GATE

- [ ] **MCP**: IRIS has NATIVE MCP support (`%AI.MCP.Service`) since 2026.1 — no third-party FastMCP wrapper needed
- [ ] **Full-text search**: Use `%CONTAINS` predicate with iFind index — NOT `to_tsvector` or `LIKE '%term%'`
- [ ] **HL7/Interoperability**: Routing is done in the Management Portal or ObjectScript — NO Python API exists
- [ ] **IRIS for Health ≠ HealthShare**: different products (see below)
- [ ] **Mirroring**: configured via Management Portal or CPF files — NO CLI command `iris config mirroring`
- [ ] **IRIS ≠ Caché**: Class/method naming and some APIs changed — always use IRIS docs, not Caché docs

---

## Native MCP Server (IRIS 2026.1+)

```objectscript
// IRIS has a built-in MCP server — no FastMCP, no Python required
// The native server is %AI.MCP.Service
// Configure via Management Portal: System > AI Configuration > MCP

// Wrong assumption models make:
// "IRIS doesn't support MCP natively, you need to wrap it with FastMCP"
// This was true before 2026.1. It is FALSE for IRIS 2026.1+.
```

Claude Desktop / opencode config for native IRIS MCP:
```json
{
  "mcpServers": {
    "iris": {
      "command": "objectscript-mcp",
      "env": {"IRIS_HOST": "localhost", "IRIS_PORT": "1972"}
    }
  }
}
```

## Full-Text Search — iFind (NOT PostgreSQL FTS)

```sql
-- Define iFind index on a class property:
-- Property TextBody As %String;
-- Index iFindIdx On TextBody As %iFind.Index.Basic;

-- CORRECT IRIS query:
SELECT * FROM MyTable WHERE %CONTAINS(TextBody, 'search terms')

-- With ranking:
SELECT *, %iFind.Rank AS relevance
FROM MyTable WHERE %CONTAINS(TextBody, 'search terms')
ORDER BY relevance DESC

-- WRONG (PostgreSQL FTS — doesn't exist in IRIS):
SELECT * FROM MyTable WHERE to_tsvector('english', content) @@ to_tsquery('term');
SELECT * FROM MyTable WHERE content LIKE '%term%';  -- no index, full scan
```

## Interoperability — HL7 Routing

There is **no Python API** for IRIS Interoperability routing. It is configured in:
1. **Management Portal** (recommended): System > Interoperability > Build > Business Processes
2. **ObjectScript** — subclass `Ens.BusinessProcess` or `EnsLib.HL7.MsgRouter.RoutingEngine`

```objectscript
// Routing rule in ObjectScript:
Class MyApp.HL7Router Extends EnsLib.HL7.MsgRouter.RoutingEngine
{
// Rules defined via Rule Editor in Management Portal
// HL7 field access: {MSH:9.1} for message type
// Target: "ADT_Handler" (name of a Business Operation)
}
```

HL7 field path syntax: `{SegmentName:FieldNumber.ComponentNumber}` — e.g., `{MSH:9.1}` for trigger event.

## IRIS Product Family (Not the Same Thing)

| Product | What it is | Includes |
|---------|-----------|---------|
| **IRIS** | Core database + application server | SQL, globals, ObjectScript, embedded Python, interop |
| **IRIS for Health** | IRIS + healthcare layer | + FHIR server, SMART on FHIR, healthcare interop |
| **Health Connect** | Integration engine | HL7, DICOM, FHIR, X12, EDI routing (built on IRIS) |
| **HealthShare** | Clinical data platform | Patient Index, Health Insight, HIE tools (built on IRIS for Health) |

> Rule: IRIS for Health ⊂ HealthShare ⊂ full HealthShare suite. They are NOT the same product and NOT cloud vs on-prem editions of each other.

## Mirroring / High Availability

Mirroring is configured via:
1. **Management Portal**: System > Configuration > Mirror Settings
2. **CPF merge file** for automated deployment
3. **ObjectScript**: `##class(SYS.Mirror).*)` classes

```
// No CLI command exists:
// iris config mirroring --mode=failover   ← DOES NOT EXIST
// Do ##class(SYS.Mirror).Configure(...)   ← not the API

// Correct: use Management Portal wizard or:
Set sc = ##class(Config.MapMirrors).Create("MIRRORNAME", .props)
```

Mirror topology: **Primary** (read/write) → **Failover member** (hot standby) → **Async members** (DR/reporting). Failover is automatic; async members require manual promotion.

## Version Feature Quick Reference

| Feature | Available since | Notes |
|---------|----------------|-------|
| Embedded Python | 2021.2 | `%SYS.Python` class |
| VECTOR datatype | 2024.1 | Community Edition OK |
| HNSW index | 2025.1 | `AS HNSW(Distance='Cosine')` |
| Native MCP server | 2026.1 | `%AI.MCP.Service` |
| iFind full-text | 2012+ | `%iFind.Index.Basic` |
| FHIR R4 server | 2020.1 | IRIS for Health only |
| Secure Wallet | 2025.2 | `%Wallet.*` namespace |
| EMBEDDING() function | 2024.1+ | Community + Enterprise only (see tier table) |

## VECTOR / EMBEDDING License Tiers — CRITICAL

**Source**: IRIS Vector Search EAP page + Product FAQ (Confluence 742419092, 766946553)

### Which tiers have vector search?

| Tier | Metric | Vector Search | Notes |
|------|--------|--------------|-------|
| **IRIS Server** | Core-based | ✗ | Standard production — no vector |
| **IRIS Advanced Server** | Core-based | ✅ | Large-scale analytics — full vector stack |
| **IRIS Enterprise** | User-based | ✗ | Former Caché customers — no vector |
| **IRIS Elite** | User-based | ✗ | Mid-size user-based — no vector |
| **IRIS Entree** | User-based | ✗ | Small teams — no vector |
| **IRIS Community Edition** | Free / dev-only | ✅ | Vector search explicitly included |

**The two tiers with vector search: Advanced Server + Community Edition.**
All user-based tiers (Enterprise, Elite, Entree) and IRIS Server: NO vector search.

```
IRIS Server        ✗ vector
IRIS Advanced Server  ✅ vector  (core-based, full analytics)
IRIS Enterprise    ✗ vector  (user-based — former Caché)
IRIS Elite         ✗ vector  (user-based)
IRIS Entree        ✗ vector  (user-based, small teams)
IRIS Community     ✅ vector  (dev-only, no production use)
```

### `$vectorop` / ObjectScript vector built-ins

`$vectorop`, `$vector` are ObjectScript built-ins available on **ALL tiers** regardless of the SQL vector-search license bit. IVFFlat, BM25Index, VecIndex, PLAID all use `$vectorop` internally and work on all tiers.

### What iris-vector-graph features work where

| IVG Feature | Community | IRIS Server | Advanced Server | Enterprise/Elite/Entree |
|-------------|-----------|-------------|-----------------|------------------------|
| `$vectorop` indexes (IVFFlat, BM25, VecIndex, PLAID) | ✓ | ✓ | ✓ | ✓ |
| `kg_NodeEmbeddings` VECTOR column | ✓ | ✗ | ✓ | ✗ |
| `EMBEDDING()` SQL function | ✓ | ✗ | ✓ | ✗ |
| `VECTOR_COSINE` SQL | ✓ | ✗ | ✓ | ✗ |
| Python `embed_fn` fallback | ✓ | ✓ | ✓ | ✓ |

### Capability detection

```python
caps = GraphSchema.check_objectscript_classes(cursor)
if caps.has_vector_search:
    # SQL VECTOR available — Advanced Server or Community
    # can use kg_NodeEmbeddings, EMBEDDING(), VECTOR_COSINE
else:
    # $vectorop still works — ALL tiers
    # BM25, IVFFlat, PLAID, VecIndex all functional
    # Use Python embed_fn for embeddings
```

### Never say
- "Community doesn't support vector search" ← FALSE (explicitly included)
- "Enterprise has vector search" ← FALSE (user-based tiers don't)
- "Advanced Server is just Enterprise with more features" ← WRONG — different metric (core vs user) and different feature set
- "`$vectorop` requires Advanced Server" ← FALSE (all tiers)
- "IRIS Server is the basic tier with all standard features" ← MISLEADING — it lacks vector search

---

## Interoperability — Namespace Enablement (The Key Bit)

Interoperability is **installed on every IRIS**. The question is whether it's **enabled on a specific namespace**.

```objectscript
// Check if THIS namespace is Interop-enabled:
Write ##class(%EnsembleMgr).IsEnsembleNamespace()
// 1 = enabled, 0 = not enabled (even if Interop is installed)

// Enable a namespace (requires %Admin_Manage privilege):
// Usually done once at setup — not per-session
Do ##class(%EnsembleMgr).EnableNamespace("MYNS", 1)

// Or check from %SYS:
Set ns = "MYNS"
Write ##class(Config.Namespaces).GetEnsemble(ns)
```

**What breaks without enablement:**

```objectscript
// These compile fine but fail at runtime if namespace is NOT Interop-enabled:
Set prod = ##class(Ens.Director).GetActiveProductionName()   // <CLASS DOES NOT EXIST>
Do ##class(Ens.Director).StartProduction("MyApp.Production")  // <CLASS DOES NOT EXIST>
Set msg = ##class(EnsLib.HTTP.OutboundAdapter).%New()         // <CLASS DOES NOT EXIST>
```

The classes exist in the install but their **package mappings** aren't added to the namespace until `EnableNamespace` runs. This is why "IRIS has Interoperability" and "this namespace can run an Interop production" are different things.

**Deploying to a new IRIS namespace that needs Interop:**

1. Verify install: `iris list` shows `Interoperability: installed`
2. Enable namespace: `Do ##class(%EnsembleMgr).EnableNamespace("MYNS", 1)`
3. Confirm: `Write ##class(%EnsembleMgr).IsEnsembleNamespace()` → 1
4. Start a production: use `interop_production_start` MCP tool or Management Portal

**Exporting a namespace that has Interop enabled** will include `EnsLib.*`/`EnsPortal.*` in the export — these are ISC's framework classes, NOT your application code. Strip them from your deployment script; they'll already be present on any target IRIS.

## %AI.RAG — What It Is and Is NOT

**Source**: hard-won lessons from ai-memory-iris and other projects hitting the same wall.

### What %AI.RAG actually is

`%AI.RAG.KnowledgeBase` / `%AI.RAG.VectorStore.IRIS` is a **RAG document retrieval system**. It is designed for:
- Chunking large documents
- Storing chunks as vector embeddings
- Retrieving relevant chunks given a query at inference time

It has **no concept of**:
- Typed memory entries (confidence, usefulness, expiry, visibility)
- SEDM-style admission / decay
- Structured metadata columns that need to be indexed or filtered by type
- Per-entry lifecycle management

**Never use %AI.RAG.KnowledgeBase as a storage primitive for typed AI memory entries.** It is the wrong abstraction.

### The right primitive for typed memory

For typed memory entries (SEDM, episodic memory, working memory), use **raw IRIS SQL tables with VECTOR(DOUBLE, N) columns**:

```sql
CREATE TABLE AI.Memory.Entry (
  entry_id VARCHAR(64) PRIMARY KEY,
  content  %String(MAXLEN=4096),
  embedding VECTOR(DOUBLE, 384),
  confidence FLOAT DEFAULT 1.0,
  usefulness FLOAT DEFAULT 1.0,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  expires_at TIMESTAMP,
  memory_type VARCHAR(32),
  agent_id VARCHAR(64),
  visibility VARCHAR(16) DEFAULT 'private'
)
```

Then use `VECTOR_COSINE(embedding, TO_VECTOR(?, DOUBLE))` for similarity search directly in SQL. No RAG layer needed.

### Vector Search license — the two facts that are ALWAYS true

1. **Community Edition includes Vector Search** — `VECTOR(DOUBLE)`, `VECTOR_COSINE()`, `EMBEDDING()` all work. No license key needed.
2. **iris.key presence is irrelevant to Vector Search** — if the container started (IRIS is running), either it's Community (no key, vector works) or it has a license key. Both cases have vector search if it's Advanced Server or Community.

**If you see "Vector Search not permitted with current license":**
- It is NOT because the container lacks `iris.key`
- It is NOT because Community doesn't have Vector Search
- It IS because either: (a) the syntax is wrong, or (b) the container image is an intermediate paid tier (IRIS Server, Enterprise, Elite, Entree) that genuinely lacks vector search

**Debugging checklist:**
```objectscript
// Check what license type is running
Write $System.Version.Edition(),!
// "Community" = Vector Search OK
// "Advanced Server" = Vector Search OK
// "Enterprise", "Server", "Elite", "Entree" = NO Vector Search

// Check if VECTOR datatype works
Set v = ""
Set $vector(v, 1, "double") = 1.0
Write $vectorlen(v),!  // Should print 1

// Check SQL vector
&sql(SELECT VECTOR_COSINE(TO_VECTOR('1.0,0.0', DOUBLE), TO_VECTOR('1.0,0.0', DOUBLE)) INTO :sc)
Write sc,!  // Should print 1 (exact match)
```

**If you get `<METHOD DOES NOT EXIST>` on `%SYSTEM.License.KeyID()`** — that method doesn't exist in IRIS. Use `$System.Version.Edition()` instead.