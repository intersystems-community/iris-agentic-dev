//! The ascot tools over stdio: listed only when the operator turns them on, their advertised
//! contracts, and checks with no IRIS configured and with one that does not answer.

use iris_agentic_dev_core::testing::{
    assert_advertised_contracts_with_env, assert_source_contracts, require_iad_binary, tool_body,
    McpSession, ParamContract,
};
use serde_json::json;

const CONTRACTS: &[ParamContract] = &[
    ParamContract {
        tool: "iris_check",
        params: &[
            ("name", "string"),
            ("source", "string"),
            ("level", "string"),
            ("namespace", "string"),
            ("server", "string"),
        ],
        required: &["name"],
    },
    ParamContract {
        tool: "iris_check_compile_execute",
        params: &[
            ("name", "string"),
            ("content", "string"),
            ("code", "string"),
            ("level", "string"),
            ("namespace", "string"),
            ("server", "string"),
        ],
        required: &[],
    },
];

fn on() -> Vec<(String, String)> {
    vec![("IRIS_ASCOT".into(), "1".into())]
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_ascot_tools_are_listed_only_when_the_operator_turns_them_on() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let ascot = |name: &String| name == "iris_check" || name == "iris_check_compile_execute";
    let off = McpSession::start(&[]).list_tools();
    assert!(!off.iter().any(ascot), "{off:?}");
    let on = McpSession::start(&on()).list_tools();
    assert_eq!(on.iter().filter(|n| ascot(n)).count(), 2, "{on:?}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_ascot_tools_advertise_their_contracts() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts_with_env(CONTRACTS, &[("IRIS_ASCOT", "1")]);
}

#[test]
fn the_ascot_tools_read_what_they_advertise() {
    assert_source_contracts(CONTRACTS);
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn inline_source_is_checked_without_iris() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // Discovery would find the e2e job's instance: aim the localhost scan at a port nobody holds,
    // and give the Docker scan after it no daemon.
    let mut env = on();
    env.push(("IAD_TEST_SCAN_PORTS".into(), "1".into()));
    env.push((
        "DOCKER_HOST".into(),
        "unix:///nonexistent-iad-ascot.sock".into(),
    ));
    let b = tool_body(&McpSession::start(&env).call(
        "iris_check",
        &json!({"name": "Test.mac", "source": "ROUTINE Test [Type=MAC]\n Do Missing\n Quit\n"}),
    ));
    assert_eq!(b["success"], true, "{b}");
    assert_eq!(b["iris"], false, "{b}");
    assert!(b["note"].to_string().contains("no IRIS connection"), "{b}");
    assert_eq!(b["level"], "error", "{b}");
    assert_eq!(b["errors"], 1, "{b}");
    assert_eq!(b["diagnostics"][0]["severity"], "Error", "{b}");
    assert_eq!(b["diagnostics"][0]["range"]["start"]["line"], 1, "{b}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_connection_that_does_not_answer_is_reported_not_passed_as_clean() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut env = on();
    env.push(("IRIS_HOST".into(), "127.0.0.1".into()));
    env.push(("IRIS_WEB_PORT".into(), "9".into()));
    let b = tool_body(&McpSession::start(&env).call(
        "iris_check",
        &json!({"name": "Demo.Probe.cls", "source": "Class Demo.Probe\n{\nClassMethod Run() As %String\n{\n    Do ..Nope()\n    Do ##class(Demo.Elsewhere).Run()\n    Quit ##class(%Library.String).NoSuch()\n}\n}\n"}),
    ));
    assert_eq!(b["iris"], false, "{b}");
    assert!(b["note"].to_string().contains("did not answer"), "{b}");
    assert_eq!(
        b["declarations"],
        ascot::BUNDLED_DECLARATIONS.unwrap().release,
        "{b}"
    );
    // Two errors from the bundled declarations; only the connection knows Demo.Elsewhere.
    assert_eq!(b["errors"], 2, "{b}");
}
