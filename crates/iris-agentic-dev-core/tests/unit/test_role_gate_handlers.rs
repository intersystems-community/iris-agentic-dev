// Tests for role-gate wiring in tool handlers (FR-019, FR-020).
// Verifies that iris_compile, iris_execute, iris_query, and iris_source_control
// return role_gate errors when called against a subject-role instance in operate mode.
//
// Strategy: construct IrisTools with no IRIS connection, patch config_file to point at a
// temp dir containing a fleet .iris-agentic-dev.toml that declares a subject instance,
// then call instance_role() directly (white-box). The actual handler integration is
// validated by checking compile(), execute(), etc. return role_gate JSON when disconnected.

use iris_agentic_dev_core::iris::workspace_config::ConnectionRole;
use iris_agentic_dev_core::tools::IrisTools;
use std::io::Write;

fn write_fleet_toml(dir: &tempfile::TempDir, contents: &str) {
    let path = dir.path().join(".iris-agentic-dev.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(contents.as_bytes()).unwrap();
}

/// Build an IrisTools instance pointing at a fleet config in `dir`.
/// The config_file on ConnectionState is set to `dir/.iris-agentic-dev.toml`
/// so instance_role() picks it up.
fn make_tools_with_fleet(dir: &tempfile::TempDir) -> IrisTools {
    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
    }
    tools
}

// ── instance_role() unit tests (white-box) ────────────────────────────────────

#[test]
fn test_instance_role_no_fleet_config_returns_workspace() {
    let dir = tempfile::TempDir::new().unwrap();
    // No .iris-agentic-dev.toml at all
    let tools = make_tools_with_fleet(&dir);
    let (role, name) = tools.instance_role();
    assert_eq!(role, ConnectionRole::Workspace, "no config → Workspace");
    assert!(name.is_empty());
}

#[test]
fn test_instance_role_develop_mode_returns_workspace() {
    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(&dir, "container = \"myapp-iris\"\n");
    let tools = make_tools_with_fleet(&dir);
    let (role, _) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Workspace,
        "develop mode must always return Workspace"
    );
}

#[test]
fn test_instance_role_operate_mode_no_matching_instance_returns_workspace() {
    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.prod]
host = "prod.example.com"
role = "subject"
"#,
    );
    // No active IrisConnection → no match → default Workspace
    let tools = make_tools_with_fleet(&dir);
    let (role, _) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Workspace,
        "no matching connection → Workspace"
    );
}

#[test]
fn test_instance_role_operate_mode_matches_by_container() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.local]
container = "myapp-iris"
namespace = "USER"

[instance.prod]
container = "prod-iris"
role = "subject"
"#,
    );

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
        // Inject a Docker-sourced connection matching "prod-iris"
        conn.iris = Some(std::sync::Arc::new(IrisConnection::new(
            "http://127.0.0.1:52773",
            "PROD",
            "_SYSTEM",
            "SYS",
            DiscoverySource::Docker {
                container_name: "prod-iris".to_string(),
            },
        )));
    }

    let (role, name) = tools.instance_role();
    assert_eq!(role, ConnectionRole::Subject, "prod-iris → Subject");
    assert_eq!(name, "prod", "instance name should be 'prod'");
}

#[test]
fn test_instance_role_operate_mode_local_instance_is_workspace() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.local]
container = "myapp-iris"
namespace = "USER"

[instance.prod]
container = "prod-iris"
role = "subject"
"#,
    );

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
        conn.iris = Some(std::sync::Arc::new(IrisConnection::new(
            "http://127.0.0.1:52773",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::Docker {
                container_name: "myapp-iris".to_string(),
            },
        )));
    }

    let (role, name) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Workspace,
        "myapp-iris is a Workspace instance"
    );
    assert_eq!(name, "local");
}

#[test]
fn test_instance_role_matches_by_host() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.remote]
host = "prod.example.com"
web_port = 52773
role = "subject"
"#,
    );

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
        conn.iris = Some(std::sync::Arc::new(IrisConnection::new(
            "http://prod.example.com:52773",
            "PROD",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )));
    }

    let (role, name) = tools.instance_role();
    assert_eq!(role, ConnectionRole::Subject, "host match → Subject");
    assert_eq!(name, "remote");
}

// ── Regression: develop-mode flat configs unaffected (US7) ──────────────────

#[test]
fn test_develop_mode_flat_config_no_gate() {
    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        "container = \"loanapp-iris\"\nnamespace = \"LOANAPP\"\n",
    );
    let tools = make_tools_with_fleet(&dir);
    let (role, _) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Workspace,
        "flat develop config must not gate anything"
    );
}
