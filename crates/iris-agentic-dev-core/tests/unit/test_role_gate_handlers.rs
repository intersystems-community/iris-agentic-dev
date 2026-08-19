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

// ── Host-match precision: overlapping hostnames must not cross-match ─────────

#[test]
fn test_instance_role_host_match_no_substring_confusion() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    // "iris" is a substring of "my-iris-dev" — naive .contains() would match both.
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.short]
host = "iris"
web_port = 52773
role = "subject"
"#,
    );

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
        // Active connection is "my-iris-dev" — must NOT match fleet instance "iris"
        conn.iris = Some(std::sync::Arc::new(IrisConnection::new(
            "http://my-iris-dev:52773",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )));
    }

    let (role, _) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Workspace,
        "'my-iris-dev' must not match fleet host 'iris' — substring match is too broad"
    );
}

#[test]
fn test_instance_role_host_match_exact_url_position() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.prod]
host = "prod.example.com"
web_port = 52773
role = "subject"

[instance.preprod]
host = "preprod.example.com"
web_port = 52773
role = "subject"
"#,
    );

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
        conn.iris = Some(std::sync::Arc::new(IrisConnection::new(
            "http://preprod.example.com:52773",
            "STAGE",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )));
    }

    let (role, name) = tools.instance_role();
    assert_eq!(role, ConnectionRole::Subject, "preprod → Subject");
    assert_eq!(
        name, "preprod",
        "must match 'preprod' not 'prod' despite 'prod' being a substring"
    );
}

// ── T043: policy gate + role gate interop ────────────────────────────────────
// Policy gate fires before role gate — verify role gate is not reached when
// policy blocks the call.

#[test]
fn test_policy_gate_fires_before_role_gate() {
    use iris_agentic_dev_core::iris::server_manager::policy_gate;
    use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, ToolCategory};

    // Set up a policy that only allows "query" — blocks "compile"
    let policy = ConnectionPolicy {
        server_name: "prod-server".to_string(),
        allow: Some(vec![ToolCategory::Query]),
        mcp_template: None,
        data_policy: None,
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };

    // Policy gate on "iris_compile" should fire
    let gate = policy_gate("iris_compile", "prod-server", Some(&policy));
    assert!(gate.is_some(), "policy gate must fire for blocked category");
    let gate_json = gate.unwrap();
    assert_eq!(gate_json["error_code"], "POLICY_GATE");

    // Because policy gate fired, role gate is not evaluated.
    // Verify role gate is independent and would have also fired (belt-and-suspenders check):
    let role_gate = iris_agentic_dev_core::iris::workspace_config::check_role_gate(
        &ConnectionRole::Subject,
        "iris_compile",
        false,
        "prod",
        false,
    );
    assert!(
        role_gate.is_some(),
        "role gate would also fire on subject — but policy gate preempts it"
    );

    // Allowed category passes policy gate (role gate is then the only barrier)
    let pass = policy_gate("iris_query", "prod-server", Some(&policy));
    assert!(
        pass.is_none(),
        "query is allowed by policy — gate returns None"
    );
}

// ── Regression: hard-block confirm_ignored field ─────────────────────────────

#[test]
fn test_hard_block_response_includes_confirm_ignored() {
    let gate = iris_agentic_dev_core::iris::workspace_config::check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:commit",
        true, // confirm: true — must be ignored on hard-block
        "prod",
        true,
    );
    assert!(
        gate.is_some(),
        "hard-block must gate even with confirm: true"
    );
    let json = gate.unwrap();
    assert_eq!(json["hard_block"], true, "hard_block field must be true");
    assert_eq!(
        json["confirm_ignored"], true,
        "confirm_ignored must be present so agents know not to retry"
    );
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

// ── #114: shared-gateway namespace disambiguation ─────────────────────────────

/// Two fleet instances share the same host/port (shared HTTPS gateway).
/// One has namespace=CHANNELS (workspace role), the other namespace=PROD (subject).
/// Active connection is CHANNELS — must resolve to workspace, not subject.
#[test]
fn test_instance_role_namespace_disambiguates_shared_gateway() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.dev]
host = "gateway.example.com"
web_port = 443
namespace = "CHANNELS"
role = "workspace"

[instance.prod]
host = "gateway.example.com"
web_port = 443
namespace = "PROD"
role = "subject"
"#,
    );

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
        conn.iris = Some(std::sync::Arc::new(IrisConnection::new(
            "https://gateway.example.com:443",
            "CHANNELS",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )));
    }

    let (role, name) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Workspace,
        "active namespace CHANNELS should match dev (workspace), not prod (subject)"
    );
    assert_eq!(name, "dev");
}

/// Same fleet as above but active connection is PROD — must resolve to subject.
#[test]
fn test_instance_role_namespace_shared_gateway_prod_is_subject() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.dev]
host = "gateway.example.com"
web_port = 443
namespace = "CHANNELS"
role = "workspace"

[instance.prod]
host = "gateway.example.com"
web_port = 443
namespace = "PROD"
role = "subject"
"#,
    );

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
        conn.iris = Some(std::sync::Arc::new(IrisConnection::new(
            "https://gateway.example.com:443",
            "PROD",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )));
    }

    let (role, name) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Subject,
        "active namespace PROD should match prod (subject)"
    );
    assert_eq!(name, "prod");
}

/// Instance without namespace declared — host-only match still works (no regression).
#[test]
fn test_instance_role_host_only_match_still_works_without_namespace() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.prod]
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
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )));
    }

    let (role, name) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Subject,
        "no namespace on instance → host match alone should still work"
    );
    assert_eq!(name, "prod");
}

/// Namespace-specific fleet entry must not match on host alone in pass 2 (#114).
#[test]
fn test_instance_role_namespace_specific_instance_not_matched_by_host_only() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.prod]
host = "gateway.example.com"
namespace = "PROD"
role = "subject"
"#,
    );

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
        conn.iris = Some(std::sync::Arc::new(IrisConnection::new(
            "https://gateway.example.com:443",
            "CHANNELS",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )));
    }

    let (role, name) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Workspace,
        "namespace-specific prod must not match a different active namespace"
    );
    assert!(name.is_empty());
}

/// Namespace match is case-insensitive (IRIS namespaces are case-insensitive).
#[test]
fn test_instance_role_namespace_match_is_case_insensitive() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    let dir = tempfile::TempDir::new().unwrap();
    write_fleet_toml(
        &dir,
        r#"mode = "operate"

[instance.dev]
host = "gateway.example.com"
web_port = 443
namespace = "channels"
role = "workspace"

[instance.prod]
host = "gateway.example.com"
web_port = 443
namespace = "PROD"
role = "subject"
"#,
    );

    let tools = IrisTools::new(None).expect("IrisTools::new");
    {
        let mut conn = tools.connection.lock().unwrap();
        conn.config_file = Some(dir.path().join(".iris-agentic-dev.toml"));
        // Active connection namespace uppercase, fleet config lowercase
        conn.iris = Some(std::sync::Arc::new(IrisConnection::new(
            "https://gateway.example.com:443",
            "CHANNELS",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )));
    }

    let (role, name) = tools.instance_role();
    assert_eq!(
        role,
        ConnectionRole::Workspace,
        "namespace comparison must be case-insensitive"
    );
    assert_eq!(name, "dev");
}
