//! Tests for the config paths that had no test: loading a config file that is legacy,
//! malformed, or absent, and the `docker_only`-without-`container` routing added in a720d2f.
//!
//! Configs are parsed from TOML text rather than built as struct literals, so a field that
//! disappears from the struct fails here instead of being silently ignored.
//!
//! `OBJECTSCRIPT_WORKSPACE` outranks the `workspace_path` argument, so the tests that load
//! from disk clear it and serialize on one lock.

use iris_agentic_dev_core::iris::connection::DiscoverySource;
use iris_agentic_dev_core::iris::workspace_config::{
    load_workspace_config, load_workspace_config_with_path, workspace_config_to_connection,
    WorkspaceConfig,
};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Serializes the tests that read or write process environment, and restores what it found.
struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

const GUARDED: &[&str] = &[
    "OBJECTSCRIPT_WORKSPACE",
    "IRIS_CONTAINER",
    "IRIS_NAMESPACE",
    "IRIS_USERNAME",
    "IRIS_PASSWORD",
];

impl EnvGuard {
    fn new() -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let saved = GUARDED
            .iter()
            .map(|k| (*k, std::env::var(k).ok()))
            .collect();
        for k in GUARDED {
            std::env::remove_var(k);
        }
        EnvGuard { _lock: lock, saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in &self.saved {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }
}

fn parse(toml_text: &str) -> WorkspaceConfig {
    toml::from_str(toml_text).expect("config must parse")
}

// ── docker_only / nopws without a container ──────────────────────────────────────────────

/// `docker_only = true` with no `container` used to fall through to `None`, which dropped the
/// caller into the discovery cascade and an Atelier attempt. The sentinel base URL exists so
/// the terminal block-syntax guard fires before any HTTP call.
#[test]
fn docker_only_without_container_routes_to_the_docker_exec_sentinel() {
    let _g = EnvGuard::new();
    std::env::set_var("IRIS_CONTAINER", "iris-dev-iris");

    let cfg = parse("docker_only = true\n");
    let conn = workspace_config_to_connection(&cfg, "USER").expect("must return a connection");

    assert_eq!(conn.base_url, "http://127.0.0.1:1");
    assert!(
        matches!(&conn.source, DiscoverySource::Docker { container_name } if container_name == "iris-dev-iris"),
        "the container name has to reach check_config, which reads the discovery source; got {:?}",
        conn.source
    );
    assert_eq!(conn.namespace, "USER");
    assert_eq!(conn.username, "_SYSTEM");
}

#[test]
fn nopws_without_container_takes_the_same_path() {
    let _g = EnvGuard::new();
    std::env::set_var("IRIS_CONTAINER", "irishealth-ai");

    let cfg = parse("nopws = true\n");
    let conn = workspace_config_to_connection(&cfg, "USER").expect("must return a connection");

    assert_eq!(conn.base_url, "http://127.0.0.1:1");
    assert!(
        matches!(&conn.source, DiscoverySource::Docker { container_name } if container_name == "irishealth-ai"),
        "expected a Docker source naming the container, got {:?}",
        conn.source
    );
}

/// With no container anywhere, the connection still has to come back — `execute()` resolves
/// `IRIS_CONTAINER` at call time, and returning `None` here is what sent the caller to Atelier.
#[test]
fn docker_only_with_no_container_anywhere_still_returns_a_connection() {
    let _g = EnvGuard::new();

    let cfg = parse("docker_only = true\n");
    let conn = workspace_config_to_connection(&cfg, "USER").expect("must return a connection");

    assert!(
        matches!(&conn.source, DiscoverySource::Docker { container_name } if container_name.is_empty()),
        "an empty name is resolved later, not a reason to fall back to HTTP; got {:?}",
        conn.source
    );
}

#[test]
fn docker_only_reads_credentials_and_namespace_from_the_config_first() {
    let _g = EnvGuard::new();
    std::env::set_var("IRIS_NAMESPACE", "FROMENV");
    std::env::set_var("IRIS_USERNAME", "fromenv");

    let cfg = parse(
        r#"
docker_only = true
namespace = "FROMCONFIG"
username = "fromconfig"
password = "pw"
ssh_host = "docker-host.example"
"#,
    );
    let conn = workspace_config_to_connection(&cfg, "USER").expect("must return a connection");

    assert_eq!(conn.namespace, "FROMCONFIG");
    assert_eq!(conn.username, "fromconfig");
    assert_eq!(conn.password, "pw");
    assert_eq!(
        conn.ssh_host.as_deref(),
        Some("docker-host.example"),
        "ssh_host has to survive or every docker exec goes to the wrong host"
    );
}

#[test]
fn docker_only_falls_back_to_environment_when_the_config_is_silent() {
    let _g = EnvGuard::new();
    std::env::set_var("IRIS_NAMESPACE", "FROMENV");
    std::env::set_var("IRIS_USERNAME", "fromenv");
    std::env::set_var("IRIS_PASSWORD", "envpw");

    let cfg = parse("docker_only = true\n");
    let conn = workspace_config_to_connection(&cfg, "USER").expect("must return a connection");

    assert_eq!(conn.namespace, "FROMENV");
    assert_eq!(conn.username, "fromenv");
    assert_eq!(conn.password, "envpw");
}

/// Neither host nor container nor a docker flag: nothing to build a connection from.
#[test]
fn an_empty_config_returns_no_connection() {
    let _g = EnvGuard::new();
    let cfg = parse("");
    assert!(workspace_config_to_connection(&cfg, "USER").is_none());
}

// ── Loading the file ────────────────────────────────────────────────────────────────────

#[test]
fn a_missing_config_file_is_not_an_error() {
    let _g = EnvGuard::new();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_str().expect("utf-8 path");

    assert!(load_workspace_config(Some(path)).is_none());
    assert!(load_workspace_config_with_path(Some(path)).is_none());
}

#[test]
fn the_legacy_file_name_still_loads() {
    let _g = EnvGuard::new();
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".iris-dev.toml"),
        "container = \"legacy\"\n",
    )
    .expect("write legacy config");
    let path = dir.path().to_str().expect("utf-8 path");

    let cfg = load_workspace_config(Some(path)).expect("legacy config must load");
    assert_eq!(cfg.container.as_deref(), Some("legacy"));

    let (cfg, loaded) = load_workspace_config_with_path(Some(path)).expect("legacy must load");
    assert_eq!(cfg.container.as_deref(), Some("legacy"));
    assert!(loaded.ends_with(".iris-dev.toml"));
}

