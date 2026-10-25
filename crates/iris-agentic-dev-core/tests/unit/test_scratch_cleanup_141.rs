//! Spec 141 T201: how the scratch-class cleanup reads a DELETE reply.
//!
//! The live half is `tests/integration/test_scratch_cleanup_141_live.rs`, which gets the 200-with-
//! errors reply from a real IRIS. These pin the reading of each shape, so a change to one arm
//! shows up here without a container.

use iris_agentic_dev_core::iris::connection::{delete_outcome, DeleteOutcome};
use iris_agentic_dev_core::tools::with_warnings;
use reqwest::StatusCode;
use rmcp::model::CallToolResult;
use serde_json::json;

const REFUSED: &str = r#"{"status":{"errors":[{"error":"ERROR #5845: Cannot delete Foo.cls, it is checked out","code":5845}],"summary":"x"},"console":[],"result":{}}"#;
/// What IRIS 2026.2 answered when a source-control class refused in `OnBeforeDelete`: a 200, no
/// `status.errors`, the reason in `result.status` and the console.
const REFUSED_BY_SC: &str = r#"{"status":{"errors":[],"summary":""},"console":["","Deleting class IrisDevTmp.X","ERROR #5001: ScratchSC141 refuses to delete IrisDevTmp.X.CLS"],"result":{"name":"IrisDevTmp.X.cls","db":"","ts":"","cat":"CLS","enc":false,"content":"","status":"ERROR #5001: ScratchSC141 refuses to delete IrisDevTmp.X.CLS"}}"#;
const DONE: &str =
    r#"{"status":{"errors":[],"summary":""},"console":[],"result":{"name":"Foo.cls"}}"#;

#[test]
fn a_clean_200_is_gone() {
    assert_eq!(delete_outcome(StatusCode::OK, DONE), DeleteOutcome::Gone);
}

/// IRIS refuses a locked or checked-out document with a 200 and the reason in `status.errors`.
#[test]
fn a_200_with_errors_is_a_refusal_in_irises_words() {
    match delete_outcome(StatusCode::OK, REFUSED) {
        DeleteOutcome::Refused(msg) => assert!(msg.contains("5845"), "{msg}"),
        other => panic!("expected Refused, got {other:?}"),
    }
}

/// The shape a live IRIS gives for a source-control refusal has `status.errors` empty.
#[test]
fn a_200_with_the_reason_in_result_status_is_a_refusal() {
    match delete_outcome(StatusCode::OK, REFUSED_BY_SC) {
        DeleteOutcome::Refused(msg) => assert!(msg.contains("refuses to delete"), "{msg}"),
        other => panic!("expected Refused, got {other:?}"),
    }
}

/// A 404 is the document already gone, which is what the delete wanted.
#[test]
fn a_404_is_gone() {
    assert_eq!(
        delete_outcome(StatusCode::NOT_FOUND, ""),
        DeleteOutcome::Gone
    );
    assert_eq!(
        delete_outcome(StatusCode::NOT_FOUND, REFUSED),
        DeleteOutcome::Gone
    );
}

#[test]
fn any_other_status_leaves_the_document_unknown() {
    for status in [
        StatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::BAD_GATEWAY,
    ] {
        assert_eq!(
            delete_outcome(status, DONE),
            DeleteOutcome::HttpError(status.as_u16()),
            "{status}"
        );
    }
}

#[test]
fn only_gone_lets_the_guard_stand_down() {
    assert!(DeleteOutcome::Gone.is_gone());
    assert!(!DeleteOutcome::Refused("x".into()).is_gone());
    assert!(!DeleteOutcome::HttpError(500).is_gone());
    for o in [
        DeleteOutcome::Refused("locked".into()),
        DeleteOutcome::HttpError(500),
    ] {
        assert_ne!(o.why(), "deleted", "{o:?} must not read as a delete");
    }
}

/// An empty `errors` array, a body that is not JSON, and a body with no `status` are not refusals:
/// only the shape known to carry one is read as one.
#[test]
fn a_2xx_with_no_errors_listed_is_gone() {
    for body in ["", "not json", "{}", r#"{"status":{"errors":[]}}"#] {
        assert_eq!(
            delete_outcome(StatusCode::OK, body),
            DeleteOutcome::Gone,
            "{body:?}"
        );
    }
}

#[test]
fn warnings_are_added_to_the_body_and_keep_its_error_flag() {
    let ok = with_warnings(
        CallToolResult::structured(json!({"success": true, "n": 1})),
        vec!["a class stays".to_string()],
    );
    let body = ok.structured_content.clone().unwrap();
    assert_eq!(body["warnings"], json!(["a class stays"]));
    assert_eq!(body["n"], 1);
    assert_ne!(ok.is_error, Some(true));
    assert!(
        ok.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("a class stays"),
        "the text content must carry it too"
    );

    let failed = with_warnings(
        CallToolResult::structured_error(json!({"success": false, "warnings": ["earlier"]})),
        vec!["later".to_string()],
    );
    assert_eq!(failed.is_error, Some(true));
    assert_eq!(
        failed.structured_content.unwrap()["warnings"],
        json!(["earlier", "later"])
    );

    let untouched = CallToolResult::structured(json!({"success": true}));
    let same = with_warnings(untouched.clone(), Vec::new());
    assert_eq!(same.structured_content, untouched.structured_content);
    assert!(same.structured_content.unwrap().get("warnings").is_none());
}
