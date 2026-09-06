//! Live Docker + Atelier integration tests for iris/discovery.rs.
//!
//! These tests require:
//!   - A real Docker daemon running, reachable via the standard socket.
//!   - A running IRIS container with its private web port (52773) mapped to a host
//!     port. Name it in `IRIS_CONTAINER` and give the host port in `IRIS_WEB_PORT`;
//!     both default to this laptop's `iris-dev-iris` on 52780.
//!
//! ALL tests here are `#[ignore]` — they do not run on a normal `cargo test`
//! and must be invoked explicitly:
//!
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!     cargo test -p iris-agentic-dev-core --features testing \
//!     --test test_discovery_docker_live -- --include-ignored --nocapture
//!
//! Do NOT modify `tests/discovery_tests.rs` — that file intentionally asserts
//! NotFound/Explained behavior when no live IRIS is configured, and must keep
//! passing unmodified.

use iris_agentic_dev_core::iris::discovery::{
    discover_via_docker, discover_via_docker_named, probe_atelier, probe_atelier_with_client,
    DiscoveryResult,
};

/// Resolve the host port mapped to iris-dev-iris's private web port (52773).
/// Falls back to the known stable value for this session if IRIS_WEB_PORT is unset.
fn live_port() -> u16 {
    std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(52780)
}

fn live_username() -> String {
    std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string())
}

fn live_password() -> String {
    std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string())
}

/// The container to look for by name.
///
/// `iris-dev-iris` is one laptop's convention. CI names its container `iris-e2e`, so pinning the
/// literal here meant the only way to keep CI green was to `--skip` this test — which is how a
/// discovery test stopped covering discovery on the one machine where a regression would matter.
fn live_container() -> String {
    std::env::var("IRIS_CONTAINER")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "iris-dev-iris".to_string())
}

// ── probe_atelier / probe_atelier_with_client against the real container ────

#[tokio::test]
#[ignore = "requires live iris-dev-iris container on the mapped Atelier port"]
async fn probe_atelier_succeeds_against_real_container() {
    let port = live_port();
    let result = probe_atelier(
        "localhost",
        port,
        &live_username(),
        &live_password(),
        "USER",
        5000,
    )
    .await;

    let conn = result.expect("expected Some(IrisConnection) from a real IRIS Atelier endpoint");
    let version = conn.version.expect("version must be populated");
    assert!(
        version.to_uppercase().contains("IRIS"),
        "version string should contain IRIS, got: {version}"
    );
}

#[tokio::test]
#[ignore = "requires live iris-dev-iris container on the mapped Atelier port"]
async fn probe_atelier_with_client_succeeds_against_real_container() {
    let port = live_port();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(5000))
        .danger_accept_invalid_certs(true)
        .build()
        .expect("client builds");

    let result = probe_atelier_with_client(
        &client,
        "localhost",
        port,
        &live_username(),
        &live_password(),
        "USER",
    )
    .await;

    let conn = result.expect("expected Some(IrisConnection)");
    assert!(conn.base_url.contains(&port.to_string()));
    let version = conn.version.expect("version must be populated");
    assert!(version.to_uppercase().contains("IRIS"));
}

#[tokio::test]
#[ignore = "requires live iris-dev-iris container on the mapped Atelier port"]
async fn probe_atelier_against_closed_port_returns_none() {
    // Port 1 is reserved/unused on virtually every machine — connection refused
    // immediately rather than timing out, so this stays fast even without a timeout tune.
    let result = probe_atelier("localhost", 1, "_SYSTEM", "SYS", "USER", 1000).await;
    assert!(
        result.is_none(),
        "a definitely-closed port must yield None, not panic or hang"
    );
}

#[tokio::test]
#[ignore = "requires live iris-dev-iris container on the mapped Atelier port"]
async fn probe_atelier_wrong_credentials_returns_none() {
    let port = live_port();
    // Wrong password against a real, reachable IRIS Atelier endpoint must come back
    // as a clean None (401 path), never panic.
    let result = probe_atelier(
        "localhost",
        port,
        &live_username(),
        "definitely-wrong-password",
        "USER",
        5000,
    )
    .await;
    assert!(
        result.is_none(),
        "wrong credentials must yield None (401), not Some(conn)"
    );
}

// ── discover_via_docker_named ────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires live Docker daemon + the container named by IRIS_CONTAINER"]
async fn discover_via_docker_named_finds_real_container() {
    let container = live_container();
    let result = discover_via_docker_named(&container).await;
    match result {
        DiscoveryResult::Connected(conn) => {
            let version = conn.version.expect("version populated");
            assert!(version.to_uppercase().contains("IRIS"));
        }
        other => panic!("expected Connected for {container}, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires live Docker daemon"]
async fn discover_via_docker_named_missing_container_returns_not_found() {
    let result = discover_via_docker_named("definitely-does-not-exist-99999").await;
    assert!(
        matches!(result, DiscoveryResult::NotFound),
        "a nonexistent container name must yield NotFound, got {result:?}"
    );
}

// ── discover_via_docker (full unnamed scan) ──────────────────────────────────

#[tokio::test]
#[ignore = "requires live Docker daemon + at least one reachable IRIS container"]
async fn discover_via_docker_full_scan_finds_something() {
    // Multiple IRIS containers may be running on this machine concurrently
    // (e.g. los-iris, opsreview-iris, iris-dev-iris) — discover_via_docker has no
    // way to disambiguate beyond score_container_name + first-successful-probe
    // ordering, so we only assert the weaker invariant: it does not panic and it
    // finds *some* reachable IRIS instance, not which one specifically.
    let result = discover_via_docker().await;
    assert!(
        result.is_some(),
        "expected to find at least one reachable IRIS container via Docker scan"
    );
    let conn = result.unwrap();
    assert!(conn.base_url.starts_with("http://localhost:"));
}
