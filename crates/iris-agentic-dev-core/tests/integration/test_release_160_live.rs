//! Open issues fixed for 1.6.0, on live IRIS.
//!
//! `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration
//! release_160 -- --ignored --test-threads=1`

use iris_agentic_dev_core::testing::{call_tool_with_env, live_env, require_iad_binary};
use serde_json::{json, Value};

const PROD: &str = r#"Class IADTest160.Prod Extends Ens.Production
{

XData ProductionDefinition
{
<Production Name="IADTest160.Prod">
<Item Name="From DEMO LabService ADT" ClassName="EnsLib.File.PassthroughService" PoolSize="1" Enabled="false"/>
<Item Name="To-Lab Main" ClassName="EnsLib.File.PassthroughOperation" PoolSize="1" Enabled="true"/>
</Production>
}

}"#;

async fn atelier(method: reqwest::Method, path: &str, body: Option<Value>) -> Value {
    let host = std::env::var("IRIS_HOST").expect("IRIS_HOST");
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    let mut req = iris_agentic_dev_core::testing::live_client()
        .request(
            method,
            format!("http://{host}:{port}/api/atelier/v1/USER/{path}"),
        )
        .basic_auth(user, Some(pass));
    if let Some(b) = body {
        req = req.json(&b);
    }
    req.send()
        .await
        .expect("Atelier answers")
        .json()
        .await
        .unwrap_or(Value::Null)
}

/// #144: item names with spaces made `mermaid_production` return a diagram Mermaid cannot parse.
#[tokio::test]
#[ignore]
async fn release_160_mermaid_production_ids_parse() {
    require_iad_binary().expect("binary");
    let lines: Vec<&str> = PROD.lines().collect();
    atelier(
        reqwest::Method::PUT,
        "doc/IADTest160.Prod.cls?ignoreConflict=1",
        Some(json!({"enc": false, "content": lines})),
    )
    .await;
    atelier(
        reqwest::Method::POST,
        "action/compile",
        Some(json!(["IADTest160.Prod.cls"])),
    )
    .await;
    let msg = call_tool_with_env(
        "mermaid_production",
        &json!({"production": "IADTest160.Prod", "namespace": "USER"}),
        &live_env(),
    );
    atelier(reqwest::Method::DELETE, "doc/IADTest160.Prod.cls", None).await;
    let text = msg["result"]["content"][0]["text"].as_str().unwrap_or("");
    let v: Value = serde_json::from_str(text).unwrap_or(json!({"raw": msg}));
    assert_eq!(v["item_count"], 2, "{v}");
    let diagram = v["diagram"].as_str().expect("diagram");
    let id = regex::Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap();
    for line in diagram.lines().skip(1) {
        let line = line.trim();
        let ids: Vec<&str> = match line.split_once(" --> ") {
            Some((a, b)) => vec![a, b],
            None => vec![line.split('[').next().unwrap()],
        };
        for i in ids {
            assert!(id.is_match(i), "node id {i:?} in {line:?}:\n{diagram}");
        }
    }
    assert!(diagram.contains("From DEMO LabService ADT"), "{diagram}");
}
