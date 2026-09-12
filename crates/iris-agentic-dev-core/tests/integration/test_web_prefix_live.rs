//! Layer 3 for `web_prefix` (116, issue #129) — the assembled URL against a real IRIS.
//!
//! A prefix that is only ever asserted as a string proves the format call, not that IRIS is being
//! asked for a different path. These register the same `iris-dev-iris` container twice — once at
//! the root, once under a prefix it does not serve — and probe both. The prefixed one must not
//! complete an Atelier handshake, and the root one must.
//!
//! The signal is the handshake, not the HTTP status. What a web server answers for a path it does
//! not serve is version-dependent: Community 2026.2 returns 404, and the 2025.3 image CI runs
//! returns a 200 carrying an HTML error page. Only one thing is true on both — a path that is not
//! Atelier cannot report an IRIS version.
//!
//! Requires a live container. Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!     cargo test --features testing --test integration -- --include-ignored --test-threads=1

use iris_agentic_dev_core::tools::IrisTools;

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

fn live_target() -> (String, u16) {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    (host, port)
}

async fn remove(name: &str) {
    let tools = IrisTools::new(None).expect("IrisTools::new");
    let _ = tools
        .call_for_test("iris_remove_server", serde_json::json!({"name": name}))
        .await;
}

/// Register `name` against the live container, optionally under `prefix`, and probe it from a fresh
/// pool (the running pool does not hot-reload).
async fn add_and_probe(name: &str, prefix: Option<&str>) -> serde_json::Value {
    let (host, port) = live_target();
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());

    let mut args = serde_json::json!({
        "name": name,
        "host": host,
        "port": port,
        "namespace": "USER",
        "username": username,
        "password": password,
    });
    if let Some(p) = prefix {
        args["web_prefix"] = serde_json::json!(p);
    }

    let tools = IrisTools::new(None).expect("IrisTools::new");
    let added = parse_result(tools.call_for_test("iris_add_server", args).await);
    assert_eq!(added["added"], true, "iris_add_server failed: {added}");

    let tools = IrisTools::new(None).expect("IrisTools::new after add");
    parse_result(
        tools
            .call_for_test("iris_test_server", serde_json::json!({"name": name}))
            .await,
    )
}

/// Did this probe complete an Atelier handshake? Atelier's root reports the IRIS version, so a
/// version string means iad reached Atelier and parsed its answer. Every other outcome — a 404, a
/// 200 carrying an error page, a connection error — leaves the field absent.
fn atelier_version_reported(probe: &serde_json::Value) -> Option<&str> {
    probe["iris_version"].as_str().filter(|v| !v.is_empty())
}

/// The criterion above against the payload shapes the two IRIS versions actually produce, so the
/// three tests below keep meaning what they say on a runner this machine is not. Needs no IRIS: the
/// first two are what a wrong path returns, the third is what Atelier returns.
#[test]
fn the_handshake_criterion_holds_on_both_iris_versions() {
    // Community 2026.2 (local iris-dev-iris): 404 for a path it does not serve.
    let not_found = serde_json::json!({
        "name": "test-116-prefixed", "reachable": false, "nopws": false,
        "web_available": false, "http_status": 404, "latency_ms": 3
    });
    // IRIS 2025.3 (the CI image): 200 with an HTML error page, so the JSON parse fails and no
    // version comes back. Asserting `reachable == false` here is what broke on CI.
    let html_error_page = serde_json::json!({
        "name": "test-116-prefixed", "reachable": true, "nopws": false, "web_available": true,
        "auth": true, "latency_ms": 2, "parse_error": "error decoding response body"
    });
    let atelier_root = serde_json::json!({
        "name": "test-116-root", "reachable": true, "nopws": false, "web_available": true,
        "auth": true, "atelier_version": 9, "latency_ms": 4,
        "iris_version": "IRIS for UNIX (Ubuntu Server LTS for x86-64 Containers) 2026.2"
    });

    assert_eq!(atelier_version_reported(&not_found), None);
    assert_eq!(atelier_version_reported(&html_error_page), None);
    assert!(atelier_version_reported(&atelier_root).is_some());
    assert!(
        not_found["error"].is_null() && html_error_page["error"].is_null(),
        "neither shape carries `error`, so the connection-error guard must not reject them"
    );
}

