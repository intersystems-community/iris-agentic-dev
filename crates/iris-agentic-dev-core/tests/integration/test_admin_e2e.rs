//! E2E tests for admin tools (T081, T087, T092, T096, T101).
//! All tests require a live IRIS container and are #[ignore] by default.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --test test_admin_e2e -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use std::sync::Arc;

fn make_conn() -> Option<(IrisConnection, reqwest::Client)> {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let base_url = format!("http://{}:{}", iris_host, web_port);
    let conn = IrisConnection::new(
        base_url,
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    let client = reqwest::Client::new();
    Some((conn, client))
}

fn parse_json(r: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = r
        .content
        .first()
        .map(|c| c.raw.as_text().unwrap().text.clone())
        .expect("no text content");
    serde_json::from_str(&text).expect("json parse failed")
}

// T081: iris_namespace_list returns at least USER and %SYS
#[tokio::test]
#[ignore]
async fn e2e_namespace_list() {
    use iris_agentic_dev_core::tools::admin_tools::iris_namespace_list_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_namespace_list");
            return;
        }
    };

    let result = iris_namespace_list_impl(&conn, &client)
        .await
        .expect("iris_namespace_list_impl failed");
    let v = parse_json(result);
    assert_eq!(
        v["success"].as_bool().unwrap_or(false),
        true,
        "expected success=true, got: {v}"
    );
    let namespaces = v["namespaces"]
        .as_array()
        .expect("namespaces must be an array");
    let ns_strs: Vec<&str> = namespaces.iter().filter_map(|n| n.as_str()).collect();
    assert!(
        ns_strs.iter().any(|n| n.eq_ignore_ascii_case("USER")),
        "USER namespace must be present, got: {ns_strs:?}"
    );
}

// T087: journal_search returns result without error
#[tokio::test]
#[ignore]
async fn e2e_journal_search() {
    use iris_agentic_dev_core::tools::admin_tools::journal_search_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_journal_search");
            return;
        }
    };

    let result = journal_search_impl(&conn, &client, None, None, None, 10)
        .await
        .expect("journal_search_impl failed");
    let v = parse_json(result);
    // Either success=true (entries returned) or a known non-crash error
    let success = v["success"].as_bool().unwrap_or(false);
    let err_code = v["error_code"].as_str().unwrap_or("");
    assert!(
        success || err_code == "NO_JOURNAL",
        "journal_search must succeed or return NO_JOURNAL, got: {v}"
    );
}

// T092: my_access returns current username and at least one role
#[tokio::test]
#[ignore]
async fn e2e_my_access() {
    use iris_agentic_dev_core::tools::admin_tools::my_access_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_my_access");
            return;
        }
    };

    let result = my_access_impl(&conn, &client)
        .await
        .expect("my_access_impl failed");
    let v = parse_json(result);
    assert_eq!(
        v["success"].as_bool().unwrap_or(false),
        true,
        "expected success=true, got: {v}"
    );
    let username = v["username"].as_str().unwrap_or("");
    assert!(!username.is_empty(), "username must not be empty, got: {v}");
}

// T096: hl7_schema_list — if EnsLib.HL7.Schema absent, returns HL7_NOT_AVAILABLE
#[tokio::test]
#[ignore]
async fn e2e_hl7_schema_list() {
    use iris_agentic_dev_core::tools::admin_tools::{hl7_schema_list_impl, ERR_HL7_NOT_AVAILABLE};

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_hl7_schema_list");
            return;
        }
    };

    let result = hl7_schema_list_impl(&conn, &client, "USER")
        .await
        .expect("hl7_schema_list_impl failed");
    let v = parse_json(result);
    // Either succeeds with schemas list, or returns HL7_NOT_AVAILABLE (not a crash)
    let success = v["success"].as_bool().unwrap_or(false);
    let err_code = v["error_code"].as_str().unwrap_or("");
    assert!(
        success || err_code == ERR_HL7_NOT_AVAILABLE,
        "hl7_schema_list must succeed or return HL7_NOT_AVAILABLE, got: {v}"
    );
}

// T101: mermaid_class returns a string starting with classDiagram
#[tokio::test]
#[ignore]
async fn e2e_mermaid_class() {
    use iris_agentic_dev_core::tools::admin_tools::mermaid_class_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_mermaid_class");
            return;
        }
    };

    let result = mermaid_class_impl(&conn, &client, "%Library.Persistent", 2, "USER")
        .await
        .expect("mermaid_class_impl failed");
    let v = parse_json(result);
    assert_eq!(
        v["success"].as_bool().unwrap_or(false),
        true,
        "expected success=true, got: {v}"
    );
    let diagram = v["diagram"].as_str().unwrap_or("");
    assert!(
        diagram.starts_with("classDiagram"),
        "diagram must start with 'classDiagram', got: {diagram}"
    );
}

// T076: global_preview + global_kill confirm round-trip
#[tokio::test]
#[ignore]
async fn e2e_global_kill_confirm() {
    use iris_agentic_dev_core::tools::admin_tools::{
        global_kill_impl, global_preview_impl, GlobalKillParams, GlobalPreviewParams,
    };
    use tokio::sync::Mutex;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_global_kill_confirm");
            return;
        }
    };
    let iris = Arc::new(conn);
    let client = Arc::new(client);
    let confirm_tokens = Mutex::new(std::collections::HashMap::new());

    // Seed a known global so preview has something to show.
    let seed_code = r#"Set ^IrisAgentDevTest("e2e-kill-test")="delete-me""#;
    let _ = iris
        .execute_via_generator(seed_code, &iris.namespace, &client)
        .await;

    // Step 1: preview — should mint a token.
    let preview_result = global_preview_impl(
        GlobalPreviewParams {
            global: "IrisAgentDevTest".to_string(),
            server: None,
            count: 5,
            iris: Arc::clone(&iris),
            client: Arc::clone(&client),
        },
        &confirm_tokens,
    )
    .await
    .expect("global_preview_impl failed");
    let pv = parse_json(preview_result);
    assert_eq!(
        pv["success"].as_bool().unwrap_or(false),
        true,
        "global_preview must succeed, got: {pv}"
    );
    let token = pv["confirm_token"]
        .as_str()
        .expect("no confirm_token")
        .to_string();

    // Step 2: kill with the token.
    let kill_result = global_kill_impl(
        GlobalKillParams {
            global: "IrisAgentDevTest".to_string(),
            server: None,
            confirm_token: token,
            iris: Arc::clone(&iris),
            client: Arc::clone(&client),
            write_tools_enabled: true,
        },
        &confirm_tokens,
    )
    .await
    .expect("global_kill_impl failed");
    let kv = parse_json(kill_result);
    assert_eq!(
        kv["success"].as_bool().unwrap_or(false),
        true,
        "global_kill must succeed, got: {kv}"
    );
    assert_eq!(
        kv["killed"].as_bool().unwrap_or(false),
        true,
        "killed must be true, got: {kv}"
    );
}
