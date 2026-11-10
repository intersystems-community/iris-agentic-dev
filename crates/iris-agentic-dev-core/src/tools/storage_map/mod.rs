pub mod code;
pub mod diagram;
pub mod explain;
pub mod globals;
pub mod model;
pub mod path;
pub mod sample;
pub mod shape;
pub mod tree;
pub mod uses;
pub mod validate;
pub mod write;

use crate::iris::connection::IrisConnection;
use crate::tools::{err_result, CallToolResult, McpError};
use serde_json::{json, Value};

/// Handle iris_storage_map tool calls.
pub async fn handle(
    _iris: &IrisConnection,
    _client: &reqwest::Client,
    p: &Value,
    _tools: &crate::tools::IrisTools,
) -> Result<CallToolResult, McpError> {
    let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("");

    match action {
        "read" | "diagram" | "uses" | "compare" | "preview" | "propose" | "check" | "edit"
        | "replace" => err_result(json!({
            "success": false,
            "error_code": "NOT_IMPLEMENTED",
            "error": format!("Action '{}' not yet implemented", action)
        })),
        _ => err_result(json!({
            "success": false,
            "error_code": "INVALID_ACTION",
            "error": format!("Unknown action: '{}'", action)
        })),
    }
}
