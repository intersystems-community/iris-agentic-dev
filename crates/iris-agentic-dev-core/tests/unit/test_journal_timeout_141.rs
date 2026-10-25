//! Spec 141 T174: a `journal_search` that runs past the HTTP timeout reports a timeout that names the
//! journal file, its size and a narrower `time_range`, and not `IRIS_UNREACHABLE`.
//!
//! The mapping is pure. The timeout itself is a transport fact: a loopback socket that accepts and
//! never answers makes reqwest report a timeout, and a closed port makes it report a refusal.

use iris_agentic_dev_core::iris::connection::{
    code_may_have_run, generator_retryable, is_request_timeout, RequestTimedOut,
};
use iris_agentic_dev_core::tools::observability::journal_search_timeout_body;
use tokio::net::TcpListener;

#[test]
fn timeout_body_names_the_file_its_size_and_a_narrower_time_range() {
    let body = journal_search_timeout_body(
        Some("/usr/irissys/mgr/journal/20261024.002"),
        Some(734_003_200),
    );

    assert_eq!(body["success"], false, "{body}");
    assert_eq!(body["error_code"], "JOURNAL_SEARCH_TIMEOUT", "{body}");
    let msg = body["error"].as_str().expect("error text");
    assert!(
        msg.contains("/usr/irissys/mgr/journal/20261024.002"),
        "the file is not named: {msg}"
    );
    assert!(msg.contains("700 MB"), "the size is not in MB: {msg}");
    assert!(
        msg.contains("time_range"),
        "no narrower time_range hint: {msg}"
    );
    assert_ne!(body["error_code"], "IRIS_UNREACHABLE", "{body}");
}

#[test]
fn timeout_body_still_hints_when_the_file_or_size_is_unknown() {
    let body = journal_search_timeout_body(None, None);

    assert_eq!(body["error_code"], "JOURNAL_SEARCH_TIMEOUT", "{body}");
    let msg = body["error"].as_str().expect("error text");
    assert!(msg.contains("unknown"), "unknown file not said: {msg}");
    assert!(
        msg.contains("time_range"),
        "no narrower time_range hint: {msg}"
    );
}

/// A reqwest timeout, made on a loopback socket that accepts and never answers.
async fn timed_out_error() -> anyhow::Error {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        // Hold every accepted connection open without a reply until the test ends.
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(200))
        .build()
        .expect("client");
    let err = client
        .get(format!("http://{addr}/"))
        .send()
        .await
        .expect_err("a silent socket must time out");
    anyhow::Error::from(err)
}

#[tokio::test]
async fn a_reqwest_timeout_is_recognised() {
    let err = timed_out_error().await;
    assert!(is_request_timeout(&err), "not classed as a timeout: {err}");
}

#[tokio::test]
async fn a_refused_connection_is_not_a_timeout() {
    // Bind, note the port, drop the listener: nothing listens there now.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        l.local_addr().expect("addr").port()
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("client");
    let err = client
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await
        .expect_err("nothing listens on that port");
    let err = anyhow::Error::from(err);
    assert!(
        !is_request_timeout(&err),
        "a refusal was classed as a timeout: {err}"
    );
}

#[test]
fn an_iris_error_is_not_a_timeout() {
    let err = anyhow::anyhow!("compile HTTP 500 Internal Server Error");
    assert!(!is_request_timeout(&err));
}

#[test]
fn a_generator_query_timeout_is_a_timeout_and_is_never_retried() {
    // The query send in the generator wraps its timeout as this type; the code may have run.
    let err = anyhow::Error::from(RequestTimedOut("operation timed out".into()));
    assert!(is_request_timeout(&err));
    assert!(code_may_have_run(&err));
    assert!(
        !generator_retryable(&err),
        "a timed-out run would run twice"
    );
}
