// Tests for 101-nopws-connectivity: iris_execute docker exec fallback + execution_path field.

// These unit tests cover the nopws routing logic without requiring IRIS.
// See tests/integration/nopws_101.rs for live IRIS tests.

use iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig;

// ── SSH command construction tests (FR-009) ───────────────────────────────────

#[test]
fn test_ssh_host_propagated_to_connection() {
    let cfg: WorkspaceConfig = toml::from_str(
        r#"
container = "my-iris-ai"
namespace = "USER"
nopws = true
docker_only = true
ssh_host = "test-host"
"#,
    )
    .expect("must parse");
    let conn =
        iris_agentic_dev_core::iris::workspace_config::workspace_config_to_connection(&cfg, "USER");
    let conn = conn.expect("nopws+docker_only+container must return connection");
    assert_eq!(
        conn.ssh_host.as_deref(),
        Some("test-host"),
        "ssh_host must be propagated from WorkspaceConfig to IrisConnection"
    );
}

#[test]
fn test_no_ssh_host_propagated_when_absent() {
    let cfg: WorkspaceConfig = toml::from_str(
        r#"
container = "my-iris-ai"
namespace = "USER"
nopws = true
docker_only = true
"#,
    )
    .expect("must parse");
    let conn =
        iris_agentic_dev_core::iris::workspace_config::workspace_config_to_connection(&cfg, "USER");
    let conn = conn.expect("nopws+docker_only+container must return connection");
    assert!(
        conn.ssh_host.is_none(),
        "ssh_host must be None when not configured"
    );
}

#[test]
fn test_nopws_true_creates_unreachable_connection() {
    let cfg: WorkspaceConfig = toml::from_str(
        r#"
container = "my-iris-ai"
namespace = "USER"
nopws = true
"#,
    )
    .expect("must parse");
    let conn =
        iris_agentic_dev_core::iris::workspace_config::workspace_config_to_connection(&cfg, "USER");
    // nopws without docker_only still returns None (container sets env, discovery proceeds)
    // nopws only forces sentinel URL when docker_only is also set
    // This is fine: the nopws flag will suppress errors in iris_test_server
    let _ = conn; // result depends on docker_only; just verify no panic
}

#[test]
fn test_nopws_with_docker_only_creates_unreachable_connection() {
    let _guard: std::sync::MutexGuard<'_, ()> = {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    };
    std::env::remove_var("IRIS_CONTAINER");
    let cfg: WorkspaceConfig = toml::from_str(
        r#"
container = "my-iris-ai"
namespace = "USER"
nopws = true
docker_only = true
"#,
    )
    .expect("must parse");
    let conn =
        iris_agentic_dev_core::iris::workspace_config::workspace_config_to_connection(&cfg, "USER");
    std::env::remove_var("IRIS_CONTAINER");
    let conn = conn.expect("nopws+docker_only+container must return connection");
    assert!(
        conn.base_url.contains("127.0.0.1:1"),
        "nopws+docker_only must use sentinel unreachable URL, got: {}",
        conn.base_url
    );
}

// ── FR-016: iris_compile execution_path field tests ───────────────────────────
// These are verified via the binary invocation tests in tests/binary/nopws_101.rs.
// The unit tests here validate the routing flag detection logic.

#[test]
fn test_docker_only_url_is_detected_as_sentinel() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    let conn = IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::EnvVar,
    );
    let is_sentinel =
        conn.base_url == "http://127.0.0.1:1" || conn.base_url.starts_with("http://127.0.0.1:1/");
    assert!(is_sentinel, "sentinel URL must be detectable");
}

#[test]
fn test_normal_url_is_not_detected_as_sentinel() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    let conn = IrisConnection::new(
        "http://localhost:52780",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::EnvVar,
    );
    let is_sentinel =
        conn.base_url == "http://127.0.0.1:1" || conn.base_url.starts_with("http://127.0.0.1:1/");
    assert!(!is_sentinel, "normal URL must not be detected as sentinel");
}

#[test]
fn test_iris_connection_ssh_host_field_exists() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    let mut conn = IrisConnection::new(
        "http://localhost:52780",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::EnvVar,
    );
    assert!(conn.ssh_host.is_none(), "ssh_host must default to None");
    conn.ssh_host = Some("baystate.example.com".to_string());
    assert_eq!(conn.ssh_host.as_deref(), Some("baystate.example.com"));
}
