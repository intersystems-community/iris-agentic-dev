//! Integration tests for interop depth tools (056-interop-depth).
//! Requires live IRIS on iris-dev-iris container (port from IRIS_HOST/IRIS_WEB_PORT env).
//! All tests are #[ignore] — run with:
//!   cargo test -p iris-agentic-dev-core --features testing --test test_interop_depth_live -- --include-ignored

use iris_agentic_dev_core::iris::connection::DiscoverySource;
use iris_agentic_dev_core::iris::IrisConnection;
use iris_agentic_dev_core::tools::interop::{
    handle_iris_business_rule_info, handle_iris_message_body, handle_iris_production_diff,
    BusinessRuleInfoParams, MessageBodyParams, ProductionDiffParams,
};

fn parse_result(result: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = result
        .content
        .first()
        .map(|c| c.as_text().unwrap().text.clone())
        .expect("text content");
    serde_json::from_str(&text).expect("valid JSON")
}

fn live_iris() -> Option<IrisConnection> {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52773);
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    Some(IrisConnection::new(
        format!("http://{}:{}", host, port),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    ))
}

/// Seed a real Ens.MessageHeader + Ens.StringContainer body via raw ObjectScript,
/// returning the new header ID. Used to exercise the success path of
/// handle_iris_message_body, which otherwise only reaches MESSAGE_NOT_FOUND on a
/// fresh dev instance with no real Ensemble traffic.
async fn seed_message_body(iris: &IrisConnection, body_text: &str) -> Option<i64> {
    let client = IrisConnection::http_client().ok()?;
    let code = format!(
        r#"Set body=##class(Ens.StringContainer).%New("{}")
Set sc=body.%Save()
If $$$ISERR(sc) {{ Write "ERR" Quit }}
Set hdr=##class(Ens.MessageHeader).%New()
Set hdr.MessageBodyClassName=body.%ClassName(1)
Set hdr.MessageBodyId=body.%Id()
Set hdr.SourceConfigName="IrisDevTest"
Set sc2=hdr.%Save()
If $$$ISERR(sc2) {{ Write "ERR" Quit }}
Write hdr.%Id()"#,
        body_text.replace('"', "\"\"")
    );
    let out = iris
        .execute_via_generator(&code, "USER", &client)
        .await
        .ok()?;
    out.trim().parse().ok()
}

/// Seed an Ens.Rule.RuleSet with the given name, returning Some(()) on success.
async fn seed_rule_set(iris: &IrisConnection, name: &str) -> Option<()> {
    let client = IrisConnection::http_client().ok()?;
    let code = format!(
        r#"Set rs=##class(Ens.Rule.RuleSet).%New()
Set rs.Name="{name}"
Set rs.ShortDescription="Test rule set seeded by test_interop_depth_live"
Set rs.HostClass="EnsLib.MsgRouter.RoutingEngine"
Set sc=rs.%Save()
Write $Select($$$ISERR(sc):"ERR",1:"OK")"#,
        name = name.replace('"', "\"\"")
    );
    let out = iris
        .execute_via_generator(&code, "USER", &client)
        .await
        .ok()?;
    if out.trim() == "OK" {
        Some(())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// T022 / US1: iris_message_body live — real message returns body + content_type
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_message_body_real_message_returns_content() {
    let iris = live_iris().expect("live_iris always returns Some");
    let body_text = "MSH|^~\\&|TEST|TEST|||20260101||ADT^A01|1|P|2.3";
    let msg_id = seed_message_body(&iris, body_text)
        .await
        .expect("seed_message_body must succeed on live IRIS");

    let params = MessageBodyParams {
        message_id: msg_id.to_string(),
        namespace: "USER".to_string(),
        max_bytes: 65536,
        acknowledge_phi: true,
    };
    let result = handle_iris_message_body(Some(&iris), &params, "allow")
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "got: {v}");
    assert_eq!(v["content_type"], "HL7v2");
    assert!(v["body"].as_str().unwrap_or("").contains("MSH|"));
}

#[tokio::test]
#[ignore]
async fn live_message_body_real_message_redacts_with_redact_policy() {
    let iris = live_iris().expect("live_iris always returns Some");
    let body_text =
        "MSH|^~\\&|SendingApp|FAC|||20260101||ADT^A01|1|P|2.3\rPID|1||12345||DOE^JANE||19800101|F";
    let msg_id = seed_message_body(&iris, body_text)
        .await
        .expect("seed_message_body must succeed on live IRIS");

    let params = MessageBodyParams {
        message_id: msg_id.to_string(),
        namespace: "USER".to_string(),
        max_bytes: 65536,
        acknowledge_phi: false,
    };
    let result = handle_iris_message_body(Some(&iris), &params, "redact")
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "got: {v}");
    let body = v["body"].as_str().unwrap_or("");
    assert!(body.contains("[REDACTED]"), "got: {body}");
    assert!(!body.contains("DOE^JANE"), "got: {body}");
}

