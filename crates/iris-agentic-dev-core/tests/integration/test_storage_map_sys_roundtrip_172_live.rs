//! T032: every class with a SQL storage map in the container's own `%SYS` round-trips through the
//! storage-map parser and writer byte for byte (spec 172 SC-001). Read-only.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_sys_roundtrip_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Where};
use iris_agentic_dev_core::tools::storage_map::tree::{
    emit_body, normalize, parse_body, split_class,
};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

fn content_text(got: &Value) -> Option<String> {
    got["content"].as_str().map(str::to_string).or_else(|| {
        got["content"].as_array().map(|lines| {
            lines
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
    })
}

#[tokio::test]
#[ignore]
async fn every_sys_sql_map_round_trips_byte_for_byte() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let q = r
        .call(
            "iris_query",
            json!({"query": "SELECT DISTINCT $PIECE(parent,'||',1) AS Cls FROM %Dictionary.CompiledStorageSQLMap",
                "namespace": "%SYS"}),
        )
        .await;
    assert_eq!(q["success"], true, "{q}");
    let classes: Vec<String> = q["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row["Cls"].as_str().map(str::to_string))
        .collect();
    assert!(!classes.is_empty(), "no classes with SQL maps in %SYS");
    let mut failures = Vec::new();
    for cls in &classes {
        let got = r
            .call(
                "iris_doc",
                json!({"mode": "get", "name": format!("{cls}.cls"), "namespace": "%SYS"}),
            )
            .await;
        let Some(text) = content_text(&got) else {
            failures.push(format!("{cls}: get: {got}"));
            continue;
        };
        let spans = match split_class(&text) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{cls}: split: {e:?}"));
                continue;
            }
        };
        for s in spans {
            let body = &text[s.body_start..s.body_end];
            match parse_body(body) {
                Ok(nodes) => {
                    if emit_body(&normalize(nodes)) != body {
                        failures.push(format!("{cls}: emitted text differs from the stored body"));
                    }
                }
                Err(e) => failures.push(format!("{cls}: parse: {e:?}")),
            }
        }
    }
    println!(
        "%SYS classes with SQL maps: {}, failures: {}",
        classes.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "{failures:#?}");
}
