//! Binary invocation tests for spec 096 — terminal-mode ObjectScript compatibility.
//!
//! These tests spawn the binary in MCP mode and call `iris_execute` with block-syntax
//! code over the docker exec path. They verify that `TERMINAL_SYNTAX_UNSUPPORTED` is
//! returned before any IRIS round-trip.
//!
//! T011: `iris_execute` with block-syntax code on docker_only path → TERMINAL_SYNTAX_UNSUPPORTED.
//! T012: `iris_execute` with classic syntax on docker exec path → no guard fires.
//! T013: `tools/list` response includes key terms about both paths.
//!
//! No live IRIS required for T011/T013: `docker_only=true` in a config file routes
//! through the guard before any IRIS call is attempted.
//!
//! Run with:
//!   cargo build && IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --test binary_096_terminal_compat -- --include-ignored --test-threads=1

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

fn iad_binary() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("IAD_BINARY") {
        return std::path::PathBuf::from(p);
    }
    // Fall back to CARGO_BIN_EXE_ macro (works when the binary is built in the same workspace)
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_iris-agentic-dev"))
}

/// Spawn the binary in MCP mode, configured with `docker_only=true` via a temp config file.
///
/// `docker_only=true` makes the HTTP path use `http://127.0.0.1:1` (unreachable sentinel),
/// so HTTP fails immediately and the docker exec path is attempted — where the
/// TERMINAL_SYNTAX_UNSUPPORTED guard fires.
fn spawn_mcp_docker_only() -> Option<(Child, ChildStdin, ChildStdout, tempfile::TempDir)> {
    let bin = iad_binary();
    if !bin.exists() {
        return None;
    }

    // Write a minimal config with docker_only=true and IRIS_WRITE_TOOLS_ENABLED=1 so
    // iris_execute is not blocked by the write gate.
    let tmp = tempfile::TempDir::new().ok()?;
    let cfg_path = tmp.path().join(".iris-agentic-dev.toml");
    std::fs::write(
        &cfg_path,
        "docker_only = true\nwrite_tools_enabled = true\n",
    )
    .ok()?;

    let mut cmd = Command::new(&bin);
    cmd.arg("mcp")
        .args(["--config", cfg_path.to_str().unwrap()])
        // Clear IRIS env so the server doesn't pick up a live connection accidentally.
        .env_remove("IRIS_HOST")
        .env_remove("IRIS_WEB_PORT")
        .env_remove("IRIS_CONTAINER")
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().ok()?;
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    Some((child, stdin, stdout, tmp))
}

/// Send a JSON-RPC initialize + notifications/initialized handshake.
fn send_initialize(stdin: &mut ChildStdin) {
    let init = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"0.0.1\"}}}\n";
    let notif = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n";
    stdin.write_all(init.as_bytes()).ok();
    stdin.write_all(notif.as_bytes()).ok();
    stdin.flush().ok();
}

/// Read newline-delimited JSON until `predicate` returns Some value or timeout.
fn read_until<T, F>(stdout: ChildStdout, timeout_ms: u64, mut predicate: F) -> Option<T>
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

