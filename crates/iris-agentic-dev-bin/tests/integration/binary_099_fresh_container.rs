//! Binary invocation tests for spec 099 — admin fresh container setup actions.
//!
//! Layer 2: spawn `iris-agentic-dev` as a subprocess, send `initialize` +
//! `tools/call iris_admin` over stdio, assert the JSON-RPC response shape.
//! No live IRIS required for these tests — they verify wiring, not IRIS behaviour.
//!
//! Run with:
//!   cargo build && IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --test binary_099_fresh_container -- --include-ignored --test-threads=1

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

fn spawn_mcp_no_iris() -> Option<(
    std::process::Child,
    std::io::BufWriter<std::process::ChildStdin>,
    std::process::ChildStdout,
)> {
    let bin = iad_binary();
    if !bin.exists() {
        return None;
    }
    let mut cmd = Command::new(&bin);
    cmd.arg("mcp")
        .env_remove("IRIS_HOST")
        .env_remove("IRIS_CONTAINER")
        .env("IRIS_WEB_PORT", "9") // unreachable port
        // `IRIS_ADMIN_TOOLS` is the name `admin::admin_writes_enabled()` reads. This line said
        // `IRIS_ADMIN_TOOLS_ENABLED`, which nothing in `crates/*/src/` reads — so the gate was
        // never opened and the caller below was measuring an ADMIN_WRITE_DISABLED refusal, not the
        // no-connection path it documents.
        .env("IRIS_ADMIN_TOOLS", "1")
        // `fresh_container_setup` is WriteClass::Write on top of the admin gate, and a refusal from
        // either gate also satisfies the loose "structured JSON" assertion below — so an unpinned
        // run passes without the tool ever attempting a connection. Writes on is what forces the
        // request out to the closed port and back as the connection error this test documents;
        // destructive off keeps the action's Write classification (write_gate.rs:544) asserted.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().ok()?;
    let stdin = std::io::BufWriter::new(child.stdin.take().unwrap());
    let stdout = child.stdout.take().unwrap();
    Some((child, stdin, stdout))
}

fn send_initialize(stdin: &mut impl Write) {
    let init = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"0.0.1\"}}}\n";
    let notif = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n";
    stdin.write_all(init.as_bytes()).ok();
    stdin.write_all(notif.as_bytes()).ok();
    stdin.flush().ok();
}

// ---------------------------------------------------------------------------
// T099-B1: tools/list contains iris_admin with fresh_container_setup action
// ---------------------------------------------------------------------------

/// T099-B1: `tools/list` must include `iris_admin` with a description that
/// mentions `fresh_container_setup`. No live IRIS required.
#[test]
#[ignore]
fn test_iris_admin_description_mentions_fresh_container_setup() {
    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("Skipping T099-B1: binary not found at {:?}", bin);
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

    let description = read_until(stdout, 8000, |v| {
        let tools = v.get("result")?.get("tools")?.as_array()?;
        for tool in tools {
            if tool.get("name")?.as_str()? == "iris_admin" {
                let desc = tool.get("description")?.as_str()?;
                return Some(desc.to_string());
            }
        }
        None
    });

    child.kill().ok();
    child.wait().ok();

    let desc = description.expect("T099-B1: iris_admin not found in tools/list");
    assert!(
        desc.contains("fresh_container_setup"),
        "T099-B1: iris_admin description must mention fresh_container_setup: {desc}"
    );
}

// ---------------------------------------------------------------------------
// T099-B2: iris_admin fresh_container_setup returns expected JSON shape
// ---------------------------------------------------------------------------

/// T099-B2: `iris_admin` with `action=fresh_container_setup` returns a JSON object
/// with `steps` or `ready` fields even when IRIS is not reachable (connection error
/// path). The tool must not panic or return an unstructured error.
///
/// Specifically: when there is no IRIS connection, the tool returns an error JSON
/// (success=false) with an error_code, NOT a bare MCP error that would crash the caller.
#[test]
#[ignore]
fn test_iris_admin_fresh_container_setup_returns_structured_json() {
    let Some((mut child, mut stdin, stdout)) = spawn_mcp_no_iris() else {
        eprintln!("Skipping T099-B2: binary not found");
        return;
    };

    send_initialize(&mut stdin);
    std::thread::sleep(std::time::Duration::from_millis(150));

    let call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "iris_admin",
            "arguments": {
                "action": "fresh_container_setup",
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

    let result = result.expect("T099-B2: no tools/call response for fresh_container_setup");

    // When IRIS is unreachable, we expect a structured error response.
    // Either {success: false, error_code: "..."} or {ready: bool, steps: [...]}
    let has_success = result.get("success").is_some();
    let has_ready = result.get("ready").is_some();
    let has_steps = result.get("steps").is_some();
    let has_error_code = result.get("error_code").is_some();

    assert!(
        has_success || has_ready || has_steps || has_error_code,
        "T099-B2: fresh_container_setup must return structured JSON with 'success', 'ready', 'steps', or 'error_code', got: {result}"
    );
}
