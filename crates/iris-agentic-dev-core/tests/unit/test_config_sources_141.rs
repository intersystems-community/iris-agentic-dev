//! Spec 141 FR-034: which configuration files are read, and which of them is the operator file.
//!
//! Every case passes `LoadInputs` with a temporary home; none reads the developer's own
//! `~/.iris-agentic-dev.toml` or the process environment.

use iris_agentic_dev_core::iris::config_sources::{
    ConfigSnapshot, FileKind, LoadInputs, LoadedFile, NamedBy, CONFIG_FILE_NAME,
};
use iris_agentic_dev_core::iris::workspace_config::McpTemplate;
use iris_agentic_dev_core::policy::effective::PartSource;
use std::path::{Path, PathBuf};

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap()
}

fn inputs(home: &Path, cwd: &Path) -> LoadInputs {
    LoadInputs {
        cwd: canon(cwd),
        home: Some(canon(home)),
        ..Default::default()
    }
}

fn ns_of(file: &LoadedFile) -> String {
    file.table
        .get("namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

// T-R4a-U01
#[test]
fn operator_file_is_loaded_even_when_the_project_has_its_own_file() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = home.join("proj");
    write(
        &home,
        CONFIG_FILE_NAME,
        "[policy.default]\nmcpTemplate = \"live\"\n",
    );
    write(&proj, CONFIG_FILE_NAME, "namespace = \"APPNS\"\n");

    let snap = ConfigSnapshot::load(&inputs(&home, &proj));

    let op = snap.operator_file.as_ref().expect("operator file loaded");
    assert_eq!(op.kind, FileKind::Operator);
    assert_eq!(op.path, canon(&home.join(CONFIG_FILE_NAME)));
    let ws = snap
        .workspace_layer
        .as_ref()
        .expect("workspace file loaded");
    assert_eq!(ws.kind, FileKind::Workspace);
    assert_eq!(ws.path, canon(&proj.join(CONFIG_FILE_NAME)));
    assert_eq!(ns_of(ws), "APPNS");

    let policy = snap.effective_policy("default");
    assert_eq!(
        policy.mcp_template.value,
        Some(McpTemplate::Live),
        "the operator file's template must survive a project file that declares no policy"
    );
    assert_eq!(policy.mcp_template.from, PartSource::OperatorFile);
}

// T-R4a-U02
#[test]
fn the_walk_stops_before_the_home_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, CONFIG_FILE_NAME, "namespace = \"OPNS\"\n");
    // A file ABOVE home must not be reached either: the walk stops at home, it does not pass it.
    write(tmp.path(), CONFIG_FILE_NAME, "namespace = \"ABOVE\"\n");
    let deep = home.join("a").join("b");
    std::fs::create_dir_all(&deep).unwrap();

    for cwd in [home.clone(), home.join("a"), deep] {
        let snap = ConfigSnapshot::load(&inputs(&home, &cwd));
        assert!(
            snap.workspace_layer.is_none(),
            "cwd {} reached a workspace file: {:?}",
            cwd.display(),
            snap.workspace_layer.as_ref().map(|f| &f.path)
        );
        let op = snap.operator_file.as_ref().expect("operator file loaded");
        assert_eq!(op.kind, FileKind::Operator);
        assert_eq!(ns_of(op), "OPNS");
    }
}

#[test]
fn a_project_outside_home_walks_to_its_file() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let proj = tmp.path().join("elsewhere").join("proj");
    write(&proj, CONFIG_FILE_NAME, "namespace = \"APPNS\"\n");
    let deep = proj.join("src").join("x");
    std::fs::create_dir_all(&deep).unwrap();

    let snap = ConfigSnapshot::load(&inputs(&home, &deep));
    let ws = snap.workspace_layer.as_ref().expect("found by the walk");
    assert_eq!(ws.path, canon(&proj.join(CONFIG_FILE_NAME)));
    assert_eq!(ws.kind, FileKind::Workspace);
    assert!(snap.operator_file.is_none(), "no operator file exists here");
}

