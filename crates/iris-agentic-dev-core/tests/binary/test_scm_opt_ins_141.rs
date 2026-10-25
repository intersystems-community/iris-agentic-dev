//! Layer 2 for spec 141 T198: over MCP stdio, `IRIS_SCM_ALLOW_DISCONNECT` is its own opt-in. The
//! CheckIn switch does not open Disconnect or Reconnect, each refusal names its own setting, and
//! `check_config` reports each opt-in.
//!
//! Nothing listens on the default connection. A refusal has to be the gate's answer: a connection
//! error would mean the call went past it.

use iris_agentic_dev_core::testing::McpSession;
use serde_json::{json, Value};

const CHECKIN: &str = "IRIS_SCM_ALLOW_CHECKIN";
const DISCONNECT: &str = "IRIS_SCM_ALLOW_DISCONNECT";

/// Port 2, not 1: `127.0.0.1:1` is the sentinel the NoPWS guard reads as "no web server", which
/// would answer before the handler's gate runs.
fn session(opt_ins: &[(&str, &str)]) -> McpSession {
    let mut env: Vec<(String, String)> = vec![
        ("IRIS_HOST".into(), "127.0.0.1".into()),
        ("IRIS_WEB_PORT".into(), "2".into()),
        ("IRIS_WRITE_TOOLS_ENABLED".into(), "true".into()),
    ];
    env.extend(opt_ins.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    McpSession::start(&env)
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

fn execute(s: &mut McpSession, id: &str) -> Value {
    body(&s.call(
        "iris_source_control",
        &json!({"action": "execute", "document": "Demo.A", "action_id": id}),
    ))
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn check_config_reports_each_opt_in() {
    for (env, checkin, disconnect) in [
        (vec![], false, false),
        (vec![(CHECKIN, "1")], true, false),
        (vec![(DISCONNECT, "yes")], false, true),
        (vec![(CHECKIN, "true"), (DISCONNECT, "1")], true, true),
        (vec![(CHECKIN, "0"), (DISCONNECT, "no")], false, false),
    ] {
        let mut s = session(&env);
        let b = body(&s.call("check_config", &json!({})));
        assert_eq!(b["scm_opt_ins"]["checkin"], checkin, "{env:?}: {b}");
        assert_eq!(b["scm_opt_ins"]["disconnect"], disconnect, "{env:?}: {b}");
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_checkin_switch_leaves_disconnect_and_reconnect_refused() {
    let mut s = session(&[(CHECKIN, "1")]);
    for id in ["Disconnect", "%Disconnect", "reconnect", " %RECONNECT "] {
        let b = execute(&mut s, id);
        assert_eq!(b["error_code"], "SCM_ADMIN_BLOCKED", "{id:?}: {b}");
        assert_eq!(b["refusal"]["kind"], "operator_limit", "{id:?}: {b}");
        assert_eq!(b["refusal"]["lift"]["setting"], DISCONNECT, "{id:?}: {b}");
    }
    for id in ["CheckIn", "%CHECKIN"] {
        let b = execute(&mut s, id);
        let code = b["error_code"].as_str().unwrap_or("");
        assert!(
            code != "CHECKIN_BLOCKED" && code != "SCM_ADMIN_BLOCKED",
            "{id:?} should get past the gate to the unreachable connection: {b}"
        );
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_disconnect_switch_leaves_checkin_refused() {
    let mut s = session(&[(DISCONNECT, "1")]);
    for id in ["CheckIn", "%checkin"] {
        let b = execute(&mut s, id);
        assert_eq!(b["error_code"], "CHECKIN_BLOCKED", "{id:?}: {b}");
        assert_eq!(b["refusal"]["lift"]["setting"], CHECKIN, "{id:?}: {b}");
    }
    for id in ["Disconnect", "%Reconnect"] {
        let b = execute(&mut s, id);
        let code = b["error_code"].as_str().unwrap_or("");
        assert!(
            code != "CHECKIN_BLOCKED" && code != "SCM_ADMIN_BLOCKED",
            "{id:?} should get past the gate to the unreachable connection: {b}"
        );
    }
}
