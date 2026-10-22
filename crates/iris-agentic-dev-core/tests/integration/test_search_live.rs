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

/// The class the scoped searches look in: two `Test*` methods, so `TestAlwaysPasses` and the regex
/// `Test.*Passes` each have something to find. Put and compiled the same way every time.
async fn seed_search_fixture(iris: &IrisConnection, client: &reqwest::Client) {
    let code = " Set c=\"IadSearchFixture.UnitTestSuite\" If '##class(%Dictionary.CompiledClass).%ExistsId(c) {\n   Set d=##class(%Dictionary.ClassDefinition).%New() Set d.Name=c, d.Super=\"%UnitTest.TestCase\"\n   For n=\"TestAlwaysPasses\",\"TestAlwaysFails\" { Set m=##class(%Dictionary.MethodDefinition).%New() Set m.Name=n Do m.Implementation.WriteLine(\" Do $$$AssertEquals(1,\"_$Select(n[\"Passes\":1,1:2)_\")\") Do d.Methods.Insert(m) }\n   Set sc=d.%Save() Set:sc sc=$system.OBJ.Compile(c,\"ck-d\") Write:'sc $system.Status.GetErrorText(sc)\n }\n Write \"seeded\"";
    let out = iris
        .execute_via_generator(code, "USER", client)
        .await
        .expect("seed the search fixture");
    assert!(out.contains("seeded"), "search fixture: {out}");
}

fn make_log_store() -> Arc<Mutex<log_store::LogStore>> {
    Arc::new(Mutex::new(log_store::LogStore::new(200, 60)))
}

fn result_json(r: Result<rmcp::model::CallToolResult, rmcp::ErrorData>) -> serde_json::Value {
    let text = r.unwrap().content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap()
}

// ── T001: Basic search finds results ──────────────────────────────────────────
// IadSearchFixture.UnitTestSuite is seeded by each test (`seed_search_fixture`). These used to search
// IrisDevRunTest.UnitTestSuite, which only exists while another test happens to leave it compiled.
// on the local dev container. TestAlwaysPasses is a known method in that class.

#[tokio::test]
#[ignore]
async fn test_search_basic_finds_results() {
    let Some((iris, client)) = make_conn().await else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    seed_search_fixture(&iris, &client).await;

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "TestAlwaysPasses",
        "documents": ["IadSearchFixture.*.cls"],
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
    seed_search_fixture(&iris, &client).await;

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "TestAlwaysPasses",
        "documents": ["IadSearchFixture.*.cls"],
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
    seed_search_fixture(&iris, &client).await;

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "Test.*Passes",
        "documents": ["IadSearchFixture.*.cls"],
        "namespace": "USER",
        "regex": true
    }))
    .unwrap();

    let log = make_log_store();
    let result = handle_iris_search(&iris, &client, p, log).await;
    let v = result_json(result);

    println!("test_search_regex_flag: {v}");
    assert_eq!(v["success"], true, "regex search should succeed: {v}");
    // It passed for months while `regex=true` reached Atelier, which reads only `regex=1`, so
    // every regex search was a literal one and found nothing.
    assert!(
        v["total_found"].as_u64().unwrap_or(0) > 0,
        "Test.*Passes must match: {v}"
    );
}

