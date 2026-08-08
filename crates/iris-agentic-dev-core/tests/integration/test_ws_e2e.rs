//! WebSocket terminal session E2E integration tests (072-b).
//!
//! All tests are `#[ignore]` — they require a live IRIS container with Atelier API v7+
//! (IRIS 2023.2 or newer).
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --test test_ws_e2e -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::iris::connection::{AtelierVersion, DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::ws_session::WsSessionPool;
use std::sync::Arc;

/// Build a connection and probe the real Atelier version. Returns None if IRIS_HOST unset
/// or the probe fails. The caller should skip the test if the returned version < V7.
async fn iris_conn() -> Option<IrisConnection> {
    let host = std::env::var("IRIS_HOST").ok().filter(|s| !s.is_empty())?;
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .unwrap_or_else(|_| "52773".to_string())
        .parse()
        .unwrap_or(52773);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());

    let mut conn = IrisConnection::new(
        format!("http://{}:{}", host, port),
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );

    // Probe the real Atelier version rather than assuming V7.
    let client = reqwest::Client::new();
    if let Ok(resp) = client
        .get(format!("{}/api/atelier/", conn.base_url))
        .basic_auth(&conn.username, Some(&conn.password))
        .send()
        .await
    {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            let ver = body["result"]["content"]["api"].as_u64().unwrap_or(0);
            conn.atelier_version = match ver {
                v if v >= 8 => AtelierVersion::V8,
                v if v >= 7 => AtelierVersion::V7,
                v if v >= 2 => AtelierVersion::V2,
                _ => AtelierVersion::V1,
            };
        }
    }

    Some(conn)
}

// T047: open a WS session and immediately close it. Verifies token format.
#[tokio::test]
#[ignore]
async fn e2e_ws_open_close() {
    let conn = match iris_conn().await {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_ws_open_close");
            return;
        }
    };

    if !conn.atelier_version.supports_ws_terminal() {
        eprintln!(
            "Atelier version {:?} does not support WS terminal — skipping",
            conn.atelier_version
        );
        return;
    }

    let pool = Arc::new(WsSessionPool::new());
    let token = WsSessionPool::open(&pool, &conn, "dev", "USER")
        .await
        .expect("open WS session");

    // Verify token format.
    let parsed = WsSessionPool::parse_token(&token);
    assert!(parsed.is_some(), "token must be parseable: {token}");
    let (server, ns, uuid) = parsed.unwrap();
    assert_eq!(server, "dev");
    assert_eq!(ns, "USER");
    assert!(!uuid.is_empty(), "uuid must not be empty");
    assert!(token.starts_with("ws:dev:USER:"), "token format: {token}");

    // Close the session.
    WsSessionPool::close(&pool, &token)
        .await
        .expect("close WS session");

    eprintln!("e2e_ws_open_close passed (token={token})");
}

// T048: persistent state test — Set x in one exec, Write x in another.
#[tokio::test]
#[ignore]
async fn e2e_ws_exec_persistent() {
    let conn = match iris_conn().await {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_ws_exec_persistent");
            return;
        }
    };

    if !conn.atelier_version.supports_ws_terminal() {
        eprintln!("Atelier version does not support WS terminal — skipping");
        return;
    }

    let pool = Arc::new(WsSessionPool::new());
    let token = WsSessionPool::open(&pool, &conn, "dev", "USER")
        .await
        .expect("open WS session");

    // First exec: set a variable.
    let _out1 = WsSessionPool::exec(&pool, &token, "Set x = 42")
        .await
        .expect("exec Set x = 42");

    // Second exec: read it back — state must persist.
    let out2 = WsSessionPool::exec(&pool, &token, "Write x")
        .await
        .expect("exec Write x");

    assert!(
        out2.contains("42"),
        "expected '42' in output, got: {out2:?}"
    );

    WsSessionPool::close(&pool, &token)
        .await
        .expect("close WS session");

    eprintln!("e2e_ws_exec_persistent passed (x=42 persisted across exec calls)");
}

// T049: stale token — reference a server name not in the pool.
#[tokio::test]
#[ignore]
async fn e2e_ws_stale_token() {
    let pool = Arc::new(WsSessionPool::new());
    // Token with server_name "nonexistent-server" — not opened, so SESSION_STALE expected.
    let fake_token = WsSessionPool::make_token("nonexistent-server", "USER", "fake-uuid-0000");

    let result = WsSessionPool::exec(&pool, &fake_token, "Write 1").await;
    assert!(result.is_err(), "expected SESSION_STALE error, got Ok");
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.contains("SESSION_STALE"),
        "error must contain SESSION_STALE: {err_msg}"
    );

    eprintln!("e2e_ws_stale_token passed");
}
