//! Where the unknown-parameter check sits in the call sequence, proved against live IRIS
//! (113 T031, FR-016 and `contracts/rejection-response.md`).
//!
//! Two questions, and neither can be answered by a unit test:
//!
//! 1. **The gate answers first.** A caller with the destructive tier off who also misspells a key on
//!    `global_kill` must be told the gate refused. The security answer outranks the usability one,
//!    and getting this backwards would leak which parameters a tool a caller is not allowed to run
//!    accepts.
//! 2. **A refused call did nothing.** The check sits ahead of `tool_router.call`, so the handler is
//!    never entered — the global `global_kill` was pointed at is still there, node for node, after
//!    the refusal. This is the assertion that distinguishes "rejected" from "rejected after
//!    deleting half of it".
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS IRIS_NAMESPACE=USER`. The env-derived connection is named `_env`.

use iris_agentic_dev_core::testing::{answer_text, live_env, require_iad_binary, McpSession};

/// The global these tests seed and delete. Distinct from batch 1's `^IADParams113` so a run of both
/// in either order cannot have one test's cleanup satisfy the other's assertion.
const TEST_GLOBAL: &str = "^IADReject113";

fn env_with(gates: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = live_env();
    for (k, v) in gates {
        env.push(((*k).to_string(), (*v).to_string()));
    }
    env
}

/// The refusal body of a `tools/call` answer.
fn payload(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no text content in {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("content was not JSON ({e}): {text}"))
}

fn error_code(answer: &serde_json::Value) -> String {
    payload(answer)
        .get("error_code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no error_code in {}", answer_text(answer)))
        .to_string()
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn the_gate_answers_before_the_unknown_parameter_check() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // Both gate configurations that refuse a destructive tool, each with a bogus key alongside a
    // complete and otherwise valid argument set. In both, the answer must be the gate's.
    let cases = [
        (
            vec![
                ("IRIS_WRITE_TOOLS_ENABLED", "1"),
                ("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "0"),
            ],
            "DESTRUCTIVE_TOOLS_DISABLED",
        ),
        // Destructive on, writes off. The contract sketch guessed `DESTRUCTIVE_REQUIRES_WRITES`
        // here, but that code belongs to `validate_declared_gates`, which reads the config file at
        // startup and never sees these environment variables. At the call site the write gate is
        // checked first — the more fundamental refusal, and the one whose remedy comes first — so
        // the answer is `WRITE_TOOLS_DISABLED`. Still a gate code, which is all this test is about.
        (
            vec![
                ("IRIS_WRITE_TOOLS_ENABLED", "0"),
                ("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1"),
            ],
            "WRITE_TOOLS_DISABLED",
        ),
    ];

    for (gates, expected) in cases {
        let answer = McpSession::start(&env_with(&gates)).call(
            "global_kill",
            &serde_json::json!({
                "global": TEST_GLOBAL,
                "server": "_env",
                "confirm_token": "not-a-real-token",
                "nemspace": "USER"
            }),
        );
        let code = error_code(&answer);
        assert_eq!(
            code, expected,
            "with {gates:?} the gate must answer, not the parameter check — got {code}"
        );
        assert_ne!(
            code, "UNKNOWN_PARAMETER",
            "the typo was reported ahead of the gate, which tells a caller who may not run this \
             tool what its parameters are"
        );
    }
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary; writes and deletes ^IADReject113"]
fn a_refused_call_leaves_the_target_global_untouched() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // Both gates on, and a real token minted seconds earlier: the *only* thing standing between this
    // call and a deleted global is the unknown-parameter check. That is what makes the assertion
    // below mean something — with the check removed, this test deletes the global and fails.
    let mut mcp = McpSession::start(&env_with(&[
        ("IRIS_WRITE_TOOLS_ENABLED", "1"),
        ("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1"),
    ]));

    let seed = mcp.call(
        "iris_execute",
        &serde_json::json!({
            "code": format!(
                "Kill {TEST_GLOBAL} Set {TEST_GLOBAL}(1)=\"a\" Set {TEST_GLOBAL}(2)=\"b\" \
                 Set {TEST_GLOBAL}(3)=\"c\" Write \"seeded\",!"
            )
        }),
    );
    assert!(
        answer_text(&seed).contains("seeded"),
        "could not seed {TEST_GLOBAL}, so nothing below proves anything: {}",
        answer_text(&seed)
    );

    let before = payload(&mcp.call(
        "global_preview",
        &serde_json::json!({"global": TEST_GLOBAL, "server": "_env", "count": 10}),
    ));
    let entries_before = before
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len);
    assert_eq!(
        entries_before,
        Some(3),
        "expected the three seeded nodes: {before}"
    );
    let token = before
        .get("confirm_token")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("global_preview issued no confirm_token: {before}"))
        .to_string();

    let refused = mcp.call(
        "global_kill",
        &serde_json::json!({
            "global": TEST_GLOBAL,
            "server": "_env",
            "confirm_token": token,
            "recursive": true
        }),
    );
    assert_eq!(
        error_code(&refused),
        "UNKNOWN_PARAMETER",
        "`recursive` is not a global_kill parameter and had to be refused: {}",
        answer_text(&refused)
    );

    let after = payload(&mcp.call(
        "global_preview",
        &serde_json::json!({"global": TEST_GLOBAL, "server": "_env", "count": 10}),
    ));
    assert_eq!(
        after.get("entries"),
        before.get("entries"),
        "the refused call reached the handler — {TEST_GLOBAL} is not what it was.\nbefore: \
         {before}\nafter: {after}"
    );

    // Clean up through the tool, using only names it accepts. Also the positive control: the same
    // call minus the bogus key does delete the global, so the refusal above was the check and not
    // some unrelated failure.
    let token = after
        .get("confirm_token")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no confirm_token on the second preview: {after}"))
        .to_string();
    let killed = mcp.call(
        "global_kill",
        &serde_json::json!({"global": TEST_GLOBAL, "server": "_env", "confirm_token": token}),
    );
    assert_eq!(
        payload(&killed).get("success"),
        Some(&serde_json::json!(true)),
        "cleanup kill failed, which also means the refusal above proved nothing: {}",
        answer_text(&killed)
    );
}
