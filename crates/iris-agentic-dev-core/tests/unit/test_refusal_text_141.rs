//! Spec 141 FR-039 as amended 2026-10-24 (T192): no refusal returned to the agent names the
//! operator file's path or the edit that lifts it.
//!
//! An agent with a file-write tool read "set write_tools_enabled = true in ~/.iris-agentic-dev.toml"
//! as the next step and took it. A refusal now carries its `kind` and the `setting` it is about; the
//! operator learns the rest from stderr and from `iris-agentic-dev trust --show`. This file pins the
//! agent-facing half: the text of every refusal builder, the `lift` object, and a scan of the source
//! for any new string that hands the recipe back.

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::workspace_config::{DataPolicy, McpTemplate};
use iris_agentic_dev_core::policy::data_policy_gate::check_bulk_phi_gate;
use iris_agentic_dev_core::policy::refusal::{classify_refusal, refusal_remedy};
use iris_agentic_dev_core::policy::system_blocklist_gate::check_system_blocklist;
use iris_agentic_dev_core::testing::gated;
use iris_agentic_dev_core::tools::exec::MarkerError;
use iris_agentic_dev_core::tools::write_gate::{gate_check, GateResolution, GateSource};
use serde_json::{json, Value};

/// Every string in a JSON value, joined.
fn all_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(all_text).collect::<Vec<_>>().join("\n"),
        Value::Object(o) => o.values().map(all_text).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn result_text(result: rmcp::model::CallToolResult) -> String {
    assert_eq!(
        result.is_error,
        Some(true),
        "a refusal is an isError result"
    );
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn args(v: Value) -> serde_json::Map<String, Value> {
    v.as_object().unwrap().clone()
}

/// What an agent must never be handed: the file, the word for it, the trust command, or a
/// `key = value` line it could paste into one.
fn assert_no_recipe(what: &str, text: &str) {
    for needle in [
        ".iris-agentic-dev.toml",
        "operator file",
        "iris-agentic-dev trust",
        " = true",
        " = false",
        " = \"",
    ] {
        assert!(
            !text.contains(needle),
            "{what} hands the agent a recipe ({needle:?}): {text}"
        );
    }
}

#[test]
fn the_write_and_destructive_gates_carry_no_recipe_for_any_source() {
    // One row per variant, so a new source is a compile error here until it is listed.
    let sources = [
        GateSource::OperatorEnv,
        GateSource::ConfigFile,
        GateSource::OperatorFile,
        GateSource::LegacyAllowProd,
        GateSource::InferredSystemMode,
        GateSource::InferredNamespace,
        GateSource::InferredDefault,
        GateSource::FailClosed,
        GateSource::PolicyDeclaration,
        GateSource::UnprobedMember,
        GateSource::ReadOnlyStrict,
    ];
    for s in sources {
        match s {
            GateSource::OperatorEnv
            | GateSource::ConfigFile
            | GateSource::OperatorFile
            | GateSource::LegacyAllowProd
            | GateSource::InferredSystemMode
            | GateSource::InferredNamespace
            | GateSource::InferredDefault
            | GateSource::FailClosed
            | GateSource::PolicyDeclaration
            | GateSource::UnprobedMember
            | GateSource::ReadOnlyStrict => {}
        }
        let strict = s == GateSource::ReadOnlyStrict;
        let closed = GateResolution {
            write_enabled: false,
            write_source: s,
            destructive_enabled: false,
            destructive_source: s,
        };
        let refused = gate_check("iris_doc", Some(&args(json!({"mode": "put"}))), &closed)
            .expect("a write with writes off is refused")
            .expect("a refusal is a result");
        let t = result_text(refused);
        let code = if strict {
            "READ_ONLY_STRICT"
        } else {
            "WRITE_TOOLS_DISABLED"
        };
        assert!(t.contains(code), "{t}");
        assert_no_recipe(&format!("the write gate ({})", s.as_str()), &t);

        let open_writes = GateResolution {
            write_enabled: true,
            write_source: GateSource::OperatorEnv,
            destructive_enabled: false,
            destructive_source: s,
        };
        let refused = gate_check(
            "global_kill",
            Some(&args(json!({"global": "Zed"}))),
            &open_writes,
        )
        .expect("a kill with the destructive tier off is refused")
        .unwrap();
        let t = result_text(refused);
        let code = if strict {
            "READ_ONLY_STRICT"
        } else {
            "DESTRUCTIVE_TOOLS_DISABLED"
        };
        assert!(t.contains(code), "{t}");
        assert_no_recipe(&format!("the destructive gate ({})", s.as_str()), &t);
    }
}

#[test]
fn the_policy_gates_carry_no_recipe() {
    let env = gated::env("iris_execute", &McpTemplate::Live, "prod", &json!({}))
        .expect("live blocks execute");
    assert_eq!(env["error_code"], "ENV_GATE_BLOCKED");
    assert_no_recipe("the environment gate", &all_text(&env));

    let data = check_bulk_phi_gate("iris_message_body", &DataPolicy::Block, "prod")
        .expect("block refuses a bulk-data tool");
    assert_eq!(data["error_code"], "DATA_POLICY_BLOCKED");
    assert_no_recipe("the data policy gate", &all_text(&data));

    let custom = check_system_blocklist("^MineThing", &["^Mine*".to_string()], &[], false, "prod")
        .expect("a custom pattern blocks");
    assert_eq!(custom["error_code"], "SYSTEM_BLOCKLIST");
    assert_no_recipe("the custom blocklist", &all_text(&custom));
}

#[test]
fn the_identity_refusals_carry_no_recipe() {
    for e in [
        MarkerError::LoginFailed,
        MarkerError::TerminalLoginRequired,
        MarkerError::TerminalDenied,
        MarkerError::Mismatch {
            ran_as: "_SYSTEM".to_string(),
        },
    ] {
        assert_no_recipe(e.code(), &e.message(Some("svc")));
    }
}

fn snapshot(op: &str, ws: &str) -> (ConfigSnapshot, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    if !op.is_empty() {
        std::fs::write(home.join(CONFIG_FILE_NAME), op).unwrap();
    }
    std::fs::write(proj.join(CONFIG_FILE_NAME), ws).unwrap();
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: std::fs::canonicalize(&proj).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    });
    (snap, tmp)
}

