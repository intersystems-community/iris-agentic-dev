# iris-agentic-dev

MCP server that gives Codex tools for IRIS development — execute ObjectScript,
query globals, inspect productions, run tests, search code, manage skills, and more.

Written in Rust (2021 edition), two crates: `iris-agentic-dev-core` (tools + MCP server)
and `iris-agentic-dev-bin` (CLI entry point).

## Local dev container

`iris-dev-iris` on port 52780. Verify it's running before any IRIS-dependent work:

```bash
docker ps --filter name=iris-dev-iris
```

## Commands

```bash
cargo build                          # build
cargo clippy -- -D warnings          # lint (CI enforces clean)
cargo fmt --all                      # format (CI enforces clean)
cargo test                           # unit tests (no IRIS required)
cargo test -- --include-ignored      # full suite (requires live container)
```

For integration/e2e tests always use `--test-threads=1`:

```bash
cargo test --test '*' -- --test-threads=1 --include-ignored
```

## Testing Philosophy — NON-NEGOTIABLE

IRIS is the only valid test object.

- **Always use a live IRIS container for tests.** Never mock IRIS, mock the Atelier
  HTTP client, or stub IRIS responses in unit tests. Mocked IRIS tests lie — they
  pass when the real implementation is broken.
- **Coverage goals require `--include-ignored`** against a live container. Unit tests
  covering pure logic (parsers, guards, gates) are fine, but anything that touches
  IRIS behaviour must run against real IRIS.
- **`--test-threads=1`** is required for all IRIS integration/e2e test runs to prevent
  env-var race conditions across test binaries.

## Where to Look

| What                     | Where                                              |
| ------------------------ | -------------------------------------------------- |
| MCP tool implementations | `src/tools/` (one file per tool group)             |
| CLI entry point          | `src/bin/iris-agentic-dev.rs`                      |
| Connection + discovery   | `src/connection/`                                  |
| Skill system             | `src/skills/` + `skills/` (skill files)            |
| Error codes              | `src/errors.rs`                                    |
| Integration tests        | `tests/` (require `--include-ignored` + container) |

## Ecosystem

iad is the MCP hub. These packages integrate with it:

| Package                 | Role                                    | Integration pattern                                              |
| ----------------------- | --------------------------------------- | ---------------------------------------------------------------- |
| iris-devtester          | Container lifecycle                     | `connection_info().to_toml_snippet()` → `.iris-agentic-dev.toml` |
| iris-vector-graph       | Temporal graph + Cypher                 | `pip install iris-vector-graph[ai]` + ivg skills                 |
| iris-vector-rag-private | RAG pipeline                            | `pip install iris-vector-rag[ai]` + ivr skills                   |
| iris-pgwire             | PostgreSQL wire protocol                | `pip install iris-pgwire[ai]` + iris-pgwire skill                |
| iris-ai                 | AI Hub SDK + strategy                   | iris-ai skills/ covers full stack                                |
| iris-ai-examples        | CareConnect + KG Ticket Resolver demos  | Reference AGENTS.md for full-stack patterns                      |
| hipporag2-pipeline      | Multi-hop RAG via PPR                   | `pip install hipporag2-pipeline[ai]`                             |
| ai-hub-eap              | IRIS AI Hub EAP docs + ObjectScript SDK | aihub-eap skill, NoPWS containers                                |

For ecosystem integration patterns see `docs/ecosystem-integration.md`.

## Docs

- `docs/connecting.md` — connection config (toml file, env vars)
- `docs/tools.md` — tool reference
- `docs/skills.md` — skill system
- `docs/troubleshooting.md` — common issues
- `docs/ecosystem-integration.md` — how to integrate your package with iad
