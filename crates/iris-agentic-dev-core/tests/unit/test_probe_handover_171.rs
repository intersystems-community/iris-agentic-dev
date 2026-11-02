//! A probe hands its CSP session to the account it signed in as only when the sign-in worked and
//! the account has none, as one step (1.7.1).
//!
//! The probe used to hand over before it knew the sign-in worked, so a session IRIS opened for a
//! refused credential could become the account's. And the "has none" check and the handover were two
//! steps: two probes at once could both find the account empty and both hand over.
//!
//! Sessions are put in a store through its cookie interface, as a response would; nothing here
//! listens or connects except a logout to `127.0.0.1:1`, which fails at once.

use iris_agentic_dev_core::iris::csp_session::CspSessionStore;
use iris_agentic_dev_core::iris::http::{probe_session, TlsSetting, TlsSource};
use reqwest::cookie::CookieStore;
use reqwest::header::HeaderValue;
use reqwest::Url;
use std::sync::Arc;

/// Put a CSP session for `path` on `127.0.0.1:1` in `store`.
fn open_session(store: &CspSessionStore, path: &str, id: &str) {
    let url = Url::parse(&format!("http://127.0.0.1:1{path}")).unwrap();
    let header = HeaderValue::from_str(&format!(
        "CSPSESSIONID-SP-1-UP{}={id}; path={path}; httpOnly",
        path.replace('/', "-")
    ))
    .unwrap();
    store.set_cookies(&mut [&header].into_iter(), &url);
    assert!(!store.is_empty(), "the session was not recorded");
}

#[test]
fn an_empty_store_adopts() {
    let probe = CspSessionStore::new();
    open_session(&probe, "/api/atelier/", "p1");
    let account = CspSessionStore::new();
    assert!(account.adopt_if_empty(&probe));
    assert_eq!(account.logout_urls(), probe.logout_urls());
    let sent = account
        .cookies(&Url::parse("http://127.0.0.1:1/api/atelier/v8/USER/").unwrap())
        .expect("the account sends the adopted cookie");
    assert!(sent.to_str().unwrap().contains("p1"), "{sent:?}");
}

#[test]
fn a_store_with_a_session_keeps_it() {
    let probe = CspSessionStore::new();
    open_session(&probe, "/api/atelier/", "p1");
    let account = CspSessionStore::new();
    open_session(&account, "/api/atelier/", "mine");
    let before = account.logout_urls();
    assert!(!account.adopt_if_empty(&probe));
    assert_eq!(account.logout_urls(), before);
    let sent = account
        .cookies(&Url::parse("http://127.0.0.1:1/api/atelier/").unwrap())
        .unwrap();
    assert!(sent.to_str().unwrap().contains("mine"), "{sent:?}");
    assert!(!sent.to_str().unwrap().contains("p1"), "{sent:?}");
}

#[test]
fn two_probes_at_once_hand_over_one_session() {
    for round in 0..50 {
        let account = Arc::new(CspSessionStore::new());
        let probes: Vec<Arc<CspSessionStore>> = (0..8)
            .map(|i| {
                let p = Arc::new(CspSessionStore::new());
                open_session(&p, &format!("/app{i}/"), &format!("r{round}p{i}"));
                p
            })
            .collect();
        let barrier = Arc::new(std::sync::Barrier::new(probes.len()));
        let handles: Vec<_> = probes
            .into_iter()
            .map(|p| {
                let account = account.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    account.adopt_if_empty(&p)
                })
            })
            .collect();
        let adopted = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|a| *a)
            .count();
        assert_eq!(adopted, 1, "round {round}: {adopted} probes handed over");
        assert_eq!(account.logout_urls().len(), 1, "round {round}");
    }
}

#[tokio::test]
async fn a_probe_that_did_not_sign_in_hands_over_nothing() {
    let tls = TlsSetting::validating(TlsSource::Default);
    let session = probe_session(&tls).unwrap();
    open_session(session.store(), "/api/atelier/", "refused");
    let account = Arc::new(CspSessionStore::new());
    session.finish(&tls, &account, false).await;
    assert!(account.is_empty(), "{:?}", account.logout_urls());
}

#[tokio::test]
async fn a_probe_that_signed_in_hands_over_to_an_empty_account() {
    let tls = TlsSetting::validating(TlsSource::Default);
    let session = probe_session(&tls).unwrap();
    open_session(session.store(), "/api/atelier/", "ok");
    let account = Arc::new(CspSessionStore::new());
    session.finish(&tls, &account, true).await;
    assert_eq!(account.logout_urls().len(), 1);
}
