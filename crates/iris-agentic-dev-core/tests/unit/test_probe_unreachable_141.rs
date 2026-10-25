//! Spec 141: what a probe reports when nothing answers. The probe never reaches a server in these
//! cases, so the report is the whole result: unreachable, no version, no latency claim about a
//! server, and an error that names the cause.

use iris_agentic_dev_core::iris::http::TlsSetting;
use iris_agentic_dev_core::tools::server_tools::{probe_base_url, probe_server};

/// A host that is not a bare host is refused before any connection, and the report says which
/// host and why. It does not claim a latency: no request was made.
#[tokio::test]
async fn a_host_with_url_syntax_is_reported_unreachable_without_a_request() {
    let r = probe_server("a/b", 52773, "USER", "_SYSTEM", "SYS").await;
    assert!(!r.reachable);
    assert!(!r.auth);
    assert!(r.iris_version.is_none());
    assert!(r.atelier_version.is_none());
    assert!(r.namespace.is_none());
    assert!(r.latency_ms.is_none(), "no request was made");
    let error = r.error.expect("the refusal is reported");
    assert!(
        error.contains("a/b") && error.contains("hostname or IP address"),
        "{error}"
    );
}

/// Nothing listens on port 1: the connection is refused, the probe says unreachable, and it does
/// claim a latency, because a request was made.
#[tokio::test]
async fn a_closed_port_is_unreachable_with_the_connection_error() {
    let r = probe_server("127.0.0.1", 1, "USER", "_SYSTEM", "SYS").await;
    assert!(!r.reachable);
    assert!(!r.auth);
    assert!(r.latency_ms.is_some());
    assert!(r.error.is_some_and(|e| !e.is_empty()));
}

/// The base-URL probe (what a pool connection uses) reports the same on a closed port, and keeps
/// any path prefix out of the way of the failure: the error is the connection's, not a parse error.
#[tokio::test]
async fn a_closed_base_url_is_unreachable_with_the_connection_error() {
    let r = probe_base_url(
        "http://127.0.0.1:1/prefix/",
        &TlsSetting::default(),
        "USER",
        "_SYSTEM",
        "SYS",
    )
    .await;
    assert!(!r.reachable);
    assert!(!r.auth);
    assert!(r.namespace.is_none());
    assert!(r.latency_ms.is_some());
    assert!(r.error.is_some_and(|e| !e.is_empty()));
}
