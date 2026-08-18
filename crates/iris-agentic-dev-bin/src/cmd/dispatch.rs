//! Shared dispatch path for CLI subcommands that wrap a real MCP tool.
//!
//! `compile`/`exec`/`query`/`doc` used to re-implement their Atelier HTTP calls
//! directly instead of calling the tool methods — the same "hand-maintained parallel
//! implementation drifts from the real one" pattern found (and fixed) twice elsewhere
//! this session, in `registered_tool_names()` and the `TOOL_NAMES`/`call_for_test`
//! dispatch gap. That drift had already cost these four commands real capability:
//! none of them could route to a named `--server`, and `doc` had no path through an
//! SCM-checkout elicitation at all. See specs/076-interface-modernization/spec.md,
//! User Story 2.
//!
//! `dispatch_tool` is the fix: route through `IrisTools::call_for_test`, the exact
//! dispatcher `tool.rs`'s generic `tool <name> --args '{...}'` fallback already uses,
//! so these commands inherit every tool-level feature (multi-server routing, policy
//! gates, session carriers, elicitation) by construction instead of by a second
//! hand-maintained copy that can drift again.

use anyhow::Context;
use iris_agentic_dev_core::{
    iris::connection::IrisConnection,
    tools::{IrisTools, Toolset},
};

/// Build an `IrisTools` instance around an already-resolved connection.
///
/// Takes an already-resolved `IrisConnection` rather than resolving one itself —
/// callers that need a pre-dispatch safety check against the connection (e.g.
/// `exec`/`doc put`'s `is_write_allowed()` guard, which the `iris_execute`/`iris_doc`
/// tool methods do NOT enforce for the default, non-fleet connection role — their
/// `check_role_gate` only fires for `ConnectionRole::Subject`, so delegating naively
/// would have silently dropped the one guard the CLI actually had) need the connection
/// in hand before this runs, not resolved a second time inside it.
///
/// `Toolset::Merged` matches `tool.rs`'s own choice — `call_for_test`'s dispatch match
/// arms are unconditional on tool name and don't consult the toolset at all (toolset
/// pruning only affects what `list_tools` advertises), so the choice has no effect on
/// which tools are reachable here; it's just the convention this project already uses
/// for CLI-driven dispatch.
pub fn build_tools(iris: IrisConnection) -> anyhow::Result<IrisTools> {
    IrisTools::new_with_toolset(Some(iris), Toolset::Merged)
}

/// Dispatch `tool_name` with `args` through `call_for_test`, and parse the single JSON
/// text response back into a `serde_json::Value`.
///
/// Takes a `&IrisTools` rather than owning one so a caller that needs two related
/// calls to land in the same instance — most importantly `doc put`'s elicitation
/// resume, where the second call must find the `PendingElicitation` the first call
/// stored in that specific instance's in-process `ElicitationStore` — can build one
/// with `build_tools` and reuse it, instead of each call getting a fresh instance (and
/// a fresh, empty store) the way two separate CLI processes always would.
pub async fn call(
    tools: &IrisTools,
    tool_name: &str,
    args: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let result = tools
        .call_for_test(tool_name, args)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    for content in &result.content {
        if let Some(text) = content.raw.as_text() {
            return serde_json::from_str(&text.text)
                .with_context(|| format!("parsing {tool_name} response"));
        }
    }
    anyhow::bail!("{tool_name} returned no text content")
}

/// One-shot convenience for callers that only need a single call: build an `IrisTools`
/// around `iris` and dispatch `tool_name` once. See `call`'s doc comment for when a
/// caller needs `build_tools` + `call` separately instead.
pub async fn dispatch_tool(
    iris: IrisConnection,
    tool_name: &str,
    args: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let tools = build_tools(iris)?;
    call(&tools, tool_name, args).await
}

/// Convenience: does the parsed response report `"success": false`?
/// (`CallToolResult`'s own `is_error` flag is never set by `ok_json`/`err_json` — every
/// tool response is a "successful" MCP call carrying a JSON body that may itself
/// describe a failure. Callers must check this field, not `is_error`.)
pub fn is_failure(v: &serde_json::Value) -> bool {
    v.get("success") == Some(&serde_json::Value::Bool(false))
}
