//! Integration tests for iris_add_server / iris_servers / iris_remove_server / iris_test_server
//! (072-multi-instance-pool).
//!
//! Requires a live IRIS instance reachable at IRIS_HOST:IRIS_WEB_PORT.
//! All tests are #[ignore] — run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test -p iris-agentic-dev-core --features testing \
//!     --test test_server_pool_e2e -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::tools::{ConfigWatcher, IrisTools};

/// Extract JSON from the first text content of a CallToolResult.
fn parse_result(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let result = r.expect("call_for_test returned Err");
    let text = result
        .content
        .first()
        .expect("result has no content")
        .as_text()
        .expect("content is not text")
        .text
        .clone();
    serde_json::from_str(&text).expect("response is not valid JSON")
}

// T029 — iris_add_server / iris_servers / iris_remove_server round-trip.
//
// Verifies that:
//   1. iris_add_server persists the server to the iad-native config.
//   2. A fresh pool (new IrisTools) reflects the addition in iris_servers output.
//   3. iris_remove_server removes the entry from the iad-native config.
//   4. A fresh pool no longer lists the removed server.
//
// The test name deliberately matches the server name so stale entries from aborted
// runs are easy to identify.
#[tokio::test]
#[ignore]
async fn e2e_server_add_remove() {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);

    const SERVER_NAME: &str = "test-072-e2e";

    // Clean up any stale entry from a previous aborted run before we start.
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let _ = tools
            .call_for_test(
                "iris_remove_server",
                serde_json::json!({"name": SERVER_NAME}),
            )
            .await;
    }

    // ── Step 1: add the server ────────────────────────────────────────────────
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let v = parse_result(
            tools
                .call_for_test(
                    "iris_add_server",
                    serde_json::json!({
                        "name": SERVER_NAME,
                        "host": host,
                        "port": port,
                        "namespace": "USER",
                        "username": "_SYSTEM",
                        "password": "SYS"
                    }),
                )
                .await,
        );
        // Headless CI has no keychain — credential goes to servers.json (stored_plaintext=true).
        // KEYCHAIN_FAILED is no longer returned for KeychainUnavailable since 095.
        assert_eq!(
            v["added"], true,
            "iris_add_server should return added:true, got: {v}"
        );
        assert_eq!(v["name"], SERVER_NAME, "added name should match, got: {v}");
    }

    // ── Step 2: fresh pool — server must appear in iris_servers ──────────────
    {
        let tools = IrisTools::new(None).expect("IrisTools::new after add");
        let v = parse_result(
            tools
                .call_for_test("iris_servers", serde_json::json!({}))
                .await,
        );
        let servers = v["servers"].as_array().expect("servers should be array");
        let found = servers.iter().any(|s| s["name"] == SERVER_NAME);
        assert!(
            found,
            "iris_servers should list '{SERVER_NAME}' after iris_add_server; got: {v}"
        );
    }

    // ── Step 3: remove the server ─────────────────────────────────────────────
    {
        let tools = IrisTools::new(None).expect("IrisTools::new for remove");
        let v = parse_result(
            tools
                .call_for_test(
                    "iris_remove_server",
                    serde_json::json!({"name": SERVER_NAME}),
                )
                .await,
        );
        assert_eq!(
            v["removed"], true,
            "iris_remove_server should return removed:true, got: {v}"
        );
    }

    // ── Step 4: fresh pool — server must NOT appear in iris_servers ───────────
    {
        let tools = IrisTools::new(None).expect("IrisTools::new after remove");
        let v = parse_result(
            tools
                .call_for_test("iris_servers", serde_json::json!({}))
                .await,
        );
        let servers = v["servers"].as_array().expect("servers should be array");
        let found = servers.iter().any(|s| s["name"] == SERVER_NAME);
        assert!(
            !found,
            "iris_servers must NOT list '{SERVER_NAME}' after iris_remove_server; got: {v}"
        );
    }
}

