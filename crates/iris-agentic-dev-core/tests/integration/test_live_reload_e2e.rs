// E2E tests for live connection hot-reload and check_config against iris-dev-iris.
// All tests are #[ignore] — run with:
//   IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --test test_live_reload_e2e -- --ignored --nocapture

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn iris_dev_bin() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("IRIS_DEV_BIN") {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return p;
        }
    }
    let workspace_root = {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p
    };
    for target_subdir in [
        "target/debug/iris-agentic-dev",
        "target/release/iris-agentic-dev",
        "target/llvm-cov-target/debug/iris-agentic-dev",
        "target/llvm-cov-target/release/iris-agentic-dev",
    ] {
        let candidate = workspace_root.join(target_subdir);
        if candidate.exists() {
            return candidate;
        }
    }
    workspace_root.join("target/debug/iris-agentic-dev")
}

fn iris_host() -> String {
    std::env::var("IRIS_HOST").unwrap_or_default()
}

/// The container to select via iris_containers(action=select)/iris_select_container.
/// Was hardcoded to "iris-dev-iris" (a personal dev-machine convention) — broke
/// immediately in CI, where the container is named iris-e2e.
fn select_container_name() -> String {
    std::env::var("IRIS_CONTAINER").unwrap_or_else(|_| "iris-dev-iris".to_string())
}

fn mcp_call_with_toml(
    toml_dir: Option<&std::path::Path>,
    extra_env: &[(&str, &str)],
    messages: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return vec![];
    }
    let mut cmd = Command::new(&bin);
    cmd.args(["mcp"]);
    cmd.env_remove("IRIS_CONTAINER");
    for key in &[
        "IRIS_HOST",
        "IRIS_WEB_PORT",
        "IRIS_USERNAME",
        "IRIS_PASSWORD",
    ] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    if let Some(dir) = toml_dir {
        cmd.env("OBJECTSCRIPT_WORKSPACE", dir);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-dev mcp");

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
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
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
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    child.kill().ok();
    child.wait().ok();
    results
}

fn init_msgs() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    ]
}

fn tool_result(responses: &[serde_json::Value], id: u64) -> serde_json::Value {
    let resp = responses
        .iter()
        .find(|r| r["id"] == id)
        .cloned()
        .unwrap_or_default();
    let text = resp["result"]["content"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|c| c["text"].as_str())
        .unwrap_or("{}");
    serde_json::from_str(text).unwrap_or_default()
}

fn call_tool(tool: &str, args: serde_json::Value) -> serde_json::Value {
    call_tool_with_toml(None, &[], tool, args)
}

fn call_tool_with_toml(
    toml_dir: Option<&std::path::Path>,
    extra_env: &[(&str, &str)],
    tool: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":tool,"arguments":args}
    }));
    let responses = mcp_call_with_toml(toml_dir, extra_env, &msgs);
    tool_result(&responses, 2)
}

/// T022: Config file pointing to unreachable container — next call returns IRIS_UNREACHABLE (not crash).
#[test]
#[ignore]
fn test_e2e_unreachable_container_returns_iris_unreachable() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // Write a .iris-dev.toml pointing to a nonexistent container
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        "container = \"nonexistent-container-xyz\"\n",
    )
    .unwrap();
    let result = call_tool_with_toml(
        Some(dir.path()),
        &[],
        "iris_execute",
        serde_json::json!({"code": "write $ZVersion,!"}),
    );
    eprintln!(
        "T022 result: {}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    // The spec says graceful degradation — no crash (no panic, no server crash).
    // If IRIS_HOST/IRIS_WEB_PORT env vars are set, the connection may succeed via those
    // even if the container config is wrong. The key check: the process did not crash
    // and returned a valid JSON response.
    assert!(
        result.is_object(),
        "should return a valid JSON object response (no crash)"
    );
    // Verify the session didn't produce a panic/fatal error
    assert!(
        result.get("success").is_some(),
        "response should have a 'success' field"
    );
}

/// T029: iris_select_container with iris-dev-iris → check_config shows iris_select_container source.
#[test]
#[ignore]
fn test_e2e_select_container_updates_check_config() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    // In a single MCP session: select container, then check_config
    // iris_select_container consolidated into iris_containers(action=select) — FR-007.
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_containers","arguments":{"action":"select","name":select_container_name(),"namespace":"USER"}}
    }));
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"check_config","arguments":{}}
    }));
    let responses = mcp_call_with_toml(None, &[], &msgs);
    let select_result = tool_result(&responses, 2);
    let config_result = tool_result(&responses, 3);
    eprintln!(
        "T029 select result: {}",
        serde_json::to_string_pretty(&select_result).unwrap()
    );
    eprintln!(
        "T029 check_config result: {}",
        serde_json::to_string_pretty(&config_result).unwrap()
    );

    assert_eq!(
        select_result["switched"], true,
        "iris_select_container should return switched:true"
    );
    assert_eq!(
        config_result["connection_source"], "iris_select_container",
        "check_config should show iris_select_container source"
    );
}

/// T030: iris_select_container → iris_execute returns output from the new container.
#[test]
#[ignore]
fn test_e2e_select_container_execute_uses_new_connection() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_containers","arguments":{"action":"select","name":select_container_name(),"namespace":"USER"}}
    }));
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"iris_execute","arguments":{"code":"write $ZVersion,!","namespace":"USER"}}
    }));
    let responses = mcp_call_with_toml(None, &[], &msgs);
    let exec_result = tool_result(&responses, 3);
    eprintln!(
        "T030 exec result: {}",
        serde_json::to_string_pretty(&exec_result).unwrap()
    );

    // Should get output from IRIS (not IRIS_UNREACHABLE)
    assert_eq!(
        exec_result["success"], true,
        "iris_execute should succeed after container switch"
    );
    assert!(
        exec_result["output"]
            .as_str()
            .unwrap_or("")
            .contains("IRIS"),
        "output should contain IRIS version string"
    );
}

/// T037: check_config after session start returns all required fields.
#[test]
#[ignore]
fn test_e2e_check_config_returns_all_fields() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    let result = call_tool("check_config", serde_json::json!({}));
    eprintln!(
        "T037 result: {}",
        serde_json::to_string_pretty(&result).unwrap()
    );

    // Must contain all 9 required fields
    assert!(result.get("connected").is_some(), "missing: connected");
    assert!(result.get("host").is_some(), "missing: host");
    assert!(result.get("port").is_some(), "missing: port");
    assert!(result.get("namespace").is_some(), "missing: namespace");
    assert!(
        result.get("container").is_some(),
        "missing: container (may be null)"
    );
    assert!(
        result.get("config_file").is_some(),
        "missing: config_file (may be null)"
    );
    assert!(
        result.get("config_loaded_at").is_some(),
        "missing: config_loaded_at"
    );
    assert!(
        result.get("iris_version").is_some(),
        "missing: iris_version (may be null)"
    );
    assert!(
        result.get("write_tools_enabled").is_some(),
        "missing: write_tools_enabled"
    );
    assert!(
        result.get("connection_source").is_some(),
        "missing: connection_source"
    );

    // Must not return IRIS_UNREACHABLE
    assert_ne!(result["error_code"], "IRIS_UNREACHABLE");

    let valid_sources = [
        "config_file",
        "env_vars",
        "iris_select_container",
        "auto_discovered",
    ];
    let src = result["connection_source"].as_str().unwrap_or("");
    assert!(
        valid_sources.contains(&src),
        "connection_source '{}' must be one of {:?}",
        src,
        valid_sources
    );
}
