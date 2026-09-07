//! Layer 2 for the `UNKNOWN_PARAMETER` refusal (113 US4, FR-015/FR-016).
//!
//! The unit tests in `tests/unit/test_param_rejection.rs` pin the message the builder produces.
//! These pin that the builder is actually wired into `call_tool` and that the accepted list it
//! prints comes off the router rather than a literal in a test — spawn the binary, send the call,
//! read the answer.
//!
//! None of these need IRIS. That is the point: the refusal happens in the `call_tool` override
//! before any connection is resolved, so the server can answer "I do not accept that name" without
//! a server to ask.

use iris_agentic_dev_core::testing::{answer_text, require_iad_binary, McpSession};

/// Pull the refusal body out of a `tools/call` answer, whichever shape it arrived in.
///
/// A refusal is a `CallToolResult` carrying `structuredContent`; older clients read the same JSON
/// out of `content[0].text`. Both are checked so the test cannot pass on a shape no client reads.
fn refusal_body(answer: &serde_json::Value) -> serde_json::Value {
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
    serde_json::from_str(text).unwrap_or_else(|e| panic!("refusal text is not JSON ({e}): {text}"))
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_misspelled_parameter_is_refused_by_code_and_by_name() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // The exact call from contracts/rejection-response.md. Before this feature it returned
    // `success: true` — the query ran, against the default namespace, and the caller's `namesapce`
    // was dropped on the floor.
    let answer = McpSession::start(&[]).call(
        "iris_query",
        &serde_json::json!({"query": "SELECT 1", "namesapce": "USER"}),
    );
    let body = refusal_body(&answer);

    assert_eq!(
        body.get("success"),
        Some(&serde_json::Value::Bool(false)),
        "the refusal must carry success: false — {}",
        answer_text(&answer)
    );
    assert_eq!(
        body.get("error_code").and_then(|c| c.as_str()),
        Some("UNKNOWN_PARAMETER"),
        "wrong error code — {}",
        answer_text(&answer)
    );
    assert_eq!(
        answer.pointer("/result/isError"),
        Some(&serde_json::Value::Bool(true)),
        "a refusal is a protocol-level error (issue #95) — {}",
        answer_text(&answer)
    );

    let msg = body
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_else(|| panic!("no error text: {}", answer_text(&answer)));
    assert!(
        msg.contains("iris_query does not accept \"namesapce\""),
        "the message must name the tool and the caller's own spelling: {msg}"
    );
    assert!(
        msg.contains("Accepted parameters: "),
        "the message must list what the tool does accept: {msg}"
    );
    assert!(
        msg.contains("Did you mean \"namespace\"?"),
        "one edit away and unsuggested — SC-008 wants the error to be enough on its own: {msg}"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_accepted_list_is_the_tool_s_own_advertised_properties() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // Not a hand-maintained list: the names in the message have to be the names in the schema, or
    // the two drift the first time a parameter is added.
    let tools = iris_agentic_dev_core::tools::IrisTools::new(None).expect("IrisTools::new");
    let advertised: Vec<String> = tools
        .tool_input_schema("iris_query")
        .and_then(|s| s.get("properties").cloned())
        .and_then(|p| p.as_object().cloned())
        .expect("iris_query must advertise properties")
        .keys()
        .cloned()
        .collect();

    let answer = McpSession::start(&[]).call("iris_query", &serde_json::json!({"zzz": 1}));
    let body = refusal_body(&answer);
    let msg = body
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_default();
    let listed = msg
        .split("Accepted parameters: ")
        .nth(1)
        .and_then(|s| s.split(". ").next())
        .unwrap_or("")
        .trim_end_matches('.')
        .split(", ")
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        listed, advertised,
        "the accepted list in the message disagrees with the advertised schema"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn every_unknown_key_comes_back_in_one_answer() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer = McpSession::start(&[]).call(
        "iris_query",
        &serde_json::json!({"query": "SELECT 1", "namesapce": "USER", "frobnicate": true}),
    );
    let msg = refusal_body(&answer)
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_default()
        .to_string();
    assert!(msg.contains("\"namesapce\""), "first key missing: {msg}");
    assert!(msg.contains("\"frobnicate\""), "second key missing: {msg}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_no_argument_tool_refuses_arguments_instead_of_ignoring_them() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // `skill_list` advertises `properties: {}`. Before this feature it accepted and dropped
    // anything, which is how a caller could believe they had filtered a list that was never
    // filtered.
    let answer =
        McpSession::start(&[]).call("skill_list", &serde_json::json!({"namespace": "USER"}));
    let body = refusal_body(&answer);
    assert_eq!(
        body.get("error_code").and_then(|c| c.as_str()),
        Some("UNKNOWN_PARAMETER"),
        "{}",
        answer_text(&answer)
    );
    let msg = body
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_default();
    assert!(
        msg.contains("accepts no parameters"),
        "a tool that takes nothing has to say so: {msg}"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn an_advertised_parameter_is_never_refused() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // The other half of FR-004, and the one that would break every existing client if it regressed:
    // a call using only advertised names must not be refused for its names. It will fail here for
    // want of a connection — `IRIS_HOST` is unset by design — and that is a different error.
    let mut session = McpSession::start(&[]);
    for tool in ["iris_query", "skill_list", "check_config"] {
        let args = if tool == "iris_query" {
            serde_json::json!({"query": "SELECT 1", "namespace": "USER", "mode": "read"})
        } else {
            serde_json::json!({})
        };
        let answer = session.call(tool, &args);
        let text = answer_text(&answer);
        assert!(
            !text.contains("UNKNOWN_PARAMETER"),
            "{tool} refused a call that used only advertised names: {text}"
        );
    }
}
