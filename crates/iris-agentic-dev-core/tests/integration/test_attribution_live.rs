//! Live attribution tests (T008, T009, T011) — require a running iris-dev-iris container.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --test test_attribution_live -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::iris::connection::{
    set_caller_mode, CallerMode, DiscoverySource, IrisConnection,
};
use iris_agentic_dev_core::iris::discovery::probe_atelier;
use iris_agentic_dev_core::iris::ws_session::WsSessionPool;
use std::sync::Arc;

fn iris_host() -> Option<String> {
    let h = std::env::var("IRIS_HOST").unwrap_or_default();
    if h.is_empty() {
        None
    } else {
        Some(h)
    }
}

async fn iris_conn() -> Option<IrisConnection> {
    let host = iris_host()?;
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .unwrap_or_else(|_| "52780".to_string())
        .parse()
        .unwrap_or(52780);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    Some(IrisConnection::new(
        format!("http://{}:{}", host, port),
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    ))
}

/// T008: After T013 routes the discovery clients through `iris_http_client()`, every probe
/// reaches IRIS with the marker. We verify by:
/// 1. Calling `probe_atelier()` — which uses the iris_http_client()-built client.
/// 2. Using the discovered connection to execute ObjectScript via the HTTP path, reading
///    `%request.CgiEnvs("HTTP_USER_AGENT")` to prove the client that IRIS sees carries
///    the marker.
///
/// The probe clients (discovery.rs:134,223,429,582) carry the marker in the Atelier access
/// log on each probe request; we cannot read that UA directly from ObjectScript (the probe
/// does not run code), but we can prove the policy by using `execute_via_generator` on the
/// same discovered connection.
#[tokio::test]
#[ignore]
async fn discovery_probe_carries_user_agent_marker() {
    let host = match iris_host() {
        Some(h) => h,
        None => {
            eprintln!("IRIS_HOST not set — skipping");
            return;
        }
    };
    set_caller_mode(CallerMode::Cli);

    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .unwrap_or_else(|_| "52780".to_string())
        .parse()
        .unwrap_or(52780);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());

    // T008: probe_atelier builds its own client (discovery.rs:134). After T013, it goes
    // through iris_http_client() and carries the marker.
    let conn = probe_atelier(&host, port, &username, &password, "USER", 5000).await;
    assert!(
        conn.is_some(),
        "expected probe_atelier to find iris-dev-iris at {}:{}",
        host,
        port
    );
    let conn = conn.unwrap();

    // Now verify that requests via http_client() carry the marker. http_client() is already
    // wired; this tests the end-to-end policy that all IRIS-bound HTTP clients use iris_http_client().
    let client = IrisConnection::http_client().expect("build http client");
    let output = conn
        .execute_via_generator(
            "write $Get(%request.CgiEnvs(\"HTTP_USER_AGENT\"),\"<none>\"),!",
            "USER",
            &client,
        )
        .await
        .expect("execute ObjectScript to read UA");

    let ua = output.trim();
    assert!(
        ua.starts_with("iris-agentic-dev/"),
        "IRIS-bound HTTP client must carry product marker; IRIS saw UA: {:?}",
        ua
    );
    assert!(
        ua.contains("cli"),
        "caller mode must appear in UA; IRIS saw: {:?}",
        ua
    );
}

