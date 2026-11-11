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

/// Action enum for iris_storage_map tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Diagram,
    Uses,
    Compare,
    Preview,
    Propose,
    Check,
    Edit,
    Replace,
}

/// Call shape: action plus opt-in flags.
#[derive(Debug, Clone, PartialEq)]
pub struct CallShape {
    pub action: Action,
    pub seed_counter: bool,
    pub build_indexes: bool,
    pub include_values: bool,
    pub code_changed: Option<bool>,
}

/// Parses the action and opt-ins from params into a CallShape.
///
/// Per FR-030a: unreadable action (missing, not a string, unknown, any whitespace/case variant)
/// becomes Edit with both opt-ins true.
pub fn call_shape(params: &Value) -> CallShape {
    let action_str = params
        .get("action")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .unwrap_or("");

    let (action, is_unreadable) = match action_str.to_lowercase().as_str() {
        "read" => (Action::Read, false),
        "diagram" => (Action::Diagram, false),
        "uses" => (Action::Uses, false),
        "compare" => (Action::Compare, false),
        "preview" => (Action::Preview, false),
        "propose" => (Action::Propose, false),
        "check" => (Action::Check, false),
        "edit" => (Action::Edit, false),
        "replace" => (Action::Replace, false),
        _ => (Action::Edit, true), // Unreadable becomes Edit with opt-ins
    };

    // Opt-ins: only true if the value is boolean true
    let mut seed_counter = params
        .get("seed_counter")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut build_indexes = params
        .get("build_indexes")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let include_values = params
        .get("include_values")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Force both opt-ins to true for unreadable action
    if is_unreadable {
        seed_counter = true;
        build_indexes = true;
    }

    CallShape {
        action,
        seed_counter,
        build_indexes,
        include_values,
        code_changed: None, // Will be set later when we have before/after trees
    }
}

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
