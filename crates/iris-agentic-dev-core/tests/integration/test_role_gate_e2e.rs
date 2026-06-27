//! E2E tests for role-gate enforcement in operate mode (Amendment 001).
//!
//! Spawns the iris-agentic-dev MCP binary with OBJECTSCRIPT_WORKSPACE pointing at a
//! tempdir containing an operate-mode fleet config that declares the active connection
//! as a subject instance.  Verifies:
//!   - iris_compile returns role_gate error without confirm
//!   - iris_compile proceeds with confirm: true
//!   - iris_execute returns role_gate error without confirm
//!   - iris_query SELECT is always permitted on subject
//!   - iris_query INSERT returns role_gate without confirm
//!   - iris_source_control checkout is hard-blocked on subject (confirm has no effect)
//!   - iris_source_control status is always permitted on subject
//!   - develop-mode config (no mode field) produces no role-gate (US7 regression)
//!
//! Run with a live IRIS instance:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52773 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --test test_role_gate_e2e -- --nocapture
//!
//! All tests skip gracefully when IRIS_HOST is not set or the binary is absent.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn iris_dev_bin() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("IRIS_DEV_BIN") {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return p;
        }
    }
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/iris-agentic-dev-core
    p.pop(); // crates/
    p.push("target/debug/iris-agentic-dev");
    if !p.exists() {
        p.pop();
        p.push("release/iris-agentic-dev");
    }
    p
}

fn iris_host() -> String {
    std::env::var("IRIS_HOST").unwrap_or_default()
}

macro_rules! require_iris {
    () => {
        if iris_host().is_empty() {
            eprintln!("Skipping: IRIS_HOST not set");
            return;
        }
        if !iris_dev_bin().exists() {
            eprintln!(
                "Skipping: iris-agentic-dev binary not found at {:?}",
                iris_dev_bin()
            );
            return;
        }
    };
}

macro_rules! require_bin {
    () => {
        if !iris_dev_bin().exists() {
            eprintln!(
                "Skipping: iris-agentic-dev binary not found at {:?}",
                iris_dev_bin()
            );
            return;
        }
    };
}

fn init_msgs() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e-role-gate","version":"0.1"}
        }}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    ]
}

fn tool_result(responses: &[serde_json::Value], id: u64) -> serde_json::Value {
    let resp = responses
        .iter()
        .find(|r| r["id"] == id)
        .cloned()
        .unwrap_or_default();
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("{}");
    serde_json::from_str(text).unwrap_or_default()
}

/// Spawn the MCP binary with the given workspace dir and env vars, send messages, collect responses.
fn mcp_call_with_workspace(
    workspace_dir: &std::path::Path,
    extra_env: &[(&str, String)],
    messages: &[serde_json::Value],
    timeout_secs: u64,
) -> Vec<serde_json::Value> {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return vec![];
    }

    let mut cmd = Command::new(&bin);
    cmd.args(["mcp"]);
    cmd.env("OBJECTSCRIPT_WORKSPACE", workspace_dir);
    // Pass through IRIS connection env vars
    for key in &[
        "IRIS_HOST",
        "IRIS_WEB_PORT",
        "IRIS_USERNAME",
        "IRIS_PASSWORD",
        "IRIS_NAMESPACE",
        "IRIS_CONTAINER",
    ] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut results = vec![];

    for msg in messages {
        stdin
            .write_all((serde_json::to_string(msg).unwrap() + "\n").as_bytes())
            .unwrap();
        stdin.flush().unwrap();
        if msg.get("id").is_some() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                        results.push(v);
                        break;
                    }
                }
                if std::time::Instant::now() > deadline {
                    break;
                }
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    child.kill().ok();
    results
}

