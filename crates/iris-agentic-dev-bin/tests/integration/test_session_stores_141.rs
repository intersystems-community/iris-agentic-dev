//! Spec 141 T220: in HTTP transport mode a session's stored results belong to that session.
//!
//! Two clients open sessions on one server. The first asks `iris_info` for the document list with
//! the inline threshold at 1, so the answer goes into the log store and comes back as a `log_id`.
//! The second lists the log store with `iris_get_log` and asks for the first's id. Both used to
//! succeed, because every session was a clone of one `IrisTools` and a clone shares the store.
//!
//! Live: it needs `iris-dev-iris` for the document list. Run it behind the shared lock:
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test bin_integration
//! test_session_stores_141 -- --ignored --test-threads=1`.

use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use iris_agentic_dev_core::testing::{clean_mcp_command, stop_server};
use serde_json::{json, Value};

const TOKEN: &str = "0123456789abcdef0123456789abcdef-sessions";

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        stop_server(&mut self.child);
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start(home: &Path) -> Server {
    let port = free_port();
    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut cmd = clean_mcp_command(Path::new(bin));
    let child = cmd
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("IRIS_HOST", "localhost")
        .env("IRIS_WEB_PORT", "52780")
        .env("IRIS_USERNAME", "_SYSTEM")
        .env("IRIS_PASSWORD", "SYS")
        .env("IAD_HTTP_TOKEN", TOKEN)
        // One document is enough to overflow the inline limit, so the answer goes to the store.
        .env("IRIS_INLINE_INFO", "1")
        .current_dir(home)
        .args([
            "--transport",
            "http",
            "--bind",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the server");
    let mut server = Server { child, port };
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(status) = server.child.try_wait().unwrap() {
            panic!("the server exited early with {status}");
        }
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return server;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the server did not listen on {port} within 15 s");
}

/// One MCP session over streamable HTTP.
struct Session {
    http: reqwest::Client,
    url: String,
    id: String,
    next: u64,
}

/// The JSON-RPC message with `id` out of a response body that is JSON or a server-sent event stream.
fn message_with_id(body: &str, id: u64) -> Value {
    let candidates = std::iter::once(body.trim().to_string()).chain(
        body.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|d| d.trim().to_string()),
    );
    for c in candidates {
        if let Ok(v) = serde_json::from_str::<Value>(&c) {
            if v["id"] == json!(id) {
                return v;
            }
        }
    }
    panic!("no message with id {id} in {body}");
}

impl Session {
    async fn open(port: u16, name: &str) -> Session {
        let http = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/mcp");
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-03-26", "capabilities": {},
            "clientInfo": {"name": name, "version": "0"}}});
        let resp = http
            .post(&url)
            .bearer_auth(TOKEN)
            .header("Accept", "application/json, text/event-stream")
            .json(&init)
            .send()
            .await
            .expect("initialize");
        assert_eq!(resp.status(), 200, "initialize");
        let id = resp
            .headers()
            .get("mcp-session-id")
            .expect("the server names the session")
            .to_str()
            .unwrap()
            .to_string();
        let body = resp.text().await.unwrap();
        message_with_id(&body, 1);
        let s = Session {
            http,
            url,
            id,
            next: 2,
        };
        let r = s
            .http
            .post(&s.url)
            .bearer_auth(TOKEN)
            .header("Accept", "application/json, text/event-stream")
            .header("Mcp-Session-Id", &s.id)
            .json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "initialized: {}", r.status());
        s
    }

    /// `tools/call`, answered with the tool's own JSON body.
    async fn call(&mut self, tool: &str, args: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let resp = self
            .http
            .post(&self.url)
            .bearer_auth(TOKEN)
            .header("Accept", "application/json, text/event-stream")
            .header("Mcp-Session-Id", &self.id)
            .json(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": tool, "arguments": args}}))
            .send()
            .await
            .expect("tools/call");
        assert_eq!(resp.status(), 200, "tools/call {tool}");
        let msg = message_with_id(&resp.text().await.unwrap(), id);
        let result = &msg["result"];
        if let Some(sc) = result.get("structuredContent").filter(|v| !v.is_null()) {
            return sc.clone();
        }
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or("{}"))
            .unwrap_or_else(|_| result.clone())
    }
}

#[tokio::test]
#[ignore = "live: iris-dev-iris on 52780; run behind /tmp/iad-141-iris.lock"]
async fn a_second_http_session_cannot_list_or_fetch_the_first_sessions_log() {
    let home = tempfile::tempdir().unwrap();
    let server = start(home.path());
    let mut a = Session::open(server.port, "client-a").await;
    let mut b = Session::open(server.port, "client-b").await;
    assert_ne!(a.id, b.id, "two sessions");

    let stored = a
        .call(
            "iris_info",
            json!({"what": "documents", "namespace": "USER"}),
        )
        .await;
    let log_id = stored["log_id"]
        .as_str()
        .unwrap_or_else(|| panic!("the document list was not stored: {stored}"))
        .to_string();

    // The owner reads its own result, listed and by id.
    let own = a.call("iris_get_log", json!({})).await;
    let own_ids: Vec<&str> = own["logs"]
        .as_array()
        .unwrap_or_else(|| panic!("no logs array: {own}"))
        .iter()
        .filter_map(|l| l["id"].as_str())
        .collect();
    assert!(own_ids.contains(&log_id.as_str()), "{own}");
    let own_fetch = a.call("iris_get_log", json!({"id": log_id})).await;
    assert_eq!(own_fetch["success"], true, "{own_fetch}");

    // The other session sees an empty store and cannot fetch the id.
    let listed = b.call("iris_get_log", json!({})).await;
    assert_eq!(
        listed["logs"],
        json!([]),
        "session B lists session A's stored results: {listed}"
    );
    let fetched = b.call("iris_get_log", json!({"id": log_id})).await;
    assert_eq!(
        fetched["error_code"], "LOG_NOT_FOUND",
        "session B fetched session A's result: {fetched}"
    );
}