/// The prefix-less control. Without this the failure below proves nothing — the container could be
/// down, the credential wrong, anything.
#[tokio::test]
#[ignore]
async fn a_prefix_less_entry_still_reaches_atelier() {
    const NAME: &str = "test-116-root";
    remove(NAME).await;
    let probe = add_and_probe(NAME, None).await;
    remove(NAME).await;
    assert_eq!(
        probe["reachable"], true,
        "the container must be reachable at the root for the prefixed case to mean anything: \
         {probe}"
    );
    assert!(
        atelier_version_reported(&probe).is_some(),
        "the root probe must complete an Atelier handshake and report a version, or the prefixed \
         case cannot tell a dropped prefix from an unrelated failure: {probe}"
    );
}

/// The prefix reaches the request. `iris-dev-iris` serves Atelier at `/api/atelier/`, not at
/// `/iad116-no-such-prefix/api/atelier/`, so a probe that still succeeds means the prefix was
/// dropped somewhere between `servers.json` and the HTTP call — which is the reported bug.
#[tokio::test]
#[ignore]
async fn a_prefixed_entry_asks_iris_for_the_prefixed_path() {
    const NAME: &str = "test-116-prefixed";
    remove(NAME).await;
    let probe = add_and_probe(NAME, Some("/iad116-no-such-prefix")).await;
    remove(NAME).await;
    assert_eq!(
        atelier_version_reported(&probe),
        None,
        "a path IRIS does not serve must not complete an Atelier handshake — a version here means \
         the prefix was dropped and the probe hit the root: {probe}"
    );
    assert!(
        probe["error"].is_null(),
        "the probe must have reached the web server and been answered on the prefixed path, not \
         failed to connect at all — a connection error would make this test pass for the wrong \
         reason: {probe}"
    );
}

/// FR-004 against a real pool: the listing reports the prefix of a registered entry, and probing it
/// asks for the prefixed path. `iris_servers(probe: true)` used to build its own URL from host and
/// port, so it reported a prefixed entry as healthy no matter what the prefix was.
#[tokio::test]
#[ignore]
async fn iris_servers_probes_the_prefixed_url_not_the_root() {
    const NAME: &str = "test-116-listing";
    remove(NAME).await;

    let (host, port) = live_target();
    let tools = IrisTools::new(None).expect("IrisTools::new");
    let added = parse_result(
        tools
            .call_for_test(
                "iris_add_server",
                serde_json::json!({
                    "name": NAME,
                    "host": host,
                    "port": port,
                    "namespace": "USER",
                    "username": std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into()),
                    "password": std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into()),
                    "web_prefix": "/iad116-no-such-prefix",
                }),
            )
            .await,
    );
    assert_eq!(added["added"], true, "iris_add_server failed: {added}");

    let tools = IrisTools::new(None).expect("IrisTools::new after add");
    let listing = parse_result(
        tools
            .call_for_test("iris_servers", serde_json::json!({"probe": true}))
            .await,
    );
    remove(NAME).await;

    let entry = listing["servers"]
        .as_array()
        .expect("servers array")
        .iter()
        .find(|s| s["name"] == NAME)
        .cloned()
        .unwrap_or_else(|| panic!("`{NAME}` missing from the listing: {listing}"));

    assert_eq!(
        entry["web_prefix"].as_str(),
        Some("/iad116-no-such-prefix"),
        "the listing must report the prefix: {entry}"
    );
    assert_eq!(
        atelier_version_reported(&entry),
        None,
        "the probe must ask for the prefixed path, which this container does not serve — a version \
         here means it probed the root instead: {entry}"
    );
}
