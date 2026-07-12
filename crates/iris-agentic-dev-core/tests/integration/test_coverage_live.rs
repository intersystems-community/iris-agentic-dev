//! Live integration tests for iris_coverage — require IRIS_HOST/IRIS_WEB_PORT.
//! All tests are #[ignore]; run with: --include-ignored

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::coverage::{handle_iris_coverage, IrisCoverageParams};

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
    let client = IrisConnection::http_client().unwrap();
    Some((conn, client))
}

/// Check that mode=check returns either {ok:true} or {error_code:"BBSIZ_NOT_CONFIGURED"}.
/// Both are acceptable — the container may not have bbsiz=4096 configured.
#[tokio::test]
#[ignore]
async fn live_coverage_check_returns_ok_or_bbsiz_error() {
    let Some((iris, client)) = make_conn() else {
        println!("IRIS_HOST not set — skipping");
        return;
    };
    let params = IrisCoverageParams {
        mode: "check".to_string(),
        classes: None,
        package: None,
        test_path: None,
        target_pct: None,
        namespace: Some("USER".to_string()),
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;
    let is_ready = result["ok"].as_bool().unwrap_or(false);
    let err_code = result["error_code"].as_str().unwrap_or("");
    assert!(
        is_ready || err_code == "BBSIZ_NOT_CONFIGURED",
        "unexpected check result: {result}"
    );
}

/// Run coverage for IrisDevTest.SqlPower against IrisDevTest.SqlPowerTest.
/// Asserts the result has total_pct, classes, and meets_target fields.
/// IrisDevTest.SqlPower must be compiled in USER namespace before running.
#[tokio::test]
#[ignore]
async fn live_coverage_run_returns_structured_result() {
    let Some((iris, client)) = make_conn() else {
        println!("IRIS_HOST not set — skipping");
        return;
    };
    let params = IrisCoverageParams {
        mode: "run".to_string(),
        classes: Some(vec!["IrisDevTest.SqlPower".to_string()]),
        package: None,
        test_path: Some("IrisDevTest.SqlPowerTest".to_string()),
        target_pct: Some(80.0),
        namespace: Some("USER".to_string()),
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;

    // BBSIZ_NOT_CONFIGURED is acceptable — container may not have bbsiz configured
    if result["error_code"].as_str() == Some("BBSIZ_NOT_CONFIGURED") {
        println!("BBSIZ_NOT_CONFIGURED — monitor subsystem not available in this build");
        return;
    }

    assert_eq!(
        result["success"].as_bool(),
        Some(true),
        "run failed: {result}"
    );
    assert!(
        result["total_pct"].is_number(),
        "missing total_pct: {result}"
    );
    assert!(
        result["classes"].is_array(),
        "missing classes array: {result}"
    );
    assert!(
        result["meets_target"].is_boolean(),
        "missing meets_target: {result}"
    );

    let classes = result["classes"].as_array().unwrap();
    assert!(!classes.is_empty(), "classes array is empty: {result}");

    let first = &classes[0];
    assert!(first["class"].is_string(), "missing class name: {first}");
    assert!(first["hit"].is_number(), "missing hit: {first}");
    assert!(first["total"].is_number(), "missing total: {first}");
    assert!(first["pct"].is_number(), "missing pct: {first}");
}
