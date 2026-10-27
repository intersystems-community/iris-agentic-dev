//! Layer 2 for the 1.7.1 gate fixes, over MCP stdio.
//!
//! - S14: `check_config` reports the tools no category gate judges.
//! - S17: the tools that change where later calls go meet `allow` and the environment template of
//!   the default server, before any request reaches IRIS.
//! - R2: so do the skill and knowledge-base tools that run code on the default server.
//!
//! Nothing listens on the default connection (port 2, not 1: `127.0.0.1:1` is the sentinel the
//! NoPWS guard reads as "no web server"). A refusal has to be the gate's answer.

use iris_agentic_dev_core::testing::McpSession;
use serde_json::{json, Value};

fn session(project: &str) -> (McpSession, tempfile::TempDir) {
    let env: Vec<(String, String)> = [
        ("IRIS_HOST", "127.0.0.1"),
        ("IRIS_WEB_PORT", "2"),
        ("IRIS_WRITE_TOOLS_ENABLED", "true"),
        ("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "true"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".iris-agentic-dev.toml"), project).unwrap();
    let cwd = std::fs::canonicalize(dir.path()).unwrap();
    (McpSession::start_in(Some(&cwd), &env), dir)
}

fn body(answer: &Value) -> Value {
    if let Some(v) = answer.pointer("/result/structuredContent") {
        if !v.is_null() {
            return v.clone();
        }
    }
    answer
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_else(|| json!({"rpc": answer}))
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn check_config_reports_the_tools_no_category_gate_judges() {
    let (mut s, _d) = session("");
    let b = body(&s.call("check_config", &json!({})));
    assert_eq!(b["ungated_tools"], json!(["check_config"]), "{b}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_narrow_allow_refuses_the_connection_tools() {
    // A project file can tighten: `allow` is a restriction, not a loosening.
    let (mut s, _d) = session("[policy.default]\nallow = [\"query\", \"search\"]\n");
    let listed = s.list_tools();
    let mut seen = 0;
    for tool in [
        "iris_select_container",
        "iris_start_sandbox",
        "iris_containers",
        "iris_add_server",
    ] {
        if !listed.iter().any(|n| n == tool) {
            continue;
        }
        seen += 1;
        let b = body(&s.call(tool, &json!({})));
        assert_eq!(b["error_code"], "POLICY_GATE", "{tool}: {b}");
        assert_eq!(b["blocked_category"], "admin", "{tool}: {b}");
    }
    assert!(
        seen >= 2,
        "only {seen} connection tools are listed: {listed:?}"
    );
    // The tools that read only the registry stay.
    let b = body(&s.call("kb_recall", &json!({"query": "x"})));
    assert_ne!(b["error_code"], "POLICY_GATE", "{b}");
    // The skill tools run code on the default server, so `allow` judges them (R2). A reader that
    // can answer from the bundled skills does, and says the server's were not consulted (R2b).
    let b = body(&s.call("skill_list", &json!({})));
    assert_eq!(b["error_code"], Value::Null, "{b}");
    assert!(b["count"].as_u64().unwrap_or(0) > 20, "{b}");
    assert_eq!(b["sources"]["synthesized"]["searched"], false, "{b}");
    assert_eq!(
        b["sources"]["synthesized"]["refused"]["error_code"], "POLICY_GATE",
        "{b}"
    );
    // The dispatcher has nothing to fall back to.
    let b = body(&s.call("skill", &json!({"action": "list"})));
    assert_eq!(b["error_code"], "POLICY_GATE", "{b}");
    assert_eq!(b["blocked_category"], "skill", "{b}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_live_template_refuses_the_connection_tools() {
    let (mut s, _d) = session("[policy.default]\nmcpTemplate = \"live\"\n");
    let listed = s.list_tools();
    for tool in [
        "iris_select_container",
        "iris_containers",
        "iris_add_server",
    ] {
        if !listed.iter().any(|n| n == tool) {
            continue;
        }
        let b = body(&s.call(tool, &json!({})));
        assert_eq!(b["error_code"], "ENV_GATE_BLOCKED", "{tool}: {b}");
    }
}

const SKILL_KB_CALLS: &[(&str, &str)] = &[
    ("skill", r#"{"action":"list"}"#),
    ("kb", r#"{"action":"recall","query":"x"}"#),
    ("kb_index", "{}"),
    ("skill_forget", r#"{"name":"x"}"#),
];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_live_template_refuses_the_skill_and_kb_tools() {
    let (mut s, _d) = session("[policy.default]\nmcpTemplate = \"live\"\n");
    for (tool, args) in SKILL_KB_CALLS {
        let args: Value = serde_json::from_str(args).unwrap();
        let b = body(&s.call(tool, &args));
        assert_eq!(b["error_code"], "ENV_GATE_BLOCKED", "{tool}: {b}");
        assert_eq!(b["blocked_category"], "execute", "{tool}: {b}");
    }
    // The three readers serve the bundled skills and say the server's were not consulted.
    for (tool, args) in [
        ("skill_list", json!({})),
        ("skill_search", json!({"query": "guardrails"})),
    ] {
        let b = body(&s.call(tool, &args));
        assert_eq!(b["error_code"], Value::Null, "{tool}: {b}");
        assert!(b["count"].as_u64().unwrap_or(0) > 0, "{tool}: {b}");
        assert_eq!(
            b["sources"]["synthesized"]["refused"]["error_code"], "ENV_GATE_BLOCKED",
            "{tool}: {b}"
        );
        assert_eq!(
            b["sources"]["synthesized"]["refused"]["kind"], "operator_limit",
            "{tool}: {b}"
        );
    }
    let b = body(&s.call(
        "skill_describe",
        &json!({"name": "objectscript-guardrails"}),
    ));
    assert_eq!(b["success"], true, "{b}");
    // The registry-only tools are not judged.
    let b = body(&s.call("kb_recall", &json!({"query": "x"})));
    assert_ne!(b["error_code"], "ENV_GATE_BLOCKED", "{b}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn strict_refuses_skill_and_kb_and_leaves_skill_list_answering() {
    let (mut s, _d) = session("read_only = \"strict\"\n");
    let listed = s.list_tools();
    // `skill` and `kb` compile a scratch class on every action, so strict hides them.
    for tool in ["skill", "kb", "kb_index"] {
        assert!(
            !listed.iter().any(|n| n == tool),
            "{tool} is listed: {listed:?}"
        );
    }
    for (tool, args) in [
        ("skill", json!({"action": "list"})),
        ("kb", json!({"action": "recall", "query": "x"})),
        ("kb_index", json!({})),
    ] {
        let b = body(&s.call(tool, &args));
        assert_eq!(b["error_code"], "READ_ONLY_STRICT", "{tool}: {b}");
        assert_eq!(b["refusal"]["kind"], "operator_limit", "{tool}: {b}");
    }
    // `skill_list` reads the bundled skills and ^SKILLS through the container's terminal. It
    // compiles nothing, so strict leaves it, and the guardrail skills stay reachable.
    assert!(listed.iter().any(|n| n == "skill_list"), "{listed:?}");
    let b = body(&s.call("skill_list", &json!({})));
    assert_ne!(b["error_code"], "READ_ONLY_STRICT", "{b}");
    assert!(b["count"].as_u64().unwrap_or(0) > 0, "{b}");
}