/// Call a single tool with an operate-mode fleet config where the active IRIS_HOST
/// is declared as a subject instance.  Returns the tool result JSON.
fn call_with_subject_config(
    tool_name: &str,
    args: serde_json::Value,
    timeout_secs: u64,
) -> (serde_json::Value, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().unwrap();
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".to_string());
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52773".to_string());
    let container = std::env::var("IRIS_CONTAINER").unwrap_or_default();

    // Build fleet config: one instance matching the active connection, role=subject.
    // If IRIS_CONTAINER is set, match by container; otherwise match by host.
    let toml = if !container.is_empty() {
        format!(
            r#"mode = "operate"

[instance.prod]
container = "{container}"
namespace = "USER"
role = "subject"
"#
        )
    } else {
        format!(
            r#"mode = "operate"

[instance.prod]
host = "{host}"
web_port = {web_port}
namespace = "USER"
role = "subject"
"#
        )
    };

    std::fs::write(dir.path().join(".iris-agentic-dev.toml"), &toml).unwrap();

    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name": tool_name, "arguments": args}
    }));

    let responses = mcp_call_with_workspace(dir.path(), &[], &msgs, timeout_secs);
    (tool_result(&responses, 2), dir)
}

/// Call with a develop-mode (flat) config — used for US7 regression tests.
fn call_with_develop_config(
    tool_name: &str,
    args: serde_json::Value,
    timeout_secs: u64,
) -> (serde_json::Value, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().unwrap();
    let container = std::env::var("IRIS_CONTAINER").unwrap_or_else(|_| "iris-dev".to_string());

    let toml = format!(
        r#"container = "{container}"
namespace = "USER"
"#
    );
    std::fs::write(dir.path().join(".iris-agentic-dev.toml"), &toml).unwrap();

    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name": tool_name, "arguments": args}
    }));

    let responses = mcp_call_with_workspace(dir.path(), &[], &msgs, timeout_secs);
    (tool_result(&responses, 2), dir)
}

// ── iris_compile role gate ────────────────────────────────────────────────────

#[test]
fn e2e_role_gate_compile_subject_no_confirm_blocked() {
    require_iris!();
    let (result, _dir) = call_with_subject_config(
        "iris_compile",
        serde_json::json!({"target": "User.RoleGateTest.cls", "namespace": "USER"}),
        10,
    );
    assert_eq!(
        result["role_gate"].as_bool(),
        Some(true),
        "iris_compile on subject without confirm must return role_gate:true, got: {}",
        result
    );
    assert!(
        result["required_confirmation"].as_str().is_some(),
        "must include required_confirmation field, got: {}",
        result
    );
    assert_eq!(
        result["hard_block"].as_bool(),
        None,
        "compile gate must not be hard_block, got: {}",
        result
    );
}

#[test]
fn e2e_role_gate_compile_subject_with_confirm_proceeds() {
    require_iris!();
    let (result, _dir) = call_with_subject_config(
        "iris_compile",
        serde_json::json!({"target": "User.DoesNotExistXYZ.cls", "namespace": "USER", "confirm": true}),
        10,
    );
    // With confirm=true, role gate is bypassed — should get a compile error (not role_gate)
    assert_ne!(
        result["role_gate"].as_bool(),
        Some(true),
        "iris_compile with confirm:true must not return role_gate, got: {}",
        result
    );
}

// ── iris_execute role gate ────────────────────────────────────────────────────

#[test]
fn e2e_role_gate_execute_subject_no_confirm_blocked() {
    require_iris!();
    let (result, _dir) = call_with_subject_config(
        "iris_execute",
        serde_json::json!({"code": "Write 42", "namespace": "USER"}),
        10,
    );
    assert_eq!(
        result["role_gate"].as_bool(),
        Some(true),
        "iris_execute on subject without confirm must return role_gate:true, got: {}",
        result
    );
}

#[test]
fn e2e_role_gate_execute_subject_confirmed_proceeds() {
    require_iris!();
    let (result, _dir) = call_with_subject_config(
        "iris_execute",
        serde_json::json!({"code": "Write 42", "namespace": "USER", "confirmed": true}),
        10,
    );
    assert_ne!(
        result["role_gate"].as_bool(),
        Some(true),
        "iris_execute with confirmed:true must not return role_gate, got: {}",
        result
    );
}

// ── iris_query role gate ──────────────────────────────────────────────────────

#[test]
fn e2e_role_gate_query_select_subject_permitted() {
    require_iris!();
    let (result, _dir) = call_with_subject_config(
        "iris_query",
        serde_json::json!({"query": "SELECT 1 AS n", "namespace": "USER"}),
        10,
    );
    assert_ne!(
        result["role_gate"].as_bool(),
        Some(true),
        "SELECT on subject must never be role-gated, got: {}",
        result
    );
}

