//! S7: one CSP cookie store per (endpoint, user), for the primary identity too.
//!
//! IRIS reads `CSPSESSIONID` before `Authorization`, so two accounts on one host that shared a jar
//! would run in whichever session logged in first. These tests cover the store and the client
//! wiring with a local listener that only plays the web server's cookie part; what IRIS does with
//! the cookie is the live test's job (`test_primary_session_171_live`).

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::csp_session::{
    all_logout_urls, shared_cookie_store, store_for_identity, store_for_primary,
};
use iris_agentic_dev_core::iris::http::{call_client_ref, ClientKind, TlsSetting};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;

#[test]
fn one_store_per_endpoint_and_user() {
    let a = store_for_primary("http://h171.example:52773", "alice");
    assert!(Arc::ptr_eq(
        &a,
        &store_for_primary("http://h171.example:52773/", "ALICE")
    ));
    assert!(!Arc::ptr_eq(
        &a,
        &store_for_primary("http://h171.example:52773", "bob")
    ));
    assert!(!Arc::ptr_eq(
        &a,
        &store_for_primary("http://h171.example:52774", "alice")
    ));
    assert!(!Arc::ptr_eq(&a, &shared_cookie_store()));
    assert!(!Arc::ptr_eq(
        &a,
        &store_for_identity("http://h171.example:52773|alice")
    ));
}

/// A listener that answers every request with a `CSPSESSIONID` cookie named for the user in
/// the request's `Authorization` header, and records each request's `Cookie` header.
/// The user a request named and the `Cookie` header it carried, in arrival order.
type Seen = Arc<std::sync::Mutex<Vec<(String, Option<String>)>>>;

fn cookie_server() -> (u16, Seen) {
    use base64::Engine;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let (mut user, mut cookie) = (String::new(), None);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if lower.starts_with("cookie:") {
                    cookie = Some(line["cookie:".len()..].trim().to_string());
                }
                if lower.starts_with("authorization: basic ") {
                    let b64 = line.trim()["authorization: basic ".len()..].to_string();
                    let raw = base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .unwrap_or_default();
                    user = String::from_utf8_lossy(&raw)
                        .split(':')
                        .next()
                        .unwrap_or_default()
                        .to_string();
                }
            }
            log.lock().unwrap().push((user.clone(), cookie));
            let mut stream = stream;
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nSet-Cookie: CSPSESSIONID-SP-0-UP-api-atelier-=SESSION{user}; path=/api/atelier/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            );
        }
    });
    (port, seen)
}

fn conn(port: u16, user: &str) -> IrisConnection {
    IrisConnection::new(
        format!("http://127.0.0.1:{port}"),
        "USER",
        user,
        "pw",
        DiscoverySource::ExplicitFlag,
    )
}

async fn get_as(client: &reqwest::Client, c: &IrisConnection) {
    client
        .get(c.atelier_url("/"))
        .basic_auth(&c.username, Some(&c.password))
        .send()
        .await
        .unwrap();
}

#[tokio::test]
async fn two_accounts_on_one_host_never_send_each_others_cookie() {
    let (port, seen) = cookie_server();
    let (a, b) = (conn(port, "acct171a"), conn(port, "acct171b"));
    let (ca, cb) = (
        a.client(ClientKind::Call).unwrap(),
        b.client(ClientKind::Call).unwrap(),
    );

    get_as(&ca, &a).await;
    get_as(&cb, &b).await;
    get_as(&ca, &a).await;
    get_as(&cb, &b).await;

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[0].1, None, "first request has no cookie");
    assert_eq!(
        seen[1].1, None,
        "b's first request must not carry a's session: {seen:?}"
    );
    assert!(
        seen[2]
            .1
            .as_deref()
            .is_some_and(|c| c.contains("SESSIONacct171a") && !c.contains("171b")),
        "{seen:?}"
    );
    assert!(
        seen[3]
            .1
            .as_deref()
            .is_some_and(|c| c.contains("SESSIONacct171b") && !c.contains("171a")),
        "{seen:?}"
    );
    // Logout on exit still reaches both sessions.
    let urls = all_logout_urls();
    assert!(
        urls.iter()
            .any(|u| u.starts_with(&format!("http://127.0.0.1:{port}/api/atelier/"))),
        "{urls:?}"
    );
}

#[tokio::test]
async fn the_per_call_client_of_a_connection_keeps_the_same_separation() {
    let (port, seen) = cookie_server();
    let (a, b) = (conn(port, "acct171c"), conn(port, "acct171d"));
    let tls = TlsSetting::default();
    let ca = call_client_ref(&tls, &a.base_url, &a.username).unwrap();
    let cb = call_client_ref(&tls, &b.base_url, &b.username).unwrap();
    get_as(ca, &a).await;
    get_as(cb, &b).await;
    get_as(ca, &a).await;
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen[1].1, None, "{seen:?}");
    assert!(
        seen[2]
            .1
            .as_deref()
            .is_some_and(|c| c.contains("SESSIONacct171c")),
        "{seen:?}"
    );
}
