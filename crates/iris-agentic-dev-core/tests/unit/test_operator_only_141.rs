//! Spec 141 FR-035: settings only the operator file (or launch settings) may set.
//!
//! A workspace file that carries one is set aside, trusted or not; the operator file's value is
//! honoured. The keys are found on the raw TOML table, so no struct a workspace file deserialises
//! into has a field for them.

use iris_agentic_dev_core::iris::config_sources::{
    operator_only_items, ConfigSnapshot, LoadInputs, SetAsideKind, CONFIG_FILE_NAME,
    OPERATOR_ONLY_KEYS,
};
use std::path::{Path, PathBuf};

const SECRET: &str = "s3cret-token-value-0123456789abcdef";

fn workspace_with_operator_only_keys() -> String {
    format!(
        r#"
exec_as_os_user = true
local_skills = true
audit_payloads = true
telemetry_sink = "iris"

[transport]
http_token = "{SECRET}"

[[trust]]
path = "/somewhere/.iris-agentic-dev.toml"
format = 1
settings = ["write_tools_enabled = true"]
"#
    )
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

// T-R4a-U11
#[test]
fn a_workspace_table_with_operator_only_keys_yields_one_item_each() {
    let t: toml::Table = workspace_with_operator_only_keys().parse().unwrap();
    let items = operator_only_items(&t, Path::new("/proj/.iris-agentic-dev.toml"));
    let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "workspace:audit_payloads",
            "workspace:exec_as_os_user",
            "workspace:local_skills",
            "workspace:telemetry_sink",
            "workspace:transport.http_token",
            "workspace:trust",
        ]
    );
    for i in &items {
        assert_eq!(i.kind, SetAsideKind::OperatorOnly, "{}", i.id);
        assert_eq!(i.file.as_deref(), Some("/proj/.iris-agentic-dev.toml"));
        assert!(
            i.lift.contains("trust --show") && !i.lift.contains("~/.iris-agentic-dev.toml"),
            "lift sends the operator to `trust --show`, not to the operator file: {}",
            i.lift
        );
        assert!(
            !i.to_json().to_string().contains(SECRET),
            "a set-aside item is shown to the agent and must not carry the token: {:?}",
            i
        );
    }
}

#[test]
fn a_file_without_operator_only_keys_yields_nothing() {
    let t: toml::Table = "namespace = \"USER\"\n[transport]\nother = 1\n"
        .parse()
        .unwrap();
    assert!(operator_only_items(&t, Path::new("/p/.iris-agentic-dev.toml")).is_empty());
}

#[test]
fn the_snapshot_sets_them_aside_from_a_workspace_file_trusted_or_not() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = home.join("proj");
    let ws_body = format!(
        "write_tools_enabled = true\n{}",
        workspace_with_operator_only_keys()
    );
    let ws_path = write(&proj, CONFIG_FILE_NAME, &ws_body);
    let canon_ws = std::fs::canonicalize(&ws_path).unwrap();

    let inputs = LoadInputs {
        cwd: std::fs::canonicalize(&proj).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    };

    // Untrusted.
    let snap = ConfigSnapshot::load(&inputs);
    let only: Vec<_> = snap
        .set_aside
        .iter()
        .filter(|i| i.kind == SetAsideKind::OperatorOnly)
        .collect();
    assert_eq!(only.len(), 6, "{:?}", snap.set_aside);
    assert_eq!(
        snap.operator,
        Default::default(),
        "nothing the workspace file said reaches the operator settings"
    );

    // Trusted: an operator entry that matches the file's loosening settings.
    write(
        &home,
        CONFIG_FILE_NAME,
        &format!(
            "[[trust]]\npath = {:?}\nformat = 1\nsettings = ['write_tools_enabled = true']\n",
            canon_ws.to_string_lossy()
        ),
    );
    let snap = ConfigSnapshot::load(&inputs);
    assert!(snap.trust().is_trusted(), "{:?}", snap.trust());
    let only: Vec<_> = snap
        .set_aside
        .iter()
        .filter(|i| i.kind == SetAsideKind::OperatorOnly)
        .collect();
    assert_eq!(
        only.len(),
        6,
        "trust never changes the answer for an operator-only key: {:?}",
        snap.set_aside
    );
    assert_eq!(snap.operator, Default::default());
}

#[test]
fn the_operator_file_is_honoured() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(
        &home,
        CONFIG_FILE_NAME,
        &format!(
            "exec_as_os_user = true\nlocal_skills = true\naudit_payloads = true\ntelemetry_sink = \"iris\"\n[transport]\nhttp_token = \"{SECRET}\"\n"
        ),
    );
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: std::fs::canonicalize(&home).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    });
    assert!(snap.operator.exec_as_os_user);
    assert!(snap.operator.local_skills);
    assert!(snap.operator.audit_payloads);
    assert_eq!(
        snap.operator.telemetry_sink,
        iris_agentic_dev_core::telemetry::TelemetrySink::Iris
    );
    assert_eq!(snap.operator.http_token.as_deref(), Some(SECRET));
    assert!(
        snap.set_aside
            .iter()
            .all(|i| i.kind != SetAsideKind::OperatorOnly),
        "the operator file may set them: {:?}",
        snap.set_aside
    );
}

// T-R4a-U12
#[test]
fn no_struct_a_workspace_file_deserialises_into_has_an_operator_only_field() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/iris/workspace_config.rs"),
    )
    .unwrap();

    fn body<'a>(src: &'a str, name: &str) -> &'a str {
        let head = format!("pub struct {name} {{");
        let start = src
            .find(&head)
            .unwrap_or_else(|| panic!("`{head}` not found; the scan would check nothing"));
        let rest = &src[start..];
        let end = rest.find("\n}\n").expect("struct end");
        &rest[..end]
    }

    let fields: Vec<&str> = OPERATOR_ONLY_KEYS
        .iter()
        .map(|k| k.rsplit('.').next().unwrap())
        .collect();
    assert!(fields.len() >= 5);

    for (name, must_have) in [
        ("WorkspaceConfig", "pub container:"),
        ("FleetConfig", "pub instance:"),
        ("InstanceConfig", "pub container:"),
    ] {
        let b = body(&src, name);
        assert!(
            b.contains(must_have),
            "the scan of {name} read the wrong text: {b}"
        );
        for f in &fields {
            assert!(
                !b.contains(&format!("pub {f}:")),
                "{name} must not have a `{f}` field: a workspace file could set it"
            );
        }
    }
}
