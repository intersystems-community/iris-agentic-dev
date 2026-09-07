//! #135 against live IRIS: a class that documents `CodeMode = objectgenerator` reaches IRIS and
//! compiles, and a class that actually declares it is refused with nothing left behind.
//!
//! The unit tests in `test_code_edit_gate_unit.rs` cover the scan itself. These cover the thing the
//! reporter measured, which the unit tests cannot: the document really is written, really compiles,
//! and the refusal really is a refusal rather than a write followed by an error.
//!
//! The reporter's two false-positive classes are reproduced verbatim from the issue.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS IRIS_NAMESPACE=USER`.

use iris_agentic_dev_core::testing::{answer_text, live_env, require_iad_binary, McpSession};

/// `iris_doc` put is write-classified and delete is destructive, so both tiers have to be open or
/// the gate answers before the CodeMode scan ever runs — which would make these tests pass for the
/// wrong reason.
fn open_gates() -> Vec<(String, String)> {
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    env.push((
        "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
        "1".to_string(),
    ));
    env
}

/// `McpSession::call` hands back the whole JSON-RPC envelope; the tool's own JSON sits inside
/// `result.content[0].text`. Reading fields off the envelope gets `Null` for every one of them,
/// which reads as a product failure and is not one.
fn payload(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no text content in {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("content was not JSON ({e}): {text}"))
}

// Raw strings, not `\n\` continuations: a backslash-newline in Rust eats the next line's leading
// whitespace too, which pushes `Return` into column 1, where ObjectScript reads it as a label
// rather than a command. The class then stores fine and fails to compile for a reason that has
// nothing to do with the gate.

/// Reporter's case 1: a guardrail class warning teammates off generators.
const DOC_COMMENT_CLASS: &str = r#"Class IADP135.FalsePos
{

/// Never use CodeMode = objectgenerator here without load-source.
ClassMethod Ping() As %String
{
    Return "fp"
}

}
"#;

/// Reporter's case 2: the keyword as a return value.
const STRING_LITERAL_CLASS: &str = r#"Class IADP135.FalsePosStr
{

ClassMethod Ping() As %String
{
    Return "CodeMode = objectgenerator"
}

}
"#;

/// The true positive. Must stay refused. The keyword sits on line 4.
const GENERATOR_CLASS: &str = r#"Class IADP135.RealGenerator
{

ClassMethod Gen() As %Status [ CodeMode = objectgenerator ]
{
    Do %code.WriteLine(" Quit 1")
    Quit $$$OK
}

}
"#;

fn put(mcp: &mut McpSession, name: &str, content: &str) -> serde_json::Value {
    payload(&mcp.call(
        "iris_doc",
        &serde_json::json!({"mode": "put", "name": name, "content": content}),
    ))
}

fn delete(mcp: &mut McpSession, name: &str) {
    let _ = mcp.call(
        "iris_doc",
        &serde_json::json!({"mode": "delete", "name": name}),
    );
}

fn exists(mcp: &mut McpSession, name: &str) -> bool {
    let got = payload(&mcp.call(
        "iris_doc",
        &serde_json::json!({"mode": "get", "name": name}),
    ));
    got["success"].as_bool().unwrap_or(false)
}

#[test]
#[ignore]
fn a_class_documenting_the_keyword_in_a_comment_reaches_iris_and_compiles() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&open_gates());
    let name = "IADP135.FalsePos.cls";
    delete(&mut mcp, name); // a previous run that died mid-way must not decide this one

    let put_result = put(&mut mcp, name, DOC_COMMENT_CLASS);
    assert!(
        put_result["success"].as_bool().unwrap_or(false),
        "the reporter's documenting class must be written, got: {put_result}"
    );
    assert!(
        put_result.get("code_edit_blocked").is_none(),
        "no gate should fire, got: {put_result}"
    );

    let compiled = payload(&mcp.call("iris_compile", &serde_json::json!({"target": name})));
    assert!(
        compiled["success"].as_bool().unwrap_or(false),
        "the class must compile once written, got: {compiled}"
    );

    assert!(exists(&mut mcp, name), "the class must be readable back");
    delete(&mut mcp, name);
}

#[test]
#[ignore]
fn a_class_returning_the_keyword_as_a_string_reaches_iris_and_compiles() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&open_gates());
    let name = "IADP135.FalsePosStr.cls";
    delete(&mut mcp, name);

    let put_result = put(&mut mcp, name, STRING_LITERAL_CLASS);
    assert!(
        put_result["success"].as_bool().unwrap_or(false),
        "a string literal is data, not a keyword list, got: {put_result}"
    );

    let compiled = payload(&mcp.call("iris_compile", &serde_json::json!({"target": name})));
    assert!(
        compiled["success"].as_bool().unwrap_or(false),
        "the class must compile once written, got: {compiled}"
    );
    delete(&mut mcp, name);
}

#[test]
#[ignore]
fn a_real_generator_is_refused_and_never_reaches_iris() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&open_gates());
    let name = "IADP135.RealGenerator.cls";
    delete(&mut mcp, name);

    let refused = put(&mut mcp, name, GENERATOR_CLASS);
    assert_eq!(
        refused["error_code"], "COMPILE_TIME_EXEC_BLOCKED",
        "the true positive must stay refused, got: {refused}"
    );
    assert_eq!(
        refused["line"], 4,
        "the refusal must name the line, got: {refused}"
    );
    assert!(
        refused["line_text"]
            .as_str()
            .unwrap_or_default()
            .contains("CodeMode = objectgenerator"),
        "the refusal must quote the offending line, got: {refused}"
    );

    // The side effect is the assertion that matters: a refusal that already wrote is not a refusal.
    assert!(
        !exists(&mut mcp, name),
        "the refused class must not exist in IRIS"
    );
}
