//! Spec 141 C6, binary layer: `iris_test_server` with an ad-hoc `host` over stdio.
//!
//! No IRIS is involved. Each test binds a loopback listener and checks whether anything connects:
//! a `host` with URL syntax is refused before any socket opens, and a `302` from the probed port is
//! not followed to a second listener.
//!
//! Run: `IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test --features testing --test binary
//! test_server_probe_141 -- --ignored --test-threads=1`

use iris_agentic_dev_core::testing::McpSession;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

/// Count the connections `listener` accepts within `window`, answering each with `reply`.
fn accepts_within(listener: TcpListener, window: Duration, reply: String) -> mpsc::Receiver<usize> {
    let (tx, rx) = mpsc::channel();
    listener.set_nonblocking(true).expect("nonblocking");
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + window;
        let mut count = 0;
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut sock, _)) => {
                    count += 1;
                    let mut buf = [0u8; 1024];
                    let _ = sock.set_read_timeout(Some(Duration::from_millis(500)));
                    let _ = sock.read(&mut buf);
                    let _ = sock.write_all(reply.as_bytes());
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        let _ = tx.send(count);
    });
    rx
}

fn text_body(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content: {answer}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

#[test]
#[ignore]
fn bad_host_never_connects() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let accepted = accepts_within(
        listener,
        Duration::from_secs(2),
        "HTTP/1.1 200 OK\r\n\r\n".to_string(),
    );

    let mut s = McpSession::start(&[]);
    let answer = s.call(
        "iris_test_server",
        &serde_json::json!({"host": "127.0.0.1/x#", "web_port": port}),
    );
    let body = text_body(&answer);
    assert_eq!(body["error_code"], "INVALID_PARAMS", "{answer}");
    assert_eq!(
        accepted.recv().unwrap(),
        0,
        "the probe connected to the listener"
    );
}

#[test]
#[ignore]
fn redirect_is_not_followed() {
    let elsewhere = TcpListener::bind("127.0.0.1:0").expect("bind");
    let elsewhere_port = elsewhere.local_addr().expect("addr").port();
    let followed = accepts_within(
        elsewhere,
        Duration::from_secs(3),
        "HTTP/1.1 200 OK\r\n\r\n".to_string(),
    );

    let redirect = format!(
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{elsewhere_port}/api/atelier/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let front = TcpListener::bind("127.0.0.1:0").expect("bind");
    let front_port = front.local_addr().expect("addr").port();
    let first = accepts_within(front, Duration::from_secs(3), redirect);

    let mut s = McpSession::start(&[]);
    let answer = s.call(
        "iris_test_server",
        &serde_json::json!({"host": "127.0.0.1", "web_port": front_port}),
    );
    let body = text_body(&answer);
    assert_eq!(body["reachable"], true, "{answer}");
    assert!(
        body["error"].as_str().unwrap_or("").contains("302"),
        "the 302 is reported, not followed: {answer}"
    );
    assert!(
        first.recv().unwrap() >= 1,
        "the probe never reached the front listener"
    );
    assert_eq!(
        followed.recv().unwrap(),
        0,
        "the 302 was followed to another origin"
    );
}
