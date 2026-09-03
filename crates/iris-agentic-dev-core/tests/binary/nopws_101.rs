// 101-nopws-connectivity: Binary invocation tests (Layer 2).
// These spawn the IAD binary and verify MCP protocol responses contain NoPWS fields.
// Run with: IAD_BINARY=./target/debug/iris-agentic-dev cargo test --test nopws_101_binary -- --include-ignored

use std::io::Write;
use std::process::{Command, Stdio};

fn iad_binary() -> String {
    std::env::var("IAD_BINARY").unwrap_or_else(|_| "./target/debug/iris-agentic-dev".to_string())
}

fn binary_exists() -> bool {
    std::path::Path::new(&iad_binary()).exists()
}

/// FR-012: Binary invocation test — spawn binary, call tools/list, assert iris_execute is listed.
#[ignore]
#[test]
fn test_binary_tools_list_includes_iris_execute() {
    if !binary_exists() {
        eprintln!("IAD_BINARY not found at {}, skipping", iad_binary());
        return;
    }

    let mut child = Command::new(iad_binary())
        .env("IRIS_HOST", "localhost")
        .env("IRIS_WEB_PORT", "52780")
        .env("IRIS_USERNAME", "_SYSTEM")
        .env("IRIS_PASSWORD", "SYS")
        .env("IRIS_NAMESPACE", "USER")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("must spawn binary");

    let stdin = child.stdin.as_mut().unwrap();

    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }
    });
    writeln!(stdin, "{}", serde_json::to_string(&init).unwrap()).unwrap();

    let list = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    writeln!(stdin, "{}", serde_json::to_string(&list).unwrap()).unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("must wait");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut found_iris_execute = false;
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["id"] == 2 {
                if let Some(tools) = v["result"]["tools"].as_array() {
                    for tool in tools {
                        if tool["name"].as_str() == Some("iris_execute") {
                            found_iris_execute = true;
                        }
                    }
                }
            }
        }
    }

    assert!(
        found_iris_execute,
        "tools/list must include iris_execute. stdout:\n{stdout}"
    );
}

/// FR-012: Binary test — iris_test_server response must include nopws_detected field.
/// This validates the binary returns the expected NoPWS fields in the tool response schema.
#[ignore]
#[test]
fn test_binary_iris_execute_has_execution_path_field_in_docker_only_mode() {
    if !binary_exists() {
        eprintln!("IAD_BINARY not found at {}, skipping", iad_binary());
        return;
    }

    use tempfile::TempDir;

    // Create a temp config with docker_only=true to force docker exec path
    let dir = TempDir::new().unwrap();
    let config_content = format!(
        r#"
container = "iris-dev-iris"
namespace = "USER"
nopws = true
docker_only = true
"#
    );
    let config_path = dir.path().join(".iris-agentic-dev.toml");
    std::fs::write(&config_path, config_content).unwrap();

    let mut child = Command::new(iad_binary())
        .env("OBJECTSCRIPT_WORKSPACE", dir.path())
        .env("IRIS_USERNAME", "_SYSTEM")
        .env("IRIS_PASSWORD", "SYS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("must spawn binary");

    let stdin = child.stdin.as_mut().unwrap();

    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }
    });
    writeln!(stdin, "{}", serde_json::to_string(&init).unwrap()).unwrap();

    let call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "iris_execute",
            "arguments": {"code": "Write 1"}
        }
    });
    writeln!(stdin, "{}", serde_json::to_string(&call).unwrap()).unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("must wait");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut found_execution_path = false;
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["id"] == 2 {
                if let Some(arr) = v["result"]["content"].as_array() {
                    for item in arr {
                        if let Some(text) = item["text"].as_str() {
                            if let Ok(resp) = serde_json::from_str::<serde_json::Value>(text) {
                                if resp["execution_path"].is_string() {
                                    found_execution_path = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(
        found_execution_path,
        "iris_execute in docker_only mode must include execution_path field. stdout:\n{stdout}"
    );
}

/// FR-016: Binary test — iris_compile response in docker_only mode must include execution_path.
#[ignore]
#[test]
fn test_binary_iris_compile_has_execution_path_in_docker_only_mode() {
    if !binary_exists() {
        eprintln!("IAD_BINARY not found at {}, skipping", iad_binary());
        return;
    }

    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let config_content = r#"
container = "iris-dev-iris"
namespace = "USER"
nopws = true
docker_only = true
"#;
    let config_path = dir.path().join(".iris-agentic-dev.toml");
    std::fs::write(&config_path, config_content).unwrap();

    let mut child = Command::new(iad_binary())
        .env("OBJECTSCRIPT_WORKSPACE", dir.path())
        .env("IRIS_USERNAME", "_SYSTEM")
        .env("IRIS_PASSWORD", "SYS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("must spawn binary");

    let stdin = child.stdin.as_mut().unwrap();

    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }
    });
    writeln!(stdin, "{}", serde_json::to_string(&init).unwrap()).unwrap();

    let call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "iris_compile",
            "arguments": {"target": "User.TestNoPWSClass.cls"}
        }
    });
    writeln!(stdin, "{}", serde_json::to_string(&call).unwrap()).unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("must wait");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut found_execution_path = false;
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["id"] == 2 {
                if let Some(arr) = v["result"]["content"].as_array() {
                    for item in arr {
                        if let Some(text) = item["text"].as_str() {
                            if let Ok(resp) = serde_json::from_str::<serde_json::Value>(text) {
                                if resp["execution_path"].is_string() {
                                    found_execution_path = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(
        found_execution_path,
        "iris_compile in docker_only mode must include execution_path field. stdout:\n{stdout}"
    );
}
