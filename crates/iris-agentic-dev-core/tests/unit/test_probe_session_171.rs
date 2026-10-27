//! N2 (security fix): a probe that tests a password must not ride the account's CSP session.
//!
//! IRIS reads `CSPSESSIONID` before `Authorization`, so a probe that joined the store of the account
//! it signs in as was answered from the session already open for that account, whatever password it
//! sent, and reported `auth: true` for a wrong one. A probe now has a jar of its own, and it ends
//! the session it opened. The listener below answers 200 only to a request that carries a cookie.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::csp_session::store_for_primary;
use iris_agentic_dev_core::iris::http::TlsSetting;
use iris_agentic_dev_core::tools::server_tools::probe_base_url;
use reqwest::cookie::CookieStore;
use reqwest::header::HeaderValue;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

const COOKIE: &str = "CSPSESSIONID-SP-0-UP-api-atelier-=abc; path=/api/atelier/";
const OK_BODY: &str = r#"{"result":{"content":{"version":"IRIS for UNIX 2026.2","api":8}}}"#;

/// A listener standing in for the web server: a logout gets 401, a request with a cookie gets 200
/// and the Atelier root body, one without gets 401. With `sets_cookie`, the 401 for a request
/// without a cookie also opens a session, which a later probe request could carry.
fn listener(opens_session: bool) -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let log = heads.clone();
    std::thread::spawn(move || {
        for stream in l.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            log.lock().unwrap().push(head.clone());
            let has_cookie = head.to_ascii_lowercase().contains("\r\ncookie:");
            let reply = if head.contains("IRISLogout=end") {
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            } else if has_cookie || opens_session {
                format!(
                    "HTTP/1.1 200 OK\r\nSet-Cookie: {COOKIE}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{OK_BODY}",
                    OK_BODY.len()
                )
            } else {
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            };
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (port, heads)
}

/// Open a session for `user` on `base_url`, as an earlier tool call would have.
fn seed_session(base_url: &str, user: &str) {
    let url = reqwest::Url::parse(&format!("{base_url}/api/atelier/")).unwrap();
    let header = HeaderValue::from_static(COOKIE);
    store_for_primary(base_url, user).set_cookies(&mut std::iter::once(&header), &url);
}

#[tokio::test]
async fn a_credential_probe_is_not_answered_from_the_accounts_session() {
    let (port, heads) = listener(false);
    let base = format!("http://127.0.0.1:{port}");
    seed_session(&base, "probeuser");
    let r = probe_base_url(
        &base,
        &TlsSetting::default(),
        "USER",
        "probeuser",
        "the-wrong-password",
    )
    .await;
    assert!(r.reachable, "{r:?}");
    assert!(!r.auth, "the probe rode an existing session: {r:?}");
    let heads = heads.lock().unwrap();
    assert!(
        heads
            .iter()
            .all(|h| !h.to_ascii_lowercase().contains("\r\ncookie:")),
        "the probe sent a session cookie: {heads:?}"
    );
}

#[tokio::test]
async fn a_probe_ends_the_session_it_opened() {
    let (port, heads) = listener(true);
    let base = format!("http://127.0.0.1:{port}");
    let r = probe_base_url(&base, &TlsSetting::default(), "USER", "enduser", "pw").await;
    assert!(r.auth, "{r:?}");
    let heads = heads.lock().unwrap();
    assert!(
        heads.iter().any(|h| h.contains("IRISLogout=end")),
        "the session the probe opened was left open: {heads:?}"
    );
}

#[tokio::test]
async fn the_connection_probe_is_not_answered_from_the_accounts_session() {
    let (port, _) = listener(false);
    let base = format!("http://127.0.0.1:{port}");
    seed_session(&base, "connuser");
    let mut conn = IrisConnection::new(
        &base,
        "USER",
        "connuser",
        "wrong-pw",
        DiscoverySource::EnvVar,
    );
    conn.probe().await;
    assert!(
        conn.version.is_none(),
        "the probe was answered from an existing session: {:?}",
        conn.version
    );
}

/// The connection's own startup probe opens the account's first session, and the calls that follow
/// use it. Without that, the first calls of a process, which run together, each open a session
/// before any cookie is stored, and all but one are never ended (132 B1).
#[tokio::test]
async fn the_connection_probe_hands_its_session_to_an_account_that_has_none() {
    let (port, heads) = listener(true);
    let base = format!("http://127.0.0.1:{port}");
    let mut conn = IrisConnection::new(&base, "USER", "handuser", "pw", DiscoverySource::EnvVar);
    conn.probe().await;
    assert!(conn.version.is_some(), "the probe was not answered");
    let urls = store_for_primary(&base, "handuser").logout_urls();
    assert_eq!(
        urls.len(),
        1,
        "the account's store did not take the probe's session: {urls:?}"
    );
    let heads = heads.lock().unwrap();
    assert!(
        heads.iter().all(|h| !h.contains("IRISLogout=end")),
        "the probe ended a session it handed over: {heads:?}"
    );
}

/// An account that already has a session keeps it, and the probe's own is ended.
#[tokio::test]
async fn the_connection_probe_does_not_replace_the_accounts_session() {
    let (port, heads) = listener(true);
    let base = format!("http://127.0.0.1:{port}");
    seed_session(&base, "keepuser");
    let before = store_for_primary(&base, "keepuser").logout_urls();
    let mut conn = IrisConnection::new(&base, "USER", "keepuser", "pw", DiscoverySource::EnvVar);
    conn.probe().await;
    assert_eq!(store_for_primary(&base, "keepuser").logout_urls(), before);
    let heads = heads.lock().unwrap();
    // The probe's own session was ended, and the request that ended it carried the probe's cookie
    // (the seeded one has the same name, so only the count of logouts is checked).
    assert_eq!(
        heads
            .iter()
            .filter(|h| h.contains("IRISLogout=end"))
            .count(),
        1,
        "{heads:?}"
    );
}
