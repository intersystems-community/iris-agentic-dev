//! Output-schema-only response shapes (076-interface-modernization, User Story 1).
//!
//! These types exist purely to be handed to `#[tool(output_schema = schema_for_output::<T>()
//! ...)]` — they document a tool's actual return shape for `list_tools` consumers, they are
//! never constructed at runtime. Tool bodies are untouched: they still build their response via
//! `ok_json(serde_json::json!({...}))`/`err_json(...)`, exactly as before. Declaring an output
//! schema is additive, read-only documentation of what those calls already produce; if a tool's
//! body and its declared schema ever drift, only `list_tools`'s advertised shape is stale, not
//! runtime behavior — there is no shared code path that could make this cause an actual response
//! to change.
//!
//! Every tool here follows this project's dominant error convention: on failure, `err_json(code,
//! msg)` returns exactly `{"success": false, "error_code": code, "error": msg}`. That shape is
//! shared as [`ToolError`] rather than redefined per tool. A tool whose success and error paths
//! are the *only* two shapes it ever returns gets a `#[serde(untagged)]` enum of `Ok(...) |
//! Err(ToolError)`, which schemars renders as a `oneOf` — matching reality more closely than
//! picking one branch and ignoring the other. A tool with no embedded-JSON error path at all
//! (its only failure mode is an `McpError` via `?`, which becomes a protocol-level `isError`
//! response outside this schema's scope entirely — see `iris_ws_exec`/`iris_ws_close`) declares
//! only its success shape.
//!
//! Not every tool is covered here — see spec 076 User Story 1 for the full list and the explicit
//! reasoning for which tools were deferred (genuinely heterogeneous/dynamic shapes that a single
//! schema would either misdescribe or have to render so permissively it stops being useful
//! documentation).

use schemars::JsonSchema;
use serde::Serialize;
use std::sync::Arc;

/// Wrap a `#[serde(untagged)]` `Ok(...) | Err(ToolError)` enum's schema for use as an MCP
/// `outputSchema`.
///
/// `rmcp::Tool::with_output_schema::<T>()` (and the `#[tool(output_schema = schema_for_output::<T>
/// ...)]` shorthand it's built on) *panics* unless the generated root schema has a literal
/// `"type": "object"` key — that's MCP's own requirement, not this project's. schemars renders a
/// `#[serde(untagged)]` enum as a bare `{"oneOf": [...]}` with no root `"type"` at all, so handing
/// one of those straight to `with_output_schema` fails every time, for every tool with a
/// success-or-error union shape (which is most of them — this project's error convention embeds
/// failures in an otherwise-"successful" MCP response rather than using `isError`).
///
/// `rmcp::handler::server::tool::schema_for_type::<T>()` is the same schema generator
/// `schema_for_output` itself calls, minus the root-type validation — so this takes exactly that
/// raw `{"oneOf": [...]}` and adds the one key MCP requires, then hands it to
/// `with_raw_output_schema` (which, deliberately, performs no validation of its own — it's the
/// escape hatch for exactly this case). The result is a real, spec-compliant schema: any given
/// response validates against the `Ok` shape or the `Err` shape, which is the actual truth for
/// every tool this is used on.
pub fn oneof_output_schema<T: JsonSchema + 'static>(
) -> Arc<serde_json::Map<String, serde_json::Value>> {
    let inner = rmcp::handler::server::tool::schema_for_type::<T>();
    let mut obj = (*inner).clone();
    obj.insert(
        "type".to_string(),
        serde_json::Value::String("object".to_string()),
    );
    Arc::new(obj)
}

/// This project's dominant embedded-JSON error shape, produced by the shared `err_json` helper.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToolError {
    /// Always `false` on this path — errors are reported inside an otherwise-successful MCP
    /// response, not via the protocol-level `isError` flag. See `dispatch.rs`'s `is_failure`.
    pub success: bool,
    pub error_code: String,
    pub error: String,
}

// ── iris_servers ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisServersResponse {
    pub servers: Vec<ServerEntry>,
    pub count: usize,
}

/// One registered server. Only `name` and `source` are always present — a pool entry whose
/// connection metadata couldn't be constructed omits the rest rather than nulling them.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ServerEntry {
    pub name: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Always `null` today — `iris_servers` never probes connectivity itself; call
    /// `iris_test_server` for that. Modeled as `Option<bool>`, not a fixed null, since a future
    /// change could populate it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reachable: Option<bool>,
}

