use crate::elicitation::ElicitationStore;
use crate::iris::connection::IrisConnection;
use rmcp::{
    handler::server::router::tool::ToolRouter, handler::server::wrapper::Parameters, model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::Arc;
pub mod admin;
pub mod doc;
pub mod info;
pub mod interop;
pub mod log_store;
pub mod scm;
pub mod search;
pub mod skills_tools;
pub mod symbols_local;

pub use doc::{DocMode, IrisDocParams};
pub use scm::ScmParams;

/// Controls which tools are registered at startup.
/// Read from `IRIS_TOOLSET` env var or `--toolset` CLI flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toolset {
    /// All 34 tools — current behavior (default when IRIS_TOOLSET unset).
    Baseline,
    /// 29 tools — stub tools/actions removed; no merged tools.
    Nostub,
    /// 23 tools — stubs removed + 4 merger groups consolidated.
    Merged,
}

impl Toolset {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "nostub" => Toolset::Nostub,
            "merged" => Toolset::Merged,
            _ => Toolset::Baseline,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Toolset::Baseline => "baseline",
            Toolset::Nostub => "nostub",
            Toolset::Merged => "merged",
        }
    }
}

pub const ERR_NO_TESTS_FOUND: &str = "NO_TESTS_FOUND";
pub const ERR_NAMESPACE_NOT_FOUND: &str = "NAMESPACE_NOT_FOUND";
pub const ERR_TEST_EXECUTION_ERROR: &str = "TEST_EXECUTION_ERROR";

/// A single tool call entry for the session history ring buffer.
#[derive(Debug, Clone)]
pub struct ToolCallEntry {
    pub tool: String,
    pub success: bool,
    pub timestamp: std::time::Instant,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompileParams {
    pub target: String,
    #[serde(default = "default_flags")]
    pub flags: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default)]
    pub force_writable: bool,
    /// If true, bypass the log store and return all errors/warnings inline regardless of count.
    #[serde(default)]
    pub inline: bool,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TestParams {
    pub pattern: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default = "default_test_timeout")]
    pub timeout: u64,
}

