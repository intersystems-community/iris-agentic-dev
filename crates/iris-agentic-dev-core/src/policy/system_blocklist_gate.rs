//! System global blocklist gate (051-phi-policy-env-gates, US3).
//!
//! Gate [3] in `dispatch_gate`. Checks global name against hardcoded system blocklist
//! and per-connection custom blocklist. Not bypassable by any config or `acknowledgePhi`.

use crate::policy::patterns::{
    first_match, first_match_owned, matches_any_owned, SYSTEM_BLOCKLIST,
};

/// Gate [3]: block access to a global matching the system or custom blocklist.
///
/// Evaluation order:
/// 1. If `is_kill_op` and `global_name` matches `kill_allowlist` → permitted (kill exemption).
/// 2. Check `SYSTEM_BLOCKLIST` — match → `SYSTEM_BLOCKLIST` error.
/// 3. Check `custom_blocklist` — match → `SYSTEM_BLOCKLIST` error.
/// 4. No match → permitted.
///
/// Returns `Some(error_json)` when blocked, `None` when permitted.
pub fn check_system_blocklist(
    global_name: &str,
    custom_blocklist: &[String],
    kill_allowlist: &[String],
    is_kill_op: bool,
    server_name: &str,
) -> Option<serde_json::Value> {
    // Kill-operation exemption: if this is a kill and the global is in the kill allowlist, permit
    if is_kill_op && matches_any_owned(global_name, kill_allowlist) {
        return None;
    }

    // Check hardcoded system blocklist
    if let Some(matched) = first_match(global_name, SYSTEM_BLOCKLIST) {
        return Some(blocklist_error(global_name, matched, server_name));
    }

    // Check per-connection custom blocklist
    if let Some(matched) = first_match_owned(global_name, custom_blocklist) {
        return Some(blocklist_error(global_name, &matched, server_name));
    }

    None
}

fn blocklist_error(
    global_name: &str,
    matched_pattern: &str,
    server_name: &str,
) -> serde_json::Value {
    serde_json::json!({
        "success": false,
        "error_code": "SYSTEM_BLOCKLIST",
        "system_blocklist": true,
        "server_name": server_name,
        "global_name": global_name,
        "matched_pattern": matched_pattern,
        "message": format!(
            "Access to global '{}' is blocked by the system blocklist (matched '{}') for server '{}'. \
             System globals cannot be accessed regardless of data policy.",
            global_name, matched_pattern, server_name
        ),
        "remediation": "System globals are permanently blocked. No configuration override exists.",
    })
}
