//! Spec 141 T213 (FR-044): read-mode `iris_query` runs only `SELECT`, `WITH` and `EXPLAIN`.
//!
//! Sent through the pipeline (`call_gated`) and the MCP pipe, with every spelling of the mode the
//! tool accepts (`normalize_dispatch_args` trims and lower-cases it) and with it left out. The
//! default server is at an address nothing listens on, so a statement that got past the check
//! fails with a connection error, which is not `SQL_WRITE_BLOCKED`.

use crate::support_gate_141::*;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

const REFUSED: &[&str] = &[
    "CALL MyApp.Proc()",
    "call myapp.proc()",
    "/* c */ CALL MyApp.Proc()",
    "TUNE TABLE Sample.Person",
    "BUILD INDEX ON Sample.Person",
    "PURGE QUERIES",
    "SET OPTION SELECTMODE = ODBC",
    "FREEZE PLANS",
    "SELECT 1; CALL MyApp.Proc()",
];

fn modes() -> Vec<Value> {
    vec![
        json!({"mode": "read"}),
        json!({"mode": "Read"}),
        json!({"mode": "  READ "}),
        json!({}),
    ]
}

fn with_query(mode: &Value, query: &str) -> Value {
    let mut args = mode.clone();
    args["query"] = json!(query);
    args
}

#[tokio::test]
async fn call_gated_refuses_every_non_select_in_every_spelling_of_read_mode() {
    let rig = rig_with("", Toolset::Merged, &[]);
    let mut checked = 0;
    for mode in modes() {
        for q in REFUSED {
            let answer = rig
                .tools
                .call_gated("iris_query", with_query(&mode, q))
                .await
                .expect("a refusal is an answer");
            let body = body_of(&answer);
            assert_eq!(code_of(&body), "SQL_WRITE_BLOCKED", "{mode} {q}: {body}");
            let msg = body["error"].as_str().unwrap_or("");
            assert!(msg.contains("mode=\"write\""), "names write mode: {body}");
            checked += 1;
        }
    }
    assert_eq!(checked, 4 * REFUSED.len());
}

#[tokio::test]
async fn call_gated_lets_a_select_reach_iris() {
    let rig = rig_with("", Toolset::Merged, &[]);
    for mode in modes() {
        for q in [
            "SELECT 1",
            "WITH c AS (SELECT 1 AS a) SELECT a FROM c",
            "SELECT 1;",
        ] {
            // The handler's connection failure may come back as an error; either is not a refusal.
            let body = match rig
                .tools
                .call_gated("iris_query", with_query(&mode, q))
                .await
            {
                Ok(answer) => body_of(&answer),
                Err(e) => json!({"error": e}),
            };
            assert_ne!(code_of(&body), "SQL_WRITE_BLOCKED", "{mode} {q}: {body}");
        }
    }
}

#[tokio::test]
async fn the_mcp_tail_refuses_them_too() {
    let rig = rig_with("", Toolset::Merged, &[]);
    let mut client = McpClient::start(rig.tools.clone()).await;
    for q in REFUSED {
        let body = client.call("iris_query", json!({"query": q})).await;
        assert_eq!(code_of(&body), "SQL_WRITE_BLOCKED", "{q}: {body}");
    }
}
