//! Spec 141 T214: `iris_interop_query` body mode reads message bodies, so it sits under the
//! bulk-data gate. `body_class` joins the body table, `body_where` filters on it and `body_select`
//! returns its columns: any of the three makes the call a body read, whatever `what` says.
//!
//! The same call is sent down both tails of the pipeline, `call_gated` (the CLI and the tests) and
//! the MCP server over a pipe, and against the gate function directly. The default server is at an
//! address nothing listens on, so a call that passes the gates fails with a connection error, which
//! is not a refusal.

use crate::support_gate_141::*;
use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy};
use iris_agentic_dev_core::policy::refusal::REFUSAL_CODES;
use iris_agentic_dev_core::testing::gated;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

/// Every shape of body read the handler accepts.
fn body_reads() -> Vec<Value> {
    vec![
        json!({"what": "messages", "body_class": "Ens.StringContainer"}),
        json!({"what": "messages", "body_class": "Ens.StringContainer", "body_where": "1=1"}),
        json!({"what": "messages", "body_class": "Ens.StringContainer", "body_select": ["StringValue"]}),
        json!({"what": "messages", "body_where": "1=1"}),
        json!({"what": "messages", "body_select": ["StringValue"]}),
        // `what` is not read by the gate: the handler's default and its case are not a way round.
        json!({"body_class": "Ens.StringContainer"}),
        json!({"what": "Messages", "body_class": "Ens.StringContainer"}),
        json!({"what": "logs", "body_class": "Ens.StringContainer"}),
        // The Search Table returns the indexed body field itself (`PropValue`: an HL7 PID, a
        // name), so a search is a body read in every shape the handler takes.
        json!({"what": "messages", "search_table": {"prop": "P", "value": "v"}}),
        json!({"what": "messages", "search_table": {"prop": "P", "value_like": "v%"}}),
        json!({"what": "messages", "search_table": {"prop": "P", "value": "v", "class": "EnsLib.HL7.Message"}}),
        json!({"what": "messages", "search_table": {"prop": "P", "value": "v", "extent": "EnsLib.XML.SearchTable"}}),
        json!({"search_table": {"prop": "P", "value": "v"}}),
        json!({"what": "logs", "search_table": {"prop": "P", "value": "v"}}),
        // The handler ignores a filter it cannot read; the gate does not, so a malformed one is
        // no way round it.
        json!({"what": "messages", "search_table": "P"}),
        json!({"what": "messages", "search_table": {}}),
    ]
}

/// Calls that read headers, logs or queues and name no body.
fn header_reads() -> Vec<Value> {
    vec![
        json!({"what": "messages"}),
        json!({"what": "messages", "session_id": 1, "limit": 5}),
        json!({"what": "logs", "component": "x"}),
        json!({"what": "queues"}),
        json!({"what": "messages", "search_table": null}),
    ]
}

/// A Search Table search returns indexed body values, so under `allow` it needs the same
/// `acknowledgePhi` an `iris_message_body` read does. Any other body read does not (yet).
fn needs_ack(args: &Value) -> bool {
    args.get("search_table").is_some_and(|v| !v.is_null())
}

/// `args` with the acknowledgement in the spelling `key`.
fn acknowledged(args: &Value, key: &str) -> Value {
    let mut a = args.clone();
    a[key] = json!(true);
    a
}

fn policy(data_policy: DataPolicy) -> ConnectionPolicy {
    ConnectionPolicy {
        data_policy: Some(data_policy),
        ..ConnectionPolicy::default()
    }
}

