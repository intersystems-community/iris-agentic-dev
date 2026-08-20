//! Binary invocation tests for CLI flag wiring and config round-trips (spec 081).
//!
//! These tests launch `iris-agentic-dev mcp` as a real subprocess, send JSON-RPC
//! over stdio, and assert on the response. No live IRIS required — tools that would
//! call IRIS will fail at the connection layer, but `tools/list` and `check_config`
//! work regardless.
//!
//! Run with:
//!   cargo test --test test_mcp_binary_config -- --include-ignored --test-threads=1

use std::io::Write;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

// ---------------------------------------------------------------------------
// Wire helpers
// ---------------------------------------------------------------------------

/// Spawn the binary in stdio MCP mode with optional --config path.
fn spawn_mcp(config: Option<&str>) -> (Child, ChildStdin, ChildStdout) {
    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp");
    if let Some(cfg) = config {
        cmd.args(["--config", cfg]);
    }
    // Unset IRIS env vars so tests don't pick up a live connection accidentally.
    // write_tools_enabled tests need a clean env.
    cmd.env_remove("IRIS_WRITE_TOOLS_ENABLED")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .env_remove("IRIS_ALLOW_PROD")
        .env_remove("IRIS_ENABLED_TOOLS")
        .env_remove("IRIS_DISABLED_TOOLS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().expect("failed to spawn iris-agentic-dev");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    (child, stdin, stdout)
}

/// Send a JSON-RPC initialize + notifications/initialized handshake.
fn send_initialize(stdin: &mut ChildStdin) {
    let init = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"0.0.1\"}}}\n";
    let notif = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n";
    stdin.write_all(init.as_bytes()).ok();
    stdin.write_all(notif.as_bytes()).ok();
}

/// Read newline-delimited JSON from stdout until `predicate` returns Some value, or timeout.
///
/// Spawns a reader thread so blocking reads don't hang the test process.
fn read_until<T, F>(stdout: ChildStdout, timeout_ms: u64, mut predicate: F) -> Option<T>
where
    T: Send + 'static,
    F: FnMut(&serde_json::Value) -> Option<T> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<T>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
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

/// Extract tool names from a `tools/list` response.
fn tools_list_names(v: &serde_json::Value) -> Option<Vec<String>> {
    let tools = v.get("result")?.get("tools")?.as_array()?;
    Some(
        tools
            .iter()
            .filter_map(|t| t.get("name")?.as_str().map(String::from))
            .collect(),
    )
}

/// Send `tools/list` and return the tool names.
fn get_tool_names(stdin: &mut ChildStdin, stdout: ChildStdout) -> Vec<String> {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n";
    stdin.write_all(req.as_bytes()).ok();
    read_until(stdout, 5000, tools_list_names).unwrap_or_default()
}

/// Send `check_config` tool call and return the parsed JSON result.
fn get_check_config(stdin: &mut ChildStdin, stdout: ChildStdout) -> serde_json::Value {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"check_config\",\"arguments\":{}}}\n";
    stdin.write_all(req.as_bytes()).ok();
    let result = read_until(stdout, 8000, |v| {
        let content = v.get("result")?.get("content")?.as_array()?;
        for c in content {
            if let Some(text) = c.get("text").and_then(|t| t.as_str()) {
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text) {
                    if obj.get("server_version").is_some() {
                        return Some(obj);
                    }
                }
            }
        }
        None
    });
    result.unwrap_or(serde_json::Value::Null)
}

// ---------------------------------------------------------------------------
// T-081-01: --config enabled_tools limits tools/list
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn config_file_enabled_tools_limits_tools_list() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        r#"enabled_tools = ["check_config", "iris_query"]"#,
    )
    .unwrap();

    let (mut child, mut stdin, stdout) = spawn_mcp(Some(&cfg_path.to_string_lossy()));
    send_initialize(&mut stdin);
    let names = get_tool_names(&mut stdin, stdout);
    child.kill().ok();
    let _ = child.wait();

    assert_eq!(
        names.len(),
        2,
        "expected exactly 2 tools, got {}: {:?}",
        names.len(),
        names
    );
    assert!(
        names.contains(&"check_config".to_string()),
        "check_config missing from tools list: {:?}",
        names
    );
    assert!(
        names.contains(&"iris_query".to_string()),
        "iris_query missing from tools list: {:?}",
        names
    );
}

