//! Strict read-only with `irisAudit = true` is refused, on every path (1.7.1, S17).
//!
//! Writing each audit record compiles a class on the server, and strict read-only means iad runs
//! nothing there, so the two settings cannot both hold. The MCP server already refused to start on
//! the combination. The CLI's `tool` command, a session whose configuration only becomes readable
//! after startup, and the tools that answer whatever the configuration says never asked, so a call
//! there could write the audit class under strict.
//!
//! Nothing listens at the rig's addresses, so a call that got past the refusal fails with a
//! connection error or answers from the bundled skills, never with the refusal code.

use crate::support_gate_141::*;
use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::tools::write_gate::GateConfigError;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};
use std::path::PathBuf;

const STRICT: &str = "read_only = \"strict\"\n";
const AUDIT: &str = "[policy.default]\nirisAudit = true\n";
const CODE: &str = "READ_ONLY_STRICT_WITH_AUDIT";

struct Loaded {
    snap: ConfigSnapshot,
    operator: PathBuf,
    project: PathBuf,
    _tmp: tempfile::TempDir,
}

/// The configuration a process started in `proj` would read, from the strings as written.
fn load(operator: &str, project: Option<&str>, launch_strict: bool) -> Loaded {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let home = root.join("home");
    let proj = root.join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(home.join(CONFIG_FILE_NAME), operator).unwrap();
    if let Some(p) = project {
        std::fs::write(proj.join(CONFIG_FILE_NAME), p).unwrap();
    }
    let inputs = LoadInputs {
        cwd: proj.clone(),
        home: Some(home.clone()),
        launch_read_only_strict: launch_strict,
        ..Default::default()
    };
    Loaded {
        snap: ConfigSnapshot::load(&inputs),
        operator: home.join(CONFIG_FILE_NAME),
        project: proj.join(CONFIG_FILE_NAME),
        _tmp: tmp,
    }
}

/// The combination is refused, and the message names both settings and where each one lives.
fn assert_conflict(what: &str, l: &Loaded, strict_from: &str, audit_server: &str, audit_in: &str) {
    assert_eq!(
        l.snap.gate_error(),
        Some(GateConfigError::ReadOnlyStrictWithAudit),
        "{what}"
    );
    let msg = l
        .snap
        .strict_audit_conflict()
        .unwrap_or_else(|| panic!("{what}: no conflict message"));
    for needle in [
        "read_only = \"strict\"",
        strict_from,
        "irisAudit = true",
        &format!("[policy.{audit_server}]"),
        audit_in,
        "Remove one of the two",
    ] {
        assert!(
            msg.contains(needle),
            "{what}: {needle:?} missing from {msg}"
        );
    }
}

// ── layer 1: the configuration as written ───────────────────────────────────────────────────────

#[test]
fn both_in_the_operator_file() {
    let l = load(&format!("{STRICT}{AUDIT}"), None, false);
    let op = l.operator.display().to_string();
    assert_conflict("operator file", &l, &op, "default", &op);
}

#[test]
fn strict_from_the_launch_environment() {
    let l = load(AUDIT, None, true);
    let op = l.operator.display().to_string();
    assert_conflict("IAD_READ_ONLY", &l, "IAD_READ_ONLY", "default", &op);
}

#[test]
fn audit_from_the_project_file() {
    // Audit is a tightening setting, so a project file sets it without trust.
    let l = load(STRICT, Some(AUDIT), false);
    let op = l.operator.display().to_string();
    let proj = l.project.display().to_string();
    assert_conflict("project audit", &l, &op, "default", &proj);
}

#[test]
fn strict_from_the_project_file() {
    let l = load(AUDIT, Some(STRICT), false);
    let op = l.operator.display().to_string();
    let proj = l.project.display().to_string();
    assert_conflict("project strict", &l, &proj, "default", &op);
}

#[test]
fn audit_on_another_server() {
    let l = load(
        &format!("{STRICT}[policy.prod]\nirisAudit = true\n"),
        None,
        false,
    );
    let op = l.operator.display().to_string();
    assert_conflict("prod audit", &l, &op, "prod", &op);
}

