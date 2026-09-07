//! A value outside a declared enum is still rejected by the handler, with the same message.
//!
//! The enums added for FR-006 are schema annotations. `#[schemars(extend("enum" = [...]))]` changes
//! what `tools/list` advertises; it does not change what serde accepts, so `{"mode": "bogus"}` still
//! deserializes into `Option<String>` and still reaches the handler. That is deliberate (research
//! decision 2: handler validation is unchanged) and it is the part worth pinning — a client that
//! ignores the enum must keep getting the error that names the permitted values, not a serde
//! type-mismatch that names none of them.
//!
//! These run against live IRIS because both handlers acquire a connection before they validate.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

fn make_iris_tools() -> Option<iris_agentic_dev_core::tools::IrisTools> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52773".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let conn = IrisConnection::new(
        format!("http://{host}:{web_port}"),
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    Some(iris_agentic_dev_core::tools::IrisTools::new(Some(conn)).expect("IrisTools::new"))
}

fn parse_result(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let r = r.expect("call_for_test returned Err");
    let text = r.content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({"raw": text}))
}

/// The message must list the values, not merely say no.
#[tokio::test]
async fn a_mode_outside_the_enum_names_the_permitted_values() {
    let Some(tools) = make_iris_tools() else {
        return;
    };
    let v = parse_result(
        tools
            .call_for_test(
                "iris_system_performance",
                serde_json::json!({"mode": "bogus"}),
            )
            .await,
    );
    assert_eq!(
        v.get("success"),
        Some(&serde_json::json!(false)),
        "a mode outside the declared enum must fail: {v}"
    );
    let error = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_else(|| panic!("rejection must carry an `error` string: {v}"));
    assert!(
        error.contains("bogus"),
        "the error must quote the value the caller sent: {error}"
    );
    for value in ["start", "status", "last_runid"] {
        assert!(
            error.contains(value),
            "the error must list `{value}` among the valid modes so the caller can correct the \
             call without reading the docs: {error}"
        );
    }
}

/// `iris_admin` refuses through the dispatch table's catch-all arm, with its own error code. This is
/// the response the enum's twenty-five values are drawn from, so it has to keep saying INVALID_ACTION
/// and not degrade into a deserialization failure.
#[tokio::test]
async fn an_action_outside_the_enum_returns_invalid_action() {
    let Some(tools) = make_iris_tools() else {
        return;
    };
    let v = parse_result(
        tools
            .call_for_test("iris_admin", serde_json::json!({"action": "bogus"}))
            .await,
    );
    assert_eq!(
        v.get("error_code"),
        Some(&serde_json::json!("INVALID_ACTION")),
        "an unlisted action must come back as INVALID_ACTION: {v}"
    );
    let error = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_else(|| panic!("rejection must carry an `error` string: {v}"));
    assert!(
        error.contains("list_namespaces") && error.contains("mirror_failover"),
        "the error must enumerate the accepted actions: {error}"
    );
}

/// The other half of the same claim: a value *inside* the enum is not rejected. Without this, an
/// enum that accidentally excluded every working value would still pass the test above.
#[tokio::test]
async fn a_mode_inside_the_enum_is_not_refused_for_its_value() {
    let Some(tools) = make_iris_tools() else {
        return;
    };
    let v = parse_result(
        tools
            .call_for_test(
                "iris_system_performance",
                serde_json::json!({"mode": "last_runid"}),
            )
            .await,
    );
    let error = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
    assert!(
        !error.contains("unknown mode"),
        "`last_runid` is in the declared enum and must not be refused as an unknown mode: {v}"
    );
}
