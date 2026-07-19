# Release notes — v0.9.3

## What's new

### `iris_doc_search` — search the official InterSystems documentation

A new tool that searches `docs.intersystems.com` via its Algolia index and returns
ranked hits with title, URL, content excerpt, and breadcrumbs.

```text
iris_doc_search(query, version?, product?, hits?)
```

Use it when you need to:

- Find all ways to accomplish something in IRIS ("what are the SQL execution APIs?")
- Look up specific API details without guessing ("what does SQLCODE -30 mean?")
- Discover what's new in a release ("what new AI/ML features shipped in 2026.1?")
- Get vector SQL function signatures, interoperability class names, etc.

Optional filters: `version` (e.g. `"2026.1"`) and `product` (e.g. `"InterSystems IRIS"`).
Returns up to 10 hits with 600-character excerpts.

**Why a dedicated tool?** `docs.intersystems.com` is a JavaScript SPA — `WebFetch`
and `curl` return only the nav shell, not page content. This tool hits the real
Algolia search index that the site itself uses, so answers come from authoritative
documentation, not training-data memory.

### Updated `iris-docs` skill

The `iris-docs` skill now leads with `iris_doc_search` and includes a decision table
explaining when to use each documentation tool (`iris_doc_search` vs `docs_introspect`
vs `iris_doc`). A fallback curl recipe and known-gotchas section are included.

## Fixes

None.

## Upgrade

```bash
brew upgrade iris-agentic-dev
```
