//! Integration tests for iris_add_server / iris_servers / iris_remove_server / iris_test_server
//! (072-multi-instance-pool).
//!
//! Requires a live IRIS instance reachable at IRIS_HOST:IRIS_WEB_PORT.
//! All tests are #[ignore] — run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test -p iris-agentic-dev-core --features testing \
//!     --test test_server_pool_e2e -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::tools::IrisTools;

/// Extract JSON from the first text content of a CallToolResult.
fn parse_result(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let result = r.expect("call_for_test returned Err");
    let text = result
        .content
        .first()
        .expect("result has no content")
        .raw
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
        // KEYCHAIN_FAILED means no default keychain store (e.g. headless CI) — skip gracefully.
        if v["error_code"].as_str() == Some("KEYCHAIN_FAILED") {
            eprintln!("e2e_server_add_remove: no keychain available, skipping");
            return;
        }
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