#[test]
fn the_current_file_name_wins_over_the_legacy_one() {
    let _g = EnvGuard::new();
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        "container = \"current\"\n",
    )
    .expect("write current config");
    std::fs::write(
        dir.path().join(".iris-dev.toml"),
        "container = \"legacy\"\n",
    )
    .expect("write legacy config");
    let path = dir.path().to_str().expect("utf-8 path");

    let (cfg, loaded) = load_workspace_config_with_path(Some(path)).expect("config must load");
    assert_eq!(cfg.container.as_deref(), Some("current"));
    assert!(loaded.ends_with(".iris-agentic-dev.toml"));
}

/// A typo in the config must not take the process down, and must not look like an empty config
/// either — the warning in the log is the only signal, so the return has to be `None`.
#[test]
fn a_malformed_config_warns_and_returns_none() {
    let _g = EnvGuard::new();
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        "container = \"unterminated\ndocker_only = yes\n",
    )
    .expect("write malformed config");
    let path = dir.path().to_str().expect("utf-8 path");

    assert!(load_workspace_config(Some(path)).is_none());
    assert!(load_workspace_config_with_path(Some(path)).is_none());
}

#[test]
fn objectscript_workspace_outranks_the_argument() {
    let _g = EnvGuard::new();
    let env_dir = tempfile::tempdir().expect("tempdir");
    let arg_dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        env_dir.path().join(".iris-agentic-dev.toml"),
        "container = \"from-env\"\n",
    )
    .expect("write env config");
    std::fs::write(
        arg_dir.path().join(".iris-agentic-dev.toml"),
        "container = \"from-arg\"\n",
    )
    .expect("write arg config");
    std::env::set_var("OBJECTSCRIPT_WORKSPACE", env_dir.path());

    let cfg = load_workspace_config(arg_dir.path().to_str()).expect("config must load");
    assert_eq!(cfg.container.as_deref(), Some("from-env"));
}
