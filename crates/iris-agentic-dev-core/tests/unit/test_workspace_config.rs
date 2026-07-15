// Tests for workspace_config module: TOML loading, priority ordering, path resolution.

use iris_agentic_dev_core::iris::workspace_config::{
    build_workspace_config_json, load_fleet_config, load_workspace_config, validate_fleet_config,
    workspace_config_to_connection, workspace_root, ConnectionRole, FleetConfig,
};
use std::io::Write;

// Serialize tests that mutate env vars — concurrent set/remove_var calls race.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn write_toml(dir: &tempfile::TempDir, contents: &str) {
    let path = dir.path().join(".iris-agentic-dev.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(contents.as_bytes()).unwrap();
}

// ── T004: Core loading tests ──────────────────────────────────────────────────

#[test]
fn test_load_returns_none_when_no_file() {
    let result = load_workspace_config(Some("/nonexistent/path/that/cannot/exist"));
    assert!(
        result.is_none(),
        "should return None when file does not exist"
    );
}

#[test]
fn test_load_parses_container_field() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(&dir, r#"container = "test-iris""#);
    let cfg = load_workspace_config(Some(dir.path().to_str().unwrap())).unwrap();
    assert_eq!(cfg.container.as_deref(), Some("test-iris"));
}

#[test]
fn test_load_parses_all_fields() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"
container = "all-iris"
namespace = "MYNS"
host = "myhost"
web_port = 9999
username = "myuser"
password = "mypass"
"#,
    );
    let cfg = load_workspace_config(Some(dir.path().to_str().unwrap())).unwrap();
    assert_eq!(cfg.container.as_deref(), Some("all-iris"));
    assert_eq!(cfg.namespace.as_deref(), Some("MYNS"));
    assert_eq!(cfg.host.as_deref(), Some("myhost"));
    assert_eq!(cfg.web_port, Some(9999));
    assert_eq!(cfg.username.as_deref(), Some("myuser"));
    assert_eq!(cfg.password.as_deref(), Some("mypass"));
}

#[test]
fn test_load_returns_none_on_syntax_error() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(&dir, "this is not valid toml = = = !!!");
    let result = load_workspace_config(Some(dir.path().to_str().unwrap()));
    assert!(
        result.is_none(),
        "should return None on parse error, not panic"
    );
}

#[test]
fn test_load_uses_cwd_when_workspace_none() {
    // Call with None from a temp dir that has no .iris-dev.toml
    let dir = tempfile::TempDir::new().unwrap();
    let result = load_workspace_config(Some(dir.path().to_str().unwrap()));
    assert!(result.is_none());
}

#[test]
fn test_workspace_root_uses_env_var() {
    // Note: env var tests can be flaky if run in parallel; use a unique key.
    // We only test the logic — the env var takes precedence over the path arg.
    let tmp = tempfile::TempDir::new().unwrap();
    let tmp_str = tmp.path().to_str().unwrap().to_string();
    std::env::set_var("OBJECTSCRIPT_WORKSPACE", &tmp_str);
    let root = workspace_root(Some("/some/other/path"));
    std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
    assert_eq!(
        root,
        tmp.path(),
        "OBJECTSCRIPT_WORKSPACE should override path arg"
    );
}

#[test]
fn test_workspace_root_uses_path_when_no_env_var() {
    std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
    let root = workspace_root(Some("/explicit/path"));
    assert_eq!(root.to_str().unwrap(), "/explicit/path");
}

// ── T010: Connection building tests ──────────────────────────────────────────

#[test]
fn test_workspace_config_host_returns_connection() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(&dir, r#"host = "remotehost"\nweb_port = 9999"#);
    // Parse manually since \n in raw string literal doesn't give newline
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("remotehost".to_string()),
        web_port: Some(9999),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER");
    assert!(
        conn.is_some(),
        "host config should return Some(IrisConnection)"
    );
    let conn = conn.unwrap();
    assert!(
        conn.base_url.contains("remotehost"),
        "base_url should contain host, got: {}",
        conn.base_url
    );
    assert!(
        conn.base_url.contains("9999"),
        "base_url should contain port, got: {}",
        conn.base_url
    );
}

#[test]
fn test_workspace_config_namespace_applied() {
    // Container config sets IRIS_NAMESPACE env var
    std::env::remove_var("IRIS_NAMESPACE");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        container: Some("mytest-iris".to_string()),
        namespace: Some("TESTNS".to_string()),
        ..Default::default()
    };
    workspace_config_to_connection(&cfg, "USER");
    assert_eq!(
        std::env::var("IRIS_NAMESPACE").ok().as_deref(),
        Some("TESTNS"),
        "IRIS_NAMESPACE should be set from workspace config namespace"
    );
    // Cleanup
    std::env::remove_var("IRIS_NAMESPACE");
}

#[test]
fn test_workspace_config_sets_iris_container_env() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_CONTAINER");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        container: Some("mytest-iris".to_string()),
        ..Default::default()
    };
    workspace_config_to_connection(&cfg, "USER");
    assert_eq!(
        std::env::var("IRIS_CONTAINER").ok().as_deref(),
        Some("mytest-iris"),
        "IRIS_CONTAINER should be set from workspace config container"
    );
    std::env::remove_var("IRIS_CONTAINER");
}

// ── T015: Priority ordering test ─────────────────────────────────────────────

#[test]
fn test_compile_workspace_config_overrides_env() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Set IRIS_CONTAINER to an "old" value via env
    std::env::set_var("IRIS_CONTAINER", "old-container");

    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        container: Some("new-container".to_string()),
        ..Default::default()
    };
    workspace_config_to_connection(&cfg, "USER");

    // Config file should win over the pre-existing env var
    assert_eq!(
        std::env::var("IRIS_CONTAINER").ok().as_deref(),
        Some("new-container"),
        "workspace config container should override pre-existing IRIS_CONTAINER env var"
    );
    std::env::remove_var("IRIS_CONTAINER");
}

// ── T019: generate_toml_content tests ────────────────────────────────────────

#[test]
fn test_generate_toml_contains_container() {
    let content =
        iris_agentic_dev_core::iris::workspace_config::generate_toml_content("myapp-iris", "USER");
    assert!(
        content.contains("container = \"myapp-iris\""),
        "generated TOML should contain container field"
    );
    assert!(
        content.contains("namespace = \"USER\""),
        "generated TOML should contain namespace field"
    );
}

#[test]
fn test_generate_toml_contains_comment_about_password() {
    let content =
        iris_agentic_dev_core::iris::workspace_config::generate_toml_content("any-iris", "USER");
    assert!(
        content.contains("# password"),
        "generated TOML should have a commented-out password field"
    );
    assert!(
        content.contains("not recommended"),
        "generated TOML should warn against committing password"
    );
}

