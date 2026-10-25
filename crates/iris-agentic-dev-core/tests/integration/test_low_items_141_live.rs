//! Spec 141 T221, the low-severity items that need a live IRIS.
//!
//! Serial, `#[ignore]`, on the live server (`LiveTarget`: `iris-dev-iris` unless `IRIS_CONTAINER` says otherwise):
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration low_items_141 -- --ignored --test-threads=1`

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection, TerminalTarget};
use iris_agentic_dev_core::iris::connection_pool::ConnectionPool;
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::testing::{EnvGuard, LiveTarget};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};
use std::sync::Arc;

fn conn() -> IrisConnection {
    IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )
}

/// Real docker accepts `--` before the container: the argument list the local `docker exec` builds
/// still reaches IRIS.
#[tokio::test]
#[ignore = "live: docker and iris-dev-iris"]
async fn the_local_docker_exec_with_a_double_dash_still_reaches_iris() {
    let out = conn()
        .execute_terminal(
            "Write \"IAD141\",(6*7)",
            "USER",
            &TerminalTarget {
                container: LiveTarget::from_env().container,
                ssh_host: None,
                login: None,
                ssh_strict: false,
            },
        )
        .await
        .expect("docker exec reached IRIS");
    assert!(out.text.contains("IAD14142"), "{:?}", out.text);
}

// ── the cap on a body read from IRIS ────────────────────────────────────────────────────────────

fn dev_url() -> String {
    LiveTarget::from_env().base_url()
}

async fn live_tools() -> IrisTools {
    let mut c = IrisConnection::new(
        dev_url(),
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    );
    c.probe().await;
    let tools = IrisTools::with_registry_and_toolset(
        Some(c.clone()),
        SkillRegistry::new(),
        Toolset::Merged,
        None,
        None,
        true,
        Default::default(),
    )
    .unwrap();
    let mut pool = ConnectionPool::builder();
    pool.add("dev".to_string(), c, true);
    *tools.pool.write().unwrap() = Arc::new(pool.build());
    tools
}

async fn call(tools: &IrisTools, tool: &str, args: Value) -> Value {
    let r = tools.call_gated(tool, args).await.expect("a result");
    r.structured_content.clone().unwrap_or_else(|| {
        let text = r.content[0].as_text().expect("text").text.clone();
        serde_json::from_str(&text).unwrap_or(json!({"raw": text}))
    })
}

/// The USER namespace's document list is the largest answer a call returns here (1.6 MB). Under the
/// default cap it comes back whole; under a cap below its size each reader that met it reports
/// `RESPONSE_TOO_LARGE`, naming the cap, and not an empty list.
#[tokio::test]
#[ignore = "live: iris-dev-iris"]
async fn an_answer_over_the_cap_is_reported_not_returned_as_empty() {
    let tools = live_tools().await;
    let docs = json!({"what": "documents", "namespace": "USER", "inline": true});

    let mut g = EnvGuard::new();
    g.unset("IAD_MAX_RESPONSE_BYTES");
    let whole = call(&tools, "iris_info", docs.clone()).await;
    assert_eq!(
        whole["success"], true,
        "the default cap lets it through: {whole}"
    );
    assert!(
        whole["documents"].as_array().map_or(0, Vec::len) > 100
            || whole["result"]["content"].as_array().map_or(0, Vec::len) > 100,
        "the control: a long list came back"
    );

    g.set("IAD_MAX_RESPONSE_BYTES", "200000");
    let cut = call(&tools, "iris_info", docs).await;
    assert_eq!(cut["success"], false, "{cut}");
    assert_eq!(cut["error_code"], "RESPONSE_TOO_LARGE", "{cut}");
    let msg = cut.to_string();
    assert!(
        msg.contains("200000") && msg.contains("IAD_MAX_RESPONSE_BYTES"),
        "{msg}"
    );

    // The same pipeline net covers a reader in another tool: a 70 KB class source under a 10 KB cap.
    g.set("IAD_MAX_RESPONSE_BYTES", "10000");
    let doc = call(
        &tools,
        "iris_doc",
        json!({"mode": "get", "name": "%Dictionary.ClassDefinition.cls", "namespace": "%SYS"}),
    )
    .await;
    assert_eq!(doc["error_code"], "RESPONSE_TOO_LARGE", "{doc}");
}
