//! Spec 141 T203 (clarification Q18): `read_only = "strict"` closes the write and destructive gates
//! of every server, and outranks every setting that would open them.
//!
//! Strict used to switch off the scratch-class tools and nothing else, so an operator who set it on
//! a Development instance, expecting iad to touch nothing, still had `iris_doc` put and
//! `global_kill` working. The gates are resolved as data in `write_gate`; these tests read that
//! resolution from TOML strings and then send calls down `call_gated`, where the refusal has to
//! come from the gate: nothing listens at the servers' addresses, so a call that got past it would
//! fail with a connection error, and a connection error is never an accepted answer.

use crate::support_gate_141::*;
use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::SystemMode;
use iris_agentic_dev_core::tools::write_gate::{
    resolve_declared, resolve_member, GateSource, MemberDeclaration, OperatorEnvGates,
};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};
use std::path::Path;

/// The operator file asks for strict and, in the same breath, for every gate open.
const STRICT_AND_OPEN: &str = r#"
read_only = "strict"
write_tools_enabled = true
destructive_tools_enabled = true
"#;

/// The same file without strict: the control, so a closed gate below is strict's doing.
const OPEN: &str = r#"
write_tools_enabled = true
destructive_tools_enabled = true
"#;

const PROD_OPEN: &str = r#"
mode = "operate"

[instance.prod]
host = "127.0.0.1"
web_port = 1

[policy.prod]
mcpTemplate = "dev"
writeTools = true
destructiveTools = true
"#;

fn load(operator: Option<&str>, workspace: Option<&str>, launch_strict: bool) -> ConfigSnapshot {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = home.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let put = |dir: &Path, body: &str| std::fs::write(dir.join(CONFIG_FILE_NAME), body).unwrap();
    if let Some(b) = operator {
        put(&home, b);
    }
    if let Some(b) = workspace {
        put(&proj, b);
    }
    ConfigSnapshot::load(&LoadInputs {
        cwd: std::fs::canonicalize(&proj).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        launch_read_only_strict: launch_strict,
        ..Default::default()
    })
}

/// The operator also exported both gate variables and the legacy override, the loosest launch
/// settings there are.
fn loosest_launch() -> OperatorEnvGates {
    OperatorEnvGates {
        write_tools_enabled: Some(true),
        destructive_enabled: Some(true),
        allow_prod: true,
    }
}

// ── TOML round-trips ────────────────────────────────────────────────────────────────────────────

#[test]
fn strict_with_both_gates_declared_open_resolves_closed() {
    let snap = load(Some(STRICT_AND_OPEN), None, false);
    for mode in [
        SystemMode::Development,
        SystemMode::Test,
        SystemMode::Live,
        SystemMode::Unknown,
        SystemMode::Unset,
    ] {
        let g = resolve_declared(
            &OperatorEnvGates::default(),
            snap.declared_gates(),
            mode,
            "USER",
        );
        assert!(!g.write_enabled, "{mode:?}: write gate must be closed");
        assert!(
            !g.destructive_enabled,
            "{mode:?}: destructive must be closed"
        );
        assert_eq!(g.write_source, GateSource::ReadOnlyStrict, "{mode:?}");
        assert_eq!(g.destructive_source, GateSource::ReadOnlyStrict, "{mode:?}");
    }
}

#[test]
fn without_strict_the_same_file_opens_both_gates() {
    // The control: the file alone opens the gates, so the test above can fail.
    let snap = load(Some(OPEN), None, false);
    let g = resolve_declared(
        &OperatorEnvGates::default(),
        snap.declared_gates(),
        SystemMode::Development,
        "USER",
    );
    assert!(g.write_enabled && g.destructive_enabled, "{g:?}");
    assert_eq!(g.write_source, GateSource::OperatorFile);
}

#[test]
fn strict_outranks_the_operator_environment_and_the_legacy_override() {
    let snap = load(Some(STRICT_AND_OPEN), None, false);
    let g = resolve_declared(
        &loosest_launch(),
        snap.declared_gates(),
        SystemMode::Development,
        "USER",
    );
    assert!(!g.write_enabled, "{g:?}");
    assert!(!g.destructive_enabled, "{g:?}");
    assert_eq!(g.write_source, GateSource::ReadOnlyStrict);
}

#[test]
fn strict_from_any_source_closes_the_gates() {
    // An untrusted project file may set strict, since it only tightens; launch settings may too.
    for (label, op, ws, launch) in [
        ("operator file", Some(STRICT_AND_OPEN), None, false),
        (
            "project file",
            Some(OPEN),
            Some("read_only = \"strict\"\n"),
            false,
        ),
        ("launch settings", Some(OPEN), None, true),
        (
            "project file that says off, operator file strict",
            Some(STRICT_AND_OPEN),
            Some("read_only = \"off\"\n"),
            false,
        ),
    ] {
        let snap = load(op, ws, launch);
        let g = resolve_declared(
            &loosest_launch(),
            snap.declared_gates(),
            SystemMode::Development,
            "USER",
        );
        assert!(!g.write_enabled, "{label}: {g:?}");
        assert!(!g.destructive_enabled, "{label}: {g:?}");
        assert_eq!(g.write_source, GateSource::ReadOnlyStrict, "{label}");
    }
}