fn default_test_timeout() -> u64 {
    60
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolsParams {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct IntrospectParams {
    pub class_name: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DebugMapParams {
    #[serde(default)]
    pub routine: String,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub error_string: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateClassParams {
    pub description: String,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateTestParams {
    pub class_name: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SkillNameParams {
    pub name: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SkillSearchParams {
    pub query: String,
    #[serde(default = "default_limit")]
    pub top_k: usize,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct KbIndexParams {
    pub workspace_path: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct KbRecallParams {
    pub query: String,
    #[serde(default = "default_limit")]
    pub top_k: usize,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AgentHistoryParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolsLocalParams {
    pub query: String,
    pub workspace_path: Option<String>,
    #[serde(default = "default_symbols_local_limit")]
    pub limit: usize,
}
fn default_symbols_local_limit() -> usize {
    50
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CapturePacketParams {
    #[serde(default = "default_namespace")]
    pub namespace: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ErrorLogsParams {
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    /// If true, bypass the log store and return all entries inline regardless of count.
    #[serde(default)]
    pub inline: bool,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommunityPkgParams {
    pub name: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoParams {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetLogParams {
    /// UUID of a stored log entry. If omitted, lists all stored entries.
    pub id: Option<String>,
    /// Max entries to return from the stored result. Must be > 0 if provided.
    pub limit: Option<usize>,
    /// Start index into the stored result. Default 0.
    #[serde(default)]
    pub offset: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SourceMapParams {
    pub cls_text: String,
    pub cls_name: String,
    pub workspace_path: Option<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecuteParams {
    pub code: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default = "default_execute_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub confirmed: bool,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryParams {
    pub query: String,
    /// Query parameters as strings (e.g. ["Alice", "42"])
    #[serde(default)]
    pub parameters: Vec<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListContainersParams {
    pub workspace_root: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SelectContainerParams {
    pub name: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default = "default_username")]
    pub username: String,
    #[serde(default = "default_password")]
    pub password: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartSandboxParams {
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_edition")]
    pub edition: String,
}

fn default_flags() -> String {
    "cuk".to_string()
}
fn default_namespace() -> String {
    "USER".to_string()
}
fn default_limit() -> usize {
    20
}
fn default_max_entries() -> usize {
    50
}
fn default_execute_timeout() -> u64 {
    30
}
fn default_username() -> String {
    "_SYSTEM".to_string()
}
fn default_password() -> String {
    "SYS".to_string()
}
fn default_edition() -> String {
    "community".to_string()
}

// ── iris_test SQL result types ────────────────────────────────────────────────

/// One row from %UnitTest.Result.TestSuite.
#[derive(Debug, Clone)]
pub struct SuiteRow {
    pub id: String,
    pub name: String,
    pub status: i64,
    pub duration_ms: Option<f64>,
}

/// One row from %UnitTest.Result.TestMethod.
#[derive(Debug, Clone)]
pub struct MethodRow {
    pub suite_id: String,
    pub name: String,
    pub class_name: String,
    pub status: i64,
    pub duration_ms: Option<f64>,
    pub error_description: String,
    pub error_action: String,
}

/// Maps IRIS %UnitTest status integer to a status string.
/// Status=1 → "passed", Status=0 → "failed", other with ErrorAction → "error", other → "failed".
pub fn map_status_int(status: i64, error_action: &str) -> &'static str {
    match status {
        1 => "passed",
        0 => "failed",
        _ => {
            if !error_action.is_empty() {
                "error"
            } else {
                "failed"
            }
        }
    }
}

/// Build the compact (inline) TestRun JSON from SQL rows.
/// When empty rows are provided, returns a NO_TESTS_FOUND response.
pub fn build_test_run_from_sql(suites: &[SuiteRow], methods: &[MethodRow]) -> serde_json::Value {
    if suites.is_empty() {
        return serde_json::json!({
            "success": false,
            "error_code": ERR_NO_TESTS_FOUND,
            "error": "Pattern matched no test classes",
            "total": 0,
            "passed": 0,
            "failed": 0,
            "errors": 0,
            "skipped": 0,
        });
    }

    let mut total = 0u64;
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut errors = 0u64;
    let skipped = 0u64;
    let mut duration_ms_total = 0.0f64;

    let mut suite_jsons = Vec::new();
    for suite in suites {
        let suite_methods: Vec<&MethodRow> =
            methods.iter().filter(|m| m.suite_id == suite.id).collect();
        let s_tests = suite_methods.len() as u64;
        let s_failures = suite_methods
            .iter()
            .filter(|m| map_status_int(m.status, &m.error_action) == "failed")
            .count() as u64;
        let s_errors = suite_methods
            .iter()
            .filter(|m| map_status_int(m.status, &m.error_action) == "error")
            .count() as u64;
        let s_dur = suite.duration_ms.unwrap_or(0.0);

        total += s_tests;
        passed += suite_methods
            .iter()
            .filter(|m| map_status_int(m.status, &m.error_action) == "passed")
            .count() as u64;
        failed += s_failures;
        errors += s_errors;
        duration_ms_total += s_dur;

        suite_jsons.push(serde_json::json!({
            "name": suite.name,
            "tests": s_tests,
            "failures": s_failures,
            "errors": s_errors,
            "duration_ms": s_dur,
        }));
    }

    let success = failed == 0 && errors == 0;
    serde_json::json!({
        "success": success,
        "total": total,
        "passed": passed,
        "failed": failed,
        "errors": errors,
        "skipped": skipped,
        "duration_ms": duration_ms_total,
        "test_suites": suite_jsons,
    })
}

/// Build the full per-case TestRun JSON for log store storage.
pub fn build_test_detail(suites: &[SuiteRow], methods: &[MethodRow]) -> serde_json::Value {
    let mut suite_jsons = Vec::new();
    for suite in suites {
        let suite_methods: Vec<&MethodRow> =
            methods.iter().filter(|m| m.suite_id == suite.id).collect();
        let cases: Vec<serde_json::Value> = suite_methods
            .iter()
            .map(|m| {
                let status = map_status_int(m.status, &m.error_action);
                let failure_message = if !m.error_description.is_empty() {
                    serde_json::Value::String(m.error_description.clone())
                } else {
                    serde_json::Value::Null
                };
                serde_json::json!({
                    "name": m.name,
                    "class_name": m.class_name,
                    "status": status,
                    "duration_ms": m.duration_ms,
                    "failure_message": failure_message,
                })
            })
            .collect();
        suite_jsons.push(serde_json::json!({
            "name": suite.name,
            "tests": cases.len(),
            "failures": cases.iter().filter(|c| c["status"] == "failed").count(),
            "errors": cases.iter().filter(|c| c["status"] == "error").count(),
            "duration_ms": suite.duration_ms,
            "test_cases": cases,
        }));
    }
    serde_json::json!({"test_suites": suite_jsons})
}

fn iris_unreachable() -> McpError {
    McpError::invalid_request("IRIS_UNREACHABLE: no IRIS connection. Set IRIS_HOST and IRIS_WEB_PORT env vars, or ensure IRIS is reachable on a discoverable port (52773, 41773, 51773, 8080).", None)
}
fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}
fn err_json(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    ok_json(serde_json::json!({"success": false, "error_code": code, "error": msg}))
}
pub fn write_open_hint(namespace: &str, document: &str) {
    if let Some(home) = dirs::home_dir() {
        let dir = home.join(".iris-dev");
        let _ = std::fs::create_dir_all(&dir);
        let uri = format!("isfs://{}/{}", namespace, document);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let json = serde_json::json!({"uri": uri, "ts": ts});
        let _ = std::fs::write(dir.join("open-hint.json"), json.to_string());
    }
}

fn err_json_with_url(
    code: &str,
    msg: &str,
    attempted_url: &str,
) -> Result<CallToolResult, McpError> {
    ok_json(serde_json::json!({
        "success": false,
        "error_code": code,
        "error": msg,
        "attempted_url": attempted_url,
        "hint": "Check IRIS_HOST and IRIS_WEB_PORT (and IRIS_WEB_PREFIX if using a non-root gateway)"
    }))
}
// Bug 20: delegate to the canonical implementation in iris::discovery instead of duplicating.
fn score_container(name: &str, workspace_basename: &str) -> i64 {
    crate::iris::discovery::score_container_name(name, workspace_basename) as i64
}

fn extract_port(ports: &str, container_port: &str) -> Option<u16> {
    let pat = format!("(\\d+)->{}", regex::escape(container_port));
    regex::Regex::new(&pat)
        .ok()?
        .captures(ports)
        .and_then(|c| c[1].parse().ok())
}

async fn list_iris_containers(workspace_basename: &str) -> Vec<serde_json::Value> {
    let mut containers: Vec<serde_json::Value> = Vec::new();

    if let Ok(out) = tokio::process::Command::new("idt")
        .args(["container", "list", "--format", "json"])
        .output()
        .await
    {
        if out.status.success() {
            if let Ok(items) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) {
                for item in items {
                    let name = item["name"].as_str().unwrap_or("").to_string();
                    let ports = item["ports"].as_str().unwrap_or("");
                    let sp = extract_port(ports, "1972")
                        .map(|p| serde_json::json!(p))
                        .unwrap_or(serde_json::Value::Null);
                    let wp = extract_port(ports, "52773")
                        .map(|p| serde_json::json!(p))
                        .unwrap_or(serde_json::Value::Null);
                    let score = score_container(&name, workspace_basename);
                    containers.push(serde_json::json!({
                        "name": name, "port_superserver": sp, "port_web": wp,
                        "image": item["image"], "status": item.get("status").unwrap_or(&serde_json::json!("running")),
                        "age": item.get("age").unwrap_or(&serde_json::json!("")), "score": score,
                    }));
                }
                return sort_containers(containers);
            }
        }
    }

    if let Ok(out) = tokio::process::Command::new("docker")
        .args([
            "ps",
            "--format",
            "{{.Names}}\t{{.Image}}\t{{.Ports}}\t{{.Status}}\t{{.RunningFor}}",
        ])
        .output()
        .await
    {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let parts: Vec<&str> = line.splitn(5, '\t').collect();
                if parts.len() < 5 {
                    continue;
                }
                let (name, image, ports_raw, age) = (parts[0], parts[1], parts[2], parts[4]);
                if !image.to_lowercase().contains("intersystems")
                    && !image.to_lowercase().contains("iris")
                {
                    continue;
                }
                let sp = extract_port(ports_raw, "1972")
                    .map(|p| serde_json::json!(p))
                    .unwrap_or(serde_json::Value::Null);
                let wp = extract_port(ports_raw, "52773")
                    .map(|p| serde_json::json!(p))
                    .unwrap_or(serde_json::Value::Null);
                let score = score_container(name, workspace_basename);
                containers.push(serde_json::json!({
                    "name": name, "port_superserver": sp, "port_web": wp,
                    "image": image, "status": "running", "age": age, "score": score,
                }));
            }
        }
    }
    sort_containers(containers)
}

fn sort_containers(mut v: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    v.sort_by(|a, b| {
        let sa = a["score"].as_i64().unwrap_or(0);
        let sb = b["score"].as_i64().unwrap_or(0);
        sb.cmp(&sa).then_with(|| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        })
    });
    v
}

/// Public accessor for list_iris_containers used by iris-dev init.
pub async fn list_iris_containers_pub(workspace_basename: &str) -> Vec<serde_json::Value> {
    list_iris_containers(workspace_basename).await
}

/// Translate an iris_symbols query string into a SQL fragment and parameters.
/// Supports: plain substring, `Pkg.*` prefix, `Pkg.` trailing dot, mid-glob `Pkg.*.Name`, bare `*`.
pub fn translate_symbols_query(limit: usize, query: &str) -> (String, Vec<serde_json::Value>) {
    let base = format!("SELECT TOP {} Name FROM %Dictionary.ClassDefinition", limit);
    if query == "*" || query.is_empty() {
        return (format!("{} ORDER BY Name", base), vec![]);
    }
    if let Some(prefix) = query.strip_suffix(".*") {
        return (
            format!("{} WHERE Name %STARTSWITH ? ORDER BY Name", base),
            vec![serde_json::Value::String(format!("{}.", prefix))],
        );
    }
    if query.ends_with('.') {
        return (
            format!("{} WHERE Name %STARTSWITH ? ORDER BY Name", base),
            vec![serde_json::Value::String(query.to_string())],
        );
    }
    if query.contains('*') {
        return (
            format!("{} WHERE Name LIKE ? ORDER BY Name", base),
            vec![serde_json::Value::String(query.replace('*', "%"))],
        );
    }
    (
        format!("{} WHERE Name LIKE ? ORDER BY Name", base),
        vec![serde_json::Value::String(format!("%{}%", query))],
    )
}

#[derive(Clone)]
pub struct IrisTools {
    pub iris: Option<Arc<IrisConnection>>,
    pub registry: Arc<crate::skills::SkillRegistry>,
    /// Shared HTTP client — created once, reused across all tool calls.
    pub client: Arc<reqwest::Client>,
    /// Ring buffer of recent tool calls for skill_propose pattern mining.
    pub history: Arc<std::sync::Mutex<VecDeque<ToolCallEntry>>>,
    /// Pending elicitation state for SCM dialogs.
    pub elicitation_store: Arc<ElicitationStore>,
    /// UUID-keyed in-memory log store for progressive disclosure (027).
    pub log_store: Arc<std::sync::Mutex<log_store::LogStore>>,
    /// Active toolset — controls which tools are registered.
    pub toolset: Toolset,
    /// Whether write-capable interop/credential/lookup tools are available.
    /// Set at construction time from IrisConnection.is_write_allowed() (issue #26).
    pub write_tools_enabled: bool,
    tool_router: ToolRouter<IrisTools>,
}

#[tool_router]
impl IrisTools {
    pub fn new(iris: Option<IrisConnection>) -> anyhow::Result<Self> {
        let client = Arc::new(IrisConnection::http_client()?);
        let write_tools_enabled = iris.as_ref().map(|c| c.is_write_allowed()).unwrap_or(true);
        let log_max = std::env::var("IRIS_LOG_STORE_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50usize);
        let log_ttl = std::env::var("IRIS_LOG_TTL_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60u64);
        Ok(Self {
            iris: iris.map(Arc::new),
            registry: Arc::new(crate::skills::SkillRegistry::new()),
            client,
            history: Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(50))),
            elicitation_store: Arc::new(ElicitationStore::new()),
            log_store: Arc::new(std::sync::Mutex::new(log_store::LogStore::new(
                log_max, log_ttl,
            ))),
            toolset: Toolset::Baseline,
            write_tools_enabled,
            tool_router: Self::tool_router(),
        })
    }
    /// Convenience constructor for tests — same as `new` but with explicit toolset.
    pub fn new_with_toolset(
        iris: Option<IrisConnection>,
        toolset: Toolset,
    ) -> anyhow::Result<Self> {
        Self::with_registry_and_toolset(iris, crate::skills::SkillRegistry::new(), toolset)
    }

    /// Returns the set of tool names registered for the current toolset.
    /// Used by tests and by the benchmark harness to build valid_tool_names.
    pub fn registered_tool_names(&self) -> std::collections::HashSet<String> {
        // Authoritative baseline list — 34 tools matching v0.4.x (audit 2026-04-28).
        // REST(14) + Docker(16) + Local(4) = 34
        // 34 - stubs(4) = nostub(30); 30 - merged_removed(10) + merged_added(4) = merged(24)
        // Note: iris_symbols_local is no longer a stub (025-symbols-local-ts)
        let all_tools: &[&str] = &[
            // REST — 14
            "iris_compile",
            "iris_execute",
            "iris_doc",
            "iris_query",
            "iris_symbols",
            "iris_symbols_local",
            "docs_introspect",
            "iris_search",
            "iris_info",
            "iris_macro",
            "debug_capture_packet",
            "debug_get_error_logs",
            "iris_generate",
            "iris_generate_class",
            // Docker exec — 16
            "iris_test",
            "debug_map_int_to_cls",
            "debug_source_map",
            "iris_source_control",
            "interop_production_status",
            "interop_production_start",
            "interop_production_stop",
            "interop_production_update",
            "interop_production_needs_update",
            "interop_production_recover",
            "interop_logs",
            "interop_queues",
            "interop_message_search",
            "skill",
            "skill_propose",
            "skill_optimize",
            // Local/CLI — 4
            "skill_share",
            "skill_community",
            "skill_community_install",
            "kb",
            // 024-interop-depth (merged only)
            "iris_production_item",
            "iris_credential_list",
            "iris_credential_manage",
            "iris_lookup_manage",
            "iris_lookup_transfer",
            // 026-admin-tools
            "iris_admin",
        ];

        // Tools removed in nostub — 4 stubs returning NOT_IMPLEMENTED
        // iris_symbols_local is NO LONGER a stub (025-symbols-local-ts)
        let stub_tools: &[&str] = &[
            "skill_propose",
            "skill_optimize",
            "skill_share",
            "skill_community_install",
        ];

        // Tools removed in merged (on top of stubs) — 10 removed, 4 added → 29-10+4=23
        let merged_removed: &[&str] = &[
            "debug_capture_packet",
            "debug_get_error_logs",
            "debug_map_int_to_cls",
            "debug_source_map",
            "interop_production_status",
            "interop_production_start",
            "interop_production_stop",
            "interop_production_update",
            "interop_production_needs_update",
            "interop_production_recover",
        ];
        let merged_removed_2: &[&str] = &[] as &[&str]; // placeholder, counts handled above
        let merged_added: &[&str] = &[
            "iris_debug",
            "iris_production",
            "iris_interop_query",
            "iris_containers",
            // 024-interop-depth
            "iris_production_item",
            "iris_credential_list",
            "iris_credential_manage",
            "iris_lookup_manage",
            "iris_lookup_transfer",
            // 026-admin-tools
            "iris_admin",
            // 027-progressive-disclosure
            "iris_get_log",
        ];

        let mut names: std::collections::HashSet<String> =
            all_tools.iter().map(|s| s.to_string()).collect();

        match self.toolset {
            Toolset::Baseline => {}
            Toolset::Nostub => {
                for s in stub_tools {
                    names.remove(*s);
                }
            }
            Toolset::Merged => {
                for s in stub_tools {
                    names.remove(*s);
                }
                for s in merged_removed {
                    names.remove(*s);
                }
                let _ = merged_removed_2; // unused in this path
                for s in merged_added {
                    names.insert(s.to_string());
                }
                // Apply write-gate: remove write-only tools if not write-allowed
                if !self.write_tools_enabled {
                    let write_gated: &[&str] = &["iris_production_item", "iris_credential_manage"];
                    for s in write_gated {
                        names.remove(*s);
                    }
                }
            }
        }
        names
    }

    pub fn with_registry(
        iris: Option<IrisConnection>,
        registry: crate::skills::SkillRegistry,
    ) -> anyhow::Result<Self> {
        Self::with_registry_and_toolset(iris, registry, Toolset::Baseline)
    }
    pub fn with_registry_and_toolset(
        iris: Option<IrisConnection>,
        registry: crate::skills::SkillRegistry,
        toolset: Toolset,
    ) -> anyhow::Result<Self> {
        let client = Arc::new(IrisConnection::http_client()?);
        let mut router = Self::tool_router();

        // Remove tools from MCP tool list based on toolset (T017–T019, T033, FR-004–011).
        // The `#[tool_router]` macro registers all tools; we prune at construction time.
        let stubs_to_remove: &[&str] = match toolset {
            Toolset::Baseline => &[],
            // iris_symbols_local is NO LONGER a stub (025-symbols-local-ts)
            Toolset::Nostub | Toolset::Merged => &[
                "skill_propose",           // FR-005
                "skill_optimize",          // FR-005
                "skill_share",             // FR-005
                "skill_community_install", // FR-006
            ],
        };
        for name in stubs_to_remove {
            router.remove_route(name);
        }

        // For merged toolset: remove original tools replaced by merged dispatchers (T033).
        if toolset == Toolset::Merged {
            let merged_replaced: &[&str] = &[
                // Replaced by iris_debug (FR-007)
                "debug_capture_packet",
                "debug_get_error_logs",
                "debug_map_int_to_cls",
                "debug_source_map",
                // Replaced by iris_production (FR-008)
                "interop_production_status",
                "interop_production_start",
                "interop_production_stop",
                "interop_production_update",
                "interop_production_needs_update",
                "interop_production_recover",
                // agent_info removed (FR-011)
                "agent_info",
            ];
            for name in merged_replaced {
                router.remove_route(name);
            }
            // Note: iris_interop_query and iris_containers are added via #[tool_router];
            // interop_logs/queues/message_search and list/select/start_sandbox remain in
            // the router for baseline/nostub but are removed here for merged.
            let interop_query_replaced: &[&str] = &[
                "interop_logs",
                "interop_queues",
                "interop_message_search",
                "iris_list_containers",
                "iris_select_container",
                "iris_start_sandbox",
            ];
            for name in interop_query_replaced {
                router.remove_route(name);
            }
        } else {
            // For baseline and nostub: remove the merged dispatcher tools
            // (they're registered by #[tool_router] but shouldn't appear in these toolsets)
            let merged_tools: &[&str] = &[
                "iris_debug",
                "iris_production",
                "iris_interop_query",
                "iris_containers",
                // 024-interop-depth
                "iris_production_item",
                "iris_credential_list",
                "iris_credential_manage",
                "iris_lookup_manage",
                "iris_lookup_transfer",
                // 026-admin-tools
                "iris_admin",
                // 027-progressive-disclosure
                "iris_get_log",
            ];
            for name in merged_tools {
                router.remove_route(name);
            }
        }

        let write_tools_enabled = iris.as_ref().map(|c| c.is_write_allowed()).unwrap_or(true);

        // Remove write-capable interop tools when not write-allowed (issue #26 env guard).
        // These tools are always registered by #[tool_router]; we prune at construction time.
        if !write_tools_enabled && toolset == Toolset::Merged {
            let write_gated: &[&str] = &["iris_production_item", "iris_credential_manage"];
            for name in write_gated {
                router.remove_route(name);
            }
        }

        // Log connection status and write-tool availability.
        if let Some(ref c) = iris {
            tracing::info!(
                system_mode = ?c.system_mode,
                write_tools_enabled,
                namespace = %c.namespace,
                "iris-dev: write tool gate evaluated"
            );
        }

        let log_max = std::env::var("IRIS_LOG_STORE_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50usize);
        let log_ttl = std::env::var("IRIS_LOG_TTL_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60u64);

        Ok(Self {
            iris: iris.map(Arc::new),
            registry: Arc::new(registry),
            client,
            history: Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(50))),
            elicitation_store: Arc::new(ElicitationStore::new()),
            log_store: Arc::new(std::sync::Mutex::new(log_store::LogStore::new(
                log_max, log_ttl,
            ))),
            toolset,
            write_tools_enabled,
            tool_router: router,
        })
    }
    fn get_iris(&self) -> Result<&IrisConnection, McpError> {
        self.iris.as_deref().ok_or_else(iris_unreachable)
    }
    fn http_client(&self) -> &reqwest::Client {
        &self.client
    }
    fn record_call(&self, tool: &str, success: bool) {
        if let Ok(mut h) = self.history.lock() {
            if h.len() == 50 {
                h.pop_front();
            }
            h.push_back(ToolCallEntry {
                tool: tool.to_string(),
                success,
                timestamp: std::time::Instant::now(),
            });
        }
    }

    #[tool(
        description = "Compile an ObjectScript class, routine, or wildcard package on IRIS via Atelier REST. Supports 'MyApp.*.cls' for package-level compilation. Returns structured errors with line numbers, columns, and severity. No Python required."
    )]
    async fn iris_compile(
        &self,
        Parameters(p): Parameters<CompileParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        tracing::info!(namespace = %p.namespace, target = %p.target, "iris_compile");
        let client = self.http_client();

        // Local file path support: if target looks like a file path (contains / or \,
        // or ends with .cls/.mac/.inc and exists on disk), upload via Atelier PUT first.
        let is_local_path = p.target.contains('/')
            || p.target.contains('\\')
            || (p.target.ends_with(".cls") && std::path::Path::new(&p.target).exists());
        if is_local_path {
            let path = std::path::Path::new(&p.target);
            if path.exists() {
                let content = match std::fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(e) => {
                        return err_json(
                            "READ_ERROR",
                            &format!("Could not read {}: {}", p.target, e),
                        )
                    }
                };
                // Derive document name from Class declaration or from file name
                let doc_name = content
                    .lines()
                    .find(|l| l.trim_start().to_lowercase().starts_with("class "))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .map(|cls| format!("{}.cls", cls))
                    .unwrap_or_else(|| {
                        path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown.cls")
                            .to_string()
                    });
                // Upload via Atelier PUT
                let put_url = iris.versioned_ns_url(
                    &p.namespace,
                    &format!("/doc/{}?ignoreConflict=1", urlencoding::encode(&doc_name)),
                );
                let lines: Vec<&str> = content.lines().collect();
                let put_resp = client
                    .put(&put_url)
                    .basic_auth(&iris.username, Some(&iris.password))
                    .json(&serde_json::json!({"enc": false, "content": lines}))
                    .send()
                    .await
                    .map_err(|e| McpError::internal_error(format!("Upload failed: {e}"), None))?;
                if !put_resp.status().is_success() {
                    return err_json(
                        "UPLOAD_FAILED",
                        &format!("PUT {} returned HTTP {}", doc_name, put_resp.status()),
                    );
                }
                // Compile the uploaded document
                let local_src = p.target.clone();
                let compile_url = iris.versioned_ns_url(
                    &p.namespace,
                    &format!("/action/compile?flags={}", urlencoding::encode(&p.flags)),
                );
                let resp = client
                    .post(&compile_url)
                    .basic_auth(&iris.username, Some(&iris.password))
                    .json(&serde_json::json!([doc_name]))
                    .send()
                    .await
                    .map_err(|e| McpError::internal_error(format!("HTTP error: {e}"), None))?;
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                let console = body["console"].as_array().cloned().unwrap_or_default();
                let mut errors = vec![];
                if let Some(se) = body["status"]["errors"].as_array() {
                    for e in se {
                        let msg = e["error"].as_str().unwrap_or("Compile error");
                        errors.push(serde_json::json!({"severity":"error","code":"","line":0,"column":0,"text":msg}));
                    }
                }
                for line in &console {
                    let text = line.as_str().unwrap_or("");
                    if let Some(rest) = text.trim().strip_prefix("ERROR ") {
                        if errors.iter().all(|e| {
                            e["text"]
                                .as_str()
                                .map(|t| !t.contains(rest))
                                .unwrap_or(true)
                        }) {
                            errors.push(serde_json::json!({"severity":"error","code":"","line":0,"column":0,"text":rest}));
                        }
                    }
                }
                let success = errors.is_empty();
                self.record_call("iris_compile", success);
                return ok_json(serde_json::json!({
                    "success": success,
                    "target": doc_name,
                    "uploaded_from": local_src,
                    "targets_compiled": 1,
                    "namespace": p.namespace,
                    "errors": errors,
                    "warnings": [],
                    "console": console,
                }));
            }
        }

        // Expand wildcards: resolve "MyApp.*.cls" to a list of matching class names.
        // Bug 8: use p.namespace (not iris.namespace) and the correct /docnames/CLS endpoint.
        let targets: Vec<String> = if p.target.contains('*') {
            let list_url = iris.versioned_ns_url(&p.namespace, "/docnames/CLS");
            match client
                .get(&list_url)
                .basic_auth(&iris.username, Some(&iris.password))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    let pattern = p.target.replace('.', "\\.").replace('*', ".*");
                    let re = regex::Regex::new(&format!("(?i)^{}$", pattern))
                        .unwrap_or_else(|_| regex::Regex::new(".*").unwrap());
                    // /docnames/ returns an array of strings, not objects with a "name" key.
                    body["result"]["content"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .filter_map(|d| d.as_str())
                        .filter(|n| re.is_match(n))
                        .map(|n| n.to_string())
                        .collect()
                }
                _ => vec![p.target.clone()],
            }
        } else {
            vec![p.target.clone()]
        };

        if targets.is_empty() {
            return err_json(
                "NOT_FOUND",
                &format!("No documents match pattern: {}", p.target),
            );
        }

        // force_writable: attempt to enable namespace via docker exec if available
        if p.force_writable {
            let code = format!(
                "do ##class(%Library.EnsembleMgr).EnableNamespace(\"{}\",1)",
                p.namespace
            );
            let _ = iris.execute(&code, &p.namespace).await;
        }

        // Atelier compile: POST with JSON array of document names (with extensions)
        // e.g. ["MyApp.Patient.cls", "MyApp.Utils.cls"]
        let compile_url = iris.versioned_ns_url(
            &p.namespace,
            &format!("/action/compile?flags={}", urlencoding::encode(&p.flags)),
        );

        // Ensure targets have extensions.
        // Bug 16: the old check `t.contains('.')` skipped top-level classes (no package dot).
        // Correct check: append .cls only when no known extension is already present.
        let targets_with_ext: Vec<String> = targets
            .iter()
            .map(|t| {
                if !t.ends_with(".cls")
                    && !t.ends_with(".mac")
                    && !t.ends_with(".inc")
                    && !t.ends_with(".int")
                {
                    format!("{}.cls", t)
                } else {
                    t.clone()
                }
            })
            .collect();

        let resp = client
            .post(&compile_url)
            .basic_auth(&iris.username, Some(&iris.password))
            .json(&targets_with_ext)
            .send()
            .await
            .map_err(|e| McpError::internal_error(format!("HTTP error: {e}"), None))?;

        // Bug 17: `&& != 200` was dead code since 200 is always is_success().
        if !resp.status().is_success() {
            let url_str = compile_url.clone();
            let status = resp.status().as_u16();
            return err_json_with_url("IRIS_UNREACHABLE", &format!("HTTP {}", status), &url_str);
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| McpError::internal_error(format!("JSON parse error: {e}"), None))?;

        // Parse compiler output — console is at top level for query-param compile
        let console = body["console"]
            .as_array()
            .or_else(|| body["result"]["console"].as_array())
            .cloned()
            .unwrap_or_default();

        let mut errors = vec![];
        let mut warnings = vec![];

        // Check status.errors first — populated for parse errors (e.g. ERROR #5559) where
        // result.content/console may be empty even though the compile failed.
        if let Some(status_errors) = body["status"]["errors"].as_array() {
            for se in status_errors {
                let msg = se["error"].as_str().unwrap_or("Compile error");
                errors.push(
                    serde_json::json!({"severity":"error","code":"","line":0,"column":0,"text":msg}),
                );
            }
        }
        // Also check status.summary as a fallback — some IRIS versions put the error only there.
        if errors.is_empty() {
            let summary = body["status"]["summary"].as_str().unwrap_or("");
            if summary.contains("ERROR") {
                errors.push(serde_json::json!({"severity":"error","code":"","line":0,"column":0,"text":summary}));
            }
        }

        // Parse console output for per-line errors and warnings.
        // Atelier compile errors: "  1 ERROR #<code>:<line>: <message>"
        // Warnings: "  2 WARNING #<code>:<line>: <message>"
        for line in &console {
            let text = line.as_str().unwrap_or("");
            if let Some(rest) = text.trim().strip_prefix("ERROR ") {
                let parts: Vec<&str> = rest.splitn(3, ':').collect();
                let (code, line_num, msg) = if parts.len() >= 3 {
                    (
                        parts[0].trim(),
                        parts[1].trim().parse::<u32>().unwrap_or(0),
                        parts[2].trim(),
                    )
                } else {
                    ("", 0, rest)
                };
                // Deduplicate: skip if status.errors already has an identical message
                let already_have = errors
                    .iter()
                    .any(|e| e["text"].as_str().map(|t| t.contains(msg)).unwrap_or(false));
                if !already_have {
                    errors.push(serde_json::json!({"severity":"error","code":code,"line":line_num,"column":0,"text":msg}));
                }
            } else if let Some(rest) = text.trim().strip_prefix("WARNING ") {
                let parts: Vec<&str> = rest.splitn(3, ':').collect();
                let (code, line_num, msg) = if parts.len() >= 3 {
                    (
                        parts[0].trim(),
                        parts[1].trim().parse::<u32>().unwrap_or(0),
                        parts[2].trim(),
                    )
                } else {
                    ("", 0, rest)
                };
                warnings.push(serde_json::json!({"severity":"warning","code":code,"line":line_num,"column":0,"text":msg}));
            }
        }

        let success = errors.is_empty();
        self.record_call("iris_compile", success);

        // Write open hint for single non-wildcard successful compile
        let open_uri = if success && !p.target.contains('*') && targets.len() == 1 {
            write_open_hint(&p.namespace, &p.target);
            Some(format!("isfs://{}/{}", p.namespace, p.target))
        } else {
            None
        };

        let mut resp = serde_json::json!({
            "success": success,
            "target": p.target,
            "targets_compiled": targets.len(),
            "namespace": p.namespace,
            "errors": errors,
            "warnings": warnings,
            "console": console,
        });
        if let Some(uri) = open_uri {
            resp["open_uri"] = serde_json::Value::String(uri);
        }

        // Progressive disclosure (027): truncate errors array when count exceeds threshold.
        // Threshold counts distinct error+warning entries (not raw console lines).
        let threshold = log_store::read_inline_threshold("IRIS_INLINE_COMPILE", 20);
        let error_count = resp["errors"].as_array().map(|a| a.len()).unwrap_or(0)
            + resp["warnings"].as_array().map(|a| a.len()).unwrap_or(0);
        if error_count > threshold {
            // Combine errors+warnings into a single array for storage, truncate inline.
            // errors and warnings are truncated separately to preserve their structure.
            log_store::apply_truncation(
                &mut resp,
                "errors",
                threshold,
                p.inline,
                &self.log_store,
                "iris_compile",
            );
        } else {
            resp["truncated"] = serde_json::Value::Bool(false);
        }

        ok_json(resp)
    }

    #[tool(
        description = "Run %UnitTest.Manager tests on IRIS and return structured pass/fail results. Works without docker via pure-HTTP execution (default). If IRIS_CONTAINER is set, uses docker exec first and falls back to HTTP if docker fails. Pass a class pattern like 'MyApp.Tests' or 'MyApp.Tests.Order'. Returns suite-level summary inline plus log_id for per-test-case detail via iris_get_log."
    )]
    async fn iris_test(
        &self,
        Parameters(p): Parameters<TestParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(namespace = %p.namespace, pattern = %p.pattern, "iris_test");
        let timeout = std::time::Duration::from_secs(p.timeout);
        let container = std::env::var("IRIS_CONTAINER")
            .ok()
            .filter(|v| !v.is_empty());

        // When IRIS_CONTAINER is set, try docker exec path first.
        if container.is_some() {
            let iris = self.get_iris()?;
            let code = format!(
                "do ##class(%UnitTest.Manager).RunTest(\"{}\",\"/noload/run\")",
                p.pattern.replace('"', "\\\"")
            );
            let docker_result =
                tokio::time::timeout(timeout, iris.execute(&code, &p.namespace)).await;
            match docker_result {
                Ok(Ok(output_lines)) => {
                    // Docker path succeeded — build structured response additively.
                    let passed = output_lines
                        .lines()
                        .find(|l| l.to_lowercase().contains("passed:"))
                        .and_then(|l| {
                            l.split(':')
                                .nth(1)?
                                .split_whitespace()
                                .next()?
                                .parse::<u64>()
                                .ok()
                        })
                        .unwrap_or(0);
                    let failed = output_lines
                        .lines()
                        .find(|l| l.to_lowercase().contains("failed:"))
                        .and_then(|l| {
                            l.split(':')
                                .nth(1)?
                                .split_whitespace()
                                .next()?
                                .parse::<u64>()
                                .ok()
                        })
                        .unwrap_or(0);
                    let total = passed + failed;
                    if total == 0 {
                        self.record_call("iris_test", false);
                        return ok_json(serde_json::json!({
                            "success": false,
                            "error_code": ERR_NO_TESTS_FOUND,
                            "error": "Pattern matched no test classes",
                            "pattern": p.pattern,
                            "namespace": p.namespace,
                            "passed": 0,
                            "failed": 0,
                            "total": 0,
                            "errors": 0,
                            "skipped": 0,
                            "path": "docker",
                            "log_id": null,
                            "test_suites": [],
                        }));
                    }
                    let success = failed == 0;
                    // Store docker output in log store for drill-down.
                    let log_id = {
                        let id = log_store::new_log_id();
                        let entry = log_store::LogEntry {
                            id: id.clone(),
                            tool: "iris_test".to_string(),
                            created_at: std::time::Instant::now(),
                            preview: vec![],
                            full_result: serde_json::json!({"output": output_lines.trim()}),
                            total_count: total as usize,
                        };
                        if let Ok(mut s) = self.log_store.lock() {
                            s.store(entry);
                        }
                        id
                    };
                    self.record_call("iris_test", success);
                    return ok_json(serde_json::json!({
                        "success": success,
                        "pattern": p.pattern,
                        "namespace": p.namespace,
                        "total": total,
                        "passed": passed,
                        "failed": failed,
                        "errors": 0,
                        "skipped": 0,
                        "duration_ms": null,
                        "path": "docker",
                        "log_id": log_id,
                        "test_suites": [],
                        // Preserved for backward compatibility
                        "output": output_lines.trim(),
                    }));
                }
                // Docker failed or timed out — fall through to HTTP fallback.
                _ => {
                    tracing::info!("iris_test: docker exec failed, falling back to HTTP path");
                }
            }
        }

        // HTTP path (default when no container, or fallback when docker failed).
        let path_label = if container.is_some() {
            "http_fallback"
        } else {
            "http"
        };
        let iris = self.get_iris()?;
        let client = self.http_client();

        // US3: namespace existence check before running tests.
        let ns_check_code = format!(
            "write ##class(%SYS.Namespace).Exists(\"{}\")",
            p.namespace.replace('"', "\\\"")
        );
        let ns_exists = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            iris.execute_via_generator(&ns_check_code, "USER", client),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|s| s.trim().starts_with('1'))
        .unwrap_or(true); // If we can't check, assume it exists and let RunTest fail naturally.

        if !ns_exists {
            self.record_call("iris_test", false);
            return ok_json(serde_json::json!({
                "success": false,
                "error_code": ERR_NAMESPACE_NOT_FOUND,
                "error": format!("Namespace '{}' does not exist on this IRIS instance", p.namespace),
                "namespace": p.namespace,
            }));
        }

        // Generate a UUID correlation token; used as UserParam in RunTest.
        let correlation_token = log_store::new_log_id();
        let safe_pattern = p.pattern.replace('"', "\\\"");
        // Run tests via execute_via_generator (HTTP path).
        // Uses the existing ^UnitTestRoot (or default /tmp/httest) — test classes must
        // already be on disk in the test root directory for RunTest to discover them.
        // After RunTest completes, ^UnitTest.Result global IS persisted (globals bypass
        // the objectgenerator transaction boundary; SQL %Save() does not).
        // We write the run index out and return it to identify the run in the next step.
        let run_code = format!(
            r#"do ##class(%UnitTest.Manager).RunTest("{pattern}","/verbose=1","{token}")"#,
            token = correlation_token,
            pattern = safe_pattern,
        );

        let run_result = tokio::time::timeout(
            timeout,
            iris.execute_via_generator(&run_code, &p.namespace, client),
        )
        .await;

        let run_output = match run_result {
            Err(_) => {
                self.record_call("iris_test", false);
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "TIMEOUT",
                    "error": format!("Test run timed out after {}s", p.timeout),
                }));
            }
            Ok(Err(e)) => {
                self.record_call("iris_test", false);
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": ERR_TEST_EXECUTION_ERROR,
                    "error": e.to_string(),
                }));
            }
            Ok(Ok(out)) => out,
        };

        // Parse RunTest stdout to build structured results.
        // IRIS RunTest output format (per-method lines):
        //   "    ClassName begins ..."        ← class scope
        //   "      TestFoo() begins ..."
        //   "      TestFoo() PASSED in 0.0001s"
        //   "      TestBar() FAILED in 0.0001s"
        // ^UnitTest.Result only has suite-level data in the objectgenerator context
        // (class/method %Save() calls are inside nested transactions that don't commit).
        // Stdout parsing is reliable and provides timing data directly.
        let mut test_cases: Vec<serde_json::Value> = Vec::new();
        let mut current_class = String::new();
        let mut passed = 0u64;
        let mut failed = 0u64;
        let errors = 0u64;
        let mut class_map: std::collections::HashMap<String, Vec<serde_json::Value>> =
            std::collections::HashMap::new();

        // With /verbose=1, IRIS RunTest outputs:
        //   "    ClassName begins ..."
        //   "      TestFoo() begins ..."   ← method start
        //   "      TestFoo passed"          ← method result (no parens, no timing)
        //   "      TestFoo FAILED -- <msg>" ← method failure
        //   "    ClassName passed"
        for line in run_output.lines() {
            let trimmed = line.trim();
            // Class begin: "IrisDevE2E.SmokeTest begins ..."  (contains dot, no parens)
            if trimmed.ends_with("begins ...") && !trimmed.contains("()") && trimmed.contains('.') {
                current_class = trimmed.trim_end_matches(" begins ...").trim().to_string();
            }
            // Method result: "TestFoo passed" or "TestFoo FAILED" or "TestFoo FAILED -- msg"
            // These lines have no "()" and start with "Test"
            else if !trimmed.contains("()") && !trimmed.ends_with("begins ...") {
                let upper = trimmed.to_uppercase();
                let (is_passed, is_failed) = (
                    upper.ends_with(" PASSED") || upper.contains(" PASSED "),
                    upper.ends_with(" FAILED") || upper.contains(" FAILED"),
                );
                if !is_passed && !is_failed {
                    continue;
                }
                let method_name = if is_passed {
                    trimmed
                        .split(" passed")
                        .next()
                        .unwrap_or("")
                        .split(" PASSED")
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                } else {
                    trimmed
                        .split(" failed")
                        .next()
                        .unwrap_or("")
                        .split(" FAILED")
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                };
                if method_name.is_empty()
                    || (!method_name.starts_with("Test") && !method_name.starts_with("test"))
                {
                    continue;
                }
                let failure_message = if is_failed {
                    trimmed
                        .split_once(" -- ")
                        .map(|x| x.1)
                        .map(|s| serde_json::Value::String(s.trim().to_string()))
                        .unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                };
                if is_passed {
                    passed += 1;
                } else {
                    failed += 1;
                }
                let tc = serde_json::json!({
                    "name": method_name,
                    "class_name": current_class,
                    "status": if is_passed { "passed" } else { "failed" },
                    "duration_ms": null,
                    "failure_message": failure_message,
                });
                test_cases.push(tc.clone());
                class_map.entry(current_class.clone()).or_default().push(tc);
            }
        }

        let test_suites: Vec<serde_json::Value> = class_map
            .iter()
            .map(|(name, cases)| {
                let s_fail = cases.iter().filter(|c| c["status"] == "failed").count() as u64;
                serde_json::json!({
                    "name": name,
                    "tests": cases.len(),
                    "failures": s_fail,
                    "errors": 0,
                    "duration_ms": null,
                })
            })
            .collect();

        let total = passed + failed + errors;

        if total == 0 {
            self.record_call("iris_test", false);
            return ok_json(serde_json::json!({
                "success": false,
                "error_code": ERR_NO_TESTS_FOUND,
                "error": "Pattern matched no test classes",
                "pattern": p.pattern,
                "namespace": p.namespace,
                "total": 0,
                "passed": 0,
                "failed": 0,
                "path": path_label,
                "source": "stdout_parse",
            }));
        }

        let success = failed == 0 && errors == 0;

        // Store full per-case detail in log store.
        let log_id = {
            let id = log_store::new_log_id();
            let full = serde_json::json!({
                "test_suites": test_suites.iter().map(|s| {
                    let name = s["name"].as_str().unwrap_or("");
                    let cases: Vec<_> = test_cases.iter()
                        .filter(|c| c["class_name"].as_str() == Some(name))
                        .cloned()
                        .collect();
                    let mut suite = s.clone();
                    suite["test_cases"] = serde_json::Value::Array(cases);
                    suite
                }).collect::<Vec<_>>(),
                "raw_output": run_output.trim(),
            });
            let entry = log_store::LogEntry {
                id: id.clone(),
                tool: "iris_test".to_string(),
                created_at: std::time::Instant::now(),
                preview: vec![],
                full_result: full,
                total_count: total as usize,
            };
            if let Ok(mut s) = self.log_store.lock() {
                s.store(entry);
            }
            id
        };

        self.record_call("iris_test", success);
        ok_json(serde_json::json!({
            "success": success,
            "total": total,
            "passed": passed,
            "failed": failed,
            "errors": errors,
            "skipped": 0,
            "duration_ms": null,
            "path": path_label,
            "log_id": log_id,
            "pattern": p.pattern,
            "namespace": p.namespace,
            "test_suites": test_suites,
        }))
    }

    #[tool(
        description = "Execute arbitrary ObjectScript code on IRIS and return stdout. Uses pure-HTTP execution via CodeMode=objectgenerator (write temp class, compile, query result, delete). Falls back to docker exec if IRIS_CONTAINER env var is set and HTTP fails. Example: code='write $ZVERSION,!' returns the IRIS version string."
    )]
    async fn iris_execute(
        &self,
        Parameters(p): Parameters<ExecuteParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        tracing::info!(namespace = %p.namespace, "iris_execute");
        let client = self.http_client();
        let timeout = std::time::Duration::from_secs(p.timeout);

        // Try pure-HTTP execution first (write-compile-query via CodeMode=objectgenerator).
        let gen_result = tokio::time::timeout(
            timeout,
            iris.execute_via_generator(&p.code, &p.namespace, client),
        )
        .await;

        match gen_result {
            Err(_) => {
                self.record_call("iris_execute", false);
                return ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "TIMEOUT",
                    "error": format!("execution timed out after {}s", p.timeout),
                }));
            }
            Ok(Ok(output)) => {
                self.record_call("iris_execute", true);
                return ok_json(serde_json::json!({
                    "success": true,
                    "output": output.trim(),
                    "namespace": p.namespace,
                    "method": "http",
                }));
            }
            Ok(Err(_)) => {
                // HTTP path failed — fall through to docker exec.
            }
        }

        // Fallback: docker exec (requires IRIS_CONTAINER env var).
        let docker_result =
            tokio::time::timeout(timeout, iris.execute(&p.code, &p.namespace)).await;
        match docker_result {
            Err(_) => {
                self.record_call("iris_execute", false);
                ok_json(serde_json::json!({
                    "success": false,
                    "error_code": "TIMEOUT",
                    "error": format!("execution timed out after {}s", p.timeout),
                }))
            }
            Ok(Err(e)) => {
                let msg = e.to_string();
                self.record_call("iris_execute", false);
                if msg == "DOCKER_REQUIRED" {
                    ok_json(serde_json::json!({
                        "success": false,
                        "error_code": "DOCKER_REQUIRED",
                        "error": "iris_execute: HTTP execution failed and IRIS_CONTAINER is not set for docker exec fallback.",
                    }))
                } else {
                    ok_json(serde_json::json!({
                        "success": false,
                        "error_code": "EXECUTION_FAILED",
                        "error": msg,
                    }))
                }
            }
            Ok(Ok(output)) => {
                self.record_call("iris_execute", true);
                ok_json(serde_json::json!({
                    "success": true,
                    "output": output.trim(),
                    "namespace": p.namespace,
                    "method": "docker",
                }))
            }
        }
    }

    #[tool(
        description = "Read, write, delete, or check an IRIS document. mode='get' fetches source, mode='put' writes (with automatic SCM checkout if needed), mode='delete' removes, mode='head' checks existence. Supports batch ops via 'names' array and elicitation_id/elicitation_answer for SCM dialog resumption. No Python required."
    )]
    async fn iris_doc(
        &self,
        Parameters(p): Parameters<IrisDocParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        tracing::info!(namespace = %p.namespace, "iris_doc");
        let client = self.http_client();
        let result = doc::handle_iris_doc(iris, client, p, &self.elicitation_store).await;
        self.record_call("iris_doc", result.is_ok());
        result
    }

    #[tool(
        description = "Execute a SQL query on IRIS via Atelier REST. Returns rows as a JSON array with column names as keys. Supports SELECT, INSERT, UPDATE, DELETE. No Python required."
    )]
    async fn iris_query(
        &self,
        Parameters(p): Parameters<QueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        tracing::info!(namespace = %p.namespace, "iris_query");
        let client = self.http_client();
        let query_url = iris.versioned_ns_url(&p.namespace, "/action/query");
        let resp = client
            .post(&query_url)
            .basic_auth(&iris.username, Some(&iris.password))
            .json(&serde_json::json!({"query": p.query, "parameters": p.parameters}))
            .send()
            .await
            .map_err(|e| McpError::internal_error(format!("HTTP error: {e}"), None))?;

        if !resp.status().is_success() {
            return err_json_with_url(
                "IRIS_UNREACHABLE",
                &format!("HTTP {}", resp.status()),
                &query_url,
            );
        }

        let body: serde_json::Value = resp.json().await.unwrap_or_default();

        if let Some(errors) = body["status"]["errors"].as_array() {
            if !errors.is_empty() {
                let msg = errors[0]["error"].as_str().unwrap_or("SQL error");
                self.record_call("iris_query", false);
                return err_json("SQL_ERROR", msg);
            }
        }

        let rows = body["result"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let count = rows.len();
        self.record_call("iris_query", true);
        ok_json(
            serde_json::json!({"success": true, "rows": rows, "count": count, "namespace": p.namespace}),
        )
    }

    #[tool(
        description = "List running IRIS Docker containers with name-match scoring. Tries iris-devtester first, falls back to docker ps. Containers sorted by score (name similarity to workspace) descending."
    )]
    async fn iris_list_containers(
        &self,
        Parameters(p): Parameters<ListContainersParams>,
    ) -> Result<CallToolResult, McpError> {
        let workspace_basename = p
            .workspace_root
            .as_deref()
            .map(|r| {
                std::path::Path::new(r)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default();

        let containers = list_iris_containers(&workspace_basename).await;
        let suggestion = containers.first().map(|c: &serde_json::Value| {
            format!(
                "iris_select_container(name='{}')",
                c["name"].as_str().unwrap_or("")
            )
        });
        // FR-012: show which container .iris-dev.toml would select and if it's running.
        let workspace_config_json = {
            let ws_path = p.workspace_root.as_deref();
            match crate::iris::workspace_config::load_workspace_config(ws_path) {
                None => serde_json::Value::Null,
                Some(ref cfg) => {
                    let container_name = cfg.container.as_deref().unwrap_or("");
                    let running = !container_name.is_empty()
                        && containers
                            .iter()
                            .any(|c| c["name"].as_str() == Some(container_name));
                    let config_path = crate::iris::workspace_config::workspace_root(ws_path)
                        .join(".iris-dev.toml")
                        .to_string_lossy()
                        .to_string();
                    serde_json::json!({
                        "found": true,
                        "path": config_path,
                        "container": cfg.container,
                        "namespace": cfg.namespace,
                        "running": running,
                    })
                }
            }
        };
        // Add active_connection info so agents can detect workspace_config mismatches
        // without a separate iris_info call.
        let active_connection_json = match &self.iris {
            None => serde_json::Value::Null,
            Some(conn) => {
                // Extract container name from DiscoverySource if available.
                let container = match &conn.source {
                    crate::iris::connection::DiscoverySource::Docker { container_name } => {
                        serde_json::Value::String(container_name.clone())
                    }
                    _ => serde_json::Value::Null,
                };
                serde_json::json!({
                    "base_url": conn.base_url,
                    "namespace": conn.namespace,
                    "version": conn.version,
                    "container": container,
                })
            }
        };

        // Detect mismatch: workspace_config specifies a container but we're connected
        // to something different (or no container at all).
        let mismatch = if let (Some(cfg_container), Some(conn)) =
            (workspace_config_json["container"].as_str(), &self.iris)
        {
            match &conn.source {
                crate::iris::connection::DiscoverySource::Docker { container_name } => {
                    container_name != cfg_container
                }
                _ => true, // connected via non-Docker path but .iris-dev.toml specifies a container
            }
        } else {
            false
        };

        let mismatch_hint = if mismatch {
            let cfg_container = workspace_config_json["container"]
                .as_str()
                .unwrap_or("(unknown)");
            let active_container = active_connection_json["container"].as_str();
            let active_url = active_connection_json["base_url"]
                .as_str()
                .unwrap_or("(unknown)");
            let active = active_container.unwrap_or(active_url);
            serde_json::Value::String(format!(
                "Active connection: {}. .iris-dev.toml specifies: {}. Restart the MCP session from the workspace directory to apply.",
                active, cfg_container
            ))
        } else {
            serde_json::Value::Null
        };

        ok_json(serde_json::json!({
            "status": "ok",
            "containers": containers,
            "workspace_basename": workspace_basename,
            "suggestion": suggestion,
            "workspace_config": workspace_config_json,
            "active_connection": active_connection_json,
            "mismatch": mismatch,
            "mismatch_hint": mismatch_hint,
        }))
    }

    #[tool(
        description = "Validate and return connection parameters for the specified IRIS container. Does NOT hot-swap the active connection (restart the MCP session to switch containers). Returns probed version and port info so the caller can configure a new session."
    )]
    async fn iris_select_container(
        &self,
        Parameters(p): Parameters<SelectContainerParams>,
    ) -> Result<CallToolResult, McpError> {
        let workspace_basename = String::new();

        let containers = list_iris_containers(&workspace_basename).await;
        let found = containers
            .iter()
            .find(|c| c["name"].as_str() == Some(&p.name));

        let container = match found {
            Some(c) => c.clone(),
            None => {
                let available: Vec<_> = containers
                    .iter()
                    .filter_map(|c| c["name"].as_str())
                    .collect();
                return ok_json(serde_json::json!({
                    "error": "CONTAINER_NOT_FOUND",
                    "requested": p.name,
                    "available": available,
                }));
            }
        };

        let port_superserver = container["port_superserver"].as_u64().unwrap_or(1972) as u16;
        let port_web = container["port_web"].as_u64().unwrap_or(52773) as u16;
        let base_url = format!("http://localhost:{}", port_web);

        // Bug 5: the old code built new_conn and immediately dropped it without storing it.
        // IrisTools.iris is Arc<IrisConnection> behind &self — can't be mutated here.
        // Instead, probe the connection to verify it works and return accurate info.
        let mut new_conn = crate::iris::connection::IrisConnection::new(
            &base_url,
            &p.namespace,
            &p.username,
            &p.password,
            crate::iris::connection::DiscoverySource::Docker {
                container_name: p.name.clone(),
            },
        );
        new_conn.port_superserver = Some(port_superserver);
        new_conn.probe().await;
        let version = new_conn.version.clone();

        ok_json(serde_json::json!({
            "status": "ok",
            "container": p.name,
            "port_superserver": port_superserver,
            "port_web": port_web,
            "namespace": p.namespace,
            "version": version,
            "note": "Connection parameters validated. Restart the MCP session (set IRIS_HOST/IRIS_WEB_PORT) to switch containers.",
        }))
    }

    #[tool(
        description = "Start a dedicated IRIS container for the current project via iris-devtester CLI. Idempotent — returns existing container if already running."
    )]
    async fn iris_start_sandbox(
        &self,
        Parameters(p): Parameters<StartSandboxParams>,
    ) -> Result<CallToolResult, McpError> {
        let workspace = std::env::current_dir().unwrap_or_default();
        let workspace_basename = workspace
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();
        let container_name = if p.name.is_empty() {
            format!("{}-iris", workspace_basename)
        } else {
            p.name.clone()
        };

        let containers = list_iris_containers(&workspace_basename).await;
        if let Some(c) = containers
            .iter()
            .find(|c| c["name"].as_str() == Some(&container_name))
        {
            if c["port_superserver"].is_number() {
                return ok_json(serde_json::json!({
                    "name": container_name,
                    "port_superserver": c["port_superserver"],
                    "port_web": c["port_web"],
                    "started": false,
                    "idempotent": true,
                }));
            }
        }

        let output = tokio::process::Command::new("idt")
            .args([
                "container",
                "up",
                "--name",
                &container_name,
                "--edition",
                &p.edition,
            ])
            .output()
            .await;

        match output {
            Err(e) => err_json(
                "INTERNAL_ERROR",
                &format!("idt not found: {e}. Install with: pip install iris-devtester"),
            ),
            Ok(out) if !out.status.success() => {
                let msg = String::from_utf8_lossy(&out.stderr);
                err_json("INTERNAL_ERROR", &format!("idt container up failed: {msg}"))
            }
            Ok(_) => {
                let containers2 = list_iris_containers(&workspace_basename).await;
                match containers2
                    .iter()
                    .find(|c| c["name"].as_str() == Some(&container_name))
                {
                    Some(c) => ok_json(serde_json::json!({
                        "name": container_name,
                        "port_superserver": c["port_superserver"],
                        "port_web": c["port_web"],
                        "started": true,
                    })),
                    None => ok_json(serde_json::json!({
                        "name": container_name,
                        "started": true,
                        "warning": "Container started but not yet visible in container list.",
                    })),
                }
            }
        }
    }

    #[tool(
        description = "Search for ObjectScript classes matching a query in the IRIS namespace. Query supports: plain substring ('Patient'), package prefix ('HT.*' or 'HT.'), mid-glob ('HT.*.Service'), or bare '*' for all."
    )]
    async fn iris_symbols(
        &self,
        Parameters(p): Parameters<SymbolsParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let client = self.http_client();
        let (sql, params) = translate_symbols_query(p.limit, &p.query);
        match iris.query(&sql, params, &p.namespace, client).await {
            Ok(resp) => ok_json(serde_json::json!({
                "source": "iris_dictionary",
                "symbols": resp["result"]["content"],
                "count": resp["result"]["content"].as_array().map(|a| a.len()).unwrap_or(0),
                "query_hint": "Supports: plain text (substring), 'Pkg.*' (package prefix), 'Pkg.*.Name' (glob)",
            })),
            Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
        }
    }

    #[tool(
        description = "Search for ObjectScript symbols in local .cls/.mac/.inc files on disk — no IRIS connection required. query: glob pattern (MyApp.*, *Service, MyApp.Foo). workspace_path: optional path (defaults to OBJECTSCRIPT_WORKSPACE or cwd). limit: max symbols to return (default 50)."
    )]
    async fn iris_symbols_local(
        &self,
        Parameters(p): Parameters<SymbolsLocalParams>,
    ) -> Result<CallToolResult, McpError> {
        if p.query.trim().is_empty() {
            return err_json("INVALID_PARAMS", "query must not be empty");
        }
        let limit = p.limit.clamp(1, 500);

        // Resolve workspace path: param → OBJECTSCRIPT_WORKSPACE env → cwd
        let workspace = if let Some(ref ws) = p.workspace_path {
            std::path::PathBuf::from(ws)
        } else if let Ok(ws) = std::env::var("OBJECTSCRIPT_WORKSPACE") {
            std::path::PathBuf::from(ws)
        } else {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        };

        if !workspace.exists() {
            return err_json(
                "WORKSPACE_NOT_FOUND",
                &format!("{} does not exist", workspace.display()),
            );
        }

        let result = symbols_local::scan_workspace(&workspace, &p.query, limit);

        let symbols_json: Vec<serde_json::Value> = result
            .symbols
            .iter()
            .map(|s| serde_json::to_value(s).unwrap_or_default())
            .collect();
        let warnings_json: Vec<serde_json::Value> = result
            .parse_warnings
            .iter()
            .map(|w| serde_json::to_value(w).unwrap_or_default())
            .collect();
        let count = symbols_json.len();

        ok_json(serde_json::json!({
            "source": "local_filesystem",
            "symbols": symbols_json,
            "count": count,
            "query_hint": "Supports: plain text (exact), 'Pkg.*' (package prefix), '*Suffix' (suffix), 'Pkg.*.Name' (glob)",
            "parse_warnings": warnings_json,
        }))
    }

    #[tool(
        description = "Introspect an ObjectScript class — returns methods, properties, and type information."
    )]
    async fn docs_introspect(
        &self,
        Parameters(p): Parameters<IntrospectParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let client = self.http_client();
        // Bug 15: use parameterized queries instead of manual string escaping.
        let methods = iris.query(
            "SELECT Name,FormalSpec,ReturnType FROM %Dictionary.CompiledMethod WHERE parent=? ORDER BY Name",
            vec![serde_json::Value::String(p.class_name.clone())],
            &p.namespace,
            client,
        ).await.unwrap_or_default();
        let props = iris
            .query(
                "SELECT Name,Type FROM %Dictionary.CompiledProperty WHERE parent=? ORDER BY Name",
                vec![serde_json::Value::String(p.class_name.clone())],
                &p.namespace,
                client,
            )
            .await
            .unwrap_or_default();
        ok_json(
            serde_json::json!({"success": true, "class_name": p.class_name, "methods": methods["result"]["content"], "properties": props["result"]["content"]}),
        )
    }

    #[tool(
        description = "Map a .INT routine offset to the original .CLS source line. Pass routine+offset OR a raw IRIS error string like '<UNDEFINED>x+3^MyApp.Foo.1'."
    )]
    async fn debug_map_int_to_cls(
        &self,
        Parameters(mut p): Parameters<DebugMapParams>,
    ) -> Result<CallToolResult, McpError> {
        if !p.error_string.is_empty() {
            if let Some((r, o)) = parse_iris_error_string(&p.error_string) {
                p.routine = r;
                p.offset = o;
            }
        }
        let iris = self.get_iris()?;
        let _client = self.http_client();
        let code = format!(
            "Write ##class(%Studio.Debugger).SourceLine(\"{}\",{})",
            p.routine.replace('"', "\\\""),
            p.offset
        );
        match iris.execute(&code, &p.namespace).await {
            Ok(raw) => {
                let (cls_name, cls_line) = parse_source_line(raw.trim());
                ok_json(
                    serde_json::json!({"success": true, "mapping_available": cls_name.is_some(), "cls_name": cls_name, "cls_line": cls_line, "routine": p.routine, "offset": p.offset, "raw_error": if p.error_string.is_empty() { serde_json::Value::Null } else { p.error_string.into() }}),
                )
            }
            Err(e) if e.to_string() == "DOCKER_REQUIRED" => ok_json(serde_json::json!({
                "success": false, "error_code": "DOCKER_REQUIRED",
                "error": "debug_map_int requires docker exec. Set IRIS_CONTAINER=<container_name>.",
            })),
            Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
        }
    }

    #[tool(description = "Capture IRIS error state and recent error log entries for debugging.")]
    async fn debug_capture_packet(
        &self,
        Parameters(_p): Parameters<CapturePacketParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let client = self.http_client();
        match iris.query("SELECT TOP 20 ErrorCode,ErrorText,TimeStamp FROM %SYSTEM.Error ORDER BY TimeStamp DESC", vec![], &_p.namespace, client).await {
            Ok(resp) => ok_json(serde_json::json!({"success": true, "errors": resp["result"]["content"]})),
            Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
        }
    }

    #[tool(description = "Retrieve recent IRIS error log entries.")]
    async fn debug_get_error_logs(
        &self,
        Parameters(p): Parameters<ErrorLogsParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let client = self.http_client();
        // FR-012: cap max_entries to prevent runaway queries.
        let max_entries = p.max_entries.min(1000);
        let sql = format!("SELECT TOP {} ErrorCode,ErrorText,TimeStamp FROM %SYSTEM.Error ORDER BY TimeStamp DESC", max_entries);
        match iris.query(&sql, vec![], &p.namespace, client).await {
            Ok(resp) => {
                let mut result =
                    serde_json::json!({"success": true, "logs": resp["result"]["content"]});
                // Progressive disclosure (027): truncate logs when count exceeds threshold.
                let threshold = log_store::read_inline_threshold("IRIS_INLINE_ERROR_LOGS", 20);
                log_store::apply_truncation(
                    &mut result,
                    "logs",
                    threshold,
                    p.inline,
                    &self.log_store,
                    "debug_get_error_logs",
                );
                ok_json(result)
            }
            Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
        }
    }

    #[tool(
        description = "Build a .INT source map for a compiled ObjectScript class via Atelier xecute. Maps .INT routine line offsets back to .CLS source lines for stack trace resolution. No Python required."
    )]
    async fn debug_source_map(
        &self,
        Parameters(p): Parameters<SourceMapParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let _client = self.http_client();
        let cls_name = p.cls_name.trim_end_matches(".cls");
        // Build source map by querying %Studio.Debugger for each .INT method
        let code = format!(
            "set cls=\"{}\" set rtn=$translate(cls,\".\",\".\") set map=\"{{\" set first=1 set method=\"\" for {{ set method=$order(^rIndex(rtn,method)) quit:method=\"\"  set intline=$get(^rIndex(rtn,method)) if 'first {{ set map=map_\",\" }} set map=map_\"\\\"\"_method_\"\\\":\\\"\"_intline_\"\\\"\" set first=0 }} set map=map_\"}}\" write map",
            cls_name.replace('"', "\\\"")
        );
        // Bug 23: use p.namespace, not the hardcoded "USER".
        match iris.execute(&code, &p.namespace).await {
            Ok(output) => {
                let map: serde_json::Value =
                    serde_json::from_str(output.trim()).unwrap_or(serde_json::json!({}));
                ok_json(
                    serde_json::json!({"success": true, "cls_name": cls_name, "source_map": map}),
                )
            }
            Err(e) if e.to_string() == "DOCKER_REQUIRED" => ok_json(serde_json::json!({
                "success": false, "error_code": "DOCKER_REQUIRED",
                "error": "debug_source_map requires docker exec. Set IRIS_CONTAINER=<container_name>.",
            })),
            Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
        }
    }

    #[tool(
        description = "Generate an ObjectScript class from a natural language description. Requires IRIS_GENERATE_CLASS_MODEL + OPENAI_API_KEY env vars."
    )]
    async fn iris_generate_class(
        &self,
        Parameters(p): Parameters<GenerateClassParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::generate::{
            extract_class_name, validate_cls_syntax, LlmClient, GENERATE_CLASS_SYSTEM,
            RETRY_TEMPLATE,
        };
        let llm = LlmClient::from_env().ok_or_else(|| {
            McpError::invalid_request(
                "LLM_UNAVAILABLE: Set IRIS_GENERATE_CLASS_MODEL and OPENAI_API_KEY",
                None,
            )
        })?;

        let class_text = llm
            .complete(GENERATE_CLASS_SYSTEM, &p.description)
            .await
            .map_err(|e| McpError {
                code: rmcp::model::ErrorCode::INTERNAL_ERROR,
                message: format!("LLM_TIMEOUT: {}", e).into(),
                data: None,
            })?;

        if !validate_cls_syntax(&class_text) {
            return ok_json(
                serde_json::json!({"success": false, "error_code": "INVALID_OUTPUT", "raw_llm_output": class_text}),
            );
        }
        let class_name =
            extract_class_name(&class_text).unwrap_or_else(|| "Generated.Class".to_string());

        if let Some(iris) = self.iris.as_deref() {
            let _client = self.http_client();
            let code = format!(
                "Set sc=$SYSTEM.OBJ.Compile(\"{}\",\"ck-d\") Write $System.Status.IsOK(sc)",
                class_name
            );
            let compile_ok = iris
                .execute(&code, &p.namespace)
                .await
                .map(|o| o.trim() == "1")
                .unwrap_or(false);

            if !compile_ok {
                let retry_prompt = RETRY_TEMPLATE.replace("{errors}", "compilation failed");
                if let Ok(fixed) = llm
                    .complete(
                        GENERATE_CLASS_SYSTEM,
                        &format!(
                            "{}

Original: {}",
                            retry_prompt, class_text
                        ),
                    )
                    .await
                {
                    let fixed_name = extract_class_name(&fixed).unwrap_or(class_name.clone());
                    let code2 = format!(
                        "Set sc=$SYSTEM.OBJ.Compile(\"{}\",\"ck-d\") Write $System.Status.IsOK(sc)",
                        fixed_name
                    );
                    let ok2 = iris
                        .execute(&code2, &p.namespace)
                        .await
                        .map(|o| o.trim() == "1")
                        .unwrap_or(false);
                    return ok_json(
                        serde_json::json!({"success": true, "class_name": fixed_name, "class_text": fixed, "compiled": ok2, "retried": true}),
                    );
                }
            }
            return ok_json(
                serde_json::json!({"success": true, "class_name": class_name, "class_text": class_text, "compiled": compile_ok, "retried": false}),
            );
        }
        ok_json(
            serde_json::json!({"success": true, "class_name": class_name, "class_text": class_text, "compiled": false, "retried": false, "note": "No IRIS connection — could not compile"}),
        )
    }

    #[tool(
        description = "Generate a %UnitTest.TestCase for an existing ObjectScript class. Introspects the class first. Requires IRIS_GENERATE_CLASS_MODEL + OPENAI_API_KEY."
    )]
    async fn iris_generate_test(
        &self,
        Parameters(p): Parameters<GenerateTestParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::generate::{
            extract_class_name, validate_cls_syntax, LlmClient, GENERATE_TEST_SYSTEM,
        };
        let llm = LlmClient::from_env().ok_or_else(|| {
            McpError::invalid_request(
                "LLM_UNAVAILABLE: Set IRIS_GENERATE_CLASS_MODEL and OPENAI_API_KEY",
                None,
            )
        })?;

        let introspection_context = if let Some(iris) = self.iris.as_deref() {
            let client = self.http_client();
            // FR-001/C1: use parameterized query to prevent SQL injection via class_name.
            iris.query(
                "SELECT Name,FormalSpec,ReturnType FROM %Dictionary.CompiledMethod WHERE parent=? ORDER BY Name",
                vec![serde_json::Value::String(p.class_name.clone())],
                &p.namespace,
                client,
            )
                .await
                .map(|r| {
                    format!(
                        "Class: {}
Methods:
{}",
                        p.class_name,
                        serde_json::to_string_pretty(&r["result"]["content"]).unwrap_or_default()
                    )
                })
                .unwrap_or_else(|_| format!("Class: {} (introspection unavailable)", p.class_name))
        } else {
            format!(
                "Class: {} (no IRIS connection — generating scaffold)",
                p.class_name
            )
        };

        let prompt = format!(
            "Generate tests for the following ObjectScript class:

{}",
            introspection_context
        );
        let test_text = llm
            .complete(GENERATE_TEST_SYSTEM, &prompt)
            .await
            .map_err(|e| McpError {
                code: rmcp::model::ErrorCode::INTERNAL_ERROR,
                message: format!("LLM_TIMEOUT: {}", e).into(),
                data: None,
            })?;

        if !validate_cls_syntax(&test_text) {
            return ok_json(
                serde_json::json!({"success": false, "error_code": "INVALID_OUTPUT", "raw_llm_output": test_text}),
            );
        }
        let test_class_name =
            extract_class_name(&test_text).unwrap_or_else(|| format!("Test.{}", p.class_name));
        ok_json(
            serde_json::json!({"success": true, "class_name": p.class_name, "test_class_name": test_class_name, "test_text": test_text, "introspected": !introspection_context.contains("unavailable")}),
        )
    }

    #[tool(description = "List all synthesized skills in the registry.")]
    async fn skill_list(&self, _: Parameters<NoParams>) -> Result<CallToolResult, McpError> {
        if let Some(iris) = self.iris.as_deref() {
            let code = "Set key=\"\" Set result=\"[\" Set sep=\"\" For { Set key=$Order(^SKILLS(key)) Quit:key=\"\" Set skill=$Get(^SKILLS(key)) Set result=result_sep_skill Set sep=\",\" } Set result=result_\"]\" Write result";
            if let Ok(output) = iris
                .execute(code, &crate::tools::skills_tools::skills_namespace())
                .await
            {
                if let Ok(skills) = serde_json::from_str::<serde_json::Value>(output.trim()) {
                    let count = skills.as_array().map(|a| a.len()).unwrap_or(0);
                    return ok_json(serde_json::json!({"skills": skills, "count": count}));
                }
            }
        }
        ok_json(serde_json::json!({"skills": [], "count": 0}))
    }

    #[tool(description = "Describe a skill by name.")]
    async fn skill_describe(
        &self,
        Parameters(p): Parameters<SkillNameParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(iris) = self.iris.as_deref() {
            let code = format!("Write $Get(^SKILLS(\"{}\"))", p.name.replace('"', "\\\""));
            if let Ok(output) = iris
                .execute(&code, &crate::tools::skills_tools::skills_namespace())
                .await
            {
                if let Ok(skill) = serde_json::from_str::<serde_json::Value>(output.trim()) {
                    return ok_json(serde_json::json!({"success": true, "skill": skill}));
                }
            }
        }
        err_json("NOT_FOUND", &format!("Skill '{}' not found", p.name))
    }

    #[tool(
        description = "Search synthesized skills by name and description. Returns skills whose name or description contains the query terms."
    )]
    async fn skill_search(
        &self,
        Parameters(p): Parameters<SkillSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(iris) = self.iris.as_deref() {
            let query_lower = p.query.to_lowercase();
            let q = query_lower.replace('"', "");
            let code = format!(
                concat!(
                    r#"Set key="",results="[",sep="" "#,
                    r#"For {{ Set key=$Order(^SKILLS(key)) Quit:key="" "#,
                    r#"Set skill=$Get(^SKILLS(key)) "#,
                    r#"If ($ZConvert(skill,"L")["{0}")||($ZConvert(key,"L")["{0}") "#,
                    r#"{{ Set results=results_sep_skill Set sep="," }} }} "#,
                    r#"Set results=results_"]" Write results"#
                ),
                q
            );
            if let Ok(output) = iris
                .execute(&code, &crate::tools::skills_tools::skills_namespace())
                .await
            {
                if let Ok(skills) = serde_json::from_str::<Vec<serde_json::Value>>(output.trim()) {
                    let limited: Vec<_> = skills.into_iter().take(p.top_k).collect();
                    let count = limited.len();
                    return ok_json(
                        serde_json::json!({"query": p.query, "results": limited, "count": count}),
                    );
                }
            }
        }
        ok_json(serde_json::json!({"query": p.query, "results": [], "count": 0}))
    }

    #[tool(description = "Remove a skill from the registry by name.")]
    async fn skill_forget(
        &self,
        Parameters(p): Parameters<SkillNameParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(iris) = self.iris.as_deref() {
            let code = format!(
                "Kill ^SKILLS(\"{}\") Write \"OK\"",
                p.name.replace('"', "\\\"")
            );
            if iris
                .execute(&code, &crate::tools::skills_tools::skills_namespace())
                .await
                .is_ok()
            {
                return ok_json(serde_json::json!({"success": true, "name": p.name}));
            }
        }
        err_json(
            "DOCKER_REQUIRED",
            "skill_forget requires docker exec. Set IRIS_CONTAINER=<container_name>.",
        )
    }

    #[tool(
        description = "Trigger pattern miner to synthesize new skills from recorded tool calls."
    )]
    async fn skill_propose(&self, _: Parameters<NoParams>) -> Result<CallToolResult, McpError> {
        err_json(
            "NOT_IMPLEMENTED",
            "skill_propose: pattern mining not yet implemented",
        )
    }

    #[tool(description = "Optimize a skill using DSPy. Requires OBJECTSCRIPT_DSPY=true.")]
    async fn skill_optimize(
        &self,
        Parameters(_p): Parameters<SkillNameParams>,
    ) -> Result<CallToolResult, McpError> {
        err_json(
            "NOT_IMPLEMENTED",
            "skill_optimize: DSPy optimization not yet implemented",
        )
    }

    #[tool(description = "Share a skill to the community via GitHub PR.")]
    async fn skill_share(
        &self,
        Parameters(_p): Parameters<SkillNameParams>,
    ) -> Result<CallToolResult, McpError> {
        err_json(
            "NOT_IMPLEMENTED",
            "skill_share: GitHub PR integration not yet implemented",
        )
    }

    #[tool(
        description = "List all skills loaded from --subscribe packages. Use --subscribe owner/repo when starting iris-dev mcp to load community skills."
    )]
    async fn skill_community_list(
        &self,
        _: Parameters<NoParams>,
    ) -> Result<CallToolResult, McpError> {
        let skills: Vec<_> = self
            .registry
            .list_skills()
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "description": s.description,
                    "source": s.source_repo,
                })
            })
            .collect();
        let kb_items: Vec<_> = self
            .registry
            .list_kb_items()
            .iter()
            .map(|k| {
                serde_json::json!({
                    "title": k.title,
                    "source": k.source_repo,
                })
            })
            .collect();
        ok_json(serde_json::json!({
            "skills": skills,
            "kb_items": kb_items,
            "skill_count": skills.len(),
            "kb_count": kb_items.len(),
            "hint": "Start iris-dev mcp with --subscribe owner/repo to load community packages"
        }))
    }

    #[tool(description = "Install a community skill from the GitHub community repo.")]
    async fn skill_community_install(
        &self,
        Parameters(_p): Parameters<CommunityPkgParams>,
    ) -> Result<CallToolResult, McpError> {
        err_json(
            "NOT_IMPLEMENTED",
            "skill_community_install: community registry not yet implemented",
        )
    }

    #[tool(description = "Index markdown files into the IRIS knowledge base for semantic search.")]
    async fn kb_index(
        &self,
        Parameters(p): Parameters<KbIndexParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        skills_tools::handle_kb(
            iris,
            self.http_client(),
            skills_tools::KbParams {
                action: "index".into(),
                path: p.workspace_path,
                query: None,
                top_k: 0,
            },
        )
        .await
    }

    #[tool(
        description = "Search the knowledge base for relevant guidance. Searches subscribed KB packages and any indexed content."
    )]
    async fn kb_recall(
        &self,
        Parameters(p): Parameters<KbRecallParams>,
    ) -> Result<CallToolResult, McpError> {
        let q = p.query.to_lowercase();
        let mut results: Vec<serde_json::Value> = vec![];

        // Search subscribed KB items (BM25 substring match)
        for item in self.registry.list_kb_items() {
            let content_lower = item.content.to_lowercase();
            if content_lower.contains(&q) || item.title.to_lowercase().contains(&q) {
                // Extract a relevant snippet around the match
                let snippet = content_lower
                    .find(&q)
                    .and_then(|pos| {
                        // FR-018/Mo4: use char-boundary-safe slicing to prevent None on multibyte UTF-8.
                        let snippet_start = {
                            let mut s = pos.saturating_sub(150);
                            while s > 0 && !item.content.is_char_boundary(s) {
                                s -= 1;
                            }
                            s
                        };
                        let snippet_end = {
                            let mut e = (pos + q.len() + 300).min(item.content.len());
                            while e < item.content.len() && !item.content.is_char_boundary(e) {
                                e += 1;
                            }
                            e
                        };
                        item.content.get(snippet_start..snippet_end)
                    })
                    .map(|s| format!("...{}...", s.trim()))
                    .unwrap_or_else(|| item.content.chars().take(300).collect());
                results.push(serde_json::json!({
                    "title": item.title,
                    "snippet": snippet,
                    "source": item.source_repo,
                    "score": if item.title.to_lowercase().contains(&q) { 0.9 } else { 0.7 }
                }));
            }
        }

        // Sort by score descending, limit to top_k
        results.sort_by(|a, b| {
            b["score"]
                .as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&a["score"].as_f64().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(p.top_k);

        let count = results.len();
        ok_json(serde_json::json!({"query": p.query, "results": results, "count": count}))
    }

    #[tool(description = "Return recent tool call history for this session.")]
    async fn agent_history(
        &self,
        Parameters(p): Parameters<AgentHistoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let calls: Vec<serde_json::Value> = self
            .history
            .lock()
            .map(|h| {
                h.iter()
                    .rev()
                    .take(p.limit)
                    .map(|c| {
                        serde_json::json!({
                            "tool": c.tool,
                            "success": c.success,
                            "ago_secs": c.timestamp.elapsed().as_secs(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        ok_json(serde_json::json!({"calls": calls, "limit": p.limit}))
    }

    #[tool(description = "Return learning agent status: skill count, pattern count, KB size.")]
    async fn agent_stats(&self, _: Parameters<NoParams>) -> Result<CallToolResult, McpError> {
        let skill_count = self.registry.list_skills().len();
        let session_calls = self.history.lock().map(|h| h.len()).unwrap_or(0);
        let learning_enabled = std::env::var("OBJECTSCRIPT_LEARNING")
            .map(|v| v != "false")
            .unwrap_or(true);
        ok_json(serde_json::json!({
            "status": "ok",
            "skill_count": skill_count,
            "session_calls": session_calls,
            "learning_enabled": learning_enabled,
        }))
    }

    #[tool(
        description = "Returns the current state of the running IRIS Interoperability production. With full_status=true, includes per-component breakdown."
    )]
    async fn interop_production_status(
        &self,
        Parameters(p): Parameters<interop::ProductionStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        interop::interop_production_status_impl(self.iris.as_deref(), p).await
    }

    #[tool(description = "Start a named IRIS Interoperability production.")]
    async fn interop_production_start(
        &self,
        Parameters(p): Parameters<interop::ProductionNameParams>,
    ) -> Result<CallToolResult, McpError> {
        interop::interop_production_start_impl(self.iris.as_deref(), p).await
    }

    #[tool(
        description = "Stop the running IRIS Interoperability production with optional timeout and force."
    )]
    async fn interop_production_stop(
        &self,
        Parameters(p): Parameters<interop::ProductionStopParams>,
    ) -> Result<CallToolResult, McpError> {
        interop::interop_production_stop_impl(self.iris.as_deref(), p).await
    }

    #[tool(description = "Hot-apply configuration changes to the running production.")]
    async fn interop_production_update(
        &self,
        Parameters(p): Parameters<interop::ProductionUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        interop::interop_production_update_impl(self.iris.as_deref(), p).await
    }

    #[tool(
        description = "Check if the production configuration has changed and needs to be updated."
    )]
    async fn interop_production_needs_update(
        &self,
        Parameters(p): Parameters<interop::ProductionNeedsUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        interop::interop_production_needs_update_impl(self.iris.as_deref(), p).await
    }

    #[tool(description = "Recover a troubled IRIS Interoperability production.")]
    async fn interop_production_recover(
        &self,
        Parameters(p): Parameters<interop::ProductionRecoverParams>,
    ) -> Result<CallToolResult, McpError> {
        interop::interop_production_recover_impl(self.iris.as_deref(), p).await
    }

    #[tool(
        description = "Get recent Interoperability production log entries. Filter by log_type (comma-separated: error,warning,info,alert) and component name."
    )]
    async fn interop_logs(
        &self,
        Parameters(p): Parameters<interop::LogsParams>,
    ) -> Result<CallToolResult, McpError> {
        interop::interop_logs_impl(self.iris.as_deref(), p).await
    }

    #[tool(description = "Get all current Interoperability message queues and their depths.")]
    async fn interop_queues(&self, _: Parameters<NoParams>) -> Result<CallToolResult, McpError> {
        interop::interop_queues_impl(self.iris.as_deref()).await
    }

    #[tool(
        description = "Search the Interoperability message archive by source, target, or message class."
    )]
    async fn interop_message_search(
        &self,
        Parameters(p): Parameters<interop::MessageSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        interop::interop_message_search_impl(self.iris.as_deref(), p).await
    }

    #[tool(
        description = "Full-text search across IRIS documents via Atelier REST v2. Auto-upgrades to async polling for large namespaces. Supports regex, case sensitivity, category filter (CLS/MAC/INT/INC/ALL), and wildcard document scopes."
    )]
    async fn iris_search(
        &self,
        Parameters(p): Parameters<search::SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result =
            search::handle_iris_search(iris, self.http_client(), p, Arc::clone(&self.log_store))
                .await;
        self.record_call("iris_search", result.is_ok());
        result
    }

    #[tool(
        description = "Discover IRIS namespace contents. what=documents lists all docs, what=modified lists recently changed, what=namespace returns config, what=metadata returns IRIS version, what=jobs lists active jobs, what=csp_apps lists CSP apps, what=csp_debug returns debug ID, what=sa_schema returns SQL Analytics schema."
    )]
    async fn iris_info(
        &self,
        Parameters(p): Parameters<info::InfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result =
            info::handle_iris_info(iris, self.http_client(), p, Arc::clone(&self.log_store)).await;
        self.record_call("iris_info", result.is_ok());
        result
    }

    #[tool(
        description = "Inspect IRIS macros. action=list returns all macros, action=signature returns parameters, action=location finds definition file/line, action=definition returns text, action=expand expands with arguments."
    )]
    async fn iris_macro(
        &self,
        Parameters(p): Parameters<info::MacroParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result = info::handle_iris_macro(iris, self.http_client(), p).await;
        self.record_call("iris_macro", result.is_ok());
        result
    }

    #[tool(
        description = "IRIS debug tools. action=map_int maps a runtime error offset to source line, action=error_logs fetches recent error log entries, action=capture captures current error state, action=source_map builds .INT to .CLS mapping."
    )]
    async fn iris_debug(
        &self,
        Parameters(p): Parameters<info::DebugParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result = info::handle_iris_debug(iris, self.http_client(), p).await;
        self.record_call("iris_debug", result.is_ok());
        result
    }

    #[tool(
        description = "Prepare context for generating an ObjectScript class or %UnitTest. Returns a ready-to-use prompt plus IRIS namespace context (existing class names, method signatures). No API key needed — the calling AI agent does the generation using the returned prompt, then saves with iris_doc(mode=put) and compiles with iris_compile. gen_type=class for new classes, gen_type=test for %UnitTest scaffolding."
    )]
    async fn iris_generate(
        &self,
        Parameters(p): Parameters<info::GenerateParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result = info::handle_iris_generate(iris, self.http_client(), p).await;
        self.record_call("iris_generate", result.is_ok());
        result
    }

    #[tool(
        description = "Manage the learning agent skill registry. action=list returns all skills, action=describe returns one skill, action=search finds skills by keyword, action=forget removes a skill, action=propose mines recent tool calls and synthesizes a new skill (requires ≥5 calls)."
    )]
    async fn skill(
        &self,
        Parameters(p): Parameters<skills_tools::SkillParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result = skills_tools::handle_skill(iris, self.http_client(), p, &self.history).await;
        self.record_call("skill", result.is_ok());
        result
    }

    #[tool(
        description = "Community skill registry. action=list browses published skills from subscribed GitHub repos, action=install writes a community skill to the local ^SKILLS global."
    )]
    async fn skill_community(
        &self,
        Parameters(p): Parameters<skills_tools::SkillCommunityParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result =
            skills_tools::handle_skill_community(iris, self.http_client(), p, &self.registry).await;
        self.record_call("skill_community", result.is_ok());
        result
    }

    #[tool(
        description = "Knowledge base tools. action=index reads markdown/text files and stores them in ^KBCHUNKS, action=recall searches the KB for relevant content by keyword."
    )]
    async fn kb(
        &self,
        Parameters(p): Parameters<skills_tools::KbParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result = skills_tools::handle_kb(iris, self.http_client(), p).await;
        self.record_call("kb", result.is_ok());
        result
    }

    #[tool(
        description = "Session and learning agent information. what=stats returns skill count and session call count, what=history returns recent tool call history."
    )]
    async fn agent_info(
        &self,
        Parameters(p): Parameters<skills_tools::AgentInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result =
            skills_tools::handle_agent_info(iris, self.http_client(), p, &self.history).await;
        self.record_call("agent_info", result.is_ok());
        result
    }

    #[tool(
        description = "IRIS source control operations. action=status checks lock state and owner, action=menu lists available SCM actions, action=checkout checks out the document, action=execute runs a specific SCM action by ID. Handles elicitation for interactive SCM dialogs. Pass elicitation_id+answer to resume a pending SCM interaction."
    )]
    async fn iris_source_control(
        &self,
        Parameters(p): Parameters<ScmParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris()?;
        let result =
            scm::handle_iris_source_control(iris, self.http_client(), p, &self.elicitation_store)
                .await;
        self.record_call("iris_source_control", result.is_ok());
        result
    }

    // ── Merged tools (T029–T032, registered only when IRIS_TOOLSET=merged) ─────
    // These are always present in the #[tool_router] but removed via remove_route()
    // for Baseline and Nostub toolsets in with_registry_and_toolset().
    // Note: iris_debug already exists above as a real tool — it IS the merged debug dispatcher.

    #[tool(
        description = "Interoperability production lifecycle (merged). action: status=get current state, start=start named production, stop=stop production, update=hot-apply config, check=check if update needed, recover=recover troubled production."
    )]
    async fn iris_production(
        &self,
        Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("status");
        let iris_opt = self.iris.as_deref();
        let result = match action {
            "status" => {
                interop::interop_production_status_impl(
                    iris_opt,
                    interop::ProductionStatusParams {
                        namespace: p
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .unwrap_or("USER")
                            .to_string(),
                        full_status: p.get("full").and_then(|v| v.as_bool()).unwrap_or(false),
                    },
                )
                .await
            }
            "start" => {
                interop::interop_production_start_impl(
                    iris_opt,
                    interop::ProductionNameParams {
                        production: p
                            .get("production_name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        namespace: p
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .unwrap_or("USER")
                            .to_string(),
                    },
                )
                .await
            }
            "stop" => {
                interop::interop_production_stop_impl(
                    iris_opt,
                    interop::ProductionStopParams {
                        production: p
                            .get("production_name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        namespace: p
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .unwrap_or("USER")
                            .to_string(),
                        timeout: p.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30) as u32,
                        force: p.get("force").and_then(|v| v.as_bool()).unwrap_or(false),
                    },
                )
                .await
            }
            "update" => {
                interop::interop_production_update_impl(
                    iris_opt,
                    interop::ProductionUpdateParams {
                        namespace: p
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .unwrap_or("USER")
                            .to_string(),
                        timeout: 30,
                        force: false,
                    },
                )
                .await
            }
            "check" => {
                interop::interop_production_needs_update_impl(
                    iris_opt,
                    interop::ProductionNeedsUpdateParams {
                        namespace: p
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .unwrap_or("USER")
                            .to_string(),
                    },
                )
                .await
            }
            "recover" => {
                interop::interop_production_recover_impl(
                    iris_opt,
                    interop::ProductionRecoverParams {
                        namespace: p
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .unwrap_or("USER")
                            .to_string(),
                    },
                )
                .await
            }
            "get_autostart" => {
                interop::interop_autostart_get_impl(
                    iris_opt,
                    &interop::ProductionAutostartParams {
                        action: "get_autostart".into(),
                        namespace: p.get("namespace").and_then(|v| v.as_str()).unwrap_or("USER").to_string(),
                        enabled: None,
                        production: None,
                    },
                ).await
            }
            "set_autostart" => {
                interop::interop_autostart_set_impl(
                    iris_opt,
                    &interop::ProductionAutostartParams {
                        action: "set_autostart".into(),
                        namespace: p.get("namespace").and_then(|v| v.as_str()).unwrap_or("USER").to_string(),
                        enabled: p.get("enabled").and_then(|v| v.as_bool()),
                        production: p.get("production").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    },
                ).await
            }
            _ => err_json(
                "INVALID_ACTION",
                "iris_production: action must be status, start, stop, update, check, recover, get_autostart, or set_autostart",
            ),
        };
        self.record_call("iris_production", result.is_ok());
        result
    }

    #[tool(
        description = "Interoperability query dispatcher (merged). what: logs=recent log entries, queues=message queue depths, messages=search message archive."
    )]
    async fn iris_interop_query(
        &self,
        Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let what = p.get("what").and_then(|v| v.as_str()).unwrap_or("logs");
        let iris_opt = self.iris.as_deref();
        #[allow(unused_variables)]
        let result = match what {
            "logs" => {
                interop::interop_logs_impl(
                    iris_opt,
                    interop::LogsParams {
                        item_name: p
                            .get("component")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        log_type: p
                            .get("log_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("error,warning")
                            .to_string(),
                        limit: p.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as u32,
                    },
                )
                .await
            }
            "queues" => interop::interop_queues_impl(iris_opt).await,
            "messages" => {
                interop::interop_message_search_impl(
                    iris_opt,
                    interop::MessageSearchParams {
                        source: p
                            .get("source")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        target: p
                            .get("target")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        class_name: p
                            .get("message_class")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        limit: p.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as u32,
                    },
                )
                .await
            }
            _ => err_json(
                "INVALID_ACTION",
                "iris_interop_query: what must be logs, queues, or messages",
            ),
        };
        self.record_call("iris_interop_query", result.is_ok());
        result
    }

    #[tool(
        description = "Container lifecycle dispatcher (merged). action: list=list running IRIS containers, select=validate container connection, start=start sandbox container via iris-devtester."
    )]
    async fn iris_containers(
        &self,
        Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        let name = p
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let workspace = std::env::var("OBJECTSCRIPT_WORKSPACE").ok();
        let result = match action {
            "list" => {
                let params = ListContainersParams {
                    workspace_root: workspace,
                };
                self.iris_list_containers(Parameters(params)).await
            }
            "select" => {
                let params = SelectContainerParams {
                    name: name.unwrap_or_default(),
                    namespace: default_namespace(),
                    username: default_username(),
                    password: default_password(),
                };
                self.iris_select_container(Parameters(params)).await
            }
            "start" => {
                let params = StartSandboxParams {
                    name: name.unwrap_or_default(),
                    edition: default_edition(),
                };
                self.iris_start_sandbox(Parameters(params)).await
            }
            _ => err_json(
                "INVALID_ACTION",
                "iris_containers: action must be list, select, or start",
            ),
        };
        self.record_call("iris_containers", result.is_ok());
        result
    }

    // ─── 024-interop-depth: Production item control (US1) ───

    #[tool(
        description = "Enable, disable, or inspect/modify settings of an individual Interoperability production config item. action: enable|disable|get_settings|set_settings. item: exact config item name. namespace: optional. settings: key-value map (for set_settings). Works via HTTP, no Docker required."
    )]
    async fn iris_production_item(
        &self,
        Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let action = p
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let item = p
            .get("item")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let namespace = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("USER")
            .to_string();
        let settings: std::collections::HashMap<String, String> = p
            .get("settings")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let result = interop::interop_production_item_impl(
            self.iris.as_deref(),
            interop::ProductionItemParams {
                action,
                item,
                namespace,
                settings,
            },
        )
        .await;
        self.record_call("iris_production_item", result.is_ok());
        result
    }

    // ─── 024-interop-depth: Ensemble credentials (US2) ───

    #[tool(
        description = "List all Ensemble credentials (IDs and usernames only — passwords never returned). namespace: optional."
    )]
    async fn iris_credential_list(
        &self,
        Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let namespace = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("USER")
            .to_string();
        let result = interop::interop_credential_list_impl(
            self.iris.as_deref(),
            interop::CredentialListParams { namespace },
        )
        .await;
        self.record_call("iris_credential_list", result.is_ok());
        result
    }

    #[tool(
        description = "Create, update, or delete an Ensemble credential. action: create|update|delete. id: credential ID (required). username/password: required for create, optional for update. namespace: optional. Write-gated: suppressed on Live instances unless IRIS_ALLOW_PROD=1."
    )]
    async fn iris_credential_manage(
        &self,
        Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let result = interop::interop_credential_manage_impl(
            self.iris.as_deref(),
            interop::CredentialManageParams {
                action: p
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                id: p
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                username: p
                    .get("username")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                password: p
                    .get("password")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                namespace: p
                    .get("namespace")
                    .and_then(|v| v.as_str())
                    .unwrap_or("USER")
                    .to_string(),
            },
        )
        .await;
        self.record_call("iris_credential_manage", result.is_ok());
        result
    }

    // ─── 024-interop-depth: Lookup tables (US3) ───

    #[tool(
        description = "Read, write, delete, or list Ensemble lookup table entries. action: get|set|delete|list_keys|list_tables. table: table name (required except list_tables). key: required for get/set/delete. value: required for set. namespace: optional. get/list_keys/list_tables always available; set/delete write-gated."
    )]
    async fn iris_lookup_manage(
        &self,
        Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let result = interop::interop_lookup_manage_impl(
            self.iris.as_deref(),
            interop::LookupManageParams {
                action: p
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                table: p
                    .get("table")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                key: p.get("key").and_then(|v| v.as_str()).map(|s| s.to_string()),
                value: p
                    .get("value")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                namespace: p
                    .get("namespace")
                    .and_then(|v| v.as_str())
                    .unwrap_or("USER")
                    .to_string(),
            },
        )
        .await;
        self.record_call("iris_lookup_manage", result.is_ok());
        result
    }

    #[tool(
        description = "Export or import an Ensemble lookup table as XML. action: export|import. table: table name. xml: XML string (required for import). namespace: optional. export always available; import write-gated."
    )]
    async fn iris_lookup_transfer(
        &self,
        Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let result = interop::interop_lookup_transfer_impl(
            self.iris.as_deref(),
            interop::LookupTransferParams {
                action: p
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                table: p
                    .get("table")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                xml: p.get("xml").and_then(|v| v.as_str()).map(|s| s.to_string()),
                namespace: p
                    .get("namespace")
                    .and_then(|v| v.as_str())
                    .unwrap_or("USER")
                    .to_string(),
            },
        )
        .await;
        self.record_call("iris_lookup_transfer", result.is_ok());
        result
    }

    // ── 026-admin-tools: iris_admin dispatcher ───────────────────────────────

    #[tool(
        description = "IRIS administration dispatcher. action: list_namespaces, list_databases, list_users, list_roles, list_user_roles, check_permission, list_webapps, get_webapp (read — always available); create_user, update_user, delete_user, create_namespace, delete_namespace, create_webapp, delete_webapp (write — requires IRIS_ADMIN_TOOLS=1). All operations run in %SYS namespace. check_permission checks the currently connected user (IRIS_USERNAME), not an arbitrary user."
    )]
    async fn iris_admin(
        &self,
        Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let iris_opt = self.iris.as_deref();
        let result = match action {
            "list_namespaces" => admin::admin_list_namespaces_impl(iris_opt).await,
            "list_databases" => admin::admin_list_databases_impl(iris_opt).await,
            "list_users" => admin::admin_list_users_impl(iris_opt).await,
            "list_roles" => admin::admin_list_roles_impl(iris_opt).await,
            "list_webapps" => {
                let type_filter = p.get("type").and_then(|v| v.as_str());
                admin::admin_list_webapps_impl(iris_opt, type_filter).await
            }
            "list_user_roles" => {
                let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("");
                if username.is_empty() {
                    return err_json("INVALID_PARAMS", "username is required for list_user_roles");
                }
                admin::admin_list_user_roles_impl(iris_opt, username).await
            }
            "get_webapp" => {
                let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("");
                if path.is_empty() {
                    return err_json("INVALID_PARAMS", "path is required for get_webapp");
                }
                admin::admin_get_webapp_impl(iris_opt, path).await
            }
            "check_permission" => {
                let resource = p.get("resource").and_then(|v| v.as_str()).unwrap_or("");
                let permission = p.get("permission").and_then(|v| v.as_str()).unwrap_or("USE");
                if resource.is_empty() {
                    return err_json("INVALID_PARAMS", "resource is required for check_permission");
                }
                admin::admin_check_permission_impl(iris_opt, resource, permission).await
            }
            "create_user" => {
                let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("");
                let password = p.get("password").and_then(|v| v.as_str()).unwrap_or("");
                if username.is_empty() || password.is_empty() {
                    return err_json("INVALID_PARAMS", "username and password are required for create_user");
                }
                admin::admin_create_user_impl(
                    iris_opt, username, password,
                    p.get("full_name").and_then(|v| v.as_str()),
                    p.get("roles").and_then(|v| v.as_str()),
                ).await
            }
            "update_user" => {
                let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("");
                if username.is_empty() {
                    return err_json("INVALID_PARAMS", "username is required for update_user");
                }
                admin::admin_update_user_impl(
                    iris_opt, username,
                    p.get("password").and_then(|v| v.as_str()),
                    p.get("enabled").and_then(|v| v.as_bool()),
                    p.get("roles").and_then(|v| v.as_str()),
                ).await
            }
            "delete_user" => {
                let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("");
                if username.is_empty() {
                    return err_json("INVALID_PARAMS", "username is required for delete_user");
                }
                admin::admin_delete_user_impl(iris_opt, username).await
            }
            "create_namespace" => {
                let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let code_db = p.get("code_database").and_then(|v| v.as_str()).unwrap_or("");
                let data_db = p.get("data_database").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() || code_db.is_empty() || data_db.is_empty() {
                    return err_json("INVALID_PARAMS", "name, code_database, and data_database are required");
                }
                admin::admin_create_namespace_impl(iris_opt, name, code_db, data_db).await
            }
            "delete_namespace" => {
                let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() {
                    return err_json("INVALID_PARAMS", "name is required for delete_namespace");
                }
                admin::admin_delete_namespace_impl(iris_opt, name).await
            }
            "create_webapp" => {
                let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let ns = p.get("namespace").and_then(|v| v.as_str()).unwrap_or("");
                if path.is_empty() || ns.is_empty() {
                    return err_json("INVALID_PARAMS", "path and namespace are required for create_webapp");
                }
                admin::admin_create_webapp_impl(
                    iris_opt, path, ns,
                    p.get("dispatch_class").and_then(|v| v.as_str()),
                    p.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                ).await
            }
            "delete_webapp" => {
                let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("");
                if path.is_empty() {
                    return err_json("INVALID_PARAMS", "path is required for delete_webapp");
                }
                admin::admin_delete_webapp_impl(iris_opt, path).await
            }
            _ => err_json(
                "INVALID_ACTION",
                "iris_admin: action must be one of: list_namespaces, list_databases, list_users, list_roles, list_user_roles, check_permission, list_webapps, get_webapp, create_user, update_user, delete_user, create_namespace, delete_namespace, create_webapp, delete_webapp",
            ),
        };
        self.record_call("iris_admin", result.is_ok());
        result
    }

    // ── iris_get_log (027 — progressive disclosure, Merged tier only) ──────────

    #[tool(
        description = "Retrieve a stored result by log_id from the progressive disclosure store. With id: returns the full result (optionally paginated with limit/offset). Without id: lists all stored log entries with their IDs, tools, timestamps, and total counts. Use after any tool returns truncated:true."
    )]
    async fn iris_get_log(
        &self,
        Parameters(p): Parameters<GetLogParams>,
    ) -> Result<CallToolResult, McpError> {
        match p.id {
            None => {
                // List all non-expired entries
                let summaries = self
                    .log_store
                    .lock()
                    .map(|mut s| s.list())
                    .unwrap_or_default();
                ok_json(serde_json::json!({
                    "success": true,
                    "logs": summaries,
                }))
            }
            Some(ref id) => {
                // Validate limit
                if let Some(lim) = p.limit {
                    if lim == 0 {
                        return err_json("INVALID_PARAMS", "limit must be > 0");
                    }
                }

                // Check TTL / existence first
                let get_result = self
                    .log_store
                    .lock()
                    .map(|s| s.get(id))
                    .unwrap_or(log_store::GetResult::NotFound);

                match get_result {
                    log_store::GetResult::NotFound => err_json(
                        "LOG_NOT_FOUND",
                        &format!("No log entry found with id '{}'", id),
                    ),
                    log_store::GetResult::Expired => err_json(
                        "LOG_EXPIRED",
                        &format!("Log entry '{}' has expired (TTL exceeded)", id),
                    ),
                    log_store::GetResult::Found(_) => {
                        // Now handle pagination
                        let paginated = self
                            .log_store
                            .lock()
                            .ok()
                            .and_then(|s| s.get_paginated(id, p.limit, p.offset));

                        match paginated {
                            None => err_json(
                                "LOG_EXPIRED",
                                &format!("Log entry '{}' expired during retrieval", id),
                            ),
                            Some((result, has_more, total_count)) => {
                                if p.limit.is_some() {
                                    ok_json(serde_json::json!({
                                        "success": true,
                                        "log_id": id,
                                        "total_count": total_count,
                                        "offset": p.offset,
                                        "limit": p.limit,
                                        "has_more": has_more,
                                        "result": result,
                                    }))
                                } else {
                                    ok_json(serde_json::json!({
                                        "success": true,
                                        "log_id": id,
                                        "total_count": total_count,
                                        "result": result,
                                    }))
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for IrisTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "iris-dev".to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ))
            .with_instructions(
                "iris-dev: composable MCP tools for ObjectScript and IRIS development.".to_string(),
            )
    }
}

fn parse_iris_error_string(s: &str) -> Option<(String, i64)> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"<[A-Z]+>\s*[^+\s]+\+(\d+)\^([\w.%]+)").expect("valid regex")
    });
    let caps = re.captures(s)?;
    Some((caps[2].to_string(), caps[1].parse().ok()?))
}

