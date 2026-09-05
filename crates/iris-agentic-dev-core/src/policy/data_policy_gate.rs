//! PHI data policy gate (051-phi-policy-env-gates, US2).
//!
//! Two checks:
//! - `check_bulk_phi_gate`: gate [2] — hard-blocks bulk-PHI tools when dataPolicy != Allow
//! - `check_phi_name_gate`: gate [4] — blocks per-global PHI name matches without acknowledgePhi

use crate::iris::workspace_config::DataPolicy;
use crate::policy::patterns::{first_match, matches_any, PHI_NAME_PATTERNS};

/// Tools that access PHI in bulk and cannot be made PHI-aware at field level.
/// Hard-blocked on any policy other than `Allow`. No `acknowledgePhi` bypass.
///
/// These are matched against the name the router dispatches on, so every entry has to be a
/// registered tool name. `view_message_body` sat here from 051 until 1.3.2 and never matched
/// anything — the real tool is `iris_message_body`, so the gate was dead for the one tool it
/// most needed to cover. Public so `test_data_policy_gate.rs` can assert every entry appears in
/// [`crate::tools::write_gate::CLASSIFICATION`]; a name typo in a security gate fails open and
/// looks exactly like a permitted call.
pub const BULK_PHI_TOOLS: &[&str] = &["journal_search", "iris_message_body"];

/// Gate [2]: hard-block bulk-PHI tools when dataPolicy is not Allow.
///
/// Returns `Some(error_json)` when blocked, `None` when permitted.
pub fn check_bulk_phi_gate(
    tool_name: &str,
    policy: &DataPolicy,
    server_name: &str,
) -> Option<serde_json::Value> {
    if *policy == DataPolicy::Allow {
        return None;
    }
    if !BULK_PHI_TOOLS.contains(&tool_name) {
        return None;
    }

    let policy_str = match policy {
        DataPolicy::Block => "block",
        DataPolicy::Allow => "allow",
        DataPolicy::Redact => "redact",
    };

    Some(serde_json::json!({
        "success": false,
        "error_code": "DATA_POLICY_BLOCKED",
        "data_policy_blocked": true,
        "server_name": server_name,
        "data_policy": policy_str,
        "tool_name": tool_name,
        "message": format!(
            "Tool '{}' is a bulk-PHI tool and is blocked when dataPolicy is '{}' for server '{}'. \
             Note: acknowledgePhi does not apply to bulk-PHI tools — they access PHI in bulk.",
            tool_name, policy_str, server_name
        ),
        "remediation": format!(
            "Set dataPolicy = \"allow\" in [policy.{}] of .iris-agentic-dev.toml",
            server_name
        ),
    }))
}

/// Gate [4]: block access to a global whose name matches a PHI pattern, unless `acknowledge_phi` is true.
///
/// Returns `Some(error_json)` when blocked, `None` when permitted.
pub fn check_phi_name_gate(
    global_name: &str,
    acknowledge_phi: bool,
    server_name: &str,
) -> Option<serde_json::Value> {
    if acknowledge_phi {
        return None;
    }
    if !matches_any(global_name, PHI_NAME_PATTERNS) {
        return None;
    }

    let matched = first_match(global_name, PHI_NAME_PATTERNS).unwrap_or("(unknown)");

    Some(serde_json::json!({
        "success": false,
        "error_code": "PHI_GATE_BLOCKED",
        "phi_gate_blocked": true,
        "server_name": server_name,
        "global_name": global_name,
        "matched_pattern": matched,
        "message": format!(
            "Global '{}' matches PHI name pattern '{}' for server '{}'. \
             Pass acknowledgePhi: true to proceed.",
            global_name, matched, server_name
        ),
        "remediation": "Pass acknowledgePhi: true in your iris_global call to acknowledge PHI risk and proceed.",
    }))
}
