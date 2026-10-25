//! 132 B2 and B3 against live IRIS. Pure half: `tests/unit/test_tool_fixes_132.rs`.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   cargo test --features testing --test integration test_tool_fixes_132 -- --include-ignored --test-threads=1

use std::sync::Arc;

use iris_agentic_dev_core::iris::connection::{AtelierVersion, DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::ws_session::WsSessionPool;
use iris_agentic_dev_core::tools::doc::handle_iris_execute_method;
use iris_agentic_dev_core::tools::IrisExecuteMethodParams;

fn conn() -> Option<IrisConnection> {
    let host = std::env::var("IRIS_HOST").ok().filter(|s| !s.is_empty())?;
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    Some(IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string()),
        std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string()),
        DiscoverySource::EnvVar,
    ))
}

async fn call(class: &str, method: &str, args: &[&str]) -> serde_json::Value {
    let iris = conn().expect("IRIS_HOST not set");
    let p = IrisExecuteMethodParams {
        confirm: false,
        class: class.to_string(),
        method: method.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        namespace: Some("USER".to_string()),
        server: None,
    };
    let result = handle_iris_execute_method(&iris, &reqwest::Client::new(), &p)
        .await
        .unwrap();
    serde_json::from_str(&result.content[0].as_text().unwrap().text).unwrap()
}

#[tokio::test]
#[ignore = "requires live IRIS"]
async fn a_method_returning_an_error_status_reports_its_text_132() {
    let v = call("%Library.Integer", "IsValid", &["abc"]).await;
    assert_eq!(v["success"], false, "{v}");
    assert_eq!(v["error_code"], "METHOD_RETURNED_ERROR", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(err.contains("ERROR #"), "no decoded status text: {v}");
    assert!(err.contains("%Library.Integer.IsValid"), "{v}");
    assert!(!err.contains('\u{0}'), "raw $lb bytes left in: {v}");
}

#[tokio::test]
#[ignore = "requires live IRIS"]
async fn an_ok_status_and_a_plain_value_are_unchanged_132() {
    let ok = call("%Library.Integer", "IsValid", &["42"]).await;
    assert_eq!(ok["success"], true, "{ok}");
    assert_eq!(ok["return_value"], "1", "{ok}");
    // A value that starts with "0 " but is not a $lb stays a value.
    let text = call("%Library.String", "LogicalToDisplay", &["0 apples"]).await;
    assert_eq!(text["success"], true, "{text}");
    assert_eq!(text["return_value"], "0 apples", "{text}");
}

#[tokio::test]
#[ignore = "requires live IRIS with Atelier v7+"]
async fn multi_line_ws_exec_runs_each_line_132() {
    let mut iris = conn().expect("IRIS_HOST not set");
    iris.atelier_version = AtelierVersion::V8;
    let pool = Arc::new(WsSessionPool::new());
    let token = WsSessionPool::open(&pool, &iris, "dev", "USER")
        .await
        .expect("open WS session");

    let out = WsSessionPool::exec(&pool, &token, "Set tA=1\nSet tB=2\r\n\nWrite tA+tB").await;
    let persisted = WsSessionPool::exec(&pool, &token, "Write tB").await;
    let _ = WsSessionPool::close(&pool, &token).await;

    let out = out.expect("exec");
    assert!(!out.contains("<SYNTAX>"), "multi-line code failed: {out:?}");
    assert!(out.contains('3'), "Write tA+tB gave no 3: {out:?}");
    assert!(persisted.expect("exec").contains('2'), "tB did not persist");
}