// T030 — iris_test_server probes a registered server and returns reachable:true.
//
// Registers a server via iris_add_server, creates a fresh IrisTools so the pool
// picks it up from disk, calls iris_test_server, and asserts connectivity.
// Cleans up with iris_remove_server regardless of outcome.
#[tokio::test]
#[ignore]
async fn e2e_server_test() {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);

    const SERVER_NAME: &str = "test-072-e2e-probe";

    // Clean up any stale entry before starting.
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let _ = tools
            .call_for_test(
                "iris_remove_server",
                serde_json::json!({"name": SERVER_NAME}),
            )
            .await;
    }

    // ── Step 1: register the server ───────────────────────────────────────────
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let v = parse_result(
            tools
                .call_for_test(
                    "iris_add_server",
                    serde_json::json!({
                        "name": SERVER_NAME,
                        "host": host,
                        "port": port,
                        "namespace": "USER",
                        "username": "_SYSTEM",
                        "password": "SYS"
                    }),
                )
                .await,
        );
        // KEYCHAIN_FAILED means no default keychain store (e.g. headless CI) — skip gracefully.
        if v["error_code"].as_str() == Some("KEYCHAIN_FAILED") {
            eprintln!("e2e_server_test: no keychain available, skipping");
            return;
        }
        assert_eq!(v["added"], true, "iris_add_server should succeed, got: {v}");
    }

    // ── Step 2: fresh pool picks up the server — probe it ────────────────────
    let probe_result = {
        let tools = IrisTools::new(None).expect("IrisTools::new with server");
        parse_result(
            tools
                .call_for_test("iris_test_server", serde_json::json!({"name": SERVER_NAME}))
                .await,
        )
    };

    // ── Step 3: clean up — remove regardless of probe outcome ─────────────────
    {
        let tools = IrisTools::new(None).expect("IrisTools::new for cleanup");
        let _ = tools
            .call_for_test(
                "iris_remove_server",
                serde_json::json!({"name": SERVER_NAME}),
            )
            .await;
    }

    // ── Assert after cleanup so the entry is never left behind ───────────────
    assert_eq!(
        probe_result["reachable"], true,
        "iris_test_server should return reachable:true for a live IRIS instance; got: {probe_result}"
    );
    assert!(
        probe_result["atelier_version"].is_string() || probe_result["atelier_version"].is_number(),
        "iris_test_server should return an atelier_version; got: {probe_result}"
    );
}

/// T_LIVE_093_01 — iris_reload_pool hot-reload round-trip.
///
/// Verifies that:
///   1. iris_add_server persists a temp entry to the iad-native config.
///   2. iris_reload_pool rebuilds the in-memory pool and reports the server in `servers`.
///   3. The reloaded pool can route to the newly-added server (iris_test_server returns
///      something other than SERVER_NOT_FOUND — connection errors are acceptable since
///      the target port may not be live, but a SERVER_NOT_FOUND proves pool miss).
///   4. Clean up via iris_remove_server.
#[tokio::test]
#[ignore]
async fn e2e_reload_pool_hot_swap() {
    const RELOAD_SERVER: &str = "reload-test-093";

    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(52780);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());

    let tools = IrisTools::new(None).expect("IrisTools::new");

    // ── Step 1: add the server ────────────────────────────────────────────────
    let add_result = parse_result(
        tools
            .call_for_test(
                "iris_add_server",
                serde_json::json!({
                    "name": RELOAD_SERVER,
                    "host": host,
                    "port": port,
                    "namespace": "USER",
                    "username": username,
                    "password": password
                }),
            )
            .await,
    );

    if add_result["error_code"].as_str() == Some("KEYCHAIN_FAILED") {
        eprintln!("e2e_reload_pool_hot_swap: no keychain available, skipping");
        return;
    }
    assert!(
        add_result["added"].as_bool().unwrap_or(false)
            || add_result["stored_plaintext"].as_bool().unwrap_or(false),
        "iris_add_server must succeed, got: {add_result}"
    );

    // ── Step 2: hot-reload the pool ───────────────────────────────────────────
    let reload_result = parse_result(
        tools
            .call_for_test("iris_reload_pool", serde_json::json!({}))
            .await,
    );

    assert_eq!(
        reload_result["success"].as_bool(),
        Some(true),
        "iris_reload_pool must succeed, got: {reload_result}"
    );
    let servers: Vec<String> = reload_result["servers"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    assert!(
        servers.contains(&RELOAD_SERVER.to_string()),
        "iris_reload_pool servers must include {RELOAD_SERVER}, got: {servers:?}"
    );

    // ── Step 3: probe the newly-loaded server — must NOT be SERVER_NOT_FOUND ─
    let probe_result = parse_result(
        tools
            .call_for_test(
                "iris_test_server",
                serde_json::json!({"name": RELOAD_SERVER}),
            )
            .await,
    );
    assert_ne!(
        probe_result["error_code"].as_str(),
        Some("SERVER_NOT_FOUND"),
        "iris_test_server must not return SERVER_NOT_FOUND after hot-reload, got: {probe_result}"
    );

    // ── Step 4: clean up ─────────────────────────────────────────────────────
    let _ = tools
        .call_for_test(
            "iris_remove_server",
            serde_json::json!({"name": RELOAD_SERVER}),
        )
        .await;
}

