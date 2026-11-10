//! Live integration test for iris_doc PUT result.status error detection (spec 172)
//! Tests that the tool detects errors in result.status when status.errors is empty.
//! This reproduces the bug: IRIS 2026.2 can return HTTP 200 with status.errors=[] but error in result.status.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   cargo test --test integration test_doc_put_result_status -- --ignored --nocapture --test-threads=1

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};

/// Helper: an `IrisTools` over the live instance named by the environment, with both write gates
/// declared open so the calls reach the handler through the pipeline.
/// Returns None if IRIS_HOST is not set (tests are skipped in that case).
fn make_tools() -> Option<IrisTools> {
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
    Some(IrisTools::for_test_gates_open(Some(conn), Toolset::Merged).unwrap())
}

/// Helper: extract JSON result from CallToolResult.
fn result_json(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let text = r.unwrap().content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap()
}

/// Test that iris_doc PUT detects errors in result.status when status.errors is empty.
/// The bug: a class with invalid Storage (subscript name="a" instead of name="1") causes
/// IRIS to return HTTP 200, status.errors=[], and error text in result.status.
/// iad must detect this and report an error, not pretend the PUT succeeded.
#[tokio::test]
#[ignore]
async fn test_doc_put_result_status_error_in_empty_errors() {
    let Some(tools) = make_tools() else {
        return;
    };

    let name = "IadLivePutStatus.Bad.cls";

    // A class with invalid Storage: subscript name="a" instead of integer name="1"
    // This triggers ERROR #16023 in result.status with empty status.errors
    let bad_content = r#"Class IadLivePutStatus.Bad Extends %Persistent
{
    Property Name As %String;
    Property Age As %Integer;

    Storage SQLStorage
    {
    <SQLMap name="Map1">
    <Data name="Age">
    <Delimiter>"^"</Delimiter>
    <Piece>2</Piece>
    </Data>
    <Data name="Name">
    <Delimiter>"^"</Delimiter>
    <Piece>1</Piece>
    </Data>
    <Global>^IadLivePutStatus</Global>
    <Subscript name="a">
    <Expression>{ID}</Expression>
    </Subscript>
    <Type>data</Type>
    </SQLMap>
    <StreamLocation>^IadLivePutStatusS</StreamLocation>
    <Type>%Storage.SQL</Type>
    }

    Parameter SqlRowIdName = "ID";
}"#;

    // PUT the bad class — should fail with UPLOAD_FAILED and error text in response
    let p = serde_json::json!({
        "mode": "put",
        "name": name,
        "content": bad_content,
        "namespace": "USER"
    });
    let result = tools.call_gated("iris_doc", p).await;
    let json = result_json(result);

    // Clean up before asserting, so a failing run leaves nothing behind. The bad PUT leaves a
    // class IRIS cannot read back, so a valid PUT goes first, then the delete; both are checked.
    let restore = tools
        .call_gated(
            "iris_doc",
            serde_json::json!({
                "mode": "put",
                "name": name,
                "content": "Class IadLivePutStatus.Bad\n{\n}\n",
                "namespace": "USER"
            }),
        )
        .await;
    assert_eq!(
        result_json(restore)["success"],
        true,
        "restoring PUT failed"
    );
    let deleted = tools
        .call_gated(
            "iris_doc",
            serde_json::json!({"mode": "delete", "name": name, "namespace": "USER"}),
        )
        .await;
    assert_eq!(
        result_json(deleted)["success"],
        true,
        "cleanup delete failed"
    );

    // The fix: we expect an error, not success
    assert_eq!(
        json["success"], false,
        "PUT of bad Storage should fail, but it succeeded: {}",
        json
    );
    assert_eq!(
        json["error_code"], "UPLOAD_FAILED",
        "error code should be UPLOAD_FAILED: {}",
        json
    );
    assert!(
        json["error"]
            .as_str()
            .unwrap_or("")
            .contains("ERROR #16023"),
        "error message should contain #16023 from result.status: {}",
        json["error"]
    );
}

/// Test that a valid PUT still succeeds (regression test).
#[tokio::test]
#[ignore]
async fn test_doc_put_result_status_valid_class_succeeds() {
    let Some(tools) = make_tools() else {
        return;
    };

    let name = "IadLivePutStatus.Good.cls";

    // A valid class with correct Storage (subscript name="1")
    let valid_content = r#"Class IadLivePutStatus.Good Extends %Persistent
{
    Property Name As %String;
    Property Age As %Integer;

    Storage SQLStorage
    {
    <SQLMap name="Map1">
    <Data name="Age">
    <Delimiter>"^"</Delimiter>
    <Piece>2</Piece>
    </Data>
    <Data name="Name">
    <Delimiter>"^"</Delimiter>
    <Piece>1</Piece>
    </Data>
    <Global>^IadLivePutStatus</Global>
    <Subscript name="1">
    <Expression>{ID}</Expression>
    </Subscript>
    <Type>data</Type>
    </SQLMap>
    <StreamLocation>^IadLivePutStatusS</StreamLocation>
    <Type>%Storage.SQL</Type>
    }

    Parameter SqlRowIdName = "ID";
}"#;

    // PUT the good class — should succeed
    let p = serde_json::json!({
        "mode": "put",
        "name": name,
        "content": valid_content,
        "namespace": "USER"
    });
    let result = tools.call_gated("iris_doc", p).await;
    let json = result_json(result);

    assert_eq!(
        json["success"], true,
        "PUT of valid Storage should succeed: {}",
        json
    );

    // Cleanup: DELETE the class
    let delete_p = serde_json::json!({
        "mode": "delete",
        "name": name,
        "namespace": "USER"
    });
    let _ = tools.call_gated("iris_doc", delete_p).await;
}