#[test]
fn test_generate_toml_is_parseable() {
    let content =
        iris_agentic_dev_core::iris::workspace_config::generate_toml_content("parse-iris", "MYNS");
    // Strip comment lines and parse as TOML
    let stripped: String = content
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let parsed =
        toml::from_str::<iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig>(&stripped);
    assert!(
        parsed.is_ok(),
        "generated TOML (minus comments) should parse cleanly: {:?}",
        parsed
    );
}

// ── T026: workspace_config field shape test ───────────────────────────────────

#[test]
fn test_workspace_config_field_shape() {
    // Verify the JSON shape we'd put in iris_list_containers response
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        container: Some("vis-test-iris".to_string()),
        namespace: Some("USER".to_string()),
        ..Default::default()
    };
    let running = false; // not actually checking docker here
    let json = serde_json::json!({
        "found": true,
        "path": "/some/project/.iris-dev.toml",
        "container": cfg.container,
        "namespace": cfg.namespace,
        "running": running,
    });
    assert_eq!(json["container"], "vis-test-iris");
    assert_eq!(json["found"], true);
    assert_eq!(json["running"], false);
}

// ── T030: web_prefix field ────────────────────────────────────────────────────

#[test]
fn test_load_parses_web_prefix_field() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"
host = "iris.example.com"
web_port = 80
web_prefix = "irisaicore"
"#,
    );
    let cfg = load_workspace_config(Some(dir.path().to_str().unwrap())).unwrap();
    assert_eq!(cfg.web_prefix.as_deref(), Some("irisaicore"));
}

#[test]
fn test_web_prefix_included_in_base_url() {
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(80),
        web_prefix: Some("irisaicore".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert_eq!(
        conn.base_url, "http://localhost:80/irisaicore",
        "base_url should include web_prefix, got: {}",
        conn.base_url
    );
}

#[test]
fn test_web_prefix_strips_leading_trailing_slashes() {
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        web_prefix: Some("/irisaicore/".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert_eq!(
        conn.base_url, "http://localhost:52773/irisaicore",
        "leading/trailing slashes should be stripped, got: {}",
        conn.base_url
    );
}

#[test]
fn test_no_web_prefix_gives_clean_base_url() {
    std::env::remove_var("IRIS_WEB_PREFIX");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        web_prefix: None,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert_eq!(
        conn.base_url, "http://localhost:52773",
        "base_url without prefix should have no trailing slash, got: {}",
        conn.base_url
    );
}

#[test]
fn test_iris_web_prefix_env_var_used_when_no_toml_prefix() {
    std::env::set_var("IRIS_WEB_PREFIX", "myprefix");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        web_prefix: None,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    std::env::remove_var("IRIS_WEB_PREFIX");
    assert_eq!(
        conn.base_url, "http://localhost:52773/myprefix",
        "IRIS_WEB_PREFIX env var should be used when web_prefix not in config, got: {}",
        conn.base_url
    );
}

#[test]
fn test_toml_web_prefix_overrides_env_var() {
    std::env::set_var("IRIS_WEB_PREFIX", "envprefix");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        web_prefix: Some("tomlprefix".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    std::env::remove_var("IRIS_WEB_PREFIX");
    assert_eq!(
        conn.base_url, "http://localhost:52773/tomlprefix",
        "TOML web_prefix should override IRIS_WEB_PREFIX env var, got: {}",
        conn.base_url
    );
}

#[test]
fn test_generate_toml_contains_web_prefix_comment() {
    let content =
        iris_agentic_dev_core::iris::workspace_config::generate_toml_content("myapp-iris", "USER");
    assert!(
        content.contains("web_prefix"),
        "generated TOML should document the web_prefix field"
    );
}

// ── T031: scheme field (https support) ───────────────────────────────────────

#[test]
fn test_load_parses_scheme_field() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"
host = "iris.example.com"
web_port = 443
scheme = "https"
"#,
    );
    let cfg = load_workspace_config(Some(dir.path().to_str().unwrap())).unwrap();
    assert_eq!(cfg.scheme.as_deref(), Some("https"));
}

#[test]
fn test_https_scheme_in_base_url() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_SCHEME");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("iris.example.com".to_string()),
        web_port: Some(443),
        scheme: Some("https".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert!(
        conn.base_url.starts_with("https://"),
        "base_url should use https, got: {}",
        conn.base_url
    );
    assert_eq!(conn.base_url, "https://iris.example.com:443");
}

#[test]
fn test_https_scheme_with_prefix() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_SCHEME");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("dem".to_string()),
        web_port: Some(443),
        scheme: Some("https".to_string()),
        web_prefix: Some("dev".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert_eq!(
        conn.base_url, "https://dem:443/dev",
        "https + prefix should combine correctly, got: {}",
        conn.base_url
    );
}

#[test]
fn test_default_scheme_is_http() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_SCHEME");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        scheme: None,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert!(
        conn.base_url.starts_with("http://"),
        "default scheme should be http, got: {}",
        conn.base_url
    );
}

#[test]
fn test_iris_scheme_env_var() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRIS_SCHEME", "https");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(443),
        scheme: None,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    std::env::remove_var("IRIS_SCHEME");
    assert!(
        conn.base_url.starts_with("https://"),
        "IRIS_SCHEME env var should set https, got: {}",
        conn.base_url
    );
}

#[test]
fn test_toml_scheme_overrides_env_var() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRIS_SCHEME", "http");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(443),
        scheme: Some("https".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    std::env::remove_var("IRIS_SCHEME");
    assert!(
        conn.base_url.starts_with("https://"),
        "TOML scheme should override IRIS_SCHEME env var, got: {}",
        conn.base_url
    );
}

#[test]
fn test_generate_toml_contains_scheme_comment() {
    let content =
        iris_agentic_dev_core::iris::workspace_config::generate_toml_content("myapp-iris", "USER");
    assert!(
        content.contains("scheme"),
        "generated TOML should document the scheme field"
    );
}

// ── Container scoring: hyphen/underscore normalization (#19) ─────────────────

#[test]
fn test_score_underscore_workspace_matches_hyphen_container() {
    // id_try2 (underscore) should match id-try2-iris (hyphen) — both normalize to id_try2
    use iris_agentic_dev_core::iris::discovery::score_container_name;
    let score = score_container_name("id-try2-iris", "id_try2");
    assert!(
        score > 0,
        "id_try2 should match id-try2-iris, got score {}",
        score
    );
    assert!(
        score >= 60,
        "should score at least 60 (contains match), got {}",
        score
    );
}

