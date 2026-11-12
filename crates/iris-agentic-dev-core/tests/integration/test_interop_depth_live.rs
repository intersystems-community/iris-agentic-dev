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

/// Removes every version of a seeded rule set. A seeded row that is never removed stays in the
/// namespace for good: 228 of them had piled up before this existed.
async fn drop_rule_set(iris: &IrisConnection, name: &str) {
    let Ok(client) = IrisConnection::http_client() else {
        return;
    };
    let code = format!(
        r#"Set rs=##class(%SQL.Statement).%ExecDirect(,"SELECT ID FROM Ens_Rule.RuleSet WHERE Name = ?","{name}")
While rs.%Next() {{ Do ##class(Ens.Rule.RuleSet).%DeleteId(rs.%Get("ID")) }}
Write "OK""#,
        name = name.replace('"', "\"\"")
    );
    let _ = iris.execute_via_generator(&code, "USER", &client).await;
}

/// What `get` says about a rule once its seed has been dropped.
async fn get_after_drop(iris: &IrisConnection, name: &str) -> serde_json::Value {
    let params = BusinessRuleInfoParams {
        action: "get".to_string(),
        rule_name: Some(name.to_string()),
        namespace: "USER".to_string(),
    };
    let result = handle_iris_business_rule_info(Some(iris), &params)
        .await
        .expect("Ok");
    parse_result(result)
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
    drop_rule_set(&iris, rule_name).await;
    assert_eq!(v["success"], true, "got: {v}");
    assert_eq!(v["name"], rule_name);
    assert!(v["conditions"].is_array());
    assert!(v["actions"].is_array());
    let gone = get_after_drop(&iris, rule_name).await;
    assert_eq!(
        gone["error_code"], "RULE_NOT_FOUND",
        "seed left behind: {gone}"
    );
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
    drop_rule_set(&iris, rule_name).await;
    let rules = v["rules"].as_array().expect("rules must be array");
    assert!(
        rules.iter().any(|r| r["name"] == rule_name),
        "seeded rule '{rule_name}' must appear in list: {rules:?}"
    );
    let gone = get_after_drop(&iris, rule_name).await;
    assert_eq!(
        gone["error_code"], "RULE_NOT_FOUND",
        "seed left behind: {gone}"
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

// ---------------------------------------------------------------------------
// 136: rule classes (Ens.Rule.Definition), the kind the Rule Editor opens
// ---------------------------------------------------------------------------

const RULE_CLASS: &str = "IADTest136.IntakeRule";

const RULE_CLASS_SOURCE: &str = r#"/// Intake outcome for the 136 live test
Class IADTest136.IntakeRule Extends Ens.Rule.Definition
{

Parameter RuleAssistClass = "Ens.Rule.GeneralBusinessRuleAssist";

XData RuleDefinition [ XMLNamespace = "http://www.intersystems.com/rule" ]
{
<ruleDefinition alias="" context="Ens.BusinessProcess" production="">
<ruleSet name="main" effectiveBegin="" effectiveEnd="">
<rule name="duplicate">
<when condition="(1=0)||(2=0)">
<return>"invalid"</return>
</when>
</rule>
<rule name="card" disabled="true">
<when condition="1=1">
<return>"routine"</return>
</when>
</rule>
<return>"further_review"</return>
</ruleSet>
</ruleDefinition>
}

}
"#;

fn atelier(path: &str) -> (reqwest::Client, String, String, String) {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52773".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    (
        iris_agentic_dev_core::testing::live_client(),
        format!("http://{host}:{port}/api/atelier/v1/USER/{path}"),
        user,
        pass,
    )
}

async fn put_and_compile_rule_class() {
    let (client, url, user, pass) = atelier(&format!("doc/{RULE_CLASS}.cls?ignoreConflict=1"));
    let lines: Vec<&str> = RULE_CLASS_SOURCE.lines().collect();
    let put = client
        .put(&url)
        .basic_auth(&user, Some(&pass))
        .json(&serde_json::json!({"enc": false, "content": lines}))
        .send()
        .await
        .expect("PUT reaches IRIS");
    assert!(put.status().is_success(), "PUT failed: {}", put.status());
    let (client, url, user, pass) = atelier("action/compile");
    let compiled: serde_json::Value = client
        .post(&url)
        .basic_auth(&user, Some(&pass))
        .json(&serde_json::json!([format!("{RULE_CLASS}.cls")]))
        .send()
        .await
        .expect("compile reaches IRIS")
        .json()
        .await
        .expect("compile answers JSON");
    assert_eq!(
        compiled["status"]["errors"].as_array().map(|e| e.len()),
        Some(0),
        "the rule class must compile: {compiled}"
    );
}

async fn delete_rule_class() {
    let (client, url, user, pass) = atelier(&format!("doc/{RULE_CLASS}.cls"));
    let _ = client
        .delete(&url)
        .basic_auth(&user, Some(&pass))
        .send()
        .await;
}

#[tokio::test]
#[ignore]
async fn live_business_rule_info_sees_a_rule_class() {
    let iris = live_iris().expect("live_iris always returns Some");
    put_and_compile_rule_class().await;

    let list = parse_result(
        handle_iris_business_rule_info(
            Some(&iris),
            &BusinessRuleInfoParams {
                action: "list".to_string(),
                rule_name: None,
                namespace: "USER".to_string(),
            },
        )
        .await
        .expect("Ok"),
    );
    let get = parse_result(
        handle_iris_business_rule_info(
            Some(&iris),
            &BusinessRuleInfoParams {
                action: "get".to_string(),
                rule_name: Some(RULE_CLASS.to_string()),
                namespace: "USER".to_string(),
            },
        )
        .await
        .expect("Ok"),
    );
    delete_rule_class().await;

    let listed = list["rules"]
        .as_array()
        .expect("rules array")
        .iter()
        .find(|r| r["name"] == RULE_CLASS)
        .cloned()
        .unwrap_or_else(|| panic!("the rule class is missing from list: {list}"));
    assert_eq!(listed["kind"], "class");
    assert_eq!(listed["rule_type"], "Ens.Rule.GeneralBusinessRuleAssist");
    assert_eq!(
        listed["description"],
        "Intake outcome for the 136 live test"
    );

    assert_eq!(get["success"], true, "get failed: {get}");
    assert_eq!(get["kind"], "class");
    assert_eq!(get["name"], RULE_CLASS);
    let set = &get["rule_sets"][0];
    assert_eq!(set["name"], "main");
    assert_eq!(set["rules"][0]["name"], "duplicate");
    assert_eq!(set["rules"][0]["whens"][0]["condition"], "(1=0)||(2=0)");
    assert_eq!(set["rules"][0]["whens"][0]["actions"][0]["type"], "return");
    assert_eq!(
        set["rules"][0]["whens"][0]["actions"][0]["value"],
        "\"invalid\""
    );
    assert_eq!(set["rules"][1]["disabled"], true);
    assert_eq!(set["actions"][0]["value"], "\"further_review\"");
    assert_eq!(get["conditions"].as_array().map(|c| c.len()), Some(2));
}

#[tokio::test]
#[ignore]
async fn live_business_rule_info_get_on_a_code_class_is_not_a_rule() {
    let iris = live_iris().expect("live_iris always returns Some");
    // A compiled class that is not a rule: get must not report it as one.
    let get = parse_result(
        handle_iris_business_rule_info(
            Some(&iris),
            &BusinessRuleInfoParams {
                action: "get".to_string(),
                rule_name: Some("Ens.BusinessProcess".to_string()),
                namespace: "USER".to_string(),
            },
        )
        .await
        .expect("Ok"),
    );
    assert_eq!(get["error_code"], "RULE_NOT_FOUND", "got: {get}");
}
