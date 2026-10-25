//! Spec 141 FR-043 (T193): during a session an edit to the operator file applies at once where it
//! tightens and is held until the next start where it loosens.
//!
//! The cases are TOML strings, applied (what the server runs on) against incoming (what the file
//! says now). Each loosening kind has a row, each tightening kind its reverse, and the snapshot
//! tests at the bottom check the held settings reach `set_aside`.

use iris_agentic_dev_core::iris::config_sources::{
    ConfigSnapshot, LoadInputs, SetAsideKind, TrustState, CONFIG_FILE_NAME,
};
use iris_agentic_dev_core::iris::operator_hold::hold_loosening;

fn table(s: &str) -> toml::Table {
    s.parse().unwrap_or_else(|e| panic!("{e}: {s}"))
}

/// (table the server runs on, ids held back).
fn hold(applied: &str, incoming: &str) -> (toml::Table, Vec<String>) {
    let (t, held) = hold_loosening(&table(applied), table(incoming));
    let mut ids: Vec<String> = held.into_iter().map(|h| h.id).collect();
    ids.sort();
    (t, ids)
}

/// One loosening edit: held under `id`, and the server keeps running on `kept`.
fn assert_held(applied: &str, incoming: &str, id: &str, kept: &str) {
    let (t, ids) = hold(applied, incoming);
    assert_eq!(ids, vec![id.to_string()], "{applied:?} -> {incoming:?}");
    assert_eq!(t, table(kept), "{applied:?} -> {incoming:?}");
}

/// One tightening or neutral edit: nothing held, the incoming table applies as written.
fn assert_applies(applied: &str, incoming: &str) {
    let (t, ids) = hold(applied, incoming);
    assert!(ids.is_empty(), "{applied:?} -> {incoming:?} held {ids:?}");
    assert_eq!(t, table(incoming), "{applied:?} -> {incoming:?}");
}

#[test]
fn the_write_and_destructive_gates_loosen_only_toward_true() {
    assert_held(
        "write_tools_enabled = false\n",
        "write_tools_enabled = true\n",
        "operator:write_tools_enabled",
        "write_tools_enabled = false\n",
    );
    assert_held(
        "namespace = \"USER\"\n",
        "namespace = \"USER\"\nwrite_tools_enabled = true\n",
        "operator:write_tools_enabled",
        "namespace = \"USER\"\n",
    );
    assert_held(
        "destructive_tools_enabled = false\n",
        "destructive_tools_enabled = true\n",
        "operator:destructive_tools_enabled",
        "destructive_tools_enabled = false\n",
    );
    // `false` removed leaves the system mode to decide, which may be open.
    assert_held(
        "write_tools_enabled = false\n",
        "namespace = \"USER\"\n",
        "operator:write_tools_enabled",
        "namespace = \"USER\"\nwrite_tools_enabled = false\n",
    );
    assert_held(
        "[policy.prod]\nwriteTools = false\n",
        "[policy.prod]\nwriteTools = true\n",
        "operator:policy.prod.writeTools",
        "[policy.prod]\nwriteTools = false\n",
    );
    assert_held(
        "[policy.prod]\ndestructiveTools = false\n",
        "[policy.prod]\ndestructiveTools = true\n",
        "operator:policy.prod.destructiveTools",
        "[policy.prod]\ndestructiveTools = false\n",
    );

    assert_applies(
        "write_tools_enabled = true\n",
        "write_tools_enabled = false\n",
    );
    assert_applies("write_tools_enabled = true\n", "namespace = \"USER\"\n");
    assert_applies("namespace = \"USER\"\n", "write_tools_enabled = false\n");
    assert_applies(
        "destructive_tools_enabled = true\n",
        "destructive_tools_enabled = false\n",
    );
    assert_applies(
        "[policy.prod]\nwriteTools = true\n",
        "[policy.prod]\nwriteTools = false\n",
    );
}

