# Spec 065 — iris_doc_search Tool + iris-docs Skill Upgrade

## Summary

Add an `iris_doc_search` MCP tool that queries the InterSystems documentation site via
its embedded Algolia search API, returning ranked hits with title, URL, and content
excerpt. Upgrade the `iris-docs` skill to document both the new tool and the Algolia
recipe directly. Validate the full stack with a DOC benchmark category covering five
IRIS topic clusters.

## Motivation

Today there is no tool for documentation discovery. The only docs-adjacent tools are
`docs_introspect` (reads a running container, not the docs site) and `iris_doc`
(reads/writes IRIS documents). When an agent needs to answer "what are all the ways to
make a SQL request in IRIS" it has no reliable path — `WebFetch` returns 403 or a
JavaScript SPA shell. The Algolia recipe exists in `~/.claude/skills/intersystems-docs`
but is not a tool call, requiring the agent to know to load the skill and then emit a
raw shell command.

**Target query**: "Find all the different ways to make a SQL request in IRIS and show me
the official documentation" — this should be answerable in ≤ 3 tool calls with accurate
results from the real docs.

## Background — The Algolia recipe

`docs.intersystems.com` is a JavaScript SPA. Two failure modes for naive fetching:

1. **HTTP 403** — default User-Agent is blocked.
2. **Nav shell only** — even with a browser UA at HTTP 200, `DocBook.UI.Page.cls`
   pages render content client-side; curl/WebFetch gets only the TOC skeleton.

**The working path:** Algolia search. The docs site embeds public search-only
credentials in every page's `<meta class="algolia">` tags. This is explicitly
permitted (`robots.txt`: `Content-Signal: search=yes`).

Current public credentials (re-scrape meta tags if they rotate):

- AppID: `EP91R43SFK`
- Index: `docs`
- SearchKey: `709759d92d99a5cf927e90c965741389`

Each hit returns:

- `title` — page or class name
- `URL` — canonical docs.intersystems.com URL
- `text` — full rendered page content excerpt (the real docs body, not nav)
- `breadcrumbs` — section path (e.g. "InterSystems IRIS > SQL > Dynamic SQL")
- `version`, `product` — facet values for filtering

Useful facet filters: `product:InterSystems IRIS`, `version:2025.1`.

The recipe was verified across multiple projects (opsreview, integratedml) and is the
sole reliable path to docs.intersystems.com content at query time.

## Spec

### Tool: `iris_doc_search`

**Category**: Docs (new category in `tools/mod.rs` dispatch)

**Parameters**:

```rust
pub struct IrisDocSearchParams {
    pub query: String,                  // natural-language or keyword query
    pub version: Option<String>,        // e.g. "2025.1"; None = no version filter
    pub product: Option<String>,        // e.g. "InterSystems IRIS"; None = no product filter
    pub hits: Option<u8>,              // max results, default 5, max 10
}
```

**Behavior**:

1. Build Algolia request body: `{"query": ..., "hitsPerPage": ..., "facetFilters": [...],
"attributesToRetrieve": ["title","URL","text","breadcrumbs","version","product"]}`
2. POST to `https://EP91R43SFK-dsn.algolia.net/1/indexes/docs/query` with headers:
   `X-Algolia-Application-Id: EP91R43SFK` and `X-Algolia-API-Key: <key>`
3. Return structured hits array. Each hit:

```json
{
  "title": "Dynamic SQL",
  "url": "https://docs.intersystems.com/iris20251/...",
  "excerpt": "...(first ~600 chars of text field)...",
  "breadcrumbs": "InterSystems IRIS > SQL > Dynamic SQL",
  "version": "2025.1",
  "product": "InterSystems IRIS"
}
```

1. On network error or non-200: return `{"error": "...", "hits": []}` — never panic.
2. On zero hits: return `{"hits": [], "query": "..."}` — not an error.

