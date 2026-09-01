//! Live IRIS integration tests for iris_mirror_status, iris_database_list
//! free space, and iris_system_performance (089). All tests require
//! iris-dev-iris and are #[ignore] by default.
//!
//! Note: iris_system_performance tests require Enterprise IRIS — they will
//! return an error on community builds, which is the expected behaviour.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --test test_mirror_and_freespace -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

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
        .map(|c| c.as_text().unwrap().text.clone())
        .expect("no text content");
    serde_json::from_str(&text).expect("json parse failed")
}

// T011 / T006: iris_mirror_status on community iris-dev-iris (not in a mirror)
#[tokio::test]
#[ignore]
async fn e2e_mirror_status_non_member() {
    use iris_agentic_dev_core::tools::admin_tools::iris_mirror_status_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_mirror_status_non_member");
            return;
        }
    };

    let result = iris_mirror_status_impl(&conn, &client)
        .await
        .expect("iris_mirror_status_impl failed");
    let v = parse_json(result);

    eprintln!("mirror_status response: {v}");

    assert_eq!(
        v["success"].as_bool(),
        Some(true),
        "expected success=true, got: {v}"
    );
    assert_eq!(
        v["is_member"].as_bool(),
        Some(false),
        "community iris-dev-iris is not in a mirror; expected is_member=false, got: {v}"
    );
    assert!(
        v["mirror_name"].is_null(),
        "expected mirror_name=null for non-member, got: {v}"
    );
    assert_eq!(
        v["is_primary"].as_bool(),
        Some(false),
        "expected is_primary=false for non-member, got: {v}"
    );
}

// T018 / T014: iris_database_list includes free space fields on iris-dev-iris
#[tokio::test]
#[ignore]
async fn e2e_database_list_free_space() {
    use iris_agentic_dev_core::tools::admin_tools::iris_database_list_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_database_list_free_space");
            return;
        }
    };

    let result = iris_database_list_impl(&conn, &client)
        .await
        .expect("iris_database_list_impl failed");
    let v = parse_json(result);

    eprintln!("database_list response: {v}");

    assert_eq!(
        v["success"].as_bool(),
        Some(true),
        "expected success=true, got: {v}"
    );

    // No free_space_note at root — free space query should succeed on iris-dev-iris
    assert!(
        v["free_space_note"].is_null() || !v.as_object().unwrap().contains_key("free_space_note"),
        "unexpected free_space_note: {}",
        v["free_space_note"]
    );

    let databases = v["databases"]
        .as_array()
        .expect("databases should be array");
    assert!(
        !databases.is_empty(),
        "expected at least one database, got empty array"
    );

    // At least one entry should have size_mb as a positive number
    let has_size = databases.iter().any(|db| {
        db["size_mb"].as_i64().map_or(false, |n| n > 0)
            || db["size_mb"].as_f64().map_or(false, |n| n > 0.0)
    });
    assert!(
        has_size,
        "expected at least one database with size_mb > 0, got: {databases:?}"
    );

    // At least one entry should have free_space_mb as a non-negative number
    let has_free = databases
        .iter()
        .any(|db| db["free_space_mb"].as_f64().is_some() || db["free_space_mb"].as_i64().is_some());
    assert!(
        has_free,
        "expected at least one database with free_space_mb field, got: {databases:?}"
    );

    // max_size_mb should be null or a positive integer — never an error
    for db in databases {
        let max = &db["max_size_mb"];
        assert!(
            max.is_null() || max.as_i64().map_or(false, |n| n > 0),
            "max_size_mb should be null or positive, got: {max} in {db}"
        );
    }
}

// iris_system_performance: last_runid on community iris-dev-iris
// Expected: success=true, run_id=null (no runs have been started)
// On Enterprise with prior runs, run_id will be a non-empty string.
#[tokio::test]
#[ignore]
async fn e2e_system_performance_last_runid_community() {
    use iris_agentic_dev_core::tools::admin_tools::iris_system_performance_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_last_runid_community");
            return;
        }
    };

    let result = iris_system_performance_impl(&conn, &client, "last_runid", None)
        .await
        .expect("iris_system_performance_impl failed");
    let v = parse_json(result);

    eprintln!("system_performance last_runid response: {v}");

    // success must be present — value depends on whether SystemPerformance global exists
    assert!(
        v.get("success").is_some(),
        "expected success field, got: {v}"
    );
    // mode must be last_runid on success path
    if v["success"].as_bool() == Some(true) {
        assert_eq!(
            v["mode"].as_str(),
            Some("last_runid"),
            "expected mode=last_runid, got: {v}"
        );
    }
}

// iris_system_performance: mode=status without run_id returns error
#[tokio::test]
#[ignore]
async fn e2e_system_performance_status_missing_run_id() {
    use iris_agentic_dev_core::tools::admin_tools::iris_system_performance_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_system_performance_status_missing_run_id");
            return;
        }
    };

    let result = iris_system_performance_impl(&conn, &client, "status", None)
        .await
        .expect("iris_system_performance_impl failed");
    let v = parse_json(result);

    eprintln!("system_performance status (no run_id) response: {v}");

    assert_eq!(
        v["success"].as_bool(),
        Some(false),
        "expected success=false when run_id missing, got: {v}"
    );
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("run_id"),
        "expected error mentioning run_id, got: {v}"
    );
}