fn write_body() -> Value {
    json!({"success": false, "error_code": "WRITE_TOOLS_DISABLED", "message": "writes are off"})
}

#[test]
fn an_operator_limit_lift_names_the_setting_and_nothing_else() {
    let (snap, _t) = snapshot("write_tools_enabled = false\n", "namespace = \"USER\"\n");
    let out = classify_refusal(write_body(), "default", &snap.set_aside);
    assert_eq!(out["refusal"]["kind"], "operator_limit", "{out}");
    let lift = out["refusal"]["lift"].as_object().expect("lift object");
    assert_eq!(lift.get("setting"), Some(&json!("write_tools_enabled")));
    assert_eq!(
        lift.len(),
        1,
        "`file` and `how` are the recipe; they go to stderr: {out}"
    );
    assert_no_recipe("the classified refusal", &all_text(&out));
}

#[test]
fn an_untrusted_setting_lift_names_the_setting_and_points_at_its_set_aside_item() {
    let (snap, _t) = snapshot("", "write_tools_enabled = true\nnamespace = \"PROD\"\n");
    let out = classify_refusal(write_body(), "default", &snap.set_aside);
    assert_eq!(out["refusal"]["kind"], "untrusted_setting", "{out}");
    assert_eq!(out["refusal"]["set_aside"], "workspace:write_tools_enabled");
    let lift = out["refusal"]["lift"].as_object().unwrap();
    assert_eq!(lift.len(), 1, "{out}");
    assert_eq!(lift["setting"], "write_tools_enabled");
    assert_no_recipe("the classified refusal", &all_text(&out));
}

#[test]
fn the_remedy_for_the_operator_names_the_file_and_the_edit() {
    let (snap, _t) = snapshot("write_tools_enabled = false\n", "namespace = \"USER\"\n");
    let out = classify_refusal(write_body(), "default", &snap.set_aside);
    let remedy = refusal_remedy(&out, snap.operator_path.as_deref(), &snap.set_aside)
        .expect("an operator limit has a remedy");
    let op = snap.operator_path.as_ref().unwrap().display().to_string();
    assert!(remedy.line.contains(&op), "{}", remedy.line);
    assert!(
        remedy.line.contains("write_tools_enabled = true"),
        "{}",
        remedy.line
    );
    assert!(remedy.line.contains("restart"), "{}", remedy.line);
    assert_eq!(remedy.key, "default:write_tools_enabled");

    // An acknowledgement and a fixed rule have no remedy for the operator to carry out.
    let ack = json!({"success": false, "error_code": "PHI_GATE_BLOCKED", "error": "x"});
    let ack = classify_refusal(ack, "default", &snap.set_aside);
    assert!(refusal_remedy(&ack, snap.operator_path.as_deref(), &snap.set_aside).is_none());
}

#[test]
fn the_remedy_for_an_untrusted_setting_is_the_trust_command_and_a_restart() {
    let (snap, _t) = snapshot("", "write_tools_enabled = true\nnamespace = \"PROD\"\n");
    let out = classify_refusal(write_body(), "default", &snap.set_aside);
    let remedy = refusal_remedy(&out, snap.operator_path.as_deref(), &snap.set_aside).unwrap();
    assert!(
        remedy.line.contains("iris-agentic-dev trust"),
        "{}",
        remedy.line
    );
    assert!(remedy.line.contains("restart"), "{}", remedy.line);
}

/// Source files whose job is to say the recipe to the operator, on stderr or in the CLI.
const MAY_NAME_THE_OPERATOR_FILE: &[&str] = &[
    // The remedy builder: its output goes to stderr and `trust --show`, never to the agent.
    "policy/refusal.rs",
    // Held-back settings: the line goes to stderr; `check_config` carries "restart iad" only.
    "iris/operator_hold.rs",
    // Test support: panic messages for a developer.
    "testing.rs",
];

fn rust_sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rust_sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// A string literal anywhere in the core crate that names the operator file as a place to edit is
/// a recipe waiting to be returned. Comments are skipped; tests live outside `src` or in
/// `#[cfg(test)]` modules that come after the code.
#[test]
fn no_source_string_hands_the_agent_the_operator_file() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    assert!(files.len() > 50, "the scan found no sources under {src:?}");
    let mut hits = Vec::new();
    for f in &files {
        let rel = f
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if MAY_NAME_THE_OPERATOR_FILE.contains(&rel.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(f).unwrap();
        // Everything from the first `#[cfg(test)]` on is test code.
        let code = text.split("#[cfg(test)]").next().unwrap();
        for (n, line) in code.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            let names_file = line.contains("~/.iris-agentic-dev.toml")
                || line.contains("home-directory .iris-agentic-dev.toml")
                || line.contains("operator file")
                || line.contains("(the operator");
            let in_a_string = line.matches('"').count() >= 1;
            if names_file && in_a_string {
                hits.push(format!("{rel}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "these strings name the operator file; if the agent can read one, it will edit the file:\n{}",
        hits.join("\n")
    );
}
