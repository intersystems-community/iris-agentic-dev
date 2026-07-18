//! Live integration tests for iris_coverage — require IRIS_HOST/IRIS_WEB_PORT.
//! All tests are #[ignore]; run with: --include-ignored

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::coverage::{
    handle_iris_coverage, testcoverage_available, IrisCoverageParams,
};

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

/// mode=check returns either {ok:true} or {error_code:"BBSIZ_NOT_CONFIGURED"}.
/// Both are acceptable — the container may not have gmheap configured.
/// Now also verifies testcoverage_available field is present.
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
        cobertura_path: None,
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;
    let is_ready = result["ok"].as_bool().unwrap_or(false);
    let err_code = result["error_code"].as_str().unwrap_or("");
    assert!(
        is_ready || err_code == "BBSIZ_NOT_CONFIGURED",
        "unexpected check result: {result}"
    );
    // testcoverage_available must always be present in check result
    assert!(
        result["testcoverage_available"].is_boolean(),
        "testcoverage_available field missing from check result: {result}"
    );
}

/// mode=check includes testcoverage_hint when TestCoverage not installed.
#[tokio::test]
#[ignore]
async fn live_coverage_check_testcoverage_hint_when_unavailable() {
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
        cobertura_path: None,
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;
    let tc_avail = result["testcoverage_available"].as_bool().unwrap_or(true);
    if !tc_avail {
        let hint = result["testcoverage_hint"].as_str().unwrap_or("");
        assert!(
            hint.contains("testcoverage") || hint.contains("zpm"),
            "expected install hint: {result}"
        );
    }
    // If tc_avail=true, no hint needed — either case is valid
}

/// testcoverage_available() helper returns a bool without panicking.
#[tokio::test]
#[ignore]
async fn live_testcoverage_available_returns_bool() {
    let Some((iris, client)) = make_conn() else {
        println!("IRIS_HOST not set — skipping");
        return;
    };
    // Result is either true or false — we just verify it doesn't panic
    let available = testcoverage_available(&iris, &client, "USER").await;
    println!("TestCoverage available: {available}");
}

/// mode=run returns structured result with total_pct, classes, meets_target.
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
        cobertura_path: None,
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;

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
    // testcoverage_available must be present
    assert!(
        result["testcoverage_available"].is_boolean(),
        "testcoverage_available missing: {result}"
    );

    let classes = result["classes"].as_array().unwrap();
    assert!(!classes.is_empty(), "classes array is empty: {result}");

    let first = &classes[0];
    assert!(first["class"].is_string(), "missing class name: {first}");
    assert!(first["hit"].is_number(), "missing hit: {first}");
    assert!(first["total"].is_number(), "missing total: {first}");
    assert!(first["pct"].is_number(), "missing pct: {first}");
}

