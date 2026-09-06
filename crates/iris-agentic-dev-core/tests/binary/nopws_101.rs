// 101-nopws-connectivity: Binary invocation tests (Layer 2).
// These spawn the IAD binary and verify MCP protocol responses contain NoPWS fields.
// Run with: IAD_BINARY=./target/debug/iris-agentic-dev cargo test --test nopws_101_binary -- --include-ignored

use std::io::Write;
use std::process::Stdio;

// `require_iad_binary` resolves the path against the workspace root and panics when the binary is
// missing. The local helper it replaced defaulted to the relative `./target/debug/...`, which never
// resolves from a crate-root working directory, and reported the miss by returning early — so all
// three tests in this file printed `ok` without executing an assertion for the whole 1.3.x line.
// See `iris_agentic_dev_core::testing`.
use iris_agentic_dev_core::testing::require_iad_binary;

/// The container the docker_only tests below should target.
///
/// Read here in the parent, because the children deliberately `env_remove("IRIS_CONTAINER")` —
/// the name has to reach them through the toml instead, which is what puts them in docker_only
/// mode. Writing `iris-dev-iris` into that toml pinned both tests to one laptop's container, so on
/// a CI runner they exercised a `docker exec` that could never land anywhere.
fn live_container() -> String {
    std::env::var("IRIS_CONTAINER")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "iris-dev-iris".to_string())
}

/// FR-012: Binary invocation test — spawn binary, call tools/list, assert iris_execute is listed.
#[ignore]
#[test]
fn test_binary_tools_list_includes_iris_execute() {
    let Some(bin) = require_iad_binary() else {
        return;
    };

    let mut child = std::process::Command::new(&bin)
        // The MCP server is the `mcp` subcommand. Spawning the bare binary prints the usage
        // banner and exits, which these tests read as an empty stdout.
        .arg("mcp")
        // Declare the gate state instead of inheriting the operator's (or the CI e2e job's):
        // iris_execute and iris_compile are write tools and refuse when the gate is off.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
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
    let Some(bin) = require_iad_binary() else {
        return;
    };

    use tempfile::TempDir;

    // Create a temp config with docker_only=true to force docker exec path
    let dir = TempDir::new().unwrap();
    let config_content = format!(
        r#"
container = "{}"
namespace = "USER"
nopws = true
docker_only = true
"#,
        live_container()
    );
    let config_path = dir.path().join(".iris-agentic-dev.toml");
    std::fs::write(&config_path, config_content).unwrap();

    let mut child = std::process::Command::new(&bin)
        // The MCP server is the `mcp` subcommand. Spawning the bare binary prints the usage
        // banner and exits, which these tests read as an empty stdout.
        .arg("mcp")
        // Declare the gate state instead of inheriting the operator's (or the CI e2e job's):
        // iris_execute and iris_compile are write tools and refuse when the gate is off.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        // The config file under OBJECTSCRIPT_WORKSPACE is what puts this run in docker_only
        // mode. Inherited connection vars outrank it, so a shell (or the CI e2e job) with
        // IRIS_HOST/IRIS_WEB_PORT set silently moved the call onto the Atelier path — where
        // iris_compile has no execution_path field to assert on.
        .env_remove("IRIS_HOST")
        .env_remove("IRIS_WEB_PORT")
        .env_remove("IRIS_CONTAINER")
        .env_remove("IRIS_NAMESPACE")
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
    let Some(bin) = require_iad_binary() else {
        return;
    };

    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let config_content = format!(
        r#"
container = "{}"
namespace = "USER"
nopws = true
docker_only = true
"#,
        live_container()
    );
    let config_path = dir.path().join(".iris-agentic-dev.toml");
    std::fs::write(&config_path, config_content).unwrap();

    let mut child = std::process::Command::new(&bin)
        // The MCP server is the `mcp` subcommand. Spawning the bare binary prints the usage
        // banner and exits, which these tests read as an empty stdout.
        .arg("mcp")
        // Declare the gate state instead of inheriting the operator's (or the CI e2e job's):
        // iris_execute and iris_compile are write tools and refuse when the gate is off.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        // The config file under OBJECTSCRIPT_WORKSPACE is what puts this run in docker_only
        // mode. Inherited connection vars outrank it, so a shell (or the CI e2e job) with
        // IRIS_HOST/IRIS_WEB_PORT set silently moved the call onto the Atelier path — where
        // iris_compile has no execution_path field to assert on.
        .env_remove("IRIS_HOST")
        .env_remove("IRIS_WEB_PORT")
        .env_remove("IRIS_CONTAINER")
        .env_remove("IRIS_NAMESPACE")
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

/// iris_compile must report a missing container the same way iris_execute does.
///
/// Needs docker, not IRIS. `COMPILE_FAILED` with the raw daemon text buried in it reads as "your
/// class did not compile" when nothing was ever compiled — an agent in docker_only mode should be
/// told the container is what is wrong, and which leg failed.
#[ignore]
#[test]
fn test_binary_iris_compile_missing_container_reports_container_unreachable() {
    let Some(bin) = require_iad_binary() else {
        return;
    };

    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let config_content = r#"
container = "iad-no-such-container-xyz"
namespace = "USER"
nopws = true
docker_only = true
"#;
    std::fs::write(dir.path().join(".iris-agentic-dev.toml"), config_content).unwrap();

    let mut child = std::process::Command::new(&bin)
        .arg("mcp")
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .env_remove("IRIS_HOST")
        .env_remove("IRIS_WEB_PORT")
        .env_remove("IRIS_CONTAINER")
        .env_remove("IRIS_NAMESPACE")
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

    let resp = tool_response(&stdout)
        .unwrap_or_else(|| panic!("no tools/call response in stdout:\n{stdout}"));

    assert_eq!(
        resp["error_code"].as_str(),
        Some("CONTAINER_UNREACHABLE"),
        "expected CONTAINER_UNREACHABLE from iris_compile: {resp}"
    );
    assert_eq!(
        resp["execution_path"].as_str(),
        Some("docker_exec_local"),
        "the failure must still say which leg ran: {resp}"
    );
    assert!(
        resp["error"]
            .as_str()
            .unwrap_or("")
            .contains("iad-no-such-container-xyz"),
        "the error must name the container that was tried: {resp}"
    );
}

/// The tool payload from a tools/call response, whichever content item carries it.
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
