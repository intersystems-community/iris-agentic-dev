//! The request watch of test builds (`testing::take_requests`) sees every request an IRIS client
//! sends, whatever its kind, so a live test can say which requests a call made without a relay in
//! front of the server.
//!
//! Nothing listens at `127.0.0.1:1`: each request fails, and the watch has still seen it, because
//! reqwest asks the cookie provider before it connects. No socket is opened by the test.

use iris_agentic_dev_core::iris::http::{
    client_for, ClientKind, IdentifyClient, TlsSetting, TlsSource,
};
use iris_agentic_dev_core::testing::seen_requests;
use std::time::Duration;

const KINDS: &[ClientKind] = &[
    ClientKind::Call,
    ClientKind::Probe,
    ClientKind::Search,
    ClientKind::Scan,
    ClientKind::Batch,
    ClientKind::Raw,
    ClientKind::Logout,
    ClientKind::AdHoc,
    ClientKind::OwnProbe,
];

fn path(tag: &str) -> String {
    format!(
        "/api/atelier/v8/USER/doc/IrisDevTmpWatch{tag}{}.cls",
        std::process::id()
    )
}

fn seen(p: &str) -> bool {
    seen_requests()
        .iter()
        .any(|l| l == &format!("127.0.0.1:1{p}"))
}

#[tokio::test]
async fn every_client_kind_is_watched() {
    let tls = TlsSetting::validating(TlsSource::Default);
    for kind in KINDS {
        let p = path(&format!("{kind:?}"));
        let client = client_for(&tls, *kind).unwrap();
        let sent = client
            .put(format!("http://127.0.0.1:1{p}"))
            .timeout(Duration::from_secs(2))
            .send()
            .await;
        assert!(sent.is_err(), "{kind:?}: nothing listens at 127.0.0.1:1");
        assert!(seen(&p), "{kind:?}: the watch did not see {p}");
    }
}

#[tokio::test]
async fn the_identify_client_is_watched() {
    let tls = TlsSetting::validating(TlsSource::Default);
    let p = path("Identify");
    let client = IdentifyClient::new(&tls, Duration::from_secs(2)).unwrap();
    assert!(client.get(&format!("http://127.0.0.1:1{p}")).await.is_err());
    assert!(seen(&p), "the watch did not see {p}");
}

#[tokio::test]
async fn the_query_is_kept() {
    let tls = TlsSetting::validating(TlsSource::Default);
    let p = format!(
        "/api/atelier/v8/USER/action/compile?tag=watch{}",
        std::process::id()
    );
    let client = client_for(&tls, ClientKind::Call).unwrap();
    let _ = client
        .post(format!("http://127.0.0.1:1{p}"))
        .timeout(Duration::from_secs(2))
        .send()
        .await;
    assert!(seen(&p), "the watch did not see {p}");
}