**Credentials storage**: Hard-coded constants in `tools/doc_search.rs`. These are
public search-only keys embedded in every docs page (not secrets). Add a comment
pointing to the re-scrape procedure in case they rotate.

**HTTP client**: Reuse the existing `reqwest` client pattern from other tools. No new
dependency needed — `reqwest` is already in `Cargo.toml`.

**Tool name in manifest**: `iris_doc_search` — goes between `iris_doc` and
`iris_execute` alphabetically.

### Skill: `skills/skills/iris-docs` upgrade

The current `iris-docs` skill only covers the Documatic URL pattern (class reference
pages for known class names). It does not document Algolia and does not mention
`iris_doc_search`. Upgrade it to:

1. Lead with `iris_doc_search` as the primary lookup path.
2. Keep Documatic URL pattern as a fallback (still useful for direct class reference
   when you know the exact class name and version).
3. Add Algolia recipe section (for when you need to run the query manually or the tool
   is unavailable) — copy from `~/.claude/skills/intersystems-docs/SKILL.md`.
4. Add a "When to use which" decision table:

| Situation                                        | Use                                |
| ------------------------------------------------ | ---------------------------------- |
| "What are all the ways to do X in IRIS?"         | `iris_doc_search`                  |
| "What methods does `%SQL.Statement` have?"       | `iris_doc_search` or Documatic URL |
| "Does this method exist / what's its signature?" | `docs_introspect` (live container) |
| "Read/write a class document in IRIS"            | `iris_doc`                         |

### Benchmark: DOC category

New category `DOC` in `benchmark/021/`. Five tasks, each requiring the agent to use
`iris_doc_search` and produce a correct, factually-grounded answer. Judge scores on
answer quality (correct facts present), not just tool usage.

#### System prompts

**DOC baseline** — agent has no skill loaded, knows only the tools available:

```text
You are an IRIS developer. Use available MCP tools to answer documentation questions.
Complete the task efficiently.
```

**DOC merged** — agent has `iris-docs` skill loaded inline:

```text
You are an IRIS developer. The iris-docs skill is loaded below.
Use iris_doc_search to find authoritative answers from docs.intersystems.com.
--- SKILL: iris-docs ---
{skill content}
--- END SKILL ---
Complete the task efficiently.
```

Lift signal: does the skill meaningfully change which tool the agent reaches for
(baseline may try iris_execute or iris_query to enumerate classes; merged should go
straight to iris_doc_search) and does it improve answer completeness?

#### Tasks

**DOC-01** — SQL execution methods (the motivating query)

```yaml
id: DOC-01
category: DOC
path: both
description: >
  List all the different ways to execute SQL in IRIS ObjectScript. For each method,
  give the class or syntax name and one sentence on when to use it.
expected_behavior: >
  Answer names at least 4 distinct mechanisms: (1) embedded SQL (&sql()), (2)
  %SQL.Statement / %SQL.StatementResult, (3) %ResultSet with %SQL.Statement or legacy
  ##class(%ResultSet), (4) iris.dbapi (Python). Optionally mentions ODBC/JDBC and
  xDBC. Answer is grounded in documentation, not hallucinated. Agent uses
  iris_doc_search rather than guessing from training data.
tags: [docs, sql, discovery]
```

**DOC-02** — Interoperability adapter base classes

```yaml
id: DOC-02
category: DOC
path: both
description: >
  What base classes should a custom IRIS Interoperability Business Operation and its
  Outbound Adapter extend? Give the exact class names.
expected_behavior: >
  Correctly names Ens.BusinessOperation (or subclass) for the operation and
  Ens.OutboundAdapter (or a specific subclass like EnsLib.HTTP.OutboundAdapter) for
  the adapter. Does NOT hallucinate class names like Ens.Adapter or
  Ens.BusinessAdapter. Sources the answer from docs, not training data alone.
tags: [docs, interoperability, ensemble]
```

**DOC-03** — Security: checking permissions at runtime