#[test]
fn a_value_that_is_not_strict_closes_nothing() {
    let snap = load(Some(&format!("read_only = \"off\"\n{OPEN}")), None, false);
    let g = resolve_declared(
        &OperatorEnvGates::default(),
        snap.declared_gates(),
        SystemMode::Development,
        "USER",
    );
    assert!(g.write_enabled && g.destructive_enabled, "{g:?}");
}

#[test]
fn strict_closes_a_pool_member_whose_policy_opens_it() {
    let toml = format!("read_only = \"strict\"\n{PROD_OPEN}");
    let snap = load(Some(&toml), None, false);
    let policy = snap.effective_policy("prod");
    assert_eq!(
        policy.write_tools.value,
        Some(true),
        "the control: prod declares writes open"
    );
    let decl = MemberDeclaration {
        top_level: snap.declared_gates(),
        write_tools: policy.write_tools.value,
        destructive_tools: policy.destructive_tools.value,
    };
    for mode in [
        SystemMode::Development,
        SystemMode::Test,
        SystemMode::Unknown,
    ] {
        let g = resolve_member(&loosest_launch(), &decl, mode, "USER");
        assert!(!g.write_enabled, "{mode:?}: {g:?}");
        assert!(!g.destructive_enabled, "{mode:?}: {g:?}");
        assert_eq!(g.write_source, GateSource::ReadOnlyStrict, "{mode:?}");
        assert_eq!(g.destructive_source, GateSource::ReadOnlyStrict, "{mode:?}");
    }
    // Without strict the same member opens.
    let snap = load(Some(PROD_OPEN), None, false);
    let policy = snap.effective_policy("prod");
    let decl = MemberDeclaration {
        top_level: snap.declared_gates(),
        write_tools: policy.write_tools.value,
        destructive_tools: policy.destructive_tools.value,
    };
    let g = resolve_member(
        &OperatorEnvGates::default(),
        &decl,
        SystemMode::Development,
        "USER",
    );
    assert!(g.write_enabled && g.destructive_enabled, "{g:?}");
}

// ── through the pipeline, both spellings ────────────────────────────────────────────────────────

/// What the gate said: the code, and that the gate (not the scratch-class check after it) said it.
fn assert_gate_refusal(label: &str, body: &Value, gate: &str) {
    assert_eq!(
        body["error_code"], "READ_ONLY_STRICT",
        "{label}: expected strict read-only's refusal. Body: {body}"
    );
    assert_eq!(
        body["gate"], gate,
        "{label}: the refusal must come from the {gate} gate, not the scratch-class check. Body: {body}"
    );
    assert_eq!(
        body["gate_source"], "read_only_strict",
        "{label}: it names what closed the gate. Body: {body}"
    );
    let text = body.to_string();
    assert!(
        !text.contains("error sending request") && body["error_code"] != "IRIS_UNREACHABLE",
        "{label}: the call reached the network: {body}"
    );
}

/// [`assert_gate_refusal`] for a body the pipeline classified: an operator limit, because the
/// operator lifts it by removing the setting.
fn assert_pipeline_refusal(label: &str, body: &Value, gate: &str) {
    assert_gate_refusal(label, body, gate);
    assert_eq!(body["refusal"]["kind"], "operator_limit", "{label}: {body}");
}

async fn call(rig: &Rig, tool: &str, args: Value) -> Value {
    let r = rig.tools.call_gated(tool, args).await.unwrap();
    body_of(&r)
}

#[tokio::test]
async fn iris_doc_put_is_refused_by_the_write_gate_in_every_spelling() {
    let rig = rig(STRICT_AND_OPEN);
    for (key, value) in [
        ("mode", "put"),
        ("mode", "PUT"),
        ("mode", " put "),
        ("action", "put"),
        ("action", "Put"),
    ] {
        let mut args = json!({"name": "Strict.T203.cls", "content": "Class Strict.T203 {}"});
        args[key] = json!(value);
        let b = call(&rig, "iris_doc", args).await;
        assert_pipeline_refusal(&format!("{key}={value:?}"), &b, "write_tools");
    }
    // With `server` named, both spellings of it.
    for server in ["dev", " dev "] {
        let b = call(
            &rig,
            "iris_doc",
            json!({"mode": "put", "name": "Strict.T203.cls", "content": "x", "server": server}),
        )
        .await;
        assert_pipeline_refusal(&format!("server={server:?}"), &b, "write_tools");
    }
}

#[tokio::test]
async fn global_kill_is_refused_by_the_write_gate_not_the_destructive_tier() {
    // Destructive is a subset of write, and the write gate is checked first: the more fundamental
    // refusal. Both are closed by strict, so the agent hears about the write gate.
    let rig = rig(STRICT_AND_OPEN);
    for global in ["^Strict.T203", "Strict.T203", " ^Strict.T203 "] {
        let b = call(
            &rig,
            "global_kill",
            json!({"global": global, "confirm_token": "x"}),
        )
        .await;
        assert_pipeline_refusal(&format!("global={global:?}"), &b, "write_tools");
    }
}

