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
cargo test --features testing        # unit tests (no IRIS required)
cargo test --features testing -- --include-ignored   # full suite (requires live container)
```

Always pass `--features testing`. Every aggregate test target declares
`required-features = ["testing"]`, and cargo skips them silently without it — a bare `cargo test`
reports green while never compiling anything. CI passes the flag on every job.

For integration/e2e tests always use `--test-threads=1`:

```bash
cargo test --features testing --test '*' -- --test-threads=1 --include-ignored
```

### Test target layout

Both crates declare a handful of `[[test]]` targets, not one per file. Each is an aggregator
(`tests/<dir>/main.rs`) whose only content is `mod` lines:

| Crate  | Target            | Files | What it holds                                      |
| ------ | ----------------- | ----- | -------------------------------------------------- |
| `core` | `unit`            | 100   | Pure logic — parsers, guards, gates, contracts     |
| `core` | `integration`     | 54    | Live IRIS via `iris-dev-iris`; must stay serial    |
| `core` | `binary`          | 14    | Spawn `iris-agentic-dev`, talk JSON-RPC over stdio |
| `core` | `misc`            | 15    | Older top-level `tests/*.rs`                       |
| `core` | `skills`          | 1     | Single file, so it stays its own target            |
| `bin`  | `bin_unit`        | 10    | CLI arg parsing, config resolution                 |
| `bin`  | `bin_integration` | 14    | Spawned-binary and live-IRIS CLI paths             |
| `bin`  | `bin_misc`        | 2     | Older top-level `tests/*.rs`                       |

Cargo runs test binaries strictly one after another and gives you no knob to change that, so a
per-file target charges every run a process spawn. At 233 targets across the two crates that was
~85% of a warm run: 254 s, of which about 40 s was actually running tests. The eight aggregates
bring the same suite in at ~65 s.

Almost all of that came from the core crate. Aggregating the bin crate's 26 targets cut its own
CPU time from 26.8 s to 11.6 s but moved the workspace wall clock barely at all, because the core
build dominates. It is here for the guard coverage and the consistency, not for the clock.

Two consequences:

- **Add a file, add its `mod` line.** An unlisted file compiles nowhere and its tests never run,
  and `cargo test` still reports ok. `test_test_target_layout.rs` fails when that happens — do not
  delete it.
- **Do not add a per-file `[[test]]` block.** `autotests = false` is set, so a file declared as its
  own target _and_ listed in an aggregator compiles twice. The same guard test catches it.

`unit` touches no IRIS and no shared env, so it can run parallel — 5.7 s against 18.6 s serial:

```bash
RUST_TEST_THREADS=12 cargo test --features testing --test unit
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
  env-var race conditions. This matters more since the files were aggregated into one
  binary per group: tests in the same target share a process, so a `set_var` in one is
  visible to the next.

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

## Issue Closure — NON-NEGOTIABLE

Comment on a fixed issue saying what changed. **Never close it.** Closing is the reporter's
move — they filed it, so they are the one who can confirm the thing they hit stopped
happening. If nobody answers within a week of the release carrying the fix, close it then.
Constitution → Development Workflow → Issue Closure.

## Docs

- `docs/connecting.md` — connection config (toml file, env vars)
- `docs/tools.md` — tool reference
- `docs/skills.md` — skill system
- `docs/troubleshooting.md` — common issues
- `docs/agent-attribution.md` — caller attribution, User-Agent marker, IRIS audit guide

## Active Technologies

- Rust 2021 + `rmcp` 3.1.3, `schemars` 1 (`#[schemars(extend(...))]` for enums), `serde` — no new dependency (113-typed-tool-schemas)

- Rust 2021 + `rmcp`, `tokio`, `serde`/`serde_json`/`toml`; config in `.iris-agentic-dev.toml`, no database (085-write-gate-integrity)

- Dockerfile (no specific version), Bash (GHA steps), Markdown + `gcr.io/distroless/static-debian12` (base image), `docker/build-push-action@v6`, `docker/metadata-action@v5` (068-windows-docker)
- GHCR (`ghcr.io/intersystems-community/iris-agentic-dev`) (068-windows-docker)
- TypeScript 5, Node.js (VS Code extension host runtime) + VS Code API (`vscode`), Node built-ins (`https`, `fs`, (069-vscode-binary-install)
- Two files in `context.globalStorageUri` (VersionMarker + ManagedBinary) (069-vscode-binary-install)

## Recent Changes

- 114-tool-surface-discovery: `tool --list` / `tool <name> --schema` / `--json` read the tool router with no IRIS connection (7,648 B for the whole surface against 106,658 B for MCP `tools/list`); `tool_catalogue()` + `summarize_description()` in `tools/mod.rs` are the one source both arms read; `prose-only-enum` repaired — it was at zero because `TOOL_DESC` lacked `re.S` (so `iris_admin`'s backslash-continued description was never parsed) and `FIELD_DECL` could not read `pub r#type:`; twelve dispatcher value sets now declared, checked against the handler's own match arms rather than against prose; plan at `specs/114-tool-surface-discovery/plan.md`
- 113-typed-tool-schemas: per-tool params structs replace `AnyParams` so all 81 tools advertise their properties/types/enums; `#[serde(deny_unknown_fields)]` everywhere, so all 81 emit `additionalProperties: false`; one `UNKNOWN_PARAMETER` validation site in `call_tool` after `gate_check`; plan at `specs/113-typed-tool-schemas/plan.md`
- 089-iris-perf-monitoring: new `iris_mirror_status` tool (`%SYSTEM.Mirror` classmethods in %SYS); `iris_database_list` extended with `size_mb`/`free_space_mb`/`max_size_mb`/`free_pct` from `%SYS.DatabaseQuery:FreeSpace`; `my_access`/`capability_matrix` roles decoded from `$LB` via `$LISTTOSTRING`; Server Manager path prefix double-slash fixed; plan at `specs/089-iris-perf-monitoring/plan.md`
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
