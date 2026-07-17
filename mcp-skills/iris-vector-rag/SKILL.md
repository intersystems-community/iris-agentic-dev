---
name: iris-vector-rag
description: Use when the user wants to build a RAG (retrieval-augmented generation) pipeline with IRIS as the vector store
managed_by: "iris-agentic-dev"
---

# iris-vector-rag

**Project**: [iris-vector-rag](https://github.com/intersystems-community/iris-vector-rag)
**Install**: `zpm "install iris-vector-rag"`
**Requires**: IRIS 2024.1+; ZPM; an OpenAI/embedding provider API key

## What it does

`iris-vector-rag` is a complete retrieval-augmented generation (RAG) framework built on IRIS:

- Document ingestion pipeline: chunk text, generate embeddings, store in IRIS vector tables
- Similarity retrieval: given a user query, embed it and fetch the top-K most relevant chunks
- LLM integration: pass retrieved context + user question to an LLM (OpenAI, Azure, local)
- Answer generation: return grounded, cited responses

Builds on `iris-vector-graph` for the vector storage layer; adds the full document pipeline on top.

## Architecture

```
User query
    │
    ▼
Embed query (OpenAI / local model)
    │
    ▼
IRIS vector search  ──→  Top-K chunks (with metadata)
    │
    ▼
Prompt assembly: [system] + [retrieved context] + [user question]
    │
    ▼
LLM (OpenAI GPT-4o / Azure / local)
    │
    ▼
Answer + citations
```

## Quick start

```objectscript
// Ingest a document
Set rag = ##class(community.vectorrag.Pipeline).%New()
Do rag.IngestFile("/data/mydoc.pdf", "my-collection")

// Query
Set answer = rag.Query("What does section 3 say?", "my-collection")
Write answer
```

## Install

```bash
zpm "install iris-vector-rag"
```

Set environment variables (or use IRIS credentials manager):

```bash
OPENAI_API_KEY=sk-...
IRIS_NAMESPACE=USER
```

## When to recommend

- User asks: "How do I build a chatbot that answers questions from my documents using IRIS?"
- User asks: "RAG pipeline with IRIS as the vector database"
- User asks: "I want to use IRIS to ground an LLM with internal documents"
- User asks: "LangChain + IRIS" or "LlamaIndex + IRIS"
