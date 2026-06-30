//! Unified dispatch gate (051-phi-policy-env-gates).
//!
//! `dispatch_gate()` is the single pre-dispatch check that runs before every tool call.
//! Gate evaluation order (fixed):
//!   [1] Environment template (mcpTemplate)
//!   [2] Bulk-PHI hard-block (dataPolicy + bulk-PHI tool list)
//!   [3] System global blocklist (hardcoded + custom)
//!   [4] Per-global PHI name pattern gate (acknowledgePhi bypass)
//!
//! New error codes (per constitution Error Code Registry):
//!   ENV_GATE_BLOCKED    — mcpTemplate blocks the tool category
//!   DATA_POLICY_BLOCKED — dataPolicy=block on PHI-capable tool
//!   SYSTEM_BLOCKLIST    — global name matches hardcoded or custom blocklist
//!   PHI_GATE_BLOCKED    — global name matches PHI pattern, no acknowledgePhi
//!   MISSING_PARAMS      — required parameter absent (e.g. journal_search with no filters)
//!   NAMESPACE_NOT_FOUND — requested namespace does not exist on this IRIS instance
//!   DATABASE_NOT_FOUND  — requested database name not found

use crate::iris::workspace_config::ConnectionPolicy;

/// The return type for all gate checks: `Ok(())` = permitted, `Err(json)` = blocked.
pub type GateResult = Result<(), serde_json::Value>;

/// Run all security gates for a tool call before any IRIS operation executes.
///
/// # Parameters
/// - `tool_name`: the tool being called (e.g. `"iris_execute"`)
/// - `server_name`: the connection name (for error messages)
/// - `policy`: the active per-connection policy, or `None` (no gates fire when absent)
/// - `params`: the full tool call parameters as JSON
///
/// # Returns
/// `Ok(())` if all gates permit the call, or `Err(json)` with a structured error
/// matching one of the registered error codes.
pub fn dispatch_gate(
    tool_name: &str,
    server_name: &str,
    policy: Option<&ConnectionPolicy>,
    params: &serde_json::Value,
) -> GateResult {
    let Some(policy) = policy else {
        return Ok(());
    };

    // [1] Environment template gate
    let template = policy
        .mcp_template
        .as_ref()
        .unwrap_or(&crate::iris::workspace_config::McpTemplate::Dev);
    if let Some(err) =
        crate::policy::env_gate::check_env_gate(tool_name, template, server_name, params)
    {
        return Err(err);
    }

    // [2] Bulk-PHI hard-block
    let data_policy = policy
        .data_policy
        .as_ref()
        .unwrap_or(&crate::iris::workspace_config::DataPolicy::Block);
    if let Some(err) =
        crate::policy::data_policy_gate::check_bulk_phi_gate(tool_name, data_policy, server_name)
    {
        return Err(err);
    }

    // [3] System global blocklist (only fires when global_name param is present)
    let global_name = params
        .get("global_name")
        .and_then(|v| v.as_str())
        .or_else(|| params.get("globalName").and_then(|v| v.as_str()));

    if let Some(gname) = global_name {
        let is_kill_op = params
            .get("action")
            .and_then(|v| v.as_str())
            .map(|a| a == "kill")
            .unwrap_or(false);

        if let Some(err) = crate::policy::system_blocklist_gate::check_system_blocklist(
            gname,
            &policy.global_blocklist,
            &policy.data_policy_kill_allowlist,
            is_kill_op,
            server_name,
        ) {
            return Err(err);
        }

        // [4] Per-global PHI name pattern gate
        let acknowledge_phi = params
            .get("acknowledgePhi")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if let Some(err) = crate::policy::data_policy_gate::check_phi_name_gate(
            gname,
            acknowledge_phi,
            server_name,
        ) {
            return Err(err);
        }
    }

    Ok(())
}
