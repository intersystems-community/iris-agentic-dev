//! Integration tests for iris_query SQL power extensions (057-sql-power): explain, count, write.
//! Requires live IRIS on iris-dev-iris container (port from IRIS_HOST/IRIS_WEB_PORT env).
//! All tests are #[ignore] — run with:
//!   cargo test -p iris-agentic-dev-core --features testing --test test_sql_power_live -- --include-ignored
//!
//! These tests exercise the mode dispatch via a direct HTTP call to a locally-spawned
//! iris-dev binary's MCP endpoint is NOT used here — instead they call the Atelier REST
//! endpoint directly with the same SQL the implementation generates, to validate the
//! IRIS-side behavior these modes depend on (EXPLAIN, COUNT, %SQL.Statement rowcount).
//! Full end-to-end tool-call coverage lives in the unit tests using mocked gates; these
//! integration tests validate the underlying IRIS API assumptions documented in research.md.

use iris_agentic_dev_core::iris::connection::DiscoverySource;
use iris_agentic_dev_core::iris::IrisConnection;
use iris_agentic_dev_core::tools::IrisTools;

fn make_iris_tools() -> IrisTools {
    IrisTools::new(Some(live_iris())).expect("IrisTools::new")
}

/// Serializes tests that PUT+compile the shared IrisDevTest.SqlPower class. Without
/// this, two tests running concurrently can race — one's PUT+compile leaves the class
/// transiently uncompiled while another's INSERT/TRUNCATE runs against it, producing
/// "Table not found, class has not been fully compiled" (observed in CI).
static SQL_POWER_TABLE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn parse_call_result(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let r = r.expect("call_for_test returned Err");
    let text = r.content[0].raw.as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({"raw": text}))
}

async fn put_and_compile_test_table(iris: &IrisConnection) {
    let client = IrisConnection::http_client().expect("http client");
    let put_url = iris.versioned_ns_url("USER", "/doc/IrisDevTest.SqlPower.cls");
    let _ = client
        .put(&put_url)
        .basic_auth(&iris.username, Some(&iris.password))
        .json(&serde_json::json!({
            "enc": false,
            "content": [
                "Class IrisDevTest.SqlPower Extends %Persistent",
                "{",
                "Property Name As %String;",
                "}"
            ]
        }))
        .send()
        .await
        .expect("PUT");
    let compile_url = iris.versioned_ns_url("USER", "/action/compile?flags=cuk");
    let _ = client
        .post(&compile_url)
        .basic_auth(&iris.username, Some(&iris.password))
        .json(&serde_json::json!(["IrisDevTest.SqlPower.cls"]))
        .send()
        .await
        .expect("compile");
}

// ---------------------------------------------------------------------------
// T033 / US3: write mode INSERT -> rows_affected=1
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_write_insert_returns_rows_affected_one() {
    let _guard = SQL_POWER_TABLE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let iris = live_iris();
    put_and_compile_test_table(&iris).await;
    let tools = make_iris_tools();

    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "INSERT INTO IrisDevTest.SqlPower (Name) VALUES ('sql_power_test')",
                "mode": "write",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_call_result(result);
    assert_eq!(v["success"], true, "got: {v}");
    assert_eq!(v["rows_affected"], 1);
}

// ---------------------------------------------------------------------------
// T033 / US3: write mode on mcpTemplate=live -> ENV_GATE_BLOCKED (via check_env_gate directly,
// since IrisTools::call_for_test bypasses the mcpTemplate policy layer, which lives at the
// server_manager/policy config level, not on IrisTools itself)
// ---------------------------------------------------------------------------

#[test]
fn write_mode_env_gate_blocked_on_live_direct() {
    use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
    use iris_agentic_dev_core::policy::env_gate::check_env_gate;
    let params = serde_json::json!({"mode": "write"});
    let result = check_env_gate("iris_query", &McpTemplate::Live, "test-server", &params);
    assert!(result.is_some());
    assert_eq!(result.unwrap()["error_code"], "ENV_GATE_BLOCKED");
}