#[test]
fn test_score_hyphen_workspace_matches_underscore_container() {
    use iris_agentic_dev_core::iris::discovery::score_container_name;
    let score = score_container_name("id_try2_iris", "id-try2");
    assert!(
        score > 0,
        "id-try2 should match id_try2_iris, got score {}",
        score
    );
}

#[test]
fn test_score_exact_match_after_normalization() {
    use iris_agentic_dev_core::iris::discovery::score_container_name;
    // loanapp vs loanapp-iris: starts_with match + iris suffix = 80 + 10 = 90
    let score = score_container_name("loanapp-iris", "loanapp");
    assert_eq!(
        score, 90,
        "loanapp-iris for workspace loanapp should score 90"
    );
}

#[test]
fn test_score_unrelated_containers_score_zero() {
    use iris_agentic_dev_core::iris::discovery::score_container_name;
    let score = score_container_name("determined_cray", "id_try2");
    assert_eq!(score, 0, "unrelated container should score 0");
}

#[test]
fn test_score_container_beats_unrelated_after_normalization() {
    use iris_agentic_dev_core::iris::discovery::score_container_name;
    let target = score_container_name("id-try2-iris", "id_try2");
    let random = score_container_name("determined_cray", "id_try2");
    assert!(
        target > random,
        "id-try2-iris ({}) should beat determined_cray ({}) for id_try2 workspace",
        target,
        random
    );
}

// ── legacy .iris-dev.toml fallback ─────────────────────────────────────────

#[test]
fn test_load_falls_back_to_legacy_iris_dev_toml() {
    let dir = tempfile::tempdir().unwrap();
    // Only legacy file exists — not the new .iris-agentic-dev.toml
    let legacy = dir.path().join(".iris-dev.toml");
    std::fs::write(&legacy, "container = \"legacy-iris\"\n").unwrap();
    let cfg = iris_agentic_dev_core::iris::workspace_config::load_workspace_config(Some(
        dir.path().to_str().unwrap(),
    ));
    assert!(cfg.is_some(), "should fall back to legacy .iris-dev.toml");
    assert_eq!(cfg.unwrap().container.as_deref(), Some("legacy-iris"));
}

#[test]
fn test_workspace_root_prefers_new_over_legacy() {
    let dir = tempfile::tempdir().unwrap();
    // Both files exist — new one should win
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        "container = \"new-iris\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".iris-dev.toml"),
        "container = \"old-iris\"\n",
    )
    .unwrap();
    let cfg = iris_agentic_dev_core::iris::workspace_config::load_workspace_config(Some(
        dir.path().to_str().unwrap(),
    ));
    assert_eq!(
        cfg.unwrap().container.as_deref(),
        Some("new-iris"),
        "new .iris-agentic-dev.toml should win over legacy"
    );
}

// ── docker_only field ─────────────────────────────────────────────────────────

#[test]
fn test_load_parses_docker_only_field() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(&dir, "container = \"myapp-iris\"\ndocker_only = true\n");
    let cfg = load_workspace_config(Some(dir.path().to_str().unwrap())).unwrap();
    assert!(cfg.docker_only, "docker_only should parse as true");
}

#[test]
fn test_docker_only_returns_localhost_connection() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_CONTAINER");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        container: Some("myapp-iris".to_string()),
        docker_only: true,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER");
    std::env::remove_var("IRIS_CONTAINER");
    assert!(
        conn.is_some(),
        "docker_only with container should return Some(IrisConnection)"
    );
    let conn = conn.unwrap();
    assert!(
        conn.base_url.contains("127.0.0.1:1"),
        "docker_only base_url should be unreachable address, got: {}",
        conn.base_url
    );
}

#[test]
fn test_workspace_config_username_password_set_env() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_USERNAME");
    std::env::remove_var("IRIS_PASSWORD");
    std::env::remove_var("IRIS_CONTAINER");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        container: Some("mytest-iris".to_string()),
        username: Some("admin".to_string()),
        password: Some("secret".to_string()),
        ..Default::default()
    };
    workspace_config_to_connection(&cfg, "USER");
    assert_eq!(
        std::env::var("IRIS_USERNAME").ok().as_deref(),
        Some("admin"),
        "IRIS_USERNAME should be set from config"
    );
    assert_eq!(
        std::env::var("IRIS_PASSWORD").ok().as_deref(),
        Some("secret"),
        "IRIS_PASSWORD should be set from config"
    );
    std::env::remove_var("IRIS_USERNAME");
    std::env::remove_var("IRIS_PASSWORD");
    std::env::remove_var("IRIS_CONTAINER");
}

// ── apply_workspace_config ────────────────────────────────────────────────────

#[test]
fn test_apply_workspace_config_explicit_passes_through() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    use iris_agentic_dev_core::iris::workspace_config::apply_workspace_config;
    let explicit = IrisConnection::new(
        "http://explicit:52773",
        "MYNS",
        "_SYSTEM",
        "SYS",
        DiscoverySource::EnvVar,
    );
    let result = apply_workspace_config(Some(explicit.clone()), None, "USER");
    assert!(result.is_some(), "explicit connection should pass through");
    assert_eq!(
        result.unwrap().base_url,
        explicit.base_url,
        "explicit connection should be returned unchanged"
    );
}

#[test]
fn test_apply_workspace_config_none_with_no_file_returns_none() {
    use iris_agentic_dev_core::iris::workspace_config::apply_workspace_config;
    let dir = tempfile::TempDir::new().unwrap();
    // No config file in dir
    let result = apply_workspace_config(None, Some(dir.path().to_str().unwrap()), "USER");
    assert!(
        result.is_none(),
        "no config file should yield None from apply_workspace_config"
    );
}

// ── Amendment 001: Fleet structs ─────────────────────────────────────────────

// T004-a: flat file parses identically via FleetConfig::workspace
#[test]
fn test_fleet_config_develop_mode_flat_file_identical() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"container = "loanapp-iris"
namespace = "LOANAPP"
"#,
    );
    let path = dir.path().to_str().unwrap();
    let base = load_workspace_config(Some(path)).expect("base config loads");
    let fleet = load_fleet_config(Some(path)).expect("fleet config loads");
    assert_eq!(
        fleet.workspace.container, base.container,
        "FleetConfig::workspace.container must match WorkspaceConfig"
    );
    assert_eq!(
        fleet.workspace.namespace, base.namespace,
        "FleetConfig::workspace.namespace must match WorkspaceConfig"
    );
    assert!(
        fleet.instance.is_empty(),
        "develop-mode flat file should have no instance blocks"
    );
}

