//! Constitution IV: `iris_storage_map` with no IRIS connection answers `IRIS_UNREACHABLE` for every
//! action, before any action logic runs (spec 172 T010a).

use iris_agentic_dev_core::tools::{IrisTools, Toolset};

const ACTIONS: &[&str] = &[
    "read", "diagram", "uses", "compare", "preview", "propose", "check", "edit", "replace",
];

#[tokio::test]
async fn every_action_with_no_connection_is_iris_unreachable() {
    let t = IrisTools::for_test_gates_open(None, Toolset::Merged).expect("tools");
    for action in ACTIONS {
        // The pipeline refuses before the handler runs, as an MCP error whose message starts with
        // the code; a handler-level answer would carry it as `error_code`. Either is the code.
        let text = match t
            .call_gated(
                "iris_storage_map",
                serde_json::json!({"action": action, "class": "IadSm172.Member"}),
            )
            .await
        {
            Err(e) => e,
            Ok(res) => res.content[0]
                .as_text()
                .map(|t| t.text.clone())
                .unwrap_or_default(),
        };
        assert!(text.contains("IRIS_UNREACHABLE"), "{action}: {text}");
    }
}
