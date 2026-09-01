# iris-agentic-dev

MCP server that gives Claude Code tools for IRIS development — execute ObjectScript,
query globals, inspect productions, run tests, search code, manage skills, and more.

Written in Rust (2021 edition), two crates: `iris-agentic-dev-core` (tools + MCP server)
and `iris-agentic-dev-bin` (CLI entry point).

## Local dev container

| Container       | TCP port | Web port | Image                   | Atelier REST | WebGateway      |
| --------------- | -------- | -------- | ----------------------- | ------------ | --------------- |
| `iris-dev-iris` | 11975    | 52780    | `iris-community:2026.2` | yes (52780)  | none — PWS only |

**NoPWS note:** Community 2026.2 has PWS on 52780. Enterprise 2026.2.0AI builds do NOT
(DPP-1192) — `atelier_rest=false`, use `docker_only=true` for those.

Verify running before any IRIS-dependent work:

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

## Test Coverage Policy — NON-NEGOTIABLE

Every new feature, tool, CLI flag, config field, and skill must have tests at the
right layer before the PR is considered done. "It compiles" is not enough.

**Three required layers:**

1. **Unit / TOML round-trip** — parse the config string (not a struct literal) and
   assert the resulting struct fields and env vars are correct. Catches serde silent-drop
   (the #110 pattern: field missing from struct, TOML key silently ignored).

2. **Binary invocation** (for any CLI flag or `mcp.rs` wiring) — spawn
   `iris-agentic-dev` as a subprocess, send `initialize` + `tools/list` or
   `tools/call` over stdio, assert on the JSON-RPC response. No live IRIS needed.
   Catches "flag exists but was never wired" (the #111 pattern: `self.config` ignored).
   Use `IAD_BINARY=./target/debug/iris-agentic-dev` and `#[ignore]`; CI builds the
   binary first and passes the env var.

3. **Live IRIS integration** (for any tool that calls IRIS) — `#[ignore]` test against
   `iris-dev-iris` (localhost:52780). Covers actual IRIS behavior, not just wiring.

**Version consistency:** every file that must agree with the workspace version
(`Cargo.toml`, `package.json`, `.claude-plugin/plugin.json`, etc.) must have an
explicit cross-file assertion test. Adding a new version-bearing file without adding
a test for it is a bug waiting to ship.

**When in doubt:** ask "if I changed this flag/field/file silently, would any test
fail?" If the answer is no, the test is missing.

## Release Notes & Changelog — NON-NEGOTIABLE

Before closing any release (tagging, publishing, merging release branch):

1. Run `/no-ai-slop` on all release notes and changelog entries.
2. Address every flagged item before publishing.
3. Release notes must read like a human wrote them for other humans — no filler phrases,
   no hedging, no passive voice, no "This release includes…" boilerplate.

## Docs

- `docs/connecting.md` — connection config (toml file, env vars)
- `docs/tools.md` — tool reference
- `docs/skills.md` — skill system
- `docs/troubleshooting.md` — common issues
- `docs/agent-attribution.md` — caller attribution, User-Agent marker, IRIS audit guide

## Active Technologies

- Rust 2021 + `rmcp`, `tokio`, `serde`/`serde_json`/`toml`; config in `.iris-agentic-dev.toml`, no database (085-write-gate-integrity)

- Dockerfile (no specific version), Bash (GHA steps), Markdown + `gcr.io/distroless/static-debian12` (base image), `docker/build-push-action@v6`, `docker/metadata-action@v5` (068-windows-docker)
- GHCR (`ghcr.io/intersystems-community/iris-agentic-dev`) (068-windows-docker)
- TypeScript 5, Node.js (VS Code extension host runtime) + VS Code API (`vscode`), Node built-ins (`https`, `fs`, (069-vscode-binary-install)
- Two files in `context.globalStorageUri` (VersionMarker + ManagedBinary) (069-vscode-binary-install)

## Recent Changes

- 089-iris-perf-monitoring: new `iris_mirror_status` tool (`%SYSTEM.Mirror` classmethods in %SYS); `iris_database_list` extended with `size_mb`/`free_space_mb`/`max_size_mb`/`free_pct` from `%SYS.DatabaseQuery:FreeSpace`; plan at `specs/089-iris-perf-monitoring/plan.md`
- 088-windows-vscdb-credential-fallback: `resolve_credential` on Windows now falls back to `state.vscdb` (safeStorage / AES-256-GCM) when Windows Credential Manager has no entry; `vscode_payload.rs` moved to core; DPAPI error message includes current Windows username; `check-sm-credential` delegates to core
- 087-execute-gate-bypass: `iris_execute` now enforces the destructive gate when `Kill ^<global>` appears literally in the code string; `contains_global_kill` in `write_gate.rs`; 22 unit tests + 4 live IRIS tests; indirection gap documented in spec and error message
- 086-agent-attribution-audit: caller marker in `User-Agent` on every IRIS-bound request, opt-in `%SYS.Audit` emission via `[policy.<server>].irisAudit`, `docs/agent-attribution.md`; plan at `specs/086-agent-attribution-audit/plan.md`
- 085-write-gate-integrity: write/destructive gates resolved as data and enforced once in `call_tool`; plan at `specs/085-write-gate-integrity/plan.md`
- 068-windows-docker: Added Dockerfile (no specific version), Bash (GHA steps), Markdown + `gcr.io/distroless/static-debian12` (base image), `docker/build-push-action@v6`, `docker/metadata-action@v5`

<!-- codebase-memory-mcp: Code Discovery Protocol -->

## Code Discovery Protocol (codebase-memory-mcp)

**ALWAYS use `codebase-memory-mcp` tools FIRST for any code exploration:**

- `search_graph(name_pattern/label/qn_pattern)` — find functions, classes, routes
- `trace_path(function_name, mode=calls|data_flow|cross_service)` — call chains
- `get_code_snippet(qualified_name)` — exact symbol source with precise line ranges
- `query_graph(query)` — complex Cypher patterns across the codebase graph
- `get_architecture(aspects)` — project structure overview
- `search_code(pattern)` — graph-augmented text search

Use `Grep`/`Glob`/`Read` freely for text, configs, and non-code files, and always
`Read` a file before editing it. If the project is not indexed yet, run
`index_repository` first.
