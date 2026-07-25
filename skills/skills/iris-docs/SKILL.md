---
name: iris-docs
description: Look up InterSystems IRIS documentation. Use iris_doc_search for discovery questions and API lookups. Covers the Algolia search recipe, Documatic URL pattern, and when to use docs_introspect vs iris_doc_search vs iris_doc.
tags:
  - iris
  - objectscript
  - documentation
  - verification
author: tdyar
state: reviewed
---

# iris-docs — InterSystems Documentation Lookup

## Decision table — which tool to use

| Situation                                        | Tool                               |
| ------------------------------------------------ | ---------------------------------- |
| "What are all the ways to do X in IRIS?"         | `iris_doc_search`                  |
| "What does error code Y mean?"                   | `iris_doc_search`                  |
| "What methods does `%SQL.Statement` have?"       | `iris_doc_search`                  |
| "Does this method exist / what's its signature?" | `docs_introspect` (live container) |
| "Read/write a class document in IRIS"            | `iris_doc`                         |

**`iris_doc_search` is the primary path.** Use it for any question where the answer
lives in official documentation. `docs_introspect` is for inspecting classes in a
running container. `iris_doc` is for reading/writing IRIS documents, not docs.

---

## Primary: `iris_doc_search` tool

```json
{
  "tool": "iris_doc_search",
  "query": "SQL execution methods ObjectScript",
  "product": "InterSystems IRIS",
  "version": "2025.1",
  "hits": 5
}
```

Parameters:

- `query` — natural language or keyword (required)
- `version` — e.g. `"2025.1"`, `"2026.1"` (optional; omit for all versions)
- `product` — e.g. `"InterSystems IRIS"` (optional; omit for all products)
- `hits` — max results 1–10 (optional; default 5)

Returns `{query, total_hits, hits: [{title, url, excerpt, breadcrumbs, version, product}]}`.

**DO NOT use WebFetch or curl on DocBook URLs.** `docs.intersystems.com` is a JavaScript
SPA. Naive fetching returns only a navigation shell, not the documentation content. The
`iris_doc_search` tool uses the real Algolia search index and returns actual body text.
`Doc.View.cls` and `DocBook.UI.Page.cls` give you a JS nav shell and often a 504.
Documatic URLs (below) are the exception — they render server-side and WebFetch works.

---

## Query formulation — bare tokens win

The index behaves as if terms are AND-ed. Every word you add shrinks the result set
toward zero. Measured against the live tool:

| Query                                                                   | Hits                          |
| ----------------------------------------------------------------------- | ----------------------------- |
| `CREATE INDEX AS HNSW parameters M efConstruct Distance`                | 0                             |
| `CREATE INDEX AS HNSW syntax vector index`                              | 0                             |
| `efConstruction`                                                        | 133, correct page is top hit  |
| `efConstruct`                                                           | 1492, same top hit            |
| `vector search index efSearch tune table selectivity optimizer use ANN` | Caché 2011/2017 upgrade notes |

Rules:

1. Start with ONE distinctive token — a class name, a parameter name, an error code.
   Exact identifiers are the strongest signal the index has.
2. Add terms only to narrow a result set that is already non-empty.
3. Never put a hypothesis in the query. Speculative terms ("tune", "selectivity",
   "optimizer") drag results toward unrelated decade-old pages.
4. **Zero hits means your query was over-specified, not that the feature is
   undocumented.** Reformulate down to a bare token before you conclude the docs
   don't cover it. One session read 0 hits as "not documented" and burned ~9.5 hours
   guessing at parameters that were on a Class Reference page the whole time.

`iris_doc_search` returns parameter and member NAMES but not their default VALUES.
For defaults you have to fetch the page.

---

## Recipe: "what parameters does class X accept?"

1. `iris_doc_search` on the bare class name or bare parameter name → find the Class
   Reference hit.
2. WebFetch the Documatic URL for that class → parameters with their defaults.
3. If a live container is up, prefer it. `iris_doc mode=get name=<Class>.cls` plus
   `%Dictionary.CompiledIndex` and `%Dictionary.CompiledParameter` are the most
   authoritative sources available — the compiled dictionary is ground truth, and
   it is what finally settled the HNSW parameter question above.

---

## Fallback: Algolia recipe (manual, if tool is unavailable)

The docs site embeds public search-only credentials in every page. This is permitted by
`robots.txt` (`Content-Signal: search=yes`).

```bash
curl -sS "https://EP91R43SFK-dsn.algolia.net/1/indexes/docs/query" \
  -H "X-Algolia-API-Key: 709759d92d99a5cf927e90c965741389" \
  -H "X-Algolia-Application-Id: EP91R43SFK" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "YOUR QUERY HERE",
    "hitsPerPage": 5,
    "facetFilters": ["product:InterSystems IRIS", "version:2025.1"],
    "attributesToRetrieve": ["title","URL","text","breadcrumbs","version","product"]
  }' | python3 -c "
import sys,json
d=json.load(sys.stdin)
print('nbHits:', d.get('nbHits'))
for h in d.get('hits',[]):
    print('---')
    print(h.get('title',''))
    print(h.get('URL',''))
    print(str(h.get('text',''))[:500])
"
```

Re-scrape credentials if key rotates:

```bash
UA="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
curl -sS -A "$UA" "https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=GCM_monitoring" \
  | grep -oiE 'ALG-[A-Za-z]+.*content="[^"]*"'
```

---

## Fallback: Documatic URL (class reference, known class name)

For direct class reference when you know the exact class name:

```text
https://docs.intersystems.com/iris{VERSION}/csp/documatic/%25CSP.Documatic.cls?LIBRARY={LIBRARY}&CLASSNAME={CLASS}
```

Version codes: `iris20261`, `iris20251`, `iris20241`, `irislatest`

Library codes: `%25SYS` (system classes), `USER` (app classes), `ENSLIB` (Ensemble/Interop)

Add `&PRIVATE=1` to include private/internal members.

**Documatic renders server-side — WebFetch works on it.** This is the one docs.intersystems.com
path that does. It carries parameters with their default values, which `iris_doc_search`
does not return. Use `iris_doc_search` first to find the class name, then Documatic for
the details.

---

## Known gotchas (verified against IRIS 2025.1)

- `$SYSTEM.Security.Check(resource, permission)` — checks CURRENT user. Method name
  is `Check`, NOT `CheckPermission()` or `HasPermission()` (neither exists)
- `%SYSTEM.Security.CheckUserPermission(username, resource, permission)` — checks
  ANOTHER user; requires `%Admin_Secure:USE`
- `Ens.*` classes are in `ENSLIB` library, NOT `%SYS`
- `Ens.Director.GetAutoStart()` — does NOT exist; use `$GET(^Ens.AutoStart)` directly
- `SYS.Database` SQL table does NOT exist; use `##class(%ResultSet).%New("SYS.Database:List")`
- `$ZVERSION` (not `$VERSION`) is the full IRIS version string
- Documatic namespace `%SYS` in URLs must be `%25SYS` (URL-encoded percent sign)
