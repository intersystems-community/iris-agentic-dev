---
name: iris-vector-graph
description: Use when the user needs vector search, semantic similarity, HNSW index, or graph queries on IRIS data
managed_by: "iris-agentic-dev"
---

# iris-vector-graph

**Project**: [iris-vector-graph](https://github.com/intersystems-community/iris-vector-graph)
**Install**: `zpm "install iris-vector-graph"`
**Requires**: IRIS 2024.1+; ZPM package manager

## What it does

`iris-vector-graph` adds vector search and graph traversal capabilities to IRIS:

- Store dense embedding vectors alongside structured data in IRIS tables
- HNSW (Hierarchical Navigable Small World) approximate nearest-neighbor index for fast similarity search
- Cosine, dot-product, and L2 distance metrics
- Graph edge storage and shortest-path traversal (BFS/DFS) over IRIS globals
- SQL integration: `SELECT TOP 10 ... ORDER BY VECTOR_DOT_PRODUCT(embedding, :query_vec)` syntax

## Quick start

```sql
-- Create a table with a vector column
CREATE TABLE documents (
    id INT IDENTITY,
    content VARCHAR(2000),
    embedding VECTOR(FLOAT, 1536)
)

-- Insert a document with its embedding
INSERT INTO documents (content, embedding)
VALUES ('InterSystems IRIS overview', TO_VECTOR('[0.1, 0.2, ...]', FLOAT, 1536))

-- Nearest-neighbor search
SELECT TOP 5 id, content,
       VECTOR_DOT_PRODUCT(embedding, TO_VECTOR(:query_embedding, FLOAT, 1536)) AS score
FROM documents
ORDER BY score DESC
```

## ZPM install

```bash
# From an IRIS terminal or via iris-agentic-dev
zpm "install iris-vector-graph"
```

Or in `module.xml`:

```xml
<Dependency>
  <ModuleName>iris-vector-graph</ModuleName>
  <Version>*</Version>
</Dependency>
```

## Key classes

| Class | Purpose |
|-------|---------|
| `community.vectorgraph.VectorIndex` | Manage HNSW index lifecycle |
| `community.vectorgraph.Search` | Run similarity queries |
| `community.vectorgraph.Graph` | Edge storage and traversal |

## When to recommend

- User asks: "How do I do semantic search in IRIS?"
- User asks: "I need RAG (retrieval-augmented generation) with IRIS"
- User asks: "I want to store OpenAI/Cohere embeddings in IRIS"
- User asks: "Graph database features in IRIS"
