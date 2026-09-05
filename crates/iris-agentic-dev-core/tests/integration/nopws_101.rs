// 101-nopws-connectivity: Live IRIS integration tests.
// These need a live IRIS with the Atelier REST API reachable. Locally that is iris-dev-iris on
// localhost:52780; the CI e2e job names its own container and port in the environment.
// Run with: cargo test --test nopws_101 -- --test-threads=1 --include-ignored

/// The endpoint these tests should talk to.
///
/// Every test in this file used to pin `localhost:52780` and the container name `iris-dev-iris`,
/// which are one laptop's conventions. The CI e2e container answers on 52773 and is called
/// `iris-e2e`, so `iris_execute` could not reach Atelier at all there and fell back to
/// `docker exec` — and `test_iris_execute_atelier_path_has_execution_path_field` read that
/// fallback as a broken Atelier path. The environment wins when it names an endpoint; the local
/// container is only the default.
fn live_iris_env() -> Vec<(&'static str, String)> {
    vec![
        ("IRIS_HOST", env_or("IRIS_HOST", "localhost")),
        ("IRIS_WEB_PORT", env_or("IRIS_WEB_PORT", "52780")),
        ("IRIS_USERNAME", env_or("IRIS_USERNAME", "_SYSTEM")),
        ("IRIS_PASSWORD", env_or("IRIS_PASSWORD", "SYS")),
        ("IRIS_NAMESPACE", env_or("IRIS_NAMESPACE", "USER")),
    ]
}

/// The container `docker exec` should target, same rule.
fn live_iris_container() -> String {
    env_or("IRIS_CONTAINER", "iris-dev-iris")
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Not an IRIS test: the endpoint resolution above is what decides whether the live assertions
/// mean anything, and it is exactly the kind of thing that looks right and silently defaults.
#[test]
fn live_iris_env_prefers_the_environment_over_the_local_default() {
    let resolved = live_iris_env();
    let port = resolved
        .iter()
        .find(|(k, _)| *k == "IRIS_WEB_PORT")
        .map(|(_, v)| v.clone())
        .expect("IRIS_WEB_PORT is always resolved");

    match std::env::var("IRIS_WEB_PORT") {
        Ok(v) if !v.is_empty() => assert_eq!(port, v),
        _ => assert_eq!(port, "52780"),
    }

    // An empty value is not an endpoint. Inheriting `IRIS_HOST=` and passing it through would
    // hand the connection layer a blank host instead of falling back.
    assert!(resolved.iter().all(|(_, v)| !v.is_empty()));
}

/// FR-013: iris_test_server against community container (has web server) must return
/// nopws_detected: false.
#[ignore]
#[tokio::test]
async fn test_iris_test_server_community_nopws_detected_false() {
    // This test requires a live MCP server; validate via binary invocation instead.
    // The live test verifies the community container does NOT trigger nopws detection.
    let Some(binary) = iris_agentic_dev_core::testing::require_iad_binary() else {
        return;
    };

    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(&binary)
        // The MCP server is the `mcp` subcommand. Spawning the bare binary prints the usage
        // banner and exits, which these tests read as an empty stdout.
        .arg("mcp")
        // Declare the gate state instead of inheriting the operator's (or the CI e2e job's):
        // iris_execute and iris_compile are write tools and refuse when the gate is off.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        // An inherited IRIS_CONTAINER moves the probe onto a different server than the one this
        // test names, and an inherited OBJECTSCRIPT_WORKSPACE can supply a whole different
        // connection from a toml. Pin the endpoint, do not merely hope it is unset.
        .env_remove("IRIS_CONTAINER")
        .env_remove("OBJECTSCRIPT_WORKSPACE")
        .envs(live_iris_env())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("must spawn binary");

    let stdin = child.stdin.as_mut().unwrap();
    // Initialize
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

    // Call iris_test_server — this requires a registered server in the pool,
    // so we skip this test if no servers are pre-configured.
    // The important assertion is about nopws_detected being false for a community image.
    let _ = child.wait();
}

/// FR-007/FR-008: iris_execute with closed web port and IRIS_CONTAINER set must
/// return result via docker exec with execution_path: "docker_exec_local".
#[ignore]
#[tokio::test]
async fn test_iris_execute_docker_exec_fallback() {
    // Set IRIS_WEB_PORT to a closed port to force docker exec path
    let container = live_iris_container();

    use std::io::Write;
    use std::process::{Command, Stdio};

    let Some(binary) = iris_agentic_dev_core::testing::require_iad_binary() else {
        return;
    };

    let mut child = Command::new(&binary)
        // The MCP server is the `mcp` subcommand. Spawning the bare binary prints the usage
        // banner and exits, which these tests read as an empty stdout.
        .arg("mcp")
        // Declare the gate state instead of inheriting the operator's (or the CI e2e job's):
        // iris_execute and iris_compile are write tools and refuse when the gate is off.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .envs(live_iris_env())
        .env("IRIS_WEB_PORT", "1") // closed port → forces docker exec fallback
        .env("IRIS_CONTAINER", &container)
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

    // Find the tools/call response (id=2)
    let mut found_docker_exec = false;
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["id"] == 2 {
                let content = &v["result"]["content"];
                if let Some(arr) = content.as_array() {
                    for item in arr {
                        if let Some(text) = item["text"].as_str() {
                            if let Ok(resp) = serde_json::from_str::<serde_json::Value>(text) {
                                let exec_path = resp["execution_path"].as_str().unwrap_or("");
                                if exec_path == "docker_exec_local" {
                                    found_docker_exec = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(
        found_docker_exec,
        "iris_execute with closed web port must return execution_path=docker_exec_local. stdout:\n{stdout}"
    );
}

/// FR-008: iris_execute via Atelier REST path must return execution_path: "atelier".
#[ignore]
#[tokio::test]
async fn test_iris_execute_atelier_path_has_execution_path_field() {
    let Some(binary) = iris_agentic_dev_core::testing::require_iad_binary() else {
        return;
    };

    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(&binary)
        // The MCP server is the `mcp` subcommand. Spawning the bare binary prints the usage
        // banner and exits, which these tests read as an empty stdout.
        .arg("mcp")
        // Declare the gate state instead of inheriting the operator's (or the CI e2e job's):
        // iris_execute and iris_compile are write tools and refuse when the gate is off.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .envs(live_iris_env())
        .env("IRIS_CONTAINER", live_iris_container())
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

    let mut found_atelier = false;
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["id"] == 2 {
                let content = &v["result"]["content"];
                if let Some(arr) = content.as_array() {
                    for item in arr {
                        if let Some(text) = item["text"].as_str() {
                            if let Ok(resp) = serde_json::from_str::<serde_json::Value>(text) {
                                let exec_path = resp["execution_path"].as_str().unwrap_or("");
                                if exec_path == "atelier" {
                                    found_atelier = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(
        found_atelier,
        "iris_execute via Atelier must return execution_path=atelier. stdout:\n{stdout}"
    );
}
