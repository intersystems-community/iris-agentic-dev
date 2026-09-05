//! Binary invocation tests for spec 095 — iris_add_server plaintext credential fallback.
//!
//! On Linux CI (no OS keychain), `iris_add_server` must fall back to writing the
//! credential into servers.json and return `{added: true, stored_plaintext: true, ...}`.
//!
//! These tests are `#[ignore]` and require IAD_BINARY to be set (CI builds it first).
//! On macOS dev machines where a keychain is available, the test may exercise the keychain
//! path instead — only CI (Linux) canonically validates the plaintext fallback.
//!
//! Run with:
//!   cargo build && IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --test binary_095_add_server_plaintext -- --include-ignored --test-threads=1

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

// A relative `IAD_BINARY` — the form CLAUDE.md and every doc comment in this crate tell you to
// pass — is resolved against the process working directory, which for a workspace member's test
// binary is the *member* directory. `./target/debug/iris-agentic-dev` therefore never resolved
// here. `iad_binary_path` resolves relative values against the workspace root, and there is one
// copy of that rule instead of six.
fn iad_binary() -> std::path::PathBuf {
    iris_agentic_dev_core::testing::iad_binary_path()
}

/// Read one newline-delimited JSON line matching `predicate` or time out.
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

/// Parse `tools/call` result content as the inner JSON object.
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

// ---------------------------------------------------------------------------
// T095-B1: iris_add_server on headless host → plaintext fallback
// ---------------------------------------------------------------------------

/// T095-B1: When the OS keychain is unavailable (Linux CI), `iris_add_server` must
/// return `{added: true, stored_plaintext: true}` and write the credential into
/// servers.json rather than returning a KEYCHAIN_FAILED error.
///
/// On macOS with a real keychain the test verifies that at minimum `added: true` is
/// returned (either path succeeds).
#[test]
#[ignore]
fn test_add_server_returns_success_without_keychain() {
    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("Skipping T095-B1: binary not found at {:?}", bin);
        return;
    }

    // Isolated temp dir for servers.json and config.
    let tmp = tempfile::TempDir::new().expect("tmp dir");
    let servers_dir = tmp.path().join(".config").join("iris-agentic-dev");
    std::fs::create_dir_all(&servers_dir).expect("create servers dir");
    let servers_json = servers_dir.join("servers.json");

    // Minimal toml config — no IRIS connection needed for this tool.
    let cfg_path = tmp.path().join(".iris-agentic-dev.toml");
    std::fs::write(&cfg_path, "").expect("write empty config");

    let mut cmd = Command::new(&bin);
    cmd.arg("mcp")
        .args(["--config", cfg_path.to_str().unwrap()])
        // Override HOME so servers.json writes to our temp dir.
        .env("HOME", tmp.path())
        .env_remove("IRIS_HOST")
        .env_remove("IRIS_CONTAINER")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("spawn binary");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    // Handshake.
    let init = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"0.0.1\"}}}\n";
    let notif = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n";
    stdin.write_all(init.as_bytes()).ok();
    stdin.write_all(notif.as_bytes()).ok();
    stdin.flush().ok();
    std::thread::sleep(std::time::Duration::from_millis(150));

    // Call iris_add_server.
    let call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "iris_add_server",
            "arguments": {
                "name": "test-plaintext",
                "host": "192.0.2.1",
                "port": 52780,
                "namespace": "USER",
                "username": "_SYSTEM",
                "password": "SYS"
            }
        }
    })
    .to_string()
        + "\n";
    stdin.write_all(call.as_bytes()).ok();
    stdin.flush().ok();

    let result = read_until(stdout, 10000, |v| {
        if v.get("id")?.as_u64()? != 2 {
            return None;
        }
        parse_tool_result(v)
    });

    child.kill().ok();
    child.wait().ok();

    let result = result.expect("T095-B1: no tools/call response received");

    // On any platform, the call must succeed (added: true).
    assert_eq!(
        result["added"].as_bool(),
        Some(true),
        "T095-B1: iris_add_server must return added=true, got: {result}"
    );
    // The error_code must NOT be KEYCHAIN_FAILED — that was the old behaviour.
    assert_ne!(
        result["error_code"].as_str(),
        Some("KEYCHAIN_FAILED"),
        "T095-B1: KEYCHAIN_FAILED must no longer be returned when credential can be stored, got: {result}"
    );

    // On Linux (no keychain), stored_plaintext must be true and servers.json must
    // contain the credential field. On macOS (keychain available) we don't assert
    // stored_plaintext because it goes through the keychain path.
    if result["stored_plaintext"].as_bool() == Some(true) {
        // Plaintext path: verify the servers.json was written with the credential field.
        let contents = std::fs::read_to_string(&servers_json)
            .expect("T095-B1: servers.json must exist after iris_add_server");
        assert!(
            contents.contains("\"password\""),
            "T095-B1: servers.json must contain 'password' field when stored_plaintext=true, got: {contents}"
        );
        assert!(
            contents.contains("\"test-plaintext\""),
            "T095-B1: servers.json must contain the server name, got: {contents}"
        );
    }
}

// ---------------------------------------------------------------------------
// T095-B2: tools/list — iris_add_server description mentions plaintext fallback
// ---------------------------------------------------------------------------

/// T095-B2: The `iris_add_server` tool description must mention headless / plaintext
/// fallback so agents understand the behaviour in MCP contexts.
#[test]
#[ignore]
fn test_add_server_description_mentions_plaintext_fallback() {
    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("Skipping T095-B2: binary not found at {:?}", bin);
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

    let init = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"0.0.1\"}}}\n";
    let notif = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n";
    stdin.write_all(init.as_bytes()).ok();
    stdin.write_all(notif.as_bytes()).ok();
    stdin.flush().ok();
    std::thread::sleep(std::time::Duration::from_millis(50));

    let list_req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n";
    stdin.write_all(list_req.as_bytes()).ok();
    stdin.flush().ok();

    let description = read_until(stdout, 8000, |v| {
        let tools = v.get("result")?.get("tools")?.as_array()?;
        for tool in tools {
            if tool.get("name")?.as_str()? == "iris_add_server" {
                let desc = tool.get("description")?.as_str()?;
                return Some(desc.to_string());
            }
        }
        None
    });

    child.kill().ok();
    child.wait().ok();

    let desc = description.expect("T095-B2: iris_add_server description not found in tools/list");

    assert!(
        desc.contains("plaintext") || desc.contains("headless"),
        "T095-B2: iris_add_server description must mention plaintext/headless fallback: {desc}"
    );
}
