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
/// 1. Check `SYSTEM_BLOCKLIST` — match → `SYSTEM_BLOCKLIST` error. Nothing lifts this: the kill
///    allowlist does not reach it, and neither does any configuration.
/// 2. Check `custom_blocklist`. A match is `SYSTEM_BLOCKLIST` too, unless this is a kill and the
///    global is in `kill_allowlist`, which exempts custom patterns only.
/// 3. No match → permitted.
///
/// Returns `Some(error_json)` when blocked, `None` when permitted.
pub fn check_system_blocklist(
    global_name: &str,
    custom_blocklist: &[String],
    kill_allowlist: &[String],
    is_kill_op: bool,
    server_name: &str,
) -> Option<serde_json::Value> {
    // The hardcoded list first, so no allowlist can lift it (spec 141 FR-030b).
    if let Some(matched) = first_match(global_name, SYSTEM_BLOCKLIST) {
        return Some(blocklist_error(global_name, matched, server_name, false));
    }

    // Per-connection custom blocklist, with the kill-operation exemption.
    if let Some(matched) = first_match_owned(global_name, custom_blocklist) {
        if is_kill_op && matches_any_owned(global_name, kill_allowlist) {
            return None;
        }
        return Some(blocklist_error(global_name, &matched, server_name, true));
    }

    None
}

fn blocklist_error(
    global_name: &str,
    matched_pattern: &str,
    server_name: &str,
    custom: bool,
) -> serde_json::Value {
    let remediation = if custom {
        format!(
            "This pattern comes from the operator's blocklist for server '{server_name}'; only \
             the operator can change it."
        )
    } else {
        "System globals are permanently blocked. No configuration override exists.".to_string()
    };
    serde_json::json!({
        "success": false,
        "error_code": "SYSTEM_BLOCKLIST",
        "system_blocklist": true,
        "custom_pattern": custom,
        "server_name": server_name,
        "global_name": global_name,
        "matched_pattern": matched_pattern,
        "message": format!(
            "Access to global '{}' is blocked by the {} blocklist (matched '{}') for server '{}'.{}",
            global_name,
            if custom { "custom" } else { "system" },
            matched_pattern,
            server_name,
            if custom { "" } else { " System globals cannot be accessed regardless of data policy." }
        ),
        "remediation": remediation,
    })
}