/// T022 — iris_test_server ad-hoc probe: reachable:true, auth:true.
///
/// Calls `iris_test_server` with raw host/port/username/password (no pool entry).
/// Asserts `reachable: true`, `auth: true`, and a non-null `iris_version`.
#[tokio::test]
#[ignore]
async fn test_iris_test_server_adhoc_reachable() {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());

    let tools = IrisTools::new(None).expect("IrisTools::new");
    let v = parse_result(
        tools
            .call_for_test(
                "iris_test_server",
                serde_json::json!({
                    "host": host,
                    "web_port": port,
                    "username": username,
                    "password": password,
                }),
            )
            .await,
    );

    assert_eq!(
        v["reachable"].as_bool(),
        Some(true),
        "T022: ad-hoc probe must return reachable:true; got: {v}"
    );
    assert_eq!(
        v["auth"].as_bool(),
        Some(true),
        "T022: ad-hoc probe must return auth:true; got: {v}"
    );
    assert!(
        v["iris_version"].is_string() || v["iris_version"].is_number(),
        "T022: ad-hoc probe must return iris_version; got: {v}"
    );
}

/// T023 — iris_test_server ad-hoc probe: wrong password → reachable:true, auth:false.
#[tokio::test]
#[ignore]
async fn test_iris_test_server_adhoc_wrong_password() {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let correct_pw = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());

    let tools = IrisTools::new(None).expect("IrisTools::new");

    // Pre-check: if a known-wrong password also returns auth:true, the container has
    // authentication disabled (e.g. fresh community container after clear_password_change_flag).
    // In that state the test cannot meaningfully assert auth:false — skip it.
    let precheck = parse_result(
        tools
            .call_for_test(
                "iris_test_server",
                serde_json::json!({
                    "host": host,
                    "web_port": port,
                    "username": "_SYSTEM",
                    "password": correct_pw,
                }),
            )
            .await,
    );
    if precheck["auth"].as_bool() != Some(true) {
        eprintln!("skipping T023: correct password did not return auth:true — container may be down or misconfigured");
        return;
    }

    let v = parse_result(
        tools
            .call_for_test(
                "iris_test_server",
                serde_json::json!({
                    "host": host,
                    "web_port": port,
                    "username": "_SYSTEM",
                    "password": "WRONGPASS_098",
                }),
            )
            .await,
    );

    if v["auth"].as_bool() == Some(true) {
        eprintln!("skipping T023: IRIS container accepts any password (auth enforcement disabled — common after fresh_container_setup on community image)");
        return;
    }

    assert_eq!(
        v["reachable"].as_bool(),
        Some(true),
        "T023: wrong-password probe must return reachable:true (server is up); got: {v}"
    );
    assert_eq!(
        v["auth"].as_bool(),
        Some(false),
        "T023: wrong-password probe must return auth:false; got: {v}"
    );
}

