//! Every tool and action that returns message-body content needs `acknowledgePhi=true` under
//! `dataPolicy = "allow"`, as `iris_message_body` does.
//!
//! `ACKNOWLEDGED_BODY_READS` is the one list. The first tests read the real tool registry, so a new
//! tool or argument that carries body content has to be declared there (or argued for below) before
//! this file passes. The rest send each reader down the gate in both spellings of the argument.

use crate::support_gate_141::*;
use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy};
use iris_agentic_dev_core::policy::data_policy_gate::{
    ACKNOWLEDGED_BODY_READS, BODY_READ_TOOLS, BULK_PHI_TOOLS, REDACTING_TOOLS,
};
use iris_agentic_dev_core::policy::refusal::REFUSAL_CODES;
use iris_agentic_dev_core::testing::gated;
use iris_agentic_dev_core::tools::write_gate::normalize_dispatch_args;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Bulk-data tools that return no message body, so the acknowledgement list does not name them.
/// `journal_search` returns a journal record's value, which is not a message; its handler already
/// withholds the value of a PHI-named global unless `acknowledgePhi` is set. A name leaves this list
/// only by joining `ACKNOWLEDGED_BODY_READS`.
const NOT_BODY_READERS: &[&str] = &["journal_search"];

/// tool name -> its input property names, over every toolset.
fn registry() -> BTreeMap<String, BTreeSet<String>> {
    let mut all: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for toolset in [Toolset::Baseline, Toolset::Merged, Toolset::Nostub] {
        let tools = IrisTools::new_with_toolset(None, toolset).expect("IrisTools");
        for name in tools.registered_tool_names() {
            let schema = tools.tool_input_schema(&name).unwrap_or(Value::Null);
            let props = schema["properties"]
                .as_object()
                .map(|o| o.keys().cloned().collect::<BTreeSet<_>>())
                .unwrap_or_default();
            all.entry(name).or_default().extend(props);
        }
    }
    all
}

fn is_body_property(prop: &str) -> bool {
    prop.contains("body") || prop == "search_table"
}

#[test]
fn every_declared_reader_is_a_registered_tool_with_that_argument() {
    let reg = registry();
    for (tool, args) in ACKNOWLEDGED_BODY_READS {
        let props = reg
            .get(*tool)
            .unwrap_or_else(|| panic!("{tool} is not a registered tool"));
        for arg in *args {
            assert!(
                props.contains(*arg),
                "{tool} has no `{arg}` argument; a misspelt name here fails open"
            );
        }
        assert!(
            props.contains("acknowledgePhi"),
            "{tool} cannot be acknowledged: it has no `acknowledgePhi` argument"
        );
    }
}

#[test]
fn every_tool_that_looks_like_a_body_reader_is_declared() {
    let declared: BTreeMap<&str, &[&str]> = ACKNOWLEDGED_BODY_READS.iter().copied().collect();
    for (tool, props) in registry() {
        if NOT_BODY_READERS.contains(&tool.as_str()) {
            continue;
        }
        if tool.contains("body") || tool == "stream_inspect" {
            let args = declared
                .get(tool.as_str())
                .unwrap_or_else(|| panic!("{tool} returns body content and is not declared"));
            assert!(
                args.is_empty(),
                "{tool} is read whole: every call is a body read"
            );
        }
        for prop in props.iter().filter(|p| is_body_property(p)) {
            let args = declared.get(tool.as_str()).unwrap_or_else(|| {
                panic!("{tool} takes `{prop}`, which names body content; declare it")
            });
            assert!(
                args.is_empty() || args.contains(&prop.as_str()),
                "{tool} takes `{prop}`, which names body content; declare it"
            );
        }
    }
}

#[test]
fn the_acknowledgement_list_and_the_bulk_data_gate_cannot_drift() {
    let declared: BTreeMap<&str, &[&str]> = ACKNOWLEDGED_BODY_READS.iter().copied().collect();
    // Every argument that makes `iris_interop_query` a body read for the bulk-data gate needs the
    // acknowledgement too.
    for (tool, args) in BODY_READ_TOOLS {
        let acked = declared
            .get(tool)
            .unwrap_or_else(|| panic!("{tool} is a body-read tool the acknowledgement skips"));
        for arg in *args {
            assert!(
                acked.is_empty() || acked.contains(arg),
                "{tool} {arg} is a body read the acknowledgement skips"
            );
        }
    }
    // Every bulk-data tool is either argued out above or declared whole.
    for tool in BULK_PHI_TOOLS {
        if NOT_BODY_READERS.contains(tool) {
            continue;
        }
        let acked = declared
            .get(tool)
            .unwrap_or_else(|| panic!("{tool} is a bulk-data tool the acknowledgement skips"));
        assert!(acked.is_empty(), "{tool} is read whole");
    }
    // A tool that returns body content under `redact` returns it under `allow`.
    for tool in REDACTING_TOOLS {
        assert!(declared.contains_key(tool), "{tool} redacts bodies");
    }
    // The exemption list names real bulk-data tools only.
    for tool in NOT_BODY_READERS {
        assert!(BULK_PHI_TOOLS.contains(tool), "{tool} is not a bulk tool");
        assert!(
            !declared.contains_key(tool),
            "{tool} is declared and exempt"
        );
    }
}