#[test]
fn turning_the_compile_time_gate_off_loosens() {
    assert_held(
        "compile_time_gate = true\n",
        "compile_time_gate = false\n",
        "operator:compile_time_gate",
        "compile_time_gate = true\n",
    );
    assert_held(
        "namespace = \"USER\"\n",
        "namespace = \"USER\"\ncompile_time_gate = false\n",
        "operator:compile_time_gate",
        "namespace = \"USER\"\n",
    );
    assert_held(
        "[policy.prod]\ncompileTimeGate = true\n",
        "[policy.prod]\ncompileTimeGate = false\n",
        "operator:policy.prod.compileTimeGate",
        "[policy.prod]\ncompileTimeGate = true\n",
    );
    assert_applies("compile_time_gate = false\n", "compile_time_gate = true\n");
    // The default is on, so removing an explicit `true` changes nothing.
    assert_applies("compile_time_gate = true\n", "namespace = \"USER\"\n");
}

#[test]
fn leaving_strict_read_only_loosens() {
    assert_held(
        "read_only = \"strict\"\n",
        "namespace = \"USER\"\n",
        "operator:read_only",
        "namespace = \"USER\"\nread_only = \"strict\"\n",
    );
    assert_held(
        "read_only = \"strict\"\n",
        "read_only = \"off\"\n",
        "operator:read_only",
        "read_only = \"strict\"\n",
    );
    assert_applies("namespace = \"USER\"\n", "read_only = \"strict\"\n");
    assert_applies("read_only = \"off\"\n", "read_only = \"strict\"\n");
}

#[test]
fn the_environment_template_loosens_toward_dev() {
    assert_held(
        "[policy.prod]\nmcpTemplate = \"live\"\n",
        "[policy.prod]\nmcpTemplate = \"dev\"\n",
        "operator:policy.prod.mcpTemplate",
        "[policy.prod]\nmcpTemplate = \"live\"\n",
    );
    assert_held(
        "[policy.prod]\nmcpTemplate = \"live\"\n",
        "[policy.prod]\nmcpTemplate = \"test\"\n",
        "operator:policy.prod.mcpTemplate",
        "[policy.prod]\nmcpTemplate = \"live\"\n",
    );
    // Absent means the system mode decides, which may be live.
    assert_held(
        "namespace = \"USER\"\n",
        "namespace = \"USER\"\n[policy.prod]\nmcpTemplate = \"dev\"\n",
        "operator:policy.prod.mcpTemplate",
        "namespace = \"USER\"\n",
    );
    assert_held(
        "[policy.prod]\nmcpTemplate = \"test\"\n",
        "namespace = \"USER\"\n",
        "operator:policy.prod.mcpTemplate",
        "namespace = \"USER\"\n[policy.prod]\nmcpTemplate = \"test\"\n",
    );
    assert_applies(
        "[policy.prod]\nmcpTemplate = \"dev\"\n",
        "[policy.prod]\nmcpTemplate = \"live\"\n",
    );
    assert_applies(
        "[policy.prod]\nmcpTemplate = \"test\"\n",
        "[policy.prod]\nmcpTemplate = \"live\"\n",
    );
    assert_applies(
        "namespace = \"USER\"\n",
        "[policy.prod]\nmcpTemplate = \"live\"\n",
    );
    assert_applies(
        "[policy.prod]\nmcpTemplate = \"dev\"\n",
        "namespace = \"USER\"\n",
    );
}

#[test]
fn widening_the_allow_list_loosens_and_narrowing_it_does_not() {
    assert_held(
        "[policy.prod]\nallow = [\"query\"]\n",
        "[policy.prod]\nallow = [\"query\", \"compile\"]\n",
        "operator:policy.prod.allow",
        "[policy.prod]\nallow = [\"query\"]\n",
    );
    // No list means every category.
    assert_held(
        "[policy.prod]\nallow = [\"query\"]\n",
        "namespace = \"USER\"\n",
        "operator:policy.prod.allow",
        "namespace = \"USER\"\n[policy.prod]\nallow = [\"query\"]\n",
    );
    assert_applies(
        "[policy.prod]\nallow = [\"query\", \"compile\"]\n",
        "[policy.prod]\nallow = [\"query\"]\n",
    );
    assert_applies(
        "namespace = \"USER\"\n",
        "[policy.prod]\nallow = [\"query\"]\n",
    );
}

