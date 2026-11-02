//! Security fix S5: an empty or whitespace-only `server` is `INVALID_PARAMS`, not the default
//! connection.
//!
//! Before the fix `server = ""` fell through to the default connection, so a caller that meant to
//! name a server and sent an empty string was judged by the default's policy. Every tool that
//! names a server is covered, through both tails of the pipeline.

use crate::support_gate_141::*;
use iris_agentic_dev_core::tools::write_gate::{TargetKind, TargetSource, CLASSIFICATION};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

const BLANKS: &[&str] = &["", " ", "\t", "  \n ", "\u{a0}"];

/// Every IRIS-kind tool in the toolset that names a server by argument, with the argument.
fn named_by_argument(
    names: &std::collections::HashSet<String>,
) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for r in CLASSIFICATION
        .iter()
        .filter(|r| r.kind == TargetKind::Iris && names.contains(r.tool))
    {
        match r.target {
            TargetSource::Arg("server") | TargetSource::ArgOrPending("server") => {
                out.push((r.tool, "server"))
            }
            TargetSource::Args(list) => out.extend(list.iter().map(|a| (r.tool, *a))),
            _ => {}
        }
    }
    out
}

fn assert_invalid_params(tail: &str, tool: &str, arg: &str, value: &str, body: &Value) {
    assert_eq!(
        code_of(body),
        "INVALID_PARAMS",
        "{tail}: {tool} {arg}={value:?} answered {body}"
    );
    assert_eq!(
        body["success"], false,
        "{tail}: {tool} {arg}={value:?}: {body}"
    );
    assert!(
        body["error"].as_str().unwrap_or("").contains(arg),
        "{tail}: the message does not name {arg}: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blank_server_is_invalid_params_for_every_tool_that_names_one() {
    let rig = rig(PROD_LOCKED);
    let list = named_by_argument(&rig.tools.registered_tool_names());
    assert!(list.len() > 20, "found only {} tools", list.len());
    for (tool, arg) in list {
        for value in BLANKS {
            let answer = rig.tools.call_gated(tool, json!({ arg: value })).await;
            let body = match &answer {
                Ok(r) => body_of(r),
                Err(e) => json!({"error": e.to_string()}),
            };
            assert_invalid_params("call_gated", tool, arg, value, &body);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blank_server_is_invalid_params_over_mcp() {
    let rig = rig(PROD_LOCKED);
    let mut client = McpClient::start(rig.tools.clone()).await;
    for (tool, arg) in [
        ("iris_execute", "server"),
        ("iris_query", "server"),
        ("iris_doc", "server"),
        ("iris_global", "server"),
        ("compare_namespace", "server_a"),
        ("compare_namespace", "server_b"),
    ] {
        for value in BLANKS {
            let body = client.call(tool, json!({ arg: value })).await;
            assert_invalid_params("mcp", tool, arg, value, &body);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn naming_a_server_or_none_is_unchanged() {
    let rig = rig_with(PROD_LOCKED, Toolset::Merged, &[("prod", UNROUTABLE)]);
    // A named server is judged by its own policy.
    let named = rig
        .tools
        .call_gated("iris_execute", json!({"code": "Write 1", "server": "prod"}))
        .await;
    let body = body_of(&named.expect("a refusal is a result"));
    assert_ne!(code_of(&body), "INVALID_PARAMS", "{body}");
    assert!(body.to_string().contains("prod"), "{body}");
    // No `server`, and a null one, mean the default connection.
    for args in [
        json!({"code": "Write 1"}),
        json!({"code": "Write 1", "server": null}),
    ] {
        let r = rig.tools.call_gated("iris_execute", args.clone()).await;
        let body = r
            .as_ref()
            .map(body_of)
            .unwrap_or_else(|e| json!({"error": e.to_string()}));
        assert_ne!(code_of(&body), "INVALID_PARAMS", "{args}: {body}");
    }
}
