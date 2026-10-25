//! Spec 141 R5 C17, live half: `iris_coverage` runs a `%UnitTest.TestProduction` subclass the way
//! `iris_test` does.
//!
//! Against iris-dev-iris (an Interoperability namespace in USER). `IadCov141.ProdTest` extends
//! `%UnitTest.TestProduction` and calls `IadCov141.Target.Hit()`. `RunTest` cannot run such a class;
//! `.Run()` can, so a covered method that was hit shows the test ran. The classes are deleted at the
//! end.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration
//! test_coverage_dispatch_141 -- --include-ignored --test-threads=1`

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

const TARGET: &str = "Class IadCov141.Target Extends %RegisteredObject\n{\n\nClassMethod Hit() As %Integer\n{\n    Set x = 1\n    Set y = x + 1\n    Quit y\n}\n\n}\n";
const PROD: &str = "Class IadCov141.Prod Extends Ens.Production\n{\n\nXData ProductionDefinition\n{\n<Production Name=\"IadCov141.Prod\" TestingEnabled=\"true\" LogGeneralTraceEvents=\"false\">\n  <Description></Description>\n  <ActorPoolSize>1</ActorPoolSize>\n</Production>\n}\n\n}\n";
const TEST: &str = "Class IadCov141.ProdTest Extends %UnitTest.TestProduction\n{\n\nParameter PRODUCTION = \"IadCov141.Prod\";\n\nMethod TestHit()\n{\n    Do $$$AssertEquals(##class(IadCov141.Target).Hit(), 2, \"Hit returns 2\")\n}\n\n}\n";

fn conn() -> IrisConnection {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    )
}

fn body(r: &rmcp::model::CallToolResult) -> Value {
    r.structured_content.clone().unwrap_or(Value::Null)
}

async fn put(tools: &IrisTools, name: &str, content: &str) {
    let r = tools
        .call_for_test(
            "iris_doc",
            json!({"mode": "put", "name": name, "content": content, "compile": true, "namespace": "USER"}),
        )
        .await
        .expect("iris_doc answers");
    assert_ne!(r.is_error, Some(true), "put {name}: {:?}", body(&r));
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn coverage_runs_a_test_production_through_dot_run() {
    let tools = IrisTools::for_test_gates_open(Some(conn()), Toolset::Merged).expect("tools");
    use super::test_coverage_diag_141_live::{restore_unit_test_root, use_unit_test_root};
    // A fresh instance has no `^UnitTestRoot`, and iad does not set it.
    use_unit_test_root(false).await;
    put(&tools, "IadCov141.Target.cls", TARGET).await;
    put(&tools, "IadCov141.Prod.cls", PROD).await;
    put(&tools, "IadCov141.ProdTest.cls", TEST).await;

    let r = tools
        .call_for_test(
            "iris_coverage",
            json!({"mode": "run", "classes": ["IadCov141.Target"], "test_path": "IadCov141.ProdTest", "namespace": "USER"}),
        )
        .await
        .expect("iris_coverage answers");
    let b = body(&r);

    // Teardown first, so a failed assertion leaves nothing behind.
    restore_unit_test_root().await;
    for name in [
        "IadCov141.ProdTest.cls",
        "IadCov141.Prod.cls",
        "IadCov141.Target.cls",
    ] {
        let _ = tools
            .call_for_test(
                "iris_doc",
                json!({"mode": "delete", "name": name, "namespace": "USER"}),
            )
            .await;
    }

    assert_eq!(b["success"], true, "coverage run failed: {b}");
    let classes = b["classes"].as_array().expect("classes array");
    let target = classes
        .iter()
        .find(|c| c["class"] == "IadCov141.Target")
        .unwrap_or_else(|| panic!("IadCov141.Target not in {classes:?}"));
    assert!(
        target["hit"].as_u64().unwrap_or(0) > 0,
        "the TestProduction did not run: no line of IadCov141.Target was hit: {target}"
    );
}