#[test]
fn the_destructive_gate_names_strict_when_strict_closed_it() {
    // Strict closes both gates together, so through the pipeline the write gate answers first.
    // The destructive refusal is reached only on a resolution where writes are open and the tier
    // is closed by strict; build that one directly so the wording and the code are pinned.
    use iris_agentic_dev_core::tools::write_gate::{gate_for_class, GateResolution, WriteClass};
    let gates = GateResolution {
        write_enabled: true,
        write_source: GateSource::OperatorFile,
        destructive_enabled: false,
        destructive_source: GateSource::ReadOnlyStrict,
    };
    let refused = gate_for_class("global_kill", WriteClass::Destructive, &gates)
        .expect("a closed destructive gate refuses")
        .expect("a refusal is a result, not a protocol error");
    let b = body_of(&refused);
    assert_gate_refusal("destructive", &b, "destructive_tools");
}

#[tokio::test]
async fn a_named_server_whose_policy_opens_its_gates_is_closed_too() {
    let toml = format!("read_only = \"strict\"\n{PROD_OPEN}");
    let rig = rig_with(&toml, Toolset::Merged, &[("prod", UNROUTABLE)]);
    for (tool, args) in [
        (
            "iris_doc",
            json!({"mode": "put", "name": "Strict.T203.cls", "content": "x", "server": "prod"}),
        ),
        (
            "iris_doc",
            json!({"action": "put", "name": "Strict.T203.cls", "content": "x", "server": " prod "}),
        ),
        (
            "global_kill",
            json!({"global": "^Strict.T203", "confirm_token": "x", "server": "prod"}),
        ),
    ] {
        let b = call(&rig, tool, args.clone()).await;
        assert_pipeline_refusal(&format!("{tool} {args}"), &b, "write_tools");
        assert_eq!(b["server_name"], "prod", "{b}");
    }
}

#[tokio::test]
async fn launch_settings_close_the_gates_through_the_pipeline() {
    let rig = rig_launched(OPEN, Toolset::Merged, &[], true);
    let b = call(
        &rig,
        "iris_doc",
        json!({"mode": "put", "name": "Strict.T203.cls", "content": "x"}),
    )
    .await;
    assert_pipeline_refusal("IAD_READ_ONLY=strict", &b, "write_tools");
}

#[tokio::test]
async fn without_strict_the_same_calls_get_past_the_gate() {
    // The control for the tests above: with the file alone, the gate is open and the call reaches
    // the network, where nothing listens.
    let rig = rig(OPEN);
    let b = call(
        &rig,
        "iris_doc",
        json!({"mode": "put", "name": "Strict.T203.cls", "content": "x"}),
    )
    .await;
    assert_ne!(b["error_code"], "READ_ONLY_STRICT", "{b}");
    assert_ne!(b["error_code"], "WRITE_TOOLS_DISABLED", "{b}");
}

#[tokio::test]
async fn reads_still_work_under_strict() {
    let rig = rig(STRICT_AND_OPEN);
    let b = call(&rig, "check_config", json!({})).await;
    assert_ne!(b["error_code"], "READ_ONLY_STRICT", "{b}");
    let b = call(&rig, "iris_servers", json!({})).await;
    assert_ne!(b["error_code"], "READ_ONLY_STRICT", "{b}");
}

// ── check_config ────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn check_config_reports_the_gates_closed_and_why() {
    let rig = rig(STRICT_AND_OPEN);
    let b = call(&rig, "check_config", json!({})).await;
    assert_eq!(b["write_tools_enabled"], false, "{b}");
    assert_eq!(b["write_tools_source"], "read_only_strict", "{b}");
    assert_eq!(b["destructive_tools_enabled"], false, "{b}");
    assert_eq!(b["destructive_tools_source"], "read_only_strict", "{b}");
    assert_eq!(b["read_only"], "strict", "{b}");
    let default = &b["effective_policy"]["default"];
    assert_eq!(default["writeTools"]["value"], false, "{b}");
    assert_eq!(default["writeTools"]["from"], "read_only_strict", "{b}");
    assert_eq!(default["destructiveTools"]["value"], false, "{b}");
}

#[tokio::test]
async fn check_config_reports_a_pool_member_closed_by_strict() {
    let toml = format!("read_only = \"strict\"\n{PROD_OPEN}");
    let rig = rig_with(&toml, Toolset::Merged, &[("prod", UNROUTABLE)]);
    let b = call(&rig, "check_config", json!({})).await;
    let prod = &b["effective_policy"]["prod"];
    assert_eq!(prod["writeTools"]["value"], false, "{b}");
    assert_eq!(prod["writeTools"]["from"], "read_only_strict", "{b}");
    assert_eq!(prod["destructiveTools"]["value"], false, "{b}");
}