#[test]
fn data_policy_loosens_from_block_toward_allow() {
    assert_held(
        "[policy.prod]\ndataPolicy = \"block\"\n",
        "[policy.prod]\ndataPolicy = \"allow\"\n",
        "operator:policy.prod.dataPolicy",
        "[policy.prod]\ndataPolicy = \"block\"\n",
    );
    assert_held(
        "[policy.prod]\ndataPolicy = \"block\"\n",
        "[policy.prod]\ndataPolicy = \"redact\"\n",
        "operator:policy.prod.dataPolicy",
        "[policy.prod]\ndataPolicy = \"block\"\n",
    );
    assert_held(
        "namespace = \"USER\"\n",
        "namespace = \"USER\"\n[policy.prod]\ndataPolicy = \"allow\"\n",
        "operator:policy.prod.dataPolicy",
        "namespace = \"USER\"\n",
    );
    assert_held(
        "[policy.prod]\ndataPolicy = \"redact\"\n",
        "[policy.prod]\ndataPolicy = \"allow\"\n",
        "operator:policy.prod.dataPolicy",
        "[policy.prod]\ndataPolicy = \"redact\"\n",
    );
    assert_applies(
        "[policy.prod]\ndataPolicy = \"allow\"\n",
        "[policy.prod]\ndataPolicy = \"block\"\n",
    );
    // The default is block, so removing an explicit block changes nothing.
    assert_applies(
        "[policy.prod]\ndataPolicy = \"block\"\n",
        "namespace = \"USER\"\n",
    );
}

#[test]
fn blocklist_entries_and_the_kill_allowlist() {
    assert_held(
        "[policy.prod]\nglobalBlocklist = [\"^A*\", \"^B*\"]\n",
        "[policy.prod]\nglobalBlocklist = [\"^A*\"]\n",
        "operator:policy.prod.globalBlocklist",
        "[policy.prod]\nglobalBlocklist = [\"^A*\", \"^B*\"]\n",
    );
    assert_applies(
        "[policy.prod]\nglobalBlocklist = [\"^A*\"]\n",
        "[policy.prod]\nglobalBlocklist = [\"^A*\", \"^B*\"]\n",
    );
    assert_held(
        "namespace = \"USER\"\n",
        "namespace = \"USER\"\n[policy.prod]\ndataPolicyKillAllowlist = [\"^T*\"]\n",
        "operator:policy.prod.dataPolicyKillAllowlist",
        "namespace = \"USER\"\n",
    );
    assert_held(
        "[policy.prod]\ndataPolicyKillAllowlist = [\"^T*\"]\n",
        "[policy.prod]\ndataPolicyKillAllowlist = [\"^T*\", \"^U*\"]\n",
        "operator:policy.prod.dataPolicyKillAllowlist",
        "[policy.prod]\ndataPolicyKillAllowlist = [\"^T*\"]\n",
    );
    assert_applies(
        "[policy.prod]\ndataPolicyKillAllowlist = [\"^T*\", \"^U*\"]\n",
        "[policy.prod]\ndataPolicyKillAllowlist = [\"^T*\"]\n",
    );
    assert_applies(
        "[policy.prod]\ndataPolicyKillAllowlist = [\"^T*\"]\n",
        "namespace = \"USER\"\n",
    );
}

#[test]
fn audit_and_role() {
    assert_held(
        "[policy.prod]\nirisAudit = true\n",
        "[policy.prod]\nirisAudit = false\n",
        "operator:policy.prod.irisAudit",
        "[policy.prod]\nirisAudit = true\n",
    );
    assert_applies(
        "[policy.prod]\nirisAudit = false\n",
        "[policy.prod]\nirisAudit = true\n",
    );
    assert_held(
        "[instance.prod]\nhost = \"h\"\nrole = \"subject\"\n",
        "[instance.prod]\nhost = \"h\"\nrole = \"workbench\"\n",
        "operator:instance.prod.role",
        "[instance.prod]\nhost = \"h\"\nrole = \"subject\"\n",
    );
    assert_held(
        "[instance.prod]\nhost = \"h\"\nrole = \"subject\"\n",
        "[instance.prod]\nhost = \"h\"\n",
        "operator:instance.prod.role",
        "[instance.prod]\nhost = \"h\"\nrole = \"subject\"\n",
    );
    assert_applies(
        "[instance.prod]\nhost = \"h\"\nrole = \"workbench\"\n",
        "[instance.prod]\nhost = \"h\"\nrole = \"subject\"\n",
    );
}

