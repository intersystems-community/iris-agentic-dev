//! Spec 141 R5 C15: `read_only = "strict"` in any configuration file, or `IAD_READ_ONLY=strict` in
//! launch settings, switches the scratch-class tools off.
//!
//! The setting only tightens, so a workspace file that nobody trusted may set it. Every case parses
//! a TOML string through `ConfigSnapshot::load` with a temporary home; none reads the developer's
//! own configuration or the process environment.

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::testing::BEHAVIOR_ENV_VARS;
use std::path::{Path, PathBuf};

fn write(dir: &Path, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(CONFIG_FILE_NAME), body).unwrap();
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap()
}

/// A home with an operator file and a project directory with a workspace file; either body may be
/// `None`.
fn load(operator: Option<&str>, workspace: Option<&str>, launch: bool) -> ConfigSnapshot {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = home.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    if let Some(b) = operator {
        write(&home, b);
    }
    if let Some(b) = workspace {
        write(&proj, b);
    }
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: canon(&proj),
        home: Some(canon(&home)),
        launch_read_only_strict: launch,
        ..Default::default()
    });
    // The snapshot owns what it read; the directory can go.
    drop(tmp);
    snap
}

#[test]
fn nothing_declared_is_not_strict() {
    let snap = load(None, None, false);
    assert!(!snap.read_only_strict());
    assert_eq!(snap.read_only_source(), None);
}

#[test]
fn an_untrusted_workspace_file_may_set_strict() {
    // No [[trust]] entry exists anywhere. A loosening key would be set aside; this one tightens.
    let snap = load(None, Some("read_only = \"strict\"\n"), false);
    assert!(
        snap.read_only_strict(),
        "an untrusted project file's read_only = \"strict\" must apply: it only tightens"
    );
    assert!(
        snap.set_aside
            .iter()
            .all(|s| !s.setting.contains("read_only")),
        "strict is not set aside: {:?}",
        snap.set_aside
    );
    assert!(snap
        .read_only_source()
        .is_some_and(|s| s.contains(CONFIG_FILE_NAME)));
}

#[test]
fn the_operator_file_may_set_strict() {
    let snap = load(Some("read_only = \"strict\"\n"), None, false);
    assert!(snap.read_only_strict());
}

#[test]
fn launch_settings_set_strict_without_any_file() {
    let snap = load(None, None, true);
    assert!(snap.read_only_strict());
    assert_eq!(snap.read_only_source().as_deref(), Some("IAD_READ_ONLY"));
}

#[test]
fn any_one_source_is_enough_and_none_can_lift_another() {
    // The operator asked for strict; a project file that says something else does not undo it.
    let snap = load(
        Some("read_only = \"strict\"\n"),
        Some("read_only = \"off\"\n"),
        false,
    );
    assert!(snap.read_only_strict());
    let snap = load(None, Some("read_only = \"off\"\n"), true);
    assert!(snap.read_only_strict());
}

#[test]
fn the_value_is_read_trimmed_and_case_insensitive() {
    for v in ["strict", " Strict ", "STRICT"] {
        let snap = load(None, Some(&format!("read_only = \"{v}\"\n")), false);
        assert!(snap.read_only_strict(), "{v:?}");
    }
}

#[test]
fn a_value_that_is_not_strict_is_ignored_and_reported() {
    for v in ["strikt", "soft", "true", ""] {
        let snap = load(None, Some(&format!("read_only = \"{v}\"\n")), false);
        assert!(!snap.read_only_strict(), "{v:?} must not switch strict on");
        assert!(
            snap.config_errors.iter().any(|e| e.contains("read_only")),
            "{v:?}: the file said read_only and iad did not understand it; config_errors: {:?}",
            snap.config_errors
        );
    }
    // "off" is a value iad understands: no note.
    let snap = load(None, Some("read_only = \"off\"\n"), false);
    assert!(!snap.read_only_strict());
    assert!(snap.config_errors.iter().all(|e| !e.contains("read_only")));
}

#[test]
fn the_launch_variable_is_cleared_for_spawned_children() {
    assert!(
        BEHAVIOR_ENV_VARS.contains(&"IAD_READ_ONLY"),
        "IAD_READ_ONLY changes what the server does; a spawn helper must clear it"
    );
}

// ── Strict read-only and irisAudit (T178) ────────────────────────────────────────────────────────
//
// The audit emission compiles a scratch class on the server, so asking for both is asking iad to
// write to a server it was told to leave alone. Neither setting is dropped quietly: the startup
// check reports a configuration error that names both.

const AUDIT_ON: &str = "[policy.default]\nirisAudit = true\n";

fn conflict(snap: &ConfigSnapshot) -> Option<String> {
    snap.gate_error().map(|e| format!("{}: {e}", e.code()))
}

#[test]
fn strict_with_iris_audit_is_a_configuration_error_naming_both_settings() {
    for (label, op, ws, launch) in [
        (
            "both in the operator file",
            Some(format!("read_only = \"strict\"\n{AUDIT_ON}")),
            None,
            false,
        ),
        (
            "strict from the launch variable, audit in the operator file",
            Some(AUDIT_ON.to_string()),
            None,
            true,
        ),
        (
            "strict in the project file, audit in the operator file",
            Some(AUDIT_ON.to_string()),
            Some("read_only = \"strict\"\n".to_string()),
            false,
        ),
        (
            "audit on a named server",
            Some(
                "read_only = \"strict\"\n[instance.prod]\nhost = \"127.0.0.1\"\nweb_port = 1\n\
                 [policy.prod]\nirisAudit = true\n"
                    .to_string(),
            ),
            None,
            false,
        ),
    ] {
        let snap = load(op.as_deref(), ws.as_deref(), launch);
        let msg = conflict(&snap).unwrap_or_else(|| panic!("{label}: no configuration error"));
        assert!(
            msg.contains("read_only") && msg.contains("strict") && msg.contains("irisAudit"),
            "{label}: the error must name both settings: {msg}"
        );
    }
}

#[test]
fn either_setting_alone_is_fine() {
    let strict_only = load(Some("read_only = \"strict\"\n"), None, false);
    assert_eq!(conflict(&strict_only), None);
    let audit_only = load(Some(AUDIT_ON), None, false);
    assert_eq!(conflict(&audit_only), None);
    let audit_off = load(
        Some("read_only = \"strict\"\n[policy.default]\nirisAudit = false\n"),
        None,
        false,
    );
    assert_eq!(conflict(&audit_off), None);
}

/// `check_config` reports the same conflict, under `config_errors`, whichever way the session got
/// there (a hot reload keeps the last good snapshot and cannot exit).
#[tokio::test]
async fn check_config_reports_the_conflict() {
    use crate::support_gate_141::{body_of, rig};
    let r = rig(&format!("read_only = \"strict\"\n{AUDIT_ON}"));
    let result = r
        .tools
        .call_for_test("check_config", serde_json::json!({}))
        .await
        .expect("check_config answers");
    let body = body_of(&result);
    let errors = body["config_errors"].to_string();
    assert!(
        errors.contains("read_only") && errors.contains("irisAudit"),
        "config_errors must name both settings: {body}"
    );

    let r = rig("read_only = \"strict\"\n");
    let result = r
        .tools
        .call_for_test("check_config", serde_json::json!({}))
        .await
        .expect("check_config answers");
    let body = body_of(&result);
    assert!(
        body["config_errors"]
            .as_array()
            .is_some_and(|a| a.is_empty()),
        "strict alone is not an error: {body}"
    );
}