/// T023b — SC-002: discover-then-add workflow end-to-end.
///
/// Calls `iris_test_server` (ad-hoc probe) to confirm the server is reachable,
/// then `iris_add_server` to persist it, then `iris_servers` to verify it is in the pool.
#[tokio::test]
#[ignore]
async fn test_sc002_discover_then_add_workflow() {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());

    const SERVER_NAME: &str = "sc002-discover-then-add-098";

    // Clean up any stale entry from a previous run.
    let tools = IrisTools::new(None).expect("IrisTools::new");
    let _ = tools
        .call_for_test(
            "iris_remove_server",
            serde_json::json!({"name": SERVER_NAME}),
        )
        .await;

    // Step 1: ad-hoc probe confirms reachable.
    let probe_v = parse_result(
        tools
            .call_for_test(
                "iris_test_server",
                serde_json::json!({
                    "host": host,
                    "web_port": port,
                    "username": username,
                    "password": password,
                }),
            )
            .await,
    );
    assert_eq!(
        probe_v["reachable"].as_bool(),
        Some(true),
        "T023b: ad-hoc probe must be reachable; got: {probe_v}"
    );

    // Step 2: add the server to the pool.
    let add_v = parse_result(
        tools
            .call_for_test(
                "iris_add_server",
                serde_json::json!({
                    "name": SERVER_NAME,
                    "host": host,
                    "port": port,
                    "namespace": "USER",
                    "username": username,
                    "password": password,
                }),
            )
            .await,
    );
    if add_v["error_code"].as_str() == Some("KEYCHAIN_FAILED") {
        eprintln!("T023b: no keychain available, skipping add step");
        return;
    }
    assert!(
        add_v["added"].as_bool().unwrap_or(false)
            || add_v["stored_plaintext"].as_bool().unwrap_or(false),
        "T023b: iris_add_server must succeed; got: {add_v}"
    );

    // Step 3: fresh pool must contain the server.
    let list_v = parse_result(
        IrisTools::new(None)
            .expect("IrisTools::new")
            .call_for_test("iris_servers", serde_json::json!({}))
            .await,
    );

    // Clean up.
    let _ = tools
        .call_for_test(
            "iris_remove_server",
            serde_json::json!({"name": SERVER_NAME}),
        )
        .await;

    let servers = list_v["servers"].as_array().expect("servers array");
    assert!(
        servers
            .iter()
            .any(|s| s["name"].as_str() == Some(SERVER_NAME)),
        "T023b: server must appear in pool after iris_add_server; got: {list_v}"
    );
}

