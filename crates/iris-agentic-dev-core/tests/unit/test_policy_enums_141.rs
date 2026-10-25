//! Spec 141 R1 D11: policy enums are read case-insensitively, and a value iad cannot read
//! tightens that one field. Every case parses a TOML string through the snapshot loader.
//!
//! Before 1.7.0 `mcpTemplate = "Live"` read as no template (so as `dev`), and one bad value failed
//! the whole fleet parse, which removed every `[policy.*]`, every role and every pool entry at once.

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::workspace_config::{
    ConnectionRole, DataPolicy, McpTemplate, ToolCategory,
};

fn load(toml: &str) -> (ConfigSnapshot, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cwd = dir.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(home.join(CONFIG_FILE_NAME), toml).unwrap();
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: std::fs::canonicalize(&cwd).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    });
    (snap, dir)
}

#[test]
fn mcp_template_is_case_insensitive() {
    for (spelling, want) in [
        ("Live", McpTemplate::Live),
        ("LIVE", McpTemplate::Live),
        (" live ", McpTemplate::Live),
        ("Test", McpTemplate::Test),
        ("DEV", McpTemplate::Dev),
    ] {
        let (snap, _d) = load(&format!("[policy.prod]\nmcpTemplate = \"{spelling}\"\n"));
        assert_eq!(
            snap.effective_policy("prod").mcp_template.value,
            Some(want),
            "mcpTemplate = {spelling:?}"
        );
        assert!(
            snap.config_errors.is_empty(),
            "{spelling:?} is a valid spelling, got {:?}",
            snap.config_errors
        );
    }
}

#[test]
fn an_unknown_template_is_live_and_is_reported() {
    let (snap, _d) = load("[policy.prod]\nmcpTemplate = \"staging\"\n");
    assert_eq!(
        snap.effective_policy("prod").mcp_template.value,
        Some(McpTemplate::Live)
    );
    assert!(
        snap.config_errors.iter().any(|e| e.contains("staging")),
        "the value iad could not read must be listed: {:?}",
        snap.config_errors
    );
}

#[test]
fn allow_entries_are_case_insensitive_and_an_unknown_one_is_dropped_and_reported() {
    let (snap, _d) = load("[policy.prod]\nallow = [\"Query\", \"querry\"]\n");
    assert_eq!(
        snap.effective_policy("prod").allow.value,
        Some(vec![ToolCategory::Query]),
        "the list only narrows: the entry iad cannot read is dropped"
    );
    assert!(
        snap.config_errors.iter().any(|e| e.contains("querry")),
        "{:?}",
        snap.config_errors
    );
}

#[test]
fn data_policy_and_role_are_case_insensitive_and_fail_closed() {
    let (snap, _d) = load(
        "mode = \"operate\"\n\
         [instance.a]\nhost = \"a\"\nrole = \"Subject\"\n\
         [instance.b]\nhost = \"b\"\nrole = \"nonsense\"\n\
         [policy.a]\ndataPolicy = \"Block\"\n\
         [policy.b]\ndataPolicy = \"maybe\"\n\
         [policy.c]\ndataPolicy = \"ALLOW\"\n",
    );
    assert_eq!(
        snap.effective_policy("a").data_policy.value,
        DataPolicy::Block
    );
    assert_eq!(
        snap.effective_policy("a").role.value,
        ConnectionRole::Subject
    );
    assert_eq!(
        snap.effective_policy("b").data_policy.value,
        DataPolicy::Block,
        "an unreadable dataPolicy is block"
    );
    assert_eq!(
        snap.effective_policy("b").role.value,
        ConnectionRole::Subject,
        "an unreadable role is subject"
    );
    assert_eq!(
        snap.effective_policy("c").data_policy.value,
        DataPolicy::Allow
    );
}

#[test]
fn one_bad_value_does_not_take_the_other_policies_with_it() {
    let (snap, _d) = load(
        "mode = \"operate\"\n\
         [instance.a]\nhost = \"a\"\nrole = \"subject\"\n\
         [policy.a]\nmcpTemplate = \"live\"\nallow = [\"search\"]\n\
         [policy.b]\nmcpTemplate = \"garbage\"\n",
    );
    let a = snap.effective_policy("a");
    assert_eq!(a.mcp_template.value, Some(McpTemplate::Live));
    assert_eq!(a.allow.value, Some(vec![ToolCategory::Search]));
    assert_eq!(a.role.value, ConnectionRole::Subject);
    assert_eq!(
        snap.fleet.instance.get("a").map(|i| i.role),
        Some(ConnectionRole::Subject),
        "the instance survives the neighbouring bad value"
    );
}

#[test]
fn a_syntax_error_makes_the_snapshot_invalid_and_closes_the_gates() {
    let (snap, _d) = load("[policy.prod\nmcpTemplate = \"live\"\n");
    assert!(snap.is_invalid());
    let d = snap.declared_gates();
    assert_eq!(d.write_tools_enabled, Some(false));
    assert_eq!(d.destructive_tools_enabled, Some(false));
}

#[test]
fn write_tools_from_an_untrusted_workspace_file_is_set_aside_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let proj = dir.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join(CONFIG_FILE_NAME),
        "[policy.prod]\nwriteTools = true\n",
    )
    .unwrap();
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: std::fs::canonicalize(&proj).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    });
    let p = snap.effective_policy("prod");
    assert_ne!(
        p.write_tools.value,
        Some(true),
        "a project file that has not been trusted cannot declare a member writable"
    );
    assert!(
        snap.set_aside
            .iter()
            .any(|i| i.server.as_deref() == Some("prod") && i.setting.contains("writeTools")),
        "the ignored setting must be listed: {:?}",
        snap.set_aside
    );
}
