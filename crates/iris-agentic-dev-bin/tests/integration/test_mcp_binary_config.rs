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
// T-081-05 / T039 (085): destructive_tools_enabled=true + write_tools_enabled=false
// must refuse to start — not log and carry on.
// ---------------------------------------------------------------------------

/// Start the binary against a contradictory gate config, attempt a handshake, and return
/// `(exit code, stdout, stderr)`.
///
/// The handshake is attempted on purpose: a server that exits before speaking MCP is the
/// observable difference between "refused to start" and "logged a warning and served requests".
/// Writes to a dead child's stdin fail with `EPIPE`, which is the expected outcome here, so send
/// errors are ignored and the assertion is made on what came back.
fn run_with_gate_config(args: &[&str], cwd: &std::path::Path) -> (Option<i32>, String, String) {
    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp")
        .args(args)
        .current_dir(cwd)
        // An operator env var would legitimately outrank the file, and that is a different test.
        .env_remove("IRIS_WRITE_TOOLS_ENABLED")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .env_remove("IRIS_TOOLSET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("failed to spawn iris-agentic-dev");

    let mut stdin = child.stdin.take().unwrap();
    send_initialize(&mut stdin);
    drop(stdin);

    let out = child.wait_with_output().expect("wait failed");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// T039 (FR-005, FR-006, SC-004). The contradictory combination must produce exit 2 and no session.
///
/// The test this replaced asserted only that `DESTRUCTIVE_REQUIRES_WRITES` appeared on stderr — and
/// it passed for two releases while the server did the *opposite* of what it logged: the warning
/// sat above a `return None` in the config loader, which skipped the gate export and left
/// `is_write_allowed()` falling through to the namespace heuristic, so `USER` came back `true` and
/// writes were on. A log line is not a behavior. Exit code and the absence of a session are.
///
/// Both config entry points are checked, because they are separate call sites in `mcp.rs`
/// (`--config` → `apply_explicit_config_file`, discovery → `apply_workspace_config_with_path`) and
/// a validation call wired into one of them would leave the other exactly as it shipped.
#[test]
#[ignore]
fn destructive_requires_writes_exits_two_without_serving() {
    let contradiction = "write_tools_enabled = false\ndestructive_tools_enabled = true\n";

    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("cfg.toml");
    std::fs::write(&cfg_path, contradiction).unwrap();
    let explicit = cfg_path.to_string_lossy().to_string();

    // Discovery path: the file has to be named .iris-agentic-dev.toml and found from --workspace.
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join(".iris-agentic-dev.toml"), contradiction).unwrap();

    for (label, args) in [
        ("--config", vec!["--config", explicit.as_str()]),
        ("--workspace discovery", vec!["--workspace", "."]),
    ] {
        let cwd = if label == "--config" {
            dir.path()
        } else {
            ws.path()
        };
        let (code, stdout, stderr) = run_with_gate_config(&args, cwd);

        assert_eq!(
            code,
            Some(2),
            "[{label}] expected exit 2, got {code:?}. docs/tools.md and \
             specs/073-destructive-gate promise iad refuses to start on this combination.\n\
             stderr: {stderr}\nstdout: {stdout}"
        );
        assert!(
            stderr.contains("DESTRUCTIVE_REQUIRES_WRITES"),
            "[{label}] the refusal must name the code an operator can search for; stderr: {stderr}"
        );
        // No MCP session. `initialize` was sent; a reply to it means the server came up and served,
        // which is the defect regardless of what the exit code says afterward.
        assert!(
            !stdout.contains("serverInfo") && !stdout.contains("protocolVersion"),
            "[{label}] the server answered initialize before exiting — it served a session under a \
             configuration it refused. stdout: {stdout}"
        );
        assert!(
            stdout.trim().is_empty(),
            "[{label}] expected nothing on stdout, got: {stdout}"
        );
    }
}

/// The inverse, so the exit above cannot be a blanket refusal of the destructive key. This is the
/// configuration an operator who wants the destructive tier actually writes, and it must start.
#[test]
#[ignore]
fn destructive_with_writes_on_starts_normally() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n",
    )
    .unwrap();

    let (code, stdout, stderr) = run_with_gate_config(&["--workspace", "."], dir.path());
    assert_eq!(
        code,
        Some(0),
        "writes on + destructive on is a valid configuration and must start; stderr: {stderr}"
    );
    assert!(
        stdout.contains("serverInfo"),
        "expected an initialize reply; stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("DESTRUCTIVE_REQUIRES_WRITES"),
        "nothing to refuse here; stderr: {stderr}"
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

// ---------------------------------------------------------------------------
// T025 (085): one server process, config rewritten three times
// ---------------------------------------------------------------------------

/// A live MCP session that survives more than one request.
///
/// Every helper above consumes `stdout` (it moves into a reader thread), so each existing test in
/// this file gets exactly one round trip and then kills the child. That is a large part of why the
/// rewrite-twice case was never written: defects 1, 2 and 3 of spec 085 all live in the *second*
/// config load inside one process, and a cold start writes the file once and can never reach them.
struct Session {
    child: Child,
    stdin: ChildStdin,
    reader: std::io::BufReader<ChildStdout>,
    next_id: u64,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

impl Session {
    /// Spawn with workspace discovery — deliberately *not* `--config`.
    ///
    /// `check_reload`'s watcher is what is under test, and it watches
    /// `workspace_root()/.iris-agentic-dev.toml`. `OBJECTSCRIPT_WORKSPACE` is the one input
    /// `workspace_root` consults before anything else, so it pins the watch path without depending
    /// on the test process's cwd.
    fn start(workspace: &std::path::Path) -> Self {
        Self::start_with_env(workspace, &[])
    }

    /// `start`, plus environment overrides applied last — used to serve a different `IRIS_TOOLSET`
    /// from the same fixture.
    fn start_with_env(workspace: &std::path::Path, extra_env: &[(&str, &str)]) -> Self {
        let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
        let mut cmd = Command::new(bin);
        cmd.arg("mcp")
            .current_dir(workspace)
            .env("OBJECTSCRIPT_WORKSPACE", workspace)
            // An operator env var outranks the config file (FR-003), so one left in the developer's
            // shell would quietly become the thing under test instead of the file.
            .env_remove("IRIS_WRITE_TOOLS_ENABLED")
            .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
            .env_remove("IRIS_ALLOW_PROD")
            .env_remove("IRIS_ENABLED_TOOLS")
            .env_remove("IRIS_DISABLED_TOOLS")
            .env_remove("IRIS_CONTAINER")
            .env_remove("IRIS_TOOLSET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("failed to spawn iris-agentic-dev");
        let stdin = child.stdin.take().unwrap();
        let reader = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut s = Session {
            child,
            stdin,
            reader,
            next_id: 1,
        };
        s.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "t025", "version": "0.0.1"},
            }),
        );
        s.stdin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n",
            )
            .expect("initialized notification");
        s.stdin.flush().ok();
        s
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        use std::io::BufRead;
        let id = self.next_id;
        self.next_id += 1;
        let line = serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        })
        .to_string();
        self.stdin
            .write_all(line.as_bytes())
            .expect("write request");
        self.stdin.write_all(b"\n").expect("write newline");
        self.stdin.flush().ok();
        loop {
            let mut buf = String::new();
            let n = self.reader.read_line(&mut buf).expect("read response");
            assert!(n > 0, "server closed stdout before answering {method}");
            let Ok(v) = serde_json::from_str::<serde_json::Value>(buf.trim()) else {
                continue;
            };
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                return v;
            }
        }
    }

    /// Call a tool and return its decoded payload — structured content when present, otherwise the
    /// text block, and `Null` for a JSON-RPC-level error.
    fn call(&mut self, tool: &str, args: serde_json::Value) -> serde_json::Value {
        let v = self.request(
            "tools/call",
            serde_json::json!({"name": tool, "arguments": args}),
        );
        if let Some(sc) = v.pointer("/result/structuredContent") {
            if !sc.is_null() {
                return sc.clone();
            }
        }
        v.pointer("/result/content/0/text")
            .and_then(|t| t.as_str())
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or(serde_json::Value::Null)
    }
}