// T-R4a-U03
#[cfg(unix)]
#[test]
fn a_config_flag_naming_the_operator_file_through_a_symlink_is_read_once() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let op = write(&home, CONFIG_FILE_NAME, "namespace = \"OPNS\"\n");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let link = proj.join(CONFIG_FILE_NAME);
    std::os::unix::fs::symlink(&op, &link).unwrap();

    // The same file reached through --config and through the walk.
    let mut by_flag = inputs(&home, &proj);
    by_flag.config_flag = Some(link.clone());
    let by_walk = inputs(&home, &proj);

    for (label, i) in [("--config", by_flag), ("walk", by_walk)] {
        let snap = ConfigSnapshot::load(&i);
        assert!(
            snap.workspace_layer.is_none(),
            "{label}: a file that is the operator file must not also be a workspace file"
        );
        let opf = snap.operator_file.as_ref().expect("operator file loaded");
        assert_eq!(opf.kind, FileKind::Operator, "{label}");
        assert_eq!(opf.path, canon(&op), "{label}");
    }
}

// T-R4a-U04
#[test]
fn precedence_is_config_then_env_workspace_then_flag_then_walk() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let walk = tmp.path().join("walk");
    let flag = tmp.path().join("flag");
    let env = tmp.path().join("env");
    let cfg = tmp.path().join("cfgdir");
    write(&walk, CONFIG_FILE_NAME, "namespace = \"WALK\"\n");
    write(&flag, CONFIG_FILE_NAME, "namespace = \"FLAG\"\n");
    write(&env, CONFIG_FILE_NAME, "namespace = \"ENV\"\n");
    let cfg_file = write(&cfg, "custom.toml", "namespace = \"CFG\"\n");

    let base = || {
        let mut i = inputs(&home, &walk);
        i.workspace_flag = Some(flag.to_string_lossy().into_owned());
        i.env_workspace = Some(env.to_string_lossy().into_owned());
        i
    };

    // OBJECTSCRIPT_WORKSPACE beats --workspace beats the walk; it names a config file.
    let snap = ConfigSnapshot::load(&base());
    let ws = snap.workspace_layer.as_ref().unwrap();
    assert_eq!(ns_of(ws), "ENV");
    assert_eq!(ws.kind, FileKind::Named);
    assert_eq!(ws.named_by, Some(NamedBy::ObjectscriptWorkspace));

    // --config beats all of them.
    let mut i = base();
    i.config_flag = Some(cfg_file);
    let snap = ConfigSnapshot::load(&i);
    let ws = snap.workspace_layer.as_ref().unwrap();
    assert_eq!(ns_of(ws), "CFG");
    assert_eq!(ws.kind, FileKind::Named);
    assert_eq!(ws.named_by, Some(NamedBy::ConfigFlag));

    // --workspace beats the walk; the file is a workspace file the flag named.
    let mut i = inputs(&home, &walk);
    i.workspace_flag = Some(flag.to_string_lossy().into_owned());
    let snap = ConfigSnapshot::load(&i);
    let ws = snap.workspace_layer.as_ref().unwrap();
    assert_eq!(ns_of(ws), "FLAG");
    assert_eq!(ws.kind, FileKind::Workspace);
    assert_eq!(ws.named_by, Some(NamedBy::WorkspaceFlag));

    // The walk alone.
    let snap = ConfigSnapshot::load(&inputs(&home, &walk));
    let ws = snap.workspace_layer.as_ref().unwrap();
    assert_eq!(ns_of(ws), "WALK");
    assert_eq!(ws.named_by, None);
}

// T-R4a-U05
#[test]
fn the_legacy_home_file_is_not_read_and_the_rename_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, ".iris-dev.toml", "namespace = \"LEGACY\"\n");

    let snap = ConfigSnapshot::load(&inputs(&home, &home));
    assert!(
        snap.operator_file.is_none(),
        "~/.iris-dev.toml must not be read as the operator file"
    );
    assert!(snap.workspace_layer.is_none());
    assert!(
        snap.config_errors
            .iter()
            .any(|e| e.contains(".iris-dev.toml") && e.contains(".iris-agentic-dev.toml")),
        "config_errors must say to rename it: {:?}",
        snap.config_errors
    );
}

#[test]
fn a_legacy_project_file_found_by_the_walk_is_still_a_workspace_file() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let proj = tmp.path().join("proj");
    write(&proj, ".iris-dev.toml", "namespace = \"LEGACY\"\n");

    let snap = ConfigSnapshot::load(&inputs(&home, &proj));
    let ws = snap.workspace_layer.as_ref().expect("legacy project file");
    assert_eq!(ns_of(ws), "LEGACY");
    assert_eq!(ws.kind, FileKind::Workspace);
}

