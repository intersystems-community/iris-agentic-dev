//! Live integration tests for iris_doc — exercises handle_iris_doc against real IRIS.
//! All tests are `#[ignore]` — run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   cargo test --test test_doc_live -- --ignored --nocapture --test-threads=1

use iris_agentic_dev_core::elicitation::{CheckoutCache, ElicitationStore};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::doc::{handle_iris_doc, IrisDocParams};

/// Helper: create an IrisConnection from environment variables.
/// Returns None if IRIS_HOST is not set (tests are skipped in that case).
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
    Some((conn, reqwest::Client::new()))
}

/// Helper: extract JSON result from CallToolResult.
fn result_json(r: Result<rmcp::model::CallToolResult, rmcp::ErrorData>) -> serde_json::Value {
    let text = r.unwrap().content[0].raw.as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap()
}

// ────────────────────────────────────────────────────────────────────────────
// 1. PUT and GET round-trip
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_put_and_get_cls() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let name = "CoverageTest.DocLive.cls";
    let content = "Class CoverageTest.DocLive {}";

    // PUT the document
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "put",
        "name": name,
        "content": content,
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "put failed: {}", json);

    // GET the document
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "get",
        "name": name,
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "get failed: {}", json);
    assert!(json["content"].as_str().unwrap().contains("DocLive"));

    // Cleanup: DELETE
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "delete",
        "name": name,
        "namespace": "USER"
    }))
    .unwrap();
    let _ = handle_iris_doc(&iris, &client, p, &store, &cache).await;
}

// ────────────────────────────────────────────────────────────────────────────
// 2. PUT with .mac extension (ROUTINE header injection)
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_put_mac_injects_routine_header() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let name = "CoverageTest.DocLiveMac.mac";
    let content = "Write \"Hello\"\nQuit";

    // PUT the .mac file (no ROUTINE header in input)
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "put",
        "name": name,
        "content": content,
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "put .mac failed: {}", json);

    // Cleanup: DELETE
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "delete",
        "name": name,
        "namespace": "USER"
    }))
    .unwrap();
    let _ = handle_iris_doc(&iris, &client, p, &store, &cache).await;
}

// ────────────────────────────────────────────────────────────────────────────
// 3. PUT with .inc extension (INC header injection)
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_put_inc_injects_routine_header() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let name = "CoverageTest.DocLiveInc.inc";
    let content = "#define MyMacro 123";

    // PUT the .inc file (no ROUTINE header in input)
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "put",
        "name": name,
        "content": content,
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "put .inc failed: {}", json);

    // Cleanup: DELETE
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "delete",
        "name": name,
        "namespace": "USER"
    }))
    .unwrap();
    let _ = handle_iris_doc(&iris, &client, p, &store, &cache).await;
}

// ────────────────────────────────────────────────────────────────────────────
// 4. HEAD on an existing document (%Library.Object always exists)
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_head_exists() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    // Put a class first so we have something known to HEAD against
    let put_p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "put",
        "name": "CoverageTest.HeadExists.cls",
        "content": "Class CoverageTest.HeadExists {}",
        "namespace": "USER"
    }))
    .unwrap();
    let _ = handle_iris_doc(&iris, &client, put_p, &store, &cache).await;

    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "head",
        "name": "CoverageTest.HeadExists.cls",
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "head failed: {}", json);
    assert_eq!(json["exists"], true, "exists should be true");

    // Cleanup
    let del_p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "delete",
        "name": "CoverageTest.HeadExists.cls",
        "namespace": "USER"
    }))
    .unwrap();
    let _ = handle_iris_doc(&iris, &client, del_p, &store, &cache).await;
}

// ────────────────────────────────────────────────────────────────────────────
// 5. HEAD on a nonexistent document (should return NOT_FOUND in result, not exists=false)
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_head_not_found() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "head",
        "name": "CoverageTest.DoesNotExist99.cls",
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    // head returns success:true always, but exists:false for 404
    assert_eq!(json["success"], true);
    assert_eq!(json["exists"], false, "exists should be false for 404");
}

// ────────────────────────────────────────────────────────────────────────────
// 6. GET on a nonexistent document
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_get_not_found() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "get",
        "name": "CoverageTest.DoesNotExist99.cls",
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], false);
    assert_eq!(json["error_code"], "NOT_FOUND", "error_code: {}", json);
}