#[test]
fn either_alone_is_fine() {
    for (what, operator, launch) in [
        ("strict alone", STRICT, false),
        ("launch strict alone", "", true),
        ("audit alone", AUDIT, false),
        (
            "audit with read_only off",
            "read_only = \"off\"\n[policy.default]\nirisAudit = true\n",
            false,
        ),
        (
            "strict with audit false",
            "read_only = \"strict\"\n[policy.default]\nirisAudit = false\n",
            false,
        ),
    ] {
        let l = load(operator, None, launch);
        assert_eq!(l.snap.gate_error(), None, "{what}");
        assert_eq!(l.snap.strict_audit_conflict(), None, "{what}");
    }
}

// ── the pipeline: a session built on the combination refuses ───────────────────────────────────

/// The answer and whether it is an error. A call that went past every gate fails on the network.
async fn call(rig: &Rig, tool: &str, args: Value) -> (Value, bool) {
    match rig.tools.call_gated(tool, args).await {
        Ok(r) => (body_of(&r), r.is_error == Some(true)),
        Err(e) => (json!({"error": e, "went_past_the_gates": true}), true),
    }
}

fn combo_rigs() -> Vec<(&'static str, Rig)> {
    let both = format!("{STRICT}{AUDIT}");
    vec![
        (
            "baseline",
            rig_with_skills(&both, Toolset::Baseline, &[("prod", UNROUTABLE)], false),
        ),
        (
            "merged",
            rig_with_skills(&both, Toolset::Merged, &[("prod", UNROUTABLE)], false),
        ),
        (
            "launch strict",
            rig_with_skills(AUDIT, Toolset::Merged, &[("prod", UNROUTABLE)], true),
        ),
    ]
}

#[tokio::test]
async fn every_iris_tool_is_refused() {
    for (what, rig) in combo_rigs() {
        for (tool, args) in [
            ("iris_query", json!({"query": "SELECT 1"})),
            ("iris_execute", json!({"code": "Write 1"})),
            (
                "iris_doc",
                json!({"mode": "get", "name": "%Library.String.cls"}),
            ),
            ("iris_info", json!({"what": "version"})),
            ("kb", json!({"action": "recall", "query": "x"})),
        ] {
            let (body, is_error) = call(&rig, tool, args).await;
            assert_eq!(body["error_code"], CODE, "{what} {tool}: {body}");
            assert!(is_error, "{what} {tool}: a refusal is isError: {body}");
            let msg = body["error"].as_str().unwrap_or_default();
            assert!(
                msg.contains("irisAudit") && msg.contains("read_only"),
                "{what} {tool}: the message names both settings: {msg}"
            );
        }
    }
}

#[tokio::test]
async fn the_skill_readers_serve_the_bundled_skills() {
    for (what, rig) in combo_rigs() {
        let (body, _) = call(&rig, "skill_list", json!({})).await;
        assert!(body["error_code"].is_null(), "{what}: {body}");
        assert_eq!(body["sources"]["synthesized"]["searched"], false, "{what}");
        assert_eq!(
            body["sources"]["synthesized"]["refused"]["error_code"], CODE,
            "{what}: {body}"
        );
    }
}

#[tokio::test]
async fn check_config_reports_it() {
    for (what, rig) in combo_rigs() {
        let (body, _) = call(&rig, "check_config", json!({})).await;
        let errors = body["config_errors"].to_string();
        assert!(errors.contains(CODE), "{what}: {body}");
    }
}

#[tokio::test]
async fn without_audit_strict_still_lets_reads_through() {
    // The control: the refusals above are the combination's doing, not strict's.
    let rig = rig_with_skills(STRICT, Toolset::Merged, &[("prod", UNROUTABLE)], false);
    let (body, _) = call(&rig, "iris_query", json!({"query": "SELECT 1"})).await;
    assert_eq!(body["went_past_the_gates"], true, "{body}");
}
