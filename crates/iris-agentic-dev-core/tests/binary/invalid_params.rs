//! Layer 2 for the normalized type-error response (113 T032, FR-017).
//!
//! A parameter of the wrong type was already rejected before this feature — rmcp fails the
//! deserialization and rmcp's failure is a JSON-RPC transport error carrying serde's phrasing. So
//! FR-016 was satisfied structurally and FR-017 was not: no `success`, no `error_code`, nothing a
//! client can branch on without matching English. These tests pin the shape.
//!
//! The second test is the only place the one intentional behavior change in this feature is pinned.
//! `query_audit_log {"limit": "100"}` used to be read with `as_u64()`, which returns `None` for a
//! JSON string, so the string was discarded and the tool answered with the default of 100 rows —
//! the same answer it gives for a valid call, which is what made the bug invisible. It is now a
//! refusal. Research decision 3 accepted that: a caller who quoted a number gets told, rather than
//! getting a plausible answer to a question they did not ask.
//!
//! No IRIS needed. Deserialization happens before any connection is resolved.

use iris_agentic_dev_core::testing::{answer_text, require_iad_binary, McpSession};

/// The refusal body, from `structuredContent` or from the text content, whichever the answer used.
fn refusal_body(answer: &serde_json::Value) -> serde_json::Value {
    let result = answer
        .get("result")
        .unwrap_or_else(|| panic!("expected a result, got: {}", answer_text(answer)));
    if let Some(sc) = result.get("structuredContent") {
        return sc.clone();
    }
    let text = result
        .pointer("/content/0/text")
        .and_then(|t| t.as_str())
        .unwrap_or_else(|| panic!("no structuredContent and no text: {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("refusal text is not JSON ({e}): {text}"))
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_wrong_typed_parameter_is_refused_as_invalid_params() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer = McpSession::start(&[]).call("iris_query", &serde_json::json!({"query": 42}));
    let body = refusal_body(&answer);

    assert_eq!(
        body.get("success"),
        Some(&serde_json::Value::Bool(false)),
        "{}",
        answer_text(&answer)
    );
    assert_eq!(
        body.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "a type error is a value rejection, distinguishable from UNKNOWN_PARAMETER — {}",
        answer_text(&answer)
    );

    let msg = body
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_default();
    assert!(
        msg.contains("iris_query"),
        "the message must name the tool: {msg}"
    );
    // serde's own text is kept, not paraphrased: it names the offending value and the expected type,
    // and rewriting it into something prettier is how the field name gets lost.
    assert!(
        msg.contains("expected a string") && msg.contains("42"),
        "serde's original text was not preserved: {msg}"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_quoted_integer_is_refused_rather_than_silently_defaulted() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer =
        McpSession::start(&[]).call("query_audit_log", &serde_json::json!({"limit": "100"}));
    let body = refusal_body(&answer);
    assert_eq!(
        body.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "`limit: \"100\"` used to be dropped and answered with the default — {}",
        answer_text(&answer)
    );
    let msg = body
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_default();
    assert!(
        msg.contains("limit"),
        "the message must name the parameter that was wrong: {msg}"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn an_unknown_name_and_a_bad_value_are_different_codes() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // FR-017 in one test: the two refusals a caller has to tell apart, side by side. "I do not know
    // that name" and "I know the name, that value is not permitted" are different repairs.
    let mut session = McpSession::start(&[]);
    let unknown = session.call("query_audit_log", &serde_json::json!({"lmit": 100}));
    let bad_value = session.call("query_audit_log", &serde_json::json!({"limit": "100"}));

    assert_eq!(
        refusal_body(&unknown)
            .get("error_code")
            .and_then(|c| c.as_str()),
        Some("UNKNOWN_PARAMETER"),
        "{}",
        answer_text(&unknown)
    );
    assert_eq!(
        refusal_body(&bad_value)
            .get("error_code")
            .and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "{}",
        answer_text(&bad_value)
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_answer_is_a_tool_result_not_a_json_rpc_error() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // The shape half of FR-017. Before normalization this came back as a JSON-RPC `error` member,
    // which a client reads as "the call never happened" rather than "the call was refused, here is
    // why" — and which no `structuredContent` reader can see at all.
    let answer = McpSession::start(&[]).call("iris_query", &serde_json::json!({"query": 42}));
    assert!(
        answer.get("error").is_none(),
        "still a transport-level error: {}",
        answer_text(&answer)
    );
    assert_eq!(
        answer.pointer("/result/isError"),
        Some(&serde_json::Value::Bool(true)),
        "a refusal carries the protocol error flag (issue #95): {}",
        answer_text(&answer)
    );
}
