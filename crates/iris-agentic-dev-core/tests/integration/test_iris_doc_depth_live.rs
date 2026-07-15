//! Live integration tests for 053-doc-depth: iris_doc fragment/compiled/list + iris_execute_method
//!
//! All tests are `#[ignore]` — run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   cargo test --test test_iris_doc_depth_live -- --ignored --nocapture

use iris_agentic_dev_core::elicitation::{CheckoutCache, ElicitationStore};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::doc::{
    handle_iris_doc, handle_iris_execute_method, IrisDocParams,
};
use iris_agentic_dev_core::tools::IrisExecuteMethodParams;

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

fn fragment_params(name: &str, start: i64, end: i64) -> IrisDocParams {
    serde_json::from_value(serde_json::json!({
        "mode": "fragment",
        "name": name,
        "start": start,
        "end": end,
    }))
    .unwrap()
}

fn compiled_params(name: &str) -> IrisDocParams {
    serde_json::from_value(serde_json::json!({
        "mode": "compiled",
        "name": name,
    }))
    .unwrap()
}

fn list_params(pattern: &str, category: &str, max_results: i64) -> IrisDocParams {
    serde_json::from_value(serde_json::json!({
        "mode": "list",
        "pattern": pattern,
        "category": category,
        "max_results": max_results,
    }))
    .unwrap()
}

fn execute_method_params(class: &str, method: &str, args: Vec<&str>) -> IrisExecuteMethodParams {
    IrisExecuteMethodParams {
        class: class.to_string(),
        method: method.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        namespace: "USER".to_string(),
    }
}

// ── T019: fragment live ───────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_fragment_live_library_integer() {
    let Some((iris, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    let store = ElicitationStore::default();
    let cache = CheckoutCache::default();
    let p = fragment_params("%Library.Integer.cls", 1, 5);
    let result = handle_iris_doc(&iris, &client, p, &store, &cache)
        .await
        .unwrap();
    let text = result.content[0].raw.as_text().unwrap().text.clone();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    println!("fragment result: {v}");
    assert_eq!(v["success"], true, "{v}");
    let lines = v["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 5, "expected 5 lines: {v}");
    for line in lines {
        assert!(line.as_str().is_some(), "each line should be a string");
    }
}

// ── T025: compiled live ───────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_compiled_live_library_integer() {
    let Some((iris, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    let store = ElicitationStore::default();
    let cache = CheckoutCache::default();
    let p = compiled_params("%Library.Integer.cls");
    let result = handle_iris_doc(&iris, &client, p, &store, &cache)
        .await
        .unwrap();
    let text = result.content[0].raw.as_text().unwrap().text.clone();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    println!(
        "compiled result: success={}, category={}",
        v["success"], v["category"]
    );
    // Success or NOT_COMPILED are both valid — system class INT may be empty or inaccessible
    let is_success = v["success"].as_bool() == Some(true);
    if is_success {
        assert_eq!(v["category"], "INT", "category should be INT: {v}");
        // total_lines >= 0 (may be 0 or 1 for system classes with empty first lines)
        assert!(
            v["total_lines"].as_i64().unwrap_or(-1) >= 0,
            "total_lines should be non-negative: {v}"
        );
    } else {
        let code = v["error_code"].as_str().unwrap_or("");
        assert_eq!(
            code, "NOT_COMPILED",
            "on failure should be NOT_COMPILED: {v}"
        );
    }
}

// ── T032: list live ───────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_list_live_library_cls() {
    let Some((iris, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    let store = ElicitationStore::default();
    let cache = CheckoutCache::default();
    let p = list_params("%Library.*", "CLS", 5);
    let result = handle_iris_doc(&iris, &client, p, &store, &cache)
        .await
        .unwrap();
    let text = result.content[0].raw.as_text().unwrap().text.clone();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    println!("list result: {v}");
    assert_eq!(v["success"], true, "{v}");
    let docs = v["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 5, "expected exactly 5 (max_results=5): {v}");
    assert_eq!(v["count"], 5, "count should equal docs.len(): {v}");
    assert_eq!(v["truncated"], true, "should be truncated: {v}");
}

// ── T039: iris_execute_method live ────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_execute_method_live_get_version() {
    let Some((iris, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    // %SYSTEM.Version:GetVersion() returns the IRIS version string — safe, no side effects
    let p = execute_method_params("%SYSTEM.Version", "GetVersion", vec![]);
    let result = handle_iris_execute_method(&iris, &client, &p)
        .await
        .unwrap();
    let text = result.content[0].raw.as_text().unwrap().text.clone();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    println!("execute_method GetVersion(): {v}");
    assert_eq!(v["success"], true, "{v}");
    let rv = v["return_value"].as_str().unwrap_or("");
    assert!(!rv.is_empty(), "return_value should not be empty: {v}");
    // IRIS version string contains "IRIS" or a version number
    assert!(
        rv.contains("IRIS") || rv.chars().any(|c| c.is_ascii_digit()),
        "return_value should look like a version string: '{rv}'"
    );
}

#[tokio::test]
#[ignore]
async fn test_execute_method_live_integer_isvalid_numeric() {
    let Some((iris, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    // %Library.Integer:IsValid(42) should return 1 (valid integer)
    let p = execute_method_params("%Library.Integer", "IsValid", vec!["42"]);
    let result = handle_iris_execute_method(&iris, &client, &p)
        .await
        .unwrap();
    let text = result.content[0].raw.as_text().unwrap().text.clone();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    println!("execute_method IsValid(42): {v}");
    assert_eq!(v["success"], true, "{v}");
    assert_eq!(
        v["return_value"].as_str().unwrap_or("").trim(),
        "1",
        "IsValid(42) should return 1: {v}"
    );
}
