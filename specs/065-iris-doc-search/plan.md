# Implementation Plan: 065 — iris_doc_search

**Branch**: `065-iris-doc-search` | **Date**: 2026-07-18 | **Spec**: specs/065-iris-doc-search/spec.md

## Summary

Add `iris_doc_search` — an MCP tool that POSTs to the InterSystems docs Algolia API and
returns ranked hits with title, URL, content excerpt, and breadcrumbs. Upgrade the
`iris-docs` skill to lead with the new tool. Validate with a DOC benchmark category
(5 tasks, answer-quality scoring) with lift ≥ +0.20 required before merge.

## Technical Context

**Language/Version**: Rust 2021, workspace crate `iris-agentic-dev-core`
**Primary Dependencies**: `reqwest` (workspace, already present, features: json + rustls-tls), `serde_json` (workspace)
**Storage**: N/A — stateless HTTP tool
**Testing**: `cargo test` (unit, no IRIS needed), `#[ignore]` integration test (live network to Algolia)
**Target Platform**: macOS arm64/x86_64, Linux x86_64, Windows x86_64 (same as all tools)
**Performance Goals**: Single Algolia query, p95 < 2s (network-bound, not optimizable)
**Constraints**: No new crate deps (reqwest already workspace). Algolia creds hard-coded as constants; comment documents re-scrape procedure.

## Constitution Check

| Principle                     | Status | Notes                                                                                                                                                           |
| ----------------------------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I. Zero-Install               | ✅     | Pure HTTP call via existing reqwest. No new binary deps.                                                                                                        |
| II. ObjectScript Sanity       | ✅ N/A | No ObjectScript called. Tool hits Algolia, not IRIS.                                                                                                            |
| III. HTTP-First               | ✅     | Tool is outbound HTTP only; no Docker required. Goes in Merged tier.                                                                                            |
| IV. Test-First                | ✅     | Unit tests (request serialization, response parsing, empty hits, no-network path) written before implementation. Integration test `#[ignore]` for live Algolia. |
| V. Output Shape Parity        | ✅     | New tool, no existing variant. Shape documented in contracts/.                                                                                                  |
| VI. Environment Guard         | ✅ N/A | Read-only tool (fetches external docs). No write gate needed.                                                                                                   |
| VII. Dependency Minimalism    | ✅     | No new crates. `reqwest` with `json` feature already in workspace.                                                                                              |
| VIII. 90% Coverage Gate       | ✅     | Polish phase includes `cargo llvm-cov --include-ignored` ≥ 90% check.                                                                                           |
| IX. Tool Lift Requirement     | ✅     | DOC benchmark (5 tasks). Lift gate ≥ +0.20. Results in `lift-results.md`.                                                                                       |
| X. ObjectScript Coverage Gate | ✅ N/A | Pure Rust tool; no ObjectScript shipped.                                                                                                                        |

## Architecture

### New file: `crates/iris-agentic-dev-core/src/tools/doc_search.rs`

```rust
pub struct IrisDocSearchParams { query, version, product, hits }
pub async fn handle_iris_doc_search(params) -> serde_json::Value
fn build_request_body(params) -> serde_json::Value      // pure, testable
fn parse_response(json) -> Vec<DocHit>                  // pure, testable
struct DocHit { title, url, excerpt, breadcrumbs, version, product }
const ALGOLIA_APP_ID, ALGOLIA_SEARCH_KEY, ALGOLIA_INDEX, ALGOLIA_ENDPOINT
```

### Changes to `mod.rs`

1. Add `pub mod doc_search;`
2. Add `IrisDocSearchParams` dispatch in `call_tool` between `iris_doc` and `iris_execute`
3. Add `"iris_doc_search"` to `registered_tool_names()` `merged_added` list
4. Add `iris_doc_search` MCP tool descriptor (name, description, input_schema)
5. Add `iris_doc_search` async handler method on `IrisMcpTools`

### Changes to `iris-agentic-dev-bin/src/cmd/tool.rs`

Add `"iris_doc_search"` to `TOOL_NAMES` between `"iris_doc"` and `"iris_execute"`.

### Skill upgrade: `skills/skills/iris-docs/SKILL.md`

Rewrite to lead with `iris_doc_search`, add Algolia recipe fallback, add decision table.

### Benchmark additions

- `benchmark/021/tasks/DOC-01.yaml` through `DOC-05.yaml`
- `benchmark/021/runner/claude_code.py`: add `DOC_SYSTEM_BASELINE`, `DOC_SYSTEM_MERGED`, DOC routing
- `benchmark/021/runner/judge.py`: add `DOC_CATEGORY_NOTE`

## Phases

### Phase 1: Tool implementation + unit tests

- `doc_search.rs` with constants, params, pure builder/parser fns
- Unit tests: build_request_body (with/without filters), parse_response (normal, empty, error), params serialization
- `mod.rs` wiring: dispatch, registration, descriptor, handler
- `tool.rs` TOOL_NAMES entry

### Phase 2: Integration test + skill upgrade

- `#[ignore]` integration test against live Algolia (real network)
- `iris-docs` skill rewrite

### Phase 3: Benchmark + lift measurement

- DOC-01..DOC-05 task YAML files
- Runner/judge additions
- Run baseline + merged, record lift in `lift-results.md`

### Phase 4: Polish

- `cargo llvm-cov --include-ignored` ≥ 90%
- Release notes draft
- `cargo clippy --all-targets -- -D warnings` clean
- `cargo fmt --all`

## Algolia Endpoint Details

```text
POST https://EP91R43SFK-dsn.algolia.net/1/indexes/docs/query
Headers:
  X-Algolia-Application-Id: EP91R43SFK
  X-Algolia-API-Key: 709759d92d99a5cf927e90c965741389
  Content-Type: application/json

Body:
{
  "query": "<user query>",
  "hitsPerPage": 5,
  "facetFilters": ["product:InterSystems IRIS"],   // optional
  "attributesToRetrieve": ["title","URL","text","breadcrumbs","version","product"]
}

Response hits[]:
  objectID, title, URL, text, breadcrumbs (array or string), version, product
```

Re-scrape creds if key rotates:

```bash
UA="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
curl -sS -A "$UA" "https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=GCM_monitoring" \
  | grep -oiE 'ALG-[A-Za-z]+.*content="[^"]*"'
```