/// Write the config and push its mtime forward.
///
/// `ConfigWatcher::has_changed` compares `new > old` on the mtime, so two writes inside one
/// filesystem timestamp tick look like no change at all — the test would then be measuring clock
/// resolution rather than the reload. `File::set_modified` makes the bump explicit instead of
/// sleeping a second per step.
fn write_config_and_bump_mtime(path: &std::path::Path, body: &str, tick: u64) {
    std::fs::write(path, body).expect("write config");
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("reopen config");
    let when = std::time::SystemTime::now() + std::time::Duration::from_secs(10 * (tick + 1));
    f.set_modified(when).expect("bump mtime");
}

/// T025 (FR-002, FR-023, SC-002, SC-003). One process, the config rewritten `true` → `false` →
/// `true`, asserting at every step that the *reported* gate and an *attempted write* agree.
///
/// This is the test whose absence let three defects ship together. The old code exported the config
/// value into `IRIS_WRITE_TOOLS_ENABLED` only when that variable was unset, so the first load won
/// permanently: step 2 reported `true` while the operator had asked for `false`, and step 3 could
/// not distinguish a working reload from a stuck one. Every other test in this file passes
/// `--config` and reads once, which is a single cold start — the branch holding the bug is
/// unreachable that way.
///
/// The write attempt matters as much as the report. `check_config` alone would have passed against
/// the broken build at step 3 by coincidence, because the value it was stuck on happened to be the
/// wanted one.
#[test]
#[ignore]
fn config_rewritten_twice_in_one_process_moves_the_gate_both_ways() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join(".iris-agentic-dev.toml");

    // The live dev container. The gate is decided before dispatch, so a refusal needs no IRIS at
    // all — but the *permitted* half of each step has to reach a real server, otherwise "not
    // refused" would be indistinguishable from "could not connect".
    let toml = |writes: bool| {
        format!(
            "host = \"localhost\"\nport = 52780\nnamespace = \"USER\"\n\
             username = \"_SYSTEM\"\npassword = \"SYS\"\nwrite_tools_enabled = {writes}\n"
        )
    };

    std::fs::write(&cfg, toml(true)).unwrap();
    let mut s = Session::start(dir.path());

    for (tick, declared) in [true, false, true].into_iter().enumerate() {
        let step = match tick {
            0 => "start: writes on",
            1 => "edit 1: writes off",
            _ => "edit 2: writes back on",
        };
        write_config_and_bump_mtime(&cfg, &toml(declared), tick as u64);

        let report = s.call("check_config", serde_json::json!({}));
        assert_eq!(
            report["write_tools_enabled"],
            serde_json::json!(declared),
            "{step}: check_config must follow the file. It reported {:?} with the file declaring \
             {declared}, which means the config-to-gate mapping is not idempotent (defect 1). \
             Full report: {report}",
            report["write_tools_enabled"]
        );
        assert_eq!(
            report["write_tools_source"],
            serde_json::json!("config_file"),
            "{step}: the file declared the gate, so the reported source must say so: {report}"
        );

        // Now the half that reporting cannot fake. `iris_global` set was one of the live-verified
        // bypasses: with the gate off it returned success while check_config said false.
        let attempted = s.call(
            "iris_global",
            serde_json::json!({
                "action": "set",
                "global_name": "IADGate085Reload",
                "subscripts": ["t025"],
                "value": "1",
            }),
        );
        let refused = attempted["error_code"] == serde_json::json!("WRITE_TOOLS_DISABLED");
        assert_eq!(
            refused,
            !declared,
            "{step}: the write attempt disagrees with the report. Writes were declared {declared}, \
             so the call should have been {}. Got: {attempted}",
            if declared {
                "dispatched to IRIS"
            } else {
                "refused with WRITE_TOOLS_DISABLED"
            }
        );
    }
}