// ---------------------------------------------------------------------------
// T-081-02: --config disabled_tools removes named tools
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn config_file_disabled_tools_removes_named_tools() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        r#"disabled_tools = ["agent_history", "agent_stats"]"#,
    )
    .unwrap();

    let (mut child, mut stdin, stdout) = spawn_mcp(Some(&cfg_path.to_string_lossy()));
    send_initialize(&mut stdin);
    let names = get_tool_names(&mut stdin, stdout);
    child.kill().ok();
    let _ = child.wait();

    assert!(
        !names.is_empty(),
        "tools/list returned empty — server likely failed to start"
    );
    assert!(
        !names.contains(&"agent_history".to_string()),
        "agent_history should have been removed by disabled_tools"
    );
    assert!(
        !names.contains(&"agent_stats".to_string()),
        "agent_stats should have been removed by disabled_tools"
    );
}

// ---------------------------------------------------------------------------
// T-081-03: --config write_tools_enabled=false reflected in check_config
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn config_file_write_tools_disabled_shown_in_check_config() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        "enabled_tools = [\"check_config\"]\nwrite_tools_enabled = false\n",
    )
    .unwrap();

    let (mut child, mut stdin, stdout) = spawn_mcp(Some(&cfg_path.to_string_lossy()));
    send_initialize(&mut stdin);
    let cfg = get_check_config(&mut stdin, stdout);
    child.kill().ok();
    let _ = child.wait();

    assert_eq!(
        cfg.get("write_tools_enabled").and_then(|v| v.as_bool()),
        Some(false),
        "expected write_tools_enabled=false in check_config output, got: {}",
        cfg
    );
}

// ---------------------------------------------------------------------------
// T-081-04: --config write_tools_enabled=true reflected in check_config
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn config_file_write_tools_enabled_shown_in_check_config() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        "enabled_tools = [\"check_config\"]\nwrite_tools_enabled = true\n",
    )
    .unwrap();

    let (mut child, mut stdin, stdout) = spawn_mcp(Some(&cfg_path.to_string_lossy()));
    send_initialize(&mut stdin);
    let cfg = get_check_config(&mut stdin, stdout);
    child.kill().ok();
    let _ = child.wait();

    assert_eq!(
        cfg.get("write_tools_enabled").and_then(|v| v.as_bool()),
        Some(true),
        "expected write_tools_enabled=true in check_config output, got: {}",
        cfg
    );
}

// ---------------------------------------------------------------------------
// T-081-05: destructive_tools_enabled=true + write_tools_enabled=false → error logged
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn config_file_destructive_requires_write_logs_error() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        "write_tools_enabled = false\ndestructive_tools_enabled = true\n",
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    // Capture stderr this time to assert the error message
    let mut child = Command::new(bin)
        .args(["mcp", "--config", &cfg_path.to_string_lossy()])
        .env_remove("IRIS_WRITE_TOOLS_ENABLED")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn iris-agentic-dev");

    // Send empty stdin — server will exit when connection closes
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("wait failed");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("DESTRUCTIVE_REQUIRES_WRITES"),
        "expected DESTRUCTIVE_REQUIRES_WRITES in stderr, got: {}",
        stderr
    );
}

// ---------------------------------------------------------------------------
// T-081-06: --workspace flag applies .iris-agentic-dev.toml from that dir
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn workspace_flag_applies_enabled_tools_from_toml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        r#"enabled_tools = ["check_config"]"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut child = Command::new(bin)
        .args(["mcp", "--workspace", &dir.path().to_string_lossy()])
        .env_remove("IRIS_WRITE_TOOLS_ENABLED")
        .env_remove("IRIS_ENABLED_TOOLS")
        .env_remove("IRIS_DISABLED_TOOLS")
        .env_remove("OBJECTSCRIPT_WORKSPACE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    send_initialize(&mut stdin);
    let names = get_tool_names(&mut stdin, stdout);
    child.kill().ok();
    let _ = child.wait();

    assert_eq!(
        names.len(),
        1,
        "expected exactly 1 tool via --workspace, got {}: {:?}",
        names.len(),
        names
    );
    assert!(
        names.contains(&"check_config".to_string()),
        "check_config missing from tools list: {:?}",
        names
    );
}