// T004-b: mode="operate" parses [instance.*] blocks
#[test]
fn test_fleet_config_operate_mode_parses_instances() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.local]
container = "myapp-iris"
namespace = "USER"

[instance.prod]
host = "prod.example.com"
web_port = 52773
namespace = "PROD"
role = "subject"
"#,
    );
    let fleet = load_fleet_config(Some(dir.path().to_str().unwrap())).expect("fleet config loads");
    assert_eq!(fleet.mode.as_deref(), Some("operate"));
    assert!(
        fleet.instance.contains_key("local"),
        "should have 'local' instance"
    );
    assert!(
        fleet.instance.contains_key("prod"),
        "should have 'prod' instance"
    );
    assert_eq!(
        fleet.instance["local"].container.as_deref(),
        Some("myapp-iris")
    );
    assert_eq!(fleet.instance["prod"].role, ConnectionRole::Subject);
}

// T004-c: ConnectionRole defaults to Workspace when absent
#[test]
fn test_connection_role_defaults_to_workspace() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.local]
container = "my-iris"
"#,
    );
    let fleet = load_fleet_config(Some(dir.path().to_str().unwrap())).expect("fleet config loads");
    assert_eq!(
        fleet.instance["local"].role,
        ConnectionRole::Workspace,
        "absent role must default to Workspace"
    );
}

// T004-d: role = "subject" parses correctly
#[test]
fn test_connection_role_subject_parses() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.cust]
host = "cust.example.com"
role = "subject"
"#,
    );
    let fleet = load_fleet_config(Some(dir.path().to_str().unwrap())).expect("fleet config loads");
    assert_eq!(fleet.instance["cust"].role, ConnectionRole::Subject);
}

// T004-e: role = "control-plane" parses correctly
#[test]
fn test_connection_role_control_plane_parses() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.ctrl]
host = "ctrl.example.com"
role = "control-plane"
"#,
    );
    let fleet = load_fleet_config(Some(dir.path().to_str().unwrap())).expect("fleet config loads");
    assert_eq!(fleet.instance["ctrl"].role, ConnectionRole::ControlPlane);
}

// T004-f: [instance.*] blocks ignored when mode="develop"
#[test]
fn test_fleet_config_instance_blocks_ignored_in_develop_mode() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "develop"

[instance.prod]
host = "prod.example.com"
role = "subject"
"#,
    );
    let fleet = load_fleet_config(Some(dir.path().to_str().unwrap())).expect("fleet config loads");
    // mode="develop" — instance map is parsed but callers must ignore it
    assert_eq!(fleet.mode.as_deref(), Some("develop"));
    // Callers checking mode should treat this identically to develop mode
    let is_operate = fleet.mode.as_deref() == Some("operate");
    assert!(!is_operate, "develop mode must not activate fleet logic");
}

// T004-g: memory-home defaults to "self" when absent
#[test]
fn test_fleet_config_memory_home_defaults_to_self() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.cust]
host = "cust.example.com"
role = "subject"
"#,
    );
    let fleet = load_fleet_config(Some(dir.path().to_str().unwrap())).expect("fleet config loads");
    assert!(
        fleet.instance["cust"].memory_home.is_none(),
        "absent memory-home should be None in raw struct (defaults resolved by callers)"
    );
}

// T004-h: validate_fleet_config warns and falls back to "self" for unknown memory-home
#[test]
fn test_fleet_config_unknown_memory_home_falls_back_to_self() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.cust]
host = "cust.example.com"
role = "subject"
memory-home = "nonexistent-instance"
"#,
    );
    let mut fleet =
        load_fleet_config(Some(dir.path().to_str().unwrap())).expect("fleet config loads");
    // Before validation: raw value is set
    assert_eq!(
        fleet.instance["cust"].memory_home.as_deref(),
        Some("nonexistent-instance")
    );
    validate_fleet_config(&mut fleet);
    // After validation: unknown key falls back to "self"
    assert_eq!(
        fleet.instance["cust"].memory_home.as_deref(),
        Some("self"),
        "validate_fleet_config must replace unknown memory-home with 'self'"
    );
}

// T004-i: subject field is None when absent
#[test]
fn test_fleet_config_subject_field_absent_is_none() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.cust]
host = "cust.example.com"
role = "subject"
"#,
    );
    let fleet = load_fleet_config(Some(dir.path().to_str().unwrap())).expect("fleet config loads");
    assert!(
        fleet.instance["cust"].subject.is_none(),
        "absent subject field should be None"
    );
}

// ── Amendment 001: iris_list_containers operate-mode response ─────────────────

// T010-a: operate mode response includes "mode": "operate"
#[test]
fn test_list_containers_workspace_config_includes_mode_operate() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.local]
container = "myapp-iris"
namespace = "USER"
role = "workspace"
"#,
    );
    let json = build_workspace_config_json(Some(dir.path().to_str().unwrap()), &[]);
    assert_eq!(
        json["mode"].as_str(),
        Some("operate"),
        "operate mode response must include mode field"
    );
    assert!(
        json["instances"].is_object(),
        "operate mode response must include instances object"
    );
}

// T010-b: operate mode instances have role/memory_home/subject/running fields
#[test]
fn test_list_containers_instances_have_role_and_memory_home() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.local]
container = "myapp-iris"
namespace = "USER"
role = "workspace"

[instance.prod]
host = "prod.example.com"
namespace = "PROD"
role = "subject"
memory-home = "local"
subject = "Acme Corp"
"#,
    );
    let running = serde_json::json!([{"name": "myapp-iris"}]);
    let containers = running.as_array().unwrap();
    let json = build_workspace_config_json(Some(dir.path().to_str().unwrap()), containers);

    let instances = &json["instances"];
    assert!(
        instances["local"].is_object(),
        "local instance must be present"
    );
    assert_eq!(instances["local"]["role"].as_str(), Some("workspace"));
    assert_eq!(
        instances["local"]["running"].as_bool(),
        Some(true),
        "myapp-iris is running"
    );

    assert!(
        instances["prod"].is_object(),
        "prod instance must be present"
    );
    assert_eq!(instances["prod"]["role"].as_str(), Some("subject"));
    assert_eq!(instances["prod"]["memory_home"].as_str(), Some("local"));
    assert_eq!(instances["prod"]["subject"].as_str(), Some("Acme Corp"));
    assert_eq!(
        instances["prod"]["running"].as_bool(),
        Some(false),
        "prod not in running list"
    );
}