/// mode=run with package param auto-discovers classes.
#[tokio::test]
#[ignore]
async fn live_coverage_run_with_package_param() {
    let Some((iris, client)) = make_conn() else {
        println!("IRIS_HOST not set — skipping");
        return;
    };
    let params = IrisCoverageParams {
        mode: "run".to_string(),
        classes: None,
        package: Some("IrisDevTest".to_string()),
        test_path: Some("IrisDevTest.SqlPowerTest".to_string()),
        target_pct: None,
        namespace: Some("USER".to_string()),
        cobertura_path: None,
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;

    if result["error_code"].as_str() == Some("BBSIZ_NOT_CONFIGURED") {
        println!("BBSIZ_NOT_CONFIGURED — skipping");
        return;
    }
    if result["error_code"].as_str() == Some("NO_CLASSES") {
        println!("IrisDevTest package not populated — skipping");
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
}

/// mode=run returns cobertura_skipped when cobertura_path set but TestCoverage unavailable.
#[tokio::test]
#[ignore]
async fn live_coverage_run_cobertura_skipped_when_unavailable() {
    let Some((iris, client)) = make_conn() else {
        println!("IRIS_HOST not set — skipping");
        return;
    };
    let tc_avail = testcoverage_available(&iris, &client, "USER").await;
    if tc_avail {
        println!("TestCoverage installed — cobertura_skipped won't appear, skipping test");
        return;
    }

    let params = IrisCoverageParams {
        mode: "run".to_string(),
        classes: Some(vec!["IrisDevTest.SqlPower".to_string()]),
        package: None,
        test_path: Some("IrisDevTest.SqlPowerTest".to_string()),
        target_pct: None,
        namespace: Some("USER".to_string()),
        cobertura_path: Some("/tmp/coverage.xml".to_string()),
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;

    if result["error_code"].as_str() == Some("BBSIZ_NOT_CONFIGURED") {
        println!("BBSIZ_NOT_CONFIGURED — skipping");
        return;
    }

    assert!(
        result["cobertura_skipped"].as_str().is_some(),
        "expected cobertura_skipped when TestCoverage unavailable: {result}"
    );
}

/// mode=start → mode=stop manual flow.
#[tokio::test]
#[ignore]
async fn live_coverage_start_stop_flow() {
    let Some((iris, client)) = make_conn() else {
        println!("IRIS_HOST not set — skipping");
        return;
    };

    // Start
    let start_params = IrisCoverageParams {
        mode: "start".to_string(),
        classes: Some(vec!["IrisDevTest.SqlPower".to_string()]),
        package: None,
        test_path: None,
        target_pct: None,
        namespace: Some("USER".to_string()),
        cobertura_path: None,
    };
    let start_result = handle_iris_coverage(&iris, &client, &start_params).await;

    if start_result["error_code"].as_str() == Some("BBSIZ_NOT_CONFIGURED")
        || start_result["error_code"].as_str() == Some("MONITOR_IN_USE")
    {
        println!("monitor unavailable or in use — skipping start/stop test");
        return;
    }

    assert_eq!(
        start_result["started"].as_bool(),
        Some(true),
        "start failed: {start_result}"
    );
    assert!(
        start_result["routines"].is_array(),
        "missing routines: {start_result}"
    );

    // Stop
    let stop_params = IrisCoverageParams {
        mode: "stop".to_string(),
        classes: None,
        package: None,
        test_path: None,
        target_pct: None,
        namespace: Some("USER".to_string()),
        cobertura_path: None,
    };
    let stop_result = handle_iris_coverage(&iris, &client, &stop_params).await;
    assert_eq!(
        stop_result["stopped"].as_bool(),
        Some(true),
        "stop failed: {stop_result}"
    );
}

/// mode=run missing test_path returns MISSING_PARAM error.
#[tokio::test]
#[ignore]
async fn live_coverage_run_missing_test_path_returns_error() {
    let Some((iris, client)) = make_conn() else {
        println!("IRIS_HOST not set — skipping");
        return;
    };
    let params = IrisCoverageParams {
        mode: "run".to_string(),
        classes: Some(vec!["IrisDevTest.SqlPower".to_string()]),
        package: None,
        test_path: None,
        target_pct: None,
        namespace: Some("USER".to_string()),
        cobertura_path: None,
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;
    assert_eq!(result["error_code"].as_str(), Some("MISSING_PARAM"));
}

/// mode=run with neither classes nor package returns MISSING_PARAM.
#[tokio::test]
#[ignore]
async fn live_coverage_run_no_classes_or_package_returns_error() {
    let Some((iris, client)) = make_conn() else {
        println!("IRIS_HOST not set — skipping");
        return;
    };
    let params = IrisCoverageParams {
        mode: "run".to_string(),
        classes: None,
        package: None,
        test_path: Some("IrisDevTest.SqlPowerTest".to_string()),
        target_pct: None,
        namespace: Some("USER".to_string()),
        cobertura_path: None,
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;
    assert_eq!(result["error_code"].as_str(), Some("MISSING_PARAM"));
}

/// mode=invalid returns INVALID_ACTION error.
#[tokio::test]
#[ignore]
async fn live_coverage_invalid_mode_returns_error() {
    let Some((iris, client)) = make_conn() else {
        println!("IRIS_HOST not set — skipping");
        return;
    };
    let params = IrisCoverageParams {
        mode: "bogus".to_string(),
        classes: None,
        package: None,
        test_path: None,
        target_pct: None,
        namespace: Some("USER".to_string()),
        cobertura_path: None,
    };
    let result = handle_iris_coverage(&iris, &client, &params).await;
    assert_eq!(result["error_code"].as_str(), Some("INVALID_ACTION"));
}