#[test]
fn connection_loosening_settings() {
    assert_held(
        "host = \"h\"\n",
        "host = \"h\"\ntls_verify = false\n",
        "operator:tls_verify",
        "host = \"h\"\n",
    );
    assert_held(
        "[instance.prod]\nhost = \"h\"\ntls_verify = true\n",
        "[instance.prod]\nhost = \"h\"\ntls_verify = false\n",
        "operator:instance.prod.tls_verify",
        "[instance.prod]\nhost = \"h\"\ntls_verify = true\n",
    );
    assert_held(
        "host = \"h\"\n",
        "host = \"h\"\nssh_host = \"jump\"\n",
        "operator:ssh_host",
        "host = \"h\"\n",
    );
    assert_held(
        "[instance.prod]\nhost = \"h\"\n",
        "[instance.prod]\nhost = \"h\"\nssh_host = \"jump\"\n",
        "operator:instance.prod.ssh_host",
        "[instance.prod]\nhost = \"h\"\n",
    );
    // A service account and its password go together.
    assert_held(
        "[instance.prod]\nhost = \"h\"\n",
        "[instance.prod]\nhost = \"h\"\nservice_username = \"svc\"\nservice_password = \"pw\"\n",
        "operator:instance.prod.service_username",
        "[instance.prod]\nhost = \"h\"\n",
    );
    assert_held(
        "[instance.prod]\nhost = \"h\"\nservice_username = \"svc\"\n",
        "[instance.prod]\nhost = \"h\"\nservice_username = \"root\"\n",
        "operator:instance.prod.service_username",
        "[instance.prod]\nhost = \"h\"\nservice_username = \"svc\"\n",
    );
    assert_held(
        "host = \"h\"\n",
        "host = \"h\"\nservice_username = \"svc\"\n",
        "operator:service_username",
        "host = \"h\"\n",
    );

    assert_applies(
        "host = \"h\"\ntls_verify = false\n",
        "host = \"h\"\ntls_verify = true\n",
    );
    assert_applies("host = \"h\"\ntls_verify = false\n", "host = \"h\"\n");
    assert_applies("host = \"h\"\nssh_host = \"jump\"\n", "host = \"h\"\n");
    assert_applies(
        "[instance.prod]\nhost = \"h\"\nservice_username = \"svc\"\n",
        "[instance.prod]\nhost = \"h\"\n",
    );
}

#[test]
fn operator_only_opt_ins() {
    for key in ["exec_as_os_user", "local_skills", "audit_payloads"] {
        assert_held(
            "namespace = \"USER\"\n",
            &format!("namespace = \"USER\"\n{key} = true\n"),
            &format!("operator:{key}"),
            "namespace = \"USER\"\n",
        );
        assert_held(
            &format!("{key} = false\n"),
            &format!("{key} = true\n"),
            &format!("operator:{key}"),
            &format!("{key} = false\n"),
        );
        assert_applies(&format!("{key} = true\n"), &format!("{key} = false\n"));
        assert_applies(&format!("{key} = true\n"), "namespace = \"USER\"\n");
    }
    assert_held(
        "[policy.prod]\nirisAudit = false\n",
        "[policy.prod]\nirisAudit = false\nexecAsOsUser = true\n",
        "operator:policy.prod.execAsOsUser",
        "[policy.prod]\nirisAudit = false\n",
    );
    assert_applies(
        "[policy.prod]\nexecAsOsUser = true\n",
        "[policy.prod]\nexecAsOsUser = false\n",
    );
    // Removing the transport token would open the HTTP transport.
    assert_held(
        "[transport]\nhttp_token = \"abc\"\n",
        "namespace = \"USER\"\n",
        "operator:transport.http_token",
        "namespace = \"USER\"\n[transport]\nhttp_token = \"abc\"\n",
    );
    assert_applies(
        "namespace = \"USER\"\n",
        "[transport]\nhttp_token = \"abc\"\n",
    );
}

const TRUST_A: &str =
    "[[trust]]\npath = \"/p/a.toml\"\nformat = 1\nsettings = [\"write_tools_enabled = true\"]\n";
const TRUST_A2: &str = "[[trust]]\npath = \"/p/a.toml\"\nformat = 1\nsettings = [\"write_tools_enabled = true\", \"destructive_tools_enabled = true\"]\n";
const TRUST_B: &str =
    "[[trust]]\npath = \"/p/b.toml\"\nformat = 1\nsettings = [\"write_tools_enabled = true\"]\n";