fn parse_source_line(raw: &str) -> (Option<String>, Option<i64>) {
    if raw.is_empty() {
        return (None, None);
    }
    if let Some((cls, line)) = raw.split_once(':') {
        return (
            Some(cls.trim_end_matches(".cls").to_string()),
            line.trim().parse().ok(),
        );
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_port ──────────────────────────────────────────────────────────
    #[test]
    fn test_extract_port_standard() {
        assert_eq!(
            extract_port("0.0.0.0:52780->52773/tcp", "52773"),
            Some(52780)
        );
    }
    #[test]
    fn test_extract_port_superserver() {
        assert_eq!(extract_port("0.0.0.0:1974->1972/tcp", "1972"), Some(1974));
    }
    #[test]
    fn test_extract_port_not_present() {
        assert_eq!(extract_port("0.0.0.0:52780->52773/tcp", "1972"), None);
    }
    #[test]
    fn test_extract_port_multiple_mappings() {
        let ports = "0.0.0.0:1974->1972/tcp, 0.0.0.0:52775->52773/tcp";
        assert_eq!(extract_port(ports, "52773"), Some(52775));
        assert_eq!(extract_port(ports, "1972"), Some(1974));
    }
    #[test]
    fn test_extract_port_empty_string() {
        assert_eq!(extract_port("", "52773"), None);
    }

    // ── parse_iris_error_string ───────────────────────────────────────────────
    #[test]
    fn test_parse_iris_error_standard() {
        let s = "<UNDEFINED>x+3^Ens.Director.1";
        let result = parse_iris_error_string(s);
        assert_eq!(result, Some(("Ens.Director.1".to_string(), 3)));
    }
    #[test]
    fn test_parse_iris_error_divide() {
        let s = "<DIVIDE>x+1^MyApp.Foo.1";
        let result = parse_iris_error_string(s);
        assert_eq!(result, Some(("MyApp.Foo.1".to_string(), 1)));
    }
    #[test]
    fn test_parse_iris_error_no_match() {
        assert!(parse_iris_error_string("just a plain error").is_none());
        assert!(parse_iris_error_string("").is_none());
    }
    #[test]
    fn test_parse_iris_error_large_offset() {
        let s = "<ERROR>routine+99^Some.Class.INT";
        let result = parse_iris_error_string(s);
        assert_eq!(result, Some(("Some.Class.INT".to_string(), 99)));
    }

    // ── parse_source_line ─────────────────────────────────────────────────────
    #[test]
    fn test_parse_source_line_with_cls() {
        let (cls, line) = parse_source_line("MyApp.Foo.cls:42");
        assert_eq!(cls.as_deref(), Some("MyApp.Foo"));
        assert_eq!(line, Some(42));
    }
    #[test]
    fn test_parse_source_line_without_cls() {
        let (cls, line) = parse_source_line("MyApp.Foo:10");
        assert_eq!(cls.as_deref(), Some("MyApp.Foo"));
        assert_eq!(line, Some(10));
    }
    #[test]
    fn test_parse_source_line_empty() {
        let (cls, line) = parse_source_line("");
        assert!(cls.is_none());
        assert!(line.is_none());
    }
    #[test]
    fn test_parse_source_line_no_colon() {
        let (cls, line) = parse_source_line("NoColonHere");
        assert!(cls.is_none());
        assert!(line.is_none());
    }

    // ── translate_symbols_query ───────────────────────────────────────────────
    #[test]
    fn test_translate_bare_star_no_where() {
        let (sql, params) = translate_symbols_query(20, "*");
        assert!(!sql.contains("WHERE"), "bare * has no WHERE: {}", sql);
        assert!(params.is_empty());
    }
    #[test]
    fn test_translate_empty_no_where() {
        let (sql, params) = translate_symbols_query(20, "");
        assert!(!sql.contains("WHERE"), "empty has no WHERE: {}", sql);
        assert!(params.is_empty());
    }
    #[test]
    fn test_translate_glob_suffix() {
        let (sql, params) = translate_symbols_query(10, "HT.*");
        assert!(sql.contains("%STARTSWITH"));
        assert_eq!(params[0].as_str(), Some("HT."));
    }
    #[test]
    fn test_translate_trailing_dot() {
        let (sql, params) = translate_symbols_query(10, "Ens.");
        assert!(sql.contains("%STARTSWITH"));
        assert_eq!(params[0].as_str(), Some("Ens."));
    }
    #[test]
    fn test_translate_mid_glob() {
        let (sql, params) = translate_symbols_query(5, "A.*.B");
        assert!(sql.contains("LIKE"));
        let p = params[0].as_str().unwrap();
        assert_eq!(p, "A.%.B");
    }
    #[test]
    fn test_translate_plain_wraps_in_percent() {
        let (sql, params) = translate_symbols_query(20, "Patient");
        assert!(sql.contains("LIKE"));
        assert_eq!(params[0].as_str(), Some("%Patient%"));
    }
    #[test]
    fn test_translate_limit_in_sql() {
        let (sql, _) = translate_symbols_query(42, "Foo");
        assert!(sql.contains("42"), "limit must appear in SQL: {}", sql);
    }

    // ── sort_containers ───────────────────────────────────────────────────────
    #[test]
    fn test_sort_containers_by_score() {
        let containers = vec![
            serde_json::json!({"name":"z-iris","score":10}),
            serde_json::json!({"name":"a-iris","score":90}),
            serde_json::json!({"name":"m-iris","score":50}),
        ];
        let sorted = sort_containers(containers);
        assert_eq!(sorted[0]["name"].as_str(), Some("a-iris"));
        assert_eq!(sorted[1]["name"].as_str(), Some("m-iris"));
        assert_eq!(sorted[2]["name"].as_str(), Some("z-iris"));
    }
    #[test]
    fn test_sort_containers_tiebreak_by_name() {
        let containers = vec![
            serde_json::json!({"name":"z-iris","score":50}),
            serde_json::json!({"name":"a-iris","score":50}),
        ];
        let sorted = sort_containers(containers);
        assert_eq!(sorted[0]["name"].as_str(), Some("a-iris"));
    }
}