/// T032 — iris_servers(probe=true): live server entry has reachable:true, latency_ms present.
///
/// Registers a server, then calls `iris_servers` with `probe=true`, and asserts
/// the entry for that server has `reachable: true` and a non-null `latency_ms`.
#[tokio::test]
#[ignore]
async fn test_iris_servers_probe_true_live() {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());

    const SERVER_NAME: &str = "probe-test-098-us2";

    // Clean up any stale entry.
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let _ = tools
            .call_for_test(
                "iris_remove_server",
                serde_json::json!({"name": SERVER_NAME}),
            )
            .await;
    }

    // Register a server.
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let v = parse_result(
            tools
                .call_for_test(
                    "iris_add_server",
                    serde_json::json!({
                        "name": SERVER_NAME,
                        "host": host,
                        "port": port,
                        "namespace": "USER",
                        "username": username,
                        "password": password,
                    }),
                )
                .await,
        );
        if v["error_code"].as_str() == Some("KEYCHAIN_FAILED") {
            eprintln!("T032: no keychain, skipping");
            return;
        }
        assert!(
            v["added"].as_bool().unwrap_or(false)
                || v["stored_plaintext"].as_bool().unwrap_or(false),
            "T032: iris_add_server must succeed; got: {v}"
        );
    }

    // Fresh pool picks up the server, then probe.
    let probe_v = {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        parse_result(
            tools
                .call_for_test("iris_servers", serde_json::json!({"probe": true}))
                .await,
        )
    };

    // Clean up.
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let _ = tools
            .call_for_test(
                "iris_remove_server",
                serde_json::json!({"name": SERVER_NAME}),
            )
            .await;
    }

    let servers = probe_v["servers"].as_array().expect("servers array");
    let entry = servers
        .iter()
        .find(|s| s["name"].as_str() == Some(SERVER_NAME))
        .unwrap_or_else(|| {
            panic!("T032: must find {SERVER_NAME} in probe response; got: {probe_v}")
        });

    assert_eq!(
        entry["reachable"].as_bool(),
        Some(true),
        "T032: live server must be reachable:true; got: {entry}"
    );
    assert!(
        entry["latency_ms"].is_number(),
        "T032: live server must have latency_ms; got: {entry}"
    );
}

/// T033 — iris_servers(probe=true): differential result when one server is reachable and one is not.
///
/// Registers two servers: one live (iris-dev-iris) and one pointing to a closed port.
/// Calls `iris_servers(probe=true)` and asserts the live entry has `reachable:true` and
/// the closed-port entry has `reachable:false`.
#[tokio::test]
#[ignore]
async fn test_iris_servers_probe_differential() {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());

    const LIVE_NAME: &str = "t033-live-server-098";
    const DEAD_NAME: &str = "t033-dead-server-098";

    // Clean up stale entries.
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let _ = tools
            .call_for_test("iris_remove_server", serde_json::json!({"name": LIVE_NAME}))
            .await;
        let _ = tools
            .call_for_test("iris_remove_server", serde_json::json!({"name": DEAD_NAME}))
            .await;
    }

    // Add live server.
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let v = parse_result(
            tools
                .call_for_test(
                    "iris_add_server",
                    serde_json::json!({
                        "name": LIVE_NAME,
                        "host": host,
                        "port": port,
                        "namespace": "USER",
                        "username": username,
                        "password": password,
                    }),
                )
                .await,
        );
        if v["error_code"].as_str() == Some("KEYCHAIN_FAILED") {
            eprintln!("T033: no keychain, skipping");
            return;
        }
        assert!(
            v["added"].as_bool().unwrap_or(false)
                || v["stored_plaintext"].as_bool().unwrap_or(false),
            "T033: live add must succeed; got: {v}"
        );
    }

    // Add dead server (port 1 is always closed).
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let v = parse_result(
            tools
                .call_for_test(
                    "iris_add_server",
                    serde_json::json!({
                        "name": DEAD_NAME,
                        "host": "127.0.0.1",
                        "port": 1,
                        "namespace": "USER",
                        "username": "_SYSTEM",
                        "password": "SYS",
                    }),
                )
                .await,
        );
        assert!(
            v["added"].as_bool().unwrap_or(false)
                || v["stored_plaintext"].as_bool().unwrap_or(false),
            "T033: dead add must succeed; got: {v}"
        );
    }

    // Probe both.
    let probe_v = parse_result(
        IrisTools::new(None)
            .expect("IrisTools::new")
            .call_for_test("iris_servers", serde_json::json!({"probe": true}))
            .await,
    );

    // Clean up.
    {
        let tools = IrisTools::new(None).expect("IrisTools::new");
        let _ = tools
            .call_for_test("iris_remove_server", serde_json::json!({"name": LIVE_NAME}))
            .await;
        let _ = tools
            .call_for_test("iris_remove_server", serde_json::json!({"name": DEAD_NAME}))
            .await;
    }

    let servers = probe_v["servers"].as_array().expect("servers array");

    let live_entry = servers
        .iter()
        .find(|s| s["name"].as_str() == Some(LIVE_NAME))
        .unwrap_or_else(|| panic!("T033: must find {LIVE_NAME} in probe response; got: {probe_v}"));
    let dead_entry = servers
        .iter()
        .find(|s| s["name"].as_str() == Some(DEAD_NAME))
        .unwrap_or_else(|| panic!("T033: must find {DEAD_NAME} in probe response; got: {probe_v}"));

    assert_eq!(
        live_entry["reachable"].as_bool(),
        Some(true),
        "T033: live server must be reachable:true; got: {live_entry}"
    );
    assert_eq!(
        dead_entry["reachable"].as_bool(),
        Some(false),
        "T033: closed-port server must be reachable:false; got: {dead_entry}"
    );
}