// ────────────────────────────────────────────────────────────────────────────
// 7. Batch GET with multiple documents
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_batch_get() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "get",
        "names": ["%Library.Object.cls", "%Library.Persistent.cls"],
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "batch get failed: {}", json);
    let docs = json["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 2, "expected 2 documents, got {}", docs.len());
}

// ────────────────────────────────────────────────────────────────────────────
// 8. INSERT and DELETE_LINES sequence
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_insert_and_delete_lines() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let name = "CoverageTest.DocLiveInsert.cls";

    // 8a. PUT a class with 3 lines
    let initial_content = "Class CoverageTest.DocLiveInsert {\n\n}";
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "put",
        "name": name,
        "content": initial_content,
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "put failed: {}", json);

    // 8b. Fetch back via get to read actual line 2 content (IRIS normalizes class layout)
    let fetch_p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "get",
        "name": name,
        "namespace": "USER"
    }))
    .unwrap();
    let fetch_result = handle_iris_doc(&iris, &client, fetch_p, &store, &cache).await;
    let fetch_json = result_json(fetch_result);
    // IRIS normalizes "Class Foo {\n\n}" → ["Class Foo", "{", "", "}"], so line 2 = "{"
    // Use the actual content at line index 1 (0-based) as the stale-edit guard.
    let line2_content = fetch_json["content"]
        .as_str()
        .unwrap_or("")
        .lines()
        .nth(1)
        .unwrap_or("")
        .to_string();

    // INSERT a method before line 2, using the actual current line content as guard
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "insert",
        "name": name,
        "line": 2,
        "content": "  Method Test() {}",
        "expected": line2_content,
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "insert failed: {}", json);
    assert_eq!(json["edit"], "insert");

    // 8c. GET to verify the inserted line is there; capture IRIS-normalized content at line 2
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "get",
        "name": name,
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true);
    assert!(
        json["content"].as_str().unwrap().contains("Method Test"),
        "inserted method not found"
    );
    // IRIS normalizes "Method Test() {}" → "Method Test()" (strips empty body braces).
    // Capture the actual stored line 2 so delete_lines' `expected` guard matches reality.
    let inserted_line2 = json["content"]
        .as_str()
        .unwrap_or("")
        .lines()
        .nth(1)
        .unwrap_or("")
        .to_string();

    // 8d. DELETE_LINES to remove the inserted line, using IRIS-normalized content as guard
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "delete_lines",
        "name": name,
        "start": 2,
        "end": 2,
        "expected": inserted_line2,
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "delete_lines failed: {}", json);
    assert_eq!(json["edit"], "delete_lines");

    // Cleanup: DELETE
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "delete",
        "name": name,
        "namespace": "USER"
    }))
    .unwrap();
    let _ = handle_iris_doc(&iris, &client, p, &store, &cache).await;
}

// ────────────────────────────────────────────────────────────────────────────
// 9. PUT with empty name returns MISSING_PARAMS
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_put_missing_name_returns_error() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "put",
        "name": "",
        "content": "Class Foo {}",
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], false);
    assert_eq!(json["error_code"], "MISSING_PARAMS", "error: {}", json);
}

// ────────────────────────────────────────────────────────────────────────────
// 10. DELETE on an existing document
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_delete() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let name = "CoverageTest.DocLiveDelete.cls";

    // PUT a document
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "put",
        "name": name,
        "content": "Class CoverageTest.DocLiveDelete {}",
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "put failed: {}", json);

    // DELETE it
    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "delete",
        "name": name,
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], true, "delete failed: {}", json);
}

// ────────────────────────────────────────────────────────────────────────────
// 11. GET with no name (require_name guard)
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_doc_get_missing_name_returns_error() {
    let Some((iris, client)) = make_conn() else {
        return;
    };
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    let p: IrisDocParams = serde_json::from_value(serde_json::json!({
        "mode": "get",
        "namespace": "USER"
    }))
    .unwrap();
    let result = handle_iris_doc(&iris, &client, p, &store, &cache).await;
    let json = result_json(result);
    assert_eq!(json["success"], false);
    assert_eq!(json["error_code"], "MISSING_PARAMS", "error: {}", json);
}
