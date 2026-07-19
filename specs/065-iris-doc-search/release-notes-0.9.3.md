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

### `check_config` capabilities block

`check_config` now returns a `capabilities` field derived from the active connection
with zero additional network calls:

```json
"capabilities": {
  "private_web_server": true,
  "atelier_rest": true,
  "compile_path": "atelier",
  "webgateway_url": null
}
```

`compile_path` is `"docker_exec"` for `docker_only = true` configs and for NoPWS
builds (IRIS 2026.2.0AI). `iris_compile` now reads this at call time and routes to
`docker exec` immediately — no port 52773 probe, no retry.

### Ecosystem skill ownership decentralized

`iris-vector-graph` and `iris-vector-rag` skills now live in their own repos and are
fetched from there. Three new ecosystem packages registered: `iris-pgwire`,
`iris-ai-examples`, and a stub for `iris-vector-graph` / `iris-vector-rag` pointing
to their respective repos.

A new `docs/ecosystem-integration.md` explains the three integration patterns
(connection handoff, skills contributor, downstream consumer) with a checklist.

### Docs: first-hurdle fixes

- `docs/connecting.md` — `check_config` callout moved to the top; `iris-agentic-dev init`
  surfaced for native IRIS (not just Docker); global config file paths documented
  (`~/.config/iris-agentic-dev/config.toml` on Mac/Linux); `write_tools_enabled = false`
  explained as an enforceable gate for shared servers
- `docs/troubleshooting.md` — NoPWS section updated to reflect shipped auto-detection;
  stale "once implemented" language removed

## Fixes

None.

## Upgrade

```bash
brew upgrade iris-agentic-dev
```
