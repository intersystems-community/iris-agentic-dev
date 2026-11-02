//! `iris_check` against iris-dev-iris: a document stored on the server is fetched and checked, and
//! a name it does not define is resolved through iad's connection, so a wrong member of a system
//! class is an error rather than a hint.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS
//! IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test --features testing --test integration
//! test_check_ascot -- --include-ignored --test-threads=1`

use iris_agentic_dev_core::testing::{live_env, McpSession};

fn body(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content: {answer}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

#[test]
#[ignore = "requires live IRIS"]
fn a_stored_document_is_fetched_and_a_wrong_system_member_is_an_error() {
    let mut env = live_env();
    env.retain(|(k, _)| k != "IRIS_CONTAINER");
    let mut s = McpSession::start(&env);
    let name = "Live.CheckProbe.cls";
    let source = "Class Live.CheckProbe\n{\nClassMethod Run()\n{\n Write ##class(%SYSTEM.Version).GetVersion()\n Write ##class(%SYSTEM.Version).NoSuchMethod()\n}\n}\n";
    let put = body(&s.call(
        "iris_doc",
        &serde_json::json!({"mode": "put", "name": name, "content": source, "namespace": "USER"}),
    ));
    assert_eq!(put["success"], true, "{put}");
    let b = body(&s.call(
        "iris_check",
        &serde_json::json!({"name": name, "namespace": "USER"}),
    ));
    s.call(
        "iris_doc",
        &serde_json::json!({"mode": "delete", "name": name, "namespace": "USER"}),
    );
    assert_eq!(b["success"], true, "{b}");
    assert_eq!(b["iris"], true, "{b}");
    assert!(b["note"].is_null(), "{b}");
    assert_eq!(b["errors"], 1, "{b}");
    assert_eq!(b["hints"], 0, "{b}");
    assert!(
        b["diagnostics"][0]["message"]
            .as_str()
            .unwrap_or("")
            .contains("NoSuchMethod"),
        "{b}"
    );
    let missing = body(&s.call(
        "iris_check",
        &serde_json::json!({"name": "Live.NoSuchDoc.cls", "namespace": "USER"}),
    ));
    assert_eq!(missing["error_code"], "DOCUMENT_NOT_FOUND", "{missing}");
}
