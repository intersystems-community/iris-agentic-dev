//! Spec 172: a failed read of a class says what failed. `IRIS_UNREACHABLE` is kept for a request
//! that never reached IRIS; an IRIS that answered with an error status gets the same code
//! `iris_doc` gives that status.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::storage_map::write::{get_doc, read_failure, DocReadStatus};

#[test]
fn an_error_status_is_named_by_its_status() {
    for (status, code) in [
        (401, "AUTH_ERROR"),
        (403, "AUTH_ERROR"),
        (500, "SERVER_ERROR"),
        (400, "BAD_REQUEST"),
        (423, "LOCKED"),
        (418, "HTTP_ERROR"),
    ] {
        let e = anyhow::Error::new(DocReadStatus {
            doc: "A.cls".into(),
            status: reqwest::StatusCode::from_u16(status).unwrap(),
        });
        assert_eq!(read_failure(&e), code, "{status}");
        assert!(e.to_string().contains(&status.to_string()), "{e}");
    }
}

#[test]
fn an_answer_without_content_is_an_http_error() {
    assert_eq!(read_failure(&anyhow::anyhow!("no content")), "HTTP_ERROR");
}

#[tokio::test]
async fn a_request_that_never_reached_iris_is_unreachable() {
    // Port 9 (discard) has no listener: the request fails before any answer.
    let iris = IrisConnection::new(
        "http://127.0.0.1:9",
        "USER",
        "nobody",
        "x",
        DiscoverySource::ExplicitFlag,
    );
    let e = get_doc(&iris, &reqwest::Client::new(), "A.cls", "USER")
        .await
        .expect_err("nothing listens on port 9");
    assert_eq!(read_failure(&e), "IRIS_UNREACHABLE", "{e:?}");
}
