// probe_server failure branches, driven by real sockets.
//
// probe_server has five outcomes: client-build failure, timeout, transport error, HTTP 401,
// HTTP non-success, and a parsed Atelier root. The live tests against iris-dev-iris cover the
// success and 401 paths. The two failure paths below need a server that misbehaves, which no
// IRIS container will do on request, so these bind a TCP listener and misbehave directly.
//
// This is not a mocked IRIS. Nothing here stands in for IRIS or asserts anything about IRIS
// behaviour — the listener exists so the socket does what the branch under test needs
// (stall, or answer 500), and the assertions are about our own ProbeResult.

use iris_agentic_dev_core::tools::server_tools::probe_server;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

/// Bind an ephemeral port on loopback and hand back the port plus the listener.
async fn bind_loopback() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let port = listener.local_addr().expect("local_addr").port();
    (listener, port)
}

#[tokio::test]
async fn probe_reports_transport_error_when_nothing_is_listening() {
    // Bind then drop, so the port is closed and almost certainly unused.
    let (listener, port) = bind_loopback().await;
    drop(listener);

    let r = probe_server("127.0.0.1", port, "USER", "_SYSTEM", "SYS").await;

    assert!(!r.reachable, "a closed port is not reachable");
    assert!(!r.auth);
    assert!(r.error.is_some(), "the transport error must be reported");
    assert!(
        r.latency_ms.is_some(),
        "latency is measured even on failure — it is how long the caller waited"
    );
    assert!(r.iris_version.is_none());
}

#[tokio::test]
async fn probe_reports_http_status_when_the_server_answers_with_an_error() {
    let (listener, port) = bind_loopback().await;

    let server = tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let _ = sock
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n")
                .await;
            let _ = sock.flush().await;
        }
    });

    let r = probe_server("127.0.0.1", port, "USER", "_SYSTEM", "SYS").await;
    let _ = server.await;

    assert!(
        r.reachable,
        "the server answered, so it is reachable — the status is a separate fact"
    );
    assert!(!r.auth, "a 503 is not an authenticated session");
    assert_eq!(
        r.namespace.as_deref(),
        Some("USER"),
        "the requested namespace is echoed back so the caller knows what was probed"
    );
    let err = r.error.expect("a non-success status must be reported");
    assert!(
        err.contains("503"),
        "the error must name the status code, got: {err}"
    );
}

#[tokio::test]
async fn probe_reports_auth_failure_on_401() {
    let (listener, port) = bind_loopback().await;

    let server = tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let _ = sock
                .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                .await;
            let _ = sock.flush().await;
        }
    });

    let r = probe_server("127.0.0.1", port, "USER", "_SYSTEM", "wrong").await;
    let _ = server.await;

    assert!(r.reachable, "a 401 means the server is there");
    assert!(!r.auth, "a 401 means the credentials are not");
    let err = r.error.expect("401 must be reported");
    assert!(
        err.contains("401"),
        "the error must name the status code, got: {err}"
    );
}

/// A server that accepts and then says nothing is worse than one that refuses: the caller waits.
/// `probe_server` caps that wait at 5 seconds with its own `tokio::time::timeout`, inside
/// reqwest's 10-second request timeout, so the outer bound is what fires here.
///
/// This test costs 5 seconds of wall clock. Faking the clock would mean pausing tokio's timer,
/// which reqwest also uses, and then the two timeouts race on whichever the runtime advances
/// first — a test that passes for the wrong reason half the time.
#[tokio::test]
async fn probe_reports_a_timeout_when_the_server_accepts_and_never_answers() {
    let (listener, port) = bind_loopback().await;

    let server = tokio::spawn(async move {
        // Hold the connection open and write nothing until the probe gives up.
        if let Ok((sock, _)) = listener.accept().await {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            drop(sock);
        }
    });

    let started = std::time::Instant::now();
    let r = probe_server("127.0.0.1", port, "USER", "_SYSTEM", "SYS").await;
    let elapsed = started.elapsed();
    server.abort();

    assert!(!r.reachable, "a server that never answers is not reachable");
    assert!(!r.auth);
    assert!(r.iris_version.is_none());
    assert!(
        r.namespace.is_none(),
        "nothing was probed, so nothing to echo"
    );
    let err = r.error.expect("the timeout must be reported");
    assert!(
        err.contains("timed out after 5 seconds"),
        "the message must say how long the caller waited, got: {err}"
    );
    assert!(
        elapsed >= std::time::Duration::from_secs(5),
        "the probe returned in {elapsed:?} — it cannot have waited out the 5-second bound"
    );
    assert!(
        r.latency_ms.unwrap_or(0) >= 5000,
        "reported latency must be the real wait, got {:?}",
        r.latency_ms
    );
}
