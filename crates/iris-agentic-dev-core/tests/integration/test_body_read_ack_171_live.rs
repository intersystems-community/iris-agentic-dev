//! Every body reader needs `acknowledgePhi=true` under `dataPolicy = "allow"`, against
//! `iris-dev-iris` and the built binary: without it the call is refused with `PHI_ACK_REQUIRED`
//! before any IRIS request; with it, in either spelling, it reaches IRIS.
//!
//! The ids are ones no instance holds, so a call that reaches IRIS answers `*_NOT_FOUND`, which a
//! refusal never does. Nothing in it needs data.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     body_read_ack_171 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::testing::{
    data_reads_allowed_env, live_env, require_iad_binary, McpSession,
};
use serde_json::{json, Value};

fn call(mcp: &mut McpSession, tool: &str, args: Value) -> Value {
    let answer = mcp.call(tool, &args);
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no text content in {answer}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

/// (tool, arguments, the answer code of a call that reaches IRIS with them; empty is success).
fn readers() -> Vec<(&'static str, Value, &'static str)> {
    vec![
        (
            "iris_interop_query",
            json!({"what": "messages", "body_class": "Ens.StringContainer", "limit": 1}),
            "",
        ),
        (
            "iris_interop_query",
            json!({"what": "messages", "body_class": "Ens.StringContainer", "body_where": "1=0", "limit": 1}),
            "",
        ),
        (
            "iris_interop_query",
            json!({"what": "messages", "body_class": "Ens.StringContainer", "body_select": ["StringValue"], "limit": 1}),
            "",
        ),
        (
            "iris_interop_query",
            json!({"what": "messages", "limit": 1, "search_table": {
                "prop": "RootElement", "value": "IadNoSuchRoot", "extent": "EnsLib.XML.SearchTable"}}),
            "",
        ),
        (
            "iris_message_body",
            json!({"message_id": "987654321"}),
            "MESSAGE_NOT_FOUND",
        ),
        (
            "stream_inspect",
            json!({"oid": "987654321"}),
            "STREAM_NOT_FOUND",
        ),
    ]
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn under_allow_a_body_read_is_refused_without_the_acknowledgement() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut env = live_env();
    env.extend(data_reads_allowed_env());
    let mut mcp = McpSession::start(&env);
    for (tool, args, _) in readers() {
        let r = call(&mut mcp, tool, args.clone());
        assert_eq!(r["error_code"], "PHI_ACK_REQUIRED", "{tool} {args}: {r}");
        assert_eq!(
            r["refusal"]["kind"], "acknowledgement_required",
            "{tool} {args}: {r}"
        );
        for not_true in [json!(false), json!("true")] {
            let mut a = args.clone();
            a["acknowledgePhi"] = not_true.clone();
            let r = call(&mut mcp, tool, a.clone());
            assert_eq!(r["error_code"], "PHI_ACK_REQUIRED", "{tool} {a}: {r}");
        }
    }
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn under_allow_an_acknowledged_body_read_reaches_iris_in_either_spelling() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut env = live_env();
    env.extend(data_reads_allowed_env());
    let mut mcp = McpSession::start(&env);
    for (tool, args, reached) in readers() {
        for key in ["acknowledgePhi", "acknowledge_phi"] {
            let mut a = args.clone();
            a[key] = json!(true);
            let r = call(&mut mcp, tool, a.clone());
            if reached.is_empty() {
                assert_eq!(r["success"], true, "{tool} {a}: {r}");
                assert!(r.get("error_code").is_none(), "{tool} {a}: {r}");
            } else {
                assert_eq!(r["error_code"], reached, "{tool} {a}: {r}");
            }
        }
    }
}
