//! Environment template gate (051-phi-policy-env-gates, US1).
//!
//! Checks `mcpTemplate` against the tool's category and returns `ENV_GATE_BLOCKED`
//! when the template forbids it. Gate [1] in `dispatch_gate_for`.

use crate::iris::workspace_config::{McpTemplate, ToolCategory};

/// Tool categories blocked per template.
const LIVE_BLOCKED: &[ToolCategory] = &[
    ToolCategory::Execute,
    ToolCategory::Compile,
    ToolCategory::SourceControl,
];

const TEST_BLOCKED: &[ToolCategory] = &[ToolCategory::Execute, ToolCategory::Compile];

/// Check whether `tool_name` is blocked by the server's `mcpTemplate`, for the categories the
/// pipeline worked out with [`effective_category`], the one place an action raises a tool's
/// category (an `iris_global` set is `Execute`, an `iris_doc` put is `Compile`).
///
/// Returns `Some(error_json)` when blocked, `None` when permitted.
pub fn check_env_gate_for(
    tool_name: &str,
    template: &McpTemplate,
    server_name: &str,
    categories: &[ToolCategory],
) -> Option<serde_json::Value> {
    let blocked = match template {
        McpTemplate::Dev => return None,
        McpTemplate::Live => LIVE_BLOCKED,
        McpTemplate::Test => TEST_BLOCKED,
    };
    let category = categories.iter().find(|c| blocked.contains(c))?;

    let template_str = match template {
        McpTemplate::Dev => "dev",
        McpTemplate::Test => "test",
        McpTemplate::Live => "live",
    };
    Some(serde_json::json!({
        "success": false,
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
            "The environment template of server '{server_name}' is the operator's to change; \
             the refusal's `refusal.lift.setting` names it."
        ),
    }))
}