/// Page through `tools/list` and return every advertised tool name.
///
/// The list paginates (`next_cursor`), so a single request returns a prefix — asserting over one
/// page would silently stop covering whatever fell past the page boundary.
fn list_all_tools(s: &mut Session) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let mut cursor: Option<String> = None;
    loop {
        let params = match &cursor {
            Some(c) => serde_json::json!({"cursor": c}),
            None => serde_json::json!({}),
        };
        let v = s.request("tools/list", params);
        let page = v
            .pointer("/result/tools")
            .and_then(|t| t.as_array().cloned())
            .unwrap_or_else(|| panic!("tools/list returned no tools array: {v}"));
        for t in page {
            if let Some(n) = t.get("name").and_then(|n| n.as_str()) {
                names.insert(n.to_string());
            }
        }
        cursor = v
            .pointer("/result/nextCursor")
            .and_then(|c| c.as_str())
            .map(str::to_string);
        if cursor.is_none() {
            return names;
        }
    }
}

/// T036 (FR-026, US3 scenario 2). With the gate off, *every* write-capable tool in
/// `CLASSIFICATION` is called over stdio and must answer `WRITE_TOOLS_DISABLED`.
///
/// The table is the test's input, so there is no per-tool test to forget: classifying a new tool
/// `wr`/`de` automatically enrolls it here, and a tool that reaches dispatch anyway shows up by
/// name. That is the half the reporter's four ungated tools needed — each one had a gate somewhere
/// in the codebase and no test that walked the whole surface.
///
/// No live IRIS. `gate_check` runs in `call_tool` before the connection is resolved and before the
/// handler deserializes its arguments, so the refusal does not depend on a container being up, or
/// even on the call's other arguments being valid. That is deliberate: this test has to run on
/// every CI job, not only the ones with a container.
#[test]
#[ignore] // Requires the built binary; CI runs with --include-ignored.
fn every_write_capable_tool_refuses_with_the_gate_off() {
    use iris_agentic_dev_core::tools::write_gate::{WriteClass, CLASSIFICATION, ERR_WRITE_GATE};

    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = dir.path().join(".iris-agentic-dev.toml");
    // Port 1 never answers, so nothing here can reach IRIS even if a gate leaked. The gate is
    // resolved from the parsed declaration, not from the connection, so `write_tools_enabled =
    // false` still lands as source `config_file` with no server on the other end.
    std::fs::write(
        &cfg,
        "host = \"127.0.0.1\"\nport = 1\nnamespace = \"USER\"\nusername = \"_SYSTEM\"\n\
         password = \"SYS\"\nwrite_tools_enabled = false\n",
    )
    .expect("write config");

    let mut failures: Vec<String> = Vec::new();
    let mut exercised = 0usize;
    let mut never_advertised: std::collections::BTreeSet<&str> =
        CLASSIFICATION.iter().map(|e| e.tool).collect();

    // Both tool surfaces, because each prunes tools the other keeps: merged drops the four
    // `debug_*` tools and the container trio into dispatchers, baseline is the only tier that still
    // advertises the four skill stubs. Testing one tier would leave the other's exclusive tools
    // enrolled in the table and never called.
    for toolset in ["merged", "baseline"] {
        let mut s = Session::start_with_env(dir.path(), &[("IRIS_TOOLSET", toolset)]);
        let report = s.call("check_config", serde_json::json!({}));
        assert_eq!(
            report["write_tools_enabled"],
            serde_json::json!(false),
            "{toolset}: the fixture is only meaningful with the gate off; check_config says: \
             {report}"
        );

        let advertised = list_all_tools(&mut s);
        for entry in CLASSIFICATION {
            // A tool this toolset does not advertise cannot be called here — but it must be
            // callable in *some* tier, or the table names something unreachable, which the
            // core-crate reverse-completeness test would already have caught.
            if !advertised.contains(entry.tool) {
                continue;
            }
            never_advertised.remove(entry.tool);

            // Every way to reach a non-read class: the default call, plus each mutating action. The
            // action is passed under `action`, which `classify` consults for every tool — including
            // the two whose real parameter is `mode` (`iris_doc`, `iris_query`), since it checks
            // both.
            let mut cases: Vec<(WriteClass, serde_json::Value)> = Vec::new();
            if entry.default != WriteClass::ReadOnly {
                cases.push((entry.default, serde_json::json!({})));
            }
            for (action, class) in entry.actions {
                if *class != WriteClass::ReadOnly {
                    cases.push((*class, serde_json::json!({"action": action})));
                }
            }

            for (class, args) in cases {
                exercised += 1;
                // The raw envelope, not the decoded payload: a call that got past the gate and then
                // failed on its own arguments has no payload to show, and "answered null" would
                // hide which of the two happened.
                let raw = s.request(
                    "tools/call",
                    serde_json::json!({"name": entry.tool, "arguments": args}),
                );
                let code = raw
                    .pointer("/result/structuredContent/error_code")
                    .cloned()
                    .or_else(|| {
                        raw.pointer("/result/content/0/text")
                            .and_then(|t| t.as_str())
                            .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
                            .and_then(|v| v.get("error_code").cloned())
                    });
                if code.as_ref() != Some(&serde_json::json!(ERR_WRITE_GATE)) {
                    failures.push(format!(
                        "[{toolset}] {} {args} is classified {class:?} but answered {}",
                        entry.tool,
                        serde_json::to_string(&raw).unwrap_or_default()
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} write-capable call(s) were not refused by the write gate. Each one is reachable with \
         writes off:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );

    // Non-vacuity. If the table were emptied, the classification renamed, or the session silently
    // failing to answer, the loop above would find nothing to complain about. The floor sits under
    // the present count, so reclassifying a tool downward is fine and losing the enumeration is not.
    assert!(
        exercised >= 60,
        "only {exercised} write-capable call(s) were exercised (expected 60+) — the table drove \
         almost nothing"
    );
    // And every classified tool has to be advertised by at least one of the two tiers, or it is
    // enrolled in the table and never actually called here.
    assert!(
        never_advertised.is_empty(),
        "{} classified tool(s) are advertised by neither merged nor baseline, so this test never \
         calls them: {never_advertised:?}",
        never_advertised.len()
    );
}
