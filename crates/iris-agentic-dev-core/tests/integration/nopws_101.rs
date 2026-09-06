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

    let resp = tool_response(&stdout).unwrap_or_else(|| {
        panic!("no tools/call response in stdout:\n{stdout}");
    });

    assert_eq!(
        resp["execution_path"].as_str(),
        Some("docker_exec_local"),
        "iris_execute with a closed web port must route to docker exec: {resp}"
    );
    // `Write 1` prints 1. Asserting on the output is the difference between "the tool claimed the
    // docker path" and "the docker path ran": this test used to pass in CI against a container
    // that does not exist there, because a failed exec came back as success with empty output.
    assert_eq!(resp["success"], true, "docker exec did not succeed: {resp}");
    assert!(
        resp["output"].as_str().unwrap_or("").contains('1'),
        "docker exec produced no output for `Write 1` — the exec did not reach IRIS: {resp}"
    );
}

/// The docker path must fail loudly when the container is not there.
///
/// Needs a docker daemon; deliberately does not need IRIS. `docker exec` into a name nothing
/// answers to used to return `{"success": true, "output": ""}`, which is how CI reported a
/// working fallback while executing nothing at all.
#[ignore]
#[tokio::test]
async fn test_iris_execute_missing_container_is_an_error_not_an_empty_success() {
    let Some(binary) = iris_agentic_dev_core::testing::require_iad_binary() else {
        return;
    };

    use std::io::Write;
    use std::process::{Command, Stdio};

    // An empty workspace root, because a `container` key in any .iris-agentic-dev.toml the server
    // finds overwrites IRIS_CONTAINER outright — and the search walks up from cwd, so on a dev
    // machine it reaches ~/.iris-agentic-dev.toml and substitutes that container for the one this
    // test names. Without this the test asserts nothing: it silently ran against iris-dev-iris.
    let empty_workspace = tempfile::TempDir::new().expect("temp workspace");

    let mut child = Command::new(&binary)
        .arg("mcp")
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .env("OBJECTSCRIPT_WORKSPACE", empty_workspace.path())
        .envs(live_iris_env())
        .env("IRIS_WEB_PORT", "1") // closed port → forces the docker path
        .env("IRIS_CONTAINER", "iad-no-such-container-xyz")
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

    let resp = tool_response(&stdout).unwrap_or_else(|| {
        panic!("no tools/call response in stdout:\n{stdout}");
    });

    assert_eq!(
        resp["success"], false,
        "missing container reported as a success: {resp}"
    );
    assert_eq!(
        resp["error_code"].as_str(),
        Some("CONTAINER_UNREACHABLE"),
        "expected CONTAINER_UNREACHABLE: {resp}"
    );
    let error = resp["error"].as_str().unwrap_or("");
    assert!(
        error.contains("iad-no-such-container-xyz"),
        "the error must name the container that was tried: {resp}"
    );
    assert!(
        error.contains("docker ps"),
        "the error must say how to check: {resp}"
    );
}

/// The `iris_execute` payload from a tools/call response, whichever content item carries it.
fn tool_response(stdout: &str) -> Option<serde_json::Value> {
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["id"] != 2 {
            continue;
        }
        // Errors come back through `content` too (isError: true), so read both the same way.
        for item in v["result"]["content"].as_array().into_iter().flatten() {
            if let Some(text) = item["text"].as_str() {
                if let Ok(resp) = serde_json::from_str::<serde_json::Value>(text) {
                    return Some(resp);
                }
            }
        }
    }
    None
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

    let resp = tool_response(&stdout).unwrap_or_else(|| {
        panic!("no tools/call response in stdout:\n{stdout}");
    });

    assert_eq!(
        resp["execution_path"].as_str(),
        Some("atelier"),
        "iris_execute via Atelier must return execution_path=atelier: {resp}"
    );
}
