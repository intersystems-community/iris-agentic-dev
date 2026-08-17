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
    pub note: String,
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

// ── debug_capture_packet / debug_get_error_logs ─────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct DebugCapturePacketOk {
    pub success: bool,
    /// `%SYSTEM.Error` query rows — free-form JSON rather than a fixed struct.
    pub errors: serde_json::Value,
    /// Present only on the community-edition fallback path (`%SYSTEM.Error` unavailable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DebugCapturePacketResponse {
    Ok(DebugCapturePacketOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DebugGetErrorLogsOk {
    pub success: bool,
    /// `%SYSTEM.Error` query rows (or `[]` on the community-edition fallback) — free-form JSON.
    pub logs: serde_json::Value,
    /// Absent only on the community-edition fallback path, which returns before progressive
    /// disclosure (`log_store::apply_truncation`) ever runs. Present (true or false) otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DebugGetErrorLogsResponse {
    Ok(DebugGetErrorLogsOk),
    Err(ToolError),
}

// ── iris_add_server / iris_remove_server / iris_import_servers ─────────────

/// The `iad-native` server-config mutation tools (`iris_add_server`, `iris_remove_server`,
/// `iris_import_servers`) predate this project's `err_json`/`ToolError` convention and use their
/// own bespoke error shape instead: `{error_code, message}`, with an optional `source` field
/// (only `iris_remove_server`'s `REMOVE_NOT_ALLOWED` case sets it) — never a `success` key at
/// all on the error path, unlike `ToolError`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ServerMutationError {
    pub error_code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisAddServerOk {
    pub added: bool,
    pub name: String,
    pub note: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisAddServerResponse {
    Ok(IrisAddServerOk),
    Err(ServerMutationError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisRemoveServerOk {
    pub removed: bool,
    pub name: String,
    pub note: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisRemoveServerResponse {
    Ok(IrisRemoveServerOk),
    Err(ServerMutationError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisImportServersOk {
    pub success: bool,
    pub imported: usize,
    pub skipped: usize,
    pub no_keychain: Vec<String>,
    pub note: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisImportServersResponse {
    Ok(IrisImportServersOk),
    Err(ServerMutationError),
}

// ── iris_test_server ─────────────────────────────────────────────────────────

/// Unlike almost every other tool in this file, `iris_test_server` never calls `err_json` — every
/// outcome (network error, non-2xx status, JSON parse failure, success) goes through `ok_json`
/// with `reachable` as the discriminant, so this is one flat shape with optional fields rather
/// than an `Ok | Err` union.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisTestServerResponse {
    pub name: String,
    pub reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atelier_version: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iris_version: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

// ── global_kill / iris_namespace_list / iris_database_list ─────────────────
// ── iris_namespace_create / iris_database_stats ─────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct GlobalKillOk {
    pub success: bool,
    pub killed: bool,
    pub global: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum GlobalKillResponse {
    Ok(GlobalKillOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisNamespaceListOk {
    pub success: bool,
    pub namespaces: Vec<String>,
    pub count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisNamespaceListResponse {
    Ok(IrisNamespaceListOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DatabaseEntry {
    pub directory: String,
    pub mounted: bool,
    pub size_mb: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDatabaseListOk {
    pub success: bool,
    pub databases: Vec<DatabaseEntry>,
    pub count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisDatabaseListResponse {
    Ok(IrisDatabaseListOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisNamespaceCreateOk {
    pub success: bool,
    pub created: bool,
    pub name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisNamespaceCreateResponse {
    Ok(IrisNamespaceCreateOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DbStatEntry {
    pub directory: String,
    pub free_space_mb: f64,
    pub free_blocks: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDatabaseStatsOk {
    pub success: bool,
    pub stats: Vec<DbStatEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisDatabaseStatsResponse {
    Ok(IrisDatabaseStatsOk),
    Err(ToolError),
}

// ── my_access / capability_matrix / hl7_schema_list / journal_search ───────

#[derive(Debug, Serialize, JsonSchema)]
pub struct MyAccessOk {
    pub success: bool,
    pub username: String,
    pub full_name: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MyAccessResponse {
    Ok(MyAccessOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CapabilityMatrixOk {
    pub success: bool,
    pub user: String,
    /// Absent when the queried user has no `Security.Users` row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    pub roles: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CapabilityMatrixResponse {
    Ok(CapabilityMatrixOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Hl7SchemaListOk {
    pub success: bool,
    pub schemas: Vec<String>,
    pub count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Hl7SchemaListResponse {
    Ok(Hl7SchemaListOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct JournalEntry {
    pub timestamp: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub job_id: i64,
    pub global: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct JournalSearchOk {
    pub success: bool,
    pub entries: Vec<JournalEntry>,
    pub returned: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum JournalSearchResponse {
    Ok(JournalSearchOk),
    Err(ToolError),
}

// ── compare_document / compare_namespace / global_preview ──────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct CompareDocumentOk {
    pub success: bool,
    pub document: String,
    pub server_a: String,
    pub server_b: String,
    pub namespace: String,
    pub same: bool,
    pub diff: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CompareDocumentResponse {
    Ok(CompareDocumentOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CompareNamespaceOk {
    pub success: bool,
    pub namespace: String,
    pub server_a: String,
    pub server_b: String,
    pub only_in_a: Vec<String>,
    pub only_in_b: Vec<String>,
    pub different: Vec<String>,
    pub same_count: usize,
    pub unchecked_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CompareNamespaceResponse {
    Ok(CompareNamespaceOk),
    Err(ToolError),
}

/// `global_preview` has no embedded-JSON error path at all — its one fallible step
/// (`execute_via_generator`) propagates via `?` as a protocol-level `McpError`, outside this
/// schema's scope (same pattern as `iris_ws_exec`/`iris_ws_close`). One flat success shape.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GlobalPreviewResponse {
    pub success: bool,
    pub global: String,
    pub server: Option<String>,
    pub entries: Vec<GlobalPreviewEntry>,
    pub total_subscripts: u32,
    pub confirm_token: String,
    pub confirm_expires: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GlobalPreviewEntry {
    pub key: String,
    pub value: String,
}

// ── query_audit_log / stream_inspect / iris_credential_list ────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct AuditLogEntry {
    /// Raw `%SYS.Audit` SQL columns — left as JSON values rather than String since the audit
    /// log's own columns can legitimately be null (e.g. no matching row for a filter).
    pub event: serde_json::Value,
    pub event_type: serde_json::Value,
    pub username: serde_json::Value,
    pub timestamp: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct QueryAuditLogOk {
    pub success: bool,
    pub entries: Vec<AuditLogEntry>,
    pub count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum QueryAuditLogResponse {
    Ok(QueryAuditLogOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StreamInspectOk {
    pub success: bool,
    pub oid: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub size: i64,
    pub content: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum StreamInspectResponse {
    Ok(StreamInspectOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CredentialEntry {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCredentialListOk {
    pub success: bool,
    pub credentials: Vec<CredentialEntry>,
    pub count: usize,
    pub truncated: bool,
    pub total_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisCredentialListResponse {
    Ok(IrisCredentialListOk),
    Err(ToolError),
}

// ── hl7_schema_inspect / mermaid_class / mermaid_production ─────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct Hl7SegmentField {
    pub field: String,
    pub description: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Hl7SchemaInspectSegmentOk {
    pub success: bool,
    pub schema: String,
    pub segment: String,
    pub fields: Vec<Hl7SegmentField>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Hl7SchemaInspectStructuresOk {
    pub success: bool,
    pub schema: String,
    pub structures: Vec<String>,
}

/// Two distinct success shapes, not one — segment-level lookup (`fields`) and message-structure
/// listing (`structures`) never appear together. schemars renders a 3-variant untagged enum as a
/// 3-way `oneOf`, same mechanism as the 2-variant `Ok | Err` case elsewhere in this file.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Hl7SchemaInspectResponse {
    Segment(Hl7SchemaInspectSegmentOk),
    Structures(Hl7SchemaInspectStructuresOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MermaidClassOk {
    pub success: bool,
    pub class: String,
    pub depth: u32,
    /// A `classDiagram`-prefixed Mermaid string, not structured JSON — this is a diagram, not
    /// tabular data.
    pub diagram: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MermaidClassResponse {
    Ok(MermaidClassOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MermaidProductionOk {
    pub success: bool,
    pub production: String,
    pub item_count: usize,
    /// A `flowchart TD`-prefixed Mermaid string.
    pub diagram: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MermaidProductionResponse {
    Ok(MermaidProductionOk),
    Err(ToolError),
}

// ── telemetry_query / telemetry_export_trace ────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct TelemetryRecord {
    pub tool: String,
    pub success: bool,
    pub duration_ms: u64,
    pub timestamp: String,
    pub session_id: String,
    /// The original call's params — free-form JSON, shaped differently per tool.
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TelemetryQueryOk {
    pub records: Vec<TelemetryRecord>,
    pub truncated: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TelemetryQueryResponse {
    Ok(TelemetryQueryOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TelemetryExportTraceOk {
    /// `{from, to, via, count, ts}` dispatch-trace records — left as free-form JSON rather than
    /// duplicating `trace_export::aggregate_trace`'s own record struct here.
    pub traces: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TelemetryExportTraceResponse {
    Ok(TelemetryExportTraceOk),
    Err(ToolError),
}

// ── skill_propose / skill_optimize / skill_share / skill_community_install ─
//
// All four are stubs — every call returns err_json("NOT_IMPLEMENTED", ...) unconditionally, so
// `ToolError` alone (no Ok variant, no oneOf) is the complete, accurate shape. Declaring it now
// means a future real implementation that changes the response shape without updating this file
// gets caught by test_output_schema_shapes-style coverage, instead of silently drifting.

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

// ── resolve_dynamic_dispatch / find_subclass_implementations ───────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct ResolveDynamicDispatchOk {
    pub success: bool,
    pub method_name: String,
    /// Absent on the empty-candidates early-return path, which skips these two fields entirely
    /// rather than nulling them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// `{class, origin, formal_spec, confidence}` objects — left as free-form JSON rather than
    /// duplicating the ObjectScript-generated shape here.
    pub candidates: Vec<serde_json::Value>,
    pub candidate_count: usize,
    pub confidence: f64,
    pub truncated: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ResolveDynamicDispatchResponse {
    Ok(ResolveDynamicDispatchOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindSubclassImplementationsOk {
    pub success: bool,
    pub method_name: String,
    pub base_classes: Vec<String>,
    pub namespace: String,
    /// `{class, formal_spec, confidence}` objects — free-form JSON, same reasoning as
    /// `ResolveDynamicDispatchOk::candidates`.
    pub implementations: Vec<serde_json::Value>,
    pub implementation_count: usize,
    pub confidence: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FindSubclassImplementationsResponse {
    Ok(FindSubclassImplementationsOk),
    Err(ToolError),
}

// ── skill_describe / skill_search ────────────────────────────────────────────

/// Distinct from `ToolError` — `skill_describe`'s one failure path (`NOT_FOUND`) adds `sources`
/// and `note` fields describing where it looked, per FR-004's "never a bare miss" requirement.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillNotFoundError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    pub sources: serde_json::Value,
    pub note: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillDescribeOk {
    pub success: bool,
    /// Bundled or synthesized skill metadata — free-form JSON, same reasoning as
    /// `SkillListResponse::skills`.
    pub skill: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SkillDescribeResponse {
    Ok(SkillDescribeOk),
    Err(SkillNotFoundError),
}

/// `skill_search` has no error path at all — bundled and synthesized skills are both handled
/// gracefully with no IRIS connection, so there's nothing left to fail on. One flat shape.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillSearchResponse {
    pub query: String,
    pub results: Vec<serde_json::Value>,
    pub count: usize,
    pub sources: serde_json::Value,
    pub note: String,
}

// ── iris_get_log ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisGetLogListOk {
    pub success: bool,
    /// Log-entry summaries (id, tool, timestamp, total count) — free-form JSON rather than
    /// duplicating `LogStore::list`'s own summary struct here.
    pub logs: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisGetLogPaginatedOk {
    pub success: bool,
    pub log_id: String,
    pub total_count: usize,
    pub offset: usize,
    pub limit: Option<usize>,
    pub has_more: bool,
    pub result: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisGetLogFullOk {
    pub success: bool,
    pub log_id: String,
    pub total_count: usize,
    pub result: serde_json::Value,
}

/// Three distinct success shapes, not one — the no-`id` listing path, the `id`+`limit` paginated
/// path, and the `id`-only full-result path never overlap in which fields they carry.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisGetLogResponse {
    List(IrisGetLogListOk),
    Paginated(IrisGetLogPaginatedOk),
    Full(IrisGetLogFullOk),
    Err(ToolError),
}

// ── agent_info / kb / kb_index ───────────────────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct AgentInfoStatsOk {
    pub success: bool,
    pub skill_count: usize,
    pub session_calls: usize,
    pub learning_enabled: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AgentInfoHistoryOk {
    pub success: bool,
    pub calls: Vec<serde_json::Value>,
}

/// `what=stats` and `what=history` never share fields — two distinct success shapes, driven by
/// the `what` param, same pattern as `hl7_schema_inspect` and `iris_get_log`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum AgentInfoResponse {
    Stats(AgentInfoStatsOk),
    History(AgentInfoHistoryOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbIndexOk {
    pub success: bool,
    pub indexed: usize,
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KbRecallActionOk {
    pub success: bool,
    pub query: String,
    /// `{file, excerpt}` objects built from raw ObjectScript-generated JSON — free-form.
    pub results: serde_json::Value,
}

/// The `kb` tool is action-multiplexed (`action=index` or `action=recall`); `kb_index` is a
/// separate, single-purpose tool that always calls the same underlying handler with
/// `action="index"` hardcoded, so it only ever produces the `Index` shape (plus `ToolError` for
/// the shared `LEARNING_DISABLED` gate) — see `KbIndexResponse` below.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum KbResponse {
    Index(KbIndexOk),
    Recall(KbRecallActionOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum KbIndexResponse {
    Ok(KbIndexOk),
    Err(ToolError),
}

// ── iris_credential_manage / iris_lookup_manage / iris_lookup_transfer ──────

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCredentialManageOk {
    pub success: bool,
    pub action: String,
    pub id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisCredentialManageResponse {
    Ok(IrisCredentialManageOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LookupListTablesOk {
    pub success: bool,
    pub tables: Vec<String>,
    pub count: usize,
    pub truncated: bool,
    pub total_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LookupGetOk {
    pub success: bool,
    pub table: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LookupSetOk {
    pub success: bool,
    pub table: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LookupDeleteOk {
    pub success: bool,
    pub table: String,
    pub key: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LookupListKeysOk {
    pub success: bool,
    pub table: String,
    pub keys: Vec<String>,
    pub count: usize,
}

/// Five distinct success shapes, one per `action` (`list_tables`/`get`/`set`/`delete`/
/// `list_keys`) — none share the same field set.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisLookupManageResponse {
    ListTables(LookupListTablesOk),
    Get(LookupGetOk),
    Set(LookupSetOk),
    Delete(LookupDeleteOk),
    ListKeys(LookupListKeysOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LookupExportOk {
    pub success: bool,
    pub table: String,
    pub xml: String,
    pub entry_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LookupImportOk {
    pub success: bool,
    pub table: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisLookupTransferResponse {
    Export(LookupExportOk),
    Import(LookupImportOk),
    Err(ToolError),
}

// ── iris_list_containers / iris_select_container / iris_start_sandbox ──────

/// No embedded-JSON error path at all — this tool never fails, only reports what it found.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisListContainersResponse {
    pub status: String,
    /// Raw container descriptors from `iris-devtester`/`docker ps` — free-form JSON.
    pub containers: Vec<serde_json::Value>,
    pub workspace_basename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    pub workspace_config: serde_json::Value,
    pub active_connection: serde_json::Value,
    pub mismatch: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mismatch_hint: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisSelectContainerOk {
    pub status: String,
    pub switched: bool,
    pub container: String,
    pub port_superserver: u16,
    pub port_web: u16,
    pub namespace: String,
    pub version: Option<String>,
    pub write_tools_enabled: bool,
}

/// Distinct from `ToolError` — `iris_select_container`'s two failure shapes put the error CODE
/// directly in the `error` field itself (never `error_code`+`error`), and each carries different
/// extra context (`requested`/`available` vs `container`/`port_web`/`message`).
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisSelectContainerNotFound {
    pub success: bool,
    pub error: String,
    pub requested: String,
    pub available: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisSelectContainerUnreachable {
    pub success: bool,
    pub error: String,
    pub container: String,
    pub port_web: u16,
    pub message: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisSelectContainerResponse {
    Ok(IrisSelectContainerOk),
    NotFound(IrisSelectContainerNotFound),
    Unreachable(IrisSelectContainerUnreachable),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisStartSandboxIdempotentOk {
    pub name: String,
    /// Pulled straight from the container descriptor JSON — free-form, same reasoning as
    /// `IrisListContainersResponse::containers`.
    pub port_superserver: serde_json::Value,
    pub port_web: serde_json::Value,
    pub started: bool,
    pub idempotent: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisStartSandboxStartedOk {
    pub name: String,
    pub port_superserver: serde_json::Value,
    pub port_web: serde_json::Value,
    pub started: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisStartSandboxStartedNoPortsOk {
    pub name: String,
    pub started: bool,
    pub warning: String,
}

/// Three success shapes (idempotent-found / started-with-ports / started-but-not-yet-visible)
/// plus `ToolError` for the `idt` CLI failure paths.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisStartSandboxResponse {
    Idempotent(IrisStartSandboxIdempotentOk),
    Started(IrisStartSandboxStartedOk),
    StartedNoPorts(IrisStartSandboxStartedNoPortsOk),
    Err(ToolError),
}

// ── iris_generate_class / iris_generate_test ────────────────────────────────

/// Shared by both generate tools — an LLM response that failed `validate_cls_syntax` returns
/// this instead of `ToolError` (no `error` field; `raw_llm_output` instead).
#[derive(Debug, Serialize, JsonSchema)]
pub struct GenerateInvalidOutputError {
    pub success: bool,
    pub error_code: String,
    pub raw_llm_output: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisGenerateClassOk {
    pub success: bool,
    pub class_name: String,
    pub class_text: String,
    pub compiled: bool,
    pub retried: bool,
    /// Present only on the no-IRIS-connection path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `LLM_UNAVAILABLE`/`LLM_TIMEOUT` propagate via `?` as protocol-level `McpError`, outside this
/// schema's scope — the only embedded-JSON failure is the invalid-output case.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisGenerateClassResponse {
    Ok(IrisGenerateClassOk),
    InvalidOutput(GenerateInvalidOutputError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisGenerateTestOk {
    pub success: bool,
    pub class_name: String,
    pub test_class_name: String,
    pub test_text: String,
    pub introspected: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisGenerateTestResponse {
    Ok(IrisGenerateTestOk),
    InvalidOutput(GenerateInvalidOutputError),
}

// ── resolve_storage / iris_info / iris_table_info ───────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct ResolveStorageOk {
    pub success: bool,
    pub class: String,
    /// `{name, type, data_location, id_location, index_location}` objects — free-form JSON,
    /// values come straight from an SQL query row.
    pub storages: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ResolveStorageResponse {
    Ok(ResolveStorageOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisInfoOk {
    pub success: bool,
    pub what: String,
    pub namespace: String,
    /// Shape depends entirely on `what` (raw Atelier REST response body) — free-form JSON.
    pub result: serde_json::Value,
    /// Present only for `what=documents`, where the document list is flattened to a top-level
    /// key and progressive-disclosure truncation (`log_store::apply_truncation`) may apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisInfoResponse {
    Ok(IrisInfoOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisTableInfoOk {
    pub success: bool,
    /// Two internal shapes depending on whether the table is class-projected or a plain DDL
    /// table (`type: "class_projection"` vs `"ddl_table"`, differing fields beyond that) — left
    /// as free-form JSON rather than a second nested union, to keep this batch's scope real.
    pub result: serde_json::Value,
}

/// Distinct from `ToolError` — no `error_code` field, just `error` plus `table`/`namespace`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisTableInfoNotFound {
    pub success: bool,
    pub error: String,
    pub table: String,
    pub namespace: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisTableInfoResponse {
    Ok(IrisTableInfoOk),
    NotFound(IrisTableInfoNotFound),
}

// ── iris_doc_search ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocSearchOk {
    pub query: String,
    pub total_hits: u64,
    /// `{title, url, excerpt, breadcrumbs, version, product}` objects — free-form JSON.
    pub hits: Vec<serde_json::Value>,
}

/// Distinct from `ToolError` — no `success`/`error_code` fields at all, just `error` + `hits`
/// (always empty on this path).
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocSearchError {
    pub error: String,
    pub hits: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisDocSearchResponse {
    Ok(IrisDocSearchOk),
    Err(IrisDocSearchError),
}

// ── iris_message_body / iris_business_rule_info / iris_production_diff ─────

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisMessageBodyOk {
    pub success: bool,
    pub message_id: String,
    pub content_type: String,
    pub body: String,
    pub truncated: bool,
    pub actual_size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bytes_clamped: Option<bool>,
}

/// `iris_message_body`'s wrapper calls the shared cross-tool policy gate
/// (`crate::policy::gate::dispatch_gate`) *before* the impl function this file otherwise models
/// runs at all, and short-circuits with `ok_json(gate)` on a block — a real gap in this file's
/// first pass at this tool's schema, caught while modeling `iris_execute_method` (which calls
/// the same gate) in a later batch. The gate's blocked-response shape genuinely varies by which
/// of its four internal checks fired (env-template, bulk-PHI, global blocklist, PHI-name
/// pattern) — left as free-form JSON rather than a fourth nested union on top of this tool's own
/// two variants; see `PolicyGateBlocked`'s doc comment below for the shared reasoning.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisMessageBodyResponse {
    Ok(IrisMessageBodyOk),
    Err(ToolError),
    GateBlocked(serde_json::Value),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BusinessRuleListOk {
    pub success: bool,
    /// `{name, class_name, description, modified}` objects — free-form JSON.
    pub rules: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BusinessRuleGetOk {
    pub success: bool,
    pub name: String,
    pub description: serde_json::Value,
    /// Placeholder `{}` entries, one per condition/action — the RuleSet's own condition/action
    /// objects aren't introspected further, only counted. Left as free-form JSON.
    pub conditions: Vec<serde_json::Value>,
    pub actions: Vec<serde_json::Value>,
}

/// `action=list` and `action=get` never share fields — two distinct success shapes.
/// Same retroactive fix as `IrisMessageBodyResponse` above — this tool's wrapper also calls the
/// shared policy gate before its own logic runs.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisBusinessRuleInfoResponse {
    List(BusinessRuleListOk),
    Get(BusinessRuleGetOk),
    Err(ToolError),
    GateBlocked(serde_json::Value),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProductionDiffChange {
    pub item_name: String,
    pub item_type: String,
    pub status: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisProductionDiffOk {
    pub success: bool,
    pub in_sync: bool,
    pub changes: Vec<ProductionDiffChange>,
}

/// Same retroactive fix as `IrisMessageBodyResponse` above.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisProductionDiffResponse {
    Ok(IrisProductionDiffOk),
    Err(ToolError),
    GateBlocked(serde_json::Value),
}

// ── iris_execute_method / iris_macro / iris_debug / iris_generate ──────────
// ── skill / skill_community ─────────────────────────────────────────────────

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisExecuteMethodOk {
    pub success: bool,
    pub return_value: String,
}

/// This tool's wrapper calls the shared cross-tool policy gate
/// (`crate::policy::gate::dispatch_gate`) before `handle_iris_execute_method` runs — the same
/// gate `iris_message_body`/`iris_business_rule_info`/`iris_production_diff` call, hence the
/// same `GateBlocked` treatment. At least 6 more tools in this codebase call this same gate
/// (`iris_compile`, `iris_execute`, `iris_query`, `iris_source_control`, `iris_global`) — a
/// future batch declaring their schemas must account for it too, not just their own impl
/// function's shape.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisExecuteMethodResponse {
    Ok(IrisExecuteMethodOk),
    Err(ToolError),
    GateBlocked(serde_json::Value),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisMacroListOk {
    pub success: bool,
    pub macros: Vec<String>,
    pub note: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisMacroActionOk {
    pub success: bool,
    pub name: String,
    pub action: String,
    /// Raw Atelier `/action/getmacro` response body — free-form JSON.
    pub result: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisMacroResponse {
    List(IrisMacroListOk),
    Action(IrisMacroActionOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDebugMapIntOk {
    pub success: bool,
    pub error_string: String,
    pub source_location: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDebugErrorLogsOk {
    pub success: bool,
    pub logs: Vec<serde_json::Value>,
    pub note: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDebugCaptureOk {
    pub success: bool,
    pub capture: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDebugSourceMapOk {
    pub success: bool,
    pub class: String,
    pub mapping: String,
}

/// Distinct from batch 1's `debug_map_int_to_cls`/`debug_source_map` tools — `iris_debug` is a
/// separate implementation in `info.rs`, not a thin dispatcher to those same handlers, so it
/// gets its own response types rather than reusing theirs. Its `DOCKER_REQUIRED` failure path
/// happens to already match `ToolError`'s exact shape (`success`, `error_code`, `error`), so no
/// bespoke error type is needed here, unlike `skill_forget`'s identical-looking case in batch 1.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisDebugResponse {
    MapInt(IrisDebugMapIntOk),
    ErrorLogs(IrisDebugErrorLogsOk),
    Capture(IrisDebugCaptureOk),
    SourceMap(IrisDebugSourceMapOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisGenerateTestGenContext {
    /// `%Dictionary.CompiledMethod` query rows — free-form JSON.
    pub methods: serde_json::Value,
    pub suggested_class_name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisGenerateTestGenOk {
    pub success: bool,
    pub gen_type: String,
    pub target_class: String,
    pub namespace: String,
    pub prompt: String,
    pub context: IrisGenerateTestGenContext,
    pub instructions: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisGenerateClassGenContext {
    pub existing_classes: Vec<String>,
    pub suggested_package: String,
    pub iris_version: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisGenerateClassGenOk {
    pub success: bool,
    pub gen_type: String,
    pub namespace: String,
    pub prompt: String,
    pub context: IrisGenerateClassGenContext,
    pub instructions: String,
}

/// `iris_generate` (the context-provider tool — distinct from the LLM-backed
/// `iris_generate_class`/`iris_generate_test`) has no embedded-JSON error path at all; HTTP
/// failures propagate via `?`. Two success shapes, driven by `gen_type`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisGenerateResponse {
    Test(IrisGenerateTestGenOk),
    Class(IrisGenerateClassGenOk),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillListActionOk {
    pub success: bool,
    /// `{name, description, usage_count}` objects built from raw `^SKILLS` global data —
    /// free-form JSON.
    pub skills: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillDescribeActionOk {
    pub success: bool,
    pub name: String,
    pub description: String,
    pub body: String,
    /// Parsed from a pipe-delimited `^SKILLS` global value — always a numeric string, never a
    /// JSON number, hence `String` here.
    pub usage_count: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillSearchActionOk {
    pub success: bool,
    pub query: String,
    /// `{name, description}` objects — free-form JSON.
    pub results: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillForgetActionOk {
    pub success: bool,
    pub name: String,
    pub action: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProposedSkill {
    pub name: String,
    pub description: String,
    pub body: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillProposeActionOk {
    pub success: bool,
    pub skill: ProposedSkill,
}

/// The `skill` tool (learning-agent skill registry management — distinct from the individual
/// `skill_list`/`skill_describe`/`skill_search`/`skill_forget` tools, which are separate
/// implementations reading `^SKILLS` directly rather than delegating to this one) is
/// action-multiplexed across five shapes, all sharing the same `ToolError` failure convention
/// (`LEARNING_DISABLED`/`NOT_FOUND`/`INSUFFICIENT_HISTORY`/`INVALID_PARAM`).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SkillResponse {
    List(SkillListActionOk),
    Describe(SkillDescribeActionOk),
    Search(SkillSearchActionOk),
    Forget(SkillForgetActionOk),
    Propose(SkillProposeActionOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillCommunityListActionOk {
    pub success: bool,
    /// `{name, description}` objects — free-form JSON.
    pub skills: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillCommunityInstallActionOk {
    pub success: bool,
    pub installed: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SkillCommunityResponse {
    List(SkillCommunityListActionOk),
    Install(SkillCommunityInstallActionOk),
    Err(ToolError),
}

// ── iris_query ────────────────────────────────────────────────────────────────
//
// One of the five core execution tools (spec 076 US2's CLI delegation target) — deferred out
// of batches 1-6 because it's genuinely one of the largest, highest-variance tools in the
// codebase: four modes (default/select, explain, count, write), three independent gate
// mechanisms ahead of any of them (`dispatch_gate`, `crate::iris::server_manager::policy_gate`,
// `workspace_config::check_role_gate`), and per-mode bespoke error shapes beyond the shared
// `ToolError` convention. Modeled in full rather than left dynamic, since — unlike this file's
// other deferred tools — every branch was actually read this pass.

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisQuerySelectOk {
    pub success: bool,
    /// SQL result rows — free-form JSON, shape depends entirely on the query's own SELECT list.
    pub rows: Vec<serde_json::Value>,
    pub count: usize,
    pub namespace: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisQueryExplainOk {
    pub success: bool,
    pub plan_text: String,
    pub query_hash: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisQueryCountOk {
    pub success: bool,
    pub count: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisQueryWriteOk {
    pub success: bool,
    pub rows_affected: i64,
    /// Present only when `force: true` was passed (DML that would otherwise be blocked, run
    /// anyway because write tools are enabled).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_ignored: Option<bool>,
    /// Present only when the UPDATE/DELETE rows-affected pre-check couldn't confidently parse
    /// the statement (multi-table UPDATE, missing table name) and was skipped rather than run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_check_skipped: Option<bool>,
}

/// `mode="select"` (the default)'s `SQL_WRITE_BLOCKED` case — a destructive keyword was found
/// without `force: true`. Extends `ToolError`'s fields with `blocked_keyword`, and optionally
/// `force_ignored` (set when `force: true` was passed but write tools aren't enabled, so the
/// override itself was ignored) — too different from plain `ToolError` to reuse it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SqlWriteBlockedError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    pub blocked_keyword: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_ignored: Option<bool>,
}

/// `mode="write"`'s DDL-keyword-detected case (distinct from the plain `err_json` DDL_NOT_ALLOWED
/// case for statement types `validate_dml_sql` doesn't recognize at all, which matches `ToolError`
/// exactly and needs no separate type).
#[derive(Debug, Serialize, JsonSchema)]
pub struct DdlNotAllowedError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    pub blocked_keyword: String,
}

/// `mode="write"`'s UPDATE/DELETE rows-affected pre-check limit.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RowsLimitExceededError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    pub actual_count: i64,
    pub limit: u32,
}

/// The three independent gate mechanisms `iris_query` runs before any mode-specific logic
/// (`dispatch_gate`, `policy_gate` with its own `allowed_categories` field, `check_role_gate`)
/// each produce their own free-form blocked-response JSON — genuinely three different possible
/// shapes, not one. Left dynamic rather than three more nested variants; see `IrisMessageBodyResponse`'s
/// doc comment for the same reasoning applied to a single gate.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisQueryResponse {
    Select(IrisQuerySelectOk),
    Explain(IrisQueryExplainOk),
    Count(IrisQueryCountOk),
    Write(IrisQueryWriteOk),
    SqlWriteBlocked(SqlWriteBlockedError),
    DdlNotAllowed(DdlNotAllowedError),
    RowsLimitExceeded(RowsLimitExceededError),
    Err(ToolError),
    /// `IRIS_UNREACHABLE` on this tool's four HTTP-calling modes goes through
    /// `err_json_with_url`, not plain `err_json` — see `IrisUnreachableWithUrlError`'s doc
    /// comment. Found and added while modeling `iris_compile` (batch 8), which shares the same
    /// helper; this variant was missing from this type's first pass in batch 7. Doesn't change
    /// what actually validates (schemars only emits `additionalProperties: false` under
    /// `#[serde(deny_unknown_fields)]`, which nothing here uses, so the plain `Err(ToolError)`
    /// variant already accepted these responses' extra fields) — this only fixes the schema's
    /// own accuracy as documentation.
    UnreachableWithUrl(IrisUnreachableWithUrlError),
}

// ── iris_compile ──────────────────────────────────────────────────────────────
//
// The second of the five core execution tools (see `IrisQueryResponse`'s doc comment). Three
// sub-paths depending on connection capability and target shape: docker-exec (no Atelier REST
// available), local-file upload+compile, and the normal Atelier `/action/compile` path — each
// with its own success shape and, for the last one, its own progressive-disclosure truncation.

/// Shared by every mode of every core execution tool that reports an unreachable IRIS instance
/// via the `err_json_with_url` helper (`iris_compile`, `iris_query`, and likely `iris_execute`/
/// `iris_test`/`iris_doc` once those are modeled) — adds `attempted_url` and a fixed `hint`
/// string on top of `ToolError`'s three fields, so it doesn't fit `ToolError` exactly despite
/// looking like a minor variant of it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisUnreachableWithUrlError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    pub attempted_url: String,
    pub hint: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CompileErrorEntry {
    pub severity: String,
    pub code: String,
    pub line: u32,
    pub column: u32,
    pub text: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCompileDockerExecOk {
    pub success: bool,
    pub target: String,
    pub namespace: String,
    pub method: String,
    pub output: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCompileLocalPathOk {
    pub success: bool,
    pub target: String,
    pub uploaded_from: String,
    pub targets_compiled: usize,
    pub namespace: String,
    pub errors: Vec<CompileErrorEntry>,
    /// Always `[]` on this path today — the local-file upload branch never populates warnings,
    /// only errors — but modeled as a real array rather than a fixed empty one, since nothing
    /// stops a future change from populating it the way the main compile path already does.
    pub warnings: Vec<serde_json::Value>,
    pub console: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCompileOk {
    pub success: bool,
    pub target: String,
    pub targets_compiled: usize,
    pub namespace: String,
    pub errors: Vec<CompileErrorEntry>,
    pub warnings: Vec<CompileErrorEntry>,
    /// Raw Atelier compile console lines — free-form JSON (normally strings, but sourced
    /// directly from the HTTP response body rather than something this code constructs).
    pub console: Vec<serde_json::Value>,
    /// Present only for a successful, non-wildcard, single-target compile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_uri: Option<String>,
    /// Progressive disclosure (`log_store::apply_truncation`) — present whenever the
    /// errors+warnings count was checked against the threshold, which is every response on
    /// this path (unlike `debug_get_error_logs`'s community-edition fallback, there's no early
    /// return here that skips the check).
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisCompileResponse {
    DockerExec(IrisCompileDockerExecOk),
    LocalPath(IrisCompileLocalPathOk),
    Ok(IrisCompileOk),
    Err(ToolError),
    UnreachableWithUrl(IrisUnreachableWithUrlError),
    GateBlocked(serde_json::Value),
}

// ── iris_test ─────────────────────────────────────────────────────────────────
//
// The third of the five core execution tools. No policy gate here — unlike `iris_query`/
// `iris_compile`, `iris_test` never calls `dispatch_gate`/`policy_gate`/`check_role_gate`, so
// there's no `GateBlocked` variant to add. Its complexity is elsewhere: parsing free-text
// `%UnitTest.Manager` RunTest stdout into structured pass/fail results, an optional coverage
// sub-run wrapping the whole thing, and a `NO_TESTS_FOUND` case IRIS itself doesn't distinguish
// from "zero test methods ran" at the protocol level — this code has to infer it.

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisTestCase {
    pub name: String,
    pub class_name: String,
    pub status: String,
    /// Always `null` today — stdout parsing recovers pass/fail and a failure message, but not
    /// per-method timing. Modeled as `Option<u64>`, not a fixed null, in case that changes.
    pub duration_ms: Option<u64>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisTestSuite {
    pub name: String,
    pub tests: usize,
    pub failures: u64,
    /// Always `0` — this code never distinguishes a suite-level error from a failure.
    pub errors: u64,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisTestOk {
    pub success: bool,
    pub total: u64,
    pub passed: u64,
    pub failed: u64,
    /// Always `0` at the top level too — see `IrisTestSuite::errors`.
    pub errors: u64,
    /// Always `0` — this tool never skips individual test methods.
    pub skipped: u64,
    pub duration_ms: Option<u64>,
    pub path: String,
    /// Full per-test-case detail (nested under each suite) is stored here, retrievable via
    /// `iris_get_log` — this response carries only the suite-level summary.
    pub log_id: String,
    pub pattern: String,
    pub namespace: String,
    pub test_suites: Vec<IrisTestSuite>,
    /// Present only when `coverage: true` was passed — the `mode=report` result from the
    /// coverage monitor `iris_test` itself started/stopped around the run. Tightened from
    /// `serde_json::Value` once `iris_coverage`'s own schema landed (batch 12) —
    /// `IrisCoverageReportResult` deliberately narrows to just the two shapes `mode=report`
    /// can actually produce, not the full 5-variant `IrisCoverageResponse` (which also
    /// covers check/run/start/stop, none reachable from here).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<IrisCoverageReportResult>,
}

/// Distinct from `ToolError` — adds a `namespace` field.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisTestNamespaceNotFoundError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    pub namespace: String,
}

/// The "pattern matched no test classes" case — IRIS creates a synthetic 1-failure suite at the
/// path-separator level rather than reporting this as a real error, so this code has to infer
/// it from an empty parsed-method list and report it as its own shape: `ToolError`'s three
/// fields plus a pattern-shape-aware `hint`, echoed `pattern`/`namespace`, and zeroed counts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisTestNoTestsFoundError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    pub hint: String,
    pub pattern: String,
    pub namespace: String,
    pub total: u64,
    pub passed: u64,
    pub failed: u64,
    pub path: String,
    pub source: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisTestResponse {
    Ok(IrisTestOk),
    NamespaceNotFound(IrisTestNamespaceNotFoundError),
    NoTestsFound(IrisTestNoTestsFoundError),
    Err(ToolError),
}

// ── iris_execute ──────────────────────────────────────────────────────────────
//
// The fourth core execution tool. Same three gate mechanisms as `iris_query`/`iris_compile`.
// Two genuinely different success shapes depending on which path actually ran (`method:
// "http"` vs `method: "docker"`) — the HTTP path carries the service-account routing
// diagnostics (`auth_user`/`service_account_env`) and the `%ctx` session carrier
// (`session_state`); the docker fallback path carries neither, since it always runs as the
// primary connection's own identity and has no session support.

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisExecuteHttpOk {
    pub success: bool,
    pub output: String,
    pub namespace: String,
    pub method: String,
    /// The identity this call actually authenticated as — always present, distinct from
    /// `error_code`'s absence-means-success convention, so callers can observe service-account
    /// routing on every HTTP-path call, not just failures.
    pub auth_user: String,
    /// Empty string when `IRIS_SERVICE_USERNAME` isn't configured, not omitted — this is a
    /// diagnostic field, not an optional one.
    pub service_account_env: String,
    /// Present only when the executed code's own output looked like an ObjectScript runtime
    /// error (`ERROR: `/`ERROR($ZERROR): ` prefix) — `success` is `false` in that case, but the
    /// code path is otherwise identical to a normal completion, hence no separate error type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Present only when `use_session: true` was passed — the opaque Base64 `%ctx` carrier
    /// token to round-trip on the next call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sql_translated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation_warning: Option<Vec<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisExecuteDockerOk {
    pub success: bool,
    pub output: String,
    pub namespace: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sql_translated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation_warning: Option<Vec<String>>,
}

/// The `%ctx` session carrier's own fatal-error path (bad token, restore failure, serialize
/// failure) — takes priority over normal output processing on the HTTP path, and carries the
/// same routing diagnostics `IrisExecuteHttpOk` does, on top of `ToolError`'s three fields.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisExecuteSessionError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    pub namespace: String,
    pub method: String,
    pub auth_user: String,
    pub service_account_env: String,
}

/// The docker-fallback path's `DOCKER_REQUIRED` case — reported as `HTTP_EXECUTION_FAILED`
/// instead, since HTTP (not docker) is the primary path and its real failure is the actual
/// cause; carries the original HTTP error as `http_error` on top of `ToolError`'s three fields.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisExecuteHttpExecutionFailedError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    pub http_error: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisExecuteResponse {
    Http(IrisExecuteHttpOk),
    Docker(IrisExecuteDockerOk),
    SessionError(IrisExecuteSessionError),
    HttpExecutionFailed(IrisExecuteHttpExecutionFailedError),
    Err(ToolError),
    GateBlocked(serde_json::Value),
}

// ── iris_doc ───────────────────────────────────────────────────────────────────
//
// The fifth and last core execution tool, and the largest by mode count: get, put,
// delete, head, fragment, compiled, list, insert, delete_lines — plus a top-level
// elicitation-resume path handled before mode dispatch (so it works uniformly for
// put and the two surgical-edit modes). No gate calls at all (`grep` confirms — like
// `iris_test`, not like `iris_query`/`iris_compile`/`iris_execute`), so no `GateBlocked`
// variant.
//
// Every single-document read/write mode (get/put/delete/head/fragment/compiled) shares
// one required-name guard (`require_name`) whose JSON happens to already match
// `ToolError`'s three fields exactly, so MISSING_PARAMS across all of them reuses it
// rather than a bespoke type.
//
// The two batch paths (`names` non-empty on get/delete) are deliberately modeled with
// `serde_json::Value` entries rather than a nested per-item oneof: each entry is itself
// `{name, content}` on success or `{name, error}` on failure, decided per-document, and
// the outer response is unconditionally `success: true` even when every entry failed —
// the real status lives inside each entry. A oneof-of-oneof here would cost more schema
// complexity than it documents.

/// mode=get, single document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocGetOk {
    pub success: bool,
    pub name: String,
    pub content: String,
    /// Atelier's per-document ETag/timestamp, empty string if the server didn't send one.
    pub timestamp: String,
}

/// mode=get with `names` set (batch fetch, concurrent). Always `success: true` at this
/// level — see the module-level note on why per-document outcomes live inside `documents`
/// instead of here.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocGetBatchOk {
    pub success: bool,
    /// Each entry is `{name, content}` (fetched) or `{name, error}` (failed) — see the
    /// module doc comment for why this stays `Value` rather than a nested oneof.
    pub documents: Vec<serde_json::Value>,
}

/// The SCM checkout confirmation dialog — returned by mode=put and, with the extra
/// edit-annotation fields populated, by mode=insert/mode=delete_lines when the target
/// document needs checkout before either can write. Resume by calling again with
/// `elicitation_id` + `elicitation_answer: "yes"|"no"`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocElicitationRequired {
    pub success: bool,
    pub elicitation_required: bool,
    pub elicitation_id: String,
    pub message: String,
    pub options: Vec<String>,
    /// Present only when this dialog was raised from mode=insert or mode=delete_lines
    /// (`annotate_edit` merges the pending edit's metadata onto the dialog response so a
    /// caller resuming it can still see what edit is about to happen).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inserted_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_added: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_removed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

/// The shared "a write actually landed" shape — covers mode=put (with or without
/// `compile: true`), an elicitation-resume write, and a successful mode=insert/
/// mode=delete_lines edit, all via the same optional annotation fields. Keeping these
/// as one struct rather than four separate ones reflects what the code actually does:
/// `do_write`'s core success JSON gets progressively more fields merged onto it
/// (`annotate_edit`) depending on which caller invoked it, never fewer.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocWriteOk {
    /// `true` unless `compile: true` was passed and the compile itself failed — in that
    /// case the write landed but `success` reports the compile outcome, not plain "write
    /// happened", and `compile_errors` explains why.
    pub success: bool,
    pub name: String,
    pub open_uri: String,
    /// `true` if the class had an explicit Storage block that got stripped before writing
    /// (see `allow_storage_regeneration` on the request).
    pub storage_stripped: bool,
    /// Present only when `compile: true` was passed on the write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compile_errors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compile_console: Option<Vec<String>>,
    /// Present only when this write was reached via an elicitation resume
    /// (`elicitation_id`/`elicitation_answer: "yes"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed: Option<bool>,
    /// Present only when this write came from mode=insert or mode=delete_lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inserted_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_added: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_removed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// Post-write line count, re-fetched fresh so a chained surgical edit has authoritative
    /// numbers (IRIS renumbers .cls source on save). Present on a surgical-edit or resumed
    /// write when the re-fetch itself succeeded; absent if the write succeeded but the
    /// immediate re-fetch failed (rare — `finalize_edit` swallows that failure rather than
    /// turning a landed write into an error).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_lines: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// mode=delete, single document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocDeleteOk {
    pub success: bool,
    pub name: String,
}

/// mode=delete with `names` set (batch). Unlike batch-get, `success` here does reflect the
/// real outcome (`errors.is_empty()`) — partial failure is visible at the top level, not
/// just per-entry.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocDeleteBatchOk {
    pub success: bool,
    pub deleted: Vec<String>,
    /// Each entry is `{name, error}`.
    pub errors: Vec<serde_json::Value>,
}

/// mode=head.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocHeadOk {
    pub success: bool,
    pub name: String,
    pub exists: bool,
    /// The ETag header, empty string if absent — same convention as mode=get's `timestamp`.
    pub timestamp: String,
}

/// mode=fragment: a 1-based inclusive line-range read.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocFragmentOk {
    pub success: bool,
    pub name: String,
    pub lines: Vec<String>,
    /// The actual (post-clamp) start/end, which may differ from the request if it ran past
    /// end-of-file.
    pub start: i64,
    pub end: i64,
    /// `true` if `end` (or `start`) had to be clamped to fit the document.
    pub clamped: bool,
    pub total_lines: i64,
}

/// mode=compiled: the INT form of a compiled document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocCompiledOk {
    pub success: bool,
    pub name: String,
    /// The derived IRIS routine name the INT form was read from (e.g. `MyApp.Patient.1`
    /// for a `.cls`).
    pub routine: String,
    /// Always `"INT"` — `OBJ` is accepted as a request value but not yet implemented.
    pub category: String,
    pub content: String,
    pub total_lines: i64,
}

/// mode=list: glob-matched docnames.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocListOk {
    pub success: bool,
    pub documents: Vec<IrisDocListEntry>,
    pub count: i64,
    /// `true` if the real match count exceeded `max_results` and the list was truncated.
    pub truncated: bool,
    pub namespace: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocListEntry {
    pub name: String,
    pub category: String,
    pub ts: String,
}

/// The `STALE_CONTENT` refusal from mode=insert (positional) or mode=delete_lines when the
/// live document no longer matches the caller's `expected` text at the targeted line(s).
/// Extends `ToolError`'s three fields with the exact divergence, so a caller can decide
/// whether to re-fetch-and-retry or investigate further, without re-fetching blind first.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocStaleContentError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    /// 1-based line number of the first divergence.
    pub line: i64,
    pub expected_line: String,
    pub actual_line: String,
}

/// A rare failure path: mode=insert/mode=delete_lines already fetched the document,
/// computed the diff, and called through to the actual write — which then failed
/// (`SCM_REJECTED`, an HTTP error, or a requested `compile: true` failing) after the edit
/// was already known. `annotate_edit` merges the edit metadata onto the failure the same
/// way it does onto a success, so these carry the same optional annotation fields as
/// `IrisDocWriteOk` on top of `ToolError`'s three.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisDocEditFailedError {
    pub success: bool,
    pub error_code: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inserted_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_added: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_removed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisDocResponse {
    Get(IrisDocGetOk),
    GetBatch(IrisDocGetBatchOk),
    ElicitationRequired(IrisDocElicitationRequired),
    WriteOk(IrisDocWriteOk),
    Delete(IrisDocDeleteOk),
    DeleteBatch(IrisDocDeleteBatchOk),
    Head(IrisDocHeadOk),
    Fragment(IrisDocFragmentOk),
    Compiled(IrisDocCompiledOk),
    List(IrisDocListOk),
    StaleContent(IrisDocStaleContentError),
    EditFailed(IrisDocEditFailedError),
    Err(ToolError),
}

// ── iris_coverage ────────────────────────────────────────────────────────────
//
// A fourth error-shape convention, distinct from `ToolError`: this tool's own local
// `err_json` helper uses `message`, not `error`, as the free-text field name
// (`{success, error_code, message}`). No gate calls. mode=check's success case is a fifth
// convention on top of that — it has no `success` field at all, using `ok: true` instead
// (mirrors `IrisDocSearchError`'s precedent of a tool inventing its own top-level marker).
//
// The ObjectScript-side output protocol is pipe-delimited text, not JSON, parsed by
// `parse_check_output`/`parse_coverage_output` — both have a JSON-passthrough branch
// (`trimmed.starts_with('{')`) explicitly for feeding test fixtures directly; real IRIS
// output is never JSON-shaped, so that branch is not modeled as a schema variant.
//
// mode=run and mode=check both merge extra fields onto their parsed result — including
// onto an *error* result, unconditionally, not just onto success — so `IrisCoverageError`
// carries all of those extras as optional rather than needing a per-mode error type.
// mode=report calls the exact same coverage-result parser as mode=run but skips the merge
// entirely, which is why `IrisCoverageRunOk`'s run-specific fields are optional: report's
// success case is the same shape with them simply absent.

/// mode=check success: `{ok: true, ...}`, not `{success: true, ...}` — this tool's own
/// invention, kept as-is rather than normalized, since this is documenting the real shape.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCoverageCheckOk {
    pub ok: bool,
    /// Always `"ready"` when `ok` is true.
    pub bbsiz_state: String,
    pub testcoverage_available: bool,
    /// Present only when `testcoverage_available` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testcoverage_hint: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCoverageClassEntry {
    pub class: String,
    pub routine: String,
    pub hit: i64,
    pub total: i64,
    pub pct: f64,
}

/// mode=run and mode=report success (report's own success is this same shape with the
/// run-only fields absent — see the module note on why they're optional here).
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCoverageRunOk {
    pub success: bool,
    pub total_pct: f64,
    pub hits: i64,
    pub total: i64,
    pub classes: Vec<IrisCoverageClassEntry>,
    /// mode=run only, and only when `target_pct` was passed on the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meets_target: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_pct: Option<f64>,
    /// mode=run only — always present there, absent on mode=report (no merge step).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testcoverage_available: Option<bool>,
    /// mode=run only, and only when `cobertura_path` was requested but the TestCoverage
    /// IPM package isn't installed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cobertura_skipped: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCoverageStartOk {
    pub success: bool,
    pub started: bool,
    pub routines: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCoverageStopOk {
    pub success: bool,
    pub stopped: bool,
}

/// The one error shape shared by every mode (`MISSING_PARAM`, `NO_CLASSES`,
/// `INVALID_ACTION`, `SQL_ERROR`, `MONITOR_IN_USE`, `BBSIZ_NOT_CONFIGURED`,
/// `IRIS_EXECUTE_ERROR`, `PARSE_ERROR`, `IRIS_UNREACHABLE`) — note `message`, not `error`
/// (see the module note). The trailing fields are mode-specific merge extras, all optional
/// since which ones (if any) appear depends on which mode produced this error and whether
/// that mode's merge step ran at all.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IrisCoverageError {
    pub success: bool,
    pub error_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// mode=check's `BBSIZ_NOT_CONFIGURED` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// mode=run only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meets_target: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cobertura_skipped: Option<String>,
    /// mode=check or mode=run, when their merge step ran (a raw transport failure that
    /// short-circuits before the merge has neither this nor `testcoverage_hint`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testcoverage_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testcoverage_hint: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisCoverageResponse {
    CheckOk(IrisCoverageCheckOk),
    RunOk(IrisCoverageRunOk),
    StartOk(IrisCoverageStartOk),
    StopOk(IrisCoverageStopOk),
    Err(IrisCoverageError),
}

/// The narrower shape of just `mode=report`'s two possible outcomes — used as the type of
/// `IrisTestOk::coverage`, which always calls `iris_coverage` with `mode: "report"`
/// internally. See that field's doc comment for why this exists instead of reusing the
/// full `IrisCoverageResponse` (whose other three variants can never appear there).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisCoverageReportResult {
    Ok(IrisCoverageRunOk),
    Err(IrisCoverageError),
}
