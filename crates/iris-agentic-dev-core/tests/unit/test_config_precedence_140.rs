//! Spec 140: which source chooses the connection. A project's `.iris-agentic-dev.toml` outranks
//! `IRIS_*`; the same file in the home directory is a personal default and yields to `IRIS_HOST`.
//! The binary tests are in `bin/tests/integration/binary_140_config_precedence.rs`.

use iris_agentic_dev_core::iris::config_sources::{
    ConfigSnapshot, FileKind, LoadInputs, CONFIG_FILE_NAME,
};
use iris_agentic_dev_core::iris::workspace_config::file_chooses_connection;

#[test]
fn a_project_file_chooses_over_iris_host() {
    assert!(file_chooses_connection(FileKind::Workspace, true));
    assert!(file_chooses_connection(FileKind::Workspace, false));
    assert!(file_chooses_connection(FileKind::Named, true));
}

#[test]
fn the_operator_file_yields_to_iris_host_only() {
    assert!(!file_chooses_connection(FileKind::Operator, true));
    assert!(file_chooses_connection(FileKind::Operator, false));
}

/// What decides the kind is the snapshot: a file the load reads as the operator file yields, a
/// project file does not. On macOS `/var` is a link to `/private/var`, so the walk up from the
/// working directory and `$HOME` can name the same directory two ways. They still count as one.
#[cfg(unix)]
#[test]
fn a_home_reached_through_a_symlink_is_still_home() {
    let real = tempfile::tempdir().unwrap();
    let link_parent = tempfile::tempdir().unwrap();
    let link = link_parent.path().join("home-link");
    std::os::unix::fs::symlink(real.path(), &link).unwrap();
    std::fs::write(real.path().join(CONFIG_FILE_NAME), "host = \"x\"\n").unwrap();

    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: real.path().to_path_buf(),
        home: Some(link),
        host_env: Some("elsewhere".to_string()),
        ..Default::default()
    });
    let (_, _, kind) = snap.connection_config.clone().expect("the operator file");
    assert_eq!(kind, FileKind::Operator);
    assert!(!snap.file_chooses_connection(), "it yields to IRIS_HOST");
    assert!(snap.workspace_layer.is_none());
}

#[test]
fn a_project_file_in_a_snapshot_chooses_even_with_iris_host_set() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join(CONFIG_FILE_NAME), "host = \"x\"\n").unwrap();
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: proj,
        home: Some(home),
        host_env: Some("elsewhere".to_string()),
        ..Default::default()
    });
    assert!(snap.file_chooses_connection());
}

/// A workspace file is checked in with the repository, so its `host` is the repository's choice.
/// The operator's `IRIS_USERNAME`/`IRIS_PASSWORD` go to it only when it is this machine or the
/// server `IRIS_HOST` already names. Otherwise a cloned repository could send the operator's
/// password anywhere, and from 1.6.3 the file outranks `IRIS_HOST` (security review, 2026-10-22).
#[test]
fn env_credentials_go_only_to_loopback_or_the_iris_host_server() {
    use iris_agentic_dev_core::iris::endpoint::Endpoint;
    use iris_agentic_dev_core::iris::workspace_config::env_credentials_may_go_to;
    let ep = |host: &str| Endpoint::parse(None, host, 52773, None).unwrap();
    for host in [
        "localhost",
        "LOCALHOST",
        "127.0.0.1",
        "127.4.5.6",
        "::1",
        "[::1]",
    ] {
        assert!(
            env_credentials_may_go_to(&ep(host), None),
            "{host} is loopback"
        );
    }
    assert!(env_credentials_may_go_to(
        &ep("iris.corp"),
        Some(&ep("iris.corp"))
    ));
    assert!(env_credentials_may_go_to(
        &ep("IRIS.corp "),
        Some(&ep("iris.CORP"))
    ));
    assert!(!env_credentials_may_go_to(&ep("evil.example"), None));
    assert!(!env_credentials_may_go_to(
        &ep("evil.example"),
        Some(&ep("iris.corp"))
    ));
    assert!(!env_credentials_may_go_to(
        &ep("localhost.evil.example"),
        None
    ));
    assert!(!env_credentials_may_go_to(
        &ep("127.0.0.1.evil.example"),
        None
    ));
}

/// The loopback check reads `host`, so `scheme` must not be able to move the URL: with
/// `scheme = "http://evil.example/#"` and `host = "localhost"` the URL went to evil.example.
#[test]
fn only_http_and_https_are_schemes() {
    use iris_agentic_dev_core::iris::workspace_config::checked_scheme;
    assert_eq!(checked_scheme("http").as_deref(), Some("http"));
    assert_eq!(checked_scheme("HTTPS").as_deref(), Some("https"));
    assert_eq!(checked_scheme("https://").as_deref(), Some("https"));
    assert_eq!(checked_scheme("//https//").as_deref(), Some("https"));
    for bad in [
        "http://evil.example/#",
        "ftp",
        "javascript",
        "http@x",
        "https:x",
    ] {
        assert_eq!(checked_scheme(bad), None, "{bad}");
    }
}

// ── T194: a file names a connection only when it names a host or a container ───────────────────