/// One call per body reader, as an agent would write it.
fn readers() -> Vec<(&'static str, Value)> {
    vec![
        (
            "iris_interop_query",
            json!({"what": "messages", "body_class": "Ens.StringContainer"}),
        ),
        (
            "iris_interop_query",
            json!({"what": "messages", "body_where": "1=1"}),
        ),
        (
            "iris_interop_query",
            json!({"what": "messages", "body_select": ["StringValue"]}),
        ),
        (
            "iris_interop_query",
            json!({"what": "messages", "search_table": {"prop": "P", "value": "v"}}),
        ),
        ("iris_message_body", json!({"message_id": "1"})),
        ("stream_inspect", json!({"oid": "1"})),
    ]
}

/// The gate as the router calls it: argument spellings normalised first, as `call_tool` does.
fn dispatch(tool: &str, policy: &ConnectionPolicy, args: &Value) -> Result<(), Value> {
    let mut a = args.clone();
    if let Some(map) = a.as_object_mut() {
        normalize_dispatch_args(map);
    }
    gated::dispatch(tool, "prod", Some(policy), &a)
}

fn allow() -> ConnectionPolicy {
    ConnectionPolicy {
        data_policy: Some(DataPolicy::Allow),
        ..ConnectionPolicy::default()
    }
}

#[test]
fn under_allow_every_body_reader_needs_acknowledge_phi_true_in_either_spelling() {
    for (tool, args) in readers() {
        let e = dispatch(tool, &allow(), &args).expect_err(&format!(
            "{tool} {args} was admitted without an acknowledgement"
        ));
        assert_eq!(e["error_code"], "PHI_ACK_REQUIRED", "{tool} {args}: {e}");
        for not_true in [json!(false), json!("true"), json!(1), json!(null)] {
            for key in ["acknowledgePhi", "acknowledge_phi"] {
                let mut a = args.clone();
                a[key] = not_true.clone();
                let e = dispatch(tool, &allow(), &a)
                    .expect_err(&format!("{tool} {key}={not_true} admitted {args}"));
                assert_eq!(e["error_code"], "PHI_ACK_REQUIRED", "{tool} {a}: {e}");
            }
        }
        for key in ["acknowledgePhi", "acknowledge_phi"] {
            let mut a = args.clone();
            a[key] = json!(true);
            dispatch(tool, &allow(), &a)
                .unwrap_or_else(|e| panic!("{tool} {a} must be admitted: {e}"));
        }
    }
}

#[test]
fn an_acknowledgement_does_not_lift_block_or_redact_for_the_gate_tools() {
    for (tool, args) in readers() {
        for key in ["acknowledgePhi", "acknowledge_phi"] {
            let mut a = args.clone();
            a[key] = json!(true);
            let block = ConnectionPolicy {
                data_policy: Some(DataPolicy::Block),
                ..ConnectionPolicy::default()
            };
            // `stream_inspect` answers a refused stream with its metadata, so its handler (not the
            // gate) judges `block`; the others are refused before any IRIS request.
            if tool != "stream_inspect" {
                let e = dispatch(tool, &block, &a).expect_err(&format!("{tool} {a} under block"));
                assert_ne!(e["error_code"], "PHI_ACK_REQUIRED", "{tool}: {e}");
            }
        }
    }
}

const ALLOW: &str = "[policy.default]\ndataPolicy = \"allow\"\n";

/// Both tails refuse before any IRIS request (the default server is unroutable), and a call with
/// the acknowledgement gets past the gate in both spellings.
#[tokio::test]
async fn both_tails_refuse_and_admit() {
    let rig = rig_with(ALLOW, Toolset::Merged, &[]);
    let mut client = McpClient::start(rig.tools.clone()).await;
    for (tool, args) in readers() {
        let answer = rig
            .tools
            .call_gated(tool, args.clone())
            .await
            .expect("a refusal is an answer");
        assert_eq!(
            code_of(&body_of(&answer)),
            "PHI_ACK_REQUIRED",
            "{tool} {args}"
        );
        let body = client.call(tool, args.clone()).await;
        assert_eq!(code_of(&body), "PHI_ACK_REQUIRED", "{tool} {args}: {body}");
        assert_eq!(
            body["refusal"]["kind"], "acknowledgement_required",
            "{body}"
        );

        for key in ["acknowledgePhi", "acknowledge_phi"] {
            let mut a = args.clone();
            a[key] = json!(true);
            let answer = rig.tools.call_gated(tool, a.clone()).await;
            let body = answer
                .as_ref()
                .map(body_of)
                .unwrap_or_else(|e| json!({"error": e}));
            assert!(
                !REFUSAL_CODES.contains(&code_of(&body).as_str()),
                "{tool} {a} was refused: {body}"
            );
            assert_ne!(
                body["error_code"], "UNKNOWN_PARAMETER",
                "{tool} {a}: {body}"
            );
            let body = client.call(tool, a.clone()).await;
            assert!(
                !REFUSAL_CODES.contains(&code_of(&body).as_str()),
                "{tool} {a} was refused: {body}"
            );
            assert_ne!(
                body["error_code"], "UNKNOWN_PARAMETER",
                "{tool} {a}: {body}"
            );
        }
    }
}
