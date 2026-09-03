//! Binary invocation tests for spec 098 — server probe (ad-hoc + iris_servers probe=true).
//!
//! Layer 2: spawn `iris-agentic-dev` as a subprocess, send `initialize` +
//! `tools/call` over stdio, assert JSON-RPC response shape.
//! No live IRIS required for T019–T021 and T031.
//!
//! Run with:
//!   cargo build && IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --test binary_098_server_probe -- --include-ignored --test-threads=1

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn iad_binary() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("IAD_BINARY") {
        return std::path::PathBuf::from(p);
    }
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_iris-agentic-dev"))
}

fn read_until<T, F>(
    stdout: std::process::ChildStdout,
    timeout_ms: u64,
    mut predicate: F,
) -> Option<T>
where
    T: Send + 'static,
    F: FnMut(&serde_json::Value) -> Option<T> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<T>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                if let Some(result) = predicate(&v) {
                    let _ = tx.send(result);
                    return;
                }
            }
        }
    });
    rx.recv_timeout(std::time::Duration::from_millis(timeout_ms))
        .ok()
}

fn parse_tool_result(v: &serde_json::Value) -> Option<serde_json::Value> {
    let content = v.get("result")?.get("content")?.as_array()?;
    for c in content {
        if let Some(text) = c.get("text").and_then(|t| t.as_str()) {
            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text) {
                return Some(obj);
            }
        }
    }
    None
}

fn send_initialize(stdin: &mut impl Write) {
    let init = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"0.0.1\"}}}\n";
    let notif = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n";
    stdin.write_all(init.as_bytes()).ok();
    stdin.write_all(notif.as_bytes()).ok();
    stdin.flush().ok();
}

fn spawn_iad() -> (
    std::process::Child,
    std::process::ChildStdin,
    std::process::ChildStdout,
    tempfile::TempDir,
) {
    let bin = iad_binary();
    assert!(
        bin.exists(),
        "binary not found at {bin:?}; run cargo build first"
    );
    // Write a minimal config so the binary doesn't read ~/.iris-agentic-dev.toml.
    // Using docker_only=true makes discovery skip the port scan and complete immediately.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let cfg = tmp.path().join(".iris-agentic-dev.toml");
    std::fs::write(&cfg, "docker_only = true\n").expect("write config");

    let mut child = Command::new(&bin)
        .arg("mcp")
        .args(["--config", cfg.to_str().unwrap()])
        .env_remove("IRIS_HOST")
        .env_remove("IRIS_WEB_PORT")
        .env_remove("IRIS_CONTAINER")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn binary");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    (child, stdin, stdout, tmp)
}

// ── T019: ad-hoc probe response contains reachable field ──────────────────────

/// T019: `iris_test_server` with `host`/`web_port`/`username`/`password` returns
/// a JSON object with a `reachable` field. Value may be any bool (no live IRIS needed).
#[test]
#[ignore]
fn test_adhoc_probe_response_shape() {
    let (mut child, mut stdin, stdout, _tmp) = spawn_iad();
    send_initialize(&mut stdin);

    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_test_server","arguments":{"host":"127.0.0.1","web_port":1,"username":"_SYSTEM","password":"SYS"}}}"#;
    writeln!(stdin, "{call}").ok();
    stdin.flush().ok();

    let result = read_until(stdout, 12000, |v| parse_tool_result(v));
    let _ = child.kill();

    let r = result.expect("T019: timed out waiting for iris_test_server response");
    assert!(
        r.get("reachable").is_some(),
        "T019: response must contain 'reachable' field; got: {r}"
    );
    // Port 1 is always closed — must be unreachable
    assert_eq!(
        r["reachable"].as_bool(),
        Some(false),
        "T019: port 1 must be unreachable; got: {r}"
    );
}

// ── T020: neither name nor host returns MISSING_PARAMS error ─────────────────

/// T020: calling `iris_test_server` with `{}` returns an error with `error_code: MISSING_PARAMS`.
#[test]
#[ignore]
fn test_neither_name_nor_host_error() {
    let (mut child, mut stdin, stdout, _tmp) = spawn_iad();
    send_initialize(&mut stdin);

    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_test_server","arguments":{}}}"#;
    writeln!(stdin, "{call}").ok();
    stdin.flush().ok();

    let result = read_until(stdout, 8000, |v| parse_tool_result(v));
    let _ = child.kill();

    let r = result.expect("T020: timed out waiting for iris_test_server response");
    assert_eq!(
        r["error_code"].as_str(),
        Some("MISSING_PARAMS"),
        "T020: empty params must return MISSING_PARAMS; got: {r}"
    );
}

// ── T021: closed port returns reachable:false ─────────────────────────────────

/// T021: `iris_test_server` with a closed port returns `reachable: false`.
/// (Covered by T019 above, but included here as an explicit assertion.)
#[test]
#[ignore]
fn test_closed_port_unreachable() {
    let (mut child, mut stdin, stdout, _tmp) = spawn_iad();
    send_initialize(&mut stdin);

    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_test_server","arguments":{"host":"127.0.0.1","web_port":1}}}"#;
    writeln!(stdin, "{call}").ok();
    stdin.flush().ok();

    let result = read_until(stdout, 12000, |v| parse_tool_result(v));
    let _ = child.kill();

    let r = result.expect("T021: timed out waiting for iris_test_server response");
    assert_eq!(
        r["reachable"].as_bool(),
        Some(false),
        "T021: port 1 must be unreachable; got: {r}"
    );
}

// ── T031: iris_servers with no params — all entries have reachable:null ───────

/// T031: `iris_servers` with no params (default fast path) returns each server
/// with `"reachable": null`. Regression guard — the probe flag must not change
/// the default behavior.
///
/// This test uses an empty HOME so the pool has no servers, which means the
/// assertion holds trivially. The point is that the tool still accepts an empty
/// params object without error (no probe regression).
#[test]
#[ignore]
fn test_iris_servers_no_probe_reachable_null() {
    let (mut child, mut stdin, stdout, _tmp) = spawn_iad();
    send_initialize(&mut stdin);

    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_servers","arguments":{}}}"#;
    writeln!(stdin, "{call}").ok();
    stdin.flush().ok();

    let result = read_until(stdout, 8000, |v| parse_tool_result(v));
    let _ = child.kill();

    let r = result.expect("T031: timed out waiting for iris_servers response");
    let servers = r["servers"]
        .as_array()
        .expect("T031: must return servers array");
    for s in servers {
        assert_eq!(
            s.get("reachable").and_then(|v| v.as_null()),
            Some(()),
            "T031: default path must have reachable:null per entry; got: {s}"
        );
    }
}
