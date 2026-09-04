//! Binary invocation tests for eval session identity CLI wiring (Phase 4).
//!
//! These tests launch `iris-agentic-dev` as a subprocess and verify that:
//!   1. `capability-matrix --help` exits 0 (subcommand is wired)
//!   2. `telemetry export --help` exits 0 (subcommand is wired)
//!   3. `tool iris_info --envelope` produces the expected JSON envelope shape
//!
//! No live IRIS required for --help tests.
//! The `--envelope` test requires IRIS — tagged `#[ignore]`.
//!
//! Run with:
//!   IAD_BINARY=./target/debug/iris-agentic-dev \
//!     cargo test --test test_eval_session_binary -- --include-ignored --test-threads=1

use std::process::Command;

// A relative `IAD_BINARY` — the form the doc comment above tells you to pass — used to be handed to
// `Command::new` as-is and resolved against the process working directory, which for a workspace
// member's test binary is the *member* directory. `iad_binary_path` resolves relative values against
// the workspace root.
fn binary() -> String {
    iris_agentic_dev_core::testing::iad_binary_path()
        .to_string_lossy()
        .into_owned()
}

#[test]
#[ignore]
fn capability_matrix_help_exits_zero() {
    let out = Command::new(binary())
        .args(["capability-matrix", "--help"])
        .output()
        .expect("failed to spawn binary");
    assert!(
        out.status.success(),
        "capability-matrix --help exited {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("capability") || stdout.contains("IRIS"),
        "Expected help text, got: {stdout}"
    );
}

#[test]
#[ignore]
fn telemetry_export_help_exits_zero() {
    let out = Command::new(binary())
        .args(["telemetry", "export", "--help"])
        .output()
        .expect("failed to spawn binary");
    assert!(
        out.status.success(),
        "telemetry export --help exited {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("run-id") || stdout.contains("format"),
        "Expected help text with --run-id/--format, got: {stdout}"
    );
}

/// Requires live IRIS at localhost:52773 and the binary to be built.
#[test]
#[ignore]
fn tool_envelope_produces_json_shape() {
    let out = Command::new(binary())
        .args(["tool", "iris_info", "--envelope"])
        .env("GAUNTLET_RUN_ID", "gauntlet-testabcd")
        .output()
        .expect("failed to spawn binary");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("envelope output must be valid JSON");

    assert!(v.get("ok").is_some(), "envelope must have 'ok' field");
    assert_eq!(
        v.get("tool").and_then(|v| v.as_str()),
        Some("iris_info"),
        "envelope 'tool' must be the tool name"
    );
    assert_eq!(
        v.get("run_id").and_then(|v| v.as_str()),
        Some("gauntlet-testabcd"),
        "envelope 'run_id' must come from GAUNTLET_RUN_ID"
    );
    assert!(
        v.get("elapsed_ms").and_then(|v| v.as_u64()).is_some(),
        "envelope must have numeric 'elapsed_ms'"
    );
    assert!(v.get("result").is_some(), "envelope must have 'result'");
    assert!(v.get("error").is_some(), "envelope must have 'error'");
}
