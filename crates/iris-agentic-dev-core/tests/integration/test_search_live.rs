//! Live integration tests for iris_search — exercises handle_iris_search against real IRIS.
//! All tests are `#[ignore]` — run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   cargo test --test test_search_live -- --ignored --nocapture --test-threads=1

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::log_store;
use iris_agentic_dev_core::tools::search::{handle_iris_search, SearchParams};
use std::sync::{Arc, Mutex};

async fn make_conn() -> Option<(IrisConnection, reqwest::Client)> {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let base_url = format!("http://{}:{}", iris_host, web_port);
    let mut conn = IrisConnection::new(
        base_url,
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    // Probe to discover Atelier API version — without this the connection defaults
    // to v1 and search returns empty on v8 IRIS (the /action/search response shape
    // changed; v1 returns {} instead of an array).
    conn.probe().await;
    Some((conn, reqwest::Client::new()))
}

fn make_log_store() -> Arc<Mutex<log_store::LogStore>> {
    Arc::new(Mutex::new(log_store::LogStore::new(200, 60)))
}

fn result_json(r: Result<rmcp::model::CallToolResult, rmcp::ErrorData>) -> serde_json::Value {
    let text = r.unwrap().content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap()
}

// ── T001: Basic search finds results ──────────────────────────────────────────
// IrisDevRunTest.UnitTestSuite.cls is created by the CI setup step and exists
// on the local dev container. TestAlwaysPasses is a known method in that class.

#[tokio::test]
#[ignore]
async fn test_search_basic_finds_results() {
    let Some((iris, client)) = make_conn().await else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "TestAlwaysPasses",
        "documents": ["IrisDevRunTest.*.cls"],
        "namespace": "USER"
    }))
    .unwrap();

    let log = make_log_store();
    let result = handle_iris_search(&iris, &client, p, log).await;
    let v = result_json(result);

    println!("test_search_basic_finds_results: {v}");
    assert_eq!(v["success"], true, "search should succeed: {v}");
    let total = v["total_found"].as_i64().unwrap_or(-1);
    assert!(total > 0, "should find results for 'TestAlwaysPasses': {v}");
}

// ── T002: Inline flag bypasses truncation ────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_search_inline_bypasses_truncation() {
    let Some((iris, client)) = make_conn().await else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "TestAlwaysPasses",
        "documents": ["IrisDevRunTest.*.cls"],
        "namespace": "USER",
        "inline": true
    }))
    .unwrap();

    let log = make_log_store();
    let result = handle_iris_search(&iris, &client, p, log).await;
    let v = result_json(result);

    println!("test_search_inline_bypasses_truncation: {v}");
    assert_eq!(v["success"], true, "search with inline should succeed: {v}");
    // With inline=true, all results are returned, not truncated
    assert!(v["results"].is_array(), "results should be an array: {v}");
}

// ── T003: Regex flag exercises regex search path ──────────────────────────────

#[tokio::test]
#[ignore]
async fn test_search_regex_flag() {
    let Some((iris, client)) = make_conn().await else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "Test.*Passes",
        "documents": ["IrisDevRunTest.*.cls"],
        "namespace": "USER",
        "regex": true
    }))
    .unwrap();

    let log = make_log_store();
    let result = handle_iris_search(&iris, &client, p, log).await;
    let v = result_json(result);

    println!("test_search_regex_flag: {v}");
    assert_eq!(v["success"], true, "regex search should succeed: {v}");
}

// ── T004: Case sensitive search ───────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_search_case_sensitive() {
    let Some((iris, client)) = make_conn().await else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "TestAlwaysPasses",
        "documents": ["IrisDevRunTest.*.cls"],
        "namespace": "USER",
        "case_sensitive": true
    }))
    .unwrap();

    let log = make_log_store();
    let result = handle_iris_search(&iris, &client, p, log).await;
    let v = result_json(result);

    println!("test_search_case_sensitive: {v}");
    assert_eq!(
        v["success"], true,
        "case-sensitive search should succeed: {v}"
    );
}

// ── T005: Category filter (CLS) ───────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_search_category_cls_filter() {
    let Some((iris, client)) = make_conn().await else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "TestAlwaysPasses",
        "documents": ["IrisDevRunTest.*.cls"],
        "namespace": "USER",
        "category": "CLS"
    }))
    .unwrap();

    let log = make_log_store();
    let result = handle_iris_search(&iris, &client, p, log).await;
    let v = result_json(result);

    println!("test_search_category_cls_filter: {v}");
    assert_eq!(
        v["success"], true,
        "category filter search should succeed: {v}"
    );
}

// ── T006: Empty result set for nonexistent term ───────────────────────────────

#[tokio::test]
#[ignore]
async fn test_search_no_results_for_nonexistent_term() {
    let Some((iris, client)) = make_conn().await else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "XYZZY_NONEXISTENT_12345_COVERAGE_PROBE",
        "documents": ["IrisDevRunTest.*.cls"],
        "namespace": "USER"
    }))
    .unwrap();

    let log = make_log_store();
    let result = handle_iris_search(&iris, &client, p, log).await;
    let v = result_json(result);

    println!("test_search_no_results_for_nonexistent_term: {v}");
    assert_eq!(
        v["success"], true,
        "search should succeed even with no results: {v}"
    );
    assert_eq!(
        v["total_found"].as_i64().unwrap_or(-1),
        0,
        "should find 0 results for nonexistent term: {v}"
    );
}

// ── T007: SCOPE_REQUIRED error when documents empty ───────────────────────────

#[tokio::test]
#[ignore]
async fn test_search_scope_required_error() {
    let Some((iris, client)) = make_conn().await else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "ClassMethod",
        "documents": [],
        "namespace": "%SYS"
    }))
    .unwrap();

    let log = make_log_store();
    let result = handle_iris_search(&iris, &client, p, log).await;
    let v = result_json(result);

    println!("test_search_scope_required_error: {v}");
    assert_eq!(
        v["success"], false,
        "search should fail with empty scope: {v}"
    );
    assert_eq!(
        v["error_code"].as_str().unwrap_or(""),
        "SCOPE_REQUIRED",
        "should return SCOPE_REQUIRED error code: {v}"
    );
}
