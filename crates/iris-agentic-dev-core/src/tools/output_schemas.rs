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