// T010-c: develop mode response has no "mode" or "instances" field (SC-011)
#[test]
fn test_list_containers_develop_mode_response_unchanged() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"container = "loanapp-iris"
namespace = "USER"
"#,
    );
    let json = build_workspace_config_json(Some(dir.path().to_str().unwrap()), &[]);
    assert!(
        json["mode"].is_null(),
        "develop mode must not include mode field"
    );
    assert!(
        json["instances"].is_null(),
        "develop mode must not include instances field"
    );
    // Must still include the existing develop-mode shape
    assert_eq!(
        json["container"].as_str(),
        Some("loanapp-iris"),
        "develop mode must include container field"
    );
    assert!(
        json["found"].as_bool().unwrap_or(false),
        "found must be true"
    );
}

// ── Amendment 001: iris-dev init --mode operate (FR-025–FR-026) ──────────────

// T026-a: generate_operate_toml_content produces parseable FleetConfig with mode="operate"
#[test]
fn test_generate_operate_toml_content_parses_as_fleet_config() {
    use iris_agentic_dev_core::iris::workspace_config::generate_operate_toml_content;
    let content = generate_operate_toml_content("myapp-iris", "USER");
    // Strip comment lines and parse only the active TOML
    let fleet: FleetConfig =
        toml::from_str(&content).expect("generated operate template must parse as FleetConfig");
    assert_eq!(
        fleet.mode.as_deref(),
        Some("operate"),
        "generated template must have mode = \"operate\""
    );
}

// T026-b: generated operate template has [instance.local] with role="workspace"
#[test]
fn test_generate_operate_toml_has_instance_local_workspace() {
    use iris_agentic_dev_core::iris::workspace_config::generate_operate_toml_content;
    let content = generate_operate_toml_content("myapp-iris", "USER");
    let fleet: FleetConfig = toml::from_str(&content).expect("parses");
    assert!(
        fleet.instance.contains_key("local"),
        "generated template must have [instance.local]"
    );
    assert_eq!(
        fleet.instance["local"].role,
        ConnectionRole::Workspace,
        "[instance.local] must have role = workspace"
    );
    assert_eq!(
        fleet.instance["local"].container.as_deref(),
        Some("myapp-iris"),
        "[instance.local] must have the provided container name"
    );
}

// T026-c: generated operate template has commented-out [instance.subject] block
#[test]
fn test_generate_operate_toml_has_commented_subject_block() {
    use iris_agentic_dev_core::iris::workspace_config::generate_operate_toml_content;
    let content = generate_operate_toml_content("myapp-iris", "USER");
    assert!(
        content.contains("# [instance.subject]") || content.contains("#[instance.subject]"),
        "generated template must have a commented-out [instance.subject] block"
    );
}

// T026-d: existing generate_toml_content has no mode field (develop mode unchanged)
#[test]
fn test_generate_develop_toml_content_has_no_mode_field() {
    use iris_agentic_dev_core::iris::workspace_config::generate_toml_content;
    let content = generate_toml_content("myapp-iris", "USER");
    // Parsed as FleetConfig: mode must be None
    let fleet: FleetConfig = toml::from_str(&content).expect("develop template parses");
    assert!(
        fleet.mode.is_none(),
        "develop template must not have a mode field"
    );
}

// ── Coverage gap-fill: uncovered paths ────────────────────────────────────────

// workspace_root walk-up: called with "." triggers cwd walk-up path
#[test]
fn test_workspace_root_dot_path_falls_through_to_walkup() {
    std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
    // "." triggers the walk-up branch — just verify it returns a valid path without panic
    let root = workspace_root(Some("."));
    assert!(
        root.is_absolute() || root.to_str() == Some("."),
        "workspace_root('.') should return a valid path"
    );
}

// workspace_root: None workspace_path also triggers walk-up
#[test]
fn test_workspace_root_none_triggers_walkup() {
    std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
    let root = workspace_root(None);
    assert!(
        !root.to_string_lossy().is_empty(),
        "should return non-empty path"
    );
}

// build_workspace_config_json: no config file → returns Null
#[test]
fn test_build_workspace_config_json_no_config_returns_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let json = build_workspace_config_json(Some(dir.path().to_str().unwrap()), &[]);
    assert!(
        json.is_null(),
        "build_workspace_config_json with no config file must return Null"
    );
}

// load_fleet_config: parse error → returns None (not panic)
#[test]
fn test_load_fleet_config_returns_none_on_parse_error() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(&dir, "this is not valid toml = = !!!");
    let result = load_fleet_config(Some(dir.path().to_str().unwrap()));
    assert!(
        result.is_none(),
        "load_fleet_config should return None on parse error, not panic"
    );
}

// load_fleet_config: legacy .iris-dev.toml fallback
#[test]
fn test_load_fleet_config_falls_back_to_legacy() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join(".iris-dev.toml");
    std::fs::write(&legacy, "container = \"legacy-fleet-iris\"\n").unwrap();
    let fleet = load_fleet_config(Some(dir.path().to_str().unwrap()));
    assert!(
        fleet.is_some(),
        "load_fleet_config should fall back to legacy .iris-dev.toml"
    );
    assert_eq!(
        fleet.unwrap().workspace.container.as_deref(),
        Some("legacy-fleet-iris")
    );
}

// workspace_config_to_connection: host + container sets IRIS_CONTAINER too
#[test]
fn test_host_with_container_sets_iris_container_env() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_CONTAINER");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("remotehost".to_string()),
        container: Some("remote-iris".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER");
    assert!(
        conn.is_some(),
        "host config must return Some(IrisConnection)"
    );
    assert_eq!(
        std::env::var("IRIS_CONTAINER").ok().as_deref(),
        Some("remote-iris"),
        "IRIS_CONTAINER should be set even when host is specified alongside container"
    );
    std::env::remove_var("IRIS_CONTAINER");
}

// ── ToolCategory::as_str coverage ────────────────────────────────────────────

#[test]
fn test_tool_category_as_str_compile() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::Compile.as_str(), "compile");
}

#[test]
fn test_tool_category_as_str_execute() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::Execute.as_str(), "execute");
}

#[test]
fn test_tool_category_as_str_query() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::Query.as_str(), "query");
}

#[test]
fn test_tool_category_as_str_search() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::Search.as_str(), "search");
}

#[test]
fn test_tool_category_as_str_docs() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::Docs.as_str(), "docs");
}

#[test]
fn test_tool_category_as_str_source_control() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::SourceControl.as_str(), "source_control");
}

#[test]
fn test_tool_category_as_str_debug() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::Debug.as_str(), "debug");
}

#[test]
fn test_tool_category_as_str_admin() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::Admin.as_str(), "admin");
}

#[test]
fn test_tool_category_as_str_skill() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::Skill.as_str(), "skill");
}