// ── skill_list / skill_community_list / skill_forget ───────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillListResponse {
    /// Each entry's exact shape comes from `SkillMeta::to_json()` (bundled or synthesized skill
    /// metadata) — deliberately left as free-form JSON here rather than pinned to a struct this
    /// file would have to keep in lockstep with `bundled.rs`.
    pub skills: Vec<serde_json::Value>,
    pub count: usize,
    pub sources: serde_json::Value,
    pub note: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillCommunityListResponse {
    pub skills: Vec<CommunitySkillEntry>,
    pub kb_items: Vec<CommunityKbEntry>,
    pub skill_count: usize,
    pub kb_count: usize,
    pub hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CommunitySkillEntry {
    pub name: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CommunityKbEntry {
    pub title: String,
    pub source: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillForgetOk {
    pub success: bool,
    pub name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SkillForgetResponse {
    Ok(SkillForgetOk),
    Err(ToolError),
}

// ── agent_stats / agent_history / kb_recall ─────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct AgentStatsResponse {
    pub status: String,
    pub skill_count: usize,
    pub session_calls: usize,
    pub learning_enabled: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AgentHistoryResponse {
    pub calls: Vec<AgentHistoryCall>,
    pub limit: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AgentHistoryCall {
    pub tool: String,
    pub success: bool,
    pub ago_secs: u64,
    pub duration_ms: u64,
    pub session_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbRecallResponse {
    pub query: String,
    pub results: Vec<KbRecallHit>,
    pub count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbRecallHit {
    pub title: String,
    pub snippet: String,
    pub source: String,
    pub score: f64,
}

// ── iris_symbols / iris_symbols_local ───────────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisSymbolsOk {
    pub source: String,
    /// Raw Atelier SQL query result rows — heterogeneous by design (whatever columns the
    /// underlying `%Dictionary` query projected), so left as free-form JSON.
    pub symbols: serde_json::Value,
    pub count: usize,
    pub query_hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisSymbolsResponse {
    Ok(IrisSymbolsOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisSymbolsLocalOk {
    pub source: String,
    pub symbols: Vec<serde_json::Value>,
    pub count: usize,
    pub query_hint: String,
    pub parse_warnings: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisSymbolsLocalResponse {
    Ok(IrisSymbolsLocalOk),
    Err(ToolError),
}

// ── docs_introspect ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocsIntrospectResponse {
    pub success: bool,
    pub class_name: String,
    /// `%Dictionary.CompiledMethod` rows, with `FormalSpec` re-parsed into a structured array —
    /// left as free-form JSON rather than duplicating the ArgSpec struct here.
    pub methods: Vec<serde_json::Value>,
    pub properties: serde_json::Value,
    /// Present only for BPL/DTL classes. Shape differs by kind (`bpl` vs `dtl` — see the tool
    /// description) enough that a single typed field would need its own tagged union; left
    /// dynamic rather than half-modeled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xdata_flow: Option<serde_json::Value>,
}

// ── debug_map_int_to_cls / debug_source_map ─────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct DebugMapIntToClsOk {
    pub success: bool,
    pub mapping_available: bool,
    pub cls_name: Option<String>,
    pub cls_line: Option<i64>,
    pub routine: String,
    pub offset: i64,
    pub raw_error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DebugMapIntToClsResponse {
    Ok(DebugMapIntToClsOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DebugSourceMapOk {
    pub success: bool,
    pub cls_name: String,
    /// `{method_name: int_line}` map — keys are dynamic (one per compiled method), so this stays
    /// a free-form object rather than a fixed struct.
    pub source_map: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DebugSourceMapResponse {
    Ok(DebugSourceMapOk),
    Err(ToolError),
}

// ── iris_ws_open / iris_ws_exec / iris_ws_close ─────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisWsOpenOk {
    pub session: String,
    pub server: String,
    pub namespace: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisWsOpenResponse {
    Ok(IrisWsOpenOk),
    Err(ToolError),
}

/// `iris_ws_exec`'s only embedded-JSON shape — its error path (stale/unknown session) returns
/// `Err(McpError)` via `?`, which becomes a protocol-level `isError` response outside this
/// schema's scope, not an `err_json` value. Same for `iris_ws_close`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisWsExecResponse {
    pub output: String,
    pub session: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisWsCloseResponse {
    pub closed: bool,
}