#[test]
fn the_gate_refuses_every_body_read_unless_the_policy_is_allow() {
    for args in body_reads() {
        for p in [DataPolicy::Block, DataPolicy::Redact] {
            let e = gated::dispatch(
                "iris_interop_query",
                "prod",
                Some(&policy(p.clone())),
                &args,
            )
            .expect_err(&format!("{p:?} {args}"));
            assert_eq!(e["error_code"], "DATA_POLICY_BLOCKED", "{p:?} {args}: {e}");
            assert_eq!(e["tool_name"], "iris_interop_query", "{e}");
        }
        let allow = policy(DataPolicy::Allow);
        if needs_ack(&args) {
            let e = gated::dispatch("iris_interop_query", "prod", Some(&allow), &args)
                .expect_err(&format!("allow without an acknowledgement admitted {args}"));
            assert_eq!(e["error_code"], "PHI_ACK_REQUIRED", "{args}: {e}");
            // Anything but a boolean true is not an acknowledgement.
            for not_true in [json!(false), json!("true"), json!(1), json!(null)] {
                let mut a = args.clone();
                a["acknowledgePhi"] = not_true.clone();
                let e = gated::dispatch("iris_interop_query", "prod", Some(&allow), &a)
                    .expect_err(&format!("acknowledgePhi={not_true} admitted {args}"));
                assert_eq!(e["error_code"], "PHI_ACK_REQUIRED", "{a}: {e}");
            }
            let a = acknowledged(&args, "acknowledgePhi");
            gated::dispatch("iris_interop_query", "prod", Some(&allow), &a)
                .unwrap_or_else(|e| panic!("allow with acknowledgePhi must admit {a}: {e}"));
        } else {
            gated::dispatch("iris_interop_query", "prod", Some(&allow), &args)
                .unwrap_or_else(|e| panic!("allow must admit {args}: {e}"));
        }
    }
}

#[test]
fn an_unconfigured_server_is_on_block() {
    let e = gated::dispatch(
        "iris_interop_query",
        "prod",
        None,
        &json!({"what": "messages", "body_class": "Ens.StringContainer"}),
    )
    .expect_err("the strict default applies when no policy is set");
    assert_eq!(e["error_code"], "DATA_POLICY_BLOCKED");
}

#[test]
fn header_logs_and_queue_reads_are_not_body_reads() {
    for args in header_reads() {
        for p in [DataPolicy::Block, DataPolicy::Redact, DataPolicy::Allow] {
            let answer = gated::dispatch(
                "iris_interop_query",
                "prod",
                Some(&policy(p.clone())),
                &args,
            );
            assert!(
                answer.is_ok(),
                "{p:?} {args} must pass the bulk gate: {answer:?}"
            );
        }
    }
}

#[test]
fn only_this_tool_reads_body_arguments_as_a_body_read() {
    // `body_class` on a tool that has no such parameter is not this gate's business.
    gated::dispatch(
        "iris_query",
        "prod",
        Some(&policy(DataPolicy::Block)),
        &json!({"query": "SELECT 1", "body_class": "x"}),
    )
    .expect("iris_query is not a body read");
}

const BLOCK: &str = "[policy.default]\ndataPolicy = \"block\"\n";
const REDACT: &str = "[policy.default]\ndataPolicy = \"redact\"\n";
const ALLOW: &str = "[policy.default]\ndataPolicy = \"allow\"\n";

#[tokio::test]
async fn call_gated_refuses_body_reads_before_the_handler_runs() {
    for toml in [BLOCK, REDACT] {
        let rig = rig_with(toml, Toolset::Merged, &[]);
        for args in body_reads() {
            let answer = rig
                .tools
                .call_gated("iris_interop_query", args.clone())
                .await
                .expect("a refusal is an answer");
            let body = body_of(&answer);
            assert_eq!(
                code_of(&body),
                "DATA_POLICY_BLOCKED",
                "{toml} {args}: {body}"
            );
        }
    }
}

