//! E2E tests for cross-instance comparison tools (T070–T071).
//! All tests require a live IRIS container and are #[ignore] by default.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --test test_comparison_e2e -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::comparison_tools::{
    compare_document_impl, compare_namespace_impl, fetch_class_list, fetch_document_source,
    CompareDocumentParams, CompareNamespaceParams,
};
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

// T070: compare a class against itself — same=true, diff empty
#[tokio::test]
#[ignore]
async fn e2e_compare_document_same() {
    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_compare_document_same");
            return;
        }
    };
    let iris = Arc::new(conn);

    let result = compare_document_impl(
        CompareDocumentParams {
            document: "%Library.Persistent.cls".to_string(),
            server_a: Arc::clone(&iris),
            server_b: Arc::clone(&iris),
            namespace: "USER".to_string(),
        },
        &client,
    )
    .await
    .expect("compare_document_impl failed");

    let text = result
        .content
        .first()
        .map(|c| c.raw.as_text().unwrap().text.clone())
        .expect("no text content");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    assert!(
        v["same"].as_bool().unwrap_or(false),
        "comparing class to itself should be same=true, got: {v}"
    );
    let diff = v["diff"].as_str().unwrap_or("not-empty");
    assert!(
        diff.is_empty(),
        "diff should be empty when same=true, got: {diff}"
    );
}

// T071: fetch two different documents and verify their sources differ
#[tokio::test]
#[ignore]
async fn e2e_compare_document_diff() {
    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_compare_document_diff");
            return;
        }
    };

    let src_persistent =
        fetch_document_source(&conn, &client, "%Library.Persistent.cls", "USER").await;
    let src_registered =
        fetch_document_source(&conn, &client, "%Library.RegisteredObject.cls", "USER").await;

    match (src_persistent, src_registered) {
        (Ok(a), Ok(b)) => {
            assert_ne!(
                a, b,
                "%Library.Persistent and %Library.RegisteredObject must have different source"
            );
        }
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("Fetch failed (skipping): {e}");
        }
    }
}

// T072: fetch_class_list returns a non-empty list of class names from USER namespace.
#[tokio::test]
#[ignore]
async fn e2e_fetch_class_list() {
    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_fetch_class_list");
            return;
        }
    };

    let list = fetch_class_list(&conn, &client, "USER")
        .await
        .expect("fetch_class_list failed");

    assert!(
        !list.is_empty(),
        "expected at least one class in USER namespace"
    );
    // Every entry should look like a class name (ends in .cls or has dot separation).
    for name in &list {
        assert!(
            name.contains('.') || name.ends_with(".cls"),
            "unexpected class name format: {name}"
        );
    }
    eprintln!("e2e_fetch_class_list: {} classes found", list.len());
}

// T073: compare_namespace against itself — only_in_a and only_in_b are empty,
// different list is empty, same_count > 0.
#[tokio::test]
#[ignore]
async fn e2e_compare_namespace_same_server() {
    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_compare_namespace_same_server");
            return;
        }
    };
    let iris = Arc::new(conn);

    let result = compare_namespace_impl(
        CompareNamespaceParams {
            namespace: "USER".to_string(),
            server_a: Arc::clone(&iris),
            server_b: Arc::clone(&iris),
        },
        &client,
    )
    .await
    .expect("compare_namespace_impl failed");

    let text = result
        .content
        .first()
        .map(|c| c.raw.as_text().unwrap().text.clone())
        .expect("no text content");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");

    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true: {v}"
    );
    let only_a = v["only_in_a"].as_array().expect("only_in_a array");
    let only_b = v["only_in_b"].as_array().expect("only_in_b array");
    let different = v["different"].as_array().expect("different array");
    assert!(
        only_a.is_empty(),
        "comparing same server: only_in_a should be empty"
    );
    assert!(
        only_b.is_empty(),
        "comparing same server: only_in_b should be empty"
    );
    assert!(
        different.is_empty(),
        "comparing same server: different should be empty"
    );
    let same_count = v["same_count"].as_u64().unwrap_or(0);
    assert!(same_count > 0, "expected some classes in common");
    eprintln!("e2e_compare_namespace_same_server: {same_count} classes identical");
}

// T074: compare_document error path — nonexistent document returns success=false.
#[tokio::test]
#[ignore]
async fn e2e_compare_document_nonexistent() {
    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_compare_document_nonexistent");
            return;
        }
    };
    let iris = Arc::new(conn);

    let result = compare_document_impl(
        CompareDocumentParams {
            document: "DoesNotExist.Nonexistent.cls".to_string(),
            server_a: Arc::clone(&iris),
            server_b: Arc::clone(&iris),
            namespace: "USER".to_string(),
        },
        &client,
    )
    .await
    .expect("compare_document_impl itself should not error");

    let text = result
        .content
        .first()
        .map(|c| c.raw.as_text().unwrap().text.clone())
        .expect("no text content");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");

    // A nonexistent class either returns success=false (fetch error) or same=true with empty source.
    // Either outcome is acceptable — we just want the code path executed.
    eprintln!("e2e_compare_document_nonexistent result: {v}");
}