#[test]
fn e2e_role_gate_query_insert_subject_no_confirm_blocked() {
    require_iris!();
    let (result, _dir) = call_with_subject_config(
        "iris_query",
        // force:true to pass SQL safety gate; role gate fires before SQL safety anyway
        serde_json::json!({"query": "INSERT INTO %SYS.Users (Name) VALUES ('x')", "namespace": "USER", "force": true}),
        10,
    );
    assert_eq!(
        result["role_gate"].as_bool(),
        Some(true),
        "INSERT on subject without confirm must return role_gate:true, got: {}",
        result
    );
}

// ── iris_source_control role gate ─────────────────────────────────────────────

#[test]
fn e2e_role_gate_scm_checkout_subject_hard_blocked() {
    require_iris!();
    let (result, _dir) = call_with_subject_config(
        "iris_source_control",
        serde_json::json!({"action": "checkout", "document": "User.RoleGateTest", "namespace": "USER"}),
        10,
    );
    assert_eq!(
        result["role_gate"].as_bool(),
        Some(true),
        "checkout on subject must return role_gate:true, got: {}",
        result
    );
    assert_eq!(
        result["hard_block"].as_bool(),
        Some(true),
        "checkout on subject must be hard_block:true, got: {}",
        result
    );
}

#[test]
fn e2e_role_gate_scm_checkout_subject_confirm_still_blocked() {
    require_iris!();
    let (result, _dir) = call_with_subject_config(
        "iris_source_control",
        serde_json::json!({"action": "checkout", "document": "User.RoleGateTest", "namespace": "USER", "confirm": true}),
        10,
    );
    // Hard block: confirm has no effect
    assert_eq!(
        result["role_gate"].as_bool(),
        Some(true),
        "checkout hard_block must not be bypassable with confirm:true, got: {}",
        result
    );
    assert_eq!(
        result["hard_block"].as_bool(),
        Some(true),
        "must still be hard_block:true even with confirm:true, got: {}",
        result
    );
}

#[test]
fn e2e_role_gate_scm_status_subject_permitted() {
    require_iris!();
    let (result, _dir) = call_with_subject_config(
        "iris_source_control",
        serde_json::json!({"action": "status", "document": "User.RoleGateTest", "namespace": "USER"}),
        10,
    );
    assert_ne!(
        result["role_gate"].as_bool(),
        Some(true),
        "status on subject must not be role-gated, got: {}",
        result
    );
}

// ── US7 regression: develop-mode flat config produces no gate ─────────────────

#[test]
fn e2e_role_gate_develop_mode_compile_no_gate() {
    require_iris!();
    let (result, _dir) = call_with_develop_config(
        "iris_compile",
        serde_json::json!({"target": "User.DoesNotExistXYZ.cls", "namespace": "USER"}),
        10,
    );
    assert_ne!(
        result["role_gate"].as_bool(),
        Some(true),
        "develop-mode flat config must never gate iris_compile, got: {}",
        result
    );
}

#[test]
fn e2e_role_gate_develop_mode_execute_no_gate() {
    require_iris!();
    let (result, _dir) = call_with_develop_config(
        "iris_execute",
        serde_json::json!({"code": "Write 42", "namespace": "USER", "confirmed": true}),
        10,
    );
    assert_ne!(
        result["role_gate"].as_bool(),
        Some(true),
        "develop-mode flat config must never gate iris_execute, got: {}",
        result
    );
}

// ── Binary-only: no-config produces no gate (baseline regression) ─────────────

#[test]
fn e2e_role_gate_no_config_file_no_gate() {
    require_bin!();
    // No .iris-agentic-dev.toml at all — must never produce a role_gate response.
    let dir = tempfile::TempDir::new().unwrap();
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_compile","arguments":{"target":"Test.DoesNotExist.cls","namespace":"USER"}}
    }));
    let responses = mcp_call_with_workspace(dir.path(), &[], &msgs, 5);
    let result = tool_result(&responses, 2);
    assert_ne!(
        result["role_gate"].as_bool(),
        Some(true),
        "no config file must never produce role_gate, got: {}",
        result
    );
}