// T029 — 093 US2: background pool reload triggered by config file change.
//
// Constructs IrisTools with a ConfigWatcher pointing at a temp config file.
// Writes an initial config with just the live dev connection, calls iris_servers
// to establish the mtime baseline, then appends a new [instance.*] entry and calls
// iris_servers again — which triggers check_reload and should swap the pool.
//
// Requires: live iris-dev-iris at localhost:52780
// Run with: --include-ignored --test-threads=1
#[tokio::test]
#[ignore]
async fn test_background_pool_reload() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".iris-agentic-dev.toml");

    // mode = "operate" is required for [instance.*] entries to load from the pool
    let initial = "mode = \"operate\"\n\n[connection]\nhost = \"localhost\"\nport = 52780\nnamespace = \"USER\"\nusername = \"_SYSTEM\"\npassword = \"SYS\"\n";
    std::fs::write(&config_path, initial).unwrap();

    // Point workspace_root() at the temp dir so load_pool uses our test config.
    std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());

    // Build IrisTools with a watcher on the temp config file.
    // ConfigWatcher::new reads the current mtime so has_changed() is false initially.
    let watcher = ConfigWatcher::new(config_path.clone());
    let tools = IrisTools::with_registry_and_toolset(
        None,
        iris_agentic_dev_core::skills::SkillRegistry::new(),
        iris_agentic_dev_core::tools::Toolset::Baseline,
        watcher,
        Some(config_path.clone()),
        false,
        iris_agentic_dev_core::tools::write_gate::DeclaredGates::default(),
    )
    .expect("IrisTools::with_registry_and_toolset");

    // First call establishes the mtime baseline (check_reload sees no change).
    let before = parse_result(
        tools
            .call_for_test("iris_servers", serde_json::json!({}))
            .await,
    );
    let empty = vec![];
    let before_names: Vec<&str> = before["servers"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert!(
        !before_names.contains(&"t029-bg-reload-srv"),
        "T029: t029-bg-reload-srv must not exist before config update; got: {before}"
    );

    // Write a new config with a fleet instance added. Sleep 1ms + rename to ensure
    // the mtime is strictly newer than what the watcher last saw.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let updated = format!(
        "{initial}\n[instance.t029-bg-reload-srv]\nhost = \"192.0.2.1\"\nport = 52773\nnamespace = \"USER\"\nusername = \"_SYSTEM\"\npassword = \"SYS\"\n"
    );
    let tmp = config_path.with_extension("tmp");
    std::fs::write(&tmp, &updated).unwrap();
    std::fs::rename(&tmp, &config_path).unwrap();

    // Second call triggers check_reload → detects mtime change → reloads pool (US2).
    let after = parse_result(
        tools
            .call_for_test("iris_servers", serde_json::json!({}))
            .await,
    );

    std::env::remove_var("OBJECTSCRIPT_WORKSPACE");

    let empty2 = vec![];
    let after_names: Vec<&str> = after["servers"]
        .as_array()
        .unwrap_or(&empty2)
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();

    assert!(
        after_names.contains(&"t029-bg-reload-srv"),
        "T029: pool must include t029-bg-reload-srv after background config reload; got: {after}"
    );
}