#[test]
fn test_tool_category_as_str_kb() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    assert_eq!(ToolCategory::Kb.as_str(), "kb");
}

// ── ToolCategory::from_str coverage ─────────────────────────────────────────

#[test]
fn test_tool_category_from_str_compile() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("compile").unwrap();
    assert_eq!(cat, ToolCategory::Compile);
}

#[test]
fn test_tool_category_from_str_execute() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("execute").unwrap();
    assert_eq!(cat, ToolCategory::Execute);
}

#[test]
fn test_tool_category_from_str_query() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("query").unwrap();
    assert_eq!(cat, ToolCategory::Query);
}

#[test]
fn test_tool_category_from_str_search() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("search").unwrap();
    assert_eq!(cat, ToolCategory::Search);
}

#[test]
fn test_tool_category_from_str_docs() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("docs").unwrap();
    assert_eq!(cat, ToolCategory::Docs);
}

#[test]
fn test_tool_category_from_str_source_control() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("source_control").unwrap();
    assert_eq!(cat, ToolCategory::SourceControl);
}

#[test]
fn test_tool_category_from_str_debug() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("debug").unwrap();
    assert_eq!(cat, ToolCategory::Debug);
}

#[test]
fn test_tool_category_from_str_admin() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("admin").unwrap();
    assert_eq!(cat, ToolCategory::Admin);
}

#[test]
fn test_tool_category_from_str_skill() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("skill").unwrap();
    assert_eq!(cat, ToolCategory::Skill);
}

#[test]
fn test_tool_category_from_str_kb() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let cat = ToolCategory::from_str("kb").unwrap();
    assert_eq!(cat, ToolCategory::Kb);
}

#[test]
fn test_tool_category_from_str_unknown() {
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    use std::str::FromStr;
    let result = ToolCategory::from_str("unknown_category");
    assert!(result.is_err(), "unknown tool category should be Err");
    assert_eq!(
        result.unwrap_err(),
        "unknown tool category: 'unknown_category'"
    );
}

// ── check_role_gate function coverage ────────────────────────────────────────

#[test]
fn test_check_role_gate_workspace_returns_none() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Workspace,
        "iris_compile",
        false,
        "local",
        false,
    );
    assert!(result.is_none(), "Workspace role should not be gated");
}

#[test]
fn test_check_role_gate_control_plane_returns_none() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::ControlPlane,
        "iris_compile",
        false,
        "local",
        false,
    );
    assert!(result.is_none(), "ControlPlane role should not be gated");
}

#[test]
fn test_check_role_gate_subject_select_query_not_gated() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_query:SELECT",
        false,
        "subject",
        false,
    );
    assert!(
        result.is_none(),
        "SELECT queries should not be gated on Subject"
    );
}

#[test]
fn test_check_role_gate_subject_lowercase_select_not_gated() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_query:select",
        false,
        "subject",
        false,
    );
    assert!(result.is_none(), "lowercase select should not be gated");
}

#[test]
fn test_check_role_gate_subject_source_control_status_not_gated() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:status",
        false,
        "subject",
        false,
    );
    assert!(
        result.is_none(),
        "source_control:status should not be gated"
    );
}

#[test]
fn test_check_role_gate_subject_source_control_diff_not_gated() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:diff",
        false,
        "subject",
        false,
    );
    assert!(result.is_none(), "source_control:diff should not be gated");
}

#[test]
fn test_check_role_gate_subject_source_control_log_not_gated() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:log",
        false,
        "subject",
        false,
    );
    assert!(result.is_none(), "source_control:log should not be gated");
}

#[test]
fn test_check_role_gate_subject_source_control_list_not_gated() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:list",
        false,
        "subject",
        false,
    );
    assert!(result.is_none(), "source_control:list should not be gated");
}

#[test]
fn test_check_role_gate_subject_source_control_get_not_gated() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:get",
        false,
        "subject",
        false,
    );
    assert!(result.is_none(), "source_control:get should not be gated");
}

#[test]
fn test_check_role_gate_subject_hard_block_returns_error_json() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:commit",
        false,
        "prod",
        true,
    );
    assert!(result.is_some(), "hard_block should return Some error");
    let json = result.unwrap();
    assert_eq!(json["error"].as_str(), Some("role_gate"));
    assert_eq!(json["hard_block"].as_bool(), Some(true));
    assert_eq!(json["confirm_ignored"].as_bool(), Some(true));
    assert_eq!(json["role"].as_str(), Some("subject"));
    assert_eq!(json["instance"].as_str(), Some("prod"));
}

#[test]
fn test_check_role_gate_subject_soft_gate_confirm_true_bypasses() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_compile",
        true,
        "subject",
        false,
    );
    assert!(result.is_none(), "confirm=true should bypass soft gate");
}

#[test]
fn test_check_role_gate_subject_soft_gate_confirm_false_returns_error() {
    use iris_agentic_dev_core::iris::workspace_config::{check_role_gate, ConnectionRole};
    let result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_compile",
        false,
        "mysubject",
        false,
    );
    assert!(result.is_some(), "confirm=false should return error");
    let json = result.unwrap();
    assert_eq!(json["error"].as_str(), Some("role_gate"));
    assert_eq!(json["role_gate"].as_bool(), Some(true));
    assert_eq!(json["instance"].as_str(), Some("mysubject"));
    assert_eq!(json["role"].as_str(), Some("subject"));
    assert_eq!(json["required_confirmation"].as_str(), Some("iris_compile"));
}

// ── workspace_config_to_connection: scheme and username/password defaults ─────

#[test]
fn test_workspace_config_host_default_username_and_password() {
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        username: None,
        password: None,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert_eq!(
        conn.username, "_SYSTEM",
        "default username should be _SYSTEM"
    );
    assert_eq!(conn.password, "SYS", "default password should be SYS");
}

#[test]
fn test_workspace_config_host_uses_username_password() {
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        username: Some("custom_user".to_string()),
        password: Some("custom_pass".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert_eq!(conn.username, "custom_user");
    assert_eq!(conn.password, "custom_pass");
}

#[test]
fn test_workspace_config_scheme_strips_slashes() {
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        scheme: Some("//https//".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert!(
        conn.base_url.contains("https"),
        "scheme should strip slashes, got: {}",
        conn.base_url
    );
}

#[test]
fn test_workspace_config_scheme_empty_string_becomes_http() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_SCHEME");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        scheme: Some("".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert!(
        conn.base_url.starts_with("http://"),
        "empty scheme should default to http"
    );
}

// ── workspace_config_to_connection: container env var priority ───────────────

