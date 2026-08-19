//! MCP tools that declare `outputSchema` must return matching `structuredContent`
//! (MCP 2025-06-18 structured tool output). Cursor and other strict clients reject
//! `call_tool` results that advertise a schema but omit structured content.
//!
//! These are real `call_for_test` dispatches — the same path MCP and CLI use — with
//! no live IRIS connection required for the tools exercised here.

use iris_agentic_dev_core::tools::{IrisTools, Toolset};

fn tools() -> IrisTools {
    IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new")
}

fn structured_body(result: &rmcp::model::CallToolResult, tool: &str) -> serde_json::Value {
    let structured = result
        .structured_content
        .as_ref()
        .unwrap_or_else(|| panic!("{tool} must return structuredContent when outputSchema is declared"));
    let text = result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.as_str())
        .unwrap_or_else(|| panic!("{tool} must still return text content for backward compatibility"));
    let from_text: serde_json::Value =
        serde_json::from_str(text).unwrap_or_else(|e| panic!("{tool} text content must be JSON: {e}"));
    assert_eq!(
        structured, &from_text,
        "{tool}: structuredContent must match serialized text content"
    );
    structured.clone()
}

async fn call_raw(tools: &IrisTools, tool: &str, args: serde_json::Value) -> rmcp::model::CallToolResult {
    tools
        .call_for_test(tool, args)
        .await
        .unwrap_or_else(|e| panic!("{tool} call failed: {e}"))
}

#[tokio::test]
async fn test_check_config_returns_structured_content() {
    let result = call_raw(&tools(), "check_config", serde_json::json!({})).await;
    let body = structured_body(&result, "check_config");
    assert!(body["host"].is_string() || body["host"].is_null());
    assert!(body["connected"].is_boolean());
}

#[tokio::test]
async fn test_skill_list_returns_structured_content() {
    let result = call_raw(&tools(), "skill_list", serde_json::json!({})).await;
    let body = structured_body(&result, "skill_list");
    assert!(body["skills"].is_array());
}

#[tokio::test]
async fn test_iris_symbols_local_error_returns_structured_content() {
    let result = call_raw(
        &tools(),
        "iris_symbols_local",
        serde_json::json!({
            "query": "*.cls",
            "workspace_path": "/nonexistent/path/xyz_9999_abc"
        }),
    )
    .await;
    let body = structured_body(&result, "iris_symbols_local");
    assert_eq!(body["error_code"], "WORKSPACE_NOT_FOUND");
}
