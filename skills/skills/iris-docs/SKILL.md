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

**Important**: Documatic pages are also SPA content — use a browser UA with curl, but
prefer `iris_doc_search` unless you know the exact class name.

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