#[tokio::test]
async fn call_gated_lets_header_reads_through_and_allow_lets_body_reads_through() {
    let rig = rig_with(BLOCK, Toolset::Merged, &[]);
    for args in header_reads() {
        let answer = rig
            .tools
            .call_gated("iris_interop_query", args.clone())
            .await;
        let body = answer
            .as_ref()
            .map(body_of)
            .unwrap_or_else(|e| json!({"error": e}));
        assert!(
            !REFUSAL_CODES.contains(&code_of(&body).as_str()),
            "{args} was refused: {body}"
        );
    }
    let rig = rig_with(ALLOW, Toolset::Merged, &[]);
    for args in body_reads() {
        // A Search Table search is admitted with its acknowledgement, in either spelling.
        let args = if needs_ack(&args) {
            acknowledged(&args, "acknowledge_phi")
        } else {
            args
        };
        let answer = rig
            .tools
            .call_gated("iris_interop_query", args.clone())
            .await;
        let body = answer
            .as_ref()
            .map(body_of)
            .unwrap_or_else(|e| json!({"error": e}));
        assert!(
            !REFUSAL_CODES.contains(&code_of(&body).as_str()),
            "allow refused {args}: {body}"
        );
    }
}

#[tokio::test]
async fn the_mcp_tail_refuses_body_reads_too() {
    let rig = rig_with(BLOCK, Toolset::Merged, &[]);
    let mut client = McpClient::start(rig.tools.clone()).await;
    for args in body_reads() {
        let body = client.call("iris_interop_query", args.clone()).await;
        assert_eq!(code_of(&body), "DATA_POLICY_BLOCKED", "{args}: {body}");
    }
    let body = client
        .call("iris_interop_query", json!({"what": "messages"}))
        .await;
    assert!(
        !REFUSAL_CODES.contains(&code_of(&body).as_str()),
        "a header read was refused: {body}"
    );
}

/// Under `allow`, a Search Table search without the acknowledgement is refused with
/// `PHI_ACK_REQUIRED` before the handler runs, whichever tail it comes in on and in either
/// spelling of the argument; with it, the call reaches the handler (a connection error here).
#[tokio::test]
async fn under_allow_a_search_table_search_needs_the_acknowledgement() {
    let rig = rig_with(ALLOW, Toolset::Merged, &[]);
    let mut client = McpClient::start(rig.tools.clone()).await;
    for args in body_reads().into_iter().filter(needs_ack) {
        let answer = rig
            .tools
            .call_gated("iris_interop_query", args.clone())
            .await
            .expect("a refusal is an answer");
        assert_eq!(code_of(&body_of(&answer)), "PHI_ACK_REQUIRED", "{args}");
        let body = client.call("iris_interop_query", args.clone()).await;
        assert_eq!(code_of(&body), "PHI_ACK_REQUIRED", "{args}: {body}");
        assert_eq!(
            body["refusal"]["kind"], "acknowledgement_required",
            "{args}: {body}"
        );

        for key in ["acknowledgePhi", "acknowledge_phi"] {
            let a = acknowledged(&args, key);
            let answer = rig.tools.call_gated("iris_interop_query", a.clone()).await;
            let body = answer
                .as_ref()
                .map(body_of)
                .unwrap_or_else(|e| json!({"error": e}));
            assert!(
                !REFUSAL_CODES.contains(&code_of(&body).as_str()),
                "{key}: {a} was refused: {body}"
            );
            assert_ne!(
                body["error_code"], "UNKNOWN_PARAMETER",
                "{key}: {a}: {body}"
            );
            let body = client.call("iris_interop_query", a.clone()).await;
            assert!(
                !REFUSAL_CODES.contains(&code_of(&body).as_str()),
                "{key}: {a} was refused: {body}"
            );
            assert_ne!(
                body["error_code"], "UNKNOWN_PARAMETER",
                "{key}: {a}: {body}"
            );
        }
    }
}

/// The acknowledgement is for an `allow` server. Under `block` and `redact` the bulk-data gate
/// refuses first, and an acknowledgement does not lift that.
#[test]
fn an_acknowledgement_does_not_lift_the_data_policy() {
    let a = acknowledged(
        &json!({"what": "messages", "search_table": {"prop": "P", "value": "v"}}),
        "acknowledgePhi",
    );
    for p in [DataPolicy::Block, DataPolicy::Redact] {
        let e = gated::dispatch("iris_interop_query", "prod", Some(&policy(p.clone())), &a)
            .expect_err(&format!("{p:?}"));
        assert_eq!(e["error_code"], "DATA_POLICY_BLOCKED", "{p:?}: {e}");
    }
}
