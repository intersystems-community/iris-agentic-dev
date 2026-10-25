//! S10: no client iad builds for an IRIS server follows a redirect.
//!
//! A redirect from an IRIS-like server to another host or port is followed by `reqwest` by default,
//! up to ten hops, and the `Authorization` header goes with it when the target is on the same host
//! (and the cookie jar sends cookies to any port on the same domain). The listener below stands in
//! for the web server and answers 302 to a second listener on another port; the second one must
//! read nothing.

use iris_agentic_dev_core::iris::http::{client_for, ClientKind, TlsSetting};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// Every kind of client, so that adding a kind to the enum breaks this match until the new kind is
/// listed here and so covered by the test below.
fn all_kinds() -> Vec<ClientKind> {
    let mut v = Vec::new();
    for k in [
        ClientKind::Call,
        ClientKind::Probe,
        ClientKind::Search,
        ClientKind::Scan,
        ClientKind::Batch,
        ClientKind::Raw,
        ClientKind::Logout,
        ClientKind::AdHoc,
        ClientKind::OwnProbe,
    ] {
        match k {
            ClientKind::Call
            | ClientKind::Probe
            | ClientKind::Search
            | ClientKind::Scan
            | ClientKind::Batch
            | ClientKind::Raw
            | ClientKind::Logout
            | ClientKind::AdHoc
            | ClientKind::OwnProbe => v.push(k),
        }
    }
    v
}

/// A listener that answers every request with `reply` and records each request head.
fn listener(reply: String) -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let log = heads.clone();
    std::thread::spawn(move || {
        for stream in l.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).into_owned());
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (port, heads)
}

#[tokio::test]
async fn no_client_follows_a_redirect_with_its_credentials() {
    for kind in all_kinds() {
        let (target_port, target_heads) =
            listener("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        let (origin_port, _) = listener(format!(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target_port}/api/atelier/\r\n\
             Set-Cookie: CSPSESSIONID-SP-0-UP-api-atelier-=abc; path=/\r\n\
             Content-Length: 0\r\nConnection: close\r\n\r\n"
        ));
        let client = client_for(&TlsSetting::default(), kind).unwrap();
        let resp = client
            .get(format!("http://127.0.0.1:{origin_port}/api/atelier/"))
            .basic_auth("u", Some("Zq9-redirect-pw"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status().as_u16(),
            302,
            "{kind:?} followed the redirect instead of returning it"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            target_heads.lock().unwrap().is_empty(),
            "{kind:?} sent a request to the redirect target: {:?}",
            target_heads.lock().unwrap()
        );
    }
}
