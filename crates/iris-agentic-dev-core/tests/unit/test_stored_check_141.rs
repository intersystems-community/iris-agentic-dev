//! Spec 141 FR-015: the stored-source check's readers and listers when the server cannot be
//! reached. The check fails closed: a document it cannot read is a refusal, not a pass, and a
//! channel that failed is reported as the channel failing.
//!
//! Nothing here stands in for IRIS. The HTTP tests aim at a closed port, so the request fails the
//! way a server that is down fails; the exec tests run with no `IRIS_CONTAINER`, so the terminal
//! has nowhere to go. The walk itself is covered in `test_stored_source_141`.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::stored_check::{
    check_exec_target, check_http, doc_name_for, expand_exec, expand_http, read_exec, read_http,
    unreadable_listing, ListFailure, Outcome, Seed, Stored,
};

fn closed_port() -> (IrisConnection, reqwest::Client) {
    (
        IrisConnection::new(
            "http://127.0.0.1:1",
            "USER",
            "u",
            "p",
            DiscoverySource::ExplicitFlag,
        ),
        IrisConnection::http_client().unwrap(),
    )
}

// ── refusal bodies ────────────────────────────────────────────────────────────

#[test]
fn a_wildcard_that_cannot_be_listed_is_refused_and_says_so() {
    let v = unreadable_listing("My.*", "HTTP 500");
    assert_eq!(v["success"], serde_json::json!(false));
    assert_eq!(
        v["error_code"],
        serde_json::json!("STORED_SOURCE_UNREADABLE")
    );
    assert_eq!(v["document"], serde_json::json!("My.*"));
    assert_eq!(
        v["reason"],
        serde_json::json!("the documents matching it could not be listed: HTTP 500")
    );
    assert!(
        v.get("included_by").is_none(),
        "a listing is nobody's include"
    );
    assert!(v["message"]
        .as_str()
        .unwrap()
        .contains("Nothing was compiled"));
}

// ── Atelier, server down ──────────────────────────────────────────────────────

#[tokio::test]
async fn a_document_that_cannot_be_fetched_is_unreadable_not_missing() {
    let (iris, client) = closed_port();
    match read_http(&iris, &client, "USER", "My.Class.cls").await {
        Stored::Unreadable(reason) => assert!(reason.starts_with("the request failed"), "{reason}"),
        other => panic!("a failed request must not read as {other:?}"),
    }
}

#[tokio::test]
async fn a_name_with_no_wildcard_lists_itself_without_asking_the_server() {
    let (iris, client) = closed_port();
    let got = expand_http(&iris, &client, "USER", "My.Class").await;
    assert_eq!(got, Ok(vec!["My.Class".to_string()]));
}

#[tokio::test]
async fn a_wildcard_that_cannot_be_fetched_is_an_error_with_the_reason() {
    let (iris, client) = closed_port();
    let err = expand_http(&iris, &client, "USER", "My.*")
        .await
        .unwrap_err();
    assert!(err.starts_with("the request failed"), "{err}");
}

/// A seed whose text the caller holds and that includes nothing needs no read at all.
#[tokio::test]
async fn text_with_no_includes_is_clear_without_a_server() {
    let (iris, client) = closed_port();
    let seed = Seed::Content {
        doc: "My.Class.cls".into(),
        content: "Class My.Class\n{\n}\n".into(),
    };
    assert_eq!(check_http(&iris, &client, "USER", vec![seed]).await, None);
}

/// An include that has to be read from a server that does not answer is a refusal that names the
/// include and the document that pulled it in.
#[tokio::test]
async fn an_include_that_cannot_be_read_refuses_the_compile() {
    let (iris, client) = closed_port();
    let seed = Seed::Content {
        doc: "My.Class.cls".into(),
        content: "Include MyInc\n\nClass My.Class\n{\n}\n".into(),
    };
    let refusal = check_http(&iris, &client, "USER", vec![seed])
        .await
        .expect("an unreadable include is a refusal");
    assert_eq!(
        refusal["error_code"],
        serde_json::json!("STORED_SOURCE_UNREADABLE")
    );
    assert_eq!(refusal["document"], serde_json::json!("MyInc.inc"));
    assert_eq!(refusal["included_by"], serde_json::json!("My.Class.cls"));
}

#[tokio::test]
async fn a_stored_target_that_cannot_be_read_refuses_the_compile() {
    let (iris, client) = closed_port();
    let refusal = check_http(
        &iris,
        &client,
        "USER",
        vec![Seed::Stored("My.Class.cls".into())],
    )
    .await
    .expect("an unreadable target is a refusal");
    assert_eq!(refusal["document"], serde_json::json!("My.Class.cls"));
    assert!(refusal.get("included_by").is_none());
}

// ── exec, no container ────────────────────────────────────────────────────────

#[test]
fn a_name_with_no_known_extension_is_a_class() {
    assert_eq!(doc_name_for("My.Class"), "My.Class.cls");
    assert_eq!(doc_name_for("Inc.INC"), "Inc.INC");
    assert_eq!(doc_name_for("r.mac"), "r.mac");
}

#[tokio::test]
async fn a_name_with_no_wildcard_lists_itself_through_the_terminal_as_a_document() {
    let (iris, _) = closed_port();
    let _env = EnvGuard::without("IRIS_CONTAINER");
    let got = expand_exec(&iris, None, "USER", "My.Class").await;
    assert_eq!(got.ok(), Some(vec!["My.Class.cls".to_string()]));
}

#[tokio::test]
async fn a_wildcard_listed_with_no_container_is_a_transport_failure() {
    let (iris, _) = closed_port();
    let _env = EnvGuard::without("IRIS_CONTAINER");
    for ssh in [None, Some("host")] {
        match expand_exec(&iris, ssh, "USER", "My.*").await {
            Err(ListFailure::Transport(msg)) => assert!(msg.contains("DOCKER_REQUIRED"), "{msg}"),
            other => panic!("expected a transport failure, got {:?}", other.is_ok()),
        }
    }
}

#[tokio::test]
async fn a_document_read_with_no_container_is_a_transport_failure() {
    let (iris, _) = closed_port();
    let _env = EnvGuard::without("IRIS_CONTAINER");
    for ssh in [None, Some("host")] {
        match read_exec(&iris, ssh, "USER", "My.Class.cls").await {
            Stored::Transport(msg) => assert!(msg.contains("DOCKER_REQUIRED"), "{msg}"),
            other => panic!("expected a transport failure, got {other:?}"),
        }
    }
}

/// The exec check reports the channel failing as the channel failing, so the caller can word it
/// exactly as the compile would have worded it; the same for a wildcard that cannot be listed.
#[tokio::test]
async fn the_exec_check_reports_a_dead_channel_as_transport() {
    let (iris, _) = closed_port();
    let _env = EnvGuard::without("IRIS_CONTAINER");
    for target in ["My.Class", "My.Class, Other.*"] {
        match check_exec_target(&iris, None, "USER", target).await {
            Outcome::Transport(msg) => assert!(msg.contains("DOCKER_REQUIRED"), "{target}: {msg}"),
            other => panic!("{target}: expected transport, got {other:?}"),
        }
    }
}