fn load(op: &str, ws: Option<&str>, host_env: Option<&str>) -> ConfigSnapshot {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let (home, proj) = (root.join("home"), root.join("proj"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    if !op.is_empty() {
        std::fs::write(home.join(CONFIG_FILE_NAME), op).unwrap();
    }
    if let Some(ws) = ws {
        std::fs::write(proj.join(CONFIG_FILE_NAME), ws).unwrap();
    }
    ConfigSnapshot::load(&LoadInputs {
        cwd: proj,
        home: Some(home),
        host_env: host_env.map(str::to_string),
        ..Default::default()
    })
}

fn names_a_target(toml_text: &str) -> bool {
    use iris_agentic_dev_core::iris::workspace_config::{names_a_target, WorkspaceConfig};
    names_a_target(&toml::from_str::<WorkspaceConfig>(toml_text).unwrap())
}

#[test]
fn a_host_a_container_or_the_docker_route_names_a_target() {
    assert!(names_a_target("host = \"10.0.0.5\"\n"));
    assert!(names_a_target("container = \"iris\"\n"));
    // `docker_only` and `nopws` name the route: docker exec, never HTTP. A file that sets one and
    // let discovery pick an HTTP server instead would run the call over the transport it ruled out.
    assert!(names_a_target("docker_only = true\n"));
    assert!(names_a_target("nopws = true\n"));
    assert!(names_a_target(
        "host = \"h\"\nweb_port = 52780\nnamespace = \"APP\"\n"
    ));
    // Decoration: these shape a connection chosen elsewhere.
    for decorates in [
        "namespace = \"APP\"\n",
        "web_port = 52780\n",
        "scheme = \"https\"\n",
        "web_prefix = \"/iris\"\n",
        "username = \"u\"\npassword = \"p\"\n",
        "docker_only = false\n",
        "tls_verify = false\n",
        "# only a comment\n",
    ] {
        assert!(!names_a_target(decorates), "{decorates:?}");
    }
    // Blank is not a name.
    assert!(!names_a_target("host = \"\"\n"));
    assert!(!names_a_target("container = \"  \"\n"));
}

#[test]
fn a_project_file_that_names_no_target_does_not_choose_the_connection() {
    let snap = load("", Some("namespace = \"APP\"\n"), Some("elsewhere"));
    assert!(
        !snap.file_chooses_connection(),
        "a file with only a namespace decorates IRIS_HOST's server; it does not choose"
    );
    // It was read, and its other settings still apply.
    assert!(snap.connection_config.is_some());

    let snap = load(
        "",
        Some("host = \"x\"\nnamespace = \"APP\"\n"),
        Some("elsewhere"),
    );
    assert!(snap.file_chooses_connection());
    let snap = load("", Some("container = \"c\"\n"), Some("elsewhere"));
    assert!(snap.file_chooses_connection());
}

#[test]
fn the_operator_file_chooses_only_when_it_names_a_target_and_iris_host_is_unset() {
    let snap = load("namespace = \"APP\"\n", None, None);
    assert!(!snap.file_chooses_connection(), "no target in the file");
    let snap = load("host = \"h\"\n", None, None);
    assert!(snap.file_chooses_connection());
    let snap = load("host = \"h\"\n", None, Some("elsewhere"));
    assert!(!snap.file_chooses_connection(), "yields to IRIS_HOST");
}

/// What `check_config` says at startup, through the real constructor.
mod startup {
    use super::*;
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    use iris_agentic_dev_core::testing::EnvGuard;
    use iris_agentic_dev_core::tools::write_gate::DeclaredGates;
    use iris_agentic_dev_core::tools::{IrisTools, Toolset};

    async fn check_config(file_body: &str, iris_host: Option<&str>) -> (serde_json::Value, String) {
        let mut env = EnvGuard::clean();
        if let Some(h) = iris_host {
            env.set("IRIS_HOST", h);
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, file_body).unwrap();
        let conn = IrisConnection::new(
            "http://127.0.0.1:9",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        let tools = IrisTools::with_registry_and_toolset(
            Some(conn),
            iris_agentic_dev_core::skills::SkillRegistry::new(),
            Toolset::Merged,
            None,
            Some(path.clone()),
            true,
            DeclaredGates::default(),
        )
        .unwrap();
        let r = tools
            .call_for_test("check_config", serde_json::json!({}))
            .await
            .unwrap();
        let text = r.content[0].as_text().unwrap().text.clone();
        (
            serde_json::from_str(&text).unwrap(),
            path.to_string_lossy().into_owned(),
        )
    }

    #[tokio::test]
    async fn a_file_with_a_host_is_the_connection_source() {
        let (cfg, path) =
            check_config("host = \"127.0.0.1\"\nweb_port = 9\n", Some("127.0.0.1")).await;
        assert_eq!(cfg["connection_source"], "config_file", "{cfg}");
        assert_eq!(cfg["config_file"], path, "{cfg}");
        assert!(cfg["config_file_named_no_target"].is_null(), "{cfg}");
        assert!(cfg["config_file_warning"].is_null(), "{cfg}");
    }

    #[tokio::test]
    async fn a_file_with_only_a_namespace_is_reported_as_naming_no_target() {
        let (cfg, path) = check_config("namespace = \"APP\"\n", Some("127.0.0.1")).await;
        assert_eq!(
            cfg["connection_source"], "env_vars",
            "IRIS_HOST chose the server, not the file: {cfg}"
        );
        assert!(cfg["config_file"].is_null(), "{cfg}");
        assert_eq!(cfg["config_file_named_no_target"], path, "{cfg}");
        let w = cfg["config_file_warning"]
            .as_str()
            .unwrap_or_else(|| panic!("{cfg}"));
        assert!(w.contains(&path), "{w}");
        assert!(w.contains("neither a host nor a container"), "{w}");
    }

    #[tokio::test]
    async fn without_iris_host_a_file_with_no_target_leaves_the_connection_discovered() {
        let (cfg, path) = check_config("web_port = 52780\n", None).await;
        assert_eq!(cfg["connection_source"], "auto_discovered", "{cfg}");
        assert!(cfg["config_file"].is_null(), "{cfg}");
        assert_eq!(cfg["config_file_named_no_target"], path, "{cfg}");
        // The fallback warning must not claim there is no file.
        assert!(cfg["fallback_warning"].is_null(), "{cfg}");
    }
}