// ---------------------------------------------------------------------------
// T-081-07: no config → default toolset (~70+ tools)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn no_config_returns_default_toolset() {
    // Point --workspace at an empty temp dir so no .iris-agentic-dev.toml is found
    let dir = tempfile::tempdir().unwrap();

    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut child = Command::new(bin)
        .args(["mcp", "--workspace", &dir.path().to_string_lossy()])
        .env_remove("IRIS_WRITE_TOOLS_ENABLED")
        .env_remove("IRIS_ENABLED_TOOLS")
        .env_remove("IRIS_DISABLED_TOOLS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    send_initialize(&mut stdin);
    let names = get_tool_names(&mut stdin, stdout);
    child.kill().ok();
    let _ = child.wait();

    assert!(
        names.len() >= 70,
        "expected 70+ tools in default toolset, got {}: {:?}",
        names.len(),
        names
    );
}

// ---------------------------------------------------------------------------
// T-113-01: tools/list response omits outputSchema (#113 Cursor fix)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn tools_list_response_omits_output_schema() {
    // The full tools/list payload was ~220KB with outputSchema included, which caused
    // Cursor to silently register 0 tools (toolCount:0 bug, issue #113). The fix strips
    // outputSchema from the wire response; clients don't use it for tool registration.
    let dir = tempfile::tempdir().unwrap();

    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut child = Command::new(bin)
        .args(["mcp", "--workspace", &dir.path().to_string_lossy()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    send_initialize(&mut stdin);

    let req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n";
    stdin.write_all(req.as_bytes()).ok();

    let result = read_until(stdout, 5000, |v| {
        let tools = v.get("result")?.get("tools")?.as_array()?;
        let with_schema: Vec<String> = tools
            .iter()
            .filter_map(|t| {
                if t.get("outputSchema").is_some() {
                    t.get("name")?.as_str().map(String::from)
                } else {
                    None
                }
            })
            .collect();
        Some((tools.len(), with_schema))
    });

    child.kill().ok();
    let _ = child.wait();

    let (tool_count, tools_with_schema) = result.expect("no tools/list response received");
    assert!(
        tool_count >= 70,
        "expected 70+ tools in tools/list, got {tool_count}"
    );
    assert!(
        tools_with_schema.is_empty(),
        "tools/list must not include outputSchema (Cursor #113 regression): {:?}",
        tools_with_schema
    );
}

// ---------------------------------------------------------------------------
// T-083-01: iris_debug capture never returns DOCKER_REQUIRED (#98)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn iris_debug_capture_no_docker_required() {
    // With no IRIS_CONTAINER set, iris_debug(action="capture") must not return
    // DOCKER_REQUIRED. It will fail with a connection error (IRIS_UNREACHABLE or
    // similar), but the DOCKER_REQUIRED bail-out must be gone.
    let dir = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut child = Command::new(bin)
        .args(["mcp", "--workspace", &dir.path().to_string_lossy()])
        .env_remove("IRIS_CONTAINER")
        .env_remove("IRIS_HOST")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    send_initialize(&mut stdin);

    let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_debug","arguments":{"action":"capture"}}}"#;
    stdin.write_all(req.as_bytes()).ok();
    stdin.write_all(b"\n").ok();

    let result = read_until::<serde_json::Value, _>(stdout, 8000, |v| {
        if v.get("id")?.as_u64()? == 2 {
            Some(v.clone())
        } else {
            None
        }
    });

    child.kill().ok();
    let _ = child.wait();

    let response = result.expect("no tools/call response received");
    let body = serde_json::to_string(&response).unwrap();
    assert!(
        !body.contains("DOCKER_REQUIRED"),
        "iris_debug capture must not return DOCKER_REQUIRED on HTTP-only spawn (#98 regression): {body}"
    );
}