// ---------------------------------------------------------------------------
// T033 / US3: write mode DDL rejected before any IRIS mutation
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_write_ddl_rejected_before_iris_call() {
    let tools = make_iris_tools();
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "CREATE TABLE IrisDevTest.ShouldNotExist (id INT)",
                "mode": "write",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_call_result(result);
    assert_eq!(v["error_code"], "DDL_NOT_ALLOWED", "got: {v}");
}

// ---------------------------------------------------------------------------
// T033 / US3: TRUNCATE via write mode succeeds
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_write_truncate_succeeds() {
    let _guard = SQL_POWER_TABLE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let iris = live_iris();
    put_and_compile_test_table(&iris).await;
    let tools = make_iris_tools();

    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "TRUNCATE TABLE IrisDevTest.SqlPower",
                "mode": "write",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_call_result(result);
    assert_eq!(v["success"], true, "got: {v}");
}

fn live_iris() -> IrisConnection {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52773);
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    IrisConnection::new(
        format!("http://{}:{}", host, port),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    )
}

// ---------------------------------------------------------------------------
// T016 / US1: EXPLAIN returns non-empty plan_text-equivalent
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_explain_returns_nonempty_plan() {
    let iris = live_iris();
    let client = IrisConnection::http_client().expect("http client");
    let query_url = iris.versioned_ns_url("USER", "/action/query");
    let resp = client
        .post(&query_url)
        .basic_auth(&iris.username, Some(&iris.password))
        .json(&serde_json::json!({"query": "EXPLAIN SELECT TOP 5 * FROM %Dictionary.ClassDefinition"}))
        .send()
        .await
        .expect("request");
    let body: serde_json::Value = resp.json().await.expect("json");
    let plan = body["result"]["content"][0]["Plan"].as_str().unwrap_or("");
    assert!(!plan.is_empty(), "got: {body}");
}

// ---------------------------------------------------------------------------
// T023 / US2: count query matches direct SELECT COUNT(*)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_count_matches_direct_select_count() {
    let iris = live_iris();
    let client = IrisConnection::http_client().expect("http client");
    let query_url = iris.versioned_ns_url("USER", "/action/query");

    async fn run_count(
        client: &reqwest::Client,
        url: &str,
        iris: &IrisConnection,
        sql: &str,
    ) -> i64 {
        let resp = client
            .post(url)
            .basic_auth(&iris.username, Some(&iris.password))
            .json(&serde_json::json!({"query": sql}))
            .send()
            .await
            .expect("request");
        let body: serde_json::Value = resp.json().await.expect("json");
        body["result"]["content"][0]
            .as_object()
            .and_then(|o| o.values().next())
            .and_then(|v| v.as_i64())
            .unwrap_or(-1)
    }

    // Filter to a single fixed, always-compiled system class rather than counting the
    // whole %Dictionary.ClassDefinition table — other tests in this suite compile/drop
    // classes concurrently, and counting the live mutable table races between the two
    // sequential HTTP calls below (observed: 9867 vs 9869 on a concurrent run).
    let filter = "WHERE Name = '%Library.RegisteredObject'";
    let direct = run_count(
        &client,
        &query_url,
        &iris,
        &format!("SELECT COUNT(*) FROM %Dictionary.ClassDefinition {filter}"),
    )
    .await;
    let via_table = run_count(
        &client,
        &query_url,
        &iris,
        &iris_agentic_dev_core::tools::build_count_query(
            None,
            Some(&format!(
                "SELECT * FROM %Dictionary.ClassDefinition {filter}"
            )),
        ),
    )
    .await;
    assert_eq!(direct, via_table);
    assert_eq!(
        direct, 1,
        "expected exactly one match for the fixed class filter"
    );
    assert!(direct > 0, "expected non-zero class count, got {direct}");
}
