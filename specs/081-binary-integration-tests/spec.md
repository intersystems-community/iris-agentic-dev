# 081 — Binary integration tests for CLI flag wiring and config round-trips

## Problem

Three bugs shipped in v1.2.0 that unit tests and live-IRIS integration tests both missed:

| Bug                                                               | Root cause                                                            | Why missed                                                                                        |
| ----------------------------------------------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `--config` ignored for `enabled_tools`/`disabled_tools` (#111)    | `self.config` never passed to workspace loader in `mcp.rs`            | No test launches the binary with `--config <path>` and inspects `tools/list`                      |
| `write_tools_enabled` in `.toml` has no effect (#110)             | Field absent from `WorkspaceConfig` struct, silently dropped by serde | TOML round-trip tests used struct literals, not TOML strings; no test checked the env var was set |
| `plugin.json` version not checked (#test_plugin_manifest_version) | Nobody added it to version-bump tracking                              | The `serverVersion.test.cjs` test covered Cargo/package.json but not `plugin.json`                |

The pattern: **CLI flag wiring and TOML-to-behavior round-trips are untestable at the
unit level** because they live in `async fn run()` in `mcp.rs`, which requires a real
binary process. Our test suite had nothing between "unit test of internal logic" and
"live IRIS integration test" for these paths.

A second pattern: **version-consistency contracts** spread across multiple files are
only enforced if someone writes an explicit cross-file check.

## What we need

### Layer 1 — Binary invocation tests (offline, no IRIS)

A test binary that spawns `iris-agentic-dev mcp --config <file>` over stdio, sends
a JSON-RPC `initialize` + `tools/list` request, and asserts on the response. No live
IRIS required — `IRIS_UNREACHABLE` responses are fine for these tests.

Covers:

1. `--config <path>` applies `enabled_tools` from the file → `tools/list` returns only
   those tools.
2. `--config <path>` applies `disabled_tools` → named tools absent from `tools/list`.
3. `--config <path>` with `write_tools_enabled = false` → `check_config` reports
   `write_tools_enabled: false`.
4. `--config <path>` with `destructive_tools_enabled = true` +
   `write_tools_enabled = false` → server exits with non-zero status (DESTRUCTIVE_REQUIRES_WRITES).
5. `--workspace <dir>` with `.iris-agentic-dev.toml` in that dir → same `enabled_tools`
   behavior as `--config` (regression guard for workspace discovery path).
6. No config file → `tools/list` returns the default merged toolset (~90 tools).

### Layer 2 — TOML round-trip tests

Parse a TOML string into `WorkspaceConfig` and verify the resulting env vars and
connection shape. Catches missing struct fields before a binary invocation is needed.

Covers (inline, no network):

1. `write_tools_enabled = false` in TOML → `IRIS_WRITE_TOOLS_ENABLED=0` in env.
2. `write_tools_enabled = true` in TOML → `IRIS_WRITE_TOOLS_ENABLED=1` in env.
3. `enabled_tools = ["iris_query"]` in TOML via `toml::from_str` → struct field
   populated (not silently dropped).
4. Unknown keys in TOML → no panic, warning in log (regression guard for serde
   silent-drop pattern that caused #89, #108, #110, #111).

**Note:** Layer 2 tests (6 of them) were added as part of the #110/#111 fix commit
(`98e0531`). This spec tracks the remaining Layer 1 binary invocation tests.

### Layer 3 — Version consistency tests

A single test file that asserts all version-bearing files agree with the workspace
`Cargo.toml` version:

- `vscode-iris-agentic-dev/package.json` → `irisAgenticDev.serverVersion`
- `.claude-plugin/plugin.json` → `version`
- (Already covered: `serverVersion.test.cjs` checks package.json vs Cargo.toml)

**Note:** `test_plugin_manifest_version` for `plugin.json` already exists in
`crates/iris-agentic-dev-bin/tests/unit/`. The gap is that it wasn't run in the
standard `cargo test` suite — it requires `--test test_plugin_manifest_version`.
This spec's Layer 3 work is to ensure it runs in CI automatically (it does via
`cargo test --tests` in coverage.sh, but NOT in the standard `ci.yml` test job).

## Implementation

### Binary invocation harness

Use `std::process::Command` to spawn the installed or freshly-built binary:

```rust
// tests/integration/test_mcp_binary_config.rs
#[cfg(test)]
#[ignore] // requires built binary — run with --include-ignored
mod binary_config_tests {
    fn iad_binary() -> std::path::PathBuf {
        // Use the test-built binary from target/debug/
        std::env::var("IAD_BINARY")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/debug/iris-agentic-dev")
            })
    }

    fn send_mcp(child: &mut std::process::Child, msg: &str) -> String {
        // Write JSON-RPC over stdin, read response from stdout
        // (stdio MCP transport)
        ...
    }

    #[test]
    fn config_file_enabled_tools_limits_tools_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".iris-agentic-dev.toml"),
            r#"enabled_tools = ["check_config", "iris_query"]"#).unwrap();
        let mut child = std::process::Command::new(iad_binary())
            .args(["mcp", "--config", &dir.path().join(".iris-agentic-dev.toml")
                .to_string_lossy()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn().unwrap();
        // initialize + tools/list
        let tools = send_mcp(&mut child, ...);
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t["name"] == "check_config"));
        assert!(tools.iter().any(|t| t["name"] == "iris_query"));
        child.kill().ok();
    }
    // ... more tests
}
```

The harness needs a thin JSON-RPC over stdio helper — ~50 lines. The tests themselves
are each ~20 lines.

### CI wiring

The binary invocation tests need the binary to exist. CI should build the binary
before running these tests. Add a step to `ci.yml`:

```yaml
- name: Build binary for integration tests
  run: cargo build -p iris-agentic-dev

- name: Run binary integration tests
  run: |
    IAD_BINARY=./target/debug/iris-agentic-dev \
    cargo test --test test_mcp_binary_config -- --include-ignored --test-threads=1
```

### test_plugin_manifest_version in CI

The existing test already works — it just needs to be in the `ci.yml` test step.
The current test step runs `cargo test` without `--tests`, so external test binaries
are excluded. Fix: change the CI test step to run `cargo test --tests` or add an
explicit `--test test_plugin_manifest_version` step.

## Out of scope

- Testing `tools/call` behavior (that's live-IRIS territory).
- Testing hot-reload (the `ConfigWatcher` path) — covered by existing tests.
- HTTP transport (`--transport http`) — separate concern.

## Success criteria

1. `test_mcp_binary_config.rs` has ≥ 6 tests covering the Layer 1 scenarios above,
   all passing with `IAD_BINARY=./target/debug/iris-agentic-dev --include-ignored`.
2. CI builds the binary and runs binary config tests on every push.
3. `test_plugin_manifest_version` runs in standard CI (not just coverage.sh).
4. A `--config`/`--workspace` regression takes < 5 minutes to surface in CI
   rather than reaching users.

## Why this matters

The bugs caught by Claudio Devecchi Junior (#110, #111) were operator-facing security
controls — write protection and tool allowlists. A user hardening a shared instance
would believe the controls took effect. Binary invocation tests are the only layer that
can catch "CLI flag exists but was never wired up" — no amount of unit testing will
find it because the unit under test (the flag handler) was never called.