// ── T004: Case sensitive search ───────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_search_case_sensitive() {
    let Some((iris, client)) = make_conn().await else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    seed_search_fixture(&iris, &client).await;

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "TestAlwaysPasses",
        "documents": ["IadSearchFixture.*.cls"],
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
    seed_search_fixture(&iris, &client).await;

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "TestAlwaysPasses",
        "documents": ["IadSearchFixture.*.cls"],
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
    seed_search_fixture(&iris, &client).await;

    let p: SearchParams = serde_json::from_value(serde_json::json!({
        "query": "XYZZY_NONEXISTENT_12345_COVERAGE_PROBE",
        "documents": ["IadSearchFixture.*.cls"],
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

// ── The match limit ───────────────────────────────────────────────────────────
// Atelier stops at `max` matches (200 when the parameter is absent) and says so only in its
// `console`. Before this, iris_search sent no `max` and read no console, so a search that hit
// the cap came back as `total_found: 200` with nothing to say it was cut off.

const LIMIT_FIXTURE: &str = "IadSearchLimit.Fixture";

/// A class whose `Marker` method holds 12 lines naming `IadLimitMarker`.
async fn put_limit_fixture(iris: &IrisConnection, client: &reqwest::Client) {
    let code = format!(
        " Set c=##class(%Dictionary.ClassDefinition).%New() Set c.Name=\"{LIMIT_FIXTURE}\"\n \
         Set m=##class(%Dictionary.MethodDefinition).%New() Set m.Name=\"Marker\",m.ClassMethod=1\n \
         For i=1:1:12 {{ Do m.Implementation.WriteLine(\" Set x=\"\"IadLimitMarker\"_i_\"\"\"\") }}\n \
         Do c.Methods.Insert(m) Set sc=c.%Save() Set:sc sc=$system.OBJ.Compile(\"{LIMIT_FIXTURE}\",\"ck-d\")\n \
         Write $Select(sc:\"OK\",1:$system.Status.GetErrorText(sc))"
    );
    let out = iris
        .execute_via_generator(&code, "USER", client)
        .await
        .expect("fixture");
    assert!(out.contains("OK"), "fixture: {out}");
}

async fn drop_limit_fixture(iris: &IrisConnection, client: &reqwest::Client) {
    let _ = iris
        .execute_via_generator(
            &format!(" Do $system.OBJ.Delete(\"{LIMIT_FIXTURE}\",\"-d\")"),
            "USER",
            client,
        )
        .await;
}

async fn search_marker(
    iris: &IrisConnection,
    client: &reqwest::Client,
    limit: Option<u32>,
) -> serde_json::Value {
    let mut args = serde_json::json!({
        "query": "IadLimitMarker",
        "documents": ["IadSearchLimit.*.cls"],
        "namespace": "USER",
        "inline": true
    });
    if let Some(l) = limit {
        args["limit"] = l.into();
    }
    let p: SearchParams = serde_json::from_value(args).unwrap();
    result_json(handle_iris_search(iris, client, p, make_log_store()).await)
}

#[tokio::test]
#[ignore]
async fn a_search_cut_off_at_its_limit_says_so() {
    let Some((iris, client)) = make_conn().await else {
        panic!("IRIS_HOST unset; set IRIS_HOST=localhost IRIS_WEB_PORT=52780");
    };
    put_limit_fixture(&iris, &client).await;
    let cut = search_marker(&iris, &client, Some(5)).await;
    let whole = search_marker(&iris, &client, None).await;
    drop_limit_fixture(&iris, &client).await;

    assert_eq!(cut["total_found"], 5, "{cut}");
    assert_eq!(cut["limit_reached"], true, "{cut}");
    assert_eq!(cut["limit"], 5, "{cut}");
    assert!(
        cut["warning"].as_str().is_some_and(|w| w.contains("limit")),
        "{cut}"
    );

    assert_eq!(whole["total_found"], 12, "{whole}");
    assert_eq!(whole["limit_reached"], false, "{whole}");
    assert_eq!(whole["limit"], 200, "{whole}");
}

#[tokio::test]
#[ignore]
async fn a_limit_out_of_range_is_refused_before_any_search() {
    let Some((iris, client)) = make_conn().await else {
        panic!("IRIS_HOST unset; set IRIS_HOST=localhost IRIS_WEB_PORT=52780");
    };
    for bad in [0u32, 10_001] {
        let p: SearchParams = serde_json::from_value(serde_json::json!({
            "query": "x", "documents": ["A.*.cls"], "limit": bad
        }))
        .unwrap();
        let r = handle_iris_search(&iris, &client, p, make_log_store()).await;
        let text = r.unwrap().content[0].as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "INVALID_PARAMS", "{bad}: {v}");
    }
}

/// `regex: true` is a regex: `IadLimitMarker1[0-2]` matches lines 10 to 12 of the fixture, and
/// the same text searched literally matches nothing.
#[tokio::test]
#[ignore]
async fn a_regex_search_matches_by_pattern_anywhere_in_a_line() {
    let Some((iris, client)) = make_conn().await else {
        panic!("IRIS_HOST unset; set IRIS_HOST=localhost IRIS_WEB_PORT=52780");
    };
    put_limit_fixture(&iris, &client).await;
    let mut found = Vec::new();
    for regex in [true, false] {
        let p: SearchParams = serde_json::from_value(serde_json::json!({
            "query": "IadLimitMarker1[0-2]",
            "documents": ["IadSearchLimit.*.cls"],
            "namespace": "USER",
            "regex": regex,
            "inline": true
        }))
        .unwrap();
        found.push(result_json(
            handle_iris_search(&iris, &client, p, make_log_store()).await,
        ));
    }
    drop_limit_fixture(&iris, &client).await;
    assert_eq!(found[0]["total_found"], 3, "regex: {}", found[0]);
    assert_eq!(found[1]["total_found"], 0, "literal: {}", found[1]);
}