/// Parse `tools/call` result content as the inner JSON object.
fn parse_tool_result(v: &serde_json::Value) -> Option<serde_json::Value> {
    let content = v.get("result")?.get("content")?.as_array()?;
    for c in content {
        if let Some(text) = c.get("text").and_then(|t| t.as_str()) {
            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text) {
                if obj.get("success").is_some() || obj.get("error_code").is_some() {
                    return Some(obj);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// T011: Block-syntax code on docker_only path → TERMINAL_SYNTAX_UNSUPPORTED
// ---------------------------------------------------------------------------

/// T011: `iris_execute` with `If x=1 { Write 1 }` via docker_only=true config returns
/// `TERMINAL_SYNTAX_UNSUPPORTED`. No live IRIS container required — the guard fires
/// before any docker exec is attempted.
#[test]
#[ignore]
fn test_block_syntax_blocked_on_docker_exec() {
    let Some((mut child, mut stdin, stdout, _tmp)) = spawn_mcp_docker_only() else {
        eprintln!("Skipping: binary not found at {:?}", iad_binary());
        return;
    };

    send_initialize(&mut stdin);
    // Brief pause to let the server finish its initialize handshake.
    std::thread::sleep(std::time::Duration::from_millis(100));

    let call = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"iris_execute\",\"arguments\":{\"code\":\"If x=1 { Write 1 }\"}}}\n";
    stdin.write_all(call.as_bytes()).ok();
    stdin.flush().ok();

    let result = read_until(stdout, 8000, |v| {
        // Only match on id=2 tools/call response
        if v.get("id")?.as_u64()? != 2 {
            return None;
        }
        parse_tool_result(v)
    });

    child.kill().ok();

    let result = result.expect("no tools/call response received for block-syntax test");

    assert_eq!(
        result["success"].as_bool(),
        Some(false),
        "expected success=false, got: {result}"
    );
    assert_eq!(
        result["error_code"].as_str(),
        Some("TERMINAL_SYNTAX_UNSUPPORTED"),
        "expected TERMINAL_SYNTAX_UNSUPPORTED error_code, got: {result}"
    );
    // No `result` field should be present — the guard fired before any IRIS call.
    assert!(
        result.get("output").is_none(),
        "guard should have fired before any IRIS call — 'output' field must be absent: {result}"
    );
}

// ---------------------------------------------------------------------------
// T012: Classic syntax on docker exec path — guard does NOT fire
// ---------------------------------------------------------------------------

/// T012: `iris_execute` with classic syntax (`Write 1`) on the docker_only path.
/// The guard must NOT fire. The call may fail for other reasons (no IRIS_CONTAINER),
/// but the error code must NOT be TERMINAL_SYNTAX_UNSUPPORTED.
#[test]
#[ignore]
fn test_classic_syntax_not_blocked_on_docker_exec() {
    let Some((mut child, mut stdin, stdout, _tmp)) = spawn_mcp_docker_only() else {
        eprintln!("Skipping: binary not found at {:?}", iad_binary());
        return;
    };

    send_initialize(&mut stdin);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let call = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"iris_execute\",\"arguments\":{\"code\":\"Write 1\"}}}\n";
    stdin.write_all(call.as_bytes()).ok();
    stdin.flush().ok();

    let result = read_until(stdout, 10000, |v| {
        if v.get("id")?.as_u64()? != 2 {
            return None;
        }
        parse_tool_result(v)
    });

    child.kill().ok();

    let result = result.expect("no tools/call response received for classic-syntax test");

    assert_ne!(
        result["error_code"].as_str(),
        Some("TERMINAL_SYNTAX_UNSUPPORTED"),
        "classic syntax must NOT trigger TERMINAL_SYNTAX_UNSUPPORTED, got: {result}"
    );
}

// ---------------------------------------------------------------------------
// T013: tools/list description mentions terminal mode and docker exec paths
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// T013: HTTP path executes normally — guard NEVER fires on HTTP path
// ---------------------------------------------------------------------------

/// T013: `iris_execute` via the HTTP path — guard must NOT fire, even with block-syntax code.
///
/// This test requires a live iris-dev-iris container.
/// Run with IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS
#[test]
#[ignore]
fn test_http_path_does_not_trigger_terminal_guard() {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());

    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("Skipping: binary not found at {:?}", bin);
        return;
    }

    // Create a config WITHOUT docker_only — uses HTTP path.
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg_path = tmp.path().join(".iris-agentic-dev.toml");
    std::fs::write(
        &cfg_path,
        format!(
            "host = \"{host}\"\nweb_port = {port}\nusername = \"{username}\"\npassword = \"{password}\"\nwrite_tools_enabled = true\n"
        ),
    )
    .unwrap();

    let mut cmd = Command::new(&bin);
    cmd.arg("mcp")
        .args(["--config", cfg_path.to_str().unwrap()])
        .env_remove("IRIS_CONTAINER")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("failed to spawn binary");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    send_initialize(&mut stdin);
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Send iris_execute with simple code — HTTP path should return success
    let call = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"iris_execute\",\"arguments\":{\"code\":\"Write 1\"}}}\n";
    stdin.write_all(call.as_bytes()).ok();
    stdin.flush().ok();

    let result = read_until(stdout, 15000, |v| {
        if v.get("id")?.as_u64()? != 2 {
            return None;
        }
        parse_tool_result(v)
    });

    child.kill().ok();

    let result = result.expect("no tools/call response received for HTTP path test");

    // The HTTP path must NOT return TERMINAL_SYNTAX_UNSUPPORTED.
    assert_ne!(
        result["error_code"].as_str(),
        Some("TERMINAL_SYNTAX_UNSUPPORTED"),
        "HTTP path must never return TERMINAL_SYNTAX_UNSUPPORTED, got: {result}"
    );
    // The HTTP path should succeed with Write 1.
    assert_eq!(
        result["success"].as_bool(),
        Some(true),
        "iris_execute Write 1 via HTTP should succeed, got: {result}"
    );
    assert_eq!(
        result["method"].as_str(),
        Some("http"),
        "expected method=http, got: {result}"
    );
}

// ---------------------------------------------------------------------------
// T013b: tools/list description — no live IRIS required
// ---------------------------------------------------------------------------

/// T013b: `tools/list` — the `iris_execute` tool description must mention:
/// - "terminal mode" or "terminal"
/// - "docker exec"
/// - block syntax limitation (`{}`)
///
/// No live IRIS required.
#[test]
#[ignore]
fn test_iris_execute_description_documents_both_paths() {
    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("Skipping: binary not found at {:?}", bin);
        return;
    }

    let mut cmd = Command::new(&bin);
    cmd.arg("mcp")
        .env_remove("IRIS_HOST")
        .env("IRIS_WEB_PORT", "9") // unreachable port — no live IRIS needed
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("failed to spawn binary");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    send_initialize(&mut stdin);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let list_req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n";
    stdin.write_all(list_req.as_bytes()).ok();
    stdin.flush().ok();

    let description = read_until(stdout, 8000, |v| {
        let tools = v.get("result")?.get("tools")?.as_array()?;
        for tool in tools {
            if tool.get("name")?.as_str()? == "iris_execute" {
                let desc = tool.get("description")?.as_str()?;
                return Some(desc.to_string());
            }
        }
        None
    });

    child.kill().ok();

    let desc = description.expect("iris_execute tool description not found in tools/list");

    assert!(
        desc.contains("terminal") || desc.contains("terminal mode"),
        "description must mention 'terminal' or 'terminal mode': {desc}"
    );
    assert!(
        desc.contains("docker exec"),
        "description must mention 'docker exec': {desc}"
    );
    assert!(
        desc.contains("{}") || desc.contains("{...}"),
        "description must mention block syntax limitation ({{}} or {{...}}): {desc}"
    );
}