// ---------------------------------------------------------------------------
// T-082-01: iris_production inputSchema includes `namespace` (#103)
// ---------------------------------------------------------------------------

/// Send `tools/list` and return the full tool object for the given name.
fn get_tool_schema(
    stdin: &mut ChildStdin,
    stdout: ChildStdout,
    tool_name: &str,
) -> Option<serde_json::Value> {
    let req = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n";
    stdin.write_all(req.as_bytes()).ok();
    let name = tool_name.to_string();
    read_until(stdout, 10000, move |v| {
        let tools = v.get("result")?.get("tools")?.as_array()?;
        tools
            .iter()
            .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(name.as_str()))
            .cloned()
    })
}

#[test]
#[ignore]
fn iris_production_input_schema_has_namespace() {
    let (mut child, mut stdin, stdout) = spawn_mcp(None);
    send_initialize(&mut stdin);
    let tool = get_tool_schema(&mut stdin, stdout, "iris_production");
    child.kill().ok();
    let _ = child.wait();

    let tool = tool.expect("iris_production not found in tools/list");
    let props = tool
        .get("inputSchema")
        .and_then(|s| s.get("properties"))
        .and_then(|p| p.as_object())
        .expect("iris_production inputSchema must have properties");

    assert!(
        props.contains_key("namespace"),
        "iris_production inputSchema must document 'namespace' (#103); got keys: {:?}",
        props.keys().collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// T-no-skills-01: --no-skills removes all skill/KB/agent tools from tools/list
// T-no-skills-02: IRIS_NO_SKILLS=1 env var has the same effect
// ---------------------------------------------------------------------------

const SKILL_TOOLS: &[&str] = &[
    "skill",
    "skill_list",
    "skill_describe",
    "skill_search",
    "skill_forget",
    "skill_propose",
    "skill_optimize",
    "skill_share",
    "skill_community",
    "skill_community_list",
    "skill_community_install",
    "kb_index",
    "kb_recall",
    "agent_history",
    "agent_stats",
];

/// Spawn the binary in stdio MCP mode with extra arguments appended after `mcp`.
fn spawn_mcp_extra(
    extra_args: &[&str],
    extra_env: &[(&str, &str)],
) -> (Child, ChildStdin, ChildStdout) {
    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.env_remove("IRIS_WRITE_TOOLS_ENABLED")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .env_remove("IRIS_ALLOW_PROD")
        .env_remove("IRIS_ENABLED_TOOLS")
        .env_remove("IRIS_DISABLED_TOOLS")
        .env_remove("IRIS_NO_SKILLS");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().expect("failed to spawn iris-agentic-dev");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    (child, stdin, stdout)
}

#[test]
#[ignore]
fn no_skills_flag_removes_skill_tools() {
    let (mut child, mut stdin, stdout) = spawn_mcp_extra(&["--no-skills"], &[]);
    send_initialize(&mut stdin);
    let names = get_tool_names(&mut stdin, stdout);
    child.kill().ok();
    let _ = child.wait();

    assert!(
        !names.is_empty(),
        "tools/list returned empty — server likely failed to start"
    );
    for tool in SKILL_TOOLS {
        assert!(
            !names.contains(&tool.to_string()),
            "--no-skills: tool '{}' should be absent from tools/list",
            tool
        );
    }
    assert!(
        names.contains(&"check_config".to_string()),
        "--no-skills: check_config should still be present"
    );
}

#[test]
#[ignore]
fn no_skills_env_var_removes_skill_tools() {
    let (mut child, mut stdin, stdout) = spawn_mcp_extra(&[], &[("IRIS_NO_SKILLS", "true")]);
    send_initialize(&mut stdin);
    let names = get_tool_names(&mut stdin, stdout);
    child.kill().ok();
    let _ = child.wait();

    assert!(
        !names.is_empty(),
        "tools/list returned empty — server likely failed to start"
    );
    for tool in SKILL_TOOLS {
        assert!(
            !names.contains(&tool.to_string()),
            "IRIS_NO_SKILLS=1: tool '{}' should be absent from tools/list",
            tool
        );
    }
}

// ---------------------------------------------------------------------------
// T-write-gate-01: write_tools_enabled=false blocks iris_compile, iris_execute,
//                  iris_doc put, and iris_query write.
// No live IRIS required — the write gate fires before any connection is used.
// ---------------------------------------------------------------------------

/// Call a tool with the given JSON arguments and return the first text content.
fn call_tool(stdin: &mut ChildStdin, stdout: ChildStdout, name: &str, args: &str) -> String {
    let req = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{{\"name\":\"{name}\",\"arguments\":{args}}}}}\n"
    );
    stdin.write_all(req.as_bytes()).ok();
    let result = read_until(stdout, 8000, |v| {
        let content = v.get("result")?.get("content")?.as_array()?;
        for c in content {
            if let Some(text) = c.get("text").and_then(|t| t.as_str()) {
                return Some(text.to_string());
            }
        }
        None
    });
    result.unwrap_or_default()
}

fn assert_write_gate_error(text: &str, tool: &str) {
    let v: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|_| panic!("{tool}: response not valid JSON: {text}"));
    assert_eq!(
        v["error_code"].as_str().unwrap_or(""),
        "WRITE_TOOLS_DISABLED",
        "{tool}: expected WRITE_TOOLS_DISABLED, got: {v}"
    );
}

#[test]
#[ignore]
fn write_tools_disabled_blocks_iris_compile() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        "enabled_tools = [\"iris_compile\"]\nwrite_tools_enabled = false\n",
    )
    .unwrap();

    let (mut child, mut stdin, stdout) = spawn_mcp(Some(&cfg_path.to_string_lossy()));
    send_initialize(&mut stdin);
    let text = call_tool(
        &mut stdin,
        stdout,
        "iris_compile",
        r#"{"target":"App.Foo.cls"}"#,
    );
    child.kill().ok();
    let _ = child.wait();
    assert_write_gate_error(&text, "iris_compile");
}

