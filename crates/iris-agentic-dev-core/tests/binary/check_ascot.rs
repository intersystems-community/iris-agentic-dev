//! `iris_check` over stdio: its advertised contract, and one check of inline source with no IRIS
//! configured.

use iris_agentic_dev_core::testing::{
    assert_advertised_contracts, assert_source_contracts, require_iad_binary, McpSession,
    ParamContract,
};

const CONTRACT: &[ParamContract] = &[ParamContract {
    tool: "iris_check",
    params: &[
        ("name", "string"),
        ("source", "string"),
        ("namespace", "string"),
        ("server", "string"),
    ],
    required: &["name"],
}];

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_check_advertises_its_contract() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    assert_advertised_contracts(CONTRACT);
}

#[test]
fn iris_check_reads_what_it_advertises() {
    assert_source_contracts(CONTRACT);
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn inline_source_is_checked_without_iris() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer = McpSession::start(&[]).call(
        "iris_check",
        &serde_json::json!({"name": "Test.mac", "source": "ROUTINE Test [Type=MAC]\n Do Missing\n Quit\n"}),
    );
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content: {answer}"));
    let b: serde_json::Value = serde_json::from_str(text).expect("JSON body");
    assert_eq!(b["success"], true, "{b}");
    assert_eq!(b["iris"], false, "{b}");
    assert!(
        b["note"]
            .as_str()
            .unwrap_or("")
            .contains("No IRIS connection"),
        "{b}"
    );
    assert_eq!(b["errors"], 1, "{b}");
    assert_eq!(b["diagnostics"][0]["severity"], "Error", "{b}");
    assert_eq!(b["diagnostics"][0]["range"]["start"]["line"], 1, "{b}");
}
