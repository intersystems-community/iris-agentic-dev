//! Shared dispatch path for CLI subcommands that wrap a real MCP tool.
//!
//! `compile`/`exec`/`query`/`doc` used to re-implement their Atelier HTTP calls
//! directly instead of calling the tool methods, and every one of them ran with its own gate or
//! none (spec 141 FR-003). They now build one `IrisTools` with `for_cli` and call `call_gated`, the
//! pipeline the MCP server's `call_tool` runs: the same target resolution, the same write,
//! destructive, template, `allow` and role gates, the same refusal body.
//!
//! `for_cli` sets `strict_toolset`, so the CLI dispatches exactly the names `tools/list`
//! advertises for the toolset.

use anyhow::Context;
use iris_agentic_dev_core::{
    iris::connection::IrisConnection,
    tools::{IrisTools, Toolset},
};

/// Build an `IrisTools` around an already-resolved connection, with the operator's declared gates.
///
/// Takes an already-resolved `IrisConnection` so a caller that needs the connection in hand
/// first does not resolve it twice. `Toolset::Merged` matches `tool.rs`'s own default.
pub fn build_tools(iris: IrisConnection) -> anyhow::Result<IrisTools> {
    build_tools_for(Some(iris), Toolset::Merged)
}

/// `build_tools` for a caller with a toolset of its own, or no default connection (`tool`).
pub fn build_tools_for(
    iris: Option<IrisConnection>,
    toolset: Toolset,
) -> anyhow::Result<IrisTools> {
    IrisTools::for_cli(iris, toolset, None)
}

/// Dispatch `tool_name` with `args` through `call_gated`, and parse the single JSON
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
        .call_gated(tool_name, args)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    for content in &result.content {
        if let Some(text) = content.as_text() {
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

/// A gate's refusal carries a `refusal` object (kind, server, what lifts it). Print the whole body
/// to stderr, the one the MCP server sends, and exit 1.
///
/// The subcommands that wrap a tool each shape their own success output, and each would otherwise
/// reduce a refusal to its `error` string and lose the code and the `refusal` object.
pub fn exit_if_refused(body: &serde_json::Value) {
    if body
        .get("refusal")
        .is_some_and(serde_json::Value::is_object)
    {
        eprintln!("{body}");
        crate::exit(1);
    }
}