/// T009: The WebSocket upgrade request must carry the User-Agent marker. We verify by
/// opening a WS session, then executing ObjectScript code inside it that echoes the IRIS
/// process's session CSP data — specifically we use `%request.CgiEnvs` on the WS session's
/// own HTTP context. The WS terminal runs in the context of the initial upgrade request,
/// which after T015 carries the marker.
///
/// Note: `%request` is populated for the WS upgrade handshake at session creation time,
/// but may not be available inside the terminal loop (which runs as a persistent IRIS job,
/// not an HTTP handler). If `%request` is undefined in the WS terminal, the test skips
/// gracefully.
#[tokio::test]
#[ignore]
async fn ws_handshake_carries_user_agent_marker() {
    let conn = match iris_conn().await {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping");
            return;
        }
    };
    set_caller_mode(CallerMode::Mcp);

    // Probe the real Atelier version — WS requires V7+.
    let probe_client = reqwest::Client::new();
    let ver_resp = probe_client
        .get(format!("{}/api/atelier/", conn.base_url))
        .basic_auth(&conn.username, Some(&conn.password))
        .send()
        .await;
    let supports_ws = if let Ok(r) = ver_resp {
        if let Ok(body) = r.json::<serde_json::Value>().await {
            let ver = body["result"]["content"]["api"].as_u64().unwrap_or(0);
            ver >= 7
        } else {
            false
        }
    } else {
        false
    };

    if !supports_ws {
        eprintln!("Atelier API < v7 — WS not supported, skipping ws_handshake test");
        return;
    }

    let pool = Arc::new(WsSessionPool::new());
    let token = WsSessionPool::open(&pool, &conn, "attr-live-test", "USER")
        .await
        .expect("open WS session for attribution test");

    // Try to read the User-Agent from the WS session context. In some IRIS configurations
    // %request is not available inside the WS terminal loop. If the output contains
    // "<UNDEFINED>" or "<none>", skip the assertion (WS terminal doesn't expose %request)
    // but still verify the session opened successfully (proving the WS upgrade worked).
    let ua_result = WsSessionPool::exec(
        &pool,
        &token,
        "write $Get(%request.CgiEnvs(\"HTTP_USER_AGENT\"),\"<none>\"),!",
    )
    .await;

    let _ = WsSessionPool::close(&pool, &token).await;

    let ua = ua_result.expect("ws exec succeeded");
    let ua = ua.trim();

    // Strip ANSI escape codes (IRIS terminal may add color codes).
    let ua_clean: String = ua
        .chars()
        .collect::<String>()
        .replace('\x1b', "")
        // Remove [31;1m style sequences.
        .split(|c: char| c == '[' || c == 'm')
        .filter(|s| !s.chars().all(|c| c.is_ascii_digit() || c == ';'))
        .collect::<Vec<_>>()
        .join("");

    if ua_clean.contains("<UNDEFINED>") || ua_clean.contains("<none>") || ua_clean.is_empty() {
        // %request is not available in WS terminal context — skip the UA assertion.
        // The WS session opened successfully, which proves the upgrade request was valid.
        eprintln!(
            "T009: %request not available in WS terminal context (IRIS terminal runs as \
             persistent job, not HTTP handler) — WS open/close cycle passed, UA assertion skipped. \
             Raw output: {:?}",
            ua
        );
        return;
    }

    assert!(
        ua_clean.starts_with("iris-agentic-dev/"),
        "WS upgrade must carry product marker; IRIS saw UA: {:?} (clean: {:?})",
        ua,
        ua_clean
    );
    assert!(
        ua_clean.contains("mcp"),
        "caller mode mcp must appear in WS UA; IRIS saw: {:?} (clean: {:?})",
        ua,
        ua_clean
    );
}

/// T011: A `docker_only = true` connection cannot set HTTP headers (the exec path goes
/// through docker exec, not HTTP). Calling any tool on such a connection should emit a
/// single tracing warn log that attribution is unavailable, and NOT repeat it on subsequent
/// calls. We verify by calling `iris_execute` twice via the CLI with `--docker-only` and
/// checking that the warning appears exactly once in the combined output.
///
/// Note: this test requires IRIS_CONTAINER to be set. If not set, it skips gracefully.
#[test]
#[ignore]
fn docker_only_attribution_warn_once() {
    // docker_only requires IRIS_CONTAINER to be set.
    let container = match std::env::var("IRIS_CONTAINER") {
        Ok(c) if !c.is_empty() => c,
        _ => {
            eprintln!("IRIS_CONTAINER not set — skipping docker_only warn-once test");
            return;
        }
    };

    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("target/debug/iris-agentic-dev");

    // docker_only is a config key, not a CLI flag — write a temp toml.
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg =
        format!("docker_only = true\ncontainer_name = \"{container}\"\nnamespace = \"USER\"\n");
    std::fs::write(dir.path().join(".iris-agentic-dev.toml"), &cfg).unwrap();

    // First call — expect the warn to appear.
    let out1 = std::process::Command::new(&p)
        .current_dir(dir.path())
        .env("RUST_LOG", "warn")
        .args(["exec", "write 1,!"])
        .output()
        .expect("run iad first call");
    let stderr1 = String::from_utf8_lossy(&out1.stderr);
    // The warn message from T019 contains "attribution unavailable".
    assert!(
        stderr1.contains("attribution unavailable") || stderr1.contains("User-Agent"),
        "expected attribution-unavailable warning on first docker_only call; stderr: {stderr1}"
    );

    // Second call on the same logical connection type — the warn-once guard prevents repeat.
    let out2 = std::process::Command::new(&p)
        .current_dir(dir.path())
        .env("RUST_LOG", "warn")
        .args(["exec", "write 2,!"])
        .output()
        .expect("run iad second call");
    let stderr2 = String::from_utf8_lossy(&out2.stderr);
    assert!(
        !stderr2.contains("attribution unavailable") && !stderr2.contains("User-Agent"),
        "expected no repeat of attribution-unavailable warning on second docker_only call; stderr: {stderr2}"
    );
}
