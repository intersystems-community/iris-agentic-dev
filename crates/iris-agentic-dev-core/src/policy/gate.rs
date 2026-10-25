//! Unified dispatch gate (051-phi-policy-env-gates).
//!
//! `dispatch_gate_for()` is the single pre-dispatch check that runs before every tool call.
//! Gate evaluation order (fixed):
//!   [0] Code-edit hard-block (non-configurable; scans the `code` of every tool in
//!       `CODE_EXEC_TOOLS`, the class/method of iris_execute_method, and iris_query write SQL)
//!   [1] Environment template (mcpTemplate)
//!   [2] Bulk-PHI hard-block (dataPolicy + bulk-PHI tool list)
//!   [3] System global blocklist (hardcoded + custom)
//!   [4] Per-global PHI name pattern gate (acknowledgePhi bypass)
//!
//! New error codes (per constitution Error Code Registry):
//!   CODE_EDIT_BLOCKED   — attempt to edit class/routine code via arbitrary execution
//!   ENV_GATE_BLOCKED    — mcpTemplate blocks the tool category
//!   DATA_POLICY_BLOCKED — dataPolicy=block on PHI-capable tool
//!   SYSTEM_BLOCKLIST    — global name matches hardcoded or custom blocklist
//!   PHI_GATE_BLOCKED    — global name matches PHI pattern, no acknowledgePhi
//!   MISSING_PARAMS      — required parameter absent (e.g. journal_search with no filters)
//!   NAMESPACE_NOT_FOUND — requested namespace does not exist on this IRIS instance
//!   DATABASE_NOT_FOUND  — requested database name not found

use crate::iris::workspace_config::{ConnectionPolicy, ToolCategory};

/// The return type for all gate checks: `Ok(())` = permitted, `Err(json)` = blocked.
pub type GateResult = Result<(), serde_json::Value>;

/// The tools whose `code` parameter carries caller-supplied ObjectScript, and whose contents gate
/// `[0]` therefore has to scan.
///
/// A declared list rather than a chain of `tool_name ==` comparisons, because the chain is how
/// issue #137 happened: `iris_ws_exec` runs arbitrary ObjectScript in a terminal session, shipped
/// without a branch here, and the gate that is documented as non-configurable did not fire on it at
/// all. Adding a tool to this list is one line; forgetting to is a test failure
/// (`tests/unit/test_ws_exec_gate.rs` checks it against every tool in the router that has a `code`
/// parameter).
///
/// `iris_execute_method` and `iris_query` are not here — they carry the code to run as
/// `class`/`method` and `query`, and have their own branches below.
pub const CODE_EXEC_TOOLS: &[&str] = &["iris_execute", "iris_ws_exec"];

/// Run all security gates for a tool call before any IRIS operation executes, on the server the
/// pipeline resolved: its policy, never absent, and the categories the call has to be permitted in
/// (spec 141, R1 D8). `Ok(())` permits the call; `Err(json)` is a structured refusal carrying one
/// of the registered error codes.
pub fn dispatch_gate_for(
    tool_name: &str,
    server_name: &str,
    policy: &ConnectionPolicy,
    categories: &[ToolCategory],
    params: &serde_json::Value,
) -> GateResult {
    // [0] Code-edit hard-block — non-configurable, fires regardless of policy presence.
    // Blocks editing class/routine code through arbitrary-execution tools, which otherwise
    // sidestep the system blocklist (that gate only fires on tools carrying a global_name).
    if CODE_EXEC_TOOLS.contains(&tool_name) {
        if let Some(code) = params.get("code").and_then(|v| v.as_str()) {
            if let Some(err) =
                crate::policy::code_edit_gate::check_objectscript_code_edit(code, server_name)
            {
                return Err(err);
            }
        }
    } else if tool_name == "iris_execute_method" {
        // Naming a class and method directly reaches everything the ObjectScript gate blocks —
        // `iris_execute_method(class="%SYSTEM.OBJ", method="Delete")` deletes a class — and this
        // tool never carried a `code` param, so gate [0] used to skip it entirely. Reassemble the
        // call in the form the gate reads.
        let class = params.get("class").and_then(|v| v.as_str()).unwrap_or("");
        let method = params.get("method").and_then(|v| v.as_str()).unwrap_or("");
        if !class.is_empty() {
            let reassembled = format!("##class({class}).{method}()");
            if let Some(err) = crate::policy::code_edit_gate::check_objectscript_code_edit(
                &reassembled,
                server_name,
            ) {
                return Err(err);
            }
        }
    } else if tool_name == "iris_query" {
        // `mode="write"` is not the only way to send DML. `mode="read"` with `force=true` skips
        // the read-only SQL validation, so it reached the dictionary with this gate never
        // consulted. Gate both. A plain `mode="read"` SELECT is left alone so `%Dictionary.
        // Compiled*` introspection keeps working.
        let is_write = params.get("mode").and_then(|v| v.as_str()) == Some("write");
        let forced = params
            .get("force")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_write || forced {
            if let Some(query) = params.get("query").and_then(|v| v.as_str()) {
                if let Some(err) =
                    crate::policy::code_edit_gate::check_sql_code_edit(query, server_name)
                {
                    return Err(err);
                }
            }
        }
    }

    // [1] Environment template gate
    let template = policy
        .mcp_template
        .as_ref()
        .unwrap_or(&crate::iris::workspace_config::McpTemplate::Dev);
    if let Some(err) =
        crate::policy::env_gate::check_env_gate_for(tool_name, template, server_name, categories)
    {
        return Err(err);
    }

    // [2] Bulk-PHI hard-block
    //
    // `iris_admin` multiplexes: `action="journal_search"` runs the same journal reader as the
    // standalone `journal_search` tool. The gate saw the dispatcher's name, which is not in
    // `BULK_PHI_TOOLS`, so the block never fired on that route — and the handler then took its
    // effective policy from the caller's own `dataPolicy` parameter. Check the `action` value under
    // the same rule, so routing through a dispatcher is not a way around the gate.
    let data_policy = policy
        .data_policy
        .as_ref()
        .unwrap_or(&crate::iris::workspace_config::DataPolicy::Block);
    let action = params.get("action").and_then(|v| v.as_str()).unwrap_or("");
    //
    // `iris_interop_query` is a bulk-data tool for the calls that name a message body (T214), so
    // the tool name is judged with its arguments.
    if let Some(err) = crate::policy::data_policy_gate::check_bulk_phi_gate_for_call(
        tool_name,
        params,
        data_policy,
        server_name,
    ) {
        return Err(err);
    }
    if !action.is_empty() {
        if let Some(err) =
            crate::policy::data_policy_gate::check_bulk_phi_gate(action, data_policy, server_name)
        {
            return Err(err);
        }
    }

    // [3] System global blocklist (only fires when a global name param is present).
    // `global_preview` and `global_kill` call it `global`; reading only `global_name` let both
    // through every check below (137).
    let global_name = ["global_name", "globalName", "global"]
        .iter()
        .find_map(|k| params.get(*k).and_then(|v| v.as_str()));

    if let Some(raw_name) = global_name {
        // One canonical name for both gates, the same one the executor builds its reference from
        // (FR-015). A name that is not a name stops here, before it is compared with anything
        // (FR-016): `oddDEF("x")` is not `oddDEF`, and an exact-match rule would never see it.
        let gname_owned = crate::tools::global::canonical_global_name(raw_name)?;
        let gname = gname_owned.as_str();
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
