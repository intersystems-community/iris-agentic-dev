//! Layer 2 for the `mode`/`action` normalization in `call_tool` (security review, 2026-10-22).
//!
//! The write gate matched `mode` against its listed actions without trimming, so `" put"` took
//! `iris_doc`'s read-only default and the handler, which trims, wrote. No IRIS is needed: the gate
//! refuses before any IRIS call, so a refusal is the only answer a fixed server can give.

use iris_agentic_dev_core::testing::{answer_text, require_iad_binary, McpSession};

fn error_code(answer: &serde_json::Value) -> String {
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(|t| t.as_str())
        .unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v["error_code"].as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("no error_code: {}", answer_text(answer)))
}

fn session(writes: &str, destructive: &str) -> McpSession {
    McpSession::start(&[
        ("IRIS_WRITE_TOOLS_ENABLED".to_string(), writes.to_string()),
        (
            "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
            destructive.to_string(),
        ),
    ])
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_padded_write_mode_is_refused_when_writes_are_off() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut s = session("false", "false");
    for mode in [" put", "PUT\n", "\tdelete "] {
        let answer = s.call(
            "iris_doc",
            &serde_json::json!({"mode": mode, "name": "IadGate163.X.cls", "content": "Class IadGate163.X {}"}),
        );
        assert_eq!(error_code(&answer), "WRITE_TOOLS_DISABLED", "mode {mode:?}");
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_padded_kill_is_refused_by_the_destructive_gate() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut s = session("true", "false");
    for action in [" kill", "KILL", "\u{212A}ill"] {
        let answer = s.call(
            "iris_global",
            &serde_json::json!({"action": action, "global_name": "IadGate163"}),
        );
        assert_eq!(
            error_code(&answer),
            "DESTRUCTIVE_TOOLS_DISABLED",
            "action {action:?}"
        );
    }
}

/// GRANT and REVOKE are not reads. The read-only check refuses them before any IRIS call, and the
/// refusal points at `mode="write"`, not at `force`.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn read_mode_refuses_grant_and_revoke() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut s = session("false", "false");
    for query in ["GRANT %ALL TO UnknownUser", "REVOKE SELECT ON X FROM Y"] {
        let answer = s.call("iris_query", &serde_json::json!({"query": query}));
        assert_eq!(error_code(&answer), "SQL_WRITE_BLOCKED", "{query}");
        let text = answer_text(&answer);
        assert!(text.contains("mode=\\\"write\\\""), "{text}");
        assert!(!text.contains("force: true"), "{text}");
    }
}
