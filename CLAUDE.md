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

## Docs

- `docs/connecting.md` — connection config (toml file, env vars)
- `docs/tools.md` — tool reference
- `docs/skills.md` — skill system
- `docs/troubleshooting.md` — common issues