/// An untrusted `[instance.*]` block loses both loosening settings in the same step: its
/// `tls_verify = false` and the service account its code would run as (spec 141, FR-030, R2).
#[test]
fn an_untrusted_instance_block_loses_tls_off_and_its_service_account_together() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    let body = "[instance.p]\nhost = \"p.example\"\ntls_verify = false\n\
                service_username = \"svc\"\nservice_password = \"pw\"\ncontainer = \"db1\"\n";
    write(&proj, CONFIG_FILE_NAME, body);

    let snap = ConfigSnapshot::load(&inputs(&home, &proj));
    let p = snap.fleet.instance.get("p").expect("the instance stays");
    assert_eq!(p.host.as_deref(), Some("p.example"));
    assert_eq!(p.tls_verify, None, "validation stays on");
    assert_eq!(p.service_username, None);
    assert_eq!(p.service_password, None);
    assert_eq!(
        p.container.as_deref(),
        Some("db1"),
        "a container is a connection detail, not a loosening"
    );
}

/// Certificate validation, an SSH host and a service account are read only after the trust filter
/// (FR-030). They have no policy part, so the snapshot keeps them out of the connection settings
/// and lists them.
#[test]
fn an_untrusted_file_cannot_turn_off_tls_or_name_an_ssh_host() {
    use iris_agentic_dev_core::iris::config_sources::SetAsideKind;

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    let body = "container = \"c\"\ntls_verify = false\nssh_host = \"jump.example\"\n\
                service_username = \"svc\"\n[instance.p]\nhost = \"p.example\"\ntls_verify = false\n";
    let ws = write(&proj, CONFIG_FILE_NAME, body);
    let inputs = inputs(&home, &proj);

    let snap = ConfigSnapshot::load(&inputs);
    let (cfg, _, _) = snap.connection_config.clone().expect("the project file");
    assert_eq!(cfg.tls_verify, None, "validation stays on");
    assert_eq!(cfg.ssh_host, None, "no remote docker host");
    assert_eq!(
        cfg.container.as_deref(),
        Some("c"),
        "connection details still apply"
    );
    for id in [
        "workspace:tls_verify",
        "workspace:ssh_host",
        "workspace:service_username",
        "workspace:instance.p.tls_verify",
    ] {
        let item = snap
            .set_aside
            .iter()
            .find(|i| i.id == id)
            .unwrap_or_else(|| panic!("{id} missing: {:?}", snap.set_aside));
        assert_eq!(item.kind, SetAsideKind::UntrustedSetting);
        assert!(
            item.lift.contains("iris-agentic-dev trust"),
            "{}",
            item.lift
        );
    }

    // Trusted: the settings apply and nothing is set aside for them.
    let lines = [
        "connection.container = \"c\"",
        "instance.p.address = \"http://p.example:52773\"",
        "instance.p.tls_verify = false",
        "service_username = \"svc\"",
        "ssh_host = \"jump.example\"",
        "tls_verify = false",
    ];
    write(
        &home,
        CONFIG_FILE_NAME,
        &format!(
            "[[trust]]\npath = {:?}\nformat = 1\nsettings = {:?}\n",
            canon(&ws).to_string_lossy(),
            lines
        ),
    );
    let snap = ConfigSnapshot::load(&inputs);
    assert!(snap.trust().is_trusted(), "{:?}", snap.trust());
    let (cfg, _, _) = snap.connection_config.clone().unwrap();
    assert_eq!(cfg.tls_verify, Some(false));
    assert_eq!(cfg.ssh_host.as_deref(), Some("jump.example"));
    assert!(snap.set_aside.is_empty(), "{:?}", snap.set_aside);
}

/// The operator file may turn validation off for itself without any trust.
#[test]
fn the_operator_file_keeps_its_tls_and_ssh_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(
        &home,
        CONFIG_FILE_NAME,
        "container = \"c\"\ntls_verify = false\nssh_host = \"jump.example\"\n",
    );
    let snap = ConfigSnapshot::load(&inputs(&home, &home));
    let (cfg, _, kind) = snap.connection_config.clone().expect("the operator file");
    assert_eq!(kind, FileKind::Operator);
    assert_eq!(cfg.tls_verify, Some(false));
    assert_eq!(cfg.ssh_host.as_deref(), Some("jump.example"));
}
