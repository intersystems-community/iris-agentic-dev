//! Environment template gate (051-phi-policy-env-gates, US1).
//!
//! Checks `mcpTemplate` against the tool's category and returns `ENV_GATE_BLOCKED`
//! when the template forbids it. Gate [1] in `dispatch_gate`.

use crate::iris::workspace_config::{McpTemplate, ToolCategory};

/// Tool categories blocked per template.
const LIVE_BLOCKED: &[ToolCategory] = &[
    ToolCategory::Execute,
    ToolCategory::Compile,
    ToolCategory::SourceControl,
];

const TEST_BLOCKED: &[ToolCategory] = &[ToolCategory::Execute, ToolCategory::Compile];

/// Check whether `tool_name` is blocked by the active `mcpTemplate`.
///
/// `params` is the full tool call parameters; used for action-aware category overrides
/// (e.g. `iris_global` with `action=set` is classified as `Execute` even though
/// the default category is `Query`).
///
/// Returns `Some(error_json)` when blocked, `None` when permitted.
pub fn check_env_gate(
    tool_name: &str,
    template: &McpTemplate,
    server_name: &str,
    params: &serde_json::Value,
) -> Option<serde_json::Value> {
    let blocked = match template {
        McpTemplate::Dev => return None,
        McpTemplate::Live => LIVE_BLOCKED,
        McpTemplate::Test => TEST_BLOCKED,
    };

    let mut category = crate::iris::server_manager::tool_to_category_pub(tool_name)?;

    // Action-aware override for iris_global: set/kill are write operations → Execute.
    if tool_name == "iris_global" {
        if let Some(action) = params.get("action").and_then(|v| v.as_str()) {
            if action == "set" || action == "kill" {
                category = ToolCategory::Execute;
            }
        }
    }

    // 057-sql-power: mode="write" is a DML operation → Execute. explain/count stay Query.
    if tool_name == "iris_query" {
        if let Some(mode) = params.get("mode").and_then(|v| v.as_str()) {
            if mode == "write" {
                category = ToolCategory::Execute;
            }
        }
    }

    if blocked.contains(&category) {
        let template_str = match template {
            McpTemplate::Dev => "dev",
            McpTemplate::Test => "test",
            McpTemplate::Live => "live",
        };
        return Some(serde_json::json!({
            "error_code": "ENV_GATE_BLOCKED",
            "env_gate_blocked": true,
            "server_name": server_name,
            "template": template_str,
            "blocked_category": category.as_str(),
            "message": format!(
                "Tool '{}' is blocked by environment template '{}' for server '{}'. \
                 Category '{}' is not permitted in {} mode.",
                tool_name, template_str, server_name, category.as_str(), template_str
            ),
            "remediation": format!(
                "Set mcpTemplate = \"dev\" or mcpTemplate = \"test\" in [policy.{}] of .iris-agentic-dev.toml",
                server_name
            ),
        }));
    }

    None
}