// ---------------------------------------------------------------------------
// T022 / US1: iris_message_body live — unknown ID returns MESSAGE_NOT_FOUND
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_message_body_unknown_id_returns_message_not_found() {
    let iris = live_iris();
    let params = MessageBodyParams {
        message_id: "999999999".to_string(),
        namespace: "USER".to_string(),
        max_bytes: 65536,
        acknowledge_phi: true,
    };
    let result = handle_iris_message_body(iris.as_ref(), &params, "allow")
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "MESSAGE_NOT_FOUND", "got: {v}");
}

// ---------------------------------------------------------------------------
// T028 / US2: iris_business_rule_info list — succeeds (possibly empty) on dev IRIS
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_business_rule_info_list_returns_success() {
    let iris = live_iris();
    let params = BusinessRuleInfoParams {
        action: "list".to_string(),
        rule_name: None,
        namespace: "USER".to_string(),
    };
    let result = handle_iris_business_rule_info(iris.as_ref(), &params)
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "business_rule_info list failed: {v}");
    assert!(v["rules"].is_array());
}

// ---------------------------------------------------------------------------
// T029 / US2: iris_business_rule_info get — non-existent rule returns RULE_NOT_FOUND
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_business_rule_info_get_nonexistent_returns_rule_not_found() {
    let iris = live_iris();
    let params = BusinessRuleInfoParams {
        action: "get".to_string(),
        rule_name: Some("DoesNotExist.RuleSet99".to_string()),
        namespace: "USER".to_string(),
    };
    let result = handle_iris_business_rule_info(iris.as_ref(), &params)
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "RULE_NOT_FOUND", "got: {v}");
}

#[tokio::test]
#[ignore]
async fn live_business_rule_info_get_real_rule_returns_success() {
    let iris = live_iris().expect("live_iris always returns Some");
    let rule_name = "Test.IrisDevTestRuleSetGet";
    seed_rule_set(&iris, rule_name)
        .await
        .expect("seed_rule_set must succeed on live IRIS");

    let params = BusinessRuleInfoParams {
        action: "get".to_string(),
        rule_name: Some(rule_name.to_string()),
        namespace: "USER".to_string(),
    };
    let result = handle_iris_business_rule_info(Some(&iris), &params)
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "got: {v}");
    assert_eq!(v["name"], rule_name);
    assert!(v["conditions"].is_array());
    assert!(v["actions"].is_array());
}

#[tokio::test]
#[ignore]
async fn live_business_rule_info_list_includes_seeded_rule() {
    let iris = live_iris().expect("live_iris always returns Some");
    let rule_name = "Test.IrisDevTestRuleSetList";
    seed_rule_set(&iris, rule_name)
        .await
        .expect("seed_rule_set must succeed on live IRIS");

    let params = BusinessRuleInfoParams {
        action: "list".to_string(),
        rule_name: None,
        namespace: "USER".to_string(),
    };
    let result = handle_iris_business_rule_info(Some(&iris), &params)
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["success"], true, "got: {v}");
    let rules = v["rules"].as_array().expect("rules must be array");
    assert!(
        rules.iter().any(|r| r["name"] == rule_name),
        "seeded rule '{rule_name}' must appear in list: {rules:?}"
    );
}

// ---------------------------------------------------------------------------
// T036 / US3: iris_production_diff — NO_SCM or NO_PRODUCTION on dev IRIS
// (no SCM configured, no production running on iris-dev-iris by default)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_production_diff_no_scm_or_no_production() {
    let iris = live_iris();
    let params = ProductionDiffParams {
        production: None,
        namespace: "USER".to_string(),
    };
    let result = handle_iris_production_diff(iris.as_ref(), &params)
        .await
        .expect("Ok");
    let v = parse_result(result);
    // Either is an acceptable, non-panicking structured outcome on a fresh dev instance.
    let code = v["error_code"].as_str().unwrap_or("");
    assert!(
        code == "NO_PRODUCTION" || code == "NO_SCM" || v["success"] == true,
        "expected NO_PRODUCTION, NO_SCM, or success; got: {v}"
    );
}

// ---------------------------------------------------------------------------
// T039: PHI gate integration — dataPolicy=block always blocks message_body
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn live_message_body_block_always_blocks() {
    let iris = live_iris();
    let params = MessageBodyParams {
        message_id: "1".to_string(),
        namespace: "USER".to_string(),
        max_bytes: 65536,
        acknowledge_phi: false,
    };
    let result = handle_iris_message_body(iris.as_ref(), &params, "block")
        .await
        .expect("Ok");
    let v = parse_result(result);
    assert_eq!(v["error_code"], "PHI_POLICY_BLOCKED");
}

#[tokio::test]
#[ignore]
async fn live_message_body_allow_with_ack_proceeds_to_iris() {
    let iris = live_iris();
    let params = MessageBodyParams {
        message_id: "999999998".to_string(),
        namespace: "USER".to_string(),
        max_bytes: 65536,
        acknowledge_phi: true,
    };
    let result = handle_iris_message_body(iris.as_ref(), &params, "allow")
        .await
        .expect("Ok");
    let v = parse_result(result);
    // Gate must not block — proceeds to IRIS and gets MESSAGE_NOT_FOUND (not PHI codes).
    assert_eq!(v["error_code"], "MESSAGE_NOT_FOUND", "got: {v}");
}