#[test]
#[ignore]
fn write_tools_disabled_blocks_iris_execute() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        "enabled_tools = [\"iris_execute\"]\nwrite_tools_enabled = false\n",
    )
    .unwrap();

    let (mut child, mut stdin, stdout) = spawn_mcp(Some(&cfg_path.to_string_lossy()));
    send_initialize(&mut stdin);
    let text = call_tool(&mut stdin, stdout, "iris_execute", r#"{"code":"Write 1"}"#);
    child.kill().ok();
    let _ = child.wait();
    assert_write_gate_error(&text, "iris_execute");
}

#[test]
#[ignore]
fn write_tools_disabled_blocks_iris_doc_put() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        "enabled_tools = [\"iris_doc\"]\nwrite_tools_enabled = false\n",
    )
    .unwrap();

    let (mut child, mut stdin, stdout) = spawn_mcp(Some(&cfg_path.to_string_lossy()));
    send_initialize(&mut stdin);
    let text = call_tool(
        &mut stdin,
        stdout,
        "iris_doc",
        r#"{"mode":"put","name":"App.Foo.cls","content":"Class App.Foo {}"}"#,
    );
    child.kill().ok();
    let _ = child.wait();
    assert_write_gate_error(&text, "iris_doc put");
}

#[test]
#[ignore]
fn write_tools_disabled_blocks_iris_query_write() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(
        &cfg_path,
        "enabled_tools = [\"iris_query\"]\nwrite_tools_enabled = false\n",
    )
    .unwrap();

    let (mut child, mut stdin, stdout) = spawn_mcp(Some(&cfg_path.to_string_lossy()));
    send_initialize(&mut stdin);
    let text = call_tool(
        &mut stdin,
        stdout,
        "iris_query",
        r#"{"mode":"write","query":"INSERT INTO Sample.Person (Name) VALUES ('Test')"}"#,
    );
    child.kill().ok();
    let _ = child.wait();
    assert_write_gate_error(&text, "iris_query write");
}
