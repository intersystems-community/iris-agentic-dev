//! Spec 141 R5 C20 and C21: the caller's `dataPolicy` is a closed set that can only narrow the
//! server's, and both spellings of `data_policy` and `acknowledge_phi` reach one struct.

use crate::support_gate_141::*;
use iris_agentic_dev_core::iris::workspace_config::DataPolicy;
use iris_agentic_dev_core::tools::interop::resolve_body_policy;
use iris_agentic_dev_core::tools::params::batch5::IrisMessageBodyParams;
use serde_json::json;

fn code(r: Result<DataPolicy, serde_json::Value>) -> String {
    match r {
        Ok(p) => format!("ok:{p:?}"),
        Err(b) => b["error_code"].as_str().unwrap_or("").to_string(),
    }
}

/// C20. `"Allow"`, `"allow "` and `" ALLOW"` are `allow`, so they need the acknowledgement. Any
/// other spelling is an input error, not a policy that happens to pass both string tests.
#[test]
fn near_miss_policy_never_returns_raw_body() {
    for v in ["Allow", "allow ", " ALLOW", "allow"] {
        assert_eq!(
            code(resolve_body_policy(&DataPolicy::Allow, Some(v), false)),
            "PHI_ACK_REQUIRED",
            "{v:?} is allow and needs the acknowledgement"
        );
        assert_eq!(
            code(resolve_body_policy(&DataPolicy::Allow, Some(v), true)),
            "ok:Allow",
            "{v:?}"
        );
    }
    for v in ["redacted", "none", "", "  ", "alow", "true"] {
        for server in [DataPolicy::Allow, DataPolicy::Redact, DataPolicy::Block] {
            assert_eq!(
                code(resolve_body_policy(&server, Some(v), true)),
                "INVALID_PARAMS",
                "{v:?} against {server:?}"
            );
        }
    }
}

/// The caller can narrow the server's policy and never widen it. An omitted argument means the
/// server's own policy (T177), so it is not read as `block`.
#[test]
fn the_effective_policy_is_the_stricter_of_the_two() {
    use DataPolicy::*;
    let cases = [
        (Allow, Some("allow"), true, "ok:Allow"),
        (Allow, Some("redact"), false, "ok:Redact"),
        (Allow, Some("block"), false, "PHI_POLICY_BLOCKED"),
        (Allow, None, false, "PHI_ACK_REQUIRED"),
        (Allow, None, true, "ok:Allow"),
        (Redact, Some("allow"), true, "ok:Redact"),
        (Redact, Some("redact"), false, "ok:Redact"),
        (Redact, Some("block"), false, "PHI_POLICY_BLOCKED"),
        (Redact, None, false, "ok:Redact"),
        (Block, Some("allow"), true, "PHI_POLICY_BLOCKED"),
        (Block, Some("redact"), false, "PHI_POLICY_BLOCKED"),
        (Block, None, false, "PHI_POLICY_BLOCKED"),
    ];
    for (server, caller, ack, want) in cases {
        assert_eq!(
            code(resolve_body_policy(&server, caller, ack)),
            want,
            "server {server:?}, caller {caller:?}, acknowledged {ack}"
        );
    }
}

/// C21. Every other parameter on the surface is snake_case, so an agent writes `acknowledge_phi`.
#[test]
fn both_spellings_deserialize_to_one_struct() {
    let camel: IrisMessageBodyParams = serde_json::from_value(json!({
        "message_id": "7", "acknowledgePhi": true, "dataPolicy": "allow"
    }))
    .unwrap();
    let snake: IrisMessageBodyParams = serde_json::from_value(json!({
        "message_id": "7", "acknowledge_phi": true, "data_policy": "allow"
    }))
    .unwrap();
    assert_eq!(camel.acknowledge_phi, Some(true));
    assert_eq!(camel.data_policy.as_deref(), Some("allow"));
    assert_eq!(snake.acknowledge_phi, camel.acknowledge_phi);
    assert_eq!(snake.data_policy, camel.data_policy);
    // The wire name stays camelCase (FR-041).
    let out = serde_json::to_value(&snake).unwrap();
    assert_eq!(out["acknowledgePhi"], true);
    assert_eq!(out["dataPolicy"], "allow");
}

/// The advertised schema is the closed set, so a client sees it before it calls.
#[test]
fn tools_list_shows_the_enum_on_data_policy() {
    let r = rig(PROD_LOCKED);
    let schema = r
        .tools
        .tool_catalogue()
        .into_iter()
        .find(|e| e.name == "iris_message_body")
        .expect("iris_message_body is registered")
        .input_schema;
    let prop = schema["properties"]["dataPolicy"].to_string();
    for v in ["block", "redact", "allow"] {
        assert!(
            prop.contains(&format!("\"{v}\"")),
            "{v} missing from {prop}"
        );
    }
}

/// Through the pipeline, both spellings of the caller's arguments are read by the handler: an
/// acknowledgement written `acknowledge_phi` is an acknowledgement, and the call reaches the
/// network (a connection error) instead of stopping at `PHI_ACK_REQUIRED` or `UNKNOWN_PARAMETER`.
#[tokio::test]
async fn snake_case_spellings_reach_the_handler() {
    let toml = "[policy.prod]\ndataPolicy = \"allow\"\nmcpTemplate = \"dev\"\n[instance.prod]\nhost = \"127.0.0.1\"\nweb_port = 1\n";
    let r = rig(toml);
    for args in [
        json!({"message_id": "1", "server": "prod", "dataPolicy": "allow", "acknowledgePhi": true}),
        json!({"message_id": "1", "server": "prod", "data_policy": "allow", "acknowledge_phi": true}),
    ] {
        let result = r
            .tools
            .call_for_test("iris_message_body", args.clone())
            .await;
        let body = match &result {
            Ok(res) => body_of(res),
            Err(e) => json!({"error": e}),
        };
        let code = code_of(&body);
        assert!(
            ![
                "UNKNOWN_PARAMETER",
                "PHI_ACK_REQUIRED",
                "PHI_POLICY_BLOCKED",
                "DATA_POLICY_BLOCKED",
                "INVALID_PARAMS"
            ]
            .contains(&code.as_str()),
            "{args}: stopped before the handler's IRIS call with {code}: {body}"
        );
    }
}

/// The pipeline renames the snake_case spellings before any gate reads them, so a gate and the
/// handler see one name (FR-032). The camelCase spelling wins when a caller sends both.
#[test]
fn the_pipeline_reads_one_spelling() {
    use iris_agentic_dev_core::tools::write_gate::normalize_dispatch_args;
    let mut m = json!({"acknowledge_phi": true, "data_policy": "redact"})
        .as_object()
        .unwrap()
        .clone();
    normalize_dispatch_args(&mut m);
    assert_eq!(m.get("acknowledgePhi"), Some(&json!(true)));
    assert_eq!(m.get("dataPolicy"), Some(&json!("redact")));
    assert!(!m.contains_key("acknowledge_phi") && !m.contains_key("data_policy"));

    let mut both = json!({"acknowledge_phi": false, "acknowledgePhi": true})
        .as_object()
        .unwrap()
        .clone();
    normalize_dispatch_args(&mut both);
    assert_eq!(both.get("acknowledgePhi"), Some(&json!(true)));
    assert!(!both.contains_key("acknowledge_phi"));
}
