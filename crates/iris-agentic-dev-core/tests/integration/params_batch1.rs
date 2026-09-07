//! Batch 1 against live IRIS: every parameter these four tools declare is still honored.
//!
//! The schema tests in `tests/binary/schema_batch1.rs` prove the declaration. These prove the
//! declaration did not change what the tool does — a typed struct that deserializes but drops a
//! field would pass every schema assertion and break every caller.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS IRIS_NAMESPACE=USER`. The env-derived connection is named `_env`.

use iris_agentic_dev_core::testing::{
    answer_text, call_tool_with_env, live_env, require_iad_binary, McpSession,
};

/// The global these tests seed and delete. Named for the feature so a leftover node is traceable.
const TEST_GLOBAL: &str = "^IADParams113";

/// `live_env` plus the two gates. The test sets them itself rather than depending on the harness
/// environment, so it means the same thing run alone as it does in CI (Principle XII).
fn write_enabled_env() -> Vec<(String, String)> {
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    env.push((
        "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
        "1".to_string(),
    ));
    env
}

/// The `result.content[0].text` of a `tools/call` answer, parsed.
fn payload(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no text content in {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("content was not JSON ({e}): {text}"))
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn compare_document_honors_all_four_parameters() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer = call_tool_with_env(
        "compare_document",
        &serde_json::json!({
            "document": "%Library.String.cls",
            "server_a": "_env",
            "server_b": "_env",
            "namespace": "USER"
        }),
        &live_env(),
    );
    let out = payload(&answer);

    // Comparing a server against itself: the point is that all four values reached the handler.
    // `server_a`/`server_b` come back as resolved URLs rather than the names sent, so the echo
    // cannot prove those two arrived — the bogus-name call below does that instead.
    assert_eq!(
        out.get("document").and_then(serde_json::Value::as_str),
        Some("%Library.String.cls"),
        "compare_document dropped `document`: {out}"
    );
    assert_eq!(
        out.get("namespace").and_then(serde_json::Value::as_str),
        Some("USER"),
        "compare_document dropped `namespace`, which would silently compare the wrong namespace: \
         {out}"
    );
    assert_eq!(
        out.get("same"),
        Some(&serde_json::json!(true)),
        "a server compared against itself must match: {out}"
    );

    // A name that is not in the pool must be reported by name. A dropped `server_a` would resolve
    // to `""` and the message would name that instead.
    let refused = call_tool_with_env(
        "compare_document",
        &serde_json::json!({
            "document": "%Library.String.cls",
            "server_a": "no_such_instance_113",
            "server_b": "_env"
        }),
        &live_env(),
    );
    let text = answer_text(&refused);
    assert!(
        text.contains("no_such_instance_113"),
        "compare_document dropped `server_a` — the refusal names something else: {text}"
    );
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn compare_namespace_honors_all_three_parameters() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer = call_tool_with_env(
        "compare_namespace",
        &serde_json::json!({"namespace": "USER", "server_a": "_env", "server_b": "_env"}),
        &live_env(),
    );
    let out = payload(&answer);

    assert_eq!(
        out.get("namespace").and_then(serde_json::Value::as_str),
        Some("USER"),
        "compare_namespace dropped `namespace`: {out}"
    );
    assert_eq!(
        out.get("only_in_a").and_then(serde_json::Value::as_array),
        Some(&vec![]),
        "a namespace compared against itself has nothing only on one side: {out}"
    );

    let refused = call_tool_with_env(
        "compare_namespace",
        &serde_json::json!({"server_a": "_env", "server_b": "no_such_instance_113"}),
        &live_env(),
    );
    let text = answer_text(&refused);
    assert!(
        text.contains("no_such_instance_113"),
        "compare_namespace dropped `server_b` — the refusal names something else: {text}"
    );
}

/// `count` is the only integer in this batch, and the only parameter whose effect is directly
/// observable. Seeding four nodes and asking for two proves the value arrived — a dropped `count`
/// would default to 20 and return all four.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary; writes and deletes ^IADParams113"]
fn global_preview_honors_count_and_global_and_global_kill_honors_its_token() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // One session throughout: the confirm_token global_preview mints lives in the server process, so
    // a second spawn would answer CONFIRM_REQUIRED no matter what was sent.
    let mut mcp = McpSession::start(&write_enabled_env());

    let seed = mcp.call(
        "iris_execute",
        &serde_json::json!({
            "code": format!(
                "Kill {TEST_GLOBAL} Set {TEST_GLOBAL}(1)=\"a\" Set {TEST_GLOBAL}(2)=\"b\" \
                 Set {TEST_GLOBAL}(3)=\"c\" Set {TEST_GLOBAL}(4)=\"d\" Write \"seeded\",!"
            )
        }),
    );
    assert!(
        answer_text(&seed).contains("seeded"),
        "could not seed {TEST_GLOBAL}, so the count assertion below would be vacuous: {}",
        answer_text(&seed)
    );

    let preview = mcp.call(
        "global_preview",
        &serde_json::json!({"global": TEST_GLOBAL, "server": "_env", "count": 2}),
    );
    let out = payload(&preview);
    assert_eq!(
        out.get("entries")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(2),
        "global_preview ignored `count: 2` over a four-node global: {out}"
    );
    // Echoed without the caret: `normalize_global_name` strips it, and that normalization is part of
    // the behaviour this conversion must not change.
    assert_eq!(
        out.get("global").and_then(serde_json::Value::as_str),
        Some(TEST_GLOBAL.trim_start_matches('^')),
        "global_preview dropped `global`: {out}"
    );
    assert_eq!(
        out.get("server").and_then(serde_json::Value::as_str),
        Some("_env"),
        "global_preview dropped `server`: {out}"
    );

    let token = out
        .get("confirm_token")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("global_preview issued no confirm_token: {out}"))
        .to_string();
    assert!(!token.is_empty(), "confirm_token was empty: {out}");

    // The token round-trip is the proof that `confirm_token` is honored: a dropped value would be
    // indistinguishable from the CONFIRM_REQUIRED case below.
    let killed = mcp.call(
        "global_kill",
        &serde_json::json!({"global": TEST_GLOBAL, "server": "_env", "confirm_token": token}),
    );
    let out = payload(&killed);
    assert_eq!(
        out.get("success"),
        Some(&serde_json::json!(true)),
        "global_kill refused a token global_preview had just issued, so `confirm_token` was not \
         honored: {out}"
    );
}

/// Omitting `confirm_token` must still reach the handler's own error, not a deserialization failure.
/// This is what "optional" buys: the message says what to do next.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn global_kill_without_a_token_still_says_confirm_required() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer = call_tool_with_env(
        "global_kill",
        &serde_json::json!({"global": "^IADParams113NotThere"}),
        &write_enabled_env(),
    );
    let text = answer_text(&answer);
    assert!(
        text.contains("CONFIRM_REQUIRED"),
        "expected the handler's own CONFIRM_REQUIRED error for a missing confirm_token, got: {text}"
    );
}
