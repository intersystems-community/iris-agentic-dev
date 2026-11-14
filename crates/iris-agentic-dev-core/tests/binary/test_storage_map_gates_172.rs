//! Layer 2 for spec 172 FR-023: `build_indexes` is a destructive write, over MCP stdio.
//!
//! The process reads `IRIS_DESTRUCTIVE_TOOLS_ENABLED` once, at start, so a live test launched with
//! it set (CI's e2e job) cannot close the tier in-process. A spawned binary states its own launch
//! environment. Nothing listens on the default connection (port 2): a refusal has to be the gate's.

use iris_agentic_dev_core::testing::McpSession;
use serde_json::{json, Value};

fn session(destructive: &str) -> (McpSession, tempfile::TempDir) {
    let env: Vec<(String, String)> = [
        ("IRIS_HOST", "127.0.0.1"),
        ("IRIS_WEB_PORT", "2"),
        ("IRIS_WRITE_TOOLS_ENABLED", "true"),
        ("IRIS_DESTRUCTIVE_TOOLS_ENABLED", destructive),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let dir = tempfile::tempdir().unwrap();
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

fn edit(build_indexes: bool) -> Value {
    json!({"action": "edit", "class": "IadSm172.Genre", "version": "v", "namespace": "USER",
           "build_indexes": build_indexes,
           "ops": [{"op": "set", "path": "/maps/Master/data/Popular/piece", "value": "3"}]})
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn build_indexes_needs_the_destructive_tier_and_a_plain_edit_does_not() {
    let (mut closed, _d1) = session("false");
    let refused = body(&closed.call("iris_storage_map", &edit(true)));
    let plain = body(&closed.call("iris_storage_map", &edit(false)));
    drop(closed);
    let (mut open, _d2) = session("true");
    let let_through = body(&open.call("iris_storage_map", &edit(true)));

    assert_eq!(
        refused["error_code"], "DESTRUCTIVE_TOOLS_DISABLED",
        "{refused}"
    );
    assert_eq!(refused["refusal"]["kind"], "operator_limit", "{refused}");
    // Past the gate, the call reaches for IRIS and finds nothing on port 2.
    for (what, b) in [("plain edit", &plain), ("destructive open", &let_through)] {
        assert_ne!(b["error_code"], "DESTRUCTIVE_TOOLS_DISABLED", "{what}: {b}");
        assert!(b["refusal"].is_null(), "{what}: {b}");
        assert_eq!(b["error_code"], "IRIS_UNREACHABLE", "{what}: {b}");
    }
}
