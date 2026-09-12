//! Layer 2 for the `iris_ws_exec` gate (119, issue #137).
//!
//! `tests/unit/test_ws_exec_gate.rs` pins what `dispatch_gate` decides. These pin that the decision
//! is wired into the handler, by spawning the binary and sending the call.
//!
//! No IRIS needed, and that is the assertion, not a convenience: the gate runs before
//! `WsSessionPool::exec`, so a session token that was never issued still comes back refused by the
//! gate rather than by the session pool. If these ever start reporting a session error instead, the
//! gate moved after the pool and the fix is undone.

use iris_agentic_dev_core::testing::{answer_text, require_iad_binary, McpSession};

/// Writes are enabled here on purpose. `iris_ws_exec` is write-classified, so `call_tool`'s write
/// gate refuses it first when writes are off — which would make these tests pass for the wrong
/// reason and keep passing with the code-edit gate removed.
fn writes_enabled() -> Vec<(String, String)> {
    vec![("IRIS_WRITE_TOOLS_ENABLED".to_string(), "true".to_string())]
}

fn body(answer: &serde_json::Value) -> serde_json::Value {
    let result = answer
        .get("result")
        .unwrap_or_else(|| panic!("expected a result, got: {}", answer_text(answer)));
    if let Some(sc) = result.get("structuredContent") {
        return sc.clone();
    }
    let text = result
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or_else(|| panic!("no structuredContent and no text: {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("body is not JSON ({e}): {text}"))
}

fn error_code(answer: &serde_json::Value) -> String {
    body(answer)
        .get("error_code")
        .and_then(|c| c.as_str())
        .unwrap_or_else(|| panic!("no error_code in the answer: {}", answer_text(answer)))
        .to_string()
}

/// FR-001, FR-008: the reported call from issue #137.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn ws_exec_refuses_a_class_delete() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer = McpSession::start(&writes_enabled()).call(
        "iris_ws_exec",
        &serde_json::json!({
            "session": "session-that-was-never-opened",
            "code": "Do $system.OBJ.Delete(\"MyApp.Foo\")",
        }),
    );
    assert_eq!(
        error_code(&answer),
        "CODE_EDIT_BLOCKED",
        "iris_ws_exec must refuse a class delete before it reaches the session pool — {}",
        answer_text(&answer)
    );
}

/// FR-001: a direct write to a code-storage global, the other half of the same hole.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn ws_exec_refuses_a_write_to_a_code_storage_global() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer = McpSession::start(&writes_enabled()).call(
        "iris_ws_exec",
        &serde_json::json!({
            "session": "session-that-was-never-opened",
            "code": "Set ^oddDEF(\"MyApp.Foo\",1)=\"\"",
        }),
    );
    assert_eq!(
        error_code(&answer),
        "CODE_EDIT_BLOCKED",
        "^oddDEF is code storage — {}",
        answer_text(&answer)
    );
}

/// FR-005: the destructive tier, with writes on and destructive off. This is the combination a
/// developer actually runs, and the one `iris_execute` already refuses.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn ws_exec_refuses_a_literal_global_kill_when_the_destructive_tier_is_off() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let env = vec![
        ("IRIS_WRITE_TOOLS_ENABLED".to_string(), "true".to_string()),
        (
            "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
            "false".to_string(),
        ),
    ];
    let answer = McpSession::start(&env).call(
        "iris_ws_exec",
        &serde_json::json!({
            "session": "session-that-was-never-opened",
            "code": "Kill ^MyApp.Patient",
        }),
    );
    assert_eq!(
        error_code(&answer),
        "DESTRUCTIVE_TOOLS_DISABLED",
        "a literal Kill ^ must be refused — {}",
        answer_text(&answer)
    );
    let text = answer_text(&answer);
    assert!(
        text.contains("Indirect kill") || text.contains("indirect"),
        "the refusal has to say what it does not catch, or it reads as a full block — {text}"
    );
}

/// The write gate still comes first. With writes off, the answer names the write tier — the
/// code-edit gate is an additional refusal, not a replacement for the one `call_tool` already does.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_write_gate_still_answers_first_when_writes_are_off() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let env = vec![("IRIS_WRITE_TOOLS_ENABLED".to_string(), "false".to_string())];
    let answer = McpSession::start(&env).call(
        "iris_ws_exec",
        &serde_json::json!({
            "session": "session-that-was-never-opened",
            "code": "Do $system.OBJ.Delete(\"MyApp.Foo\")",
        }),
    );
    assert_eq!(
        error_code(&answer),
        "WRITE_TOOLS_DISABLED",
        "with writes off the write gate answers, in call_tool, ahead of dispatch — {}",
        answer_text(&answer)
    );
}
