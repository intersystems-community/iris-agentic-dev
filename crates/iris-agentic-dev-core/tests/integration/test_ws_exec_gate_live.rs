//! Layer 3 for the `iris_ws_exec` gate (119, issue #137) — a real terminal session on real IRIS.
//!
//! The unit and binary layers both answer without a session, which is the point of the gate running
//! first. This layer answers the question they cannot: with a session that IRIS actually opened, is
//! the refusal still a refusal, and does the session still work afterwards?
//!
//! The second half matters as much as the first. A gate that breaks the session it refused would be
//! traded for a denial of service — the caller's variables are gone and the next legitimate call
//! fails too.
//!
//! Run against `iris-dev-iris`:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --features testing --test integration ws_exec_gate -- --include-ignored \
//!     --test-threads=1

use iris_agentic_dev_core::testing::{answer_text, live_env, McpSession};

/// Live connection plus writes on. `iris_ws_exec` is write-classified, so with writes off the write
/// gate answers first and the code-edit gate is never reached — the test would pass with the fix
/// reverted.
fn env() -> Vec<(String, String)> {
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "true".to_string()));
    env
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

/// Open a session, or skip. Community 2026.2 has the PWS this needs; an Enterprise 2026.2.0AI build
/// does not (DPP-1192), and a skip there is honest where a failure would not be.
fn open_session(session: &mut McpSession) -> Option<String> {
    let answer = session.call("iris_ws_open", &serde_json::json!({"namespace": "USER"}));
    let token = body(&answer)
        .get("session")
        .and_then(|s| s.as_str())
        .map(str::to_string);
    if token.is_none() {
        eprintln!(
            "iris_ws_open did not return a session (no WebSocket terminal on this build?) — \
             skipping: {}",
            answer_text(&answer)
        );
    }
    token
}

/// FR-001, FR-008: refused on a live session, and the session survives it.
#[test]
#[ignore = "requires the iris-dev-iris container; run with --include-ignored"]
fn a_live_session_refuses_a_class_delete_and_keeps_working() {
    let mut session = McpSession::start(&env());
    let Some(token) = open_session(&mut session) else {
        return;
    };

    let blocked = session.call(
        "iris_ws_exec",
        &serde_json::json!({
            "session": token,
            "code": "Do $system.OBJ.Delete(\"MyApp.NoSuchClass\")",
        }),
    );
    assert_eq!(
        body(&blocked).get("error_code").and_then(|c| c.as_str()),
        Some("CODE_EDIT_BLOCKED"),
        "a live WebSocket session must not be a way around the code-edit gate — {}",
        answer_text(&blocked)
    );

    // The refusal happened before the pool, so the session is untouched. Prove it by using it.
    let ok = session.call(
        "iris_ws_exec",
        &serde_json::json!({"session": &token, "code": "Set gate119 = 42"}),
    );
    assert!(
        body(&ok).get("error_code").is_none(),
        "the session was refused once and is now unusable — the gate broke it: {}",
        answer_text(&ok)
    );
    let readback = session.call(
        "iris_ws_exec",
        &serde_json::json!({"session": &token, "code": "Write gate119"}),
    );
    assert!(
        answer_text(&readback).contains("42"),
        "session state must survive a blocked call — {}",
        answer_text(&readback)
    );

    let _ = session.call("iris_ws_close", &serde_json::json!({"session": &token}));
}

/// FR-002 on live IRIS: the gate is narrow enough that ordinary terminal work still runs.
#[test]
#[ignore = "requires the iris-dev-iris container; run with --include-ignored"]
fn ordinary_objectscript_still_runs_in_a_session() {
    let mut session = McpSession::start(&env());
    let Some(token) = open_session(&mut session) else {
        return;
    };

    let answer = session.call(
        "iris_ws_exec",
        &serde_json::json!({"session": &token, "code": "Write $zversion"}),
    );
    let text = answer_text(&answer);
    assert!(
        text.contains("IRIS"),
        "`Write $zversion` must reach IRIS and come back with a version — {text}"
    );

    let _ = session.call("iris_ws_close", &serde_json::json!({"session": &token}));
}

/// FR-005 on live IRIS: the destructive tier applies to the terminal too.
#[test]
#[ignore = "requires the iris-dev-iris container; run with --include-ignored"]
fn a_live_session_refuses_a_literal_global_kill() {
    let mut env = env();
    env.push((
        "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
        "false".to_string(),
    ));
    let mut session = McpSession::start(&env);
    let Some(token) = open_session(&mut session) else {
        return;
    };

    let answer = session.call(
        "iris_ws_exec",
        &serde_json::json!({"session": &token, "code": "Kill ^Gate119Fixture"}),
    );
    assert_eq!(
        body(&answer).get("error_code").and_then(|c| c.as_str()),
        Some("DESTRUCTIVE_TOOLS_DISABLED"),
        "Kill ^ through a WebSocket session must hit the same tier as iris_execute — {}",
        answer_text(&answer)
    );

    let _ = session.call("iris_ws_close", &serde_json::json!({"session": &token}));
}