#[test]
fn a_new_or_changed_trust_entry_is_held_and_a_removed_one_applies() {
    // A trust run during the session writes an entry. It loosens, so it waits for the restart.
    assert_held(
        "namespace = \"USER\"\n",
        &format!("namespace = \"USER\"\n{TRUST_A}"),
        "operator:trust:/p/a.toml",
        "namespace = \"USER\"\n",
    );
    assert_held(
        TRUST_A,
        &format!("{TRUST_A}{TRUST_B}"),
        "operator:trust:/p/b.toml",
        TRUST_A,
    );
    // The same path with other settings is a replacement: the entry in force stays in force.
    assert_held(TRUST_A, TRUST_A2, "operator:trust:/p/a.toml", TRUST_A);

    // Revoking tightens.
    assert_applies(&format!("{TRUST_A}{TRUST_B}"), TRUST_B);
    assert_applies(TRUST_A, "namespace = \"USER\"\n");
    // The same entry again is no change.
    assert_applies(TRUST_A, TRUST_A);
}

#[test]
fn connection_details_and_unrelated_keys_apply_at_once() {
    assert_applies(
        "host = \"a\"\nweb_port = 52773\nnamespace = \"USER\"\n",
        "host = \"b\"\nweb_port = 52780\nnamespace = \"APP\"\n",
    );
    assert_applies(
        "container = \"x\"\n",
        "container = \"y\"\n[instance.new]\nhost = \"h\"\n",
    );
}

#[test]
fn a_mixed_edit_applies_what_tightens_and_holds_what_loosens() {
    let (t, ids) = hold(
        "write_tools_enabled = true\n[policy.prod]\nmcpTemplate = \"test\"\ndataPolicy = \"allow\"\n",
        "write_tools_enabled = false\n[policy.prod]\nmcpTemplate = \"dev\"\ndataPolicy = \"block\"\n",
    );
    assert_eq!(ids, vec!["operator:policy.prod.mcpTemplate".to_string()]);
    assert_eq!(
        t,
        table("write_tools_enabled = false\n[policy.prod]\nmcpTemplate = \"test\"\ndataPolicy = \"block\"\n")
    );
}

#[test]
fn holding_is_idempotent_and_leaves_an_unchanged_file_alone() {
    let applied = "write_tools_enabled = false\n[policy.prod]\nmcpTemplate = \"live\"\n";
    assert_applies(applied, applied);
    // The result of one hold is what the next one starts from, and the same edit is held again.
    let incoming = "write_tools_enabled = true\n[policy.prod]\nmcpTemplate = \"dev\"\n";
    let (first, ids) = hold(applied, incoming);
    assert_eq!(ids.len(), 2, "{ids:?}");
    let (second, ids_again) = hold_loosening(&first, table(incoming));
    assert_eq!(first, second);
    assert_eq!(ids_again.len(), 2);
    // Back to what the server runs on: nothing is held any more.
    assert_applies(applied, applied);
}

#[test]
fn a_held_setting_names_the_edit() {
    let (_, held) = hold_loosening(
        &table("[policy.prod]\nmcpTemplate = \"live\"\n"),
        table("[policy.prod]\nmcpTemplate = \"dev\"\n"),
    );
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].setting, "policy.prod.mcpTemplate = \"dev\"");

    let (_, held) = hold_loosening(
        &table("read_only = \"strict\"\n"),
        table("namespace = \"USER\"\n"),
    );
    assert_eq!(held[0].setting, "read_only = \"strict\" removed");
}

// ── The snapshot ───────────────────────────────────────────────────────────────────────────────

fn load_with_baseline(op_now: &str, baseline: &str, ws: Option<&str>) -> ConfigSnapshot {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let home = root.join("home");
    let proj = root.join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(home.join(CONFIG_FILE_NAME), op_now).unwrap();
    if let Some(ws) = ws {
        std::fs::write(proj.join(CONFIG_FILE_NAME), ws).unwrap();
    }
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: proj,
        home: Some(home),
        operator_baseline: Some(table(baseline)),
        ..Default::default()
    });
    // The temp directory goes away with `tmp`; everything read is in the snapshot.
    drop(tmp);
    snap
}

