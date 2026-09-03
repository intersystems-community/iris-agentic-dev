//! Binary invocation tests for spec 093 — TOML pool hot-reload.
//!
//! Layer 2: spawn `iris-agentic-dev` as a subprocess, send `initialize` +
//! `tools/call iris_reload_pool` over stdio, assert the JSON-RPC response shape.
//! No live IRIS required — verifies the tool is wired and returns the expected JSON.
//!
//! Run with:
//!   cargo build && IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --test binary_093_reload_pool -- --include-ignored --test-threads=1

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

// ---------------------------------------------------------------------------
// T093-B1: tools/list contains iris_reload_pool
// ---------------------------------------------------------------------------

/// T093-B1: `tools/list` must include `iris_reload_pool` with a description that
/// mentions "reload" or "pool". No live IRIS required.
#[test]
#[ignore]
fn test_iris_reload_pool_in_tools_list() {
    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("Skipping T093-B1: binary not found at {:?}", bin);
        return;
    }

    let mut cmd = Command::new(&bin);
    cmd.arg("mcp")
        .env_remove("IRIS_HOST")
        .env("IRIS_WEB_PORT", "9")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("spawn binary");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    send_initialize(&mut stdin);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let list_req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n";
    stdin.write_all(list_req.as_bytes()).ok();
    stdin.flush().ok();

    let found = read_until(stdout, 8000, |v| {
        let tools = v.get("result")?.get("tools")?.as_array()?;
        for tool in tools {
            if tool.get("name")?.as_str()? == "iris_reload_pool" {
                return Some(true);
            }
        }
        None
    });

    child.kill().ok();
    assert!(
        found.is_some(),
        "T093-B1: iris_reload_pool not found in tools/list"
    );
}

// ---------------------------------------------------------------------------
// T093-B2: iris_reload_pool returns {success: true, servers_loaded, servers, note}
// ---------------------------------------------------------------------------

/// T093-B2: calling `iris_reload_pool` returns structured JSON with `success: true`
/// and `servers_loaded` as a number even when no config file exists.
#[test]
#[ignore]
fn test_iris_reload_pool_returns_success_json() {
    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("Skipping T093-B2: binary not found at {:?}", bin);
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");

    let mut cmd = Command::new(&bin);
    cmd.arg("mcp")
        .env_remove("IRIS_HOST")
        .env("IRIS_WEB_PORT", "9")
        .env("HOME", tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("spawn binary");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    send_initialize(&mut stdin);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "iris_reload_pool", "arguments": {} }
    })
    .to_string()
        + "\n";
    stdin.write_all(call.as_bytes()).ok();
    stdin.flush().ok();

    let result = read_until(stdout, 8000, |v| {
        if v.get("id")?.as_u64()? != 2 {
            return None;
        }
        parse_tool_result(v)
    });

    child.kill().ok();

    let result = result.expect("T093-B2: no response from iris_reload_pool");
    assert_eq!(
        result["success"].as_bool(),
        Some(true),
        "T093-B2: success must be true, got {result}"
    );
    assert!(
        result["servers_loaded"].is_number(),
        "T093-B2: servers_loaded must be a number, got {result}"
    );
    assert!(
        result["servers"].is_array(),
        "T093-B2: servers must be an array, got {result}"
    );
    assert!(
        result["note"].is_string(),
        "T093-B2: note must be a string, got {result}"
    );
}

// ---------------------------------------------------------------------------
// T093-B3: iris_reload_pool returns TOML_PARSE_ERROR when config is malformed
// ---------------------------------------------------------------------------

/// T093-B3: if `.iris-agentic-dev.toml` has invalid TOML syntax, `iris_reload_pool`
/// returns `{success: false, error_code: "TOML_PARSE_ERROR"}` and leaves the pool intact.
#[test]
#[ignore]
fn test_iris_reload_pool_returns_toml_parse_error_on_bad_config() {
    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("Skipping T093-B3: binary not found at {:?}", bin);
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    // Write a malformed TOML file into HOME so the binary picks it up.
    let config = tmp.path().join(".iris-agentic-dev.toml");
    std::fs::write(&config, "this is not valid toml ===\n[broken").expect("write config");

    let mut cmd = Command::new(&bin);
    cmd.arg("mcp")
        .env_remove("IRIS_HOST")
        .env("IRIS_WEB_PORT", "9")
        // Override workspace root so the binary sees our bad config.
        .env("OBJECTSCRIPT_WORKSPACE", tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("spawn binary");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    send_initialize(&mut stdin);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "iris_reload_pool", "arguments": {} }
    })
    .to_string()
        + "\n";
    stdin.write_all(call.as_bytes()).ok();
    stdin.flush().ok();

    let result = read_until(stdout, 8000, |v| {
        if v.get("id")?.as_u64()? != 2 {
            return None;
        }
        parse_tool_result(v)
    });

    child.kill().ok();

    let result = result.expect("T093-B3: no response from iris_reload_pool");
    assert_eq!(
        result["success"].as_bool(),
        Some(false),
        "T093-B3: success must be false for bad TOML, got {result}"
    );
    assert_eq!(
        result["error_code"].as_str(),
        Some("TOML_PARSE_ERROR"),
        "T093-B3: error_code must be TOML_PARSE_ERROR, got {result}"
    );
    assert!(
        result["note"].as_str().unwrap_or("").contains("preserved"),
        "T093-B3: note must mention preserved pool, got {result}"
    );
}
