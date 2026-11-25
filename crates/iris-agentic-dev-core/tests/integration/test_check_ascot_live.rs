//! The ascot tools against a live IRIS: a stored document is fetched and checked, a wrong member
//! is an Error on its line and the corrected class is clean, and `iris_check_compile_execute` runs
//! its code only after a clean compile.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS
//! IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test --features testing --test integration
//! test_check_ascot -- --include-ignored --test-threads=1`

use iris_agentic_dev_core::testing::{live_env, tool_body, McpSession};
use serde_json::json;

/// The live connection with the ascot tools turned on.
fn session() -> McpSession {
    let mut env = live_env();
    env.retain(|(k, _)| k != "IRIS_CONTAINER");
    env.push(("IRIS_ASCOT".into(), "1".into()));
    McpSession::start(&env)
}

#[test]
#[ignore = "requires live IRIS"]
fn a_stored_document_is_fetched_and_a_wrong_system_member_is_an_error() {
    let mut s = session();
    let name = "Live.CheckProbe.cls";
    let source = "Class Live.CheckProbe\n{\nClassMethod Run()\n{\n Write ##class(%SYSTEM.Version).GetVersion()\n Write ##class(%SYSTEM.Version).NoSuchMethod()\n}\n}\n";
    let put = tool_body(&s.call(
        "iris_doc",
        &json!({"mode": "put", "name": name, "content": source, "namespace": "USER"}),
    ));
    assert_eq!(put["success"], true, "{put}");
    let b = tool_body(&s.call("iris_check", &json!({"name": name, "namespace": "USER"})));
    s.call(
        "iris_doc",
        &json!({"mode": "delete", "name": name, "namespace": "USER"}),
    );
    assert_eq!(b["success"], true, "{b}");
    assert_eq!(b["iris"], true, "{b}");
    assert!(b["note"].is_null(), "{b}");
    assert_eq!(b["level"], "error", "{b}");
    assert_eq!(b["errors"], 1, "{b}");
    let d = &b["diagnostics"][0];
    assert!(d["message"].to_string().contains("NoSuchMethod"), "{b}");
    let missing = tool_body(&s.call(
        "iris_check",
        &json!({"name": "Live.NoSuchDoc.cls", "namespace": "USER"}),
    ));
    assert_eq!(missing["error_code"], "DOCUMENT_NOT_FOUND", "{missing}");
}

/// CORPUS-50's shape: a property the class lacks, which IRIS compiles and fails on at run time.
const WRONG_MEMBER: &str = "Class Live.Card Extends %RegisteredObject
{
Property Title As %String;
Property Owner As %String;
Method Summary() As %String
{
    Quit ..Title_\" (\"_..Owner_\")\"
}
ClassMethod Example(pTitle As %String, pOwner As %String) As %String
{
    Set tCard = ..%New()
    Set tCard.Turtle = pTitle
    Set tCard.Owner = pOwner
    Quit tCard.Summary()
}
}
";

#[test]
#[ignore = "requires live IRIS"]
fn a_wrong_member_is_an_error_on_its_line_and_the_corrected_class_is_clean() {
    let mut s = session();
    let b = tool_body(&s.call(
        "iris_check",
        &json!({"name": "Live.Card.cls", "source": WRONG_MEMBER, "namespace": "USER"}),
    ));
    assert_eq!(b["errors"], 1, "{b}");
    let d = &b["diagnostics"][0];
    assert!(d["message"].to_string().contains("Turtle"), "{b}");
    assert_eq!(d["range"]["start"]["line"], 11, "{b}");
    let clean = WRONG_MEMBER.replace("tCard.Turtle", "tCard.Title");
    let b = tool_body(&s.call(
        "iris_check",
        &json!({"name": "Live.Card.cls", "source": clean, "namespace": "USER"}),
    ));
    assert_eq!(b["success"], true, "{b}");
    assert_eq!(b["errors"], 0, "{b}");
    assert_eq!(b["diagnostics"], json!([]), "{b}");
}

#[test]
#[ignore = "requires live IRIS"]
fn check_compile_execute_runs_the_code_only_after_a_clean_compile() {
    let mut s = session();
    let name = "Live.CheckCompileProbe.cls";
    let good = "Class Live.CheckCompileProbe\n{\nClassMethod Twice(n As %Integer) As %Integer\n{\n    Quit n * 2\n}\n}\n";
    let b = tool_body(&s.call(
        "iris_check_compile_execute",
        &json!({
            "name": name, "content": good,
            "code": "Set x = ##class(Live.CheckCompileProbe).Twice(21)\nWrite x",
            "namespace": "USER",
        }),
    ));
    assert_eq!(b["success"], true, "{b}");
    assert_eq!(b["analysis"]["errors"], 0, "{b}");
    assert_eq!(b["compile"]["ok"], true, "{b}");
    assert_eq!(b["output"].as_str().map(str::trim), Some("42"), "{b}");
    assert!(b["error"].is_null(), "{b}");

    let broken = good.replace("Quit n * 2", "Quit n *");
    let b = tool_body(&s.call(
        "iris_check_compile_execute",
        &json!({"name": name, "content": broken, "code": "Write 1", "namespace": "USER"}),
    ));
    s.call(
        "iris_doc",
        &json!({"mode": "delete", "name": name, "namespace": "USER"}),
    );
    assert_eq!(b["success"], true, "{b}");
    assert!(b["analysis"]["errors"].as_u64() >= Some(1), "{b}");
    assert_eq!(b["compile"]["ok"], false, "{b}");
    assert!(b["output"].is_null(), "{b}");
    assert!(b["error"].to_string().contains("did not compile"), "{b}");
}
