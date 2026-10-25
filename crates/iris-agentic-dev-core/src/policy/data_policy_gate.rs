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
pub const BULK_PHI_TOOLS: &[&str] = &["journal_search", "iris_message_body", "stream_inspect"];

/// Bulk-data tools that can redact what they return: under `redact` they are admitted, and their
/// handler returns HL7 v2 redacted and refuses content iad cannot redact (spec 141 R5 C22). Every
/// other bulk tool stays refused under `redact`: `journal_search` returns a journal record's value,
/// which is not a message, so there is nothing to redact it with.
pub const REDACTING_TOOLS: &[&str] = &["iris_message_body", "stream_inspect"];

/// Bulk-data tools whose handler enforces the policy itself, because a refused call still answers
/// with metadata (`stream_inspect` returns the stream's class, size and line count, FR-030c). The
/// gate has no IRIS to ask, so refusing here would leave the caller with nothing.
pub const HANDLER_ENFORCED_TOOLS: &[&str] = &["stream_inspect"];

/// A tool that is a bulk-data tool only for the calls that name a message body: `iris_interop_query`
/// answers headers, logs and queues from its own tables, and joins `Ens.MessageBody` rows when the
/// call carries one of these arguments (spec 141 T214). The arguments are the handler's own, read
/// by name from the call.
pub const BODY_READ_TOOLS: &[(&str, &[&str])] = &[(
    "iris_interop_query",
    &["body_class", "body_where", "body_select"],
)];

/// True when `tool_name` with `params` reads message bodies: a bulk-data tool, or a tool in
/// [`BODY_READ_TOOLS`] whose call carries a body argument. An empty `body_select` list selects
/// nothing and does not count.
pub fn reads_message_bodies(tool_name: &str, params: &serde_json::Value) -> bool {
    BODY_READ_TOOLS
        .iter()
        .filter(|(tool, _)| *tool == tool_name)
        .flat_map(|(_, args)| args.iter())
        .any(|arg| {
            params
                .get(arg)
                .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
        })
}

/// [`check_bulk_phi_gate`] for a call: also refuses a call that reads message bodies through a tool
/// that is not a bulk-data tool otherwise (see [`BODY_READ_TOOLS`]). It is never admitted under
/// `redact`, since the rows it returns are not HL7 text it could redact.
pub fn check_bulk_phi_gate_for_call(
    tool_name: &str,
    params: &serde_json::Value,
    policy: &DataPolicy,
    server_name: &str,
) -> Option<serde_json::Value> {
    if *policy != DataPolicy::Allow && reads_message_bodies(tool_name, params) {
        let mut refusal = bulk_refusal(tool_name, policy, server_name);
        refusal["message"] = serde_json::json!(format!(
            "Tool '{tool_name}' reads message bodies when a call names a body_class, body_where \
             or body_select, and that is blocked unless dataPolicy is 'allow' for server \
             '{server_name}'. Without those arguments it still lists headers, logs and queues."
        ));
        return Some(refusal);
    }
    check_bulk_phi_gate(tool_name, policy, server_name)
}

/// Gate [2]: hard-block bulk-PHI tools when dataPolicy is not Allow, except the tools that redact
/// under `redact` and the ones whose handler enforces the policy.
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
    if !BULK_PHI_TOOLS.contains(&tool_name) || HANDLER_ENFORCED_TOOLS.contains(&tool_name) {
        return None;
    }
    if *policy == DataPolicy::Redact && REDACTING_TOOLS.contains(&tool_name) {
        return None;
    }
    Some(bulk_refusal(tool_name, policy, server_name))
}

fn bulk_refusal(tool_name: &str, policy: &DataPolicy, server_name: &str) -> serde_json::Value {
    let policy_str = match policy {
        DataPolicy::Block => "block",
        DataPolicy::Allow => "allow",
        DataPolicy::Redact => "redact",
    };

    serde_json::json!({
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
            "The data policy of server '{server_name}' is the operator's to change; \
             the refusal's `refusal.lift.setting` names it."
        ),
    })
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
