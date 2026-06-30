//! Integration tests for iris_admin observability actions (055-system-observability).
//! Requires live IRIS on iris-dev-iris container (port from IRIS_HOST/IRIS_PORT env).
//! All tests are #[ignore] — run with:
//!   cargo test -p iris-agentic-dev-core --features testing --test test_iris_admin_observability_live -- --ignored

use iris_agentic_dev_core::iris::connection::DiscoverySource;
use iris_agentic_dev_core::iris::IrisConnection;

/// Parse first text content from a CallToolResult into JSON.
fn parse_result(result: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = result
        .content
        .first()
        .map(|c| c.raw.as_text().unwrap().text.clone())
        .expect("text content");
    serde_json::from_str(&text).expect("valid JSON")
}

/// Open a live %SYS-capable IRIS connection from env vars.
fn live_iris() -> Option<IrisConnection> {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    Some(IrisConnection::new(
        format!("http://{}:{}", host, port),
        "%SYS",
        user,
        pass,
        DiscoverySource::EnvVar,
    ))
}

// ---------------------------------------------------------------------------
// T016 / US1: view_locks live
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_view_locks_returns_success() {
    let iris = live_iris();
    let result = iris_agentic_dev_core::tools::observability::view_locks_impl(iris.as_ref())
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "view_locks failed: {v}");
    assert!(v["locks"].is_array(), "locks should be array");
}

// ---------------------------------------------------------------------------
// T024 / US2: view_processes live — allow policy
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_view_processes_allow_returns_success() {
    let iris = live_iris();
    let result = iris_agentic_dev_core::tools::observability::view_processes_impl(
        iris.as_ref(),
        "allow",
        None,
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "view_processes failed: {v}");
    assert!(v["processes"].is_array());
    assert!(
        v["count"].as_u64().unwrap_or(0) >= 1,
        "expected at least 1 process"
    );
}

#[tokio::test]
#[ignore]
async fn live_view_processes_redact_hides_phi() {
    let iris = live_iris();
    let result = iris_agentic_dev_core::tools::observability::view_processes_impl(
        iris.as_ref(),
        "redact",
        None,
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true);
    for proc in v["processes"].as_array().unwrap() {
        assert_eq!(proc["username"], "[REDACTED]");
        assert_eq!(proc["client_node_name"], "[REDACTED]");
        assert_eq!(proc["client_ip"], "[REDACTED]");
    }
}

// ---------------------------------------------------------------------------
// T033 / US3: journal_search live (pattern filter)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_journal_search_with_pattern_returns_success() {
    let iris = live_iris();
    let result = iris_agentic_dev_core::tools::observability::journal_search_impl(
        iris.as_ref(),
        "allow",
        Some("IrisDevTest.*"),
        None,
        Some(10),
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "journal_search failed: {v}");
    assert!(v["records"].is_array());
}

// ---------------------------------------------------------------------------
// T040 / US4: namespace_mappings live
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_namespace_mappings_user_ns_returns_success() {
    let iris = live_iris();
    let result = iris_agentic_dev_core::tools::observability::namespace_mappings_impl(
        iris.as_ref(),
        Some("USER"),
        "USER",
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "namespace_mappings failed: {v}");
    assert!(v["mappings"]["globals"].is_array());
    assert!(v["mappings"]["packages"].is_array());
    assert!(v["mappings"]["routines"].is_array());
}

#[tokio::test]
#[ignore]
async fn live_namespace_mappings_nonexistent_returns_not_found() {
    let iris = live_iris();
    let result = iris_agentic_dev_core::tools::observability::namespace_mappings_impl(
        iris.as_ref(),
        Some("DOESNOTEXIST99"),
        "USER",
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "NAMESPACE_NOT_FOUND");
}

// ---------------------------------------------------------------------------
// T047 / US5: database_status live
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_database_status_all_returns_success() {
    let iris = live_iris();
    let result =
        iris_agentic_dev_core::tools::observability::database_status_impl(iris.as_ref(), None)
            .await
            .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "database_status failed: {v}");
    assert!(v["databases"].is_array());
    let has_user = v["databases"].as_array().unwrap().iter().any(|d| {
        d["name"]
            .as_str()
            .map(|n| n.eq_ignore_ascii_case("USER"))
            .unwrap_or(false)
    });
    assert!(has_user, "USER database not found in list");
}

#[tokio::test]
#[ignore]
async fn live_database_status_nonexistent_returns_not_found() {
    let iris = live_iris();
    let result = iris_agentic_dev_core::tools::observability::database_status_impl(
        iris.as_ref(),
        Some("DOESNOTEXIST99"),
    )
    .await
    .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "DATABASE_NOT_FOUND");
}