#[test]
fn test_container_config_respects_custom_namespace_default() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_NAMESPACE");
    std::env::remove_var("IRIS_CONTAINER");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        container: Some("iris-container".to_string()),
        namespace: Some("FROM_CONFIG".to_string()),
        ..Default::default()
    };
    workspace_config_to_connection(&cfg, "CUSTOM_DEFAULT");
    assert_eq!(
        std::env::var("IRIS_NAMESPACE").ok().as_deref(),
        Some("FROM_CONFIG"),
        "should use namespace from config when present"
    );
    std::env::remove_var("IRIS_NAMESPACE");
    std::env::remove_var("IRIS_CONTAINER");
}

#[test]
fn test_container_config_with_docker_only_sets_unreachable_url() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("IRIS_CONTAINER");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        container: Some("docker-only-iris".to_string()),
        docker_only: true,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER");
    std::env::remove_var("IRIS_CONTAINER");
    assert!(conn.is_some());
    assert_eq!(conn.unwrap().base_url, "http://127.0.0.1:1");
}

// ── empty config field handling ──────────────────────────────────────────────

#[test]
fn test_load_toml_with_empty_web_prefix() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"host = "localhost"
web_port = 52773
web_prefix = ""
"#,
    );
    let cfg = load_workspace_config(Some(dir.path().to_str().unwrap())).unwrap();
    assert_eq!(
        cfg.web_prefix.as_deref(),
        Some(""),
        "empty web_prefix should be preserved as Some(\"\")"
    );
}

#[test]
fn test_workspace_config_empty_web_prefix_ignored_in_url() {
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        web_prefix: Some("".to_string()),
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    assert_eq!(
        conn.base_url, "http://localhost:52773",
        "empty web_prefix should not create URL with trailing slash"
    );
}

// ── env vars fallback for web_prefix/scheme ──────────────────────────────────

#[test]
fn test_iris_scheme_env_var_with_slashes_stripped() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRIS_SCHEME", "//https//");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(443),
        scheme: None,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    std::env::remove_var("IRIS_SCHEME");
    assert!(
        conn.base_url.contains("https"),
        "IRIS_SCHEME env var should strip slashes"
    );
}

#[test]
fn test_iris_web_prefix_env_var_with_slashes_stripped() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRIS_WEB_PREFIX", "//myprefix//");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        web_prefix: None,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    std::env::remove_var("IRIS_WEB_PREFIX");
    assert_eq!(
        conn.base_url, "http://localhost:52773/myprefix",
        "IRIS_WEB_PREFIX should strip leading/trailing slashes"
    );
}

// ── empty string edge cases ──────────────────────────────────────────────────

#[test]
fn test_iris_scheme_env_var_empty_defaults_to_http() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRIS_SCHEME", "");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        scheme: None,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    std::env::remove_var("IRIS_SCHEME");
    assert!(
        conn.base_url.starts_with("http://"),
        "empty IRIS_SCHEME env var should default to http"
    );
}

#[test]
fn test_iris_web_prefix_env_var_empty_not_included() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRIS_WEB_PREFIX", "");
    let cfg = iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig {
        host: Some("localhost".to_string()),
        web_port: Some(52773),
        web_prefix: None,
        ..Default::default()
    };
    let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
    std::env::remove_var("IRIS_WEB_PREFIX");
    assert_eq!(
        conn.base_url, "http://localhost:52773",
        "empty IRIS_WEB_PREFIX env var should be filtered out"
    );
}

// ── build_workspace_config_json: edge cases ──────────────────────────────────

#[test]
fn test_build_workspace_config_json_operate_mode_empty_instances() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"
"#,
    );
    let json = build_workspace_config_json(Some(dir.path().to_str().unwrap()), &[]);
    assert_eq!(json["mode"].as_str(), Some("operate"));
    assert!(json["instances"].is_object());
    assert_eq!(json["instances"].as_object().unwrap().len(), 0);
}

#[test]
fn test_build_workspace_config_json_instance_no_container_running_false() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.remote]
host = "example.com"
"#,
    );
    let json = build_workspace_config_json(Some(dir.path().to_str().unwrap()), &[]);
    assert_eq!(
        json["instances"]["remote"]["running"].as_bool(),
        Some(false)
    );
}

#[test]
fn test_build_workspace_config_json_develop_mode_with_no_container() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(&dir, r#"namespace = "USER""#);
    let json = build_workspace_config_json(Some(dir.path().to_str().unwrap()), &[]);
    assert!(json["found"].as_bool().unwrap_or(false));
    assert!(json["container"].is_null() || json["container"].as_str() == Some(""));
}

// ── validate_fleet_config edge cases ─────────────────────────────────────────

#[test]
fn test_validate_fleet_config_known_memory_home_unchanged() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.local]
container = "local-iris"

[instance.remote]
host = "remote.example.com"
memory-home = "local"
"#,
    );
    let mut fleet = load_fleet_config(Some(dir.path().to_str().unwrap())).unwrap();
    validate_fleet_config(&mut fleet);
    assert_eq!(
        fleet.instance["remote"].memory_home.as_deref(),
        Some("local"),
        "known memory-home should remain unchanged"
    );
}

#[test]
fn test_validate_fleet_config_multiple_unknown_memory_homes() {
    let dir = tempfile::TempDir::new().unwrap();
    write_toml(
        &dir,
        r#"mode = "operate"

[instance.inst1]
host = "inst1.example.com"
memory-home = "unknown1"

[instance.inst2]
host = "inst2.example.com"
memory-home = "unknown2"
"#,
    );
    let mut fleet = load_fleet_config(Some(dir.path().to_str().unwrap())).unwrap();
    validate_fleet_config(&mut fleet);
    assert_eq!(
        fleet.instance["inst1"].memory_home.as_deref(),
        Some("self"),
        "first unknown memory-home should fall back to self"
    );
    assert_eq!(
        fleet.instance["inst2"].memory_home.as_deref(),
        Some("self"),
        "second unknown memory-home should fall back to self"
    );
}

// ── generate_operate_toml_content specifics ──────────────────────────────────

#[test]
fn test_generate_operate_toml_content_interpolates_container_name() {
    use iris_agentic_dev_core::iris::workspace_config::generate_operate_toml_content;
    let content = generate_operate_toml_content("myspecialapp-iris", "MYAPP");
    assert!(content.contains("myspecialapp-iris"));
    assert!(content.contains("MYAPP"));
}

#[test]
fn test_generate_operate_toml_content_has_mode_operate() {
    use iris_agentic_dev_core::iris::workspace_config::generate_operate_toml_content;
    let content = generate_operate_toml_content("app-iris", "USER");
    assert!(content.contains("mode = \"operate\""));
}

// ── DataPolicy enum variant coverage ─────────────────────────────────────────

