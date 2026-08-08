# Tasks 077 — HTTP Transport

## Phase 1: Tests (write first)

- [X] T1.1 Create `crates/iris-agentic-dev-bin/tests/integration/test_http_transport.rs`
- [X] T1.2 Write T-077-01: unit test — invalid transport value exits code 1
- [X] T1.3 Write T-077-02: integration test — HTTP server binds and accepts TCP (mark `#[ignore]`)
- [X] T1.4 Write T-077-03: integration test — initialize request returns 200 + serverInfo (mark `#[ignore]`)
- [X] T1.5 Write T-077-04: integration test — --bind flag restricts address (mark `#[ignore]`, skips if 127.0.0.2 unavailable)
- [X] T1.6 Confirm T-077-01 fails before implementation; T-077-02..04 compile

## Phase 2: Cargo Feature

- [X] T2.1 Add `transport-streamable-http-server` to `rmcp` features in workspace `Cargo.toml`
- [X] T2.2 Add `hyper`, `hyper-util`, `tower-service` to bin crate dependencies
- [X] T2.3 Run `cargo build` — verify it compiles

## Phase 3: Implement HTTP Transport

- [X] T3.1 Add `--bind <addr>` flag to `McpCommand` struct in `mcp.rs` (default `"127.0.0.1"`)
- [X] T3.2 Add match on `self.transport` — invalid value exits 1 with message
- [X] T3.3 Implement `run_http_transport()` — bind TcpListener, StreamableHttpService, hyper serve loop
- [X] T3.4 Run `cargo clippy -- -D warnings` — fixed io::Error::other
- [X] T3.5 Run `cargo fmt --all -- --check` — clean

## Phase 4: Run Unit Test

- [X] T4.1 T-077-01 passes

## Phase 5: Run Integration Tests

- [X] T5.1 T-077-02 through T-077-04 pass (all 4 tests pass; T-077-04 correctly skips on macOS)

## Phase 6: E2E Test (manual / scripted)

- [X] T6.1 Start `iris-agentic-dev mcp --transport http --port 18765 --bind 0.0.0.0` locally
- [X] T6.2 In aihub-iris-116, created `IAD.ToolSet.IrisAgenticDevRemote` with `<Remote URL="http://host.docker.internal:18765/mcp"/>`, `%Discover()` returned mcp toolref; `check_config` via HTTP returned `connected:true` with real IRIS data. Fixed two issues: (1) `--bind 0.0.0.0` disables Host allowlist so container can reach host; (2) factory uses Clone not take() to support multiple sessions.

## Phase 7: Coverage

- [ ] T7.1 Run `cargo llvm-cov --features testing -- --include-ignored --test-threads=1`
- [ ] T7.2 Verify coverage on `cmd/mcp.rs` ≥ 90%

## Phase 8: Documentation

- [X] T8.1 Added "HTTP transport" section to `docs/connecting.md`
- [X] T8.2 `markdownlint-cli2 --fix docs/connecting.md && prettier --write docs/connecting.md` — clean

## Phase 9: Commit

- [ ] T9.1 `git add Cargo.toml crates/ docs/connecting.md`
- [ ] T9.2 Commit: `feat: add HTTP streamable transport (--transport http --port N --bind addr)`