```yaml
id: DOC-03
category: DOC
path: both
description: >
  In ObjectScript, what is the correct way to check whether the current user has a
  specific resource:permission at runtime? Give the exact class method call.
expected_behavior: >
  Correctly identifies $SYSTEM.Security.Check(resource, permission) or
  ##class(%SYSTEM.Security).Check(resource, permission). Does NOT hallucinate
  CheckPermission() or HasPermission() which do not exist. Includes the resource
  format (e.g. %Admin_Secure:USE). Sources answer from docs.intersystems.com.
tags: [docs, security, permissions]
```

**DOC-04** — ObjectScript $JOB and background jobs

```yaml
id: DOC-04
category: DOC
path: both
description: >
  How do you start a background job in ObjectScript and get its job number? What
  special variable gives you the current process's job number?
expected_behavior: >
  Correctly identifies the JOB command to start background processes, $JOB as the
  special variable for current process job number, and that JOB returns the child
  process number in the variable after the colon (JOB ##class(Foo).Bar():'jobnum').
  May mention %SYSTEM.Process or ^$JOB for process management. Factually correct.
tags: [docs, objectscript, jobs, performance]
```

**DOC-05** — $ZVERSION / build info special variable

```yaml
id: DOC-05
category: DOC
path: both
description: >
  What ObjectScript expression gives you the full IRIS version string (e.g.
  "IRIS for UNIX (Apple M1) 2025.1.0")? What about just the numeric version?
expected_behavior: >
  Correctly identifies $ZV or $ZVERSION as the full version string special variable.
  For numeric-only version, correctly identifies $SYSTEM.Version.GetNumber() or
  similar %SYSTEM.Version class method, or notes $ZVERSION parsing. Does not
  hallucinate $VERSION or ##class(IRIS).Version().
tags: [docs, objectscript, builtins, version]
```

## What "lift" means for DOC tasks

The baseline agent will often answer from training data — Claude knows a lot about
IRIS. Lift here measures two things:

1. **Accuracy on the hard cases** — hallucinated method names ($ZVersion vs $ZVERSION,
   CheckPermission vs Check). Skill+tool should push these to 3; baseline may score 1.
2. **Grounding signal** — did the agent actually retrieve docs vs answer from memory?
   Judge should penalize ungrounded answers on tasks where the expected_behavior calls
   out specific method names (DOC-03, DOC-05 especially).

Judge `category_note` for DOC:

```text
This is a documentation retrieval task. Penalize answers that give incorrect method
names, class names, or syntax even if partially correct. Score 3 only if the key
facts in expected_behavior are all present and correct. Score 2 if mostly correct
but agent used more than 2 tool calls. Score 1 if partially correct (right concept,
wrong detail). Score 0 if hallucinated or missing the core answer.
```

## Out of scope

- Caching Algolia results (stateless is fine for now)
- Streaming response (excerpt truncation at 600 chars is sufficient)
- Indexing the documatic (class reference) index separately — Algolia covers both
  docbook and documatic pages in the same `docs` index
- Updating the global `~/.claude/skills/intersystems-docs` skill (that's a user-level
  file outside this repo; the repo skill `skills/skills/iris-docs` is the deliverable)

## Acceptance criteria

1. `iris_doc_search(query="SQL execution methods")` returns ≥ 3 hits with non-empty
   excerpts from docs.intersystems.com.
2. `iris_doc_search(query="nonexistent xyzzy frobnicator 99999")` returns `{"hits":
[], ...}` without error.
3. `iris_doc_search` with `version="2025.1"` returns only hits with that version in
   their metadata.
4. Tool appears in `iris-dev tool list` output between `iris_doc` and `iris_execute`.
5. Unit tests: request serialization, response deserialization, empty-hits path,
   credential constants present.
6. Integration test (live network, `#[ignore]`): real Algolia query returns hits.
7. DOC benchmark: merged condition scores ≥ 2.0 average across DOC-01 through DOC-05;
   lift vs baseline ≥ +0.20.