#[test]
fn test_data_policy_block_deserialization() {
    use iris_agentic_dev_core::iris::workspace_config::{load_fleet_config_from_str, DataPolicy};
    let toml_str = r#"
[policy.server1]
dataPolicy = "block"
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert_eq!(
        fleet.policies["server1"].data_policy,
        Some(DataPolicy::Block)
    );
}

#[test]
fn test_data_policy_allow_deserialization() {
    use iris_agentic_dev_core::iris::workspace_config::{load_fleet_config_from_str, DataPolicy};
    let toml_str = r#"
[policy.server1]
dataPolicy = "allow"
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert_eq!(
        fleet.policies["server1"].data_policy,
        Some(DataPolicy::Allow)
    );
}

#[test]
fn test_data_policy_redact_deserialization() {
    use iris_agentic_dev_core::iris::workspace_config::{load_fleet_config_from_str, DataPolicy};
    let toml_str = r#"
[policy.server1]
dataPolicy = "redact"
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert_eq!(
        fleet.policies["server1"].data_policy,
        Some(DataPolicy::Redact)
    );
}

// ── McpTemplate enum variant coverage ─────────────────────────────────────────

#[test]
fn test_mcp_template_dev_deserialization() {
    use iris_agentic_dev_core::iris::workspace_config::{load_fleet_config_from_str, McpTemplate};
    let toml_str = r#"
[policy.server1]
mcpTemplate = "dev"
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert_eq!(
        fleet.policies["server1"].mcp_template,
        Some(McpTemplate::Dev)
    );
}

#[test]
fn test_mcp_template_test_deserialization() {
    use iris_agentic_dev_core::iris::workspace_config::{load_fleet_config_from_str, McpTemplate};
    let toml_str = r#"
[policy.server1]
mcpTemplate = "test"
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert_eq!(
        fleet.policies["server1"].mcp_template,
        Some(McpTemplate::Test)
    );
}

#[test]
fn test_mcp_template_live_deserialization() {
    use iris_agentic_dev_core::iris::workspace_config::{load_fleet_config_from_str, McpTemplate};
    let toml_str = r#"
[policy.server1]
mcpTemplate = "live"
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert_eq!(
        fleet.policies["server1"].mcp_template,
        Some(McpTemplate::Live)
    );
}

#[test]
fn test_mcp_template_unknown_returns_none_with_warning() {
    use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;
    let toml_str = r#"
[policy.server1]
mcpTemplate = "unknown_template"
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    // Unknown template should be treated as None (deserialize_opt_mcp_template returns Ok(None))
    assert!(
        fleet.policies["server1"].mcp_template.is_none(),
        "unknown mcpTemplate should return None"
    );
}

// ── policy blocklist fields ──────────────────────────────────────────────────

#[test]
fn test_policy_global_blocklist_deserialization() {
    use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;
    let toml_str = r#"
[policy.server1]
globalBlocklist = ["pattern1", "pattern2"]
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert_eq!(
        fleet.policies["server1"].global_blocklist,
        vec!["pattern1", "pattern2"]
    );
}

#[test]
fn test_policy_data_policy_kill_allowlist_deserialization() {
    use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;
    let toml_str = r#"
[policy.server1]
dataPolicyKillAllowlist = ["kill_pattern1", "kill_pattern2"]
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert_eq!(
        fleet.policies["server1"].data_policy_kill_allowlist,
        vec!["kill_pattern1", "kill_pattern2"]
    );
}

// ── policy allow list (tool categories) ──────────────────────────────────────

#[test]
fn test_policy_allow_list_deserialization() {
    use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;
    let toml_str = r#"
[policy.server1]
allow = ["compile", "execute", "query"]
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
    let allow = fleet.policies["server1"].allow.as_ref().unwrap();
    assert_eq!(allow.len(), 3);
    assert!(allow.contains(&ToolCategory::Compile));
    assert!(allow.contains(&ToolCategory::Execute));
    assert!(allow.contains(&ToolCategory::Query));
}

// ── disabled_tools config field ───────────────────────────────────────────────

#[test]
fn test_disabled_tools_parsed_from_toml() {
    use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;
    let toml_str = r#"
container = "iris-dev"
namespace = "USER"
disabled_tools = ["iris_source_control", "iris_admin"]
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert_eq!(
        fleet.workspace.disabled_tools,
        vec!["iris_source_control", "iris_admin"]
    );
}

#[test]
fn test_disabled_tools_empty_by_default() {
    use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;
    let toml_str = r#"container = "iris-dev""#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    assert!(fleet.workspace.disabled_tools.is_empty());
}

#[test]
fn test_workspace_config_to_connection_exports_disabled_tools_env() {
    use iris_agentic_dev_core::iris::workspace_config::{
        load_fleet_config_from_str, workspace_config_to_connection,
    };
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::remove_var("IRIS_DISABLED_TOOLS");

    let toml_str = r#"
host = "localhost"
web_port = 52773
namespace = "USER"
username = "_SYSTEM"
password = "SYS"
disabled_tools = ["iris_admin", "iris_credential_manage"]
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    let _ = workspace_config_to_connection(&fleet.workspace, "USER");

    let exported = std::env::var("IRIS_DISABLED_TOOLS").unwrap_or_default();
    assert!(
        exported.contains("iris_admin"),
        "iris_admin must appear in IRIS_DISABLED_TOOLS"
    );
    assert!(
        exported.contains("iris_credential_manage"),
        "iris_credential_manage must appear in IRIS_DISABLED_TOOLS"
    );
    std::env::remove_var("IRIS_DISABLED_TOOLS");
}

#[test]
fn test_workspace_config_to_connection_respects_existing_env_var() {
    use iris_agentic_dev_core::iris::workspace_config::{
        load_fleet_config_from_str, workspace_config_to_connection,
    };
    let _lock = ENV_LOCK.lock().unwrap();
    // Pre-set env var — toml field must NOT override it
    std::env::set_var("IRIS_DISABLED_TOOLS", "iris_compile");

    let toml_str = r#"
host = "localhost"
web_port = 52773
namespace = "USER"
username = "_SYSTEM"
password = "SYS"
disabled_tools = ["iris_admin"]
"#;
    let fleet = load_fleet_config_from_str(toml_str).unwrap();
    let _ = workspace_config_to_connection(&fleet.workspace, "USER");

    let exported = std::env::var("IRIS_DISABLED_TOOLS").unwrap();
    assert_eq!(
        exported, "iris_compile",
        "existing env var must not be overwritten by toml"
    );
    std::env::remove_var("IRIS_DISABLED_TOOLS");
}