#[test]
fn a_loosened_gate_in_the_file_does_not_reach_the_declared_gates() {
    let snap = load_with_baseline(
        "write_tools_enabled = true\n",
        "write_tools_enabled = false\n",
        None,
    );
    assert_eq!(snap.declared_gates().write_tools_enabled, Some(false));
    let held: Vec<_> = snap
        .set_aside
        .iter()
        .filter(|i| i.kind == SetAsideKind::HeldForRestart)
        .collect();
    assert_eq!(held.len(), 1, "{:?}", snap.set_aside);
    assert_eq!(held[0].id, "operator:write_tools_enabled");
    assert_eq!(held[0].setting, "write_tools_enabled = true");
    assert_eq!(held[0].lift, "restart iad");
    assert!(
        held[0]
            .file
            .as_deref()
            .is_some_and(|f| f.ends_with(CONFIG_FILE_NAME)),
        "{:?}",
        held[0]
    );
    assert_eq!(snap.held.len(), 1);
}

#[test]
fn a_tightened_gate_in_the_file_applies_at_once() {
    let snap = load_with_baseline(
        "write_tools_enabled = false\n",
        "write_tools_enabled = true\n",
        None,
    );
    assert_eq!(snap.declared_gates().write_tools_enabled, Some(false));
    assert!(snap.held.is_empty(), "{:?}", snap.held);
    assert!(snap
        .set_aside
        .iter()
        .all(|i| i.kind != SetAsideKind::HeldForRestart));
}

#[test]
fn no_baseline_means_a_start_and_nothing_is_held() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::create_dir_all(root.join("home")).unwrap();
    std::fs::create_dir_all(root.join("proj")).unwrap();
    std::fs::write(
        root.join("home").join(CONFIG_FILE_NAME),
        "write_tools_enabled = true\n",
    )
    .unwrap();
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: root.join("proj"),
        home: Some(root.join("home")),
        ..Default::default()
    });
    assert_eq!(snap.declared_gates().write_tools_enabled, Some(true));
    assert!(snap.held.is_empty());
}

#[test]
fn a_trust_entry_written_during_the_session_does_not_trust_the_file_until_restart() {
    let ws = "write_tools_enabled = true\nnamespace = \"PROD\"\n";
    let entry =
        "[[trust]]\npath = \"PATH\"\nformat = 1\nsettings = [\"write_tools_enabled = true\"]\n";
    // The entry's path must be the workspace file's canonical path, which only the load knows.
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let (home, proj) = (root.join("home"), root.join("proj"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join(CONFIG_FILE_NAME), ws).unwrap();
    let ws_path = std::fs::canonicalize(proj.join(CONFIG_FILE_NAME)).unwrap();
    let entry = entry.replace("PATH", &ws_path.to_string_lossy());
    std::fs::write(home.join(CONFIG_FILE_NAME), &entry).unwrap();

    let inputs = |baseline: Option<toml::Table>| LoadInputs {
        cwd: proj.clone(),
        home: Some(home.clone()),
        operator_baseline: baseline,
        ..Default::default()
    };
    // Started with no entry: the workspace file's loosening is set aside, the entry is held.
    let during = ConfigSnapshot::load(&inputs(Some(table("namespace = \"USER\"\n"))));
    assert_eq!(during.trust(), TrustState::Untrusted);
    assert_eq!(during.declared_gates().write_tools_enabled, None);
    assert_eq!(during.held.len(), 1, "{:?}", during.held);
    assert!(during.held[0].id.starts_with("operator:trust:"));
    assert!(during
        .set_aside
        .iter()
        .any(|i| i.id == "workspace:write_tools_enabled"));

    // The next start has no baseline and trusts it.
    let restarted = ConfigSnapshot::load(&inputs(None));
    assert_eq!(restarted.trust(), TrustState::Trusted);
    assert_eq!(restarted.declared_gates().write_tools_enabled, Some(true));
}

#[test]
fn a_loosening_workspace_file_keeps_its_own_rule() {
    // The workspace file is not the operator file: an untrusted loosening is set aside until
    // `trust`, and a held operator setting does not change that.
    let snap = load_with_baseline(
        "write_tools_enabled = false\n",
        "write_tools_enabled = false\n",
        Some("write_tools_enabled = true\nnamespace = \"PROD\"\n"),
    );
    assert_eq!(snap.declared_gates().write_tools_enabled, Some(false));
    assert!(snap.held.is_empty());
    assert!(snap
        .set_aside
        .iter()
        .any(|i| i.kind == SetAsideKind::UntrustedSetting));
}
