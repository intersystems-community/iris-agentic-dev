// Integration tests: dispatch_gate() wiring through tool handlers.
//
// Verifies that when a ConnectionPolicy with mcpTemplate=live is configured,
// the tool handlers return ENV_GATE_BLOCKED before any IRIS call is made.
//
// These tests require a live IRIS instance (IRIS_HOST must be set).
// They primarily exist to exercise the dispatch_gate wiring in tools/mod.rs.
//
// Run with:
//   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_PASSWORD=SYS \
//   cargo test --test test_dispatch_gate_handlers --features testing -- --nocapture

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::IrisTools;
use std::io::Write;
use std::sync::Arc;

fn iris_host() -> String {
    std::env::var("IRIS_HOST").unwrap_or_default()
}

/// Build an IrisTools instance where the connection is sourced from ServerManager
/// with a policy config on disk that maps server_name → mcpTemplate=live.
///
/// Returns None if IRIS_HOST is not set.
fn make_tools_with_live_policy(server_name: &str) -> Option<(IrisTools, tempfile::TempDir)> {
    let host = iris_host();
    if host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());

    // Build connection with ServerManager source — this is what triggers policy lookup
    let conn = IrisConnection::new(
        format!("http://{}:{}", host, web_port),
        "USER",
        username,
        password,
        DiscoverySource::ServerManager {
            server_name: server_name.to_string(),
        },
    );

    // Write a fleet config to a tempdir; the policy lookup reads from config_file.parent()
    let dir = tempfile::TempDir::new().expect("tempdir");
    let toml_path = dir.path().join(".iris-agentic-dev.toml");
    let toml_content = format!(
        "[policy.{}]\nmcpTemplate = \"live\"\ndataPolicy = \"block\"\n",
        server_name
    );
    let mut f = std::fs::File::create(&toml_path).expect("create toml");
    f.write_all(toml_content.as_bytes()).expect("write toml");

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn_state = tools.connection.lock().unwrap();
        // Set the live connection with ServerManager source
        conn_state.iris = Some(Arc::new(conn));
        // Point config_file at the tempdir so workspace_path resolves correctly
        conn_state.config_file = Some(toml_path);
    }

    Some((tools, dir)) // return dir to keep tempdir alive for test duration
}

fn parse_result(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    match r {
        Ok(result) => {
            let text = result.content[0].as_text().unwrap().text.clone();
            serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({"raw": text}))
        }
        Err(e) => serde_json::json!({"error": e}),
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

// ── ENV_GATE_BLOCKED via tool handlers ───────────────────────────────────────

#[test]
fn live_template_blocks_iris_compile_through_handler() {
    let (tools, _dir) = match make_tools_with_live_policy("iris-live-test") {
        Some(t) => t,
        None => {
            eprintln!("Skipping: IRIS_HOST not set");
            return;
        }
    };
    let v = rt().block_on(async {
        parse_result(
            tools
                .call_for_test(
                    "iris_compile",
                    serde_json::json!({"target": "IrisDevTest.Sample", "namespace": "USER"}),
                )
                .await,
        )
    });
    assert_eq!(
        v["error_code"], "ENV_GATE_BLOCKED",
        "live template must block iris_compile through handler: {v}"
    );
    assert_eq!(v["blocked_category"], "compile");
    assert_eq!(v["template"], "live");
}

#[test]
fn live_template_blocks_iris_execute_through_handler() {
    let (tools, _dir) = match make_tools_with_live_policy("iris-live-test") {
        Some(t) => t,
        None => {
            eprintln!("Skipping: IRIS_HOST not set");
            return;
        }
    };
    let v = rt().block_on(async {
        parse_result(
            tools
                .call_for_test(
                    "iris_execute",
                    serde_json::json!({"code": "Write 1", "namespace": "USER"}),
                )
                .await,
        )
    });
    assert_eq!(
        v["error_code"], "ENV_GATE_BLOCKED",
        "live template must block iris_execute through handler: {v}"
    );
    assert_eq!(v["blocked_category"], "execute");
}

#[test]
fn live_template_blocks_source_control_through_handler() {
    let (tools, _dir) = match make_tools_with_live_policy("iris-live-test") {
        Some(t) => t,
        None => {
            eprintln!("Skipping: IRIS_HOST not set");
            return;
        }
    };
    let v = rt().block_on(async {
        parse_result(
            tools
                .call_for_test(
                    "iris_source_control",
                    serde_json::json!({"action": "status", "namespace": "USER"}),
                )
                .await,
        )
    });
    assert_eq!(
        v["error_code"], "ENV_GATE_BLOCKED",
        "live template must block iris_source_control through handler: {v}"
    );
    assert_eq!(v["blocked_category"], "source_control");
}

#[test]
fn live_template_permits_iris_query_through_handler() {
    let (tools, _dir) = match make_tools_with_live_policy("iris-live-test") {
        Some(t) => t,
        None => {
            eprintln!("Skipping: IRIS_HOST not set");
            return;
        }
    };
    let v = rt().block_on(async {
        parse_result(
            tools
                .call_for_test(
                    "iris_query",
                    serde_json::json!({"sql": "SELECT 1", "namespace": "USER"}),
                )
                .await,
        )
    });
    assert_ne!(
        v["error_code"], "ENV_GATE_BLOCKED",
        "live template must permit iris_query (query category is allowed): {v}"
    );
}
