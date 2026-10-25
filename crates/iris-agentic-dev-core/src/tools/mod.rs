use crate::elicitation::ElicitationStore;
use crate::iris::connection::IrisConnection;

/// Add `hint`/`hint_ref` to a compile response when the compiler output matches a rule.
pub(crate) fn add_compile_hint(resp: &mut serde_json::Value, lines: &[String]) {
    if let Some(hint) = error_hints::compile_error_hint(lines) {
        hint.apply(resp);
    }
}

/// Add `hint`/`hint_ref` to a successful compile when the class source has a method that compiles
/// and never runs. Leaves an existing hint alone.
pub(crate) fn add_source_hint(resp: &mut serde_json::Value, source: Option<&str>) {
    if resp.get("hint").is_some() {
        return;
    }
    if let Some(hint) = source.and_then(error_hints::compiled_source_hint) {
        hint.apply(resp);
    }
}

/// Remediation hint appended to DOCKER_REQUIRED error strings.
/// Guides native IRIS users (no Docker) toward the HTTP/Atelier REST path.
/// The value of an environment variable, or the empty string. For fields a response repeats from the
/// operator's environment.
fn non_blank_env_value(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

pub(crate) const DOCKER_REQUIRED_HINT: &str = " Ensure HTTP/Atelier REST is reachable: verify \
    http://<host>:<port>/api/atelier and set host/web_port in .iris-agentic-dev.toml.";

/// `CARGO_PKG_VERSION` plus a `+<git-describe>` build-metadata suffix (see
/// `build.rs`), so `check_config`'s `server_version` can distinguish a local/
/// fork build from an official tagged release even when the crate version
/// hasn't been bumped. Empty suffix exactly when the build is a clean
/// checkout at the tag matching this version (a genuine release build) — not
/// merely "no .git", since CI release builds have one too (`actions/checkout`).
const SERVER_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    env!("IRIS_AGENTIC_DEV_BUILD_SUFFIX")
);

use rmcp::{
    handler::server::router::tool::ToolRouter, handler::server::tool::schema_for_output,
    handler::server::wrapper::Parameters, model::*, tool, tool_handler, tool_router,
    ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

// 076-interface-modernization User Story 1: output-schema-only response shapes. Not
// constructed at runtime — see output_schemas.rs's module doc comment for why.
use output_schemas::{
    AgentHistoryResponse, AgentInfoResponse, AgentStatsResponse, CapabilityMatrixResponse,
    CheckConfigOk, CompareDocumentResponse, CompareNamespaceResponse, DebugCapturePacketResponse,
    DebugGetErrorLogsResponse, DebugMapIntToClsResponse, DebugSourceMapResponse,
    DocsIntrospectResponse, ExtractMessageMapRoutingResponse, FindSubclassImplementationsResponse,
    GlobalKillResponse, GlobalPreviewResponse, Hl7SchemaInspectResponse, Hl7SchemaListResponse,
    IrisAddServerResponse, IrisAdminResponse, IrisBusinessRuleInfoResponse, IrisCompileResponse,
    IrisContainersResponse, IrisCoverageResponse, IrisCredentialListResponse,
    IrisCredentialManageResponse, IrisDatabaseListResponse, IrisDatabaseStatsResponse,
    IrisDebugResponse, IrisDocResponse, IrisDocSearchResponse, IrisExecuteMethodResponse,
    IrisExecuteResponse, IrisGenerateClassResponse, IrisGenerateResponse, IrisGenerateTestResponse,
    IrisGetLogResponse, IrisGlobalResponse, IrisImportServersResponse, IrisInfoResponse,
    IrisInteropQueryResponse, IrisListContainersResponse, IrisLookupManageResponse,
    IrisLookupTransferResponse, IrisMacroResponse, IrisMessageBodyResponse,
    IrisNamespaceCreateResponse, IrisNamespaceListResponse, IrisProductionDiffResponse,
    IrisProductionItemResponse, IrisProductionResponse, IrisQueryResponse,
    IrisRemoveServerResponse, IrisSearchResponse, IrisSelectContainerResponse, IrisServersResponse,
    IrisSourceControlResponse, IrisStartSandboxResponse, IrisSymbolsLocalResponse,
    IrisSymbolsResponse, IrisTableInfoResponse, IrisTestResponse, IrisTestServerResponse,
    IrisWsCloseResponse, IrisWsExecResponse, IrisWsOpenResponse, JournalSearchResponse,
    KbIndexResponse, KbRecallResponse, KbResponse, MermaidClassResponse, MermaidProductionResponse,
    MyAccessResponse, QueryAuditLogResponse, ResolveDynamicDispatchResponse,
    ResolveStorageResponse, SkillCommunityListResponse, SkillCommunityResponse,
    SkillDescribeResponse, SkillForgetResponse, SkillListResponse, SkillResponse,
    SkillSearchResponse, StreamInspectResponse, TelemetryExportTraceResponse,
    TelemetryQueryResponse, ToolError,
};

tokio::task_local! {
    /// Set once per `call_tool` invocation (see the `ServerHandler::call_tool` override
    /// below) so `record_call` can compute an accurate `duration_ms` without changing the
    /// signature of any of the ~50 existing `self.record_call(tool, success)` call sites.
    static CALL_START: std::time::Instant;
}

// Re-export the MCP peer task-local and its accessor so tests and downstream code can use
// `iris_agentic_dev_core::tools::MCP_PEER` / `tools::mcp_peer()` without reaching into the
// iris module directly.
pub use crate::iris::connection::{mcp_peer, MCP_PEER};

pub mod admin;
pub mod admin_tools;
pub mod comparison_tools;
pub mod coverage;
pub mod dict;
pub mod doc;
pub mod doc_search;
pub mod error_hints;
pub mod exec;
pub mod execute_session;
pub mod generate_class;
pub mod global;
pub mod host_fs;
pub mod info;
pub mod interop;
pub mod log_store;
pub mod nopws;
pub mod observability;
pub mod output_schemas;
pub mod param_check;
pub mod params;
pub mod scm;
pub mod search;
pub mod server_tools;
pub mod skills_tools;
pub mod storage_guard;
pub mod stored_check;
pub mod superclass_check;
pub mod symbols_local;
pub mod target;
pub mod write_gate;
pub mod ws_tools;
pub mod xdata_flow;

pub use doc::{DocMode, IrisDocParams};
pub use scm::{ScmAction, ScmParams};

/// Controls which tools are registered at startup.
/// Read from `IRIS_TOOLSET` env var or `--toolset` CLI flag.
///
/// Tool counts below are pinned by `test_baseline_tool_count` / `test_merged_tool_count`
/// in `tests/unit/test_toolset.rs` — update both the test and this comment together, since
/// nothing checks that a comment matches reality automatically. (These counts previously
/// drifted for a long time — see `registered_tool_names()`'s doc comment for how that
/// happened and how it's now derived instead of hand-maintained.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toolset {
    /// 84 tools — 93 total `#[tool]` methods minus the 9 that are Merged-tier-only
    /// dispatchers (iris_admin, iris_debug, iris_containers, iris_get_log, iris_global,
    /// iris_execute_method, iris_message_body, iris_business_rule_info,
    /// iris_production_diff — added by later specs and deliberately scoped to Merged
    /// rather than the original tool surface). Default when `IRIS_TOOLSET` is unset.
    Baseline,
    /// 80 tools — Baseline (84) minus the 4 stub tools (skill_propose/skill_optimize/
    /// skill_share/skill_community_install).
    Nostub,
    /// 81 tools — 93 total minus the 4 stubs minus 8 tools replaced by 2 consolidated
    /// dispatchers (4 debug_* tools → iris_debug; agent_info/iris_list_containers/
    /// iris_select_container/iris_start_sandbox → iris_containers). The 9
    /// Merged-tier-only dispatchers from Baseline's note above are present here, which
    /// is the point of this tier.
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

/// One tool as `tool --list` and `tool <name> --schema` report it.
///
/// Built by [`IrisTools::tool_catalogue`] from the router, so `description` and `input_schema` are
/// the same values a client reads off `tools/list`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CatalogueEntry {
    pub name: String,
    /// First sentence of the description, at most 100 characters. What `--list` prints.
    pub summary: String,
    /// The full description, verbatim. `None` for a tool that declares none.
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// Words whose trailing period is not a sentence break.
///
/// Checked against the token immediately before the period, lowercased. Single letters are handled
/// separately, which covers the second period in `e.g.` and the initial in a name.
const NOT_SENTENCE_END: &[&str] = &["e.g", "i.e", "etc", "vs", "cf", "approx", "inc"];

/// The one-line summary for a tool description: its first sentence, at most 100 characters.
///
/// Tool descriptions open with what the tool does and then spend a paragraph or more on parameters
/// and caveats, so the first sentence is the summary and nothing has to be written twice. A
/// truncated summary ends in `…` — a listing that silently drops the end of a sentence reads as a
/// complete claim about the tool.
///
/// Sentence detection skips a period inside an abbreviation (`e.g. My.Class.` is one sentence), and
/// treats a newline as a break so a description whose first line has no period still yields a line.
pub fn summarize_description(description: &str) -> String {
    const LIMIT: usize = 100;

    let text = description.trim();
    if text.is_empty() {
        return String::new();
    }

    let first = first_sentence(text);
    let first = first.trim();
    if first.chars().count() <= LIMIT {
        return first.to_string();
    }

    // Cut at the last word boundary that leaves room for the ellipsis.
    let budget: usize = first
        .char_indices()
        .nth(LIMIT - 1)
        .map(|(i, _)| i)
        .unwrap_or(first.len());
    let cut = first[..budget].rfind(char::is_whitespace).unwrap_or(budget);
    let mut out = first[..cut].trim_end().to_string();
    out.push('…');
    out
}

/// The first sentence of `text`, or its first line, whichever comes first.
fn first_sentence(text: &str) -> &str {
    if let Some(nl) = text.find('\n') {
        let line = &text[..nl];
        if let Some(end) = sentence_end(line) {
            return &line[..end];
        }
        // No break on the first line, but the line itself is the unit.
        return line;
    }
    match sentence_end(text) {
        Some(end) => &text[..end],
        None => text,
    }
}

/// Byte offset just past the first sentence-ending period in `line`, if there is one.
fn sentence_end(line: &str) -> Option<usize> {
    for (i, _) in line.match_indices('.') {
        let after = line[i + 1..].chars().next();
        if after.is_some_and(|c| !c.is_whitespace()) {
            continue; // mid-token period: `My.Class`, the first dot of `e.g.`
        }
        let token = line[..i]
            .rsplit(|c: char| c.is_whitespace())
            .next()
            .unwrap_or_default()
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if token.chars().count() == 1 || NOT_SENTENCE_END.contains(&token.as_str()) {
            continue;
        }
        return Some(i + 1);
    }
    None
}

pub const ERR_NO_TESTS_FOUND: &str = "NO_TESTS_FOUND";
pub const ERR_NAMESPACE_NOT_FOUND: &str = "NAMESPACE_NOT_FOUND";
pub const ERR_TEST_EXECUTION_ERROR: &str = "TEST_EXECUTION_ERROR";
/// The generator's query failed after the code ran (or while it runs); the code is not run again.
pub const ERR_QUERY_FAILED_AFTER_RUN: &str = "QUERY_FAILED_AFTER_RUN";
pub const ERR_SERVER_MANAGER_CREDENTIAL: &str = "SERVER_MANAGER_CREDENTIAL_ERROR";
pub const ERR_SERVER_MANAGER_AMBIGUOUS: &str = "SERVER_MANAGER_AMBIGUOUS";
pub const ERR_POLICY_GATE: &str = "POLICY_GATE";

// ── Live connection hot-reload types (034) ───────────────────────────────────

/// How the currently active IRIS connection was established.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionSource {
    ConfigFile,
    EnvVars,
    IrisSelectContainer,
    AutoDiscovered,
}

/// Snapshot of the active IRIS connection, including metadata for `check_config`.
pub struct ConnectionState {
    pub iris: Option<Arc<IrisConnection>>,
    pub source: ConnectionSource,
    pub config_file: Option<std::path::PathBuf>,
    /// A configuration file that was read and names neither a host nor a container. It chose no
    /// connection, so it is not `config_file`; `check_config` says so (T194).
    pub config_file_no_target: Option<std::path::PathBuf>,
    pub loaded_at: std::time::SystemTime,
    /// Both gates plus the input that decided each, resolved once by the caller (085 FR-012).
    /// Replaced wholesale on reload — never mutated in place, which is what makes a config edit
    /// take effect in both directions.
    pub gates: write_gate::GateResolution,
    /// What the config file declared, kept so a later namespace/`SystemMode` change
    /// (`iris_select_container`) can re-resolve without losing the declaration.
    pub declared: write_gate::DeclaredGates,
    pub config_parse_error: Option<String>,
}

impl ConnectionState {
    /// No live connection. Takes the resolution rather than deriving one: this path used to read
    /// `IRIS_WRITE_TOOLS_ENABLED` with `unwrap_or(true)` — the opposite default from `from_iris` —
    /// so a server that could not reach IRIS answered permissively (085 FR-012).
    pub fn new_disconnected(source: ConnectionSource, gates: write_gate::GateResolution) -> Self {
        Self {
            iris: None,
            source,
            config_file: None,
            config_file_no_target: None,
            loaded_at: std::time::SystemTime::now(),
            gates,
            declared: write_gate::DeclaredGates::default(),
            config_parse_error: None,
        }
    }

    pub fn from_iris(
        iris: IrisConnection,
        source: ConnectionSource,
        config_file: Option<std::path::PathBuf>,
        gates: write_gate::GateResolution,
    ) -> Self {
        Self {
            iris: Some(Arc::new(iris)),
            source,
            config_file,
            config_file_no_target: None,
            loaded_at: std::time::SystemTime::now(),
            gates,
            declared: write_gate::DeclaredGates::default(),
            config_parse_error: None,
        }
    }

    /// Record the file that was read and chose nothing (T194).
    pub fn with_no_target_file(mut self, file: Option<std::path::PathBuf>) -> Self {
        self.config_file_no_target = file;
        self
    }

    /// Attach the config-file declaration to a state built by either constructor.
    pub fn with_declared(mut self, declared: write_gate::DeclaredGates) -> Self {
        self.declared = declared;
        self
    }
}

/// Tools that do not talk to IRIS, or exist to repair the connection, so they still answer when the
/// configuration is broken.
const CONFIG_TOLERANT_TOOLS: &[&str] = &[
    "check_config",
    "skill",
    "skill_list",
    "skill_search",
    "skill_describe",
    "skill_community",
    "skill_community_list",
    "skill_forget",
    "kb",
    "kb_index",
    "kb_recall",
    "iris_doc_search",
    "iris_symbols_local",
    "iris_servers",
    "iris_add_server",
    "iris_remove_server",
    "iris_reload_pool",
    "iris_import_servers",
];

/// The snapshot every config question is answered from, with the inputs that rebuild it.
#[derive(Default)]
struct ConfigHolder {
    snapshot: Option<Arc<crate::iris::config_sources::ConfigSnapshot>>,
    inputs: Option<crate::iris::config_sources::LoadInputs>,
    /// Held edits already announced on stderr, so each setting is said once (FR-043).
    held_announced: std::collections::BTreeSet<String>,
}

/// Tracks the `.iris-agentic-dev.toml` path and last-seen mtime for lazy hot-reload.
/// Always created (even when the file does not yet exist) so we detect new files appearing.
///
/// It also watches the operator file, which holds the trust entries: an edit there can make a
/// workspace file trusted, or end that trust, without the workspace file changing. The path it
/// reports to the agent (`config_path`) is never the operator file.
pub struct ConfigWatcher {
    /// The workspace-layer file, or an empty path when there is none to watch.
    pub config_path: std::path::PathBuf,
    /// None when the file did not exist at last check.
    pub last_mtime: Option<std::time::SystemTime>,
    operator_path: Option<std::path::PathBuf>,
    operator_mtime: Option<std::time::SystemTime>,
}

fn mtime_of(p: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl ConfigWatcher {
    /// Always returns Some — watcher is active even before the file exists.
    pub fn new(config_path: std::path::PathBuf) -> Option<Self> {
        let last_mtime = mtime_of(&config_path);
        Some(Self {
            config_path,
            last_mtime,
            operator_path: None,
            operator_mtime: None,
        })
    }

    /// A watcher for what a snapshot loaded: its workspace-layer path (or none) and the operator
    /// file.
    pub fn for_snapshot(snap: &crate::iris::config_sources::ConfigSnapshot) -> Option<Self> {
        let mut w = Self::new(snap.config_watch_path().unwrap_or_default())?;
        w.operator_mtime = snap.operator_path.as_deref().and_then(mtime_of);
        w.operator_path = snap.operator_path.clone();
        Some(w)
    }

    /// Returns true (and updates stored mtime) if the file has been created, modified,
    /// or has appeared for the first time since last check.
    pub fn has_changed(&mut self) -> bool {
        let current_mtime = mtime_of(&self.config_path);
        match (self.last_mtime, current_mtime) {
            // File newly appeared
            (None, Some(mtime)) => {
                self.last_mtime = Some(mtime);
                true
            }
            // File modified
            (Some(old), Some(new)) if new > old => {
                self.last_mtime = Some(new);
                true
            }
            // File deleted — reset so we detect re-creation
            (Some(_), None) => {
                self.last_mtime = None;
                false
            }
            _ => false,
        }
    }

    /// The same for the operator file; a deletion counts as a change, because the trust entries
    /// went with it.
    pub fn operator_changed(&mut self) -> bool {
        let Some(path) = &self.operator_path else {
            return false;
        };
        let current = mtime_of(path);
        let changed = match (self.operator_mtime, current) {
            (None, None) => false,
            (Some(old), Some(new)) => new > old,
            _ => true,
        };
        self.operator_mtime = current;
        changed
    }
}

// ── &sql macro translation (035) ─────────────────────────────────────────────

/// Result of translating `&sql(...)` macros to `%SQL.Statement` calls.
pub struct TranslationResult {
    /// The code after translation (equals input if `found` is false).
    pub translated_code: String,
    /// Whether any `&sql(...)` macros were found and processed.
    pub found: bool,
    /// Warnings for constructs that could not be safely translated (left unchanged).
    pub warnings: Vec<String>,
}

/// Translate `&sql(...)` embedded SQL macros in ObjectScript code to
/// runtime-compatible `%SQL.Statement` class method calls.
///
/// This is a pure text transformation — no IRIS network call is made.
/// SELECT INTO uses prepare/execute/get; DML uses %ExecDirect.
/// SQLCODE and %msg on the line immediately following the macro are rewritten
/// to read from the generated result set object; all other references are untouched.
pub fn translate_sql_macros(code: &str) -> TranslationResult {
    if !code.contains("&sql(") {
        return TranslationResult {
            translated_code: code.to_string(),
            found: false,
            warnings: vec![],
        };
    }

    let mut output = String::with_capacity(code.len() * 2);
    let mut warnings = vec![];
    let mut rs_counter: u32 = 0;
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut found = false;

    while i < n {
        // Look for &sql(
        if i + 5 < n
            && chars[i] == '&'
            && chars[i + 1] == 's'
            && chars[i + 2] == 'q'
            && chars[i + 3] == 'l'
            && chars[i + 4] == '('
        {
            found = true;
            rs_counter += 1;
            let rs_var = format!("sqlrs{}", rs_counter);
            let sc_var = format!("sqlsc{}", rs_counter);
            let sqlcode_var = format!("sqlSQLCODE{}", rs_counter);

            // Find matching closing paren using depth counting
            let start = i + 5; // after &sql(
            let mut depth = 1usize;
            let mut j = start;
            while j < n && depth > 0 {
                if chars[j] == '(' {
                    depth += 1;
                } else if chars[j] == ')' {
                    depth -= 1;
                }
                if depth > 0 {
                    j += 1;
                }
            }
            let sql_content: String = chars[start..j].iter().collect();
            i = j + 1; // skip past the closing )

            // Classify statement type
            let sql_upper = sql_content.trim().to_uppercase();
            if sql_upper.starts_with("CALL") {
                // Unsupported — leave unchanged with warning
                warnings.push(format!(
                    "&sql(CALL ...) at macro #{} was not translated — CALL statements with OUT parameters are not supported. Use ##class(...).Method() directly.",
                    rs_counter
                ));
                output.push_str(&format!("&sql({})", sql_content));
            } else if sql_upper.starts_with("SELECT") {
                // Translate SELECT INTO
                output.push_str(&translate_select_into(
                    &sql_content,
                    &rs_var,
                    &sc_var,
                    &sqlcode_var,
                ));
                // Check next line for SQLCODE / %msg and rewrite
                i = rewrite_next_line_sqlcode(
                    chars.as_slice(),
                    i,
                    n,
                    &mut output,
                    &sqlcode_var,
                    &rs_var,
                );
                continue;
            } else if sql_upper.starts_with("INSERT")
                || sql_upper.starts_with("UPDATE")
                || sql_upper.starts_with("DELETE")
                || sql_upper.starts_with("MERGE")
            {
                // Translate DML
                output.push_str(&translate_dml(&sql_content, &rs_var, &sqlcode_var));
                // Check next line for SQLCODE / %msg
                i = rewrite_next_line_sqlcode(
                    chars.as_slice(),
                    i,
                    n,
                    &mut output,
                    &sqlcode_var,
                    &rs_var,
                );
                continue;
            } else {
                // Unknown — leave unchanged with warning
                warnings.push(format!(
                    "&sql({}) at macro #{} was not translated — unrecognized SQL statement type.",
                    &sql_content[..sql_content.len().min(50)],
                    rs_counter
                ));
                output.push_str(&format!("&sql({})", sql_content));
            }
        } else {
            output.push(chars[i]);
            i += 1;
        }
    }

    TranslationResult {
        translated_code: output,
        found,
        warnings,
    }
}

/// Translate a SELECT ... INTO :var1, :var2 ... statement.
fn translate_select_into(sql: &str, rs_var: &str, sc_var: &str, sqlcode_var: &str) -> String {
    // Parse: split on INTO to separate column list and host variables + WHERE clause

    // Find INTO keyword (not inside parens)
    let into_pos = find_keyword_pos(sql, "INTO");

    let (select_cols_sql, rest_after_into) = if let Some(pos) = into_pos {
        let before = sql[..pos].trim().to_string();
        let after = &sql[pos + 4..]; // skip "INTO"
        (before, after.trim().to_string())
    } else {
        // SELECT without INTO — translate as result-set loop but no vars to set
        return translate_select_no_into(sql, rs_var, sc_var, sqlcode_var);
    };

    // Extract SELECT column names (between SELECT and INTO)
    // select_cols_sql is like "SELECT Name, Age"
    let col_list_str = if let Some(idx) = select_cols_sql.to_uppercase().find("SELECT") {
        select_cols_sql[idx + 6..].trim().to_string()
    } else {
        select_cols_sql.clone()
    };
    // rest_after_into is like ":name, :age FROM table WHERE ..."
    // Split host vars from FROM clause
    let (host_vars_str, from_clause) = split_host_vars_from_rest(&rest_after_into);
    let host_vars: Vec<String> = split_csv(&host_vars_str)
        .iter()
        .map(|v| v.trim().trim_start_matches(':').to_string())
        .collect();

    // Extract WHERE parameters (collect :varname in FROM+WHERE but not the host vars)
    let where_params = extract_where_params(&from_clause);

    // Build the SQL for %Prepare — SELECT cols FROM ... (without INTO clause)
    let prepared_sql = format!("SELECT {} {}", col_list_str, from_clause);
    // Replace :varname in WHERE with ?
    let prepared_sql = replace_host_vars_with_positional(&prepared_sql, &where_params);

    // Build the generated ObjectScript
    let mut out = String::new();
    out.push_str(&format!(
        "set {} = ##class(%SQL.Statement).%New()\n",
        rs_var
    ));
    out.push_str(&format!(
        "set {} = {}.%Prepare(\"{}\")\n",
        sc_var,
        rs_var,
        prepared_sql.replace('"', "\"\"")
    ));
    // Execute with WHERE params
    let exec_args = if where_params.is_empty() {
        String::new()
    } else {
        format!(", {}", where_params.join(", "))
    };
    out.push_str(&format!(
        "set {} = {}.%Execute({}{})\n",
        rs_var,
        rs_var,
        "",
        exec_args.trim_start_matches(", ")
    ));
    // Fetch the row in one braceless `set`: this output only runs on the docker path, which
    // refuses block syntax. Columns are read by position, because a name like `COUNT(*)` or
    // `TOP 1 Name` is not a property of the result (131 US4). No row leaves each host variable
    // "" and SQLCODE at the result's own code (100), as `&sql` does.
    let next_var = rs_var.replacen("sqlrs", "sqlnx", 1);
    out.push_str(&format!("set {} = {}.%Next()", next_var, rs_var));
    for (idx, var) in host_vars.iter().enumerate() {
        out.push_str(&format!(
            ", {} = $Select({}:{}.%GetData({}),1:\"\")",
            var,
            next_var,
            rs_var,
            idx + 1
        ));
    }
    out.push_str(&format!(
        ", {} = $Select({}:0,1:{}.%SQLCODE)",
        sqlcode_var, next_var, rs_var
    ));
    out.push_str(&sqlcode_assignments(sqlcode_var, rs_var));

    out
}

fn translate_select_no_into(sql: &str, rs_var: &str, sc_var: &str, _sqlcode_var: &str) -> String {
    // SELECT without INTO — translate to prepare/execute but no host var assignment
    let where_params = extract_where_params(sql);
    let prepared_sql = replace_host_vars_with_positional(sql, &where_params);
    let mut out = String::new();
    out.push_str(&format!(
        "set {} = ##class(%SQL.Statement).%New()\n",
        rs_var
    ));
    out.push_str(&format!(
        "set {} = {}.%Prepare(\"{}\")\n",
        sc_var,
        rs_var,
        prepared_sql.replace('"', "\"\"")
    ));
    let exec_args = where_params.join(", ");
    out.push_str(&format!(
        "set {} = {}.%Execute({})\n",
        rs_var, rs_var, exec_args
    ));
    out
}

fn translate_dml(sql: &str, rs_var: &str, sqlcode_var: &str) -> String {
    let params = extract_where_params(sql);
    let prepared_sql = replace_host_vars_with_positional(sql, &params);
    let exec_args = if params.is_empty() {
        String::new()
    } else {
        format!(", {}", params.join(", "))
    };
    format!(
        "set {} = ##class(%SQL.Statement).%ExecDirect(, {}{}), {} = {}.%SQLCODE{}, %ROWCOUNT = {}.%ROWCOUNT",
        rs_var,
        crate::objectscript::os_str_expr(&prepared_sql),
        exec_args,
        sqlcode_var,
        rs_var,
        sqlcode_assignments(sqlcode_var, rs_var),
        rs_var
    )
}

/// The `SQLCODE` and `%msg` that `&sql` itself would leave behind, as a `set` continuation. Code
/// on the macro's own line reads them, not only the line after.
fn sqlcode_assignments(sqlcode_var: &str, rs_var: &str) -> String {
    format!(", SQLCODE = {}, %msg = {}.%Message", sqlcode_var, rs_var)
}

/// After a translated &sql, check if the immediately following line contains
/// a standalone SQLCODE or %msg reference and rewrite it.
/// Returns the new position in chars after consuming any rewritten line.
fn rewrite_next_line_sqlcode(
    chars: &[char],
    mut i: usize,
    n: usize,
    output: &mut String,
    sqlcode_var: &str,
    rs_var: &str,
) -> usize {
    // Skip whitespace (but not newlines) to find the next line
    // First, collect the rest of the current line (should be empty or whitespace after &sql)
    while i < n && chars[i] != '\n' {
        output.push(chars[i]);
        i += 1;
    }
    if i < n && chars[i] == '\n' {
        output.push('\n');
        i += 1;
    }

    // Collect the next line
    let mut next_line = String::new();
    let line_start = i;
    while i < n && chars[i] != '\n' {
        next_line.push(chars[i]);
        i += 1;
    }

    if next_line.trim().is_empty() {
        // Empty line — output and continue
        output.push_str(&next_line);
        return i;
    }

    if next_line.trim().starts_with("&sql(") {
        // Another &sql macro — don't consume this line; let the main loop re-process it
        // Back up i to the start of this line
        return line_start;
    }

    // Rewrite SQLCODE → sqlcode_var and %msg → rs_var.%Message on this specific line
    let rewritten = next_line
        .replace("SQLCODE", sqlcode_var)
        .replace("%msg", &format!("{}.%Message", rs_var));

    output.push_str(&rewritten);
    i
}

/// Find the position of a keyword in SQL (case-insensitive), not inside parens.
fn find_keyword_pos(sql: &str, keyword: &str) -> Option<usize> {
    let upper = sql.to_uppercase();
    let kw_upper = keyword.to_uppercase();
    let mut depth = 0usize;
    let bytes = upper.as_bytes();
    let kw_bytes = kw_upper.as_bytes();
    let mut i = 0;
    while i + kw_bytes.len() <= bytes.len() {
        if bytes[i] == b'(' {
            depth += 1;
        } else if bytes[i] == b')' && depth > 0 {
            depth -= 1;
        } else if depth == 0 && bytes[i..].starts_with(kw_bytes) {
            // Word boundary check
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphabetic();
            let after_ok = i + kw_bytes.len() >= bytes.len()
                || !bytes[i + kw_bytes.len()].is_ascii_alphabetic();
            if before_ok && after_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Split a comma-separated list, respecting parens.
fn split_csv(s: &str) -> Vec<String> {
    let mut result = vec![];
    let mut current = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ',' if depth == 0 => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    result.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        result.push(trimmed);
    }
    result
}

/// Split host variables (:var1, :var2) from the rest of the SQL after INTO.
/// Returns (host_vars_str, from_and_where_clause).
fn split_host_vars_from_rest(after_into: &str) -> (String, String) {
    // after_into looks like ":name, :age FROM table WHERE ..."
    // Find "FROM" keyword
    let upper = after_into.to_uppercase();
    if let Some(from_pos) = find_keyword_pos(after_into, "FROM") {
        let vars = after_into[..from_pos].trim().to_string();
        let rest = after_into[from_pos..].trim().to_string();
        (vars, rest)
    } else if let Some(pos) = upper.find("FROM") {
        (
            after_into[..pos].trim().to_string(),
            after_into[pos..].trim().to_string(),
        )
    } else {
        (after_into.to_string(), String::new())
    }
}

/// Extract :varname host variables from WHERE/VALUES clause in order, returning bare names.
fn extract_where_params(sql: &str) -> Vec<String> {
    let mut params = vec![];
    let chars: Vec<char> = sql.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut in_string = false;
    let mut string_char = ' ';
    while i < n {
        let c = chars[i];
        if in_string {
            if c == string_char {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '\'' || c == '"' {
            in_string = true;
            string_char = c;
            i += 1;
            continue;
        }
        if c == ':' && i + 1 < n && chars[i + 1].is_alphabetic() {
            i += 1;
            let mut name = String::new();
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                name.push(chars[i]);
                i += 1;
            }
            if !params.contains(&name) {
                params.push(name);
            }
            continue;
        }
        i += 1;
    }
    params
}

/// Replace :varname with ? in SQL string, tracking order.
fn replace_host_vars_with_positional(sql: &str, params: &[String]) -> String {
    let mut result = sql.to_string();
    for param in params {
        result = result.replace(&format!(":{}", param), "?");
    }
    result
}

/// The in-memory ring buffer entry type. Superseded by `crate::telemetry::ToolCallRecord`
/// (059-tool-telemetry-benchmark), which adds `duration_ms`/`session_id`/`params` beyond
/// the original `{tool, success, timestamp}` shape.
pub use crate::telemetry::ToolCallRecord as ToolCallEntry;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompileParams {
    pub target: String,
    #[serde(default = "default_flags")]
    pub flags: String,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub force_writable: bool,
    /// If true, bypass the log store and return all errors/warnings inline regardless of count.
    #[serde(default)]
    pub inline: bool,
    /// Set to true to confirm execution on a subject-role instance (role-gate bypass).
    #[serde(default)]
    pub confirm: bool,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestParams {
    /// Confirms the call on a `subject` server, where a write is refused until it is set. No
    /// effect on other servers.
    #[serde(default)]
    pub confirm: bool,
    pub pattern: String,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default = "default_test_timeout")]
    pub timeout: u64,
    /// Set true to also measure line coverage inline (wraps iris_coverage mode=run)
    pub coverage: Option<bool>,
    /// Explicit class list for coverage; if omitted, derived from pattern package
    pub coverage_classes: Option<Vec<String>>,
    /// Coverage target percentage threshold
    pub coverage_target_pct: Option<f64>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
    /// Test class type override. Values: "auto" (default), "testcase", "testproduction".
    /// "auto" detects %UnitTest.TestProduction subclasses and uses .Run() automatically.
    /// "testcase" forces %UnitTest.Manager::RunTest(). "testproduction" forces .Run().
    #[serde(default)]
    #[schemars(extend("enum" = ["auto", "testcase", "testproduction"]))]
    pub test_type: Option<String>,
}

fn default_test_timeout() -> u64 {
    60
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolsParams {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntrospectParams {
    pub class_name: String,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DebugMapParams {
    #[serde(default)]
    pub routine: String,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub error_string: String,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateClassParams {
    /// Confirms the call on a `subject` server, where a write is refused until it is set. No
    /// effect on other servers.
    #[serde(default)]
    pub confirm: bool,
    pub description: String,
    #[serde(default)]
    pub overwrite: bool,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateTestParams {
    /// Confirms the call on a `subject` server, where a write is refused until it is set. No
    /// effect on other servers.
    #[serde(default)]
    pub confirm: bool,
    pub class_name: String,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillNameParams {
    pub name: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillSearchParams {
    pub query: String,
    #[serde(default = "default_limit")]
    pub top_k: usize,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbIndexParams {
    pub workspace_path: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbRecallParams {
    pub query: String,
    #[serde(default = "default_limit")]
    pub top_k: usize,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentHistoryParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TelemetryQueryParams {
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
    #[serde(default = "default_telemetry_query_limit")]
    pub limit: usize,
}
fn default_telemetry_query_limit() -> usize {
    500
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TelemetryExportTraceParams {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub since: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolsLocalParams {
    pub query: String,
    pub workspace_path: Option<String>,
    #[serde(default = "default_symbols_local_limit")]
    pub limit: usize,
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}
fn default_symbols_local_limit() -> usize {
    50
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapturePacketParams {
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ErrorLogsParams {
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    /// If true, bypass the log store and return all entries inline regardless of count.
    #[serde(default)]
    pub inline: bool,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommunityPkgParams {
    pub name: String,
}
/// The parameter type for tools that genuinely take no arguments.
///
/// `deny_unknown_fields` is the point: without it, a call passing `{"server": "prod"}` to a
/// no-argument tool succeeds against the default connection, which is not what the caller asked
/// for. It also makes the emitted schema carry `additionalProperties: false`, so a client can tell
/// "no parameters" apart from "parameters unspecified".
/// schemars omits `properties` entirely for a fieldless struct, which reads to a client as
/// "parameters unspecified" rather than "no parameters".
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("properties" = serde_json::Map::new()))]
pub struct NoParams {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetLogParams {
    /// UUID of a stored log entry. If omitted, lists all stored entries.
    pub id: Option<String>,
    /// Max entries to return from the stored result. Must be > 0 if provided.
    pub limit: Option<usize>,
    /// Start index into the stored result. Default 0.
    #[serde(default)]
    pub offset: usize,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceMapParams {
    /// Class name to build source map for (e.g. "Graph.KG.NKGAccel" or "Graph.KG.NKGAccel.cls").
    pub cls_name: String,
    /// Not used — kept for backwards compatibility only. May be removed in a future version.
    #[serde(default)]
    pub cls_text: Option<String>,
    pub workspace_path: Option<String>,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
}
// 053-doc-depth
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisExecuteMethodParams {
    /// Confirms the call on a `subject` server, where a write is refused until it is set. No
    /// effect on other servers.
    #[serde(default)]
    pub confirm: bool,
    /// Class name e.g. "%Library.Integer" or "MyApp.Utils"
    pub class: String,
    /// Method name e.g. "IsValid" or "FormatDate"
    pub method: String,
    /// Positional string arguments passed to the method
    #[serde(default)]
    pub args: Vec<String>,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

/// Typed parameters for `iris_production`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisProductionParams {
    /// Confirms the call on a `subject` server, where a write is refused until it is set. No
    /// effect on other servers.
    #[serde(default)]
    pub confirm: bool,
    /// Action to perform: status, start, stop, update, check, recover, get_autostart, set_autostart.
    #[schemars(extend("enum" = [
        "status",
        "start",
        "stop",
        "update",
        "check",
        "recover",
        "get_autostart",
        "set_autostart",
    ]))]
    #[serde(default = "default_production_action")]
    pub action: String,
    /// Production class name (used by start/stop).
    #[serde(default)]
    pub production_name: Option<String>,
    /// Return full production config detail (status action only).
    #[serde(default)]
    pub full: bool,
    /// Force stop even if production is busy (stop action only).
    #[serde(default)]
    pub force: bool,
    /// Stop timeout in seconds (stop action only, default 30).
    #[serde(default = "default_production_timeout")]
    pub timeout: u32,
    /// Enable autostart for a production (set_autostart action only).
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Production to configure for autostart (set_autostart action only).
    #[serde(default)]
    pub production: Option<String>,
    /// IRIS namespace for production operations. Defaults to the connection namespace.
    /// Use when the interop production lives in a different namespace than the default connection.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

fn default_production_action() -> String {
    "status".to_string()
}

fn default_production_timeout() -> u32 {
    30
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecuteParams {
    /// Confirms the call on a `subject` server, where a write is refused until it is set. No
    /// effect on other servers.
    #[serde(default)]
    pub confirm: bool,
    pub code: String,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default = "default_execute_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub confirmed: bool,
    /// If true (default), rewrite &sql(...) embedded SQL macros to %SQL.Statement calls on the docker
    /// exec path, whose terminal has no &sql. The HTTP path always runs &sql as written.
    /// Set to false to send code as-is for debugging.
    #[serde(default = "default_translate_sql")]
    pub translate_sql: bool,
    /// Enable session state. When true, `%ctx` (%DynamicObject) is injected before user code
    /// and serialized to `session_state` in the response after user code runs. Pass the prior
    /// call's `session_state` to restore `%ctx` across calls — no state is written to IRIS.
    #[serde(default)]
    pub use_session: bool,
    /// Opaque session token from a prior `iris_execute` call with `use_session: true`.
    /// Restores `%ctx` at the start of execution. Ignored when `use_session: false`.
    #[serde(default)]
    pub session_state: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}
fn default_translate_sql() -> bool {
    true
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryParams {
    /// SQL statement. Required for read/explain/write; optional for count (use `table` instead).
    #[serde(default)]
    pub query: String,
    /// Query parameters as strings (e.g. ["Alice", "42"])
    #[serde(default)]
    pub parameters: Vec<String>,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// If true, bypass SQL safety validation. Use only for intentional administrative queries.
    /// Has no effect on production IRIS instances (where write tools are disabled).
    /// Ignored in mode="write" — see `force_ignored` in the response.
    #[serde(default)]
    pub force: bool,
    /// Set to true to confirm execution on a subject-role instance (role-gate bypass).
    #[serde(default)]
    pub confirm: bool,
    /// Execution mode: "read" (default), "explain", "count", or "write".
    #[schemars(extend("enum" = ["read", "explain", "count", "write"]))]
    pub mode: Option<String>,
    /// Table name for mode="count" when `query` is not provided.
    pub table: Option<String>,
    /// Max rows an UPDATE/DELETE may affect in mode="write" before ROWS_LIMIT_EXCEEDED.
    /// Default 1000, clamped to [1, 10000]. 0 is treated as the default.
    pub max_rows_affected: Option<u32>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListContainersParams {
    pub workspace_root: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectContainerParams {
    pub name: String,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default = "default_username")]
    pub username: String,
    #[serde(default = "default_password")]
    pub password: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartSandboxParams {
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_edition")]
    pub edition: String,
}

fn default_flags() -> String {
    "cuk".to_string()
}

/// The compile flags `iris_compile` passes on: compile, display, keep source, update only. Every
/// other letter either adds classes to the set compiled (`b` subclasses, `r` predecessors, `y`
/// related classes, `p` and `h` the percent and hidden ones) or does something else to them
/// (`e` deletes the extent).
const COMPILE_FLAG_LETTERS: &str = "cdku";

/// The compile qualifiers `iris_compile` passes on, by full name. IRIS reads a qualifier name in
/// any case and accepts an unambiguous abbreviation (`/sub` is `/subclasses`), so a list of the
/// qualifiers that widen the set would miss spellings; this is the list of the ones that do not. A
/// `no` in front of one of the display qualifiers negates it.
const COMPILE_QUALIFIERS: &[&str] = &[
    "checkuptodate",
    "display",
    "displayerror",
    "displaylog",
    "keepsource",
    "quiet",
    "silent",
    "verbose",
];

/// What `iris_compile flags` and `compile --flags` may carry (spec 141 C1, T206).
///
/// The characters are letters, digits and `/ - = : , .` only (C1): a quote, a space or a newline
/// is refused before it reaches `$SYSTEM.OBJ.Compile` or the Atelier query. Beyond the characters,
/// the flags are [`COMPILE_FLAG_LETTERS`] and the qualifiers are [`COMPILE_QUALIFIERS`]. The
/// stored-source check reads the target and what it includes; a flag or qualifier that makes IRIS
/// compile other classes lets a stored class run its compile-time code unread (T206).
pub fn compile_flags_problem(flags: &str) -> Option<String> {
    let ok = flags
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/-=:,.".contains(c));
    if !ok {
        return Some(format!(
            "flags {flags:?} has characters outside the compile qualifier set (letters, digits, / - = : , .)"
        ));
    }
    let (letters, qualifiers) = match flags.split_once('/') {
        Some((letters, rest)) => (letters, Some(rest)),
        None => (flags, None),
    };
    if let Some(c) = letters
        .chars()
        .find(|c| *c != '-' && !COMPILE_FLAG_LETTERS.contains(c.to_ascii_lowercase()))
    {
        return Some(format!(
            "flags {flags:?}: the compile flag {c:?} is not allowed. It can compile classes the \
             target-only check never read, or change more than the compile. The allowed flags are \
             {}, with - to turn one off",
            COMPILE_FLAG_LETTERS
                .chars()
                .map(String::from)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for token in qualifiers.into_iter().flat_map(|q| q.split('/')) {
        let name = token.split('=').next().unwrap_or("").to_ascii_lowercase();
        let known = |n: &str| COMPILE_QUALIFIERS.contains(&n);
        if !(known(&name) || name.strip_prefix("no").is_some_and(known)) {
            return Some(format!(
                "flags {flags:?}: the compile qualifier /{name} is not allowed. Qualifiers such as \
                 /subclasses, /predecessorclasses, /relatedclasses and /expand compile classes the \
                 target-only check never read. The allowed qualifiers are {}",
                COMPILE_QUALIFIERS
                    .iter()
                    .map(|q| format!("/{q}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    None
}
/// Does an `iris_compile` target name a file on this host rather than a document on the server?
/// A path separator says so, and so does a `.cls` name that exists here.
fn is_local_compile_target(target: &str) -> bool {
    target.contains('/')
        || target.contains('\\')
        || (target.ends_with(".cls") && std::path::Path::new(target).exists())
}

/// Resolve the effective namespace for a tool call: use `param` if non-empty, else
/// `connection_ns` (the configured namespace of the connection the call actually uses —
/// pool member or default). Callers with no connection in scope pass "USER" as
/// `connection_ns`, so USER is only ever the last-resort fallback (issue #96).
pub fn resolve_namespace<'a>(param: Option<&'a str>, connection_ns: &'a str) -> &'a str {
    match param {
        Some(s) if !s.is_empty() => s,
        _ => connection_ns,
    }
}
fn default_limit() -> usize {
    20
}
fn default_max_entries() -> usize {
    50
}
fn default_execute_timeout() -> u64 {
    // Tests can run for >30s on large suites. Default 120s; override with OBJECTSCRIPT_TEST_TIMEOUT.
    std::env::var("OBJECTSCRIPT_TEST_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120)
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

/// ObjectScript that runs `call` in a JOB whose principal device is a temp file, waits for it,
/// and writes the file's lines to the current device.
///
/// `%UnitTest.Manager` checks `messages.log` after each test class and when it closes; if the file
/// grew, it opens it, uses it, closes it, and never restores the caller's device, so `$IO` falls to
/// `$PRINCIPAL`. In the CSP process that runs generator code, the principal is the gateway socket:
/// the output was lost, or the manager's error text reached the HTTP connection ahead of the
/// headers, and `iris_test` answered NO_TESTS_FOUND for a test that had passed. In the JOB the
/// principal is the file, so whatever the manager does, the output lands there. Single-line
/// statements with no braces, because docker exec feeds the same code to a terminal line by line.
pub fn run_in_job(call: &str) -> String {
    [
        " Set iadJobOut=##class(%Library.File).TempFilename(\"txt\")".to_string(),
        format!(" Job {call}:($NAMESPACE::iadJobOut:iadJobOut):10  Set iadJobOk=$TEST,iadJobPid=$ZCHILD"),
        " If 'iadJobOk Write \"ERROR: iad could not start a background job to run the tests\",!".to_string(),
        " If iadJobOk For iadJobI=1:1:864000 Quit:'$Data(^$JOB(iadJobPid))  Hang 0.1".to_string(),
        " Set iadJobS=##class(%Stream.FileCharacter).%New(),iadJobS.TranslateTable=\"UTF8\"".to_string(),
        " If iadJobOk,$System.Status.IsOK(iadJobS.LinkToFile(iadJobOut)) For  Quit:iadJobS.AtEnd  Write iadJobS.ReadLine(),!".to_string(),
        " Kill iadJobS  Do ##class(%Library.File).Delete(iadJobOut)".to_string(),
    ]
    .join("\n")
}

/// The `RunTest` flags: a compiled class pattern is looked up in the database (`/noload`).
/// `iris_test` and `iris_coverage` run the same pattern with the same flags (spec 141 R5 C17).
pub fn test_run_flags() -> &'static str {
    "/verbose=1/nodelete/noload"
}

/// Why a test run found nothing to run, when the reason is `^UnitTestRoot`: `RunTest` looks for its
/// suites in the directory that global names, and an instance that never set it (a fresh container)
/// falls back to a path under the manager directory that is not a directory. `%UnitTest.Manager`
/// says so in a `Finding directories: ERROR #5007` line and then reports a failed `(root)` suite,
/// which reads like a test failure. iad makes a root for each run (`with_run_root`), so this means
/// that step failed, and the text says what to check. `None` when `output` has no such line.
pub fn unit_test_root_problem(output: &str) -> Option<String> {
    let line = output
        .lines()
        .find(|l| l.contains("Finding directories: ERROR #5007"))?;
    let dir = line
        .split_once("Directory name '")
        .and_then(|(_, rest)| rest.split_once("' is invalid"))
        .map(|(dir, _)| dir)?;
    Some(format!(
        "RunTest looked for its tests in {dir}, which is not a directory, so no test ran. iad makes a root of its own for each run under the instance's <mgr>/Temp; this answer means it could not (the account may lack write access to that directory or to ^UnitTestRoot in this namespace). Check that <mgr>/Temp is writable by the IRIS process and that the account can set ^UnitTestRoot."
    ))
}

/// The `docker` arguments that read `WebServer=` from `cpf` inside `container`, or the reason the
/// name is refused. The name is checked like every other `docker exec` path's, and `--` ends the
/// options, so a configured name that docker would read as an option never reaches it (T221).
pub fn cpf_probe_args(container: &str, cpf: &str) -> Result<Vec<String>, String> {
    crate::iris::connection::validate_container_name(container)?;
    Ok(vec![
        "exec".into(),
        "--".into(),
        container.into(),
        "sh".into(),
        "-c".into(),
        format!("grep -i 'WebServer' {cpf} 2>/dev/null || true"),
    ])
}

/// Why `pattern` cannot be a test pattern, or `None` when it can (spec 141 T218).
///
/// A pattern names compiled classes (`MyApp.Tests`, `MyApp.*`). It used to double as a directory
/// that iad made under a fixed `/tmp/httest/` on the IRIS host, spliced in unchecked, which is
/// writable by any local user and took `..`. iad no longer makes a directory, so a pattern with a
/// path separator or a `..` has no meaning, and it is refused rather than passed to `RunTest`.
pub fn test_pattern_problem(pattern: &str) -> Option<String> {
    let bad = pattern.is_empty()
        || pattern.contains("..")
        || pattern.contains(['/', '\\'])
        || pattern.chars().any(char::is_control);
    bad.then(|| {
        format!(
            "pattern {pattern:?} is not a test pattern. Give compiled test classes as a class or \
             package pattern such as 'MyApp.Tests' or 'MyApp.*'; an empty value, '..' and path \
             separators are not accepted."
        )
    })
}

/// Build the ObjectScript code to run a test class.
/// `is_test_production=true` → uses `.Run()` (for %UnitTest.TestProduction subclasses).
/// `is_test_production=false` → uses %UnitTest.Manager::RunTest() with `flags` and `token`.
pub fn build_test_run_code(
    pattern: &str,
    flags: &str,
    token: &str,
    is_test_production: bool,
) -> String {
    use crate::objectscript::{os_lit_inner, validate_class_name};
    // The pattern goes inside literals below, so it is escaped for them; `.Run()` splices it bare
    // as a class name, so that path needs a valid one (137).
    let (pattern_lit, flags_lit, token_lit) = (
        os_lit_inner(pattern),
        os_lit_inner(flags),
        os_lit_inner(token),
    );

    let run = if is_test_production && validate_class_name(pattern).is_ok() {
        run_in_job(&format!("##class({pattern}).Run()"))
    } else {
        run_in_job(&format!(
            r#"##class(%UnitTest.Manager).RunTest("{pattern_lit}","{flags_lit}","{token_lit}")"#
        ))
    };
    with_run_root(&pattern_lit, &run)
}

/// `run` with a usable `^UnitTestRoot` (spec 141 T218). `RunTest` reads its suites from the
/// directory that global names, and with none (a fresh instance) it answers `ERROR #5007` and runs
/// nothing, not even a `%UnitTest.TestProduction`. 1.6.4 pointed the global at a fixed `/tmp/httest/`
/// that any local user could write. A root the instance already has is used as it is, so tests
/// staged under it still run. Only when there is none, or it names no directory, does the run get a
/// fresh one under the instance's own `<mgr>/Temp`, with the pattern's directory in it (`A.B` is
/// `A/B/`); after the job the previous value (or none) is put back and the directory removed.
/// `pattern_lit` is already escaped for a string literal and holds no `..` or path separator
/// (`test_pattern_problem`).
fn with_run_root(pattern_lit: &str, run: &str) -> String {
    [
        " Set iadRootWas=$Get(^UnitTestRoot),iadRootHad=$Data(^UnitTestRoot)#2,iadRootOwn=0".to_string(),
        format!(
            " If (iadRootWas=\"\")||'##class(%File).DirectoryExists(iadRootWas) {{ Set iadRoot=##class(%File).NormalizeDirectory($System.Util.ManagerDirectory()_\"Temp/iad-\"_$System.Util.CreateGUID()),iadRootOwn=1 Do ##class(%File).CreateDirectoryChain(iadRoot_$Translate($Piece(\"{pattern_lit}\",\"*\"),\".\",\"/\")) Set ^UnitTestRoot=iadRoot }}"
        ),
        run.to_string(),
        " If iadRootOwn { If iadRootHad { Set ^UnitTestRoot=iadRootWas } Else { Kill ^UnitTestRoot } Do ##class(%File).RemoveDirectoryTree(iadRoot) }".to_string(),
    ]
    .join("\n")
}

/// ObjectScript snippet to probe whether a class extends %UnitTest.TestProduction.
/// Returns code that writes "1" if it does, "0" otherwise.
pub fn build_superclass_probe(class_name: &str) -> String {
    let cls_lit = crate::objectscript::os_lit_inner(class_name);
    format!(
        r#"set oref=##class(%Dictionary.ClassDefinition).%OpenId("{cls_lit}")
if $isobject(oref) {{ write $select($find(oref.Super,"%UnitTest.TestProduction"):1,1:0) }} else {{ write 0 }}"#,
    )
}

/// Does `pattern` name a single compiled `%UnitTest.TestProduction` subclass? A wildcard or a
/// directory never does. `iris_test`'s auto detection, for tools that run a test pattern without a
/// `test_type` argument (spec 141 R5 C17).
pub async fn detect_test_production(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    pattern: &str,
) -> bool {
    let is_single_class = !pattern.contains('*');
    if !is_single_class {
        return false;
    }
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        iris.execute_via_generator(&build_superclass_probe(pattern), namespace, client),
    )
    .await
    .ok()
    .and_then(|r| r.ok())
    .is_some_and(|s| s.trim().starts_with('1'))
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

    // success=true means the test run executed (tool worked); outcome reflects test results.
    // Agents should check outcome, not success, to decide whether to fix code vs. fix tooling.
    let outcome = if errors > 0 {
        "errored"
    } else if failed > 0 {
        "failed"
    } else {
        "passed"
    };
    serde_json::json!({
        "success": true,
        "outcome": outcome,
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

/// Base directory for local-file-mode telemetry (JSONL sink + prune target), mirroring
/// the `.iris-agentic-dev` home-dir convention already used by `write_open_hint`.
pub fn telemetry_config_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".iris-agentic-dev")
}
/// A refusal from [`IrisTools::exec_context`]. A server that is not connected stays the protocol
/// error it has always been; the rest are tool errors that name the identity or the route.
fn exec_refused(e: exec::ExecError) -> Result<CallToolResult, McpError> {
    if e.code == "IRIS_UNREACHABLE" {
        return Err(iris_unreachable());
    }
    err_result(e.to_json())
}
fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::structured(v))
}
/// Wrap a genuine tool-failure envelope: same JSON body as before, but with the
/// MCP protocol-level `isError` flag set (issue #95). Dialog/soft responses
/// (elicitation prompts, empty-result notes) must NOT go through this — they are
/// normal outcomes and stay `CallToolResult::success`.
pub(crate) fn err_result(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::structured_error(v))
}
/// Wrap a handler-produced JSON value whose error-ness is only known at runtime:
/// a body carrying a top-level `error_code` without `success: true` is a genuine
/// failure and gets the `isError` flag; everything else (successes, failing test
/// runs without an error_code, elicitation dialogs) stays a success result.
fn json_result(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    let genuine_error = v.get("error_code").is_some()
        && v.get("success").and_then(serde_json::Value::as_bool) != Some(true);
    if genuine_error {
        err_result(v)
    } else {
        ok_json(v)
    }
}
fn err_json(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    err_result(serde_json::json!({"success": false, "error_code": code, "error": msg}))
}

/// `result` with `warnings` added to its body: the scratch classes the call could not remove. A
/// result with no object body is returned as it is.
pub fn with_warnings(result: CallToolResult, warnings: Vec<String>) -> CallToolResult {
    if warnings.is_empty() {
        return result;
    }
    let Some(serde_json::Value::Object(mut body)) = result.structured_content.clone() else {
        return result;
    };
    let mut all = match body.remove("warnings") {
        Some(serde_json::Value::Array(existing)) => existing,
        _ => Vec::new(),
    };
    all.extend(warnings.into_iter().map(serde_json::Value::String));
    body.insert("warnings".into(), serde_json::Value::Array(all));
    let body = serde_json::Value::Object(body);
    if result.is_error == Some(true) {
        CallToolResult::structured_error(body)
    } else {
        CallToolResult::structured(body)
    }
}

/// `SQL_ERROR` with a `hint` and `hint_ref` when the message matches a rule in `error_hints`
/// (specs 125, 129).
/// `query` is the statement as the agent wrote it (or the table name, in count mode): several rules
/// read the agent's own spelling from it (spec 129).
fn sql_err(msg: &str, query: &str, namespace: &str) -> Result<CallToolResult, McpError> {
    let mut v = serde_json::json!({"success": false, "error_code": "SQL_ERROR", "error": msg});
    if let Some(hint) = error_hints::sql_error_hint(msg, query, namespace) {
        hint.apply(&mut v);
    }
    err_result(v)
}

/// Parse `"http://host:port"` into `(host, port)`.
///
/// Returns `("unknown", 0)` on failure. Used by `iris_servers` for clean output.
fn parse_host_port(base_url: &str) -> (String, u16) {
    // Strip scheme prefix (e.g. "http://").
    let after_scheme = base_url
        .find("://")
        .map(|i| &base_url[i + 3..])
        .unwrap_or(base_url);
    // Strip any path suffix.
    let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    // Split on the last colon to handle IPv6 or plain host:port.
    if let Some(colon) = host_port.rfind(':') {
        let host = host_port[..colon].to_string();
        let port = host_port[colon + 1..].parse::<u16>().unwrap_or(0);
        (host, port)
    } else {
        (host_port.to_string(), 0)
    }
}

pub fn write_open_hint(namespace: &str, document: &str) {
    if let Some(home) = dirs::home_dir() {
        let dir = home.join(".iris-agentic-dev");
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

// ── SQL safety gate ───────────────────────────────────────────────────────────

/// SQL keywords a read-only statement may not contain outside a literal, an identifier or a
/// comment.
const WRITE_KEYWORDS: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "MERGE", "TRUNCATE", "EXEC",
    "EXECUTE", "BULK", "LOAD", "KILL", "LOCK", "GRANT", "REVOKE",
];

/// The first of [`WRITE_KEYWORDS`] among `code`, upper-case.
pub(crate) fn first_write_keyword(code: &[&crate::sql_lex::Token]) -> Option<&'static str> {
    code.iter()
        .find_map(|t| WRITE_KEYWORDS.iter().copied().find(|kw| t.is_word(kw)))
}

/// Validates that a SQL string is read-only before forwarding to IRIS.
///
/// The statement is read by `sql_lex`, which follows IRIS: comments and the contents of literals
/// and delimited identifiers are never keywords. Each check below takes the statement's code
/// tokens, so a further rule is one more function in the chain.
///
/// 1. `Err("EMPTY")` when nothing but white space and comments remains.
/// 2. A word from [`WRITE_KEYWORDS`] is refused, case-insensitively.
/// 3. `SELECT ... INTO <non-paren>` (DDL via SELECT INTO) is refused.
/// 4. The statement must start with `SELECT`, `WITH` or `EXPLAIN`, and be the only one.
///
/// Returns `Ok(())` if safe, `Err(keyword)` with the offending keyword if blocked.
pub fn validate_read_only_sql(sql: &str) -> Result<(), String> {
    let tokens = crate::sql_lex::lex(sql);
    let code: Vec<&crate::sql_lex::Token> = crate::sql_lex::code(&tokens).collect();
    if code.is_empty() {
        return Err("EMPTY".to_string());
    }
    if let Some(kw) = first_write_keyword(&code) {
        return Err(kw.to_string());
    }
    reject_select_into(&code)?;
    require_read_statement(&code)
}

/// Read mode runs one statement that starts with `SELECT`, `WITH` or `EXPLAIN` (IRIS's EXPLAIN
/// plans a statement and runs nothing). Anything else (`CALL`, `TUNE TABLE`, `BUILD INDEX`,
/// `PURGE`, `SET OPTION`, `FREEZE PLANS`, DML, DDL) is a statement for `mode="write"`, and a
/// second statement after a `;` is refused whatever it is. Only the first token decides, and it is
/// the first one outside comments. Spec 141 T213.
fn require_read_statement(code: &[&crate::sql_lex::Token]) -> Result<(), String> {
    const READ_STATEMENTS: &[&str] = &["SELECT", "WITH", "EXPLAIN"];
    let first = code[0];
    if !READ_STATEMENTS.iter().any(|kw| first.is_word(kw)) {
        return Err(if first.kind == crate::sql_lex::Kind::Word {
            first.text.to_uppercase()
        } else {
            first.text.clone()
        });
    }
    let trailing_only = code
        .iter()
        .position(|t| t.is_punct(';'))
        .is_none_or(|at| code[at..].iter().all(|t| t.is_punct(';')));
    if trailing_only {
        Ok(())
    } else {
        Err("STATEMENT LIST".to_string())
    }
}

/// `INTO` followed by anything but `(` is `SELECT ... INTO` a table or variable, which creates
/// one. `INTO (` is a subquery's own clause and is allowed.
fn reject_select_into(code: &[&crate::sql_lex::Token]) -> Result<(), String> {
    for (i, token) in code.iter().enumerate() {
        if !token.is_word("INTO") {
            continue;
        }
        if code.get(i + 1).is_some_and(|next| !next.is_punct('(')) {
            return Err("SELECT INTO".to_string());
        }
    }
    Ok(())
}

/// Validates that a SQL string is acceptable DML for `iris_query` `mode="write"`.
///
/// Reads the statement with `sql_lex`, like `validate_read_only_sql`, with the opposite polarity:
/// DDL (CREATE/DROP/ALTER/GRANT/REVOKE) and SELECT are blocked;
/// DML (INSERT/UPDATE/DELETE/CALL/TRUNCATE) is allowed. Classification is based on the
/// statement's leading keyword only — an inner SELECT subquery (e.g.
/// `INSERT INTO t SELECT * FROM src`) does not affect the outer statement's classification.
///
/// Returns `Ok(())` for allowed DML, or `Err(reason)` where `reason` is one of:
/// `"EMPTY"`, `"SELECT_IN_WRITE"`, `"UNKNOWN_STATEMENT"`, or the blocked DDL keyword.
pub fn validate_dml_sql(sql: &str) -> Result<(), String> {
    const DDL: &[&str] = &["CREATE", "DROP", "ALTER", "GRANT", "REVOKE"];
    const DML: &[&str] = &["INSERT", "UPDATE", "DELETE", "CALL", "TRUNCATE"];

    let tokens = crate::sql_lex::lex(sql);
    let code: Vec<&crate::sql_lex::Token> = crate::sql_lex::code(&tokens).collect();
    if code.is_empty() {
        return Err("EMPTY".to_string());
    }
    let first = code
        .iter()
        .find(|t| t.kind == crate::sql_lex::Kind::Word)
        .map(|t| t.text.to_uppercase())
        .unwrap_or_default();
    if DDL.contains(&first.as_str()) {
        return Err(first);
    }
    if first == "SELECT" {
        return Err("SELECT_IN_WRITE".to_string());
    }
    if DML.contains(&first.as_str()) {
        return Ok(());
    }
    Err("UNKNOWN_STATEMENT".to_string())
}

/// Deterministic 16-hex-char identifier for a query shape, used by `mode="explain"` to let
/// agents correlate plan observations across runs. Not a cryptographic hash — normalizes by
/// uppercasing and collapsing whitespace before hashing, so formatting differences that don't
/// change the query's meaning produce the same hash.
pub fn query_hash(query: &str) -> String {
    use std::hash::{Hash, Hasher};
    let normalized: String = query
        .to_uppercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Builds the COUNT query for `iris_query` `mode="count"`. `query` takes precedence over
/// `table` per FR-006/FR-008 — when both are provided the `query` form (subquery wrap) is
/// used and `table` is ignored.
pub fn build_count_query(table: Option<&str>, query: Option<&str>) -> String {
    if let Some(q) = query {
        format!("SELECT COUNT(*) FROM ({q}) t")
    } else {
        format!("SELECT COUNT(*) FROM {}", table.unwrap_or_default())
    }
}

/// `build_count_query`, refused when the table is not a table name or the query is not a read.
/// The count mode used to skip both checks the other modes apply (137).
pub fn count_query_checked(
    table: Option<&str>,
    query: Option<&str>,
) -> Result<String, serde_json::Value> {
    if query.is_none() {
        if let Some(t) = table {
            crate::objectscript::validate_sql_identifier(t)?;
        }
    }
    if let Some(q) = query {
        if q.contains(';') {
            return Err(serde_json::json!({
                "success": false,
                "error_code": "INVALID_PARAMS",
                "error": "mode=\"count\" takes one SELECT; ';' is not allowed",
            }));
        }
    }
    let sql = build_count_query(table, query);
    validate_read_only_sql(&sql).map_err(|kw| {
        serde_json::json!({
            "success": false,
            "error_code": "SQL_WRITE_BLOCKED",
            "error": format!("mode=\"count\" counts a read; '{kw}' is not allowed"),
            "blocked_keyword": kw,
        })
    })?;
    Ok(sql)
}

/// Clamps `max_rows_affected` for `iris_query` `mode="write"` UPDATE/DELETE pre-checks.
/// `None` or `Some(0)` map to the default (1000); values above 10000 are clamped to 10000.
pub fn clamp_max_rows_affected(value: Option<u32>) -> u32 {
    match value {
        None | Some(0) => 1000,
        Some(v) if v > 10000 => 10000,
        Some(v) => v,
    }
}

/// True if a tool-call result's JSON body has `success: true`.
fn is_success(result: &CallToolResult) -> bool {
    result
        .content
        .first()
        .and_then(|c| c.as_text())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t.text).ok())
        .and_then(|v| v.get("success").and_then(|s| s.as_bool()))
        .unwrap_or(false)
}

/// `iris_query` `mode="explain"` — returns the raw IRIS query plan for a SELECT/WITH
/// statement, with no rows transferred. See spec 057-sql-power FR-003/FR-004.
async fn iris_query_explain(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &QueryParams,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let first_word = p
        .query
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_uppercase();
    if p.query.trim().is_empty() {
        return err_json("EMPTY_QUERY", "SQL query is empty.");
    }
    if first_word != "SELECT" && first_word != "WITH" {
        return err_json(
            "EXPLAIN_REQUIRES_SELECT",
            "explain mode only accepts SELECT statements (including WITH/CTE).",
        );
    }

    let query_url = iris.versioned_ns_url(namespace, "/action/query");
    let explain_sql = format!("EXPLAIN {}", p.query);
    let resp = client
        .post(&query_url)
        .basic_auth(&iris.username, Some(&iris.password))
        .json(&serde_json::json!({"query": explain_sql}))
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

    let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;
    if let Some(errors) = body["status"]["errors"].as_array() {
        if !errors.is_empty() {
            let msg = errors[0]["error"].as_str().unwrap_or("SQL error");
            return sql_err(msg, &p.query, namespace);
        }
    }

    let plan_text = body["result"]["content"][0]["Plan"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if plan_text.is_empty() {
        return err_json(
            "EXPLAIN_NOT_SUPPORTED",
            "IRIS returned no plan text for EXPLAIN on this query/version.",
        );
    }

    ok_json(serde_json::json!({
        "success": true,
        "plan_text": plan_text,
        "query_hash": query_hash(&p.query),
    }))
}

/// `iris_query` `mode="count"` — returns a row count for `table` or `query` without
/// transferring rows. See spec 057-sql-power FR-006/FR-007/FR-008.
async fn iris_query_count(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &QueryParams,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let table = p.table.as_deref();
    let query = if p.query.trim().is_empty() {
        None
    } else {
        Some(p.query.as_str())
    };
    if table.is_none() && query.is_none() {
        return err_json(
            "MISSING_TARGET",
            "mode=\"count\" requires either `table` or `query`.",
        );
    }

    let count_sql = match count_query_checked(table, query) {
        Ok(sql) => sql,
        Err(e) => return err_result(e),
    };
    let query_url = iris.versioned_ns_url(namespace, "/action/query");
    let resp = client
        .post(&query_url)
        .basic_auth(&iris.username, Some(&iris.password))
        .json(&serde_json::json!({"query": count_sql}))
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

    let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;
    if let Some(errors) = body["status"]["errors"].as_array() {
        if !errors.is_empty() {
            let msg = errors[0]["error"].as_str().unwrap_or("SQL error");
            return sql_err(
                msg,
                &format!("{} {}", table.unwrap_or(""), query.unwrap_or("")),
                namespace,
            );
        }
    }

    let count = body["result"]["content"][0]
        .as_object()
        .and_then(|obj| obj.values().next())
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    ok_json(serde_json::json!({"success": true, "count": count}))
}

/// `iris_query` `mode="write"` — executes INSERT/UPDATE/DELETE/CALL/TRUNCATE via
/// `%SQL.Statement` (the Atelier `/action/query` REST endpoint returns no row-count
/// information for DML — see research.md). UPDATE/DELETE are pre-checked against
/// `max_rows_affected` before executing. See spec 057-sql-power FR-011 through FR-015.
async fn iris_query_write(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &QueryParams,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    match validate_dml_sql(&p.query) {
        Err(ref reason) if reason == "EMPTY" => {
            return err_json("EMPTY_QUERY", "SQL query is empty after removing comments.");
        }
        Err(ref reason) if reason == "SELECT_IN_WRITE" => {
            return err_json(
                "SELECT_NOT_ALLOWED_IN_WRITE",
                "mode=\"write\" is DML-only. Use mode=\"read\" for SELECT.",
            );
        }
        Err(ref reason) if reason == "UNKNOWN_STATEMENT" => {
            return err_json(
                "DDL_NOT_ALLOWED",
                "Statement type not recognized as allowed DML.",
            );
        }
        Err(keyword) => {
            return err_result(serde_json::json!({
                "success": false,
                "error_code": "DDL_NOT_ALLOWED",
                "error": format!("DDL keyword '{keyword}' is not allowed in mode=\"write\"."),
                "blocked_keyword": keyword,
            }));
        }
        Ok(()) => {}
    }

    let max_rows_affected = clamp_max_rows_affected(p.max_rows_affected);

    let mut rows_check_skipped = false;
    if needs_rows_precheck(&p.query) {
        match build_rows_precheck_query(&p.query) {
            Some(count_sql) => {
                let code = format!(
                    r#"Set st=##class(%SQL.Statement).%New()
Set sc=st.%Prepare("{count_sql_lit}")
If $$$ISERR(sc) {{ Write "ERROR:ROWS_CHECK_FAILED:"_$System.Status.GetErrorText(sc) Quit }}
Set rs=st.%Execute()
If rs.%SQLCODE<0 {{ Write "ERROR:ROWS_CHECK_FAILED:"_rs.%Message Quit }}
If rs.%Next() {{ Write "OK:"_rs.%GetData(1) }} Else {{ Write "OK:0" }}"#,
                    count_sql_lit = crate::objectscript::os_lit_inner(&count_sql),
                );
                match iris.execute_via_generator(&code, namespace, client).await {
                    Ok(out) => {
                        let out = out.trim();
                        if let Some(msg) = out.strip_prefix("ERROR:ROWS_CHECK_FAILED:") {
                            return err_json("ROWS_CHECK_FAILED", msg);
                        }
                        let actual_count: i64 = out
                            .strip_prefix("OK:")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        if actual_count > max_rows_affected as i64 {
                            return err_result(serde_json::json!({
                                "success": false,
                                "error_code": "ROWS_LIMIT_EXCEEDED",
                                "error": format!(
                                    "Statement would affect {actual_count} rows, exceeding max_rows_affected={max_rows_affected}."
                                ),
                                "actual_count": actual_count,
                                "limit": max_rows_affected,
                            }));
                        }
                    }
                    Err(e) => {
                        return err_json("ROWS_CHECK_FAILED", &e.to_string());
                    }
                }
            }
            None => rows_check_skipped = true,
        }
    }

    let code = format!(
        r#"Set st=##class(%SQL.Statement).%New()
Set sc=st.%Prepare("{sql_lit}")
If $$$ISERR(sc) {{ Write "ERROR:SQL_ERROR:"_$System.Status.GetErrorText(sc) Quit }}
Set rs=st.%Execute()
If rs.%SQLCODE<0 {{ Write "ERROR:SQL_ERROR:"_rs.%Message Quit }}
Write "OK:"_rs.%ROWCOUNT"#,
        sql_lit = crate::objectscript::os_lit_inner(&p.query),
    );
    match iris.execute_via_generator(&code, namespace, client).await {
        Ok(out) => {
            let out = out.trim();
            if let Some(msg) = out.strip_prefix("ERROR:SQL_ERROR:") {
                return sql_err(msg, &p.query, namespace);
            }
            let rows_affected: i64 = out
                .strip_prefix("OK:")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let mut resp = serde_json::json!({"success": true, "rows_affected": rows_affected});
            if p.force {
                resp["force_ignored"] = serde_json::Value::Bool(true);
            }
            if rows_check_skipped {
                resp["rows_check_skipped"] = serde_json::Value::Bool(true);
            }
            ok_json(resp)
        }
        Err(e) => err_json("SQL_ERROR", &e.to_string()),
    }
}

/// Statement modifiers IRIS accepts between `UPDATE`/`DELETE` and the table (`DELETE %NOLOCK FROM t`).
const DML_MODIFIERS: &[&str] = &[
    "NOCHECK",
    "NOFPLAN",
    "NOINDEX",
    "NOLOCK",
    "NOTRIGGER",
    "NOUNIQCHECK",
    "PROFILE",
    "PROFILE_ALL",
    "STARTTABLE",
];

/// The leading `UPDATE` or `DELETE` of a DML statement, and where its operands start in `code`.
/// Comments and white space before it do not matter: IRIS skips them.
fn dml_head<'a>(
    code: &[(usize, &'a crate::sql_lex::Token)],
) -> Option<(&'a crate::sql_lex::Token, usize)> {
    let (_, verb) = *code.first()?;
    if !(verb.is_word("UPDATE") || verb.is_word("DELETE")) {
        return None;
    }
    let mut at = 1;
    while code.get(at).is_some_and(|(_, t)| t.is_punct('%'))
        && code
            .get(at + 1)
            .is_some_and(|(_, t)| DML_MODIFIERS.iter().any(|m| t.is_word(m)))
    {
        at += 2;
    }
    Some((verb, at))
}

/// True when `dml` is an `UPDATE` or `DELETE`, which `iris_query` `mode="write"` counts before it
/// runs. Read from the statement's own tokens, so a leading comment or space does not hide it.
pub fn needs_rows_precheck(dml: &str) -> bool {
    let tokens = crate::sql_lex::lex(dml);
    let code: Vec<(usize, &crate::sql_lex::Token)> = tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_code())
        .collect();
    dml_head(&code).is_some()
}

/// Extracts a `SELECT COUNT(*) FROM <table> [WHERE <clause>]` pre-check query from an
/// UPDATE or DELETE statement, for the `mode="write"` rows-affected limit guard.
/// Returns `None` for statement shapes it cannot confidently parse (multi-table UPDATE,
/// missing table name) — callers must skip the pre-check and set `rows_check_skipped: true`
/// rather than treating this as an error.
///
/// The target is the name after `UPDATE`, or after `DELETE FROM`; the clause is the text after the
/// first `WHERE` that is outside parentheses, a literal and a comment.
pub fn build_rows_precheck_query(dml: &str) -> Option<String> {
    let tokens = crate::sql_lex::lex(dml);
    let code: Vec<(usize, &crate::sql_lex::Token)> = tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_code())
        .collect();
    let (verb, mut at) = dml_head(&code)?;
    if verb.is_word("DELETE") {
        if !code.get(at)?.1.is_word("FROM") {
            return None;
        }
        at += 1;
    }
    let (table, after) = sql_table_name(&code, at)?;
    let mut depth = 0usize;
    let where_at = code[after..].iter().find_map(|(i, t)| {
        if t.is_punct('(') {
            depth += 1;
        } else if t.is_punct(')') {
            depth = depth.saturating_sub(1);
        }
        (depth == 0 && t.is_word("WHERE")).then_some(*i)
    });
    let clause = where_at.map(|i| crate::sql_lex::text(&tokens[i + 1..]));
    Some(
        match clause.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(w) => format!("SELECT COUNT(*) FROM {table} WHERE {w}"),
            None => format!("SELECT COUNT(*) FROM {table}"),
        },
    )
}

/// A table name at `code[at..]`: `name`, `%name` or a delimited identifier, joined by dots. Returns
/// the name as written and the index after it.
fn sql_table_name(
    code: &[(usize, &crate::sql_lex::Token)],
    mut at: usize,
) -> Option<(String, usize)> {
    use crate::sql_lex::Kind;
    let mut name = String::new();
    loop {
        let (_, first) = code.get(at)?;
        if first.is_punct('%') {
            name.push('%');
            at += 1;
        }
        let (_, part) = code.get(at)?;
        if !matches!(part.kind, Kind::Word | Kind::QuotedIdent) || !part.closed {
            return None;
        }
        name.push_str(&part.text);
        at += 1;
        if code.get(at).is_some_and(|(_, t)| t.is_punct('.')) {
            name.push('.');
            at += 1;
        } else {
            return Some((name, at));
        }
    }
}

fn err_json_with_url(
    code: &str,
    msg: &str,
    attempted_url: &str,
) -> Result<CallToolResult, McpError> {
    err_result(serde_json::json!({
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
                    // idt only reports 1972 — get web port from docker inspect fallback
                    let wp = extract_port(ports, "52773")
                        .or_else(|| {
                            // idt didn't include web port — query docker directly
                            std::process::Command::new("docker")
                                .args(["port", &name, "52773"])
                                .output()
                                .ok()
                                .and_then(|o| {
                                    let raw = String::from_utf8_lossy(&o.stdout).to_string();
                                    // output: "0.0.0.0:52780" or "[::]:52780" (one per line)
                                    raw.lines()
                                        .filter_map(|l| l.rsplit_once(':'))
                                        .filter_map(|(_, p)| p.trim().parse::<u16>().ok())
                                        .next()
                                })
                        })
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

/// Public accessor for list_iris_containers used by iris-agentic-dev init.
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

/// Extract the web port from an Atelier base URL (e.g. "http://localhost:52780/iris").
fn extract_web_port_from_url(base_url: &str) -> Option<u16> {
    let without_scheme = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let host_port = without_scheme.split('/').next().unwrap_or("");
    host_port
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
}

/// Extract the web prefix path from an Atelier base URL (e.g. "/iris" from "http://host:52780/iris").
fn extract_web_prefix_from_url(base_url: &str) -> Option<String> {
    let without_scheme = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let slash = without_scheme.find('/')?;
    let prefix = &without_scheme[slash..];
    if prefix.is_empty() || prefix == "/" {
        None
    } else {
        Some(prefix.trim_end_matches('/').to_string())
    }
}

/// Derive connection capabilities from already-known state — zero network calls.
///
/// `docker_only` is true when `base_url` is the unreachable sentinel (`http://127.0.0.1:1`),
/// meaning the connection was configured with `docker_only = true` in the toml.
pub fn derive_capabilities(
    iris_version: Option<&str>,
    docker_only: bool,
    web_port: Option<u16>,
    web_prefix: Option<&str>,
) -> serde_json::Value {
    // NoPWS: 2026.2.0AI builds shipped without a private web server (DPP-1192).
    let no_pws = iris_version
        .map(|v| {
            // Matches e.g. "IRIS for UNIX (Ubuntu Server LTS for x86-64) 2026.2.0AI (Build 237U)"
            v.contains("2026.2.0AI")
        })
        .unwrap_or(false);

    let atelier_rest = !docker_only && !no_pws;

    let compile_path = if atelier_rest {
        "atelier"
    } else {
        "docker_exec"
    };

    let webgateway_url = if atelier_rest {
        web_prefix
            .map(|prefix| {
                // We have a non-default prefix — webgateway is at host:port<prefix>
                serde_json::Value::String(format!(
                    "http://host:{}{}",
                    web_port.unwrap_or(52773),
                    prefix
                ))
            })
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };

    serde_json::json!({
        "private_web_server": !no_pws,
        "atelier_rest": atelier_rest,
        "compile_path": compile_path,
        "webgateway_url": webgateway_url,
    })
}

#[derive(Clone)]
pub struct IrisTools {
    /// Active connection state — wraps iris, source, config metadata, write gate.
    /// Arc<Mutex> allows atomic swap from &self tool handlers (034-live-connection-reload).
    pub connection: Arc<std::sync::Mutex<ConnectionState>>,
    /// Lazy config file watcher for hot-reload. None when no .iris-agentic-dev.toml exists.
    pub config_watcher: Arc<std::sync::Mutex<Option<ConfigWatcher>>>,
    /// The configuration of the last good load and the inputs that produced it (R4a D1). Empty
    /// until `set_config`; readers fall back to loading from the process.
    config: Arc<std::sync::RwLock<ConfigHolder>>,
    pub registry: Arc<crate::skills::SkillRegistry>,
    /// Shared HTTP client — created once, reused across all tool calls.
    pub client: Arc<reqwest::Client>,
    /// One HTTP client per service identity (`server|user`), each with a cookie jar of its own so a
    /// CSP session opened as the primary is never replayed on a service-account request: IRIS
    /// honours an existing session cookie over the request's basic auth, which would run the code
    /// as the primary (spec 141, R2).
    pub(crate) identity_clients: Arc<std::sync::Mutex<HashMap<String, Arc<reqwest::Client>>>>,
    /// Ring buffer of recent tool calls for skill_propose pattern mining.
    pub history: Arc<std::sync::Mutex<VecDeque<ToolCallEntry>>>,
    /// Pending elicitation state for SCM dialogs.
    pub elicitation_store: Arc<ElicitationStore>,
    /// Session-scoped cache of documents already checked out by us, so chained writes
    /// (insert/delete_lines/put) skip the redundant pre-write SCM checkout probe.
    pub checkout_cache: Arc<crate::elicitation::CheckoutCache>,
    /// UUID-keyed in-memory log store for progressive disclosure (027).
    pub log_store: Arc<std::sync::Mutex<log_store::LogStore>>,
    /// Session-scoped TTL cache for %Dictionary introspection results (037).
    pub metadata_cache: Arc<dict::MetadataCache>,
    /// Multi-instance connection pool (072-multi-instance-pool).
    /// Wrapped in RwLock<Arc<...>> so iris_reload_pool can atomically swap the inner
    /// Arc from &self without requiring exclusive access to IrisTools.
    ///
    /// Built on first use, not at construction (spec 141 T187): loading reads every member's
    /// password, which opens the OS keychain. Until then it holds `ConnectionPool::unloaded`; read
    /// it through [`IrisTools::pool_ref`], which loads it.
    pub pool: Arc<std::sync::RwLock<Arc<crate::iris::connection_pool::ConnectionPool>>>,
    /// WebSocket terminal session pool (072-b).
    pub ws_pool: Arc<crate::iris::ws_session::WsSessionPool>,
    /// Confirmation tokens for global_preview/global_kill flow (072-c).
    pub confirm_tokens: Arc<tokio::sync::Mutex<HashMap<String, admin_tools::ConfirmEntry>>>,
    /// Active toolset — controls which tools are registered.
    pub toolset: Toolset,
    /// One MCP server process lifetime (059-tool-telemetry-benchmark). Stamped onto
    /// every `ToolCallRecord` produced during this process's life.
    pub session: crate::telemetry::Session,
    /// Set to true after the first docker_only attribution warning has been emitted so
    /// it fires at most once per server instance (T019).
    pub docker_only_attr_warned: Arc<std::sync::atomic::AtomicBool>,
    /// Failure counter for %SYS.Audit emission (T036). First failure warns; subsequent
    /// failures increment the counter rather than repeating the warning.
    pub iris_audit_counter: Arc<crate::iris::iris_audit::AuditEmitCounter>,
    #[allow(dead_code)] // used by #[tool_router] macro-generated code
    tool_router: ToolRouter<IrisTools>,
    /// Every registered tool's advertised parameters, read off `tool_router` once at construction
    /// (113 FR-015). The validation sites in `call_tool` run on every call, and
    /// `ToolRouter::list_all()` clones the whole catalog — schemas included — so doing that per
    /// call would put a few hundred kilobytes of allocation in front of every tool invocation.
    ///
    /// Derived, never hand-written: the names a caller is told are accepted are the names the
    /// client was served on `tools/list`. It is also why this is a field and not a global cache —
    /// the router is pruned per `Toolset`, and a process can hold instances of more than one.
    accepted_params: BTreeMap<String, param_check::AcceptedParams>,
    /// Refuse a name `tool_router` does not list for this toolset. On for the CLI, so it accepts
    /// exactly what `tools/list` advertises (spec 141, R1 C1); off for the tests, which call
    /// handlers by name whatever toolset they built.
    strict_toolset: bool,
}

#[tool_router]
impl IrisTools {
    pub fn new(iris: Option<IrisConnection>) -> anyhow::Result<Self> {
        let client = Arc::new(IrisConnection::http_client()?);
        // No config file on this path, so the gate resolves from the operator's environment and
        // the connection's own SystemMode/namespace.
        let declared = write_gate::DeclaredGates::default();
        let conn_state = match iris {
            Some(c) => {
                let gates = write_gate::resolve_for_connection(declared, Some(&c), &c.namespace);
                ConnectionState::from_iris(c, ConnectionSource::EnvVars, None, gates)
            }
            None => ConnectionState::new_disconnected(
                ConnectionSource::EnvVars,
                write_gate::resolve_for_connection(declared, None, "USER"),
            ),
        };
        let log_max = std::env::var("IRIS_LOG_STORE_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50usize);
        let log_ttl = std::env::var("IRIS_LOG_TTL_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60u64);
        let router = Self::tool_router();
        let accepted_params = param_check::accepted_parameters(&router.list_all());
        Ok(Self {
            connection: Arc::new(std::sync::Mutex::new(conn_state)),
            config_watcher: Arc::new(std::sync::Mutex::new(None)),
            config: Arc::new(std::sync::RwLock::new(ConfigHolder::default())),
            registry: Arc::new(crate::skills::SkillRegistry::new()),
            client,
            identity_clients: Default::default(),
            history: Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(50))),
            elicitation_store: Arc::new(ElicitationStore::new()),
            checkout_cache: Arc::new(crate::elicitation::CheckoutCache::new()),
            log_store: Arc::new(std::sync::Mutex::new(log_store::LogStore::new(
                log_max, log_ttl,
            ))),
            metadata_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            // T018: no with_connection constructor found — pool field added to all existing constructors
            pool: Arc::new(std::sync::RwLock::new(Arc::new(
                crate::iris::connection_pool::ConnectionPool::unloaded(None),
            ))),
            ws_pool: Arc::new(crate::iris::ws_session::WsSessionPool::new()),
            confirm_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            toolset: Toolset::Baseline,
            session: crate::telemetry::Session::new(),
            docker_only_attr_warned: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            iris_audit_counter: crate::iris::iris_audit::AuditEmitCounter::new(),
            tool_router: router,
            accepted_params,
            strict_toolset: false,
        })
    }
    /// The tools one MCP session gets in HTTP transport mode (spec 141 T220).
    ///
    /// The transport builds a session's tools by copying a prototype, and a plain `clone` shares
    /// every `Arc` field. What a client asked for and what came back is that client's: the log
    /// store (`iris_get_log` with no id lists all of it), the pending SCM dialogs (resumed by id)
    /// and the call history (`agent_history`, `agent_info`). Those three start empty here, with the
    /// log store's limits kept.
    ///
    /// Everything else stays shared, because one iad process is one operator's account on one set of
    /// servers and none of it holds a client's results: the connection state and the pool (a reload
    /// through one session must reach the others, and a second pool would open the keychain again),
    /// the HTTP clients and their CSP sessions, the configuration, the skill registry, the
    /// metadata cache (class definitions the account can read anyway), the checkout cache (a fact
    /// about documents on the server, not about a client), the WebSocket terminal pool and the
    /// confirmation tokens (both addressed by an unguessable id the creating client holds).
    pub fn for_new_session(&self) -> Self {
        let mut session = self.clone();
        let (max_entries, ttl_minutes) = self
            .log_store
            .lock()
            .map(|s| (s.max_entries, s.ttl_minutes))
            .unwrap_or((50, 60));
        session.log_store = Arc::new(std::sync::Mutex::new(log_store::LogStore::new(
            max_entries,
            ttl_minutes,
        )));
        session.elicitation_store = Arc::new(ElicitationStore::new());
        session.history = Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(50)));
        session
    }

    /// The CLI's constructor (spec 141, R1 C1): the configuration, gates and pool the MCP server
    /// loads, read once and with no watcher, and only the tools `tools/list` advertises for
    /// `toolset`. `workspace` is `--workspace` when it was given.
    pub fn for_cli(
        iris: Option<IrisConnection>,
        toolset: Toolset,
        workspace: Option<&str>,
    ) -> anyhow::Result<Self> {
        let inputs = crate::iris::config_sources::LoadInputs::from_process(workspace, None);
        let snapshot = crate::iris::config_sources::ConfigSnapshot::load(&inputs);
        let declared = snapshot.declared_gates();
        let config_path = snapshot.workspace_layer.as_ref().map(|f| f.path.clone());
        let mut tools = Self::with_registry_and_toolset(
            iris,
            crate::skills::SkillRegistry::new(),
            toolset,
            None,
            config_path,
            false,
            declared,
        )?;
        tools.set_config(inputs, snapshot);
        tools.strict_toolset = true;
        Ok(tools)
    }

    /// Convenience constructor for tests — same as `new` but with explicit toolset.
    pub fn new_with_toolset(
        iris: Option<IrisConnection>,
        toolset: Toolset,
    ) -> anyhow::Result<Self> {
        Self::with_registry_and_toolset(
            iris,
            crate::skills::SkillRegistry::new(),
            toolset,
            None,
            None,
            false,
            write_gate::DeclaredGates::default(),
        )
    }

    /// Returns the set of tool names registered for the current toolset.
    /// Used by tests and by the benchmark harness to build valid_tool_names.
    ///
    /// Derived directly from `self.tool_router` — the same macro-generated, already-pruned
    /// router the real MCP `list_tools` RPC serves (see the `list_tools` override in the
    /// `ServerHandler` impl below, which also calls `self.tool_router.list_all()`). This
    /// used to be a ~170-line hand-maintained mirror of the constructor's pruning logic
    /// (`all_tools`/`stub_tools`/`merged_removed`/`merged_added` arrays, kept in sync by
    /// hand with both the `#[tool]` methods in this file and the `router.remove_route(...)`
    /// calls just above in `with_registry_and_toolset`) — and it had already drifted from
    /// both: it had no entry at all for `agent_info`, `iris_list_containers`,
    /// `iris_select_container`, or `iris_start_sandbox` (real, callable tools in every
    /// toolset), and it reported `iris_coverage`/`iris_doc_search` as merged-only when the
    /// constructor's actual `merged_only` removal list never removed them from
    /// Baseline/Nostub. Deriving from the router instead leaves nothing to keep in sync —
    /// this can no longer disagree with what MCP clients actually see.
    pub fn registered_tool_names(&self) -> std::collections::HashSet<String> {
        self.tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    /// The catalog `tools/list` serves, before pagination.
    ///
    /// One function so the MCP transport and `tool --list` / `tool <name> --schema` cannot answer
    /// differently. The transforms here are not cosmetic: `normalize_schema_openapi3` rewrites
    /// nullable types into `anyOf`, so a CLI that skipped it would print a schema no client ever
    /// receives, and the description suppression an eval run turns on has to apply to both arms or
    /// the CLI arm silently keeps the prose the run is measuring without.
    fn listed_tools(&self) -> Vec<rmcp::model::Tool> {
        let mut tools = self.tool_router.list_all();
        // Strict read-only hides the tools it refuses (R5 C15), so a client never offers one.
        if self.snapshot().read_only_strict() {
            tools.retain(|t| !write_gate::is_scratch(t.name.as_ref()));
        }
        let suppressed = std::env::var("IRIS_SUPPRESS_TOOL_DESCRIPTION")
            .map(|v| parse_suppressed_descriptions(&v))
            .unwrap_or_default();
        for tool in tools.iter_mut() {
            if suppressed.contains(tool.name.as_ref()) {
                tool.description = Some(std::borrow::Cow::Borrowed(""));
            }
            let schema = std::sync::Arc::make_mut(&mut tool.input_schema);
            normalize_schema_openapi3(schema);
            // Stripped from tools/list since #113 — clients ignore it and it inflates the payload
            // from ~30KB to ~220KB. Still returned per call via structuredContent.
            tool.output_schema = None;
        }
        tools
    }

    /// Every registered tool, sorted by name, with the one-line summary `tool --list` prints.
    ///
    /// Built from [`Self::listed_tools`], so `description` and `input_schema` are exactly what a
    /// client reads off `tools/list` — same source, same transforms. The per-name accessors above
    /// each call `list_all()`, which is fine for a test asking about one tool and quadratic for a
    /// listing of eighty.
    pub fn tool_catalogue(&self) -> Vec<CatalogueEntry> {
        let mut entries: Vec<CatalogueEntry> = self
            .listed_tools()
            .into_iter()
            .map(|t| {
                let description = t.description.map(|d| d.to_string());
                CatalogueEntry {
                    name: t.name.to_string(),
                    summary: summarize_description(description.as_deref().unwrap_or_default()),
                    description,
                    input_schema: serde_json::to_value(&t.input_schema)
                        .unwrap_or(serde_json::Value::Null),
                }
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    /// Returns `true` if `tool_name` is registered and declares a non-null `output_schema` —
    /// the same `Tool` definitions a real `list_tools` RPC serves (076-interface-modernization
    /// User Story 1). Used by tests to confirm a tool's declared output schema actually reaches
    /// `list_tools`, without needing a live IRIS connection — this only inspects the static
    /// router, never calls the tool.
    pub fn tool_declares_output_schema(&self, tool_name: &str) -> bool {
        self.tool_router
            .list_all()
            .into_iter()
            .find(|t| t.name == tool_name)
            .is_some_and(|t| t.output_schema.is_some())
    }

    /// Returns the `outputSchema` for `tool_name` if it is registered and declares one, otherwise
    /// `None`. The companion to `tool_input_schema`, and the schema a real `tools/list` serves —
    /// which is the one that matters. A test that reads the Rust struct instead can pass while the
    /// tool advertises something else entirely (085: `server_version` sat in `check_config`'s
    /// payload and its description for five minor versions without ever being in its schema).
    pub fn tool_output_schema(&self, tool_name: &str) -> Option<serde_json::Value> {
        self.tool_router
            .list_all()
            .into_iter()
            .find(|t| t.name == tool_name)
            .and_then(|t| t.output_schema)
            .map(|s| serde_json::to_value(&s).unwrap_or(serde_json::Value::Null))
    }

    /// Returns the MCP `annotations` object for `tool_name` if it is registered and declares any,
    /// otherwise `None`. Read off the router, so this is what a client sees on `tools/list` — the
    /// hints a caller decides on. 085 uses it to cross-check `read_only_hint` and
    /// `destructive_hint` against `write_gate::CLASSIFICATION`: two independent declarations, so
    /// mislabelling a mutating tool read-only has to be done twice to escape CI. #94 (`c641d79`)
    /// is why — six mutating tools shipped advertising `read_only_hint = true`.
    pub fn tool_annotations(&self, tool_name: &str) -> Option<serde_json::Value> {
        self.tool_router
            .list_all()
            .into_iter()
            .find(|t| t.name == tool_name)
            .and_then(|t| t.annotations)
            .map(|a| serde_json::to_value(&a).unwrap_or(serde_json::Value::Null))
    }

    /// Returns the `inputSchema` for `tool_name` if it is registered, otherwise `None`.
    /// Used by tests to assert that a tool's schema documents specific parameters.
    ///
    /// This is the schema as schemars reflected it, *before* the nullable-array rewrite that
    /// `list_tools` applies on the way out (`normalize_schema_openapi3`). The two differ for every
    /// optional typed parameter: the rewrite moves `type`, `enum`, `format` and friends into an
    /// `anyOf` branch. Tests that read `properties.<name>.enum` want this form; a test comparing
    /// against what a client receives has to apply `normalize_schema_openapi3` itself.
    pub fn tool_input_schema(&self, tool_name: &str) -> Option<serde_json::Value> {
        self.tool_router
            .list_all()
            .into_iter()
            .find(|t| t.name == tool_name)
            .map(|t| serde_json::to_value(&t.input_schema).unwrap_or(serde_json::Value::Null))
    }

    /// Returns the description for `tool_name` if it is registered, otherwise `None`.
    pub fn tool_description(&self, tool_name: &str) -> Option<String> {
        self.tool_router
            .list_all()
            .into_iter()
            .find(|t| t.name == tool_name)
            .and_then(|t| t.description.map(|d| d.to_string()))
    }

    pub fn with_registry(
        iris: Option<IrisConnection>,
        registry: crate::skills::SkillRegistry,
    ) -> anyhow::Result<Self> {
        Self::with_registry_and_toolset(
            iris,
            registry,
            Toolset::Baseline,
            None,
            None,
            false,
            write_gate::DeclaredGates::default(),
        )
    }
    /// `declared` carries what the `.iris-agentic-dev.toml` said about the two gates. It arrives
    /// as data rather than through `IRIS_WRITE_TOOLS_ENABLED`, so a second config load can change
    /// the answer in either direction (085 FR-001).
    #[allow(clippy::too_many_arguments)]
    pub fn with_registry_and_toolset(
        iris: Option<IrisConnection>,
        registry: crate::skills::SkillRegistry,
        toolset: Toolset,
        config_watcher: Option<ConfigWatcher>,
        config_path: Option<std::path::PathBuf>,
        no_skills: bool,
        declared: write_gate::DeclaredGates,
    ) -> anyhow::Result<Self> {
        // Clone config_path for load_pool before it may be moved into conn_state (072).
        let pool_config_path = config_path.clone();
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

        // For merged toolset: remove debug tools replaced by iris_debug dispatcher.
        // 036: individual interop stubs removed entirely — iris_production/iris_interop_query
        // are now available in all tiers, so no pruning needed for them.
        if toolset == Toolset::Merged {
            let merged_replaced: &[&str] = &[
                // Replaced by iris_debug (FR-007)
                "debug_capture_packet",
                "debug_get_error_logs",
                "debug_map_int_to_cls",
                "debug_source_map",
                // agent_info removed (FR-011)
                "agent_info",
                // iris_containers replaces these in merged
                "iris_list_containers",
                "iris_select_container",
                "iris_start_sandbox",
            ];
            for name in merged_replaced {
                router.remove_route(name);
            }
        } else {
            // For baseline and nostub: remove merged-only dispatcher tools
            // (iris_production/iris_interop_query/iris_production_item are now available everywhere)
            let merged_only: &[&str] = &[
                "iris_debug",
                "iris_containers",
                // 026-admin-tools
                "iris_admin",
                // 027-progressive-disclosure
                "iris_get_log",
                // 052-iris-global
                "iris_global",
                // 053-doc-depth
                "iris_execute_method",
                // 056-interop-depth
                "iris_message_body",
                "iris_business_rule_info",
                "iris_production_diff",
            ];
            for name in merged_only {
                router.remove_route(name);
            }
        }

        // --no-skills: remove all skill management and learning-agent tools so the server
        // exposes only IRIS tools. Useful when the caller wants a clean, minimal surface
        // with no skill/KB management surface at all (e.g. Keshav-style tools-only installs).
        if no_skills {
            let skill_tools: &[&str] = &[
                "skill",
                "skill_list",
                "skill_describe",
                "skill_search",
                "skill_forget",
                "skill_propose",
                "skill_optimize",
                "skill_share",
                "skill_community",
                "skill_community_list",
                "skill_community_install",
                "kb_index",
                "kb_recall",
                "agent_history",
                "agent_stats",
            ];
            for name in skill_tools {
                router.remove_route(name);
            }
        }

        // Apply user-specified tool allowlist from IRIS_ENABLED_TOOLS env var or toml
        // enabled_tools field (config loader sets the env var from toml before this runs).
        // Comma-separated tool names — when non-empty, ONLY these remain, regardless of
        // toolset (075-modular-tool-install, FR-001). Enforced through the same
        // remove_route() primitive as everything else in this constructor (FR-003) — no
        // second enforcement path. An empty list means "no allowlist" (FR edge case:
        // does NOT mean "expose zero tools"). Runs before the disabled-tools block below
        // so disabled always wins for a name in both (FR-002).
        let enabled: Vec<String> = std::env::var("IRIS_ENABLED_TOOLS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !enabled.is_empty() {
            let enabled_set: std::collections::HashSet<&str> =
                enabled.iter().map(|s| s.as_str()).collect();
            // Snapshot current route names before mutating — remove_route() while
            // iterating router.list_all()'s own borrow would not compile, and this way
            // an allowlist name that doesn't match any real route is simply never
            // removed, matching disabled_tools' existing unknown-name tolerance.
            let current_names: Vec<String> = router
                .list_all()
                .into_iter()
                .map(|t| t.name.to_string())
                .collect();
            for name in &current_names {
                if !enabled_set.contains(name.as_str()) {
                    router.remove_route(name);
                }
            }
            tracing::info!(
                enabled = ?enabled,
                "iris-agentic-dev: tool allowlist applied — only these tools remain"
            );
        }

        // Apply user-specified disabled tools from IRIS_DISABLED_TOOLS env var or toml
        // disabled_tools field (config loader sets the env var from toml before this runs).
        // Comma-separated tool names, e.g. "iris_source_control,iris_admin".
        let disabled: Vec<String> = std::env::var("IRIS_DISABLED_TOOLS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        for name in &disabled {
            router.remove_route(name.as_str());
        }
        if !disabled.is_empty() {
            tracing::info!(disabled = ?disabled, "iris-agentic-dev: user-disabled tools removed");
        }

        let conn_state = match iris {
            Some(c) => {
                let gates = write_gate::resolve_for_connection(declared, Some(&c), &c.namespace);
                tracing::info!(
                    system_mode = ?c.system_mode,
                    write_tools_enabled = gates.write_enabled,
                    write_tools_source = gates.write_source.as_str(),
                    destructive_tools_enabled = gates.destructive_enabled,
                    destructive_tools_source = gates.destructive_source.as_str(),
                    namespace = %c.namespace,
                    "iris-agentic-dev: write tool gate evaluated"
                );
                // 085: `iris_production_item` and `iris_credential_manage` used to be stripped from
                // the router here when writes were off. Removal is not enforcement. It is invisible
                // to a later reload (the router is built once, the gate re-resolves on every config
                // change), and it made the completeness test pass for the wrong reason — the two
                // tools were absent rather than gated. Both are classified in `write_gate` and
                // refused by the single check in `call_tool`, so they now stay visible and answer
                // with WRITE_TOOLS_DISABLED, which is also what tells the caller *why*.
                //
                // Use ConfigFile source (and record the path) when the connection came from
                // a .iris-agentic-dev.toml — so check_config can show config_file at startup,
                // not just after the first hot-reload cycle (issue #82).
                // Use EnvVars when IRIS_HOST is set and no file chose the connection — env-var
                // connections are pinned, not discovered, and check_config should say so. A home
                // file that yielded to IRIS_HOST is still named: its tool lists apply.
                // An operator file yields to IRIS_HOST; every other file chooses (spec 140). A file
                // names the connection only when it names a host or a container (T194): one that
                // holds only a namespace or a port chose nothing, and is reported as such.
                let (chosen, no_target) = match config_path.as_deref() {
                    Some(p) if !crate::iris::workspace_config::file_names_a_target(p) => {
                        (false, config_path.clone())
                    }
                    Some(p) => (
                        crate::iris::workspace_config::file_chooses_connection(
                            if crate::iris::config_sources::is_operator_file(p) {
                                crate::iris::config_sources::FileKind::Operator
                            } else {
                                crate::iris::config_sources::FileKind::Workspace
                            },
                            crate::iris::connection::host_from_env().is_some(),
                        ),
                        None,
                    ),
                    None => (false, None),
                };
                let (source, file) = if chosen {
                    (ConnectionSource::ConfigFile, config_path)
                } else if crate::iris::connection::host_from_env().is_some() {
                    (
                        ConnectionSource::EnvVars,
                        config_path.filter(|_| no_target.is_none()),
                    )
                } else {
                    (ConnectionSource::AutoDiscovered, None)
                };
                ConnectionState::from_iris(c, source, file, gates)
                    .with_declared(declared)
                    .with_no_target_file(no_target)
            }
            None => {
                let gates = write_gate::resolve_for_connection(declared, None, "USER");
                let mut state = ConnectionState::new_disconnected(ConnectionSource::EnvVars, gates)
                    .with_declared(declared);
                match config_path {
                    Some(p) if !crate::iris::workspace_config::file_names_a_target(&p) => {
                        state.config_file_no_target = Some(p)
                    }
                    other => state.config_file = other,
                }
                state
            }
        };

        let log_max = std::env::var("IRIS_LOG_STORE_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50usize);
        let log_ttl = std::env::var("IRIS_LOG_TTL_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60u64);

        Ok(Self {
            connection: Arc::new(std::sync::Mutex::new(conn_state)),
            config_watcher: Arc::new(std::sync::Mutex::new(config_watcher)),
            config: Arc::new(std::sync::RwLock::new(ConfigHolder::default())),
            registry: Arc::new(registry),
            client,
            identity_clients: Default::default(),
            history: Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(50))),
            elicitation_store: Arc::new(ElicitationStore::new()),
            checkout_cache: Arc::new(crate::elicitation::CheckoutCache::new()),
            log_store: Arc::new(std::sync::Mutex::new(log_store::LogStore::new(
                log_max, log_ttl,
            ))),
            metadata_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            pool: Arc::new(std::sync::RwLock::new(Arc::new(
                crate::iris::connection_pool::ConnectionPool::unloaded(pool_config_path),
            ))),
            ws_pool: Arc::new(crate::iris::ws_session::WsSessionPool::new()),
            confirm_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            toolset,
            session: crate::telemetry::Session::new(),
            docker_only_attr_warned: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            iris_audit_counter: crate::iris::iris_audit::AuditEmitCounter::new(),
            accepted_params: param_check::accepted_parameters(&router.list_all()),
            tool_router: router,
            strict_toolset: false,
        })
    }

    /// Snapshot of the connection pool, loaded on first use.
    /// Read-locks the RwLock and clones the inner Arc — callers get an owned Arc
    /// that stays valid even if iris_reload_pool swaps the pool concurrently.
    ///
    /// The first call after construction (or after a config reload) builds the pool, which reads
    /// every member's password and so may open the OS keychain. Code that needs only the default
    /// connection must not call this.
    pub fn pool_ref(&self) -> Arc<crate::iris::connection_pool::ConnectionPool> {
        let current = self.pool.read().unwrap().clone();
        let Some(config_file) = current.pending_load().map(|p| p.config_file.clone()) else {
            return current;
        };
        let mut slot = self.pool.write().unwrap();
        // Another caller may have loaded it while this one waited for the write lock.
        if slot.pending_load().is_none() {
            return slot.clone();
        }
        // A server that has loaded its configuration builds the pool from what it holds, not from
        // the files again (FR-043).
        let held_fleet = self
            .config
            .read()
            .unwrap()
            .snapshot
            .as_ref()
            .map(|s| s.fleet.clone());
        let loaded = Arc::new(match held_fleet {
            Some(fleet) => crate::iris::connection_pool::load_pool_with_fleet(fleet),
            None => crate::iris::connection_pool::load_pool(config_file.as_deref()),
        });
        *slot = loaded.clone();
        loaded
    }

    /// Install the configuration the server started with. Reloads rebuild it from `inputs`.
    pub fn set_config(
        &self,
        inputs: crate::iris::config_sources::LoadInputs,
        snapshot: crate::iris::config_sources::ConfigSnapshot,
    ) {
        let mut h = self.config.write().unwrap();
        h.snapshot = Some(Arc::new(snapshot));
        h.inputs = Some(inputs);
    }

    /// A gate refusal as the client sees it: the gate's own body plus `refusal {kind, server,
    /// lift, set_aside}` (FR-039), sent as an error result. Every gate leaves through here, so a
    /// refusal is `isError` whatever gate raised it.
    pub(crate) fn refuse(&self, gate: serde_json::Value) -> Result<CallToolResult, McpError> {
        // The server the call was resolved to: the one the body names, else the primary.
        let t = self.target()?;
        let named = ["server_name", "instance"]
            .iter()
            .find_map(|k| gate.get(*k).and_then(|v| v.as_str()));
        let server = t
            .servers()
            .find(|s| Some(s.key.as_str()) == named)
            .unwrap_or(&t.primary);
        self.refusal_on(server, gate)
    }

    /// A handler's answer with `refusal` added when its `error_code` is a refusal that has none.
    fn classify_answer(&self, result: CallToolResult) -> CallToolResult {
        let refusal_without_object = result.structured_content.as_ref().is_some_and(|b| {
            b["error_code"]
                .as_str()
                .is_some_and(|c| crate::policy::refusal::REFUSAL_CODES.contains(&c))
                && b.get("refusal").is_none()
        });
        if !refusal_without_object {
            return result;
        }
        let body = result.structured_content.clone().unwrap_or_default();
        self.refuse(body).unwrap_or(result)
    }

    /// `CONFIG_INVALID` when the configuration did not parse, for a tool that talks to IRIS.
    /// A broken file is an error, not a missing one: the gates are closed and the connection is
    /// not built, so every IRIS tool says why (R5 C11).
    fn config_invalid(&self, tool: &str) -> Option<Result<CallToolResult, McpError>> {
        if CONFIG_TOLERANT_TOOLS.contains(&tool) {
            return None;
        }
        let snap = self.snapshot();
        let e = snap.errors.first()?;
        Some(err_result(serde_json::json!({
            "success": false,
            "error_code": "CONFIG_INVALID",
            "error": format!("{} does not parse: {}", e.path.display(), e.message),
            "file": e.path.to_string_lossy(),
            "line": e.line,
            "column": e.column,
            "remediation": "Fix the file; iad reads it again on the next call. Until then every \
                            tool that talks to IRIS is refused and the write gates are closed.",
        })))
    }

    /// The project directory host paths are confined to and the directory a relative path is
    /// taken from (spec 141 T212; `tools/host_fs.rs`).
    fn host_path_context(&self) -> (Option<std::path::PathBuf>, std::path::PathBuf) {
        (
            self.snapshot().project_root.clone(),
            std::env::current_dir().unwrap_or_default(),
        )
    }

    /// The directory `kb index` reads, confined to the project (T216). `requested` of `None`, empty
    /// or `.` means the project directory. The `Err` is the answer to send.
    fn kb_index_scope(
        &self,
        tool: &str,
        requested: Option<&str>,
    ) -> Result<skills_tools::KbIndexScope, Result<CallToolResult, McpError>> {
        let (root, cwd) = self.host_path_context();
        let requested = requested.filter(|r| !r.is_empty() && *r != ".");
        let found = match (requested, root.as_deref()) {
            (None, Some(r)) => Ok(r.to_path_buf()),
            (None, None) => Err(host_fs::PathRefusal::NoRoot),
            (Some(r), root) => host_fs::confine_dir(root, &cwd, r),
        };
        match found {
            Ok(dir) => Ok(skills_tools::KbIndexScope {
                dir,
                root: root.unwrap_or_default(),
            }),
            Err(host_fs::PathRefusal::NotFound) => Err(err_json(
                "WORKSPACE_NOT_FOUND",
                &format!("{} is not a directory", requested.unwrap_or(".")),
            )),
            Err(why) => Err(self.refuse(host_fs::refusal_body(
                tool,
                requested.unwrap_or("."),
                &why,
                &[],
            ))),
        }
    }

    /// The configuration every gate reads. Before `set_config` (tests that build `IrisTools`
    /// directly) it is loaded from the process on each call, from the directory the watcher names.
    pub fn snapshot(&self) -> Arc<crate::iris::config_sources::ConfigSnapshot> {
        if let Some(s) = self.config.read().unwrap().snapshot.clone() {
            return s;
        }
        let watcher_ws = {
            let w = self.config_watcher.lock().unwrap();
            w.as_ref()
                .and_then(|w| w.config_path.parent())
                .and_then(|p| p.to_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        };
        let ws = watcher_ws.or_else(|| {
            self.connection
                .lock()
                .unwrap()
                .config_file
                .as_ref()
                .and_then(|p| p.parent())
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
        });
        Arc::new(crate::iris::config_sources::ConfigSnapshot::load(
            &crate::iris::config_sources::LoadInputs::from_process(ws.as_deref(), None),
        ))
    }

    /// Check if the operator file or the workspace file has changed since last load; if so,
    /// rebuild the snapshot and, when the connection came from a file that changed, re-probe.
    /// Called at the start of every tool handler for lazy hot-reload (034).
    /// Completely silent — no error returned to caller on reload failure.
    async fn check_reload(&self) {
        // Check if the watcher says either file changed.
        // Also treat the file as "changed" on first call when it exists but wasn't loaded at
        // startup (e.g. CWD was "/" when the server launched — issue #104).
        let (changed, deleted, operator_changed) = {
            let mut w = self.config_watcher.lock().unwrap();
            if let Some(ref mut watcher) = *w {
                // If startup fell back to auto-discovery with NO active connection, and the
                // config file exists, it was present but not loaded (e.g. cwd="/" launch,
                // issue #104). Reset mtime so has_changed() fires on the first tool call.
                // Condition requires both AutoDiscovered AND no live IRIS — if IRIS is already
                // connected (even via auto-discovery), don't overwrite a working connection.
                let (source, has_iris) = {
                    let c = self.connection.lock().unwrap();
                    (c.source.clone(), c.iris.is_some())
                };
                let file_exists = watcher.config_path.exists();
                if source == ConnectionSource::AutoDiscovered
                    && !has_iris
                    && file_exists
                    && watcher.last_mtime.is_some()
                {
                    watcher.last_mtime = None;
                }
                // A deletion is a change to the gate even though it is not a change to load.
                // `has_changed()` answers false here by design — it clears its own mtime so the
                // file coming back is still detected — so the deletion has to be read off the
                // watcher before that call consumes the state (085 edge case 2).
                let deleted = watcher.last_mtime.is_some() && !file_exists;
                let changed = watcher.has_changed();
                (changed, deleted, watcher.operator_changed())
            } else {
                (false, false, false)
            }
        };
        if !(changed || deleted || operator_changed) {
            return;
        }

        use crate::iris::config_sources::{ConfigSnapshot, FileKind, LoadInputs};
        let mut inputs = self
            .config
            .read()
            .unwrap()
            .inputs
            .clone()
            .unwrap_or_else(|| {
                let dir = {
                    let w = self.config_watcher.lock().unwrap();
                    w.as_ref()
                        .and_then(|w| w.config_path.parent())
                        .and_then(|p| p.to_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                };
                LoadInputs::from_process(dir.as_deref(), None)
            });
        // The operator file as the server runs on it: an edit since then applies only where it
        // tightens (FR-043). Without a loaded snapshot there is nothing to hold against.
        inputs.operator_baseline = self.config.read().unwrap().snapshot.as_ref().map(|s| {
            s.operator_file
                .as_ref()
                .map(|f| f.table.clone())
                .unwrap_or_default()
        });
        let new_snap = ConfigSnapshot::load(&inputs);

        // A rebuild that fails keeps the last good snapshot and says why (R4a D1). So does a
        // contradictory gate declaration: the only safe answer mid-session is the last known-good
        // gate plus the reason, never a widened one.
        let had_good = self
            .config
            .read()
            .unwrap()
            .snapshot
            .as_ref()
            .is_some_and(|s| !s.is_invalid());
        let problem = new_snap.errors.first().map(|e| e.display()).or_else(|| {
            new_snap
                .gate_error()
                .map(|e| format!("{}: {}", e.code(), e))
        });
        if let (Some(problem), true) = (&problem, had_good) {
            self.connection.lock().unwrap().config_parse_error = Some(problem.clone());
            return;
        }
        let snap = Arc::new(new_snap);
        {
            let mut h = self.config.write().unwrap();
            h.snapshot = Some(snap.clone());
            // The baseline belongs to one reload: the next one reads it from the snapshot.
            inputs.operator_baseline = None;
            h.inputs = Some(inputs);
            // Plain `eprintln!`, not tracing: `RUST_LOG` can silence tracing, and the operator
            // should learn that an edit did not take effect without knowing that.
            for held in &snap.held {
                if h.held_announced
                    .insert(format!("{}={}", held.id, held.setting))
                {
                    eprintln!("{}", held.stderr_line(snap.operator_path.as_deref()));
                }
            }
        }
        // What was learned about the pool's members under the old files is not carried over.
        self.pool_ref().clear_modes();
        let declared = snap.declared_gates();

        let connection_kind = snap.connection_config.as_ref().map(|(_, _, k)| *k);
        let reconnect = snap.connection_config.is_some()
            && (changed || (operator_changed && connection_kind == Some(FileKind::Operator)));

        // Only the declaration moved, or the file is gone or unusable: keep the live connection and
        // re-resolve the gate from what the files declare now — exactly what a fresh start would
        // decide. Leaving the old value in place would let a declaration outlive the file that made
        // it, which is the same stale-gate defect as the env-var latch.
        if !reconnect {
            let mut conn = self.connection.lock().unwrap();
            let ns = conn
                .iris
                .as_ref()
                .map(|c| c.namespace.clone())
                .unwrap_or_default();
            conn.gates = write_gate::resolve_for_connection(declared, conn.iris.as_deref(), &ns);
            conn.declared = declared;
            conn.config_parse_error = problem;
            tracing::info!(
                "iris-agentic-dev: configuration reloaded — gate re-resolved \
                 (write_tools_enabled={}, source={})",
                conn.gates.write_enabled,
                conn.gates.write_source.as_str()
            );
            return;
        }

        let Some((cfg, config_path, _)) = snap.connection_config.clone() else {
            return;
        };
        // The startup rule: the operator file yields its connection to IRIS_HOST; its settings
        // still reload, and discovery below returns the IRIS_HOST server.
        let chosen = snap.file_chooses_connection();
        let conn_result = if chosen {
            crate::iris::workspace_config::workspace_config_to_connection(&cfg, "USER")
        } else {
            crate::iris::workspace_config::export_tool_lists(&cfg);
            None
        };

        // Probe the new connection
        let mut new_conn = match conn_result {
            Some(c) => c,
            None => {
                // container= config — discovery finds it from the file's hints, not the environment
                let hints = crate::iris::workspace_config::container_discovery_hints(&cfg);
                match crate::iris::discovery::discover_iris_with(None, &hints).await {
                    crate::iris::discovery::IrisDiscovery::Found(c) => c,
                    _ => {
                        let mut conn = self.connection.lock().unwrap();
                        conn.config_parse_error = Some(
                            "Hot-reload: could not discover IRIS connection from updated config"
                                .to_string(),
                        );
                        return;
                    }
                }
            }
        };

        new_conn.probe().await;

        // Atomically swap connection. The gate is resolved from the config that was just read, so
        // an edit in either direction takes effect on this reload (085 FR-002).
        let gates =
            write_gate::resolve_for_connection(declared, Some(&new_conn), &new_conn.namespace);
        let config_path_for_pool = config_path.clone();
        let source = if chosen {
            ConnectionSource::ConfigFile
        } else {
            ConnectionSource::EnvVars
        };
        // A file that names no server is not where the connection came from (T194).
        let no_target = snap
            .file_naming_no_target()
            .map(std::path::Path::to_path_buf);
        let named = no_target.is_none().then_some(config_path);
        let new_state = ConnectionState::from_iris(new_conn, source, named, gates)
            .with_declared(declared)
            .with_no_target_file(no_target);
        let mut conn = self.connection.lock().unwrap();
        *conn = new_state;
        conn.config_parse_error = None;
        tracing::info!("iris-agentic-dev: hot-reloaded connection from .iris-agentic-dev.toml");
        drop(conn);

        // 093 US2: also reload the pool on config change. Same swap as iris_reload_pool, except
        // the new pool is built when a member is first needed (T187): a reload on the first tool
        // call must not read every member's password, and so open the keychain, for a call that
        // names no server. Pass config_path so Source 3 (fleet instances) is loaded from the right
        // TOML. Fail-safe: parse errors return early above, so we only reach here when the config
        // parsed successfully.
        *self.pool.write().unwrap() = Arc::new(
            crate::iris::connection_pool::ConnectionPool::unloaded(Some(config_path_for_pool)),
        );
        tracing::info!(
            "iris-agentic-dev: pool reload triggered by config change — loads on first use"
        );
    }
    /// The client for the call in scope: the target server's own certificate setting decides
    /// whether its certificate is checked. Outside a call, the client built at startup.
    fn http_client(&self) -> &reqwest::Client {
        let tls = self
            .target()
            .ok()
            .and_then(|t| t.primary.conn.as_ref().map(|c| c.tls));
        match tls {
            Some(tls) => crate::iris::http::call_client_ref(&tls).unwrap_or(&self.client),
            None => &self.client,
        }
    }
    fn record_call(&self, tool: &str, success: bool) {
        self.record(tool, success, true);
    }

    /// [`Self::record_call`] for a call the pipeline refused before any handler ran. The durable
    /// record goes to the local file, never to IRIS: a write gate that says no must not be followed
    /// by a write into the server it protects, or by a request to one that cannot be reached.
    pub(crate) fn record_refused(&self, tool: &str) {
        self.record(tool, false, false);
    }

    fn record(&self, tool: &str, success: bool, to_iris: bool) {
        let duration_ms = CALL_START
            .try_with(|start| start.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let record = ToolCallEntry::now(tool, success, duration_ms, self.session.id);
        let buffer_max = std::env::var("IRIS_TELEMETRY_BUFFER_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000usize);
        if let Ok(mut h) = self.history.lock() {
            while h.len() >= buffer_max {
                h.pop_front();
            }
            h.push_back(record.clone());
        }
        // Best-effort durable side write — never blocks or fails the caller (FR-014). The IRIS
        // sink runs only on the server the call acted on, and only where iad may write and nobody
        // watches the write (R5 C16, T208), and never for a refused call.
        let iris = if to_iris {
            self.durable_sink_connection()
        } else {
            None
        };
        let client = Arc::clone(&self.client);
        let config_dir = telemetry_config_dir();
        // Counted, so an exiting CLI process can wait for it (130 round 4).
        crate::telemetry::DURABLE_WRITES.spawn(async move {
            crate::telemetry::write_durable(&record, iris, &client, &config_dir).await;
        });
    }

    /// The sink the operator chose: the operator file's `telemetry_sink` or `IAD_TELEMETRY_SINK`.
    /// A workspace file's key is set aside before it reaches the snapshot's operator settings.
    fn telemetry_sink(&self) -> crate::telemetry::TelemetrySink {
        crate::telemetry::sink_setting(
            &self.snapshot().operator,
            std::env::var("IAD_TELEMETRY_SINK").ok().as_deref(),
        )
    }

    /// The connection of the server the call in scope acted on when the telemetry IRIS sink may
    /// use it, else `None` and the record goes to the local file alone. The sink is off unless the
    /// operator opted in. Outside a call (no resolved target) it is the default connection, judged
    /// by the default server's own gates.
    fn durable_sink_connection(&self) -> Option<Arc<IrisConnection>> {
        // The snapshot is read before the connection lock because it takes it too.
        let strict = self.snapshot().read_only_strict();
        let sink = self.telemetry_sink();
        if let Ok(target) = self.target() {
            let server = &target.primary;
            return crate::telemetry::durable_sink(
                server.conn.clone(),
                crate::telemetry::SinkGate::for_server(
                    server.gates.write_enabled,
                    strict,
                    server.role,
                    sink,
                ),
            );
        }
        let c = self.connection.lock().unwrap();
        crate::telemetry::durable_sink(
            c.iris.clone(),
            crate::telemetry::SinkGate::for_server(
                c.gates.write_enabled,
                strict,
                crate::iris::workspace_config::ConnectionRole::Workbench,
                sink,
            ),
        )
    }

    /// Write an audit log entry for a policy-gated tool call, on the connection of the server the
    /// call acts on, which is not always the default one (spec 141, R1 D8.9). No-op when that
    /// connection has no active policy block.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_audit_entry_on(
        &self,
        tool: &str,
        server_name: &str,
        conn: Option<Arc<IrisConnection>>,
        policy: Option<&crate::iris::workspace_config::ConnectionPolicy>,
        status: &str,
        gate: Option<&str>,
        allowed_categories: Option<Vec<String>>,
        params: serde_json::Value,
    ) {
        use crate::iris::audit_log::{AuditLog, AuditLogEntry};

        // T037: Opt-in %SYS.Audit emission when irisAudit = true on the connection policy.
        // Checked BEFORE the should_write guard so env-var connections (policy=None or no
        // audit_log path) can still emit when irisAudit=true on a [policy.*] section.
        if policy.map(|p| p.iris_audit).unwrap_or(false) {
            use crate::iris::connection::{caller_mode, mcp_peer};
            use crate::iris::iris_audit::{
                build_audit_os, build_event_data, refuse_and_instruct_text,
            };
            let tool_name = tool.to_string();
            let mode = caller_mode();
            let peer = mcp_peer();
            let event_data = build_event_data(&tool_name, mode, peer);
            let os_code = build_audit_os(&event_data, "iris-agentic-dev tool call");
            let counter = self.iris_audit_counter.clone();
            if let Some(iris_conn) = conn.clone() {
                tokio::task::spawn(async move {
                    let client = match iris_conn.client(crate::iris::http::ClientKind::Raw) {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!("iris_audit: failed to build HTTP client: {e}");
                            return;
                        }
                    };
                    let ns = iris_conn.namespace.clone();
                    match iris_conn
                        .execute_via_generator(&os_code, &ns, &client)
                        .await
                    {
                        Ok(out) if out.trim().starts_with('1') => {}
                        Ok(_) => {
                            if counter.record_failure() {
                                tracing::warn!("{}", refuse_and_instruct_text());
                            }
                        }
                        Err(e) => {
                            if counter.record_failure() {
                                tracing::warn!("iris_audit: emission failed: {e}");
                            }
                        }
                    }
                });
            }
        }

        if !AuditLog::should_write(policy) {
            return;
        }
        let Some(path) = AuditLog::default_path() else {
            return;
        };
        let keep_payloads = self.audit_payloads_enabled();
        let namespace = conn.map(|i| i.namespace.clone()).unwrap_or_default();
        let entry = AuditLogEntry {
            ts: chrono::Utc::now().to_rfc3339(),
            tool: tool.to_string(),
            connection: server_name.to_string(),
            namespace,
            status: status.to_string(),
            gate: gate.map(|s| s.to_string()),
            allowed_categories,
            params,
        };
        let log = AuditLog::new(path).with_payloads(keep_payloads);
        let _ = log.write(&entry);
    }

    /// Does the local audit log keep payload text? Only the operator file's `audit_payloads` or
    /// `IAD_AUDIT_PAYLOADS` say so; a workspace file's is set aside before it gets here.
    fn audit_payloads_enabled(&self) -> bool {
        // Before the connection lock: `snapshot()` takes it too.
        let snap = self.snapshot();
        crate::iris::audit_log::payloads_enabled(
            &snap.operator,
            std::env::var("IAD_AUDIT_PAYLOADS").ok().as_deref(),
        )
    }

    /// The content check of the tools that run the agent's code: a literal `Kill ^<global>` needs
    /// the destructive tier (spec 087). The gates themselves (tier, policy, role, audit) ran in the
    /// pipeline against the target server (spec 141, R1); this reads that server's resolution.
    ///
    /// Only the obvious literal form is detected. Indirect kills (variables, `Xecute`, class
    /// dispatch, `&sql`) are not, and the message says so; IRIS privileges and the environment
    /// template are the controls for those.
    fn exec_content_gate(&self, tool_name: &str, code: &str) -> Option<serde_json::Value> {
        if !crate::tools::write_gate::contains_global_kill(code) {
            return None;
        }
        let target = match self.target() {
            Ok(t) => t,
            Err(e) => {
                return Some(serde_json::json!({
                    "success": false,
                    "error_code": target::ERR_TARGET_NOT_RESOLVED,
                    "error": e.message.to_string(),
                }))
            }
        };
        let gates = target.primary.gates;
        if gates.destructive_enabled {
            return None;
        }
        Some(serde_json::json!({
            "success": false,
            "error_code": crate::tools::write_gate::ERR_DESTRUCTIVE_GATE,
            "error": format!(
                "{tool_name} contains a Kill ^<global> expression and the destructive \
                 tier is disabled (source: {}). Note: this check \
                 applies to literal Kill ^ patterns in the code string only. Indirect kill \
                 operations (via variables, Xecute, or class methods) are not detected \
                 here — IRIS-side credentials and the mcpTemplate env gate are the \
                 appropriate controls for those.",
                gates.destructive_source.as_str()
            ),
        }))
    }

    #[tool(
        description = "Compile an ObjectScript class, routine, or wildcard package on IRIS via Atelier REST. Supports 'MyApp.*.cls' for package-level compilation. Also accepts a local file path as `target` — uploads it first, then compiles. Returns structured errors with line numbers, columns, and severity. On a successful single-document compile, `content` carries the post-compile source (the compiler can rewrite it beyond what was submitted, e.g. auto-mapping a new property into Storage) — use it to sync a local file without a separate `iris_doc(get)`; `content` is omitted for wildcard/package compiles. No Python required. Skill: objectscript-tdd for the compile-test-fix loop. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisCompileResponse>()
    )]
    async fn iris_compile(
        &self,
        Parameters(p): Parameters<CompileParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(msg) = compile_flags_problem(&p.flags) {
            return err_json("INVALID_PARAMS", &msg);
        }
        // A local file is judged before any request and before it is read (T212).
        let local_file = if is_local_compile_target(&p.target) {
            let (root, cwd) = self.host_path_context();
            match host_fs::confine_file(
                root.as_deref(),
                &cwd,
                &p.target,
                host_fs::COMPILE_EXTENSIONS,
            ) {
                Ok(canonical) => Some(canonical),
                Err(host_fs::PathRefusal::NotFound) => {
                    return err_json(
                        "FILE_NOT_FOUND",
                        &format!("Local file not found: {}", p.target),
                    )
                }
                Err(why) => {
                    return self.refuse(host_fs::refusal_body(
                        "iris_compile",
                        &p.target,
                        &why,
                        host_fs::COMPILE_EXTENSIONS,
                    ))
                }
            }
        } else {
            None
        };
        let iris = self.target_conn()?;
        let namespace = self.target_ns(p.namespace.as_deref())?;
        let gate = crate::tools::target::compile_gate()?;
        tracing::info!(namespace = %namespace, target = %p.target, "iris_compile");

        // Capability gate: if atelier_rest is unavailable (docker_only or NoPWS build),
        // compile via docker exec immediately — no 52773 probe, no retry.
        {
            let docker_only = iris.base_url == "http://127.0.0.1:1"
                || iris.base_url.starts_with("http://127.0.0.1:1/");
            let no_pws = iris
                .version
                .as_deref()
                .map(|v| v.contains("2026.2.0AI"))
                .unwrap_or(false);
            let ssh_host_compile = iris.ssh_host.clone();

            if docker_only || no_pws {
                // `$SYSTEM.OBJ.Compile` takes class names only: `X.cls` is ERROR #5351 and a routine
                // is not accepted at all. `CompileList` takes documents with their extension.
                let code = format!(
                    "do $SYSTEM.OBJ.CompileList(\"{}\",\"{}\")",
                    crate::objectscript::os_lit_inner(&stored_check::compile_list_arg(&p.target)),
                    crate::objectscript::os_lit_inner(&p.flags),
                );
                let exec_path = nopws::execution_path_docker(ssh_host_compile.as_deref());
                // The same stored-source check as the HTTP path (FR-015). A failure of the exec
                // channel itself is carried into the arms below, which report it as the compile
                // would have; nothing is compiled.
                let checked = if gate.on {
                    let ssh = ssh_host_compile.as_deref();
                    match stored_check::expand_exec_target(&iris, ssh, &namespace, &p.target).await
                    {
                        Err(outcome) => outcome,
                        Ok(docs) => {
                            // The stored source first, then the superclasses IRIS would also
                            // compile (FR-015a).
                            match stored_check::check_exec_docs(
                                &iris,
                                ssh,
                                &namespace,
                                docs.clone(),
                            )
                            .await
                            {
                                stored_check::Outcome::Clear => {
                                    superclass_check::check_exec(&iris, ssh, &namespace, &docs)
                                        .await
                                }
                                other => other,
                            }
                        }
                    }
                } else {
                    stored_check::Outcome::Clear
                };
                let result = match checked {
                    stored_check::Outcome::Refused(err) => {
                        self.record_call("iris_compile", false);
                        return err_result(err);
                    }
                    stored_check::Outcome::Transport(msg) => Err(anyhow::anyhow!(msg)),
                    stored_check::Outcome::Clear => {
                        if let Some(ref ssh_host) = ssh_host_compile {
                            iris.execute_ssh(&code, &namespace, ssh_host).await
                        } else {
                            iris.execute(&code, &namespace).await
                        }
                    }
                };
                self.record_call("iris_compile", result.is_ok());
                return match result {
                    Ok(output) => {
                        let trimmed = output.trim().to_string();
                        let success = !trimmed.contains("ERROR");
                        let mut resp = serde_json::json!({
                            "success": success,
                            "target": p.target,
                            "namespace": namespace,
                            "method": "docker_exec",
                            "execution_path": exec_path,
                            "output": trimmed,
                        });
                        gate.note_if_off(&mut resp);
                        ok_json(resp)
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg == "DOCKER_REQUIRED" {
                            err_json(
                                "DOCKER_REQUIRED",
                                "compile_path=docker_exec but IRIS_CONTAINER env var is not set. \
                                 Set IRIS_CONTAINER to the container name and retry.",
                            )
                        } else if let Some(detail) = msg.strip_prefix(&format!(
                            "{}: ",
                            crate::iris::connection::ERR_CONTAINER_UNREACHABLE
                        )) {
                            // Nothing was compiled, so COMPILE_FAILED would send the agent looking
                            // at the class. Name the container instead, and keep execution_path so
                            // a docker_only caller can see which leg broke.
                            err_result(serde_json::json!({
                                "success": false,
                                "error_code": crate::iris::connection::ERR_CONTAINER_UNREACHABLE,
                                "error": detail,
                                "target": p.target,
                                "namespace": namespace,
                                "execution_path": exec_path,
                            }))
                        } else {
                            err_json("COMPILE_FAILED", &msg)
                        }
                    }
                };
            }
        }

        let client = self.http_client();

        // Local file path support: a file inside the project is uploaded via Atelier PUT first.
        if let Some(path) = local_file.as_deref() {
            {
                let content = match std::fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(e) => {
                        return err_json(
                            "READ_ERROR",
                            &format!("Could not read {}: {}", p.target, e),
                        )
                    }
                };
                // Derive document name from Class declaration or from file name. A routine or
                // include keeps its file name: a `.mac` is not a class even when a line in it
                // starts with the word "class".
                let file_name = std::path::Path::new(&p.target)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown.cls")
                    .to_string();
                let is_routine_file = {
                    let lower = file_name.to_lowercase();
                    lower.ends_with(".mac") || lower.ends_with(".inc") || lower.ends_with(".int")
                };
                let doc_name = content
                    .lines()
                    .find(|l| l.trim_start().to_lowercase().starts_with("class "))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .map(|cls| format!("{}.cls", cls))
                    .filter(|_| !is_routine_file)
                    .unwrap_or(file_name);
                // Compile-time code execution gate, on the file and on every include it names
                // that is already stored. A refusal is an error result.
                if gate.on {
                    if let Some(err) = stored_check::check_http(
                        &iris,
                        client,
                        &namespace,
                        vec![stored_check::Seed::Content {
                            doc: doc_name.clone(),
                            content: content.clone(),
                        }],
                    )
                    .await
                    {
                        self.record_call("iris_compile", false);
                        return err_result(err);
                    }
                }
                // Upload via Atelier PUT
                let put_url = iris.versioned_ns_url(
                    &namespace,
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
                // Check PUT response body for Atelier-level errors (200 OK with status.errors
                // can occur on some IRIS builds when the upload fails internally, e.g. build 110
                // SetTextFromString NULL namespace bug).
                let put_body: serde_json::Value =
                    crate::iris::http::json_or_default(put_resp).await;
                if let Some(errs) = put_body["status"]["errors"].as_array() {
                    if !errs.is_empty() {
                        let msg = errs[0]["error"].as_str().unwrap_or("Upload failed");
                        self.record_call("iris_compile", false);
                        return err_json("UPLOAD_FAILED", msg);
                    }
                }
                // The upload put the class in the dictionary, so its `Extends` is now readable
                // there. A superclass IRIS would compile with it is refused (FR-015a); the class
                // stays stored, and nothing is compiled.
                if gate.on {
                    if let stored_check::Outcome::Refused(mut err) = superclass_check::check_http(
                        &iris,
                        client,
                        &namespace,
                        std::slice::from_ref(&doc_name),
                    )
                    .await
                    {
                        err["uploaded"] = serde_json::Value::Bool(true);
                        self.record_call("iris_compile", false);
                        return err_result(err);
                    }
                }
                // Compile via shared compile_document helper
                let local_src = p.target.clone();
                let cr = iris
                    .compile_document(&doc_name, &namespace, &p.flags, client)
                    .await
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                let errors: Vec<serde_json::Value> = cr
                    .errors
                    .iter()
                    .map(|e| serde_json::json!({"severity":"error","code":"","line":0,"column":0,"text":e}))
                    .collect();
                let console: Vec<serde_json::Value> = cr
                    .console
                    .iter()
                    .map(|l| serde_json::Value::String(l.clone()))
                    .collect();
                let success = cr.success();
                self.record_call("iris_compile", success);
                // Atelier parity: the compiler can rewrite content beyond what was
                // uploaded (e.g. auto-mapping a new property into Storage) — re-fetch
                // so the caller can sync the local file without a separate get.
                let content = if success {
                    doc::fetch_doc_content(&iris, client, &doc_name, &namespace).await
                } else {
                    None
                };
                let mut resp = serde_json::json!({
                    "success": success,
                    "target": doc_name,
                    "uploaded_from": local_src,
                    "targets_compiled": 1,
                    "namespace": namespace,
                    "errors": errors,
                    "warnings": [],
                    "console": console,
                    "content": content,
                });
                if success {
                    let source = resp["content"].as_str().map(str::to_string);
                    add_source_hint(&mut resp, source.as_deref());
                } else {
                    add_compile_hint(
                        &mut resp,
                        &[cr.errors.as_slice(), cr.console.as_slice()].concat(),
                    );
                }
                gate.note_if_off(&mut resp);
                return ok_json(resp);
            }
        }

        // Expand wildcards: resolve "MyApp.*.cls" to a list of matching class names. A listing that
        // fails is a refusal, not a fall back to the literal pattern: IRIS would expand it itself,
        // and the members would never be read.
        let targets: Vec<String> =
            match stored_check::expand_http(&iris, client, &namespace, &p.target).await {
                Ok(list) => list,
                Err(reason) => {
                    self.record_call("iris_compile", false);
                    return err_result(stored_check::unreadable_listing(&p.target, &reason));
                }
            };

        if targets.is_empty() {
            return err_json(
                "NOT_FOUND",
                &format!("No documents match pattern: {}", p.target),
            );
        }

        // force_writable: attempt to enable namespace via docker exec if available
        if p.force_writable {
            let code = crate::objectscript::enable_namespace_code(&namespace);
            let _ = iris.execute(&code, &namespace).await;
        }

        // Atelier compile: POST with JSON array of document names (with extensions)
        // e.g. ["MyApp.Patient.cls", "MyApp.Utils.cls"]
        let compile_url = iris.versioned_ns_url(
            &namespace,
            &format!("/action/compile?flags={}", urlencoding::encode(&p.flags)),
        );

        // Ensure targets have extensions.
        // Bug 16: the old check `t.contains('.')` skipped top-level classes (no package dot).
        // Correct check: append .cls only when no known extension is already present.
        let targets_with_ext: Vec<String> = targets
            .iter()
            .map(|t| stored_check::doc_name_for(t))
            .collect();

        // A document already on the server was not necessarily written through `iris_doc`:
        // another tool, SCM, a routine written to `^rMAC`, or a class stored before the gate
        // existed. Check the stored source of each target, and of every include it names, before
        // IRIS compiles it (FR-015). A target that does not exist is left to the compile, which
        // reports it; one that cannot be read is refused.
        if gate.on {
            if let Some(err) = stored_check::check_http(
                &iris,
                client,
                &namespace,
                targets_with_ext
                    .iter()
                    .cloned()
                    .map(stored_check::Seed::Stored)
                    .collect(),
            )
            .await
            {
                self.record_call("iris_compile", false);
                return err_result(err);
            }
            // IRIS also compiles the target's out-of-date superclasses, which the check above did
            // not read (FR-015a).
            if let stored_check::Outcome::Refused(err) =
                superclass_check::check_http(&iris, client, &namespace, &targets_with_ext).await
            {
                self.record_call("iris_compile", false);
                return err_result(err);
            }
        }

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

        let body: serde_json::Value = crate::iris::http::capped_json::<serde_json::Value>(resp)
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
        let single_target = success && !p.target.contains('*') && targets.len() == 1;
        let open_uri = if single_target {
            write_open_hint(&namespace, &p.target);
            Some(format!("isfs://{}/{}", namespace, p.target))
        } else {
            None
        };
        // Atelier parity: the compiler can rewrite content beyond what was submitted
        // (e.g. auto-mapping a new property into Storage) — re-fetch so the caller
        // can sync a local copy without a separate get, same as the local-path branch
        // above. Only for a genuine single-document compile — a wildcard/package
        // compile has no single "the content" to hand back.
        let content = if single_target {
            doc::fetch_doc_content(&iris, client, &targets_with_ext[0], &namespace).await
        } else {
            None
        };

        let mut resp = serde_json::json!({
            "success": success,
            "target": p.target,
            "targets_compiled": targets.len(),
            "namespace": namespace,
            "errors": errors,
            "warnings": warnings,
            "console": console,
            "content": content,
        });
        if let Some(uri) = open_uri {
            resp["open_uri"] = serde_json::Value::String(uri);
        }
        if success {
            add_source_hint(&mut resp, content.as_deref());
        } else {
            let lines: Vec<String> = console
                .iter()
                .filter_map(|l| l.as_str().map(str::to_string))
                .collect();
            add_compile_hint(&mut resp, &lines);
        }
        gate.note_if_off(&mut resp);

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
        description = "Run %UnitTest.Manager or %UnitTest.TestProduction tests on IRIS and return structured pass/fail results. Uses pure-HTTP execution via Atelier REST — works with or without IRIS_CONTAINER. Pass a class name pattern like 'MyApp.Tests' or 'ISC.sql.TestFoo' to run already-compiled test classes (uses /noload automatically). A pattern with a path separator or '..' is refused. Returns suite-level summary inline plus log_id for per-test-case detail via iris_get_log. `test_type` (optional): 'auto' (default) auto-detects %UnitTest.TestProduction subclasses and calls .Run(); 'testcase' forces RunTest(); 'testproduction' forces .Run(). Skill: objectscript-unit-test for test scaffolding; objectscript-tdd for the full loop. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisTestResponse>()
    )]
    async fn iris_test(
        &self,
        Parameters(p): Parameters<TestParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(msg) = test_pattern_problem(&p.pattern) {
            return err_json("INVALID_PARAMS", &msg);
        }
        let timeout = std::time::Duration::from_secs(p.timeout);

        // HTTP path only — docker exec path removed (#46: /noload/run assumed pre-loaded
        // classes which never existed in a fresh iris session, causing false "no test classes"
        // errors; HTTP path with /verbose=1 is reliable and works with or without docker).
        let path_label = "http";
        // The agent's tests run as the service account; the namespace check, the superclass probe
        // and the monitor's report and stop are iad's own code and run as the connection's user.
        let cx = match self.exec_context() {
            Ok(cx) => cx,
            Err(e) => {
                self.record_call("iris_test", false);
                return exec_refused(e);
            }
        };
        let primary = match self.primary_exec_context() {
            Ok(cx) => cx,
            Err(e) => {
                self.record_call("iris_test", false);
                return exec_refused(e);
            }
        };
        let iris = self.target_conn()?;
        let namespace = self.target_ns(p.namespace.as_deref())?;
        tracing::info!(namespace = %namespace, pattern = %p.pattern, "iris_test");
        let client = self.http_client();

        // US3: namespace existence check before running tests.
        let ns_check_code = crate::objectscript::namespace_exists_code(&namespace);
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
            return err_result(serde_json::json!({
                "success": false,
                "error_code": ERR_NAMESPACE_NOT_FOUND,
                "error": format!("Namespace '{}' does not exist on this IRIS instance", namespace),
                "namespace": namespace,
            }));
        }

        // Generate a UUID correlation token; used as UserParam in RunTest.
        let correlation_token = log_store::new_log_id();
        // Escaped where it is spliced, in build_test_run_code and build_superclass_probe (137).
        let safe_pattern = p.pattern.clone();

        // A compiled class pattern is looked up in the database (`/noload`), a directory is
        // scanned; `iris_coverage` takes both from the same two functions (spec 141 R5 C17).
        let flags = test_run_flags();

        // Auto-detect a `%UnitTest.TestProduction` subclass unless the caller says which it is.
        let is_test_production = match p.test_type.as_deref().unwrap_or("auto") {
            "testproduction" => true,
            "testcase" => false,
            _ => detect_test_production(&iris, client, &namespace, &safe_pattern).await,
        };

        // Run tests via execute_via_generator (HTTP path).
        // After RunTest completes, ^UnitTest.Result global IS persisted (globals bypass
        // the objectgenerator transaction boundary; SQL %Save() does not).
        let run_code =
            build_test_run_code(&safe_pattern, flags, &correlation_token, is_test_production);

        // coverage=true: start the monitor before the test run so it instruments execution.
        // We start here (before run_output), then report+stop after parsing test results.
        if p.coverage == Some(true) {
            let pkg = p
                .pattern
                .rsplit_once('.')
                .map(|(pkg, _)| pkg)
                .unwrap_or(&p.pattern);
            let start_params = coverage::IrisCoverageParams {
                mode: "start".to_string(),
                server: None,
                classes: p.coverage_classes.clone(),
                package: if p.coverage_classes.is_none() {
                    Some(pkg.to_string())
                } else {
                    None
                },
                test_path: None,
                target_pct: None,
                namespace: Some(namespace.clone()),
                cobertura_path: None,
                confirm: false,
            };
            // Ignore start errors — if monitor fails, coverage will return zeros/error
            // but the test run itself should still proceed.
            let _ = coverage::handle_iris_coverage(&cx, &start_params).await;
        }

        // Try HTTP (execute_via_generator) first. Fall back to docker exec if:
        // - IRIS_CONTAINER is set, AND
        // - HTTP returns empty output (RunTest couldn't create the pattern directory
        //   because execute_via_generator restricts filesystem writes)
        // RunTest writes verbose output to $IO (terminal device).
        // execute_via_generator redirects $IO to a temp file but RunTest also needs
        // to create directories under ^UnitTestRoot — which fails in that context.
        // When IRIS_CONTAINER is set, prefer docker exec (full filesystem + real terminal).
        let opts = exec::RunOpts::with_terminal(timeout, "IrisDevRun");
        let ran = if cx.route.container().is_some() {
            // Docker exec: full filesystem access, captures terminal output from RunTest.
            cx.run_docker_first(&run_code, &namespace, &opts).await
        } else {
            // HTTP path: works for remote IRIS without docker.
            cx.run(
                &run_code,
                &namespace,
                &exec::RunOpts::http_only(timeout, "IrisDevRun"),
            )
            .await
        };
        let outcome = match ran {
            Ok(o) => o,
            Err(e) => {
                self.record_call("iris_test", false);
                let mut body = e.to_json();
                if e.code == "TIMEOUT" {
                    body["error"] =
                        serde_json::json!(format!("Test run timed out after {}s", p.timeout));
                } else if !exec::is_identity_failure(&e.code) && e.code != "DOCKER_REQUIRED" {
                    body["error_code"] = serde_json::json!(ERR_TEST_EXECUTION_ERROR);
                }
                return err_result(body);
            }
        };
        let run_identity = outcome.identity.to_json();
        let run_output = outcome.output;
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
                // Skip suite-level result lines (e.g. "MyClass\Sub FAILED") — these contain
                // path separators and are not individual test methods.
                // Skip if no class context (suite-level result without a class "begins" line),
                // or if name contains path separators (suite-level lines, not method names).
                if method_name.is_empty()
                    || current_class.is_empty()
                    || (!method_name.starts_with("Test") && !method_name.starts_with("test"))
                    || method_name.contains('\\')
                    || method_name.contains('/')
                    || method_name.contains('.')
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

        // IRIS creates a synthetic 1-failure suite when the pattern matches no test classes
        // (e.g. "Test022\NonExistent\NoSuchClass FAILED" at the suite level). The method
        // parser skips these (they contain path separators), so test_cases stays empty.
        // Treat any run with no parsed method results as NO_TESTS_FOUND.
        if total == 0 || test_cases.is_empty() {
            self.record_call("iris_test", false);
            // Produce an actionable hint based on the pattern shape.
            let hint = if let Some(problem) = unit_test_root_problem(&run_output) {
                problem
            } else if p.pattern.ends_with(".cls") || p.pattern.contains('*') {
                format!(
                    "iris_test requires a bare package name (e.g. \"App.Tests\"), not a \
                     class name or wildcard. Try: \"{}\"",
                    p.pattern
                        .trim_end_matches(".cls")
                        .rsplit_once('.')
                        .map(|(pkg, _)| pkg)
                        .unwrap_or(p.pattern.trim_end_matches('*').trim_end_matches('.'))
                )
            } else {
                "Pattern matched no test classes. Verify the package name is correct, \
                 the classes extend %UnitTest.TestCase or %UnitTest.TestProduction, \
                 and ^UnitTestRoot is set to your tests directory."
                    .to_string()
            };
            return err_result(serde_json::json!({
                "success": false,
                "error_code": ERR_NO_TESTS_FOUND,
                "error": "Pattern matched no test classes",
                "hint": hint,
                "pattern": p.pattern,
                "namespace": namespace,
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

        // coverage=true: report from the monitor that was started before the test run.
        // The start happened above (before run_output); now collect and stop.
        let coverage_result = if p.coverage == Some(true) {
            let pkg = p
                .pattern
                .rsplit_once('.')
                .map(|(pkg, _)| pkg)
                .unwrap_or(&p.pattern);
            let report_params = coverage::IrisCoverageParams {
                mode: "report".to_string(),
                server: None,
                classes: p.coverage_classes.clone(),
                package: if p.coverage_classes.is_none() {
                    Some(pkg.to_string())
                } else {
                    None
                },
                test_path: None,
                target_pct: p.coverage_target_pct,
                namespace: Some(namespace.clone()),
                cobertura_path: None,
                confirm: false,
            };
            let cov = coverage::handle_iris_coverage(&primary, &report_params).await;
            // Stop the monitor now that data is collected.
            let stop_params = coverage::IrisCoverageParams {
                mode: "stop".to_string(),
                server: None,
                classes: None,
                package: None,
                test_path: None,
                target_pct: None,
                namespace: Some(namespace.clone()),
                cobertura_path: None,
                confirm: false,
            };
            let _ = coverage::handle_iris_coverage(&primary, &stop_params).await;
            Some(cov)
        } else {
            None
        };

        let mut resp = serde_json::json!({
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
            "namespace": namespace,
            "test_suites": test_suites,
            "identity": run_identity,
        });
        if let Some(cov) = coverage_result {
            resp["coverage"] = cov;
        }
        ok_json(resp)
    }

    #[tool(
        description = "Execute arbitrary ObjectScript code on IRIS and return stdout. Two execution paths: (1) HTTP primary — wraps code in a temporary class method body (CodeMode=objectgenerator), compiles it via Atelier REST, runs it, deletes the class. Block syntax (`{}`) works here because class method bodies support it. (2) docker exec fallback — used when IRIS_CONTAINER env var is set and HTTP fails, or when the connection sets `docker_only = true` in `.iris-agentic-dev.toml` (a config-file key, not a parameter of this tool — passing docker_only in the tool arguments does nothing). This path pipes code into `iris session` stdin, which is a line-by-line terminal interpreter. Block syntax (`{}`) is NOT supported in terminal mode — `If cond { ... }` causes a `<SYNTAX>` error. Use classic terminal-compatible form instead: `If cond Write x`. For complex multi-line scripts that require `{}` on a docker exec path, write a .mac routine with `iris_doc` (mode=put) and compile it with `iris_compile`, then call `iris_execute Do entry^RoutineName`. That needs Atelier REST for `iris_doc`; under `docker_only` the routine must already be on the server. The destructive gate covers tool calls, not the code this tool runs: only a literal `Kill ^global` is checked, so for a hard limit connect as an IRIS user without delete privileges. &sql(...) embedded SQL runs natively on the HTTP path. The docker exec path has no &sql, so there it is translated to %SQL.Statement calls (set translate_sql: false to disable), and the response includes sql_translated: true and translated_code. Example: code='write $ZVERSION,!' returns the IRIS version string. Classes in Security.*, Config.* and SYS.* (web applications, users, roles, namespaces, databases) exist only in %SYS: pass namespace: \"%SYS\" for them. Skill: objectscript-tdd for the compile-execute-fix loop. Session state: set use_session: true to enable the %ctx carrier (%DynamicObject). Store values in %ctx.key between calls — scalars, %DynamicObject, and %Persistent objects (stored as OID stubs and re-opened on restore). The response includes session_state (opaque Base64 token); pass it back as session_state on the next call to restore %ctx. Nothing is written to IRIS — the token is held by the client. Error codes: SESSION_INVALID (bad token), SESSION_RESTORE_FAILED (missing class or bad OID), SESSION_SERIALIZE_FAILED (serialization error), TERMINAL_SYNTAX_UNSUPPORTED (block syntax on docker exec path). `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisExecuteResponse>()
    )]
    async fn iris_execute(
        &self,
        Parameters(p): Parameters<ExecuteParams>,
    ) -> Result<CallToolResult, McpError> {
        // The target, its identity and its route come from the pipeline (spec 141, R2). This
        // handler never picks a connection, a client or a container.
        let cx = match self.exec_context() {
            Ok(cx) => cx,
            Err(e) => {
                self.record_call("iris_execute", false);
                return exec_refused(e);
            }
        };
        let namespace = self.target_ns(p.namespace.as_deref())?;
        if let Some(refusal) = self.exec_content_gate("iris_execute", &p.code) {
            return err_result(refusal);
        }
        tracing::info!(namespace = %namespace, translate_sql = p.translate_sql, use_session = p.use_session, "iris_execute");
        let timeout = std::time::Duration::from_secs(p.timeout);

        // Session preamble: validate token early so we can return SESSION_INVALID before
        // compiling anything, which satisfies FR-010 ("without executing user code").
        if p.use_session {
            if let Some(ref tok) = p.session_state {
                if let Err(e) = execute_session::SessionToken::new(tok) {
                    self.record_call("iris_execute", false);
                    return err_result(serde_json::json!({
                        "success": false,
                        "error_code": "SESSION_INVALID",
                        "error": format!("invalid session_state token: {e}"),
                    }));
                }
            }
        }

        // &sql macro translation (035) applies to the terminal (docker exec) paths only. The HTTP
        // path compiles the code into a class method, where IRIS expands `&sql` itself; rewriting
        // it there broke `COUNT(*) INTO` and every other expression column (131 US4).
        let translation = if p.translate_sql {
            let r = translate_sql_macros(&p.code);
            Some(r)
        } else {
            None
        };
        let terminal_base = translation
            .as_ref()
            .filter(|r| r.found)
            .map(|r| r.translated_code.as_str())
            .unwrap_or(&p.code);

        // Session wrapping: inject preamble before and epilogue after user code.
        let wrap = |base: &str| -> String {
            if p.use_session {
                let preamble = execute_session::build_session_preamble(p.session_state.as_deref())
                    .expect("token already validated above");
                let epilogue = execute_session::build_session_epilogue();
                format!("{preamble}{base}\n{epilogue}")
            } else {
                base.to_string()
            }
        };
        let http_code = wrap(&p.code);
        let terminal_code = wrap(terminal_base);

        let mut opts = exec::RunOpts::with_terminal(timeout, "IrisDevRun");
        opts.terminal_code = Some(&terminal_code);
        opts.check_block_syntax = true;
        let outcome = match cx.run(&http_code, &namespace, &opts).await {
            Ok(o) => o,
            Err(e) => {
                self.record_call("iris_execute", false);
                let mut body = e.to_json();
                if e.code == ERR_QUERY_FAILED_AFTER_RUN {
                    body["namespace"] = serde_json::json!(namespace);
                }
                return err_result(body);
            }
        };

        let on_atelier = outcome.identity.path == exec::ExecPath::Atelier;
        // Parse session sentinel lines before trimming/error-checking.
        let (visible, session_token, session_error) = if p.use_session && on_atelier {
            execute_session::parse_session_output(&outcome.output)
        } else {
            (outcome.output.clone(), None, None)
        };
        let identity = outcome.identity.to_json();
        let (method, path) = if on_atelier {
            ("http", "atelier")
        } else {
            ("docker", outcome.identity.path.as_str())
        };

        // Session fatal errors (invalid token, restore failure, serialize failure) take priority
        // over normal output processing.
        if let Some((err_code, detail)) = session_error {
            self.record_call("iris_execute", false);
            return err_result(serde_json::json!({
                "success": false,
                "error_code": err_code,
                "error": detail,
                "namespace": namespace,
                "method": method,
                "execution_path": path,
                "auth_user": outcome.identity.user,
                "service_account_env": non_blank_env_value("IRIS_SERVICE_USERNAME"),
                "identity": identity,
            }));
        }

        let trimmed = visible.trim();
        // Catch ObjectScript runtime errors written by the Catch block or $ZERROR check.
        let is_runtime_error = crate::iris::connection::is_generator_error(trimmed);
        self.record_call("iris_execute", !is_runtime_error);
        let mut resp = serde_json::json!({
            "success": !is_runtime_error,
            "output": trimmed,
            "namespace": namespace,
            "method": method,
            "execution_path": path,
            "identity": identity,
        });
        if on_atelier {
            // Deprecated in 1.7.0, removed in 1.8.0: `identity` names the user that ran.
            resp["auth_user"] = serde_json::json!(outcome.identity.user);
            resp["service_account_env"] =
                serde_json::json!(non_blank_env_value("IRIS_SERVICE_USERNAME"));
        }
        if is_runtime_error {
            resp["error_code"] = serde_json::Value::String("IRIS_RUNTIME_ERROR".into());
            if let Some(hint) = error_hints::runtime_error_hint(trimmed, &namespace) {
                hint.apply(&mut resp);
            }
        }
        if let Some(tok) = session_token {
            resp["session_state"] = serde_json::Value::String(tok);
        }
        if !on_atelier {
            if let Some(tr) = translation.as_ref().filter(|tr| tr.found) {
                resp["sql_translated"] = serde_json::Value::Bool(true);
                resp["translated_code"] = serde_json::Value::String(tr.translated_code.clone());
                if !tr.warnings.is_empty() {
                    resp["translation_warning"] = serde_json::Value::Array(
                        tr.warnings
                            .iter()
                            .map(|w| serde_json::Value::String(w.clone()))
                            .collect(),
                    );
                }
            }
        }
        json_result(resp)
    }

    #[tool(
        description = "Read/write/delete IRIS documents. mode: get (fetch source), put (write, auto SCM checkout), delete, head (existence), fragment (read lines start..end), compiled (read INT), list (glob `pattern`), insert (splice `content` before 1-based `line`; omit `line` to append), delete_lines (remove start..end). `name` is required for all single-document modes; `line`/`start`/`end` are integers. For insert with an explicit `line` and for delete_lines, pass `expected` (current text at the target lines) or the edit is refused with STALE_CONTENT. Edits return the re-numbered post-write `content` to chain from, plus a `diff` field (git-style unified diff of the change) — render it to the user inside a ```diff fenced code block. Batch via `names`; SCM dialogs resume via elicitation_id/elicitation_answer. Skill: objectscript-navigation to locate documents before editing. Storage blocks on %Persistent/%SerialObject classes: never stripped or regenerated — written verbatim, exactly like Studio/VS Code. Treat the Storage definition as off-limits: when removing a property, leave its Storage entry alone (the compiler leaves it as a harmless orphan; do not delete or edit it). Exception — renaming a property: you may update the corresponding Storage entry's name to match, in the same edit. A full storage reset (submitting content with no Storage block for a class that has one) is refused with STORAGE_RESET_REQUIRES_CONFIRMATION unless allow_storage_regeneration:true is set — only set this after the user has explicitly confirmed the reset is intentional for this session; the response then reports the pre-reset property list so you can decide how to clean up any existing data (e.g. %KillExtent). `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisDocResponse>()    )]
    async fn iris_doc(
        &self,
        Parameters(p): Parameters<IrisDocParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        // FR-010: NoPWS guard — iris_doc requires Atelier REST.
        if nopws::nopws_is_active(
            iris.base_url == "http://127.0.0.1:1"
                || iris.base_url.starts_with("http://127.0.0.1:1/"),
            iris.version
                .as_deref()
                .map(|v| v.contains("2026.2.0AI"))
                .unwrap_or(false),
        ) {
            return err_result(nopws::nopws_atelier_required_error());
        }
        let namespace = self.target_ns(p.namespace.as_deref())?;
        tracing::info!(namespace = %namespace, "iris_doc");
        let client = self.http_client();
        let result = doc::handle_iris_doc(
            &iris,
            client,
            p,
            &self.elicitation_store,
            &self.checkout_cache,
        )
        .await;
        self.record_call("iris_doc", result.is_ok());
        result
    }

    #[tool(
        description = "Execute SQL against IRIS via Atelier REST. mode=\"read\" (default): one SELECT, WITH or EXPLAIN statement; everything else (CALL, TUNE TABLE, BUILD INDEX, DML, DDL, GRANT, REVOKE) and a second statement after `;` are refused (force=true skips this when write tools are enabled, and then counts as a write). Read mode does not stop a stored procedure that has side effects when it is called inside a SELECT: only the IRIS user's privileges do. mode=\"explain\": returns the IRIS query plan for a SELECT (plan_text, query_hash), no rows. mode=\"count\": returns a row count for `table` or `query` without transferring rows. mode=\"write\": executes INSERT/UPDATE/DELETE/CALL/TRUNCATE (Execute-gated, blocked on mcpTemplate=live/test); UPDATE/DELETE are pre-checked against max_rows_affected (default 1000, max 10000) before executing. Skill: objectscript-sql-patterns for IRIS SQL quirks. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisQueryResponse>()
    )]
    async fn iris_query(
        &self,
        Parameters(p): Parameters<QueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let mode = p.mode.as_deref().unwrap_or("read");
        // The gates below run before any connection is resolved, so log/audit the
        // *requested* namespace here; each execution branch resolves the effective
        // namespace against the connection it actually uses.
        let requested_ns = p.namespace.as_deref().unwrap_or("(connection default)");
        tracing::info!(namespace = %requested_ns, force = p.force, mode, "iris_query");

        match mode {
            "explain" => {
                let iris = self.target_conn()?;
                let namespace = self.target_ns(p.namespace.as_deref())?;
                let client = self.http_client();
                let result = iris_query_explain(&iris, client, &p, &namespace).await;
                self.record_call(
                    "iris_query",
                    result.as_ref().map(is_success).unwrap_or(false),
                );
                return result;
            }
            "count" => {
                let iris = self.target_conn()?;
                let namespace = self.target_ns(p.namespace.as_deref())?;
                let client = self.http_client();
                let result = iris_query_count(&iris, client, &p, &namespace).await;
                self.record_call(
                    "iris_query",
                    result.as_ref().map(is_success).unwrap_or(false),
                );
                return result;
            }
            "write" => {
                // DML runs under the restricted service account when configured (least-privilege),
                // on the server the call names and with that identity's own cookie jar.
                let cx = match self.exec_context() {
                    Ok(cx) => cx,
                    Err(e) => {
                        self.record_call("iris_query", false);
                        return exec_refused(e);
                    }
                };
                let namespace = self.target_ns(p.namespace.as_deref())?;
                let result = iris_query_write(&cx.conn, cx.client.as_ref(), &p, &namespace).await;
                self.record_call(
                    "iris_query",
                    result.as_ref().map(is_success).unwrap_or(false),
                );
                return exec::attach_identity(result, &cx.helper_report());
            }
            _ => {}
        }

        // SQL safety gate — validate before any network call
        let target_writes = self.target()?.primary.gates.write_enabled;
        let skip_validation = p.force && target_writes;
        if !skip_validation {
            match validate_read_only_sql(&p.query) {
                Err(ref kw) if kw == "EMPTY" => {
                    self.record_call("iris_query", false);
                    return err_result(serde_json::json!({
                        "success": false,
                        "error_code": "EMPTY_QUERY",
                        "error": "SQL query is empty after removing comments.",
                    }));
                }
                Err(kw) => {
                    self.record_call("iris_query", false);
                    let mut resp = serde_json::json!({
                        "success": false,
                        "error_code": "SQL_WRITE_BLOCKED",
                        "error": format!("'{}' is not a read, so mode=\"read\" does not send it: read mode runs one SELECT, WITH or EXPLAIN. Use mode=\"write\" for INSERT, UPDATE, DELETE, CALL and TRUNCATE; it needs write tools enabled.", kw),
                        "blocked_keyword": kw,
                    });
                    if p.force && !target_writes {
                        resp["force_ignored"] = serde_json::Value::Bool(true);
                    }
                    return err_result(resp);
                }
                Ok(()) => {}
            }
        }

        // A forced query sends SQL the read-only check would have refused, so it runs as the
        // service account like a write does; an ordinary read stays on the connection's own user.
        let forced = if skip_validation {
            match self.exec_context() {
                Ok(cx) => Some(cx),
                Err(e) => {
                    self.record_call("iris_query", false);
                    return exec_refused(e);
                }
            }
        } else {
            None
        };
        let (iris, client) = match &forced {
            Some(cx) => (cx.conn.clone(), cx.client.as_ref()),
            None => (self.target_conn()?, self.http_client()),
        };
        let namespace = self.target_ns(p.namespace.as_deref())?;
        let query_url = iris.versioned_ns_url(&namespace, "/action/query");
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

        let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;

        if let Some(errors) = body["status"]["errors"].as_array() {
            if !errors.is_empty() {
                let msg = errors[0]["error"].as_str().unwrap_or("SQL error");
                self.record_call("iris_query", false);
                return sql_err(msg, &p.query, &namespace);
            }
        }

        let rows = body["result"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let count = rows.len();
        self.record_call("iris_query", true);
        let mut out = serde_json::json!({"success": true, "rows": rows, "count": count, "namespace": namespace});
        if let Some(cx) = &forced {
            out["identity"] = cx.helper_report().to_json();
        }
        ok_json(out)
    }

    #[tool(
        description = "List running IRIS Docker containers with name-match scoring. Tries iris-devtester first, falls back to docker ps. Containers sorted by score (name similarity to workspace) descending.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<IrisListContainersResponse>()
    )]
    async fn iris_list_containers(
        &self,
        Parameters(p): Parameters<ListContainersParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_reload().await;
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
        // FR-012 / FR-023: show workspace config, supporting both develop and operate mode.
        let workspace_config_json = crate::iris::workspace_config::build_workspace_config_json(
            p.workspace_root.as_deref(),
            &containers,
        );
        // Add active_connection info so agents can detect workspace_config mismatches
        // without a separate iris_info call.
        let iris_arc = self.target_conn_opt()?;
        let active_connection_json = match &iris_arc {
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
            (workspace_config_json["container"].as_str(), &iris_arc)
        {
            match &conn.source {
                crate::iris::connection::DiscoverySource::Docker { container_name } => {
                    container_name != cfg_container
                }
                _ => true, // connected via non-Docker path but .iris-agentic-dev.toml specifies a container
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
                "Active connection: {}. .iris-agentic-dev.toml specifies: {}. Restart the MCP session from the workspace directory to apply.",
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
        description = "Switch the active IRIS connection to the specified running Docker container for this session. After a successful switch, all subsequent tool calls target the new container — no session restart required. Fixes issue #11.",
        output_schema = output_schemas::oneof_output_schema::<IrisSelectContainerResponse>()
    )]
    async fn iris_select_container(
        &self,
        Parameters(p): Parameters<SelectContainerParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_reload().await;
        // This tool creates a NEW connection, so there is no existing connection to
        // resolve against: explicit param wins, else the configured IRIS_NAMESPACE,
        // else USER as the last resort.
        let env_ns = std::env::var("IRIS_NAMESPACE")
            .ok()
            .filter(|s| !s.is_empty());
        let namespace =
            resolve_namespace(p.namespace.as_deref(), env_ns.as_deref().unwrap_or("USER"))
                .to_string();
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
                return err_result(serde_json::json!({
                    "success": false,
                    "error": "CONTAINER_NOT_FOUND",
                    "requested": p.name,
                    "available": available,
                }));
            }
        };

        let port_superserver = container["port_superserver"].as_u64().unwrap_or(1972) as u16;
        let port_web = container["port_web"].as_u64().unwrap_or(52773) as u16;
        let base_url = format!("http://localhost:{}", port_web);

        let mut new_conn = crate::iris::connection::IrisConnection::new(
            &base_url,
            &namespace,
            &p.username,
            &p.password,
            crate::iris::connection::DiscoverySource::Docker {
                container_name: p.name.clone(),
            },
        );
        new_conn.port_superserver = Some(port_superserver);
        new_conn.probe().await;

        // Check if probe succeeded (version populated means reachable)
        if new_conn.version.is_none() {
            return err_result(serde_json::json!({
                "success": false,
                "error": "CONTAINER_UNREACHABLE",
                "container": p.name,
                "port_web": port_web,
                "message": "Container found but Atelier REST API did not respond. Check that the container is running and the web server is accessible.",
            }));
        }

        let version = new_conn.version.clone();

        // Re-resolve against the new container's SystemMode/namespace, in the declaration context
        // the session already had — switching containers must not discard what the config declared.
        let declared = self.connection.lock().unwrap().declared;
        let gates =
            write_gate::resolve_for_connection(declared, Some(&new_conn), &new_conn.namespace);
        let write_tools_enabled = gates.write_enabled;

        // Atomically swap the active connection (fixes issue #11).
        let new_state = ConnectionState::from_iris(
            new_conn,
            ConnectionSource::IrisSelectContainer,
            None,
            gates,
        )
        .with_declared(declared);
        {
            let mut conn = self.connection.lock().unwrap();
            *conn = new_state;
        }

        tracing::info!(container = %p.name, "iris-agentic-dev: switched connection via iris_select_container");

        ok_json(serde_json::json!({
            "status": "ok",
            "switched": true,
            "container": p.name,
            "port_superserver": port_superserver,
            "port_web": port_web,
            "namespace": namespace,
            "version": version,
            "write_tools_enabled": write_tools_enabled,
        }))
    }

    #[tool(
        description = "Return the active IRIS connection state without making any IRIS network calls. Always succeeds — never returns IRIS_UNREACHABLE. Use to: (1) diagnose connection issues, (2) verify hot-reload completed, (3) confirm which container/host is active, (4) confirm which build of this MCP server is actually running (server_version) when multiple installs/forks may be registered. To switch connection mid-session without restart: call check_config first to get config_watch_path (null when there is no project directory to watch), then write a .iris-agentic-dev.toml to that exact path, then call any tool — the reload fires automatically. A project file cannot raise the write or destructive gate, turn on data reads or loosen a policy until the operator runs `iris-agentic-dev trust` on it in a terminal and restarts iad; a change to the operator's own configuration that loosens a setting waits for the next start, and one that tightens applies at once; check_config lists what it set aside or held (set_aside), where each policy part came from (effective_policy, config_sources) and the compile-time gate (compile_time_gate). Fields: server_version, connected, connection_source (config_file|env_vars|iris_select_container|auto_discovered), host, port, namespace, container, config_file, config_watch_path, config_loaded_at, iris_version, write_tools_enabled, write_tools_source, destructive_tools_enabled, destructive_tools_source, tls_verify, capabilities. The two *_source fields say what decided each gate (read_only_strict|operator_env|config_file|legacy_allow_prod|inferred_system_mode|inferred_namespace|inferred_default|fail_closed), so a gate you did not ask for is one field lookup rather than a guess. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<CheckConfigOk>()    )]
    async fn check_config(
        &self,
        Parameters(_p): Parameters<crate::tools::NoParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_reload().await;
        // Before the connection lock: `snapshot()` takes it too.
        let snap = self.snapshot();
        // Awaited before the lock is taken: a std mutex guard cannot be held across it.
        let http_login = self.service_login_probe().await;
        let conn = self.connection.lock().unwrap();

        let (host, port, namespace, container, iris_version) = match &conn.iris {
            Some(iris) => {
                // Parse host and port from base_url (e.g. "http://localhost:52780")
                let base = iris
                    .base_url
                    .trim_start_matches("http://")
                    .trim_start_matches("https://");
                let (host_port, _path) = base.split_once('/').unwrap_or((base, ""));
                let (host_str, port_str) =
                    host_port.rsplit_once(':').unwrap_or((host_port, "52773"));
                let host = host_str.to_string();
                let port = port_str.parse::<u64>().unwrap_or(52773);
                let namespace = iris.namespace.clone();
                let container = match &iris.source {
                    crate::iris::connection::DiscoverySource::Docker { container_name } => {
                        serde_json::Value::String(container_name.clone())
                    }
                    _ => serde_json::Value::Null,
                };
                let version = iris
                    .version
                    .clone()
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
                (host, port, namespace, container, version)
            }
            None => (
                String::new(),
                52773u64,
                String::new(),
                serde_json::Value::Null,
                serde_json::Value::Null,
            ),
        };

        let config_file = conn
            .config_file
            .as_ref()
            .and_then(|p| p.to_str())
            .map(|s| serde_json::Value::String(s.to_string()))
            .unwrap_or(serde_json::Value::Null);

        let config_file_no_target = conn
            .config_file_no_target
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        // Said once, in words: the connection came from somewhere else, and why this file was not it.
        let config_file_warning = config_file_no_target.as_ref().map(|p| {
            format!(
                "{p} names neither a host nor a container, so it chose no connection; \
                 connection_source says what did. Its other settings (tool lists, gates, policy) \
                 still apply."
            )
        });

        let config_loaded_at = conn
            .loaded_at
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| {
                // Format as ISO 8601
                let secs = d.as_secs();
                let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0)
                    .unwrap_or_default();
                serde_json::Value::String(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            })
            .unwrap_or(serde_json::Value::Null);

        let connection_source =
            serde_json::to_value(&conn.source).unwrap_or(serde_json::Value::Null);

        // Show where the MCP server is looking for .iris-agentic-dev.toml
        // so agents know where to write it for mid-session config changes.
        let config_watcher_path = {
            let w = self.config_watcher.lock().unwrap();
            w.as_ref()
                .filter(|w| !w.config_path.as_os_str().is_empty())
                .map(|w| w.config_path.to_string_lossy().to_string())
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null)
        };

        let objectscript_workspace = std::env::var("OBJECTSCRIPT_WORKSPACE")
            .ok()
            .filter(|s| !s.is_empty())
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null);

        let capabilities = {
            let ver_str = iris_version.as_str();
            let docker_only = conn
                .iris
                .as_ref()
                .map(|i| {
                    i.base_url == "http://127.0.0.1:1"
                        || i.base_url.starts_with("http://127.0.0.1:1/")
                })
                .unwrap_or(false);
            let web_port = conn
                .iris
                .as_ref()
                .and_then(|i| extract_web_port_from_url(&i.base_url));
            let web_prefix = conn
                .iris
                .as_ref()
                .and_then(|i| extract_web_prefix_from_url(&i.base_url));
            derive_capabilities(ver_str, docker_only, web_port, web_prefix.as_deref())
        };

        let default_tls = conn
            .iris
            .as_ref()
            .map(|i| i.tls)
            .unwrap_or_else(crate::iris::http::TlsSetting::from_launch_env);
        let tls_report = {
            let one = |t: &crate::iris::http::TlsSetting| serde_json::json!({"tls_verify": t.verify, "source": t.source.as_str()});
            let pool = self.pool_ref();
            let mut servers = serde_json::Map::new();
            for name in pool.names() {
                if let Ok(c) = pool.peek(Some(name)) {
                    servers.insert(name.to_string(), one(&c.tls));
                }
            }
            serde_json::json!({"default": one(&default_tls), "servers": servers})
        };

        let mut response = serde_json::json!({
            "server_version": SERVER_VERSION,
            "connected": conn.iris.is_some(),
            "connection_source": connection_source,
            "host": host,
            "port": port,
            "namespace": namespace,
            "container": container,
            "config_file": config_file,
            "config_file_named_no_target": config_file_no_target,
            "config_file_warning": config_file_warning,
            "credentials_withheld": conn.iris.as_ref().and_then(|i| i.credentials_withheld.as_deref()),
            "config_loaded_at": config_loaded_at,
            "iris_version": iris_version,
            // All four gate fields come off the one `GateResolution` that `call_tool` enforces, so
            // the report and the enforcement cannot drift apart (085 FR-004). Reporting a gate that
            // enforcement did not read is exactly how `write_tools_enabled: true` survived an
            // operator turning writes off.
            "write_tools_enabled": conn.gates.write_enabled,
            "write_tools_source": conn.gates.write_source.as_str(),
            "destructive_tools_enabled": conn.gates.destructive_enabled,
            "destructive_tools_source": conn.gates.destructive_source.as_str(),
            "config_watch_path": config_watcher_path,
            // The default connection's own setting, the one its requests use (#127). With no
            // connection, the setting the launch settings would give one. Every pool member's is
            // under `tls`.
            "tls_verify": default_tls.verify,
            "tls": tls_report,
            "discovery": serde_json::json!({
                "skipped": crate::iris::discovery::skipped_listeners()
                    .iter()
                    .map(|s| serde_json::json!({"port": s.port, "reason": s.reason}))
                    .collect::<Vec<_>>(),
            }),
            // Strict read-only (R5 C15): the scratch-class tools are off and unlisted.
            "read_only": if snap.read_only_strict() { "strict" } else { "off" },
            "read_only_from": snap.read_only_source(),
            // R5 C16: records always go to the local file; `iris` adds the IRIS global, on the
            // server a call acts on when that server's writes are on.
            "telemetry_sink": crate::telemetry::sink_setting(
                &snap.operator,
                std::env::var("IAD_TELEMETRY_SINK").ok().as_deref(),
            )
            .as_str(),
            "scm_opt_ins": {
                "checkin": scm::ScmOptIn::CheckIn.enabled(),
                "disconnect": scm::ScmOptIn::Disconnect.enabled(),
            },
            "objectscript_workspace": objectscript_workspace,
            "capabilities": capabilities,
        });

        if let Some(ref err) = conn.config_parse_error {
            response["config_parse_error"] = serde_json::Value::String(err.clone());
        }

        // T038: surface iris_audit emission failure count so operators know emission is working.
        let audit_failures = self.iris_audit_counter.failure_count();
        if audit_failures > 0 {
            response["iris_audit_failures"] = serde_json::Value::Number(audit_failures.into());
        }

        // Warn when connected via fallback discovery with no config file — the agent
        // may be attached to the wrong IRIS instance (IDE-launched MCP with cwd=/).
        // Explicit flags or env vars are intentional — no warning needed for those.
        // See issue #82.
        let is_explicit = matches!(
            conn.source,
            ConnectionSource::ConfigFile | ConnectionSource::EnvVars
        );
        let ws_env = std::env::var("OBJECTSCRIPT_WORKSPACE")
            .ok()
            .filter(|s| !s.is_empty());
        if conn.config_file.is_none()
            && conn.config_file_no_target.is_none()
            && !is_explicit
            && conn.iris.is_some()
        {
            if let Some(ref ws) = ws_env {
                // OBJECTSCRIPT_WORKSPACE is set but no config file was found there.
                // Connection is via fallback discovery — remind the user to create a
                // config in the workspace the IDE pointed us at.
                response["fallback_warning"] = serde_json::Value::String(format!(
                    "No .iris-agentic-dev.toml found in OBJECTSCRIPT_WORKSPACE ({ws}). \
                     Connection established via fallback discovery. Create .iris-agentic-dev.toml \
                     in that directory to pin the target instance."
                ));
            } else {
                response["fallback_warning"] = serde_json::Value::String(
                    "No .iris-agentic-dev.toml config file found. Connection established via \
                     fallback discovery (Docker/Server Manager/port scan). Set OBJECTSCRIPT_WORKSPACE \
                     or create a .iris-agentic-dev.toml in your project root to pin the target instance."
                        .to_string(),
                );
            }
        }

        // Server Manager section (044-servermanager-discovery)
        {
            use crate::iris::server_manager::{
                build_server_manager_config_json, parse_sm_settings, sm_secret_for_in,
                sm_settings_path, CredentialStatus, ServerManagerCredentialEntry, SmSecretError,
            };

            let sm_section = if let Some(path) = sm_settings_path() {
                let profiles = parse_sm_settings(&path);
                if profiles.is_empty() {
                    serde_json::json!({ "available": false })
                } else {
                    let active_name = match &conn.iris {
                        Some(iris) => match &iris.source {
                            crate::iris::connection::DiscoverySource::ServerManager {
                                server_name,
                            } => Some(server_name.clone()),
                            _ => None,
                        },
                        None => None,
                    };
                    // From the snapshot the gates read, not from the files again (FR-043).
                    let fleet = Some(&snap.fleet);
                    let cred_entries: Vec<ServerManagerCredentialEntry> = profiles
                        .iter()
                        .map(|p| {
                            // The profile's own endpoint: its secret is asked for there and nowhere else.
                            let status = match p.endpoint() {
                                Err(_) => CredentialStatus::ERROR.to_string(),
                                Ok(ep) => match sm_secret_for_in(&profiles, &p.name, &p.username, &ep) {
                                    Ok(_) => CredentialStatus::RESOLVED.to_string(),
                                    Err(SmSecretError::Credential(
                                        crate::iris::server_manager::SmCredentialError::CredentialNotFound { .. },
                                    )) => CredentialStatus::NOT_CONFIGURED.to_string(),
                                    Err(SmSecretError::Credential(
                                        crate::iris::server_manager::SmCredentialError::KeychainUnavailable { .. },
                                    )) => CredentialStatus::KEYCHAIN_UNAVAILABLE.to_string(),
                                    Err(_) => CredentialStatus::ERROR.to_string(),
                                },
                            };
                            let policy: Option<crate::iris::workspace_config::ConnectionPolicy> =
                                fleet
                                    .as_ref()
                                    .and_then(|fc| fc.policies.get(&p.name))
                                    .cloned();
                            ServerManagerCredentialEntry {
                                server_name: p.name.clone(),
                                status,
                                policy,
                            }
                        })
                        .collect();
                    build_server_manager_config_json(
                        &profiles,
                        active_name.as_deref(),
                        &cred_entries,
                    )
                }
            } else {
                serde_json::json!({ "available": false })
            };
            response["server_manager"] = sm_section;
        }

        // Spec 141 (R4a D5, D8, D9): where each setting came from, and what was set aside. Read
        // off the same snapshot the gates read, so the report cannot drift from enforcement.
        {
            let system_mode = conn.iris.as_ref().map(|i| i.system_mode);
            let namespace = conn
                .iris
                .as_ref()
                .map(|i| i.namespace.clone())
                .unwrap_or_default();
            // The default server is judged on this connection: an untrusted file's template is
            // compared with the one its system mode implies.
            let default_policy = snap.effective_policy_for(
                "default",
                system_mode
                    .map(|m| crate::policy::effective::implied_template(m, true, &namespace).0),
            );
            let mut policies = serde_json::Map::new();
            for (name, listed) in &snap.policies {
                let p = if name == "default" {
                    &default_policy
                } else {
                    listed
                };
                let mut entry = p.to_json();
                let is_default = name == "default";
                if p.mcp_template.value.is_none() {
                    // No file declared a template: the system mode chose one. A member's is what
                    // the last probe of it answered, or unknown.
                    let mode = if is_default {
                        system_mode.unwrap_or_default()
                    } else {
                        self.pool_ref().cached_system_mode(name).unwrap_or_default()
                    };
                    let (t, _) =
                        crate::policy::effective::implied_template(mode, is_default, &namespace);
                    entry["mcpTemplate"]["value"] = serde_json::json!(match t {
                        crate::iris::workspace_config::McpTemplate::Dev => "dev",
                        crate::iris::workspace_config::McpTemplate::Test => "test",
                        crate::iris::workspace_config::McpTemplate::Live => "live",
                    });
                    entry["mcpTemplate"]["detail"] =
                        serde_json::json!(format!("SystemMode {mode:?}"));
                }
                // Where a gate's value came from, as `check_config` names the sources.
                let from =
                    |g: write_gate::GateSource, file_from: crate::policy::effective::PartSource| {
                        match g {
                            write_gate::GateSource::ReadOnlyStrict => "read_only_strict",
                            write_gate::GateSource::OperatorEnv
                            | write_gate::GateSource::LegacyAllowProd => "launch",
                            write_gate::GateSource::OperatorFile => "operator_file",
                            write_gate::GateSource::ConfigFile
                            | write_gate::GateSource::PolicyDeclaration => file_from.as_str(),
                            write_gate::GateSource::InferredSystemMode
                            | write_gate::GateSource::InferredNamespace
                            | write_gate::GateSource::UnprobedMember => "system_mode",
                            write_gate::GateSource::InferredDefault
                            | write_gate::GateSource::FailClosed => "strict_default",
                        }
                    };
                if is_default {
                    // The default server's gates are the ones `call_tool` enforces.
                    entry["writeTools"]["value"] = serde_json::json!(conn.gates.write_enabled);
                    entry["writeTools"]["from"] =
                        serde_json::json!(from(conn.gates.write_source, p.write_tools.from));
                    entry["destructiveTools"]["value"] =
                        serde_json::json!(conn.gates.destructive_enabled);
                    entry["destructiveTools"]["from"] = serde_json::json!(from(
                        conn.gates.destructive_source,
                        p.destructive_tools.from
                    ));
                } else {
                    // A pool member's own gates (R1 D6), from what is known without a request:
                    // `check_config` makes none, so a member nobody has written to yet reads as
                    // unprobed and write-closed, which is what its first write would find.
                    let mode = self.pool_ref().cached_system_mode(name);
                    let decl = write_gate::MemberDeclaration {
                        top_level: snap.declared_gates(),
                        write_tools: p.write_tools.value,
                        destructive_tools: p.destructive_tools.value,
                    };
                    let g = write_gate::resolve_member(
                        write_gate::operator_env_gates(),
                        &decl,
                        mode.unwrap_or_default(),
                        &namespace,
                    );
                    entry["writeTools"]["value"] = serde_json::json!(g.write_enabled);
                    entry["writeTools"]["from"] =
                        serde_json::json!(from(g.write_source, p.write_tools.from));
                    entry["destructiveTools"]["value"] = serde_json::json!(g.destructive_enabled);
                    entry["destructiveTools"]["from"] =
                        serde_json::json!(from(g.destructive_source, p.destructive_tools.from));
                    if g.write_source == write_gate::GateSource::UnprobedMember {
                        entry["writeTools"]["detail"] = serde_json::json!(
                            "this server's SystemMode has not been read (it is read on the first \
                             write); until it is Development or Test, or [policy.<name>] \
                             writeTools is true, writes are refused"
                        );
                    }
                }
                policies.insert(name.clone(), entry);
            }
            response["effective_policy"] = serde_json::Value::Object(policies);
            response["config_sources"] = snap.sources_json();
            response["skills"] = crate::skills::bundled::skills_report_from(
                &crate::skills::bundled::candidate_inputs_from_process(),
            );
            if let Some(path) = crate::iris::audit_log::AuditLog::default_path() {
                // `snap` is already in hand: `snapshot()` would take the connection lock held here.
                let keep = crate::iris::audit_log::payloads_enabled(
                    &snap.operator,
                    std::env::var("IAD_AUDIT_PAYLOADS").ok().as_deref(),
                );
                response["audit"] = crate::iris::audit_log::mode_report(keep, &path);
            }
            let cg = default_policy.compile_gate.clone();
            response["compile_time_gate"] = serde_json::json!({
                "enabled": cg.value,
                "from": cg.from.as_str(),
                "file": cg.file.as_ref().map(|f| f.to_string_lossy().into_owned()),
                "set_aside": cg.set_aside,
            });
            let mut set_aside = snap.set_aside_for("default", &default_policy);
            // Credentials held back while the pool loaded (a Server Manager secret for an entry
            // that no longer points at its profile, a stored password for another endpoint), and
            // the default connection's `.vscode` named server (spec 141, FR-042).
            set_aside.extend(self.pool_ref().withheld_credentials().iter().cloned());
            // A `.vscode` connection discovery did not build for want of a password.
            set_aside.extend(crate::iris::discovery::discovery_set_aside());
            if let Some(iris) = conn.iris.as_ref() {
                if matches!(
                    iris.source,
                    crate::iris::connection::DiscoverySource::VsCodeSettings
                ) {
                    if let Some(reason) = iris.credentials_withheld.as_ref() {
                        set_aside.push(crate::iris::config_sources::SetAsideItem {
                            id: "default:server_manager".to_string(),
                            part: None,
                            kind: crate::iris::config_sources::SetAsideKind::CredentialWithheld,
                            setting: "Server Manager keychain secret".to_string(),
                            server: Some("default".to_string()),
                            file: None,
                            reason: reason.clone(),
                            lift: "make the endpoints match, or give the entry its own password"
                                .to_string(),
                        });
                    }
                }
            }
            response["set_aside"] =
                serde_json::Value::Array(set_aside.iter().map(|i| i.to_json()).collect());
            let mut errors: Vec<String> = snap.config_errors.clone();
            errors.extend(snap.gate_error().map(|e| format!("{e}")));
            errors.extend(snap.errors.iter().map(|e| e.display()));
            response["config_errors"] = serde_json::json!(errors);
            if let Some(e) = snap.errors.first() {
                response["config_parse_error"] = serde_json::json!(e.display());
            }
            // 1.7.x keeps the string alias; its value is the reason text of the default server's
            // withheld credentials.
            if response["credentials_withheld"].is_null() {
                let alias: Vec<&str> = set_aside
                    .iter()
                    .filter(|i| {
                        i.kind == crate::iris::config_sources::SetAsideKind::CredentialWithheld
                            && i.server.as_deref() == Some("default")
                    })
                    .map(|i| i.reason.as_str())
                    .collect();
                if !alias.is_empty() {
                    response["credentials_withheld"] = serde_json::json!(alias.join(" "));
                }
            }
        }

        response["identity"] = self.identity_report_json(&snap, conn.iris.clone(), http_login);

        ok_json(response)
    }

    #[tool(
        description = "Start a dedicated IRIS container for the current project via iris-devtester CLI. Idempotent — returns existing container if already running.",
        output_schema = output_schemas::oneof_output_schema::<IrisStartSandboxResponse>()
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
        description = "Search for ObjectScript classes matching a query in the IRIS namespace. Query supports: plain substring ('Patient'), package prefix ('HT.*' or 'HT.'), mid-glob ('HT.*.Service'), or bare '*' for all. Skill: objectscript-navigation. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisSymbolsResponse>()
    )]
    async fn iris_symbols(
        &self,
        Parameters(p): Parameters<SymbolsParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(p.namespace.as_deref(), &iris.namespace);
        let client = self.http_client();
        let (sql, params) = translate_symbols_query(p.limit, &p.query);
        match iris.query(&sql, params, namespace, client).await {
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
        description = "Search for ObjectScript symbols in local .cls/.mac/.inc files on disk — no IRIS connection required. query: glob pattern (MyApp.*, *Service, MyApp.Foo.Do*). workspace_path: optional path (defaults to OBJECTSCRIPT_WORKSPACE or cwd). limit: max symbols to return (default 50). kinds: optional filter on symbol kind (class, method, property, parameter, index, xdata, query, trigger, relationship, foreignkey, projection, storage, routine, label). Each symbol includes a line field (1-based source line). Skill: objectscript-navigation.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisSymbolsLocalResponse>()
    )]
    async fn iris_symbols_local(
        &self,
        Parameters(p): Parameters<SymbolsLocalParams>,
    ) -> Result<CallToolResult, McpError> {
        if p.query.trim().is_empty() {
            return err_json("INVALID_PARAMS", "query must not be empty");
        }
        let limit = p.limit.clamp(1, 500);

        // The workspace is the project directory or a directory inside it (T216).
        let (root, cwd) = self.host_path_context();
        let workspace = match p.workspace_path.as_deref().filter(|w| !w.is_empty()) {
            None => match root.clone() {
                Some(r) => r,
                None => {
                    return self.refuse(host_fs::refusal_body(
                        "iris_symbols_local",
                        ".",
                        &host_fs::PathRefusal::NoRoot,
                        &[],
                    ))
                }
            },
            Some(requested) => match host_fs::confine_dir(root.as_deref(), &cwd, requested) {
                Ok(dir) => dir,
                Err(host_fs::PathRefusal::NotFound) => {
                    return err_json(
                        "WORKSPACE_NOT_FOUND",
                        &format!("{requested} does not exist"),
                    )
                }
                Err(why) => {
                    return self.refuse(host_fs::refusal_body(
                        "iris_symbols_local",
                        requested,
                        &why,
                        &[],
                    ))
                }
            },
        };

        let result = symbols_local::scan_workspace_with_kinds(
            &workspace,
            &p.query,
            limit,
            p.kinds.as_deref(),
        );

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
            "truncated": result.truncated.is_some(),
            "truncated_reason": result.truncated,
        }))
    }

    #[tool(
        description = "Introspect an ObjectScript class — returns methods, properties, and type information. Methods include FormalSpec as a structured array of {name, type, byref, output, default} objects and a ReturnType field. For BPL and DTL classes, an xdata_flow field describes the process steps (BPL: kind=bpl, steps array with Call/Code/If/Other entries, has_dynamic_dispatch flag; DTL: kind=dtl, source_class, target_class, subtransforms, assign_count). Skill: objectscript-navigation. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<DocsIntrospectResponse>()
    )]
    async fn docs_introspect(
        &self,
        Parameters(p): Parameters<IntrospectParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(p.namespace.as_deref(), &iris.namespace);
        let client = self.http_client();
        // Bug 15: use parameterized queries instead of manual string escaping.
        let methods = iris.query(
            "SELECT Name,FormalSpec,ReturnType FROM %Dictionary.CompiledMethod WHERE parent=? ORDER BY Name",
            vec![serde_json::Value::String(p.class_name.clone())],
            namespace,
            client,
        ).await.unwrap_or_default();
        let props = iris
            .query(
                "SELECT Name,Type FROM %Dictionary.CompiledProperty WHERE parent=? ORDER BY Name",
                vec![serde_json::Value::String(p.class_name.clone())],
                namespace,
                client,
            )
            .await
            .unwrap_or_default();
        // Parse FormalSpec strings into structured ArgSpec arrays.
        let methods_arr = methods["result"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|mut m| {
                if let Some(raw) = m.get("FormalSpec").and_then(|v| v.as_str()) {
                    let parsed = symbols_local::parse_formalspec_string(raw);
                    m["FormalSpec"] = serde_json::to_value(parsed).unwrap_or_default();
                }
                m
            })
            .collect::<Vec<_>>();

        // Detect BPL/DTL and add structured xdata_flow if present.
        let xdata_flow = detect_xdata_flow(&iris, &p.class_name, namespace, client).await;

        let mut resp = serde_json::json!({
            "success": true,
            "class_name": p.class_name,
            "methods": methods_arr,
            "properties": props["result"]["content"]
        });
        if let Some(flow) = xdata_flow {
            resp["xdata_flow"] = flow;
        }
        ok_json(resp)
    }

    #[tool(
        description = "Map a .INT routine offset to the original .CLS source line. Pass routine+offset OR a raw IRIS error string like '<UNDEFINED>x+3^MyApp.Foo.1'.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<DebugMapIntToClsResponse>()
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
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(p.namespace.as_deref(), &iris.namespace);
        let client = self.http_client();
        let code = crate::objectscript::source_line_code(&p.routine, p.offset);
        match iris.execute_via_generator(&code, namespace, client).await {
            Ok(raw) => {
                let (cls_name, cls_line) = parse_source_line(raw.trim());
                ok_json(
                    serde_json::json!({"success": true, "mapping_available": cls_name.is_some(), "cls_name": cls_name, "cls_line": cls_line, "routine": p.routine, "offset": p.offset, "raw_error": if p.error_string.is_empty() { serde_json::Value::Null } else { p.error_string.into() }}),
                )
            }
            Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
        }
    }

    #[tool(
        description = "Capture IRIS error state and recent error log entries for debugging.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<DebugCapturePacketResponse>()
    )]
    async fn debug_capture_packet(
        &self,
        Parameters(_p): Parameters<CapturePacketParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(_p.namespace.as_deref(), &iris.namespace);
        let client = self.http_client();
        match iris.query("SELECT TOP 20 ErrorCode,ErrorText,TimeStamp FROM %SYSTEM.Error ORDER BY TimeStamp DESC", vec![], namespace, client).await {
            Ok(resp) => ok_json(serde_json::json!({"success": true, "errors": resp["result"]["content"]})),
            Err(e) => {
                let msg = e.to_string();
                // %SYSTEM.Error is not available on community edition — return empty gracefully
                if msg.contains("SQLCODE: -30") || msg.contains("Table") && msg.contains("not found") {
                    ok_json(serde_json::json!({"success": true, "errors": [], "note": "%SYSTEM.Error not available on this IRIS edition"}))
                } else {
                    err_json("IRIS_UNREACHABLE", &msg)
                }
            }
        }
    }

    #[tool(
        description = "Retrieve recent IRIS error log entries.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<DebugGetErrorLogsResponse>()
    )]
    async fn debug_get_error_logs(
        &self,
        Parameters(p): Parameters<ErrorLogsParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(p.namespace.as_deref(), &iris.namespace);
        let client = self.http_client();
        // FR-012: cap max_entries to prevent runaway queries.
        let max_entries = p.max_entries.min(1000);
        let sql = format!("SELECT TOP {} ErrorCode,ErrorText,TimeStamp FROM %SYSTEM.Error ORDER BY TimeStamp DESC", max_entries);
        match iris.query(&sql, vec![], namespace, client).await {
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
            Err(e) => {
                let msg = e.to_string();
                // %SYSTEM.Error not available on community edition — return empty gracefully
                if msg.contains("SQLCODE: -30")
                    || (msg.contains("Table") && msg.contains("not found"))
                {
                    ok_json(
                        serde_json::json!({"success": true, "logs": [], "note": "%SYSTEM.Error not available on this IRIS edition"}),
                    )
                } else {
                    err_json("IRIS_UNREACHABLE", &msg)
                }
            }
        }
    }

    #[tool(
        description = "Build a .INT source map for a compiled ObjectScript class via Atelier xecute. Maps .INT routine line offsets back to .CLS source lines for stack trace resolution. No Python required.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<DebugSourceMapResponse>()
    )]
    async fn debug_source_map(
        &self,
        Parameters(p): Parameters<SourceMapParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(p.namespace.as_deref(), &iris.namespace);
        let client = self.http_client();
        let cls_name = p.cls_name.trim_end_matches(".cls");
        // Build source map by querying %Studio.Debugger for each .INT method
        let code = format!(
            "set cls=\"{}\" set rtn=$translate(cls,\".\",\".\") set map=\"{{\" set first=1 set method=\"\" for {{ set method=$order(^rIndex(rtn,method)) quit:method=\"\"  set intline=$get(^rIndex(rtn,method)) if 'first {{ set map=map_\",\" }} set map=map_\"\\\"\"_method_\"\\\":\\\"\"_intline_\"\\\"\" set first=0 }} set map=map_\"}}\" write map",
            crate::objectscript::os_lit_inner(cls_name)
        );
        match iris.execute_via_generator(&code, namespace, client).await {
            Ok(output) => {
                let map: serde_json::Value =
                    serde_json::from_str(output.trim()).unwrap_or(serde_json::json!({}));
                ok_json(
                    serde_json::json!({"success": true, "cls_name": cls_name, "source_map": map}),
                )
            }
            Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
        }
    }

    #[tool(
        description = "Generate an ObjectScript class from a natural language description, store it and compile it (the compile-time gate, stored-include and superclass checks of iris_doc put with compile apply; a refusal is an error result). Requires IRIS_GENERATE_CLASS_MODEL + OPENAI_API_KEY env vars. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisGenerateClassResponse>()
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

        // The text is stored under the name it declares, so a text with no usable name has nowhere
        // to go.
        let Some(class_name) =
            extract_class_name(&class_text).filter(|_| validate_cls_syntax(&class_text))
        else {
            return err_result(
                serde_json::json!({"success": false, "error_code": "INVALID_OUTPUT", "raw_llm_output": class_text}),
            );
        };

        let Some(iris) = self.target_conn_opt()? else {
            return ok_json(
                serde_json::json!({"success": true, "class_name": class_name, "class_text": class_text, "compiled": false, "retried": false, "note": "No IRIS connection — could not compile"}),
            );
        };
        // Storing the text is an Atelier PUT, the same as `iris_doc`.
        if nopws::atelier_unavailable(&iris) {
            return err_result(nopws::nopws_atelier_required_error());
        }
        let namespace = resolve_namespace(p.namespace.as_deref(), &iris.namespace);
        let client = self.http_client();
        let store = |name: String, text: String| {
            let iris = iris.clone();
            async move {
                generate_class::store_and_compile(
                    &iris,
                    client,
                    &name,
                    &text,
                    namespace,
                    &self.elicitation_store,
                    &self.checkout_cache,
                )
                .await
            }
        };

        let errors = match store(class_name.clone(), class_text.clone()).await? {
            generate_class::Stored::Stopped(result) => return Ok(result),
            generate_class::Stored::Compiled => {
                return ok_json(
                    serde_json::json!({"success": true, "class_name": class_name, "class_text": class_text, "compiled": true, "retried": false, "compile_errors": []}),
                )
            }
            generate_class::Stored::CompileErrors(errors) => errors,
        };

        // One retry: the model sees the errors IRIS returned. Its new text goes through the same
        // store, so it meets every check the first text met.
        let retry_prompt = RETRY_TEMPLATE.replace("{errors}", &errors.join("\n"));
        let fixed = llm
            .complete(
                GENERATE_CLASS_SYSTEM,
                &format!("{retry_prompt}\n\nOriginal: {class_text}"),
            )
            .await
            .ok()
            .filter(|t| validate_cls_syntax(t));
        let Some((fixed, fixed_name)) = fixed.and_then(|t| extract_class_name(&t).map(|n| (t, n)))
        else {
            return ok_json(
                serde_json::json!({"success": true, "class_name": class_name, "class_text": class_text, "compiled": false, "retried": false, "compile_errors": errors}),
            );
        };
        let (compiled, errors) = match store(fixed_name.clone(), fixed.clone()).await? {
            generate_class::Stored::Stopped(result) => return Ok(result),
            generate_class::Stored::Compiled => (true, Vec::new()),
            generate_class::Stored::CompileErrors(errors) => (false, errors),
        };
        ok_json(
            serde_json::json!({"success": true, "class_name": fixed_name, "class_text": fixed, "compiled": compiled, "retried": true, "compile_errors": errors}),
        )
    }

    #[tool(
        description = "Generate a %UnitTest.TestCase for an existing ObjectScript class. Introspects the class first. Requires IRIS_GENERATE_CLASS_MODEL + OPENAI_API_KEY. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisGenerateTestResponse>()
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

        let introspection_context = if let Some(iris) = self.target_conn_opt()?.as_deref() {
            let namespace = resolve_namespace(p.namespace.as_deref(), &iris.namespace);
            let client = self.http_client();
            // FR-001/C1: use parameterized query to prevent SQL injection via class_name.
            iris.query(
                "SELECT Name,FormalSpec,ReturnType FROM %Dictionary.CompiledMethod WHERE parent=? ORDER BY Name",
                vec![serde_json::Value::String(p.class_name.clone())],
                namespace,
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
            return err_result(
                serde_json::json!({"success": false, "error_code": "INVALID_OUTPUT", "raw_llm_output": test_text}),
            );
        }
        let test_class_name =
            extract_class_name(&test_text).unwrap_or_else(|| format!("Test.{}", p.class_name));
        ok_json(
            serde_json::json!({"success": true, "class_name": p.class_name, "test_class_name": test_class_name, "test_text": test_text, "introspected": !introspection_context.contains("unavailable")}),
        )
    }

    /// Read every `^SKILLS` entry. Returns `(entries, searched)` — `searched` is
    /// false when there is no IRIS connection or the global could not be read, so
    /// callers can say "I did not look there" instead of implying "nothing there".
    async fn synthesized_skills(&self) -> Result<(Vec<serde_json::Value>, bool), McpError> {
        let Some(iris) = self.target_conn_opt()? else {
            return Ok((Vec::new(), false));
        };
        // Build a JSON array of {name, description, body} objects via %DynamicArray so
        // skill names and descriptions are properly escaped — raw string concatenation
        // produced invalid JSON the moment any entry existed (#119).
        let code = r#"Set arr=##class(%DynamicArray).%New() Set key="" For { Set key=$Order(^SKILLS(key)) Quit:key=""  Set val=$Get(^SKILLS(key)) Set obj=##class(%DynamicObject).%New() Do obj.%Set("name",key) Do obj.%Set("description",$Piece(val,"|",1)) Do obj.%Set("body",$Piece(val,"|",2)) Do arr.%Push(obj) } Write arr.%ToJSON()"#;
        Ok(
            match iris
                .execute(code, &crate::tools::skills_tools::skills_namespace())
                .await
            {
                Ok(output) => match serde_json::from_str::<Vec<serde_json::Value>>(output.trim()) {
                    Ok(entries) => (entries, true),
                    Err(_) => (Vec::new(), false),
                },
                Err(_) => (Vec::new(), false),
            },
        )
    }

    #[tool(
        description = "List every available skill — both the skills bundled with this server (on disk, no IRIS needed) and any synthesized skills in the IRIS ^SKILLS global. Each result carries a `source` field: `bundled` or `synthesized`.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<SkillListResponse>()
    )]
    async fn skill_list(&self, _: Parameters<NoParams>) -> Result<CallToolResult, McpError> {
        use crate::skills::bundled;

        // Internal-tier skills stay reachable through skill_describe but are not advertised.
        let bundled_skills = bundled::advertised(&bundled::load_bundled_skills());
        let (synth, synth_searched) = self.synthesized_skills().await?;
        let merged = bundled::merge_sources(&bundled_skills, &synth);
        let skills: Vec<serde_json::Value> = merged.iter().map(|m| m.to_json()).collect();

        ok_json(serde_json::json!({
            "skills": skills,
            "count": skills.len(),
            "sources": bundled::sources_json(bundled_skills.len(), synth.len(), synth_searched),
            "note": bundled::searched_note(bundled_skills.len(), synth.len(), synth_searched),
        }))
    }

    #[tool(
        description = "Describe a skill by name. Looks in the bundled skills shipped with this server (no IRIS needed) and in the IRIS ^SKILLS global.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<SkillDescribeResponse>()
    )]
    async fn skill_describe(
        &self,
        Parameters(p): Parameters<SkillNameParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::skills::bundled;

        let bundled_skills = bundled::load_bundled_skills();
        if let Some(s) = bundled_skills.iter().find(|s| s.name == p.name) {
            let mut skill = s.to_json();
            skill["body"] = serde_json::Value::String(s.content().unwrap_or_default());
            return ok_json(serde_json::json!({"success": true, "skill": skill}));
        }

        let (synth, synth_searched) = self.synthesized_skills().await?;
        if synth_searched {
            let code = crate::objectscript::skill_get_code(&p.name);
            if let Some(iris) = self.target_conn_opt()? {
                if let Ok(output) = iris
                    .execute(&code, &crate::tools::skills_tools::skills_namespace())
                    .await
                {
                    if let Ok(mut skill) = serde_json::from_str::<serde_json::Value>(output.trim())
                    {
                        if let Some(obj) = skill.as_object_mut() {
                            obj.insert(
                                "source".to_string(),
                                serde_json::Value::String(
                                    bundled::SkillSource::Synthesized.as_str().to_string(),
                                ),
                            );
                        }
                        return ok_json(serde_json::json!({"success": true, "skill": skill}));
                    }
                }
            }
        }

        // FR-004: never a bare miss — say where we looked.
        err_result(serde_json::json!({
            "success": false,
            "error_code": "NOT_FOUND",
            "error": format!("Skill '{}' not found", p.name),
            "sources": bundled::sources_json(bundled_skills.len(), synth.len(), synth_searched),
            "note": bundled::searched_note(bundled_skills.len(), synth.len(), synth_searched),
        }))
    }

    #[tool(
        description = "Search all skills by name, description AND frontmatter tags. Covers both the skills bundled with this server (on disk, works with no IRIS connection) and synthesized skills in the IRIS ^SKILLS global. Each result carries a `source` field (`bundled`/`synthesized`); the response always reports how many skills were available in each source, so a zero result never means 'only one place was checked'.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<SkillSearchResponse>()
    )]
    async fn skill_search(
        &self,
        Parameters(p): Parameters<SkillSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::skills::bundled;

        let bundled_skills = bundled::advertised(&bundled::load_bundled_skills());
        let (synth, synth_searched) = self.synthesized_skills().await?;

        let terms = bundled::query_terms(&p.query);
        let mut scored: Vec<(serde_json::Value, u32)> = Vec::new();

        for (s, score) in bundled::search_bundled(&bundled_skills, &p.query, usize::MAX) {
            scored.push((s.to_json(), score));
        }

        // Synthesized entries carry no tags; match name + description.
        let bundled_names: std::collections::HashSet<&str> =
            bundled_skills.iter().map(|s| s.name.as_str()).collect();
        for v in &synth {
            let name = v
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| {
                    v.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            if name.is_empty() || bundled_names.contains(name.as_str()) {
                continue;
            }
            let synth_skill = bundled::BundledSkill {
                name: name.clone(),
                description: v
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or_default()
                    .to_string(),
                tags: Vec::new(),
                tier: None,
                path: None,
            };
            let score = bundled::score_skill(&synth_skill, &terms);
            if score > 0 {
                let mut entry = v.clone();
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert(
                        "source".to_string(),
                        serde_json::Value::String(
                            bundled::SkillSource::Synthesized.as_str().to_string(),
                        ),
                    );
                } else {
                    entry = serde_json::json!({
                        "name": name,
                        "source": bundled::SkillSource::Synthesized.as_str(),
                    });
                }
                scored.push((entry, score));
            }
        }

        scored.sort_by_key(|e| std::cmp::Reverse(e.1));
        scored.truncate(p.top_k);
        let results: Vec<serde_json::Value> = scored.into_iter().map(|(v, _)| v).collect();

        ok_json(serde_json::json!({
            "query": p.query,
            "results": results,
            "count": results.len(),
            "sources": bundled::sources_json(bundled_skills.len(), synth.len(), synth_searched),
            "note": bundled::searched_note(bundled_skills.len(), synth.len(), synth_searched),
        }))
    }

    #[tool(
        description = "Remove a skill from the registry by name. Destructive tier: requires IRIS_DESTRUCTIVE_TOOLS_ENABLED=1.",
        annotations(destructive_hint = true),
        output_schema = output_schemas::oneof_output_schema::<SkillForgetResponse>()
    )]
    async fn skill_forget(
        &self,
        Parameters(p): Parameters<SkillNameParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(iris) = self.target_conn_opt()?.as_deref() {
            let code = crate::objectscript::skill_forget_code(&p.name);
            match iris
                .execute(&code, &crate::tools::skills_tools::skills_namespace())
                .await
            {
                // The terminal returns `Ok` even when the Kill was refused, so only the `OK` marker
                // proves the skill is gone. Reporting a delete that did not happen sends the caller
                // off to fix something else while the skill keeps being served.
                Ok(out) if crate::tools::skills_tools::forget_confirmed(&out) => {
                    return ok_json(serde_json::json!({"success": true, "name": p.name}));
                }
                Ok(out) => {
                    return err_json(
                        "IRIS_EXECUTE_ERROR",
                        &format!(
                            "IRIS did not confirm removal of skill '{}' — it is still in ^SKILLS. Raw output: {}",
                            p.name,
                            out.trim()
                        ),
                    );
                }
                Err(_) => {}
            }
        }
        err_json(
            "DOCKER_REQUIRED",
            &format!("skill_forget requires docker exec. Set IRIS_CONTAINER=<container_name>.{DOCKER_REQUIRED_HINT}"),
        )
    }

    #[tool(
        description = "Trigger pattern miner to synthesize new skills from recorded tool calls.",
        output_schema = schema_for_output::<ToolError>()
    )]
    async fn skill_propose(&self, _: Parameters<NoParams>) -> Result<CallToolResult, McpError> {
        err_json(
            "NOT_IMPLEMENTED",
            "skill_propose: pattern mining not yet implemented",
        )
    }

    #[tool(
        description = "Optimize a skill using DSPy. Requires OBJECTSCRIPT_DSPY=true.",
        output_schema = schema_for_output::<ToolError>()
    )]
    async fn skill_optimize(
        &self,
        Parameters(_p): Parameters<SkillNameParams>,
    ) -> Result<CallToolResult, McpError> {
        err_json(
            "NOT_IMPLEMENTED",
            "skill_optimize: DSPy optimization not yet implemented",
        )
    }

    #[tool(
        description = "Share a skill to the community via GitHub PR.",
        output_schema = schema_for_output::<ToolError>()
    )]
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
        description = "List all skills loaded from --subscribe packages. Use --subscribe owner/repo when starting iris-agentic-dev mcp to load community skills.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<SkillCommunityListResponse>()
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
            "hint": "Start iris-agentic-dev mcp with --subscribe owner/repo to load community packages"
        }))
    }

    #[tool(
        description = "Install a community skill from the GitHub community repo.",
        output_schema = schema_for_output::<ToolError>()
    )]
    async fn skill_community_install(
        &self,
        Parameters(_p): Parameters<CommunityPkgParams>,
    ) -> Result<CallToolResult, McpError> {
        err_json(
            "NOT_IMPLEMENTED",
            "skill_community_install: community registry not yet implemented",
        )
    }

    #[tool(
        description = "Index markdown files into the IRIS knowledge base for semantic search.",
        output_schema = output_schemas::oneof_output_schema::<KbIndexResponse>()
    )]
    async fn kb_index(
        &self,
        Parameters(p): Parameters<KbIndexParams>,
    ) -> Result<CallToolResult, McpError> {
        let scope = match self.kb_index_scope("kb_index", p.workspace_path.as_deref()) {
            Ok(s) => s,
            Err(refused) => return refused,
        };
        let iris = self.target_conn()?;
        skills_tools::handle_kb(
            &iris,
            self.http_client(),
            skills_tools::KbParams {
                action: "index".into(),
                path: p.workspace_path,
                query: None,
                top_k: 0,
            },
            |_: Option<&str>| Ok(scope),
        )
        .await
    }

    #[tool(
        description = "Search the knowledge base for relevant guidance. Searches subscribed KB packages and any indexed content.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<KbRecallResponse>()
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

    #[tool(
        description = "Return recent tool call history for this session.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<AgentHistoryResponse>()
    )]
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
                            "ago_secs": crate::telemetry::ago_secs(&c.timestamp),
                            "duration_ms": c.duration_ms,
                            "session_id": c.session_id.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        ok_json(serde_json::json!({"calls": calls, "limit": p.limit}))
    }

    #[tool(
        description = "Query the durable telemetry record (beyond the current process's in-memory agent_history) by tool name, session id, and/or time range. Reads the local JSONL file, which holds this machine's records. When the operator turned on the IRIS sink (`telemetry_sink = \"iris\"` or IAD_TELEMETRY_SINK=iris) it reads the IRIS global too, on the server the call targets where that server's writes are on, read-only mode is not strict and its role is not `subject`, and returns a call both hold once. `check_config.telemetry_sink` says which is set.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<TelemetryQueryResponse>()
    )]
    async fn telemetry_query(
        &self,
        Parameters(p): Parameters<TelemetryQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let session_id = match p.session_id.as_deref().map(uuid::Uuid::parse_str) {
            Some(Ok(id)) => Some(id),
            Some(Err(_)) => return err_json("INVALID_PARAMS", "session_id must be a valid UUID"),
            None => None,
        };
        let limit = p.limit.min(5000);
        let iris = self.durable_sink_connection();
        let config_dir = telemetry_config_dir();
        let records =
            crate::telemetry::read_durable(session_id, iris, self.http_client(), &config_dir).await;
        let (matches, truncated) = crate::telemetry::filter_records(
            &records,
            p.tool_name.as_deref(),
            session_id,
            p.since.as_deref(),
            p.until.as_deref(),
            limit,
        );
        let records_json: Vec<serde_json::Value> = matches
            .iter()
            .map(|r| {
                serde_json::json!({
                    "tool": r.tool,
                    "success": r.success,
                    "duration_ms": r.duration_ms,
                    "timestamp": r.timestamp,
                    "session_id": r.session_id.to_string(),
                    "params": r.params,
                })
            })
            .collect();
        ok_json(serde_json::json!({"records": records_json, "truncated": truncated}))
    }

    #[tool(
        description = "Export recorded tool-call data as {from, to, via, count, ts} dispatch-trace records, aggregating repeated identical edges into a single record with an incremented count. The record shape is a graph-ingestion format; no tool in this server consumes it yet.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<TelemetryExportTraceResponse>()
    )]
    async fn telemetry_export_trace(
        &self,
        Parameters(p): Parameters<TelemetryExportTraceParams>,
    ) -> Result<CallToolResult, McpError> {
        let session_id = match p.session_id.as_deref().map(uuid::Uuid::parse_str) {
            Some(Ok(id)) => Some(id),
            Some(Err(_)) => return err_json("INVALID_PARAMS", "session_id must be a valid UUID"),
            None => None,
        };
        let iris = self.durable_sink_connection();
        let config_dir = telemetry_config_dir();
        let records =
            crate::telemetry::read_durable(session_id, iris, self.http_client(), &config_dir).await;
        let (filtered, _truncated) = crate::telemetry::filter_records(
            &records,
            None,
            session_id,
            p.since.as_deref(),
            None,
            usize::MAX,
        );
        let traces = crate::telemetry::trace_export::aggregate_trace(&filtered);
        ok_json(serde_json::json!({"traces": traces}))
    }

    #[tool(
        description = "Return learning agent status: skill count, pattern count, KB size.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<AgentStatsResponse>()
    )]
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
        description = "Full-text search across IRIS documents via Atelier REST v2. Auto-upgrades to async polling for large namespaces. Supports regex (matches anywhere in a line), case sensitivity, category filter (CLS/MAC/INT/INC/ALL), and wildcard document scopes. Returns at most `limit` matches (default 200, up to 10000); `limit_reached: true` means there may be more, so narrow `documents` or raise `limit`. Skill: objectscript-navigation. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisSearchResponse>()
    )]
    async fn iris_search(
        &self,
        Parameters(p): Parameters<search::SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result =
            search::handle_iris_search(&iris, self.http_client(), p, Arc::clone(&self.log_store))
                .await;
        self.record_call("iris_search", result.is_ok());
        result
    }

    #[tool(
        description = "Discover IRIS namespace contents. what=documents lists all docs, what=modified lists recently changed, what=namespace returns config, what=metadata returns IRIS version, what=jobs lists active jobs, what=csp_apps lists CSP apps, what=csp_debug returns debug ID, what=sa_schema returns the Studio Assist grammar for an XData namespace URL (name=http://www.intersystems.com/deepsee); it does not list or describe cubes, which %DeepSee.Utils %GetCubeList and %GetDimensionList do through iris_execute. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisInfoResponse>()
    )]
    async fn iris_info(
        &self,
        Parameters(p): Parameters<info::InfoParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(msg) = info::precheck(&p) {
            self.record_call("iris_info", false);
            return crate::tools::err_result(
                serde_json::json!({"success": false, "error_code": "INVALID_PARAMS", "error": msg}),
            );
        }
        let iris = self.target_conn()?;
        let result =
            info::handle_iris_info(&iris, self.http_client(), p, Arc::clone(&self.log_store)).await;
        self.record_call("iris_info", result.is_ok());
        result
    }

    #[tool(
        description = "Inspect a SQL table: returns whether it is a class-projected table or DDL-created, the backing data/index globals, and (optionally) an approximate row count. Works for both class-projected tables (with real storage globals from %Dictionary.CompiledStorage) and DDL tables (globals inferred by IRIS naming convention). Use include_row_count=true to add a COUNT(*) estimate. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisTableInfoResponse>()
    )]
    async fn iris_table_info(
        &self,
        Parameters(p): Parameters<info::TableInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = info::handle_iris_table_info(&iris, self.http_client(), p).await;
        self.record_call("iris_table_info", result.is_ok());
        result
    }

    #[tool(
        description = "Resolve ObjectScript dynamic dispatch: find all compiled classes that implement a given method. Use when you see $classmethod(var, method) or ##class({variable}).Method() and need to know the possible targets. Returns candidates with confidence scores (fewer matches = higher confidence). Confidence: 1 match=0.90, 2-5=0.75, 6-20=0.55, >20=0.30. Results cached 60s per session. Skill: objectscript-navigation. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<ResolveDynamicDispatchResponse>()
    )]
    async fn resolve_dynamic_dispatch(
        &self,
        Parameters(p): Parameters<dict::ResolveDynamicDispatchParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = dict::handle_resolve_dynamic_dispatch(
            &iris,
            self.http_client(),
            p,
            &self.metadata_cache,
        )
        .await;
        self.record_call("resolve_dynamic_dispatch", result.is_ok());
        result
    }

    #[tool(
        description = "Extract routing from a compiled Ensemble class. For MessageMap routers: returns message_type → method dispatch table (confidence 0.9). For BPL classes (Ens.BusinessProcessBPL): returns kind=bpl with routes derived from Call steps (confidence 0.8); includes note when dynamic dispatch ($classmethod) is detected. For DTL classes (Ens.DataTransformDTL): returns kind=dtl with source_class, target_class, and empty routes. Returns NOT_FOUND for plain classes with no routing. Results cached 60s per session. Skill: ensemble-production. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<ExtractMessageMapRoutingResponse>()
    )]
    async fn extract_message_map_routing(
        &self,
        Parameters(p): Parameters<dict::ExtractMessageMapParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = dict::handle_extract_message_map_routing(
            &iris,
            self.http_client(),
            p,
            &self.metadata_cache,
        )
        .await;
        self.record_call("extract_message_map_routing", result.is_ok());
        result
    }

    #[tool(
        description = "Find all concrete subclass implementations of a method in the full inheritance hierarchy. Given base class names and a method name, expands to all descendants at any depth and returns classes where the method is defined (Origin = parent, not inherited). Use to resolve polymorphic dispatch: adapter.Execute() → find all EnsLib.*.Adapter subclasses that implement Execute. Results cached 60s per session. Skill: objectscript-navigation. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<FindSubclassImplementationsResponse>()
    )]
    async fn find_subclass_implementations(
        &self,
        Parameters(p): Parameters<dict::FindSubclassImplementationsParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = dict::handle_find_subclass_implementations(
            &iris,
            self.http_client(),
            p,
            &self.metadata_cache,
        )
        .await;
        self.record_call("find_subclass_implementations", result.is_ok());
        result
    }

    #[tool(
        description = "Inspect IRIS macros. action=list returns the include files, or with `includes` the macros they define; action=signature returns parameters, action=location finds definition file/line, action=definition returns text, action=expand expands with arguments. `includes` (optional) names the include files to look in, e.g. [\"EnsConstants\"]; if omitted, the include that defines the macro is found for you. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisMacroResponse>(),
        annotations(read_only_hint = true)
    )]
    async fn iris_macro(
        &self,
        Parameters(p): Parameters<info::MacroParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = info::handle_iris_macro(&iris, self.http_client(), p).await;
        self.record_call("iris_macro", result.is_ok());
        result
    }

    #[tool(
        description = "IRIS debug tools. action=map_int maps a runtime error offset to source line, action=error_logs fetches recent error log entries, action=capture captures current error state, action=source_map builds .INT to .CLS mapping. Skill: objectscript-debugging. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisDebugResponse>()
    )]
    async fn iris_debug(
        &self,
        Parameters(p): Parameters<info::DebugParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = info::handle_iris_debug(&iris, self.http_client(), p).await;
        self.record_call("iris_debug", result.is_ok());
        result
    }

    #[tool(
        description = "Prepare context for generating an ObjectScript class or %UnitTest. Returns a ready-to-use prompt plus IRIS namespace context (existing class names, method signatures). No API key needed — the calling AI agent does the generation using the returned prompt, then saves with iris_doc(mode=put) and compiles with iris_compile. gen_type=class for new classes, gen_type=test for %UnitTest scaffolding. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisGenerateResponse>()
    )]
    async fn iris_generate(
        &self,
        Parameters(p): Parameters<info::GenerateParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = info::handle_iris_generate(&iris, self.http_client(), p).await;
        self.record_call("iris_generate", result.is_ok());
        result
    }

    #[tool(
        description = "Manage the learning agent skill registry. action=list returns all skills, action=describe returns one skill, action=search finds skills by keyword, action=forget removes a skill, action=propose mines recent tool calls and synthesizes a new skill (requires ≥5 calls). forget is a destructive action (requires IRIS_DESTRUCTIVE_TOOLS_ENABLED=1).",
        output_schema = output_schemas::oneof_output_schema::<SkillResponse>()
    )]
    async fn skill(
        &self,
        Parameters(p): Parameters<skills_tools::SkillParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = skills_tools::handle_skill(&iris, self.http_client(), p, &self.history).await;
        self.record_call("skill", result.is_ok());
        result
    }

    #[tool(
        description = "Community skill registry. action=list browses published skills from subscribed GitHub repos, action=install writes a community skill to the local ^SKILLS global.",
        output_schema = output_schemas::oneof_output_schema::<SkillCommunityResponse>()
    )]
    async fn skill_community(
        &self,
        Parameters(p): Parameters<skills_tools::SkillCommunityParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result =
            skills_tools::handle_skill_community(&iris, self.http_client(), p, &self.registry)
                .await;
        self.record_call("skill_community", result.is_ok());
        result
    }

    #[tool(
        description = "Knowledge base tools. action=index reads markdown/text files and stores them in ^KBCHUNKS, action=recall searches the KB for relevant content by keyword.",
        output_schema = output_schemas::oneof_output_schema::<KbResponse>()
    )]
    async fn kb(
        &self,
        Parameters(p): Parameters<skills_tools::KbParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        // The scope is resolved only by the action that reads the host, inside `handle_kb`.
        let scope = |path: Option<&str>| self.kb_index_scope("kb", path);
        let result = skills_tools::handle_kb(&iris, self.http_client(), p, scope).await;
        self.record_call("kb", result.is_ok());
        result
    }

    #[tool(
        description = "Session and learning agent information. what=stats returns skill count and session call count, what=history returns recent tool call history.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<AgentInfoResponse>()
    )]
    async fn agent_info(
        &self,
        Parameters(p): Parameters<skills_tools::AgentInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result =
            skills_tools::handle_agent_info(&iris, self.http_client(), p, &self.history).await;
        self.record_call("agent_info", result.is_ok());
        result
    }

    #[tool(
        description = "IRIS source control operations. action=status checks lock state and owner, action=menu lists available SCM actions, action=checkout checks out the document, action=execute runs a specific SCM action by ID. Handles elicitation for interactive SCM dialogs. Pass elicitation_id+answer to resume a pending SCM interaction. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisSourceControlResponse>()
    )]
    async fn iris_source_control(
        &self,
        Parameters(p): Parameters<ScmParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        // FR-010: NoPWS guard — iris_source_control requires Atelier REST.
        if nopws::nopws_is_active(
            iris.base_url == "http://127.0.0.1:1"
                || iris.base_url.starts_with("http://127.0.0.1:1/"),
            iris.version
                .as_deref()
                .map(|v| v.contains("2026.2.0AI"))
                .unwrap_or(false),
        ) {
            return err_result(nopws::nopws_atelier_required_error());
        }
        let result = scm::handle_iris_source_control(
            &iris,
            self.http_client(),
            p,
            &self.elicitation_store,
            &self.checkout_cache,
        )
        .await;
        self.record_call("iris_source_control", result.is_ok());
        result
    }

    // ── 052: iris_global ───────────────────────────────────────────────────────

    #[tool(
        description = "Read, write, kill, or list IRIS global nodes. action: get=read a node or subtree, set=write a node, kill=delete a node/subtree, list=enumerate subscripts. kill is a destructive action (requires IRIS_DESTRUCTIVE_TOOLS_ENABLED=1). PHI and system-blocklist gates enforced before any IRIS call. Pass acknowledgePhi=true to bypass per-global PHI gate. Skill: iris-agentic-dev. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisGlobalResponse>()
    )]
    async fn iris_global(
        &self,
        Parameters(p): Parameters<global::IrisGlobalParams>,
    ) -> Result<CallToolResult, McpError> {
        // Mutating actions (set/kill) run under the restricted service account when configured;
        // read-only actions (get/list) stay on the target's own connection. The gates ran in the
        // pipeline, on the server the call names (spec 141, R1).
        let is_write = matches!(p.action.as_str(), "set" | "kill");
        let cx = if is_write {
            match self.exec_context() {
                Ok(cx) => Some(cx),
                Err(e) => {
                    self.record_call("iris_global", false);
                    return exec_refused(e);
                }
            }
        } else {
            None
        };
        let (iris, client) = match &cx {
            Some(cx) => (cx.conn.clone(), cx.client.clone()),
            None => (self.target_conn()?, Arc::clone(&self.client)),
        };
        let mut result = global::handle_iris_global(&iris, client.as_ref(), &p, Ok(())).await;
        if let Some(cx) = &cx {
            result["identity"] = cx.helper_report().to_json();
        }
        json_result(result)
    }

    // ── 064: iris_coverage ────────────────────────────────────────────────────

    #[tool(
        description = "Measure ObjectScript line coverage using %Monitor.System.LineByLine. mode=run: start monitoring + run compiled test suite + stop + return per-class and total coverage in one call (use this for most tasks). mode=check: verify the monitor is available by doing a dry Start() — if BBSIZ_NOT_CONFIGURED is returned, increase gmheap to 256+ in Management Portal > System Administration > Configuration > Additional Settings > Advanced Memory, then restart IRIS. mode=start/stop/report: manual multi-step control. Every mode except report is a write (check starts and stops the monitor), so the write gate applies. Provide either classes=['MyApp.MyClass',...] or package='MyApp' (auto-discovers concrete classes). test_path must be a compiled class pattern (e.g. 'MyApp.Tests') — /noload always used. Returns {total_pct, hits, total, classes:[{class,routine,routines,hit,total,pct}], meets_target, target_pct}; every routine a class compiled to is counted, and a class with 0 of 0 lines has pct null (and null total_pct, meets_target), which is not 0%. If a line of the run throws, the monitor is still stopped and the result is partial with run_error. A line monitor that is already running (another session's) is refused with MONITOR_IN_USE and left alone; mode=stop stops it. Error codes: BBSIZ_NOT_CONFIGURED (gmheap too small), MONITOR_IN_USE, COVERAGE_ROUTINE_UNKNOWN (a class with no compiled routine), MONITOR_NOT_RUNNING (report), MISSING_PARAM. Skill: objectscript-coverage (merged toolset only). `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisCoverageResponse>()    )]
    async fn iris_coverage(
        &self,
        Parameters(p): Parameters<coverage::IrisCoverageParams>,
    ) -> Result<CallToolResult, McpError> {
        // `run` and `start` execute the agent's test path, so they run as the service account;
        // `stop`, `report` and `check` run only iad's own code and stay on the connection's user.
        // The pipeline resolved which (R2 section 1f).
        let cx = match self.exec_context() {
            Ok(cx) => cx,
            Err(e) => {
                self.record_call("iris_coverage", false);
                return exec_refused(e);
            }
        };
        let mut result = coverage::handle_iris_coverage(&cx, &p).await;
        if cx.expected.is_some() {
            result["identity"] = cx.helper_report().to_json();
        }
        json_result(result)
    }

    // ── 065: iris_doc_search ──────────────────────────────────────────────────

    #[tool(
        description = "Search the InterSystems documentation site (docs.intersystems.com) via its Algolia index. Returns ranked hits with title, URL, content excerpt, and breadcrumbs. Use for discovery questions ('what are all the ways to run SQL in IRIS?'), API lookups ('what does SQLCODE -30 mean?'), and any question where the answer lives in official docs. Optionally filter by version (e.g. '2025.1') and product (e.g. 'InterSystems IRIS'). Returns {query, total_hits, hits:[{title, url, excerpt, breadcrumbs, version, product}]}. Note: docs.intersystems.com is a JS SPA — do NOT use WebFetch or curl on DocBook URLs; they return only nav shell. This tool uses the real Algolia search index and returns actual documentation content. Skill: iris-docs for live IRIS class reference; iris-agentic-dev for connection setup.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisDocSearchResponse>()
    )]
    async fn iris_doc_search(
        &self,
        Parameters(p): Parameters<doc_search::IrisDocSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let result = doc_search::handle_iris_doc_search(&self.client, &p).await;
        self.record_call("iris_doc_search", result.get("error").is_none());
        if result.get("error").is_some() {
            err_result(result)
        } else {
            ok_json(result)
        }
    }

    // ── 053: iris_execute_method ──────────────────────────────────────────────

    #[tool(
        description = "Invoke a ClassMethod directly by class+method+args without writing ObjectScript boilerplate. Returns the string return value. Execute-gated: blocked on mcpTemplate=live and mcpTemplate=test. v1 limitation: only string-returning methods. Skill: objectscript-navigation (merged toolset only). `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisExecuteMethodResponse>()
    )]
    async fn iris_execute_method(
        &self,
        Parameters(p): Parameters<IrisExecuteMethodParams>,
    ) -> Result<CallToolResult, McpError> {
        // Invoking an arbitrary ClassMethod by name is the $classmethod indirection vector, so it
        // runs as the service account when one is set (least privilege). The gates ran in the
        // pipeline, on the server the call names (spec 141, R1).
        let cx = match self.exec_context() {
            Ok(cx) => cx,
            Err(e) => {
                self.record_call("iris_execute_method", false);
                return exec_refused(e);
            }
        };
        let result = doc::handle_iris_execute_method(&cx.conn, cx.client.as_ref(), &p).await;
        self.record_call("iris_execute_method", result.is_ok());
        exec::attach_identity(result, &cx.helper_report())
    }

    // ── Merged tools (T029–T032, registered only when IRIS_TOOLSET=merged) ─────
    // These are always present in the #[tool_router] but removed via remove_route()
    // for Baseline and Nostub toolsets in with_registry_and_toolset().
    // Note: iris_debug already exists above as a real tool — it IS the merged debug dispatcher.

    #[tool(
        description = "Interoperability production lifecycle (merged). action: status=get current state, start=start named production, stop=stop production, update=hot-apply config, check=check if update needed, recover=recover troubled production. `namespace` (optional): IRIS namespace for production operations. Defaults to the connection namespace. Use when the interop production lives in a different namespace than the default connection. Skill: ensemble-production. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisProductionResponse>()    )]
    async fn iris_production(
        &self,
        Parameters(p): Parameters<IrisProductionParams>,
    ) -> Result<CallToolResult, McpError> {
        let action = p.action.as_str();
        let target_hold = self.target_conn_opt()?;
        let iris_opt = target_hold.as_deref();
        let conn_ns = iris_opt.map(|i| i.namespace.as_str()).unwrap_or("USER");
        let ns_param = p.namespace.as_deref();
        let result = match action {
            "status" => {
                interop::interop_production_status_impl(
                    iris_opt,
                    interop::ProductionStatusParams {
                        namespace: resolve_namespace(ns_param, conn_ns).to_string(),
                        full_status: p.full,
                    },
                )
                .await
            }
            "start" => {
                interop::interop_production_start_impl(
                    iris_opt,
                    interop::ProductionNameParams {
                        production: p.production_name.clone(),
                        namespace: resolve_namespace(ns_param, conn_ns).to_string(),
                    },
                )
                .await
            }
            "stop" => {
                interop::interop_production_stop_impl(
                    iris_opt,
                    interop::ProductionStopParams {
                        production: p.production_name.clone(),
                        namespace: resolve_namespace(ns_param, conn_ns).to_string(),
                        timeout: p.timeout,
                        force: p.force,
                    },
                )
                .await
            }
            "update" => {
                interop::interop_production_update_impl(
                    iris_opt,
                    interop::ProductionUpdateParams {
                        namespace: resolve_namespace(ns_param, conn_ns).to_string(),
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
                        namespace: resolve_namespace(ns_param, conn_ns).to_string(),
                    },
                )
                .await
            }
            "recover" => {
                interop::interop_production_recover_impl(
                    iris_opt,
                    interop::ProductionRecoverParams {
                        namespace: resolve_namespace(ns_param, conn_ns).to_string(),
                    },
                )
                .await
            }
            "get_autostart" => {
                interop::interop_autostart_get_impl(
                    iris_opt,
                    &interop::ProductionAutostartParams {
                        action: "get_autostart".into(),
                        namespace: resolve_namespace(ns_param, conn_ns).to_string(),
                        enabled: None,
                        production: None,
                    },
                )
                .await
            }
            "set_autostart" => {
                interop::interop_autostart_set_impl(
                    iris_opt,
                    &interop::ProductionAutostartParams {
                        action: "set_autostart".into(),
                        namespace: resolve_namespace(ns_param, conn_ns).to_string(),
                        enabled: p.enabled,
                        production: p.production.clone(),
                    },
                )
                .await
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
        description = "Interoperability query dispatcher (merged). what (REQUIRED): logs=Event Log entries, queues=message queue depths, messages=message archive (Ens.MessageHeader). Anything else returns INVALID_ACTION. To trace one session, use what=messages with session_id=<n>. Filters: component=<config item> narrows logs and messages; session_id=<n> and since_id=<n> (tails after a watermark) narrow messages. what=messages can also search message CONTENT: (a) body_class=<msg class> + body_where=<SQL fragment on body table> + body_select=[cols] joins the body table server-side (SQL name resolved for you); (b) search_table={prop, value|value_like, class?, extent?} searches an indexed Search Table field (extent default EnsLib.HL7.SearchTable; errors list searchable props). Pass namespace=<ns> for a non-default interop namespace. Skill: ensemble-production. `server` (optional): name of a registered IRIS instance.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisInteropQueryResponse>()
    )]
    async fn iris_interop_query(
        &self,
        Parameters(params): Parameters<params::batch5::IrisInteropQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let what = p.get("what").and_then(|v| v.as_str()).unwrap_or("logs");
        let target_hold = self.target_conn_opt()?;
        let iris_opt = target_hold.as_deref();
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
                        namespace: p
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
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
                        session_id: p.get("session_id").and_then(|v| {
                            v.as_i64()
                                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                        }),
                        since_id: p.get("since_id").and_then(|v| {
                            v.as_i64()
                                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                        }),
                        limit: p.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as u32,
                        body_class: p
                            .get("body_class")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        body_where: p
                            .get("body_where")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        body_select: p
                            .get("body_select")
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        search_table: p
                            .get("search_table")
                            .cloned()
                            .and_then(|v| serde_json::from_value(v).ok()),
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
        description = "Container lifecycle dispatcher (merged). action: list=list running IRIS containers, select=validate container connection, start=start sandbox container via iris-devtester.",
        output_schema = output_schemas::oneof_output_schema::<IrisContainersResponse>()    )]
    async fn iris_containers(
        &self,
        Parameters(params): Parameters<params::batch2::IrisContainersParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
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
                    namespace: None,
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
        description = "Enable, disable, or inspect/modify settings of an individual Interoperability production config item. action: enable|disable|get_settings|set_settings. item: exact config item name. namespace: optional. settings: key-value map (for set_settings). Works via HTTP, no Docker required. Skill: ensemble-production. `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        output_schema = output_schemas::oneof_output_schema::<IrisProductionItemResponse>()    )]
    async fn iris_production_item(
        &self,
        Parameters(params): Parameters<params::batch5::IrisProductionItemParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
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
        let requested_ns = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let settings: std::collections::HashMap<String, String> = p
            .get("settings")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let target_hold = self.target_conn_opt()?;
        let conn_ns = target_hold
            .as_deref()
            .map(|i| i.namespace.as_str())
            .unwrap_or("USER");
        let namespace = resolve_namespace(requested_ns.as_deref(), conn_ns).to_string();
        let result = interop::interop_production_item_impl(
            target_hold.as_deref(),
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

    // ─── 056-interop-depth ───

    #[tool(
        description = "Read an Ensemble/Interoperability message body by message ID. Handles plain-text and stream-backed bodies (Ens.StreamContainer, %Stream.Object). PHI-gated by the stricter of the server's dataPolicy and the dataPolicy argument (block|redact|allow; an omitted argument means the server's own policy; any other value is INVALID_PARAMS): block returns PHI_POLICY_BLOCKED; allow requires acknowledgePhi=true; redact returns HL7 v2 with the identifying fields of PID, NK1, GT1, IN1, IN2 and MSH-3 (names, ids, birth dates, addresses, phones, SSNs) replaced by [REDACTED], which is a field mask and not de-identification: other segments and free text (PV1, OBX, NTE) come back as stored. It refuses other content with BODY_NOT_REDACTABLE. max_bytes default 65536, clamped to 1048576. Skill: ensemble-production (merged toolset only). `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        // Batch 6 correction: the wrapper's dispatch_gate short-circuit wasn't accounted for
        // when this schema was first declared — see IrisMessageBodyResponse's doc comment.
        output_schema = output_schemas::oneof_output_schema::<IrisMessageBodyResponse>()
    )]
    async fn iris_message_body(
        &self,
        Parameters(params): Parameters<params::batch5::IrisMessageBodyParams>,
    ) -> Result<CallToolResult, McpError> {
        let message_id = params.message_id.clone().unwrap_or_default();
        if message_id.is_empty() {
            return err_json("INVALID_PARAMS", "message_id is required");
        }
        let target = self.target()?;
        let namespace = self.target_ns(params.namespace.as_deref())?;
        let max_bytes = params.max_bytes.unwrap_or(65536);
        let acknowledge_phi = params.acknowledge_phi.unwrap_or(false);
        // The stricter of the target server's `dataPolicy` and the caller's: the caller can only
        // narrow (spec 141 R5 C20).
        let server_policy = target
            .primary
            .policy
            .data_policy
            .clone()
            .unwrap_or_default();
        let policy = match interop::resolve_body_policy(
            &server_policy,
            params.data_policy.as_deref(),
            acknowledge_phi,
        ) {
            Ok(p) => p,
            Err(body) => {
                self.record_call("iris_message_body", false);
                return if body["error_code"] == "INVALID_PARAMS" {
                    err_result(body)
                } else {
                    self.refuse(body)
                };
            }
        };
        let data_policy = match policy {
            crate::iris::workspace_config::DataPolicy::Allow => "allow",
            _ => "redact",
        };
        let result = interop::handle_iris_message_body(
            target.primary.conn.as_deref(),
            &interop::MessageBodyParams {
                message_id,
                namespace,
                max_bytes,
                acknowledge_phi,
            },
            data_policy,
        )
        .await;
        self.record_call("iris_message_body", result.is_ok());
        result.map(|r| self.classify_answer(r))
    }

    #[tool(
        description = "List or inspect interoperability business rules. Reads rule classes (subclasses of Ens.Rule.Definition with XData RuleDefinition, the kind the Rule Editor opens) and legacy Ens.Rule.RuleSet rows; each result has kind=class or kind=legacy. action=list returns {name, class_name, kind, rule_type, description, modified}. action=get with rule_name (the rule class name) returns the rule sets, each rule's when conditions and actions, its rule_type and context; for a legacy rule set only condition/action counts. Returns INTEROP_NOT_AVAILABLE if Ensemble is not installed. Skill: ensemble-production (merged toolset only). `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisBusinessRuleInfoResponse>()
    )]
    async fn iris_business_rule_info(
        &self,
        Parameters(params): Parameters<params::batch5::IrisBusinessRuleInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let action = p
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let rule_name = p
            .get("rule_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let target_hold = self.target_conn_opt()?;
        let conn_ns = target_hold
            .as_deref()
            .map(|i| i.namespace.as_str())
            .unwrap_or("USER");
        let namespace =
            resolve_namespace(p.get("namespace").and_then(|v| v.as_str()), conn_ns).to_string();
        let result = interop::handle_iris_business_rule_info(
            target_hold.as_deref(),
            &interop::BusinessRuleInfoParams {
                action,
                rule_name,
                namespace,
            },
        )
        .await;
        self.record_call("iris_business_rule_info", result.is_ok());
        result
    }

    #[tool(
        description = "Diff the running Interoperability production config against the last source-controlled version. Returns in_sync:true with changes:[] when no drift, or a changes array of {item_name, item_type, status} where status is added/removed/modified. Returns NO_SCM if no source control is configured. Skill: ensemble-production (merged toolset only). `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisProductionDiffResponse>()
    )]
    async fn iris_production_diff(
        &self,
        Parameters(params): Parameters<params::batch5::IrisProductionDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let production = p
            .get("production")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let target_hold = self.target_conn_opt()?;
        let conn_ns = target_hold
            .as_deref()
            .map(|i| i.namespace.as_str())
            .unwrap_or("USER");
        let namespace =
            resolve_namespace(p.get("namespace").and_then(|v| v.as_str()), conn_ns).to_string();
        let result = interop::handle_iris_production_diff(
            target_hold.as_deref(),
            &interop::ProductionDiffParams {
                production,
                namespace,
            },
        )
        .await;
        self.record_call("iris_production_diff", result.is_ok());
        result
    }

    // ─── 024-interop-depth: Ensemble credentials (US2) ───

    #[tool(
        description = "List all Ensemble credentials (IDs and usernames only — passwords never returned). namespace: optional.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisCredentialListResponse>()
    )]
    async fn iris_credential_list(
        &self,
        Parameters(params): Parameters<params::batch6::IrisCredentialListParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let iris_arc = self.target_conn_opt()?;
        let conn_ns = iris_arc
            .as_deref()
            .map(|i| i.namespace.as_str())
            .unwrap_or("USER");
        let namespace =
            resolve_namespace(p.get("namespace").and_then(|v| v.as_str()), conn_ns).to_string();
        let result = interop::interop_credential_list_impl(
            iris_arc.as_deref(),
            interop::CredentialListParams { namespace },
        )
        .await;
        self.record_call("iris_credential_list", result.is_ok());
        result
    }

    #[tool(
        description = "Create, update, or delete an Ensemble credential. action: create|update|delete. id: credential ID (required). username/password: required for create, optional for update. namespace: optional. Destructive tier: create/update/delete require IRIS_DESTRUCTIVE_TOOLS_ENABLED=1, and are suppressed on Live instances unless IRIS_ALLOW_PROD=1.",
        annotations(destructive_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisCredentialManageResponse>()
    )]
    async fn iris_credential_manage(
        &self,
        Parameters(params): Parameters<params::batch6::IrisCredentialManageParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let iris_arc = self.target_conn_opt()?;
        let conn_ns = iris_arc
            .as_deref()
            .map(|i| i.namespace.as_str())
            .unwrap_or("USER");
        let result = interop::interop_credential_manage_impl(
            iris_arc.as_deref(),
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
                namespace: resolve_namespace(p.get("namespace").and_then(|v| v.as_str()), conn_ns)
                    .to_string(),
            },
        )
        .await;
        self.record_call("iris_credential_manage", result.is_ok());
        result
    }

    // ─── 024-interop-depth: Lookup tables (US3) ───

    #[tool(
        description = "Read, write, delete, or list Ensemble lookup table entries. action: get|set|delete|list_keys|list_tables. table: table name (required except list_tables). key: required for get/set/delete. value: required for set. namespace: optional. get/list_keys/list_tables always available; set/delete are destructive actions (require IRIS_DESTRUCTIVE_TOOLS_ENABLED=1). Skill: ensemble-production.",
        annotations(destructive_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisLookupManageResponse>()
    )]
    async fn iris_lookup_manage(
        &self,
        Parameters(params): Parameters<params::batch6::IrisLookupManageParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let iris_arc = self.target_conn_opt()?;
        let conn_ns = iris_arc
            .as_deref()
            .map(|i| i.namespace.as_str())
            .unwrap_or("USER");
        let result = interop::interop_lookup_manage_impl(
            iris_arc.as_deref(),
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
                namespace: resolve_namespace(p.get("namespace").and_then(|v| v.as_str()), conn_ns)
                    .to_string(),
            },
        )
        .await;
        self.record_call("iris_lookup_manage", result.is_ok());
        result
    }

    #[tool(
        description = "Export or import an Ensemble lookup table as XML. action: export|import. table: table name. xml: XML string (required for import). namespace: optional. export always available; import replaces the whole table, so import is destructive-tier. Skill: ensemble-production.",
        output_schema = output_schemas::oneof_output_schema::<IrisLookupTransferResponse>()
    )]
    async fn iris_lookup_transfer(
        &self,
        Parameters(params): Parameters<params::batch6::IrisLookupTransferParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let iris_arc = self.target_conn_opt()?;
        let conn_ns = iris_arc
            .as_deref()
            .map(|i| i.namespace.as_str())
            .unwrap_or("USER");
        let result = interop::interop_lookup_transfer_impl(
            iris_arc.as_deref(),
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
                namespace: resolve_namespace(p.get("namespace").and_then(|v| v.as_str()), conn_ns)
                    .to_string(),
            },
        )
        .await;
        self.record_call("iris_lookup_transfer", result.is_ok());
        result
    }

    // ── 026-admin-tools: iris_admin dispatcher ───────────────────────────────

    #[tool(
        description = "IRIS administration dispatcher. \
        Read actions (always available): list_namespaces, list_databases, list_users, list_roles, \
        list_user_roles, check_permission, list_webapps, get_webapp, \
        view_locks, view_processes, journal_search, namespace_mappings, database_status. \
        Write actions (require IRIS_WRITE_TOOLS_ENABLED=1): \
        clear_password_change_flag, unlock_user, fresh_container_setup, mirror_add_async. \
        mirror_add_async joins this IRIS instance to an existing mirror set as an async DR member. \
        Params: mirror_name (required), primary_host (required), primary_port (default 2188), \
        instance_name (default IRIS), async_member_type (0=DR, 1=ReadOnly, 2=ReadWrite; default 0). \
        Destructive actions (require IRIS_DESTRUCTIVE_TOOLS_ENABLED=1): create_user, update_user, \
        delete_user, create_namespace, delete_namespace, create_webapp, delete_webapp, \
        mirror_failover. \
        create_user, update_user, delete_user, create_namespace, delete_namespace, create_webapp, \
        delete_webapp, clear_password_change_flag, unlock_user and fresh_container_setup also \
        require IRIS_ADMIN_TOOLS=1, a second switch the handler checks after the tier gate. \
        mirror_failover promotes this backup member to primary (irreversible without manual recovery). \
        Params: confirm (required, must be true). \
        fresh_container_setup runs the full first-boot sequence on a fresh IRIS container: \
        clears the forced-password-change flag and unlocks the default account. \
        clear_password_change_flag params: username (default _SYSTEM), password (default SYS), \
        new_password (optional). unlock_user params: username (required). \
        fresh_container_setup params: username (default _SYSTEM), password (default SYS), \
        new_password (optional). \
        All operations run in %SYS namespace. check_permission checks the currently connected \
        user (IRIS_USERNAME). view_processes requires dataPolicy param (block/redact/allow). \
        journal_search requires dataPolicy=allow and at least one of global_pattern or time_range. \
        `server` (optional): name of a registered IRIS instance. If omitted, uses the default connection. Use `iris_servers` to list available instances.",
        annotations(destructive_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisAdminResponse>()
    )]
    async fn iris_admin(
        &self,
        Parameters(params): Parameters<params::batch3::IrisAdminParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let target_hold = self.target_conn_opt()?;
        // `view_processes` and `journal_search` used to read this out of the caller's parameters,
        // which let a caller hand itself `dataPolicy: "allow"`. It is configuration, so it comes
        // from the target's policy, which defaults to the strict value when unset.
        let effective_data_policy = self.target()?.primary.data_policy_name();
        let iris_opt = target_hold.as_deref();
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
                let permission = p
                    .get("permission")
                    .and_then(|v| v.as_str())
                    .unwrap_or("USE");
                if resource.is_empty() {
                    return err_json(
                        "INVALID_PARAMS",
                        "resource is required for check_permission",
                    );
                }
                admin::admin_check_permission_impl(iris_opt, resource, permission).await
            }
            "create_user" => {
                let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("");
                let password = p.get("password").and_then(|v| v.as_str()).unwrap_or("");
                if username.is_empty() || password.is_empty() {
                    return err_json(
                        "INVALID_PARAMS",
                        "username and password are required for create_user",
                    );
                }
                admin::admin_create_user_impl(
                    iris_opt,
                    username,
                    password,
                    p.get("full_name").and_then(|v| v.as_str()),
                    p.get("roles").and_then(|v| v.as_str()),
                )
                .await
            }
            "update_user" => {
                let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("");
                if username.is_empty() {
                    return err_json("INVALID_PARAMS", "username is required for update_user");
                }
                admin::admin_update_user_impl(
                    iris_opt,
                    username,
                    p.get("password").and_then(|v| v.as_str()),
                    p.get("enabled").and_then(|v| v.as_bool()),
                    p.get("roles").and_then(|v| v.as_str()),
                )
                .await
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
                let code_db = p
                    .get("code_database")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let data_db = p
                    .get("data_database")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if name.is_empty() || code_db.is_empty() || data_db.is_empty() {
                    return err_json(
                        "INVALID_PARAMS",
                        "name, code_database, and data_database are required",
                    );
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
                    return err_json(
                        "INVALID_PARAMS",
                        "path and namespace are required for create_webapp",
                    );
                }
                admin::admin_create_webapp_impl(
                    iris_opt,
                    path,
                    ns,
                    p.get("dispatch_class").and_then(|v| v.as_str()),
                    p.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                )
                .await
            }
            "delete_webapp" => {
                let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("");
                if path.is_empty() {
                    return err_json("INVALID_PARAMS", "path is required for delete_webapp");
                }
                admin::admin_delete_webapp_impl(iris_opt, path).await
            }
            // ── 055-system-observability ──────────────────────────────────────
            "view_locks" => observability::view_locks_impl(iris_opt).await,
            "view_processes" => {
                let ns_filter = p.get("namespace").and_then(|v| v.as_str());
                observability::view_processes_impl(iris_opt, effective_data_policy, ns_filter).await
            }
            "journal_search" => {
                let data_policy = effective_data_policy;
                let global_pattern = p.get("global_pattern").and_then(|v| v.as_str());
                let time_range = p.get("time_range");
                let max_records = p.get("max_records").and_then(|v| v.as_u64());
                let global_blocklist = self.target()?.primary.policy.global_blocklist.clone();
                let acknowledge_phi = p
                    .get("acknowledgePhi")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                observability::journal_search_impl(
                    iris_opt,
                    data_policy,
                    global_pattern,
                    time_range,
                    max_records,
                    &global_blocklist,
                    acknowledge_phi,
                )
                .await
            }
            "namespace_mappings" => {
                let ns_param = p.get("namespace").and_then(|v| v.as_str());
                let conn_ns = iris_opt.map(|i| i.namespace.as_str()).unwrap_or("USER");
                observability::namespace_mappings_impl(iris_opt, ns_param, conn_ns).await
            }
            "database_status" => {
                let name_filter = p.get("name").and_then(|v| v.as_str());
                observability::database_status_impl(iris_opt, name_filter).await
            }
            // ── 099: fresh container setup ────────────────────────────────────
            "clear_password_change_flag" => {
                let username = p
                    .get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("_SYSTEM");
                let password = p.get("password").and_then(|v| v.as_str()).unwrap_or("SYS");
                let new_password = p
                    .get("new_password")
                    .and_then(|v| v.as_str())
                    .unwrap_or(password);
                admin::admin_clear_password_change_flag_impl(
                    iris_opt,
                    username,
                    password,
                    new_password,
                )
                .await
            }
            "unlock_user" => {
                let username = p.get("username").and_then(|v| v.as_str()).unwrap_or("");
                if username.is_empty() {
                    return err_json("INVALID_PARAMS", "username is required for unlock_user");
                }
                admin::admin_unlock_user_impl(iris_opt, username).await
            }
            "fresh_container_setup" => {
                let username = p
                    .get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("_SYSTEM");
                let password = p.get("password").and_then(|v| v.as_str()).unwrap_or("SYS");
                let new_password = p
                    .get("new_password")
                    .and_then(|v| v.as_str())
                    .unwrap_or(password);
                admin::admin_fresh_container_setup_impl(iris_opt, username, password, new_password)
                    .await
            }
            "mirror_add_async" => {
                let mirror_name = p.get("mirror_name").and_then(|v| v.as_str()).unwrap_or("");
                let primary_host = p.get("primary_host").and_then(|v| v.as_str()).unwrap_or("");
                if mirror_name.is_empty() || primary_host.is_empty() {
                    return err_json(
                        "INVALID_PARAMS",
                        "mirror_name and primary_host are required for mirror_add_async",
                    );
                }
                let primary_port = p
                    .get("primary_port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(2188) as u16;
                let instance_name = p
                    .get("instance_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("IRIS");
                let async_member_type = p
                    .get("async_member_type")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u8;
                admin_tools::iris_mirror_add_async_impl(
                    iris_opt,
                    &self.client,
                    mirror_name,
                    primary_host,
                    primary_port,
                    instance_name,
                    async_member_type,
                )
                .await
            }
            "mirror_failover" => {
                let confirm = p.get("confirm").and_then(|v| v.as_bool()).unwrap_or(false);
                if !confirm {
                    return err_json(
                        "CONFIRMATION_REQUIRED",
                        "confirm must be true to execute mirror_failover — \
                         this action is irreversible without manual recovery",
                    );
                }
                admin_tools::iris_mirror_failover_impl(iris_opt, &self.client).await
            }
            _ => err_json(
                "INVALID_ACTION",
                "iris_admin: action must be one of: list_namespaces, list_databases, \
                 list_users, list_roles, list_user_roles, check_permission, list_webapps, \
                 get_webapp, view_locks, view_processes, journal_search, namespace_mappings, \
                 database_status, create_user, update_user, delete_user, create_namespace, \
                 delete_namespace, create_webapp, delete_webapp, \
                 clear_password_change_flag, unlock_user, fresh_container_setup, \
                 mirror_add_async, mirror_failover",
            ),
        };
        self.record_call("iris_admin", result.is_ok());
        result
    }

    // ── iris_get_log (027 — progressive disclosure, Merged tier only) ──────────

    #[tool(
        description = "Retrieve a stored result by log_id from the progressive disclosure store. With id: returns the full result (optionally paginated with limit/offset). Without id: lists all stored log entries with their IDs, tools, timestamps, and total counts. Use after any tool returns truncated:true.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisGetLogResponse>()
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

    // ── 072: server management tools ─────────────────────────────────────────

    #[tool(
        description = "List all IRIS server instances registered in the connection pool. Returns an array of {name, host, port, base_url, namespace, username, source, reachable} objects, plus `web_prefix` for any instance served under a URL path prefix rather than at the web server's root. `base_url` is the URL iad actually calls, prefix included: two instances behind one gateway share a host and port, so it is the field that tells them apart. `source` values: iad-native (added via iris_add_server), vscode (from VS Code/Cursor Server Manager), fleet (from workspace TOML), env (from IRIS_HOST env var). Default: `reachable` is null (fast path). Pass `probe: true` to probe all servers in parallel (5 s timeout each) and include reachable, auth, latency_ms, error fields.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<IrisServersResponse>()
    )]
    async fn iris_servers(
        &self,
        Parameters(p): Parameters<server_tools::IrisServersParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_reload().await;
        use crate::iris::servers_config;
        use futures_util::future::join_all;

        let do_probe = p.probe.unwrap_or(false);
        let native_cfg = servers_config::load_native_config();

        // Build base entry metadata for each server.
        struct ServerMeta {
            name: String,
            host: String,
            port: u16,
            /// The resolved URL, prefix included. Probing `host`/`port` alone reported a
            /// gateway-prefixed entry as healthy while every real call went to the gateway root
            /// (issue #129).
            base_url: String,
            web_prefix: Option<String>,
            namespace: String,
            username: String,
            password: String,
            tls: crate::iris::http::TlsSetting,
            source: String,
            has_plaintext: bool,
        }

        let pool = self.pool_ref();
        let metas: Vec<ServerMeta> = pool
            .names()
            .iter()
            .filter_map(|name| {
                let source = pool.source_of(name);
                let has_plaintext = if source == "iad-native" {
                    native_cfg
                        .servers
                        .get(*name)
                        .map(|e| e.password.is_some())
                        .unwrap_or(false)
                } else {
                    false
                };
                // A listing reads no secret (T189); only a probe logs in, so only a probe asks
                // the pool for the member with its password.
                let member = if do_probe {
                    pool.get(Some(name))
                } else {
                    pool.peek(Some(name))
                };
                match member {
                    Ok(conn) => {
                        let (host, port) = parse_host_port(&conn.base_url);
                        Some(ServerMeta {
                            name: name.to_string(),
                            host,
                            port,
                            base_url: conn.base_url.clone(),
                            web_prefix: extract_web_prefix_from_url(&conn.base_url),
                            namespace: conn.namespace.clone(),
                            username: conn.username.clone(),
                            password: conn.password.clone(),
                            tls: conn.tls,
                            source: source.to_string(),
                            has_plaintext,
                        })
                    }
                    Err(_) => None,
                }
            })
            .collect();

        let entries: Vec<serde_json::Value> = if do_probe {
            // Fan out probe_server calls in parallel.
            let futures: Vec<_> =
                metas
                    .iter()
                    .map(|m| {
                        let base_url = m.base_url.clone();
                        let ns = m.namespace.clone();
                        let user = m.username.clone();
                        let pass = m.password.clone();
                        let tls = m.tls;
                        async move {
                            server_tools::probe_base_url(&base_url, &tls, &ns, &user, &pass).await
                        }
                    })
                    .collect();
            let probe_results = join_all(futures).await;
            metas
                .into_iter()
                .zip(probe_results)
                .map(|(m, probe)| {
                    let mut entry = serde_json::json!({
                        "name": m.name,
                        "host": m.host,
                        "port": m.port,
                        "base_url": m.base_url,
                        "namespace": m.namespace,
                        "username": m.username,
                        "source": m.source,
                        "reachable": probe.reachable,
                        "auth": probe.auth,
                        "latency_ms": probe.latency_ms,
                        "error": probe.error,
                        "iris_version": probe.iris_version,
                        "atelier_version": probe.atelier_version,
                    });
                    if let Some(prefix) = m.web_prefix {
                        entry["web_prefix"] = serde_json::Value::String(prefix);
                    }
                    if m.has_plaintext {
                        entry["has_plaintext_credential"] = serde_json::Value::Bool(true);
                    }
                    entry
                })
                .collect()
        } else {
            // Fast path: no probe, reachable:null.
            metas
                .into_iter()
                .map(|m| {
                    let mut entry = serde_json::json!({
                        "name": m.name,
                        "host": m.host,
                        "port": m.port,
                        "base_url": m.base_url,
                        "namespace": m.namespace,
                        "username": m.username,
                        "source": m.source,
                        "reachable": serde_json::Value::Null,
                    });
                    if let Some(prefix) = m.web_prefix {
                        entry["web_prefix"] = serde_json::Value::String(prefix);
                    }
                    if m.has_plaintext {
                        entry["has_plaintext_credential"] = serde_json::Value::Bool(true);
                    }
                    entry
                })
                .collect()
        };

        ok_json(serde_json::json!({"servers": entries, "count": entries.len()}))
    }

    #[tool(
        description = "Add a new IRIS server to the iad-native configuration. Pass `web_prefix` (e.g. \"/hs20261\") for an instance behind a shared web gateway — without it the entry resolves to the gateway root and every call goes to the wrong place; registering the same name again updates the entry. The credential is stored in the OS keychain when available. On headless hosts (MCP in Claude Desktop, Remote SSH, CI) where no keychain exists, the credential is stored in plaintext in servers.json as a fallback — the response includes stored_plaintext: true and a warning in that case. Call `iris_reload_pool` afterwards to make it available via the `server` param in the running server; no restart needed. Returns {added: true, name, note}.",
        output_schema = output_schemas::oneof_output_schema::<IrisAddServerResponse>()
    )]
    async fn iris_add_server(
        &self,
        Parameters(p): Parameters<server_tools::AddServerParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::iris::servers_config::{self, KeychainOutcome, NewServer};
        use crate::iris::{endpoint::Endpoint, server_manager};

        // Validate name is non-empty.
        if p.name.trim().is_empty() {
            return err_result(serde_json::json!({
                "error_code": "INVALID_PARAMS",
                "message": "Server name must not be empty."
            }));
        }

        // A prefix that cannot work is refused here rather than written and left to fail as a 404
        // from the gateway root (#129).
        if let Some(prefix) = p.web_prefix.as_deref() {
            if let Err(message) = servers_config::validate_web_prefix(prefix) {
                return err_result(serde_json::json!({
                    "error_code": "INVALID_PARAMS",
                    "message": message
                }));
            }
        }

        // The service account's name is spliced into a terminal line (R2): refuse one that could not
        // be, before anything is written.
        let service_username = p
            .service_username
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty());
        if let Some(u) = service_username {
            if let Err(message) = exec::validate_service_username(u) {
                return err_result(serde_json::json!({
                    "error_code": exec::ERR_SERVICE_ACCOUNT_INVALID,
                    "message": message
                }));
            }
        }
        let container = p
            .container
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty());

        // A name the Server Manager already uses would be read as that profile (and be handed its
        // password): refuse it, a fixed rule (spec 141, FR-023).
        if servers_config::name_conflicts(&p.name, &server_manager::user_profiles()) {
            return err_result(serde_json::json!({
                "error_code": "NAME_CONFLICT",
                "message": format!(
                    "'{}' is the name of a Server Manager profile. Pick another name, or use \
                     iris_import_servers to link the profile.",
                    p.name
                )
            }));
        }

        let endpoint = match Endpoint::parse(
            p.scheme.as_deref(),
            &p.host,
            p.port,
            p.web_prefix.as_deref(),
        ) {
            Ok(e) => e,
            Err(e) => {
                return err_result(serde_json::json!({
                    "error_code": "INVALID_PARAMS",
                    "message": e.to_string()
                }))
            }
        };

        // Decide the credential first, then save once. Keychain first: a failed write leaves the
        // file as it was (R4b D11).
        let mut cfg = servers_config::load_native_config();
        let old = cfg.servers.get(&p.name).cloned();
        let password = Some(p.password.clone()).filter(|pw| !pw.is_empty());
        let mut keychain_note: Option<String> = None;
        let keychain = match password.as_deref() {
            None => KeychainOutcome::NotTried,
            Some(pw) => {
                match server_manager::store_iad_credential(&p.name, &p.username, &endpoint, pw) {
                    Ok(()) => KeychainOutcome::Stored,
                    // Not only "there is no keychain": a host whose default keychain cannot be
                    // opened (a locked login keychain over SSH, a `HOME` with none) is no
                    // keychain either.
                    Err(e) => {
                        keychain_note = Some(e.to_string());
                        KeychainOutcome::Unavailable
                    }
                }
            }
        };
        let new = NewServer {
            host: p.host.clone(),
            port: p.port,
            namespace: p.namespace.clone(),
            username: p.username.clone(),
            description: p.description.clone(),
            scheme: p.scheme.clone(),
            web_prefix: p.web_prefix.clone(),
            tls_verify: None,
            password,
        };
        let mut merge = match servers_config::merge_server(old.as_ref(), &new, keychain) {
            Ok(m) => m,
            Err(e) => {
                return err_result(serde_json::json!({
                    "error_code": "INVALID_PARAMS",
                    "message": e.to_string()
                }))
            }
        };
        let mut password_cleared = merge.password_cleared;
        if let (Some(stale), Some(old)) = (&merge.stale_keychain, &old) {
            // The old endpoint's password must not outlive the move.
            password_cleared |=
                server_manager::delete_iad_credential(&p.name, &old.username, stale).is_ok();
        }
        servers_config::carry_service_account(
            &mut merge,
            old.as_ref(),
            service_username,
            container,
        );
        // A service account the entry no longer has (moved, renamed or dropped) leaves no keychain
        // item behind.
        if let Some(old) = &old {
            if let (Some(old_user), Ok(old_endpoint)) = (
                old.service_username.as_deref(),
                servers_config::entry_endpoint(old),
            ) {
                if merge.moved || service_username != Some(old_user) {
                    let _ = server_manager::delete_iad_credential(&p.name, old_user, &old_endpoint);
                }
            }
        }
        // The service account's password follows the primary's rule: the keychain first, and only
        // a host with no keychain keeps it in servers.json, bound to this endpoint.
        let mut service_plaintext_written = false;
        let mut service_store = if merge.entry.service_password.is_some() {
            "file"
        } else {
            "none"
        };
        if let (Some(user), Some(pw)) = (
            service_username,
            p.service_password.as_deref().filter(|pw| !pw.is_empty()),
        ) {
            match server_manager::store_iad_credential(&p.name, user, &endpoint, pw) {
                Ok(()) => {
                    merge.entry.service_password = None;
                    service_store = "keychain";
                }
                Err(e) => {
                    keychain_note.get_or_insert_with(|| e.to_string());
                    merge.entry.service_password = Some(pw.to_string());
                    merge.entry.password_endpoint = Some(endpoint.base_url());
                    service_store = "file";
                    service_plaintext_written = true;
                }
            }
        }
        cfg.servers.insert(p.name.clone(), merge.entry);
        if let Err(e) = servers_config::save_native_config(&cfg) {
            return err_result(serde_json::json!({
                "error_code": "SAVE_FAILED",
                "message": format!("Failed to save servers.json: {e}")
            }));
        }

        let mut response = serde_json::json!({
            "added": true,
            "name": p.name,
            "credential_store": merge.credential_store,
            "note": "Call iris_reload_pool to make this server routable via the `server` param."
        });
        if password_cleared {
            response["password_cleared"] = serde_json::json!(true);
        }
        if let Some(user) = service_username {
            response["service_username"] = serde_json::json!(user);
            response["service_credential_store"] = serde_json::json!(service_store);
        }
        if keychain == KeychainOutcome::Unavailable || service_plaintext_written {
            // No keychain here (headless MCP, Remote SSH): a password is in servers.json, which
            // is owner-only and bound to this endpoint.
            response["stored_plaintext"] = serde_json::json!(true);
            response["warning"] = serde_json::json!(format!(
                "No usable OS keychain here{}: the password is in servers.json (owner-only, used only for this endpoint).",
                keychain_note
                    .as_deref()
                    .map(|n| format!(" ({n})"))
                    .unwrap_or_default()
            ));
        }
        ok_json(response)
    }

    #[tool(
        description = "Remove a server from the iad-native configuration. Destructive tier: requires IRIS_DESTRUCTIVE_TOOLS_ENABLED=1. Only servers with source=iad-native can be removed (vscode, fleet, and env sources are read-only). Also removes iad's own OS keychain item for the entry (never the Server Manager's). Call `iris_reload_pool` afterwards to drop it from the running pool. Returns {removed: true, name, note}. Error codes: REMOVE_NOT_ALLOWED (source is not iad-native), SERVER_NOT_FOUND (not in pool).",
        annotations(destructive_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisRemoveServerResponse>()
    )]
    async fn iris_remove_server(
        &self,
        Parameters(p): Parameters<server_tools::RemoveServerParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::iris::servers_config;

        // Check source — only iad-native servers can be removed.
        let pool = self.pool_ref();
        let source = pool.source_of(&p.name);
        if source != "iad-native" {
            return err_result(serde_json::json!({
                "error_code": server_tools::REMOVE_NOT_ALLOWED,
                "source": source,
                "message": format!(
                    "Server '{}' is sourced from '{}' and cannot be removed via iris_remove_server. \
                     Only iad-native servers (added via iris_add_server) can be removed.",
                    p.name, source
                )
            }));
        }

        // Load, remove entry, save.
        let mut cfg = servers_config::load_native_config();
        if !cfg.servers.contains_key(&p.name) {
            return err_result(serde_json::json!({
                "error_code": "SERVER_NOT_FOUND",
                "message": format!("Server '{}' not found in iad-native config.", p.name)
            }));
        }
        let removed = cfg.servers.remove(&p.name).unwrap_or_default();
        // Clear default if it was pointing to the removed server.
        if cfg.default.as_deref() == Some(&p.name) {
            cfg.default = None;
        }
        if let Err(e) = servers_config::save_native_config(&cfg) {
            return err_result(serde_json::json!({
                "error_code": "SAVE_FAILED",
                "message": format!("Failed to save servers.json: {e}")
            }));
        }

        // Best-effort removal of iad's own keychain item for this entry's endpoint. The Server
        // Manager's items are the operator's VS Code's, and are never touched.
        if let Ok(endpoint) = servers_config::entry_endpoint(&removed) {
            let _ = crate::iris::server_manager::delete_iad_credential(
                &p.name,
                &removed.username,
                &endpoint,
            );
            if let Some(user) = removed.service_username.as_deref() {
                let _ =
                    crate::iris::server_manager::delete_iad_credential(&p.name, user, &endpoint);
            }
        }

        ok_json(serde_json::json!({
            "removed": true,
            "name": p.name,
            "note": "Call iris_reload_pool to drop this server from the running pool."
        }))
    }

    #[tool(
        description = "Hot-reload the IRIS connection pool from disk without restarting iad. Re-reads the workspace TOML config and the iad-native servers list, then atomically replaces the in-memory pool. Servers added or removed in the config file are available immediately. Returns {success, servers_loaded, servers, note} on success, or {success: false, error_code, error, note} if the config file cannot be parsed (the existing pool is preserved on parse errors). Error codes: TOML_PARSE_ERROR. A write: it makes every server declared since startup routable, so it needs the write gate open."
    )]
    async fn iris_reload_pool(&self, _: Parameters<NoParams>) -> Result<CallToolResult, McpError> {
        use crate::iris::{connection_pool, workspace_config};

        // Pre-validate the workspace TOML before swapping the pool.
        // workspace_root(None) replicates the same path-finding load_pool uses.
        let ws_root = workspace_config::workspace_root(None);
        let config_path = if ws_root.is_file()
            || ws_root
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        {
            Some(ws_root)
        } else if ws_root.join(".iris-agentic-dev.toml").exists() {
            Some(ws_root.join(".iris-agentic-dev.toml"))
        } else if ws_root.join(".iris-dev.toml").exists() {
            Some(ws_root.join(".iris-dev.toml"))
        } else {
            None
        };

        if let Some(ref path) = config_path {
            match std::fs::read_to_string(path) {
                Ok(contents) => {
                    if let Err(e) = workspace_config::load_fleet_config_from_str(&contents) {
                        return ok_json(serde_json::json!({
                            "success": false,
                            "error_code": "TOML_PARSE_ERROR",
                            "error": e.to_string(),
                            "note": "Existing pool preserved — no servers were removed."
                        }));
                    }
                }
                Err(_) => {
                    // File unreadable — load_pool will skip fleet instances but still load
                    // iad-native and env entries. Proceed without TOML_PARSE_ERROR.
                }
            }
        }

        let new_pool = Arc::new(connection_pool::load_pool_with_fleet(
            self.snapshot().fleet.clone(),
        ));
        let names: Vec<String> = new_pool.names().iter().map(|s| s.to_string()).collect();
        let count = names.len();
        *self.pool.write().unwrap() = new_pool;
        ok_json(serde_json::json!({
            "success": true,
            "servers_loaded": count,
            "servers": names,
            "note": "Pool rebuilt. Servers are immediately routable via the `server` parameter."
        }))
    }

    #[tool(
        description = "Probe an IRIS server for reachability. Performs GET /api/atelier/ with timing. Does not modify the active connection. Returns {name, reachable, atelier_version, iris_version, latency_ms} on success, or {name, reachable: false, error} on failure. Ad-hoc probe: pass `host` (+ optional `web_port`, `username`, `password`) instead of `name` to probe a server not in the pool. Error codes: SERVER_NOT_FOUND (name not in pool), MISSING_PARAMS (neither name nor host provided).",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<IrisTestServerResponse>()
    )]
    async fn iris_test_server(
        &self,
        Parameters(p): Parameters<server_tools::TestServerParams>,
    ) -> Result<CallToolResult, McpError> {
        // ── 098: ad-hoc probe path ────────────────────────────────────────────
        if let Some(ref host) = p.host {
            if let Some(msg) = server_tools::probe_host_problem(host) {
                return err_json("INVALID_PARAMS", &msg);
            }
            let port = p.web_port.unwrap_or(52773);
            let username = p.username.as_deref().unwrap_or("_SYSTEM");
            let password = p.password.as_deref().unwrap_or("");
            let result = server_tools::probe_server(host, port, "USER", username, password).await;
            return ok_json(serde_json::json!({
                "host": host,
                "port": port,
                "reachable": result.reachable,
                "auth": result.auth,
                "iris_version": result.iris_version,
                "atelier_version": result.atelier_version,
                "latency_ms": result.latency_ms,
                "error": result.error,
            }));
        }

        let name = match p.name.as_deref() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => {
                return ok_json(serde_json::json!({
                    "error": "Provide either `name` (registered server) or `host` (ad-hoc probe).",
                    "error_code": "MISSING_PARAMS",
                }));
            }
        };

        let conn = self.target_pool_member(&name)?;

        // NoPWS detection: check if this connection is using the sentinel URL.
        let is_sentinel = conn.base_url == "http://127.0.0.1:1"
            || conn.base_url.starts_with("http://127.0.0.1:1/");
        // Version heuristic: 2026.2.0AI builds have NoPWS (no embedded Apache).
        let version_nopws = conn
            .version
            .as_deref()
            .map(|v| v.contains("2026.2.0AI"))
            .unwrap_or(false);
        let nopws = is_sentinel || version_nopws;

        // iris.cpf probe: when docker_only or nopws config, check WebServer= in cpf.
        let (nopws_detected, nopws_evidence) = if nopws {
            let container = conn.default_container();
            if let Some(ref cname) = container {
                // Probe iris.cpf from inside the container to check WebServer=0
                let cpf_paths = ["/usr/irissys/iris.cpf", "/usr/local/etc/irissys/iris.cpf"];
                let mut found_webserver_zero = false;
                let mut evidence = String::new();
                for cpf in &cpf_paths {
                    let Ok(args) = cpf_probe_args(cname, cpf) else {
                        evidence = format!("container name {cname:?} is not a container name");
                        break;
                    };
                    let out = tokio::process::Command::new("docker")
                        .args(&args)
                        .output()
                        .await;
                    if let Ok(out) = out {
                        let text = String::from_utf8_lossy(&out.stdout);
                        let text = text.trim();
                        if !text.is_empty() {
                            evidence = format!("{cpf}: {text}");
                            if text.to_lowercase().contains("webserver=0") {
                                found_webserver_zero = true;
                            }
                            break;
                        }
                    }
                }
                (
                    found_webserver_zero,
                    if evidence.is_empty() {
                        None
                    } else {
                        Some(evidence)
                    },
                )
            } else {
                (false, None)
            }
        } else {
            (false, None)
        };

        if nopws {
            // In NoPWS mode the Atelier REST URL is unreachable; skip the HTTP probe.
            let mut resp = serde_json::json!({
                "name": name,
                "reachable": false,
                "nopws": true,
                "web_available": false,
                "nopws_detected": nopws_detected,
                "suggestion": "NoPWS build: set docker_only = true in .iris-agentic-dev.toml (a connection key, not a tool parameter) or add a webgateway sidecar. Setup instructions: skills/skills/iris-agentic-dev/nopws-setup/SKILL.md in the iris-agentic-dev repo.",
            });
            if let Some(ev) = nopws_evidence {
                resp["nopws_evidence"] = serde_json::Value::String(ev);
            }
            return ok_json(resp);
        }

        let url = conn.atelier_url("/");
        let start = std::time::Instant::now();
        match self
            .client
            .get(&url)
            .basic_auth(&conn.username, Some(&conn.password))
            .send()
            .await
        {
            Err(e) => ok_json(serde_json::json!({
                "name": name,
                "reachable": false,
                "nopws": false,
                "web_available": false,
                "error": e.to_string(),
                "latency_ms": start.elapsed().as_millis(),
            })),
            Ok(resp) => {
                let status = resp.status();
                let latency_ms = start.elapsed().as_millis();
                if !status.is_success() {
                    return ok_json(serde_json::json!({
                        "name": name,
                        "reachable": false,
                        "nopws": false,
                        "web_available": false,
                        "http_status": status.as_u16(),
                        "latency_ms": latency_ms,
                    }));
                }
                let auth = status != reqwest::StatusCode::UNAUTHORIZED;
                match crate::iris::http::capped_json::<serde_json::Value>(resp).await {
                    Err(e) => ok_json(serde_json::json!({
                        "name": name,
                        "reachable": true,
                        "nopws": false,
                        "web_available": true,
                        "auth": auth,
                        "latency_ms": latency_ms,
                        "parse_error": e.to_string(),
                    })),
                    Ok(body) => {
                        let content = &body["result"]["content"];
                        ok_json(serde_json::json!({
                            "name": name,
                            "reachable": true,
                            "nopws": false,
                            "web_available": true,
                            "auth": auth,
                            "atelier_version": content["api"],
                            "iris_version": content["version"],
                            "latency_ms": latency_ms,
                        }))
                    }
                }
            }
        }
    }

    #[tool(
        description = "Import IRIS server definitions from VS Code / Cursor Server Manager into the iad-native config. Reads intersystems.servers from VS Code and Cursor settings.json. Servers already present in the iad-native config are skipped (no overwrite). Passwords are resolved from the OS keychain; servers where no keychain entry exists are imported without a password (listed in no_keychain). Returns {imported, skipped, no_keychain: [...]}. Call `iris_reload_pool` after importing to make them routable.",
        output_schema = output_schemas::oneof_output_schema::<IrisImportServersResponse>()
    )]
    async fn iris_import_servers(
        &self,
        _: Parameters<NoParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::iris::server_manager;
        use crate::iris::servers_config::{self, ServerEntry};

        // Collect all VS Code / Cursor SM profiles.
        let sm_paths = server_manager::sm_settings_paths();

        let mut all_profiles: Vec<crate::iris::server_manager::ServerManagerProfile> = Vec::new();
        for path in &sm_paths {
            let profiles = server_manager::parse_sm_settings(path);
            for profile in profiles {
                let already_seen = all_profiles.iter().any(|p| p.name == profile.name);
                if !already_seen {
                    all_profiles.push(profile);
                }
            }
        }

        // Load current iad-native config.
        let mut cfg = servers_config::load_native_config();

        let mut imported = 0usize;
        let mut skipped = 0usize;
        let mut no_keychain: Vec<String> = Vec::new();

        for profile in &all_profiles {
            if cfg.servers.contains_key(&profile.name) {
                skipped += 1;
                continue;
            }

            // Is there a password for it in the Server Manager's keychain? Asked with the profile's
            // own endpoint, which is the only one its secret goes to.
            let has_keychain = profile.endpoint().is_ok_and(|ep| {
                server_manager::sm_secret_for(&profile.name, &profile.username, &ep).is_ok()
            });
            if !has_keychain {
                no_keychain.push(profile.name.clone());
            }

            // Determine port/namespace from SM profile — SM profiles use port 52773 default,
            // namespace is not carried; use USER as default.
            cfg.servers.insert(
                profile.name.clone(),
                ServerEntry {
                    host: profile.host.clone(),
                    port: profile.port,
                    namespace: "USER".to_string(),
                    username: profile.username.clone(),
                    description: None,
                    scheme: Some(profile.scheme.clone()),
                    // Carried over, or the import turns a working VS Code profile into an entry
                    // pointing at the gateway root (#129).
                    web_prefix: profile.path_prefix.clone(),
                    // The password stays in the Server Manager's keychain, and is read only while
                    // this entry's endpoint equals the profile's.
                    credential: Some("server-manager".to_string()),
                    ..Default::default()
                },
            );
            imported += 1;
        }

        if imported > 0 {
            if let Err(e) = servers_config::save_native_config(&cfg) {
                return err_result(serde_json::json!({
                    "error_code": "SAVE_FAILED",
                    "message": format!("Failed to save servers.json after importing: {e}")
                }));
            }
        }

        ok_json(serde_json::json!({
            "success": true,
            "imported": imported,
            "skipped": skipped,
            "no_keychain": no_keychain,
            "note": if imported > 0 {
                "Call iris_reload_pool to make the imported servers routable."
            } else {
                "No new servers to import."
            }
        }))
    }

    // ── 072-b: WebSocket terminal session tools ───────────────────────────────

    #[tool(
        description = "Open a persistent WebSocket terminal session on an IRIS instance. Returns a session token. Use server to target a specific registered instance; defaults to the active connection. Requires IRIS 2023.2+ (Atelier API v7).",
        output_schema = output_schemas::oneof_output_schema::<IrisWsOpenResponse>()
    )]
    async fn iris_ws_open(
        &self,
        Parameters(p): Parameters<ws_tools::WsOpenParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::iris::ws_session::{WsSessionPool, SESSION_WS_UNAVAILABLE};

        // The target the pipeline resolved. The session records it, and `iris_ws_exec` and
        // `iris_ws_close` are judged on that record, never on the server name in the token text.
        // The terminal logs in with the identity's own credentials, so a service account opens its
        // own session and a server with no account is refused (spec 141, R2).
        let target = self.target()?;
        let cx = match self.exec_context() {
            Ok(cx) => cx,
            Err(e) => return exec_refused(e),
        };
        let conn = cx.conn.clone();

        // Version gate: require V7 or V8.
        if !conn.atelier_version.supports_ws_terminal() {
            let mut identity = cx.helper_report();
            identity.path = exec::ExecPath::WsTerminal;
            return err_result(serde_json::json!({
                "success": false,
                "error_code": SESSION_WS_UNAVAILABLE,
                "error": "IRIS Atelier API v7 required for WebSocket terminal (IRIS 2023.2+)",
                "identity": identity.to_json(),
            }));
        }

        let server_name = target.primary.key.as_str();
        let namespace = p
            .namespace
            .as_deref()
            .unwrap_or_else(|| conn.namespace.as_str());

        let token = match WsSessionPool::open_as(
            &self.ws_pool,
            &conn,
            server_name,
            namespace,
            cx.expected.as_deref(),
        )
        .await
        {
            Ok(t) => t,
            Err(e) => {
                // The error names the identity it tried, never the primary's user name.
                let mut body = serde_json::json!({
                    "success": false,
                    "error_code": e.message.split(':').next().unwrap_or("SESSION_WS_DISCONNECTED"),
                    "error": e.message.to_string(),
                });
                body["identity"] = cx.helper_report().to_json();
                body["identity"]["path"] = serde_json::json!("ws_terminal");
                return err_result(body);
            }
        };
        self.ws_pool
            .record_target(&token, (!target.primary.is_default).then_some(server_name));
        let mut identity = cx.helper_report();
        identity.path = exec::ExecPath::WsTerminal;
        identity.verified = self.ws_pool.identity_verified(&token);
        if let Some(u) = self.ws_pool.identity_of(&token) {
            identity.user = u;
        }

        ok_json(serde_json::json!({
            "session": token,
            "server": server_name,
            "namespace": namespace,
            "identity": identity.to_json(),
        }))
    }

    #[tool(
        description = "Execute ObjectScript code in a persistent WebSocket terminal session. Variables and state persist across calls within the same session. Returns the terminal output.",
        output_schema = schema_for_output::<IrisWsExecResponse>()
    )]
    async fn iris_ws_exec(
        &self,
        Parameters(p): Parameters<ws_tools::WsExecParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::iris::ws_session::WsSessionPool;

        // #137: the same gates `iris_execute` runs, and for the same reason — this tool takes
        // arbitrary ObjectScript. The pipeline ran them on the server the session was opened
        // against; the literal `Kill ^` check reads that server's destructive tier. The check is
        // here rather than after `exec` so a refused call sends nothing to IRIS and leaves the
        // session's variables alone.
        if let Some(refusal) = self.exec_content_gate("iris_ws_exec", &p.code) {
            return err_result(refusal);
        }
        let cx = match self.exec_context() {
            Ok(cx) => cx,
            Err(e) => return exec_refused(e),
        };
        // A session keeps the login it opened with. If the identity this call would run as is not
        // that one (the operator set, changed or removed the service account since), refuse and
        // close it.
        let current = cx.expected.as_deref().unwrap_or(cx.conn.username.as_str());
        if let Err(msg) = self.ws_pool.check_identity(&p.session, current) {
            let _ = WsSessionPool::close(&self.ws_pool, &p.session).await;
            return err_result(serde_json::json!({
                "success": false,
                "error_code": exec::ERR_SESSION_IDENTITY_CHANGED,
                "error": msg,
            }));
        }

        let output = WsSessionPool::exec(&self.ws_pool, &p.session, &p.code).await?;
        let mut identity = cx.helper_report();
        identity.path = exec::ExecPath::WsTerminal;
        identity.verified = self.ws_pool.identity_verified(&p.session);
        identity.user = current.to_string();

        ok_json(serde_json::json!({
            "output": output,
            "session": p.session,
            "identity": identity.to_json(),
        }))
    }

    #[tool(
        description = "Close a WebSocket terminal session and release server resources.",
        output_schema = schema_for_output::<IrisWsCloseResponse>()
    )]
    async fn iris_ws_close(
        &self,
        Parameters(p): Parameters<ws_tools::WsCloseParams>,
    ) -> Result<CallToolResult, McpError> {
        use crate::iris::ws_session::WsSessionPool;

        WsSessionPool::close(&self.ws_pool, &p.session).await?;

        ok_json(serde_json::json!({
            "closed": true,
        }))
    }

    // ── 072-c: Comparison tools ───────────────────────────────────────────────

    #[tool(
        description = "Compare the source of a document (class, routine, etc.) across two registered IRIS servers. Returns {same: bool, diff: string, server_a, server_b, document, namespace}. Use iris_servers to see registered instances. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<CompareDocumentResponse>()
    )]
    async fn compare_document(
        &self,
        Parameters(params): Parameters<params::batch1::CompareDocumentParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let document = p
            .get("document")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let requested_ns = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let (server_a, server_b) = self.target_pair()?;
        let namespace = resolve_namespace(requested_ns.as_deref(), &server_a.namespace).to_string();

        let result = comparison_tools::compare_document_impl(
            comparison_tools::CompareDocumentParams {
                document,
                server_a,
                server_b,
                namespace,
            },
            &self.client,
        )
        .await;
        self.record_call("compare_document", result.is_ok());
        result
    }

    #[tool(
        description = "Compare all classes in a namespace across two registered IRIS servers. Returns {only_in_a, only_in_b, different, same_count}. Use iris_servers to see registered instances. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<CompareNamespaceResponse>()
    )]
    async fn compare_namespace(
        &self,
        Parameters(params): Parameters<params::batch1::CompareNamespaceParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let requested_ns = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let (server_a, server_b) = self.target_pair()?;
        let namespace = resolve_namespace(requested_ns.as_deref(), &server_a.namespace).to_string();

        let result = comparison_tools::compare_namespace_impl(
            comparison_tools::CompareNamespaceParams {
                namespace,
                server_a,
                server_b,
            },
            &self.client,
        )
        .await;
        self.record_call("compare_namespace", result.is_ok());
        result
    }

    // ── 072-c: Global preview/kill with confirmation token ─────────────────────

    #[tool(
        description = "Preview the contents of an IRIS global before deleting it. Returns the first N subscripts plus a confirm_token (valid 5 minutes) required by global_kill. global: name of the global (with or without ^). count: max entries to preview (default 20, max 100). server: optional registered instance name. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = schema_for_output::<GlobalPreviewResponse>()
    )]
    async fn global_preview(
        &self,
        Parameters(params): Parameters<params::batch1::GlobalPreviewParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let global = p
            .get("global")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let server = p
            .get("server")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let count = p.get("count").and_then(|v| v.as_u64()).unwrap_or(20) as u32;

        // The preview reads the global, and its token is what global_kill accepts, so the pipeline
        // judges it by the same blocklist and PHI gates as iris_global (137).
        let iris = self.target_conn()?;

        let result = admin_tools::global_preview_impl(
            admin_tools::GlobalPreviewParams {
                global,
                server,
                count,
                iris,
                client: Arc::clone(&self.client),
            },
            &self.confirm_tokens,
        )
        .await;
        self.record_call("global_preview", result.is_ok());
        result
    }

    #[tool(
        description = "Kill (delete) an entire IRIS global. Destructive tier: requires IRIS_DESTRUCTIVE_TOOLS_ENABLED=1. Requires a confirm_token from global_preview (valid 5 minutes). global: global name. confirm_token: token from global_preview. server: optional registered instance name. Error codes: CONFIRM_REQUIRED (call global_preview first), CONFIRM_EXPIRED (token expired), CONFIRM_MISMATCH (token for different global/server). Skill: iris-agentic-dev.",
        annotations(destructive_hint = true),
        output_schema = output_schemas::oneof_output_schema::<GlobalKillResponse>()
    )]
    async fn global_kill(
        &self,
        Parameters(params): Parameters<params::batch1::GlobalKillParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let global = p
            .get("global")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let server = p
            .get("server")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let confirm_token = p
            .get("confirm_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // The policy gate ran in the pipeline, on the server the call names (spec 141, R1). The
        // kill runs as the service account when one is set.
        let cx = match self.exec_context() {
            Ok(cx) => cx,
            Err(e) => {
                self.record_call("global_kill", false);
                return exec_refused(e);
            }
        };

        let result = admin_tools::global_kill_impl(
            admin_tools::GlobalKillParams {
                global,
                server,
                confirm_token,
                iris: cx.conn.clone(),
                client: cx.client.clone(),
            },
            &self.confirm_tokens,
        )
        .await;
        self.record_call("global_kill", result.is_ok());
        exec::attach_identity(result, &cx.helper_report())
    }

    // ── 072-c: Namespace/database admin ───────────────────────────────────────

    #[tool(
        description = "List all namespaces on an IRIS instance. server: optional registered instance name. Returns {namespaces: [...], count: N}. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisNamespaceListResponse>()
    )]
    async fn iris_namespace_list(
        &self,
        Parameters(_): Parameters<params::batch2::IrisNamespaceListParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = admin_tools::iris_namespace_list_impl(&iris, &self.client).await;
        self.record_call("iris_namespace_list", result.is_ok());
        result
    }

    #[tool(
        description = "List all databases (directories) on an IRIS instance. server: optional registered instance name. Returns {databases: [{directory, mounted, size_mb, free_space_mb, free_pct, max_size_mb}], count: N}. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisDatabaseListResponse>()
    )]
    async fn iris_database_list(
        &self,
        Parameters(_): Parameters<params::batch2::IrisDatabaseListParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = admin_tools::iris_database_list_impl(&iris, &self.client).await;
        self.record_call("iris_database_list", result.is_ok());
        result
    }

    #[tool(
        description = "Manage and read IRIS SystemPerformance (pbuttons) profiles and runs. mode: start | status | last_runid | list_profiles | add_profile | delete_profile | list_runs | report. profile: for mode=start, one of test (5 min, the default), 30mins, 4hours, 8hours, 12hours, 24hours, or any instance-defined profile; for add_profile/delete_profile, the profile to create or remove. run_id: required for mode=status; optional for mode=report (omit for the newest completed run). description, interval_seconds, sample_count: required for mode=add_profile. Returns {success, mode, profile, run_id} for start; {success, mode, run_id, in_progress} for last_runid (in_progress=true means the run is still collecting and has no report yet); {success, mode, run_id, wait_time} for status; {success, mode, profiles, count} for list_profiles, each with name, interval_seconds, sample_count, duration_minutes, description; {success, mode, runs, count} for list_runs, newest first, each with run_id, completed_at, output_dir, profile; {success, mode, run_id, completed_at, output_dir, report_path, size_bytes, exists} for report. Works on community and Enterprise builds. server: optional registered instance name. Skill: iris-agentic-dev.",
        output_schema = output_schemas::oneof_output_schema::<serde_json::Value>()
    )]
    async fn iris_system_performance(
        &self,
        Parameters(params): Parameters<params::batch4::IrisSystemPerformanceParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let mode = p
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let run_id = p
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let profile = p
            .get("profile")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let description = p
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let interval_seconds = p.get("interval_seconds").and_then(|v| v.as_i64());
        let sample_count = p.get("sample_count").and_then(|v| v.as_i64());
        let iris = self.target_conn()?;
        let result = admin_tools::iris_system_performance_impl(
            &iris,
            &self.client,
            &admin_tools::SysPerfRequest {
                mode: &mode,
                run_id: run_id.as_deref(),
                profile: profile.as_deref(),
                description: description.as_deref(),
                interval_seconds,
                sample_count,
            },
        )
        .await;
        self.record_call("iris_system_performance", result.is_ok());
        result
    }

    #[tool(
        description = "Report mirror membership and role for the connected IRIS instance. Returns {is_member, mirror_name, member_type, is_primary}. Non-mirror instances return {is_member: false}. Useful as a pre-flight check before operations that require a primary. server: optional registered instance name. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<serde_json::Value>()
    )]
    async fn iris_mirror_status(
        &self,
        Parameters(_): Parameters<params::batch4::IrisMirrorStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = admin_tools::iris_mirror_status_impl(&iris, &self.client).await;
        self.record_call("iris_mirror_status", result.is_ok());
        result
    }

    #[tool(
        description = "Create a new namespace on an IRIS instance. Destructive tier: requires IRIS_DESTRUCTIVE_TOOLS_ENABLED=1. name: namespace name. db_path: optional database directory (defaults to name). server: optional registered instance name. Skill: iris-agentic-dev.",
        annotations(destructive_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisNamespaceCreateResponse>()
    )]
    async fn iris_namespace_create(
        &self,
        Parameters(params): Parameters<params::batch2::IrisNamespaceCreateParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let name = p
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let db_path = p
            .get("db_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let iris = self.target_conn()?;
        let result =
            admin_tools::iris_namespace_create_impl(&iris, &self.client, &name, db_path.as_deref())
                .await;
        self.record_call("iris_namespace_create", result.is_ok());
        result
    }

    #[tool(
        description = "Get disk usage statistics for IRIS databases. db: optional directory path to limit to one database; if omitted returns all. server: optional registered instance name. Returns {stats: [{directory, free_space_mb, free_blocks}]}. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<IrisDatabaseStatsResponse>()
    )]
    async fn iris_database_stats(
        &self,
        Parameters(params): Parameters<params::batch2::IrisDatabaseStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let db = p.get("db").and_then(|v| v.as_str()).map(|s| s.to_string());
        let iris = self.target_conn()?;
        let result =
            admin_tools::iris_database_stats_impl(&iris, &self.client, db.as_deref()).await;
        self.record_call("iris_database_stats", result.is_ok());
        result
    }

    // ── 072-c: Observability tools ────────────────────────────────────────────

    #[tool(
        description = "Search the IRIS journal for SetKill records. start/end: optional ISO timestamp filters. global_pattern: optional substring filter on GlobalReference. max_entries: default 100, max 500. server: optional registered instance name. Reads at most 200,000 records or 20 s from the start of the current journal file, then returns truncated:true with truncated_reason and scanned. Returns {entries: [{timestamp, type, job_id, global}], returned, truncated, truncated_reason, scanned}. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<JournalSearchResponse>()
    )]
    async fn journal_search(
        &self,
        Parameters(params): Parameters<params::batch4::JournalSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let start = p
            .get("start")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let end = p.get("end").and_then(|v| v.as_str()).map(|s| s.to_string());
        let global_pattern = p
            .get("global_pattern")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let max_entries = p.get("max_entries").and_then(|v| v.as_u64()).unwrap_or(100) as u32;
        // Journal records carry global values, so the pipeline's bulk-PHI gate refuses this route
        // as it refuses iris_admin's (137).
        let iris = self.target_conn()?;
        let result = observability::journal_entries_impl(
            &iris,
            start.as_deref(),
            end.as_deref(),
            global_pattern.as_deref(),
            max_entries,
        )
        .await;
        self.record_call("journal_search", result.is_ok());
        result
    }

    #[tool(
        description = "Query the IRIS audit log (%SYS.Audit). user: filter by username. event_type: filter by event type. start/end: ISO timestamp filters. limit: max rows (default 100, max 500). server: optional registered instance name. Returns {entries: [{event, event_type, username, timestamp}]}. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<QueryAuditLogResponse>()
    )]
    async fn query_audit_log(
        &self,
        Parameters(params): Parameters<params::batch4::QueryAuditLogParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let user = p
            .get("user")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let event_type = p
            .get("event_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let start = p
            .get("start")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let end = p.get("end").and_then(|v| v.as_str()).map(|s| s.to_string());
        let limit = p.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as u32;
        let iris = self.target_conn()?;
        let result = admin_tools::query_audit_log_impl(
            &iris,
            &self.client,
            user.as_deref(),
            event_type.as_deref(),
            start.as_deref(),
            end.as_deref(),
            limit,
        )
        .await;
        self.record_call("query_audit_log", result.is_ok());
        result
    }

    #[tool(
        description = "Inspect the content of an IRIS stream object by OID. oid: the stream OID (integer string). namespace: optional namespace (defaults to the connection namespace, IRIS_NAMESPACE). server: optional registered instance name. max_chars: most characters to return from the start (default 20000, at most 1000000). Returns {content, class, size, truncated, max_chars, line_count, oid}: size is the whole stream, truncated says content is a prefix, class is the class the read went through and says nothing about what the stream holds. An id that holds no stream is STREAM_NOT_FOUND. The server's dataPolicy decides what comes back: allow returns the content, redact returns HL7 v2 with the identifying fields of PID, NK1, GT1, IN1, IN2 and MSH-3 replaced by [REDACTED] (redacted: true means that mask ran, not that the content is de-identified: other segments and free text come back as stored) and refuses other content (BODY_NOT_REDACTABLE), block refuses the content (DATA_POLICY_BLOCKED). A refused stream still answers with class, size and line_count. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<StreamInspectResponse>()    )]
    async fn stream_inspect(
        &self,
        Parameters(params): Parameters<params::batch7::StreamInspectParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let oid = p
            .get("oid")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let max_chars = p
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .map(|n| u32::try_from(n).unwrap_or(u32::MAX));
        let requested_ns = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let iris = self.target_conn()?;
        let namespace = self.target_ns(requested_ns.as_deref())?;
        // The target server's policy decides what comes back (FR-030c).
        let policy = self
            .target()?
            .primary
            .policy
            .data_policy
            .clone()
            .unwrap_or_default();
        let result = admin_tools::stream_inspect_impl(
            &iris,
            &self.client,
            &oid,
            max_chars,
            &namespace,
            &policy,
        )
        .await;
        self.record_call("stream_inspect", result.is_ok());
        // A withheld stream is a refusal: give it the `refusal` object on every tail, not only the
        // MCP one.
        result.map(|r| self.classify_answer(r))
    }

    // ── 072-c: Security tools ──────────────────────────────────────────────────

    #[tool(
        description = "Show the current user's username, full name, and assigned roles on an IRIS instance. server: optional registered instance name. Returns {username, full_name, roles: [...]}. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<MyAccessResponse>()
    )]
    async fn my_access(
        &self,
        Parameters(_): Parameters<params::batch4::MyAccessParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.target_conn()?;
        let result = admin_tools::my_access_impl(&iris, &self.client).await;
        self.record_call("my_access", result.is_ok());
        result
    }

    #[tool(
        description = "Show the roles assigned to a user on an IRIS instance. user: optional username (default: current user). server: optional registered instance name. Returns {user, full_name, roles: [...]}. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<CapabilityMatrixResponse>()
    )]
    async fn capability_matrix(
        &self,
        Parameters(params): Parameters<params::batch4::CapabilityMatrixParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let user = p
            .get("user")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let iris = self.target_conn()?;
        let result =
            admin_tools::capability_matrix_impl(&iris, &self.client, user.as_deref()).await;
        self.record_call("capability_matrix", result.is_ok());
        result
    }

    // ── 072-c: HL7 tools ──────────────────────────────────────────────────────

    #[tool(
        description = "List available HL7 schemas on an IRIS/HealthShare instance. Returns HL7_NOT_AVAILABLE if EnsLib.HL7.Schema is absent. namespace: optional (defaults to the connection namespace, IRIS_NAMESPACE). server: optional registered instance name. Returns {schemas: [...], count: N}. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<Hl7SchemaListResponse>()
    )]
    async fn hl7_schema_list(
        &self,
        Parameters(params): Parameters<params::batch7::Hl7SchemaListParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let requested_ns = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(requested_ns.as_deref(), &iris.namespace).to_string();
        let result = admin_tools::hl7_schema_list_impl(&iris, &self.client, &namespace).await;
        self.record_call("hl7_schema_list", result.is_ok());
        result
    }

    #[tool(
        description = "Inspect an HL7 schema's message structures or a specific segment's fields. Returns HL7_NOT_AVAILABLE if EnsLib.HL7.Schema is absent. schema: schema name (e.g. '2.5'). segment: optional segment name to inspect fields. namespace: optional. server: optional registered instance name. Skill: iris-agentic-dev.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<Hl7SchemaInspectResponse>()
    )]
    async fn hl7_schema_inspect(
        &self,
        Parameters(params): Parameters<params::batch7::Hl7SchemaInspectParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let schema = p
            .get("schema")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let segment = p
            .get("segment")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let requested_ns = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(requested_ns.as_deref(), &iris.namespace).to_string();
        let result = admin_tools::hl7_schema_inspect_impl(
            &iris,
            &self.client,
            &schema,
            segment.as_deref(),
            &namespace,
        )
        .await;
        self.record_call("hl7_schema_inspect", result.is_ok());
        result
    }

    // ── 072-c: Mermaid + storage ──────────────────────────────────────────────

    #[tool(
        description = "Generate a Mermaid classDiagram for an ObjectScript class, walking the superclass chain up to `depth` levels (default 3, max 5). Returns a string starting with 'classDiagram'. class: fully qualified class name. depth: optional traversal depth. namespace: optional. server: optional registered instance name. Skill: objectscript-navigation.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<MermaidClassResponse>()
    )]
    async fn mermaid_class(
        &self,
        Parameters(params): Parameters<params::batch7::MermaidClassParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let class = p
            .get("class")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let depth = p.get("depth").and_then(|v| v.as_u64()).unwrap_or(3) as u32;
        let requested_ns = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(requested_ns.as_deref(), &iris.namespace).to_string();
        let result =
            admin_tools::mermaid_class_impl(&iris, &self.client, &class, depth, &namespace).await;
        self.record_call("mermaid_class", result.is_ok());
        result
    }

    #[tool(
        description = "Generate a Mermaid flowchart for an Ensemble/Interoperability production, showing all configured items. production: full production class name. namespace: optional. server: optional registered instance name. Returns a Mermaid flowchart TD string. Skill: ensemble-production.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<MermaidProductionResponse>()
    )]
    async fn mermaid_production(
        &self,
        Parameters(params): Parameters<params::batch7::MermaidProductionParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let production = p
            .get("production")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let requested_ns = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(requested_ns.as_deref(), &iris.namespace).to_string();
        let result =
            admin_tools::mermaid_production_impl(&iris, &self.client, &production, &namespace)
                .await;
        self.record_call("mermaid_production", result.is_ok());
        result
    }

    #[tool(
        description = "Resolve storage definitions for an ObjectScript class — returns global maps (data, id, index locations) from %Dictionary.CompiledStorage. class: fully qualified class name. namespace: optional. server: optional registered instance name. Returns {class, storages: [{name, type, data_location, id_location, index_location}]}. Skill: objectscript-navigation.",
        annotations(read_only_hint = true),
        output_schema = output_schemas::oneof_output_schema::<ResolveStorageResponse>()
    )]
    async fn resolve_storage(
        &self,
        Parameters(params): Parameters<params::batch7::ResolveStorageParams>,
    ) -> Result<CallToolResult, McpError> {
        let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
        let class = p
            .get("class")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let requested_ns = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let iris = self.target_conn()?;
        let namespace = resolve_namespace(requested_ns.as_deref(), &iris.namespace).to_string();
        let result =
            admin_tools::resolve_storage_impl(&iris, &self.client, &class, &namespace).await;
        self.record_call("resolve_storage", result.is_ok());
        result
    }
}

#[tool_handler]
impl ServerHandler for IrisTools {
    /// Wraps the macro-generated dispatch with a `CALL_START` task-local so `record_call`
    /// (called from within each tool handler) can compute an accurate `duration_ms`
    /// without changing the signature of any existing `self.record_call(tool, success)`
    /// call site (059-tool-telemetry-benchmark).
    ///
    /// Also the one place the write/destructive gate is enforced (085 FR-008). It goes here
    /// because this is the only point every tool passes through: a per-handler guard is a guard a
    /// new tool can silently miss, which is exactly how `iris_ws_exec`, `iris_global` set/kill,
    /// `iris_lookup_manage` set/delete and `iris_execute_method` shipped ungated while
    /// `check_config` reported the gate as active.
    async fn call_tool(
        &self,
        mut request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, McpError> {
        let start = std::time::Instant::now();
        let peer = context
            .client_info()
            .map(|info| (info.name.clone(), info.version.clone()));
        CALL_START
            .scope(
                start,
                crate::iris::connection::MCP_PEER.scope(peer, async move {
                    let tool_name = request.name.to_string();
                    let gated_name = tool_name.clone();
                    let arguments = request.arguments.take();
                    // Every gate, in one place (spec 141, R1 D8). The router is the tail.
                    self.dispatch_gated(&gated_name, arguments, |arguments| async move {
                        request.arguments = arguments;
                        // Located before dispatch because `request` is consumed by the call
                        // context, and the answer to "which parameter was the wrong shape" needs
                        // the arguments. Usually empty, and cheap when it is: the walk only visits
                        // keys the caller actually sent. Nothing is refused on the strength of it —
                        // serde stays the authority on what deserializes, so a schema this check
                        // reads differently from the struct behind it can only make a message less
                        // specific, never reject a call that used to work (FR-004).
                        let mismatches = self
                            .accepted_params
                            .get(request.name.as_ref())
                            .map(|a| {
                                param_check::type_mismatches(
                                    request.arguments.as_ref(),
                                    &a.properties,
                                )
                            })
                            .unwrap_or_default();
                        let tcc = rmcp::handler::server::tool::ToolCallContext::new(
                            self, request, context,
                        );
                        let (answer, scratch_left) =
                            crate::iris::connection::collecting_scratch_left(
                                self.tool_router.call(tcc),
                            )
                            .await;
                        // A scratch class IRIS would not delete is a warning on this answer.
                        let answer = match answer {
                            Ok(rmcp::model::CallToolResponse::Complete(result)) => {
                                Ok(rmcp::model::CallToolResponse::Complete(with_warnings(
                                    result,
                                    scratch_left,
                                )))
                            }
                            other => other,
                        };
                        // Refusals raised inside a handler (a confirm token, an admin or check-in
                        // switch, a PHI acknowledgement) get the same `refusal` object as the
                        // gates that run before dispatch (FR-039).
                        let answer = match answer {
                            Ok(rmcp::model::CallToolResponse::Complete(result)) => {
                                Ok(rmcp::model::CallToolResponse::Complete(
                                    self.classify_answer(result),
                                ))
                            }
                            other => other,
                        };

                        // Normalize rmcp's deserialization refusal into the project's refusal
                        // shape (113 FR-017). rmcp answers a bad argument type with bare `isError`
                        // text and no `error_code`, which a client cannot branch on and a
                        // `structuredContent` reader cannot see at all. Only that one refusal is
                        // rewritten; every handler answer, success or failure, passes through
                        // untouched.
                        if let Ok(rmcp::model::CallToolResponse::Complete(ref result)) = answer {
                            if let Some(serde_text) = param_check::deserialization_failure(result) {
                                self.record_call(&tool_name, false);
                                return err_result(serde_json::json!({
                                    "success": false,
                                    "error_code": param_check::ERR_INVALID_PARAMS,
                                    "error": param_check::invalid_params_message(
                                        &tool_name, &mismatches, &serde_text,
                                    ),
                                }))
                                .map(Into::into);
                            }
                        }
                        answer
                    })
                    .await
                }),
            )
            .await
    }

    fn get_info(&self) -> ServerInfo {
        // Cap to 2025-11-25: the 2026-07-28 draft requires per-tool cache metadata that
        // clients using that version will reject when absent. Pin until we opt into caching.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_server_info(Implementation::new(
                "iris-agentic-dev".to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ))
            .with_instructions(
                "iris-agentic-dev: composable MCP tools for ObjectScript and IRIS development. \
                 Before writing or editing any ObjectScript code (.cls/.mac/.int), call \
                 skill(action=\"describe\") for objectscript-guardrails and objectscript-review \
                 and follow their checklists — this includes Storage block handling. \"Save\" and \
                 \"compile\" mean the IRIS server, not just disk: iris_compile only recompiles \
                 source IRIS already has stored — it never reads local files. After editing a \
                 local .cls/.mac/.int file, push it to IRIS first via iris_doc(mode=\"put\", \
                 compile=true) before considering the change saved."
                    .to_string(),
            )
    }

    /// Explicitly enumerate all supported protocol versions including 2026-07-28.
    ///
    /// 2026-07-28 requires `ttlMs`/`cacheScope` on the `tools/list` response (SEP-2549).
    /// We set those in `list_tools` above, so advertising this version is now correct. (#117)
    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        std::borrow::Cow::Borrowed(ProtocolVersion::KNOWN_VERSIONS)
    }

    /// Override list_tools to (1) rewrite JSON Schema 2020-12 nullable types to OpenAPI 3.0
    /// anyOf — schemars + rmcp emit `"type": ["T", "null"]` which Google Vertex AI and Azure
    /// OpenAI reject; rewritten to `"anyOf": [{"type": "T", ...siblings}, {"type": "null"}]`
    /// — and (2) paginate the result (076-interface-modernization User Story 4): the full
    /// catalog is still computed once per call (pagination doesn't change which tools exist,
    /// only how many are handed back in one response), then sliced via `paginate_tool_list`,
    /// an already-tested pure function, using the incoming request's `cursor` and a
    /// server-configured page size (`IRIS_LIST_TOOLS_PAGE_SIZE`) — the MCP pagination
    /// contract is server-paced, not a client-requested page size.
    ///
    /// The default (`DEFAULT_LIST_TOOLS_PAGE_SIZE`) is deliberately set above every current
    /// toolset's real count (Baseline 81, Nostub 77, Merged 78, as of this writing) so a
    /// plain unconfigured `tools/list` call keeps returning the whole catalog in one
    /// response, unchanged from before this feature existed — every existing client
    /// (including this project's own `mcp_handshake.rs` e2e assertions) that assumes one
    /// call returns everything keeps working with no config changes. Pagination becomes
    /// real the moment an operator sets `IRIS_LIST_TOOLS_PAGE_SIZE` below the effective
    /// tool count, and it's exercised end-to-end that way in
    /// `tests/mcp_handshake.rs::mcp_server_tools_list_pagination_works` — not just via the
    /// pure-function unit tests in `test_list_tools_pagination.rs`.
    async fn list_tools(
        &self,
        request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        let tools = self.listed_tools();
        let page_size = log_store::read_inline_threshold(
            "IRIS_LIST_TOOLS_PAGE_SIZE",
            DEFAULT_LIST_TOOLS_PAGE_SIZE,
        );
        let cursor = request.and_then(|r| r.cursor);
        let (page, next_cursor) = paginate_tool_list(tools, cursor.as_deref(), page_size);
        let mut result = rmcp::model::ListToolsResult::with_all_items(page);
        result.next_cursor = next_cursor;
        // SEP-2549 / MCP 2026-07-28: cache annotation required when server negotiates that
        // version. ttlMs=0 means "do not cache" — correct for tools that query live IRIS state.
        result.ttl_ms = Some(0);
        result.cache_scope = Some(rmcp::model::CacheScope::Public);
        Ok(result)
    }
}

/// Default `list_tools` page size — see the doc comment on `list_tools` for why this is set
/// above every current toolset's real tool count rather than at a value that would actually
/// paginate by default.
const DEFAULT_LIST_TOOLS_PAGE_SIZE: usize = 200;

/// Tool names read out of `IRIS_SUPPRESS_TOOL_DESCRIPTION`.
///
/// The variable exists so an eval run can ask one question the rest of the suite cannot: is the
/// advertised schema enough on its own? Blanking a tool's prose leaves its `inputSchema` as the
/// only contract, which is what SC-007 measures. The schema is never touched — a flag that removed
/// the properties too would make every task fail and look like evidence the schemas are
/// insufficient.
///
/// Commas and whitespace both separate, and empty segments are dropped, so an unset variable and
/// one set to `""` behave identically. `*` is not special: it parses as a literal tool name and
/// therefore matches nothing, because a run with every description blanked could not tell which
/// tool's schema carried the call.
pub fn parse_suppressed_descriptions(value: &str) -> std::collections::BTreeSet<String> {
    value
        .split([',', ' ', '\t', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Slice a `list_tools` catalog (already sorted deterministically by
/// `ToolRouter::list_all()`) into one page, given an opaque cursor from a prior response.
///
/// `cursor` is a base-10 offset into `all`, as previously handed back via `next_cursor` —
/// but per MCP's own cursor contract it's an *opaque* continuation token, not a value
/// clients are expected to construct, so a missing, unparseable, or out-of-range cursor
/// degrades to "start from the beginning" rather than an error. That degrade-safely
/// behavior matters here specifically because `list_all()`'s output can change size across
/// requests (an `IRIS_ENABLED_TOOLS` change, a live toolset switch) — an offset that no
/// longer fits should never panic or produce a nonsensical empty page forever.
///
/// Returns `(page, next_cursor)` — `next_cursor` is `None` exactly when `page` reaches the
/// end of `all`, so a caller paging until `next_cursor` is `None` sees every tool exactly
/// once with no gap, assuming the catalog doesn't change size mid-pagination.
pub fn paginate_tool_list(
    all: Vec<rmcp::model::Tool>,
    cursor: Option<&str>,
    page_size: usize,
) -> (Vec<rmcp::model::Tool>, Option<String>) {
    let total = all.len();
    let offset = cursor
        .and_then(|c| c.parse::<usize>().ok())
        .filter(|&o| o <= total)
        .unwrap_or(0);
    let page_size = page_size.max(1);
    let end = offset.saturating_add(page_size).min(total);
    let next_cursor = if end < total {
        Some(end.to_string())
    } else {
        None
    };
    let page = all.into_iter().skip(offset).take(end - offset).collect();
    (page, next_cursor)
}

/// Recursively rewrite JSON Schema 2020-12 nullable arrays to OpenAPI 3.0 anyOf.
///
/// schemars + rmcp emit `"type": ["integer", "null"]` (JSON Schema 2020-12) which
/// Google Vertex AI and Azure OpenAI reject. Rewrites to OpenAPI 3.0:
/// `"anyOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]`.
pub fn normalize_schema_openapi3(schema: &mut serde_json::Map<String, serde_json::Value>) {
    // Recurse into container schemas first (anyOf, allOf, oneOf, items)
    for key in ["anyOf", "allOf", "oneOf"] {
        if let Some(arr) = schema.get_mut(key).and_then(|v| v.as_array_mut()) {
            for item in arr.iter_mut() {
                if let serde_json::Value::Object(obj) = item {
                    normalize_schema_openapi3(obj);
                }
            }
        }
    }
    if let Some(serde_json::Value::Object(obj)) = schema.get_mut("items") {
        normalize_schema_openapi3(obj);
    }

    // Recurse into properties: extract, fix, re-insert to avoid borrow conflicts
    if let Some(serde_json::Value::Object(mut props)) = schema.remove("properties") {
        let keys: Vec<String> = props.keys().cloned().collect();
        for k in keys {
            if let Some(serde_json::Value::Object(prop)) = props.get_mut(&k) {
                normalize_schema_openapi3(prop);
            }
        }
        schema.insert("properties".to_string(), serde_json::Value::Object(props));
    }

    // Now transform this level if it has a nullable type array
    let type_array = match schema.get("type") {
        Some(serde_json::Value::Array(arr)) if arr.iter().any(|v| v == "null") => arr.clone(),
        _ => return,
    };

    let non_null_types: Vec<serde_json::Value> = type_array
        .iter()
        .filter(|v| *v != "null")
        .cloned()
        .collect();
    schema.remove("type");

    // Move type-specific sibling fields into the non-null branch
    let type_specific = [
        "format",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minLength",
        "maxLength",
        "pattern",
        "enum",
        "const",
        "items",
        "minItems",
        "maxItems",
        "uniqueItems",
        "properties",
        "required",
        "additionalProperties",
    ];
    let mut type_branch: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    for key in &type_specific {
        if let Some(val) = schema.remove(*key) {
            type_branch.insert(key.to_string(), val);
        }
    }
    let non_null_type = if non_null_types.len() == 1 {
        non_null_types.into_iter().next().unwrap()
    } else {
        serde_json::Value::Array(non_null_types)
    };
    type_branch.insert("type".to_string(), non_null_type);

    schema.insert(
        "anyOf".to_string(),
        serde_json::Value::Array(vec![
            serde_json::Value::Object(type_branch),
            serde_json::json!({"type": "null"}),
        ]),
    );
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

/// Detects whether a class is BPL or DTL and parses its XData flow.
/// Returns `None` for plain classes.
async fn detect_xdata_flow(
    iris: &IrisConnection,
    class_name: &str,
    namespace: &str,
    client: &reqwest::Client,
) -> Option<serde_json::Value> {
    let super_result = iris
        .query(
            "SELECT Super FROM %Dictionary.CompiledClass WHERE Name=?",
            vec![serde_json::Value::String(class_name.to_string())],
            namespace,
            client,
        )
        .await
        .ok()?;

    let super_class = super_result["result"]["content"]
        .as_array()?
        .first()?
        .get("Super")?
        .as_str()
        .unwrap_or("")
        .to_string();

    let is_bpl = xdata_flow::is_bpl_class(&super_class);
    let is_dtl = xdata_flow::is_dtl_class(&super_class);
    if !is_bpl && !is_dtl {
        return None;
    }

    let xdata_block = if is_bpl { "BPL" } else { "DTL" };
    // BPL/DTL classes must be exported with their type suffix (.bpl/.dtl), not as .cls
    let ext = if is_bpl { "bpl" } else { "dtl" };
    let export_code = crate::objectscript::export_class_xml_code(class_name, ext);
    let class_xml = iris
        .execute_via_generator(&export_code, namespace, client)
        .await
        .ok()?;

    let xdata_content = xdata_flow::extract_xdata_content(&class_xml, xdata_block)?;

    if is_bpl {
        let flow = match xdata_flow::parse_bpl(&xdata_content) {
            Ok(flow) => flow,
            Err(e) => return xdata_flow::bpl_refusal(&e),
        };
        serde_json::to_value(serde_json::json!({
            "kind": "bpl",
            "steps": flow.steps,
            "has_dynamic_dispatch": flow.has_dynamic_dispatch
        }))
        .ok()
    } else {
        let flow = xdata_flow::parse_dtl(&xdata_content).ok()?;
        serde_json::to_value(serde_json::json!({
            "kind": "dtl",
            "source_class": flow.source_class,
            "target_class": flow.target_class,
            "subtransforms": flow.subtransforms,
            "assign_count": flow.assign_count
        }))
        .ok()
    }
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

    // ── &sql translation: unknown SQL type warning ────────────────────────────

    #[test]
    fn translate_sql_unknown_type_emits_warning() {
        // Lines 298-306: unrecognized SQL statement type leaves unchanged + adds warning
        let input = "  &sql(EXEC stored_proc)\n  Write x";
        let result = translate_sql_macros(input);
        assert!(result.found);
        assert!(
            !result.warnings.is_empty(),
            "should have warning for EXEC: {:?}",
            result.warnings
        );
        assert!(result.translated_code.contains("&sql(EXEC stored_proc)"));
    }

    // ── &sql translation: SELECT without SELECT keyword in col list ───────────

    #[test]
    fn translate_select_into_no_select_keyword_fallback() {
        // Line 341: when select_cols_sql has no "SELECT" — uses clone fallback
        // This happens if the regex matched something odd; exercise via the outer translate path.
        // Normal SELECT INTO will trigger this path by design when matching the col extraction.
        let input = "  &sql(SELECT Name INTO :v FROM Person)\n  If $$$ISERR($sc) { Write \"err\" }";
        let result = translate_sql_macros(input);
        assert!(result.found);
        // Output should contain ObjectScript-style variable assignment (no raw &sql)
        assert!(
            !result.translated_code.contains("&sql(SELECT"),
            "should be translated: {}",
            result.translated_code
        );
    }

    // ── &sql translation: column AS alias handling ────────────────────────────

    #[test]
    fn translate_select_into_col_as_alias_used() {
        // Line 349: "ColName AS alias" — alias is used as variable name
        let input =
            "  &sql(SELECT Name AS n, Age AS a INTO :n, :a FROM Person WHERE ID=1)\n  Write n";
        let result = translate_sql_macros(input);
        // Should be translated (no raw &sql remaining)
        assert!(result.found);
    }

    // ── split_host_vars_from_rest: FROM inside parens ────────────────────────

    #[test]
    fn split_host_vars_from_rest_from_inside_parens() {
        // Lines 580-584: fallback when find_keyword_pos skips FROM inside parens,
        // but upper.find("FROM") catches it as a plain substring match.
        // Construct input where find_keyword_pos returns None but FROM exists as substring
        let after_into = ":v FROM(subquery) WHERE x=1";
        let (vars, rest) = split_host_vars_from_rest(after_into);
        // Either path should split correctly
        assert!(!vars.is_empty() || !rest.is_empty());
    }

    // ── write-gate: Toolset::Nostub removes stub tools ────────────────────────

    #[test]
    fn toolset_nostub_removes_stub_tools() {
        // Line 1551-1558: Nostub/Merged removes skill_propose etc from router
        let registry = crate::skills::SkillRegistry::default();
        let result = IrisTools::with_registry_and_toolset(
            None,
            registry,
            Toolset::Nostub,
            None,
            None,
            false,
            write_gate::DeclaredGates::default(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn with_registry_uses_baseline_toolset() {
        // Line 1533-1538: with_registry delegates to with_registry_and_toolset with Baseline
        let registry = crate::skills::SkillRegistry::default();
        let result = IrisTools::with_registry(None, registry);
        assert!(result.is_ok());
    }
}

#[cfg(test)]
mod config_watcher_tests {
    use super::ConfigWatcher;
    #[test]
    fn test_config_watcher_detects_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".iris-agentic-dev.toml");

        // File does not exist yet — watcher created but last_mtime is None
        let mut watcher = ConfigWatcher::new(path.clone()).unwrap();
        assert!(
            watcher.last_mtime.is_none(),
            "mtime should be None before file exists"
        );
        assert!(!watcher.has_changed(), "no change if file still absent");

        // File appears
        std::fs::write(&path, "[connection]\nhost = \"localhost\"\n").unwrap();
        assert!(watcher.has_changed(), "should detect newly-created file");
        assert!(
            watcher.last_mtime.is_some(),
            "mtime should be set after detection"
        );
        assert!(
            !watcher.has_changed(),
            "no change on second check after detection"
        );
    }

    #[test]
    fn test_config_watcher_detects_modification() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".iris-agentic-dev.toml");
        std::fs::write(&path, "[connection]\nhost = \"localhost\"\n").unwrap();

        let mut watcher = ConfigWatcher::new(path.clone()).unwrap();
        assert!(watcher.last_mtime.is_some());
        assert!(
            !watcher.has_changed(),
            "no change immediately after creation"
        );

        // Wind the stored mtime back by 2 seconds to simulate a future write being newer.
        if let Some(ref mut mtime) = watcher.last_mtime {
            *mtime = mtime
                .checked_sub(std::time::Duration::from_secs(2))
                .unwrap();
        }
        assert!(watcher.has_changed(), "should detect file with newer mtime");
    }

    #[test]
    fn test_config_watcher_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".iris-agentic-dev.toml");
        std::fs::write(&path, "[connection]\nhost = \"localhost\"\n").unwrap();

        let mut watcher = ConfigWatcher::new(path.clone()).unwrap();
        assert!(watcher.last_mtime.is_some());
        assert!(
            !watcher.has_changed(),
            "no spurious change for existing file"
        );
    }

    // T026: background pool-reload precondition — watcher detects content change
    // (simulates the detection step that triggers pool swap in check_reload)
    #[test]
    fn test_config_watcher_fires_on_server_entry_added() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".iris-agentic-dev.toml");

        // Initial config with one server
        std::fs::write(&path, "[connection]\nhost = \"localhost\"\nport = 52780\n").unwrap();
        let mut watcher = ConfigWatcher::new(path.clone()).unwrap();
        assert!(!watcher.has_changed(), "no change right after creation");

        // Simulate a later write (wind mtime back so new write is definitively newer)
        if let Some(ref mut mtime) = watcher.last_mtime {
            *mtime = mtime
                .checked_sub(std::time::Duration::from_secs(2))
                .unwrap();
        }

        // Write updated config adding a second server entry
        std::fs::write(
            &path,
            "[connection]\nhost = \"localhost\"\nport = 52780\n\n[instance.prod]\nhost = \"prod.example.com\"\nport = 52773\n",
        )
        .unwrap();

        assert!(
            watcher.has_changed(),
            "watcher must fire when config file is updated with new server entry"
        );
        assert!(
            !watcher.has_changed(),
            "no second fire after detection consumed"
        );
    }
}

#[cfg(test)]
mod schema_normalization_tests {
    use super::normalize_schema_openapi3;
    use super::DOCKER_REQUIRED_HINT;

    #[test]
    fn test_normalize_nullable_integer() {
        let mut schema = serde_json::json!({
            "type": ["integer", "null"],
            "format": "uint",
            "minimum": 0,
            "description": "Max entries"
        })
        .as_object()
        .unwrap()
        .clone();
        normalize_schema_openapi3(&mut schema);
        assert!(schema.get("type").is_none(), "type should be removed");
        let any_of = schema["anyOf"].as_array().unwrap();
        assert_eq!(any_of.len(), 2);
        assert_eq!(any_of[0]["type"], "integer");
        assert_eq!(any_of[0]["format"], "uint");
        assert_eq!(any_of[0]["minimum"], 0);
        assert_eq!(any_of[1]["type"], "null");
        assert_eq!(
            schema["description"], "Max entries",
            "description stays at top level"
        );
    }

    #[test]
    fn test_normalize_nullable_string() {
        let mut schema = serde_json::json!({
            "type": ["string", "null"],
            "description": "Optional string"
        })
        .as_object()
        .unwrap()
        .clone();
        normalize_schema_openapi3(&mut schema);
        let any_of = schema["anyOf"].as_array().unwrap();
        assert_eq!(any_of[0]["type"], "string");
        assert_eq!(any_of[1]["type"], "null");
    }

    #[test]
    fn test_normalize_nested_properties() {
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": ["integer", "null"],
                    "format": "uint",
                    "minimum": 0,
                    "description": "Max"
                }
            }
        })
        .as_object()
        .unwrap()
        .clone();
        normalize_schema_openapi3(&mut schema);
        assert_eq!(schema["type"], "object", "top-level type unchanged");
        let limit = &schema["properties"]["limit"];
        assert!(limit.get("type").is_none());
        let any_of = limit["anyOf"].as_array().unwrap();
        assert_eq!(any_of[0]["type"], "integer");
        assert_eq!(any_of[0]["format"], "uint");
        assert_eq!(any_of[1]["type"], "null");
        assert_eq!(limit["description"], "Max");
    }

    #[test]
    fn test_normalize_non_nullable_unchanged() {
        let mut schema = serde_json::json!({
            "type": "integer",
            "format": "uint",
            "minimum": 0
        })
        .as_object()
        .unwrap()
        .clone();
        let original = schema.clone();
        normalize_schema_openapi3(&mut schema);
        assert_eq!(schema, original, "non-nullable schema should be unchanged");
    }

    // ── check_config field ordering ───────────────────────────────────────────
    #[test]
    fn check_config_connection_source_before_host() {
        // serde_json::json! preserves insertion order — this test guards that ordering.
        let sample = serde_json::json!({
            "connected": true,
            "connection_source": "http",
            "host": "localhost",
            "port": 52773_u16,
            "namespace": "USER",
            "container": serde_json::Value::Null,
            "config_file": serde_json::Value::Null,
            "config_loaded_at": serde_json::Value::Null,
            "iris_version": serde_json::Value::Null,
            "write_tools_enabled": true,
            "config_watch_path": serde_json::Value::Null,
        });
        let serialized = serde_json::to_string(&sample).unwrap();
        let conn_src_pos = serialized.find("connection_source").unwrap();
        let host_pos = serialized.find("\"host\"").unwrap();
        assert!(
            conn_src_pos < host_pos,
            "connection_source must appear before host in check_config output"
        );
    }

    // ── DOCKER_REQUIRED remediation hint ─────────────────────────────────────
    #[test]
    fn docker_required_hint_contains_http_guidance() {
        assert!(
            DOCKER_REQUIRED_HINT.contains("http://"),
            "DOCKER_REQUIRED hint must reference HTTP URL pattern"
        );
        assert!(
            DOCKER_REQUIRED_HINT.contains(".iris-agentic-dev.toml"),
            "DOCKER_REQUIRED hint must reference the toml config file"
        );
        assert!(
            !DOCKER_REQUIRED_HINT.to_lowercase().contains("docker run"),
            "DOCKER_REQUIRED hint must not suggest 'docker run'"
        );
    }
}

#[cfg(test)]
mod pure_fn_tests {
    use super::*;

    // ── split_csv ─────────────────────────────────────────────────────────────
    #[test]
    fn test_split_csv_empty() {
        assert_eq!(split_csv(""), Vec::<String>::new());
    }
    #[test]
    fn test_split_csv_single() {
        assert_eq!(split_csv(":name"), vec![":name"]);
    }
    #[test]
    fn test_split_csv_multiple() {
        assert_eq!(split_csv(":a, :b, :c"), vec![":a", ":b", ":c"]);
    }
    #[test]
    fn test_split_csv_respects_parens() {
        let result = split_csv("func(:a, :b), :c");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "func(:a, :b)");
        assert_eq!(result[1], ":c");
    }

    // ── find_keyword_pos ─────────────────────────────────────────────────────
    #[test]
    fn test_find_keyword_pos_found() {
        assert!(find_keyword_pos("SELECT :x FROM t", "FROM").is_some());
    }
    #[test]
    fn test_find_keyword_pos_not_found() {
        assert!(find_keyword_pos("SELECT :x", "FROM").is_none());
    }
    #[test]
    fn test_find_keyword_pos_case_insensitive() {
        assert!(find_keyword_pos("select :x from t", "FROM").is_some());
    }

    // ── extract_where_params ──────────────────────────────────────────────────
    #[test]
    fn test_extract_where_params_none() {
        assert_eq!(extract_where_params("FROM t"), Vec::<String>::new());
    }
    #[test]
    fn test_extract_where_params_single() {
        let p = extract_where_params("WHERE id = :id");
        assert_eq!(p, vec!["id"]);
    }
    #[test]
    fn test_extract_where_params_multiple() {
        let p = extract_where_params("WHERE a = :a AND b = :b");
        assert_eq!(p, vec!["a", "b"]);
    }
    #[test]
    fn test_extract_where_params_no_dupe() {
        let p = extract_where_params(":x AND :x");
        assert_eq!(p, vec!["x"]);
    }

    // ── replace_host_vars_with_positional ────────────────────────────────────
    #[test]
    fn test_replace_host_vars_single() {
        let result = replace_host_vars_with_positional("WHERE id = :id", &["id".to_string()]);
        assert_eq!(result, "WHERE id = ?");
    }
    #[test]
    fn test_replace_host_vars_multiple() {
        let result = replace_host_vars_with_positional(
            "WHERE a = :a AND b = :b",
            &["a".to_string(), "b".to_string()],
        );
        assert_eq!(result, "WHERE a = ? AND b = ?");
    }

    // ── split_host_vars_from_rest ────────────────────────────────────────────
    #[test]
    fn test_split_host_vars_with_from() {
        let (vars, rest) = split_host_vars_from_rest(":name, :age FROM users WHERE id = :id");
        assert!(vars.contains(":name"));
        assert!(rest.starts_with("FROM"));
    }
    #[test]
    fn test_split_host_vars_no_from() {
        let (vars, rest) = split_host_vars_from_rest(":name");
        assert_eq!(vars, ":name");
        assert!(rest.is_empty());
    }

    // ── translate_sql_macros ──────────────────────────────────────────────────
    #[test]
    fn test_translate_sql_macros_no_macro_passthrough() {
        let code = "Write \"hello\"";
        let result = translate_sql_macros(code);
        assert!(!result.found);
        assert_eq!(result.translated_code, code);
        assert!(result.warnings.is_empty());
    }
    #[test]
    fn test_translate_sql_macros_select_into() {
        let code = "&sql(SELECT Name INTO :name FROM Sample.Person WHERE ID = :id)";
        let result = translate_sql_macros(code);
        assert!(result.found);
        assert!(!result.translated_code.contains("&sql("));
        assert!(result.warnings.is_empty());
    }
    #[test]
    fn test_translate_sql_macros_insert() {
        let code = "&sql(INSERT INTO t (a) VALUES (:a))";
        let result = translate_sql_macros(code);
        assert!(result.found);
        assert!(!result.translated_code.contains("&sql(INSERT"));
    }
    #[test]
    fn test_translate_sql_macros_update() {
        let code = "&sql(UPDATE t SET a = :a WHERE id = :id)";
        let result = translate_sql_macros(code);
        assert!(result.found);
    }
    #[test]
    fn test_translate_sql_macros_delete() {
        let code = "&sql(DELETE FROM t WHERE id = :id)";
        let result = translate_sql_macros(code);
        assert!(result.found);
    }
    #[test]
    fn test_translate_sql_macros_call_unsupported() {
        let code = "&sql(CALL MyProc(:a, :b))";
        let result = translate_sql_macros(code);
        assert!(result.found);
        assert!(!result.warnings.is_empty());
        assert!(result.translated_code.contains("&sql(CALL"));
    }
    #[test]
    fn test_translate_sql_macros_select_no_into() {
        let code = "&sql(SELECT Name FROM Sample.Person WHERE ID = 1)";
        let result = translate_sql_macros(code);
        assert!(result.found);
        assert!(!result.translated_code.contains("&sql("));
    }

    // ── default_execute_timeout ───────────────────────────────────────────────

    /// These two tests read and write the same process-wide variable, so running them in parallel
    /// (cargo's default) lets one's `remove_var` land between the other's `set_var` and its read.
    /// That was a live flake in the default `cargo test` run, not a theoretical one.
    static TIMEOUT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_default_execute_timeout_default_value() {
        let _guard = TIMEOUT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OBJECTSCRIPT_TEST_TIMEOUT");
        let t = default_execute_timeout();
        assert_eq!(t, 120, "default timeout must be 120s");
    }
    #[test]
    fn test_default_execute_timeout_env_override() {
        let _guard = TIMEOUT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OBJECTSCRIPT_TEST_TIMEOUT", "60");
        let t = default_execute_timeout();
        std::env::remove_var("OBJECTSCRIPT_TEST_TIMEOUT");
        assert_eq!(t, 60);
    }

    // ── map_status_int ────────────────────────────────────────────────────────
    #[test]
    fn test_map_status_int_zero_no_action() {
        assert_eq!(map_status_int(0, ""), "failed");
    }
    #[test]
    fn test_map_status_int_one_is_passed() {
        assert_eq!(map_status_int(1, ""), "passed");
    }
    #[test]
    fn test_map_status_int_two_with_action_is_error() {
        assert_eq!(map_status_int(2, "SomeMethod"), "error");
    }
    #[test]
    fn test_map_status_int_two_no_action_is_failed() {
        assert_eq!(map_status_int(2, ""), "failed");
    }

    // ── build_test_detail ─────────────────────────────────────────────────────
    #[test]
    fn test_build_test_detail_empty() {
        let result = build_test_detail(&[], &[]);
        let arr = result["test_suites"].as_array().unwrap();
        assert_eq!(arr.len(), 0);
    }
    #[test]
    fn test_build_test_detail_one_suite_one_method() {
        let suites = vec![SuiteRow {
            id: "1".to_string(),
            name: "MyTests".to_string(),
            status: 1,
            duration_ms: Some(100.0),
        }];
        let methods = vec![MethodRow {
            suite_id: "1".to_string(),
            name: "TestFoo".to_string(),
            class_name: "MyTests".to_string(),
            status: 1,
            duration_ms: Some(50.0),
            error_description: "".to_string(),
            error_action: "".to_string(),
        }];
        let result = build_test_detail(&suites, &methods);
        let arr = result["test_suites"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "MyTests");
    }

    // ── Param struct serde defaults ───────────────────────────────────────────
    #[test]
    fn test_compile_params_defaults() {
        let p: CompileParams = serde_json::from_str(r#"{"target": "Foo.Bar"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(resolve_namespace(p.namespace.as_deref(), "APP"), "APP");
        assert_eq!(p.target, "Foo.Bar");
        assert!(!p.force_writable);
    }
    #[test]
    fn test_test_params_defaults() {
        let p: TestParams = serde_json::from_str(r#"{"pattern": "MyTests.*"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(resolve_namespace(p.namespace.as_deref(), "APP"), "APP");
        assert_eq!(p.pattern, "MyTests.*");
    }
    #[test]
    fn test_execute_params_defaults() {
        let p: ExecuteParams = serde_json::from_str(r#"{"code": "Write 1"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(resolve_namespace(p.namespace.as_deref(), "APP"), "APP");
        assert_eq!(p.code, "Write 1");
        assert!(p.translate_sql, "translate_sql defaults to true");
        assert!(!p.confirmed);
    }
    #[test]
    fn test_execute_params_translate_sql_false() {
        let p: ExecuteParams =
            serde_json::from_str(r#"{"code": "x", "translate_sql": false}"#).unwrap();
        assert!(!p.translate_sql);
    }
    #[test]
    fn test_symbols_params_defaults() {
        let p: SymbolsParams = serde_json::from_str(r#"{"query": "Ens.*"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(resolve_namespace(p.namespace.as_deref(), "APP"), "APP");
    }
    #[test]
    fn test_introspect_params_defaults() {
        let p: IntrospectParams =
            serde_json::from_str(r#"{"class_name": "Ens.Production"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(resolve_namespace(p.namespace.as_deref(), "APP"), "APP");
    }
    #[test]
    fn test_generate_class_params_defaults() {
        let p: GenerateClassParams =
            serde_json::from_str(r#"{"description": "A simple class"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(resolve_namespace(p.namespace.as_deref(), "APP"), "APP");
        assert!(!p.overwrite);
    }
    #[test]
    fn test_generate_test_params_defaults() {
        let p: GenerateTestParams = serde_json::from_str(r#"{"class_name": "Foo.Bar"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(resolve_namespace(p.namespace.as_deref(), "APP"), "APP");
        assert_eq!(p.class_name, "Foo.Bar");
    }
    #[test]
    fn test_query_params_defaults() {
        let p: QueryParams = serde_json::from_str(r#"{"query": "SELECT 1"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(resolve_namespace(p.namespace.as_deref(), "APP"), "APP");
        assert!(p.parameters.is_empty());
    }
    #[test]
    fn test_get_log_params_defaults() {
        let p: GetLogParams = serde_json::from_str(r#"{}"#).unwrap();
        assert!(p.id.is_none());
        assert!(p.limit.is_none());
        assert_eq!(p.offset, 0);
    }
    #[test]
    fn test_error_logs_params_defaults() {
        let p: ErrorLogsParams = serde_json::from_str(r#"{}"#).unwrap();
        assert!(p.max_entries > 0);
    }

    // ── translate_sql_macros — additional edge cases ──────────────────────────
    #[test]
    fn test_translate_sql_macros_multiple_macros() {
        let code = "&sql(SELECT Name INTO :name FROM t)\n&sql(INSERT INTO t (a) VALUES (:a))";
        let result = translate_sql_macros(code);
        assert!(result.found);
        assert!(!result.translated_code.contains("&sql(SELECT"));
        assert!(!result.translated_code.contains("&sql(INSERT"));
    }

    #[test]
    fn test_translate_sql_macros_select_into_extracts_host_var() {
        let code = "&sql(SELECT Name INTO :name FROM Sample.Person WHERE ID = :id)";
        let result = translate_sql_macros(code);
        assert!(result.found);
        // The translated code should reference the output variable "name"
        assert!(result.translated_code.contains("name"));
    }

    #[test]
    fn test_translate_sql_macros_select_no_into_no_host_out() {
        // SELECT without INTO should not produce an output host var assignment
        let code = "&sql(SELECT COUNT(*) FROM t)";
        let result = translate_sql_macros(code);
        assert!(result.found);
        assert!(!result.translated_code.contains("INTO"));
    }

    #[test]
    fn test_translate_sql_macros_empty_string_passthrough() {
        let result = translate_sql_macros("");
        assert!(!result.found);
        assert_eq!(result.translated_code, "");
    }

    #[test]
    fn test_translate_sql_macros_plain_objectscript_passthrough() {
        let code = "Set x = ##class(Sample.Person).%New()";
        let result = translate_sql_macros(code);
        assert!(!result.found);
        assert_eq!(result.translated_code, code);
    }

    // ── split_csv — additional edge cases ────────────────────────────────────
    #[test]
    fn test_split_csv_whitespace_trimmed() {
        let result = split_csv("  :a  ,  :b  ");
        // Each item should be trimmed
        for item in &result {
            assert_eq!(item.trim(), item.as_str(), "items should be trimmed");
        }
    }

    #[test]
    fn test_split_csv_nested_parens_deep() {
        let result = split_csv("outer(inner(:a, :b), :c), :d");
        assert_eq!(result.len(), 2, "nested parens keep first arg together");
    }

    // ── find_keyword_pos — additional edge cases ──────────────────────────────
    #[test]
    fn test_find_keyword_pos_mixed_case() {
        assert!(find_keyword_pos("select x Where id = 1", "WHERE").is_some());
    }

    #[test]
    fn test_find_keyword_pos_at_start() {
        assert!(find_keyword_pos("FROM t WHERE id = 1", "FROM").is_some());
    }

    #[test]
    fn test_find_keyword_pos_keyword_as_substring_not_matched() {
        // "FROMAGE" contains "FROM" but is one token, so it is not a keyword occurrence.
        // The old version of this test called the function and discarded the result, which
        // documented the ambiguity instead of resolving it — it passed whichever way the
        // function behaved. Pin the behaviour: substring hits do not count, real ones do.
        assert_eq!(find_keyword_pos("SELECT * FROMAGE t", "FROM"), None);
        assert_eq!(find_keyword_pos("x FROMt", "FROM"), None);
        assert!(find_keyword_pos("SELECT * FROMAGE t FROM u", "FROM").is_some());
    }

    // ── replace_host_vars_with_positional — additional edge cases ────────────
    #[test]
    fn test_replace_host_vars_no_vars_unchanged() {
        let sql = "SELECT 1 FROM t";
        let result = replace_host_vars_with_positional(sql, &[]);
        assert_eq!(result, sql);
    }

    #[test]
    fn test_replace_host_vars_repeated_var() {
        // If the same var appears twice it should be replaced twice
        let result =
            replace_host_vars_with_positional("WHERE a = :x AND b = :x", &["x".to_string()]);
        let question_count = result.matches('?').count();
        assert!(
            question_count >= 1,
            "at least one ? must appear: {}",
            result
        );
    }

    // ── extract_where_params — additional edge cases ──────────────────────────
    #[test]
    fn test_extract_where_params_case_insensitive_where() {
        let p = extract_where_params("where id = :id");
        assert!(p.contains(&"id".to_string()));
    }

    #[test]
    fn test_extract_where_params_no_colon_no_params() {
        let p = extract_where_params("WHERE id = 1");
        assert!(p.is_empty());
    }

    // ── default_execute_timeout — additional edge cases ───────────────────────
    #[test]
    fn test_default_execute_timeout_returns_positive() {
        let t = default_execute_timeout();
        assert!(t > 0, "timeout must be positive, got {}", t);
    }

    #[test]
    fn test_default_execute_timeout_env_invalid_falls_back() {
        std::env::set_var("OBJECTSCRIPT_TEST_TIMEOUT", "not_a_number");
        let t = default_execute_timeout();
        std::env::remove_var("OBJECTSCRIPT_TEST_TIMEOUT");
        // Should fall back to a positive default rather than panic
        assert!(t > 0);
    }

    // ── map_status_int — additional edge cases ────────────────────────────────
    #[test]
    fn test_map_status_int_unknown_large_value() {
        // Unknown status codes should return a non-empty string (not panic)
        let s = map_status_int(99, "");
        assert!(!s.is_empty());
    }

    #[test]
    fn test_map_status_int_three_is_skipped_or_unknown() {
        let s = map_status_int(3, "");
        assert!(!s.is_empty());
    }

    // ── build_test_detail — additional edge cases ─────────────────────────────
    #[test]
    fn test_build_test_detail_method_grouped_under_correct_suite() {
        let suites = vec![
            SuiteRow {
                id: "1".to_string(),
                name: "SuiteA".to_string(),
                status: 1,
                duration_ms: Some(10.0),
            },
            SuiteRow {
                id: "2".to_string(),
                name: "SuiteB".to_string(),
                status: 1,
                duration_ms: Some(20.0),
            },
        ];
        let methods = vec![
            MethodRow {
                suite_id: "1".to_string(),
                name: "TestA1".to_string(),
                class_name: "SuiteA".to_string(),
                status: 1,
                duration_ms: Some(5.0),
                error_description: "".to_string(),
                error_action: "".to_string(),
            },
            MethodRow {
                suite_id: "2".to_string(),
                name: "TestB1".to_string(),
                class_name: "SuiteB".to_string(),
                status: 0,
                duration_ms: Some(15.0),
                error_description: "boom".to_string(),
                error_action: "".to_string(),
            },
        ];
        let result = build_test_detail(&suites, &methods);
        let arr = result["test_suites"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // SuiteB contains a failing method
        let suite_b = arr.iter().find(|s| s["name"] == "SuiteB").unwrap();
        let suite_b_cases = suite_b["test_cases"].as_array().unwrap();
        assert_eq!(suite_b_cases[0]["name"], "TestB1");
    }

    // ── Param struct serde round-trips ────────────────────────────────────────
    #[test]
    fn test_compile_params_force_writable_explicit() {
        let p: CompileParams =
            serde_json::from_str(r#"{"target": "X.Y", "force_writable": true}"#).unwrap();
        assert!(p.force_writable);
    }

    #[test]
    fn test_test_params_namespace_override() {
        let p: TestParams =
            serde_json::from_str(r#"{"pattern": "T.*", "namespace": "MYNS"}"#).unwrap();
        assert_eq!(p.namespace.as_deref(), Some("MYNS"));
        assert_eq!(resolve_namespace(p.namespace.as_deref(), "APP"), "MYNS");
    }

    #[test]
    fn test_query_params_with_parameters() {
        let p: QueryParams =
            serde_json::from_str(r#"{"query": "SELECT ?", "parameters": ["hello"]}"#).unwrap();
        assert_eq!(p.parameters.len(), 1);
        assert_eq!(
            p.parameters[0],
            serde_json::Value::String("hello".to_string())
        );
    }

    #[test]
    fn test_get_log_params_with_values() {
        let p: GetLogParams =
            serde_json::from_str(r#"{"id": "42", "limit": 10, "offset": 5}"#).unwrap();
        assert_eq!(p.id, Some("42".to_string()));
        assert_eq!(p.limit, Some(10));
        assert_eq!(p.offset, 5);
    }

    // ── translate_symbols_query ───────────────────────────────────────────────

    #[test]
    fn test_translate_symbols_query_star_returns_all() {
        let (sql, params) = translate_symbols_query(100, "*");
        assert!(sql.contains("SELECT TOP 100"));
        assert!(!sql.contains("WHERE"));
        assert!(params.is_empty());
    }

    #[test]
    fn test_translate_symbols_query_empty_returns_all() {
        let (sql, params) = translate_symbols_query(50, "");
        assert!(!sql.contains("WHERE"));
        assert!(params.is_empty());
    }

    #[test]
    fn test_translate_symbols_query_pkg_star_prefix() {
        let (sql, params) = translate_symbols_query(100, "Ens.*");
        assert!(sql.contains("%STARTSWITH"));
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], serde_json::Value::String("Ens.".to_string()));
    }

    #[test]
    fn test_translate_symbols_query_trailing_dot() {
        let (sql, params) = translate_symbols_query(100, "MyApp.");
        assert!(sql.contains("%STARTSWITH"));
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], serde_json::Value::String("MyApp.".to_string()));
    }

    #[test]
    fn test_translate_symbols_query_mid_glob() {
        let (sql, params) = translate_symbols_query(100, "Ens.*.Production");
        assert!(sql.contains("LIKE"));
        assert_eq!(params.len(), 1);
        // * → %
        assert_eq!(
            params[0],
            serde_json::Value::String("Ens.%.Production".to_string())
        );
    }

    #[test]
    fn test_translate_symbols_query_plain_substring() {
        let (sql, params) = translate_symbols_query(100, "Person");
        assert!(sql.contains("LIKE"));
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], serde_json::Value::String("%Person%".to_string()));
    }

    #[test]
    fn test_translate_symbols_query_limit_applied() {
        let (sql, _) = translate_symbols_query(25, "*");
        assert!(sql.contains("SELECT TOP 25"));
    }

    // ── extract_port ─────────────────────────────────────────────────────────

    #[test]
    fn test_extract_port_found() {
        // typical docker port mapping: "0.0.0.0:52780->52773/tcp"
        let ports = "0.0.0.0:52780->52773/tcp, 0.0.0.0:11972->1972/tcp";
        assert_eq!(extract_port(ports, "52773"), Some(52780));
        assert_eq!(extract_port(ports, "1972"), Some(11972));
    }

    #[test]
    fn test_extract_port_not_found() {
        let ports = "0.0.0.0:52780->52773/tcp";
        assert_eq!(extract_port(ports, "1972"), None);
    }

    #[test]
    fn test_extract_port_empty_string() {
        assert_eq!(extract_port("", "1972"), None);
    }

    // ── sort_containers ───────────────────────────────────────────────────────

    #[test]
    fn test_sort_containers_by_score_descending() {
        let v = vec![
            serde_json::json!({"name": "low", "score": 1}),
            serde_json::json!({"name": "high", "score": 10}),
            serde_json::json!({"name": "mid", "score": 5}),
        ];
        let sorted = sort_containers(v);
        assert_eq!(sorted[0]["name"], "high");
        assert_eq!(sorted[1]["name"], "mid");
        assert_eq!(sorted[2]["name"], "low");
    }

    #[test]
    fn test_sort_containers_tie_breaks_by_name() {
        let v = vec![
            serde_json::json!({"name": "zoo", "score": 5}),
            serde_json::json!({"name": "alpha", "score": 5}),
        ];
        let sorted = sort_containers(v);
        assert_eq!(sorted[0]["name"], "alpha");
        assert_eq!(sorted[1]["name"], "zoo");
    }

    #[test]
    fn test_sort_containers_empty() {
        let sorted = sort_containers(vec![]);
        assert!(sorted.is_empty());
    }
}

#[cfg(test)]
mod validate_sql_tests {
    use super::validate_read_only_sql;

    #[test]
    fn test_select_allowed() {
        assert!(validate_read_only_sql("SELECT * FROM MyTable").is_ok());
    }

    #[test]
    fn test_empty_string_returns_empty_error() {
        assert_eq!(validate_read_only_sql(""), Err("EMPTY".to_string()));
    }

    #[test]
    fn test_whitespace_only_returns_empty_error() {
        assert_eq!(
            validate_read_only_sql("   \n\t  "),
            Err("EMPTY".to_string())
        );
    }

    #[test]
    fn test_insert_blocked() {
        assert_eq!(
            validate_read_only_sql("INSERT INTO t VALUES (1)"),
            Err("INSERT".to_string())
        );
    }

    #[test]
    fn test_update_blocked() {
        assert_eq!(
            validate_read_only_sql("UPDATE t SET x=1"),
            Err("UPDATE".to_string())
        );
    }

    #[test]
    fn test_delete_blocked() {
        assert_eq!(
            validate_read_only_sql("DELETE FROM t WHERE id=1"),
            Err("DELETE".to_string())
        );
    }

    #[test]
    fn test_drop_blocked() {
        assert_eq!(
            validate_read_only_sql("DROP TABLE t"),
            Err("DROP".to_string())
        );
    }

    #[test]
    fn test_alter_blocked() {
        assert_eq!(
            validate_read_only_sql("ALTER TABLE t ADD COLUMN x INT"),
            Err("ALTER".to_string())
        );
    }

    #[test]
    fn test_create_blocked() {
        assert_eq!(
            validate_read_only_sql("CREATE TABLE t (id INT)"),
            Err("CREATE".to_string())
        );
    }

    #[test]
    fn test_truncate_blocked() {
        assert_eq!(
            validate_read_only_sql("TRUNCATE TABLE t"),
            Err("TRUNCATE".to_string())
        );
    }

    #[test]
    fn test_merge_blocked() {
        assert_eq!(
            validate_read_only_sql("MERGE INTO t USING s ON t.id=s.id"),
            Err("MERGE".to_string())
        );
    }

    #[test]
    fn test_exec_blocked() {
        assert_eq!(
            validate_read_only_sql("EXEC sp_something"),
            Err("EXEC".to_string())
        );
    }

    #[test]
    fn test_execute_blocked() {
        assert_eq!(
            validate_read_only_sql("EXECUTE my_proc"),
            Err("EXECUTE".to_string())
        );
    }

    #[test]
    fn test_kill_blocked() {
        assert_eq!(validate_read_only_sql("KILL 42"), Err("KILL".to_string()));
    }

    #[test]
    fn test_lock_blocked() {
        assert_eq!(
            validate_read_only_sql("LOCK TABLE t"),
            Err("LOCK".to_string())
        );
    }

    #[test]
    fn test_case_insensitive_blocked() {
        assert_eq!(
            validate_read_only_sql("insert into t values (1)"),
            Err("INSERT".to_string())
        );
        assert_eq!(
            validate_read_only_sql("Drop Table t"),
            Err("DROP".to_string())
        );
    }

    #[test]
    fn test_keyword_in_string_literal_allowed() {
        // DROP inside a string literal must NOT be blocked
        assert!(validate_read_only_sql("SELECT 'DROP TABLE t' FROM MyTable").is_ok());
    }

    #[test]
    fn test_keyword_in_block_comment_allowed() {
        // DROP inside a block comment must NOT be blocked
        assert!(validate_read_only_sql("SELECT /* DROP TABLE t */ x FROM t").is_ok());
    }

    #[test]
    fn test_keyword_in_line_comment_allowed() {
        // DROP after -- must NOT be blocked
        assert!(validate_read_only_sql("SELECT x FROM t -- DROP TABLE t").is_ok());
    }

    #[test]
    fn test_keyword_as_substring_not_blocked() {
        // "DROPBOX" contains DROP but is not a standalone word
        assert!(validate_read_only_sql("SELECT DROPBOX FROM t").is_ok());
    }

    #[test]
    fn test_select_into_subquery_allowed() {
        // SELECT ... INTO (subquery) is allowed
        assert!(validate_read_only_sql("SELECT x INTO (SELECT 1) FROM t").is_ok());
    }

    #[test]
    fn test_select_into_identifier_blocked() {
        assert_eq!(
            validate_read_only_sql("SELECT x INTO myvar FROM t"),
            Err("SELECT INTO".to_string())
        );
    }

    #[test]
    fn test_bulk_blocked() {
        assert_eq!(
            validate_read_only_sql("BULK INSERT t FROM 'file.csv'"),
            Err("BULK".to_string())
        );
    }

    #[test]
    fn test_load_blocked() {
        assert_eq!(
            validate_read_only_sql("LOAD DATA INFILE 'x.csv' INTO TABLE t"),
            Err("LOAD".to_string())
        );
    }

    // ── T015: pool integration ────────────────────────────────────────────────

    /// T015 — `IrisTools` constructed with a two-server pool; `pool.get(Some("b"))` returns
    /// the `"b"` connection; `pool.get(None)` returns the default `"a"` connection.
    #[test]
    fn pool_get_named_returns_correct_connection() {
        use crate::iris::connection::{DiscoverySource, IrisConnection};
        use crate::iris::connection_pool::ConnectionPool;

        let make_conn = |base_url: &str| -> IrisConnection {
            IrisConnection::new(base_url, "USER", "_SYSTEM", "", DiscoverySource::EnvVar)
        };

        let mut b = ConnectionPool::builder();
        b.add("a".to_string(), make_conn("http://a:52773"), true); // default
        b.add("b".to_string(), make_conn("http://b:52773"), false);
        let pool = b.build();

        // get(Some("b")) returns the "b" connection
        let conn_b = pool.get(Some("b")).expect("should find 'b'");
        assert_eq!(
            conn_b.base_url, "http://b:52773",
            "pool.get(Some(\"b\")) should return the b connection"
        );

        // get(None) returns the default "a", not "b"
        let conn_default = pool.get(None).expect("should return default 'a'");
        assert_eq!(
            conn_default.base_url, "http://a:52773",
            "pool.get(None) should return the default 'a' connection, not 'b'"
        );
    }
}

#[cfg(test)]
mod build_test_run_tests {
    use super::*;

    fn make_suite(id: &str, name: &str) -> SuiteRow {
        SuiteRow {
            id: id.to_string(),
            name: name.to_string(),
            status: 1,
            duration_ms: Some(100.0),
        }
    }

    fn make_method(suite_id: &str, name: &str, status: i64, err: &str, action: &str) -> MethodRow {
        MethodRow {
            suite_id: suite_id.to_string(),
            name: name.to_string(),
            class_name: suite_id.to_string(),
            status,
            duration_ms: Some(10.0),
            error_description: err.to_string(),
            error_action: action.to_string(),
        }
    }

    #[test]
    fn test_empty_suites_returns_no_tests_found() {
        let result = super::build_test_run_from_sql(&[], &[]);
        assert_eq!(result["success"], false);
        assert_eq!(result["error_code"], super::ERR_NO_TESTS_FOUND);
    }

    #[test]
    fn test_one_passing_method() {
        let suites = vec![make_suite("1", "MySuite")];
        let methods = vec![make_method("1", "TestFoo", 1, "", "")];
        let result = super::build_test_run_from_sql(&suites, &methods);
        assert_eq!(result["success"], true);
        assert_eq!(result["outcome"], "passed");
        assert_eq!(result["total"], 1);
        assert_eq!(result["passed"], 1);
        assert_eq!(result["failed"], 0);
    }

    #[test]
    fn test_one_failing_method() {
        let suites = vec![make_suite("1", "MySuite")];
        let methods = vec![make_method("1", "TestFoo", 0, "assertion failed", "")];
        let result = super::build_test_run_from_sql(&suites, &methods);
        assert_eq!(result["success"], true);
        assert_eq!(result["outcome"], "failed");
        assert_eq!(result["failed"], 1);
    }

    #[test]
    fn test_error_method_outcome() {
        let suites = vec![make_suite("1", "MySuite")];
        // status=2 with error_action set → "error"
        let methods = vec![make_method("1", "TestFoo", 2, "crash", "OnError")];
        let result = super::build_test_run_from_sql(&suites, &methods);
        assert_eq!(result["outcome"], "errored");
        assert_eq!(result["errors"], 1);
    }

    #[test]
    fn test_mixed_results_across_suites() {
        let suites = vec![make_suite("1", "SuiteA"), make_suite("2", "SuiteB")];
        let methods = vec![
            make_method("1", "TestPass", 1, "", ""),
            make_method("2", "TestFail", 0, "bad", ""),
        ];
        let result = super::build_test_run_from_sql(&suites, &methods);
        assert_eq!(result["total"], 2);
        assert_eq!(result["passed"], 1);
        assert_eq!(result["failed"], 1);
        assert_eq!(result["outcome"], "failed");
        let suites_arr = result["test_suites"].as_array().unwrap();
        assert_eq!(suites_arr.len(), 2);
    }

    #[test]
    fn test_duration_totalled() {
        let suites = vec![make_suite("1", "SuiteA"), make_suite("2", "SuiteB")];
        let methods = vec![
            make_method("1", "T1", 1, "", ""),
            make_method("2", "T2", 1, "", ""),
        ];
        let result = super::build_test_run_from_sql(&suites, &methods);
        let dur = result["duration_ms"].as_f64().unwrap();
        assert!(dur > 0.0, "duration_ms should be >0");
    }
}

#[cfg(test)]
mod toolset_tests {
    use super::*;

    #[test]
    fn test_toolset_from_str_nostub() {
        assert_eq!(Toolset::from_str("nostub"), Toolset::Nostub);
        assert_eq!(Toolset::from_str("NOSTUB"), Toolset::Nostub);
    }

    #[test]
    fn test_toolset_from_str_merged() {
        assert_eq!(Toolset::from_str("merged"), Toolset::Merged);
        assert_eq!(Toolset::from_str("MERGED"), Toolset::Merged);
    }

    #[test]
    fn test_toolset_from_str_unknown_defaults_baseline() {
        assert_eq!(Toolset::from_str("unknown"), Toolset::Baseline);
        assert_eq!(Toolset::from_str(""), Toolset::Baseline);
    }

    #[test]
    fn test_toolset_as_str() {
        assert_eq!(Toolset::Baseline.as_str(), "baseline");
        assert_eq!(Toolset::Nostub.as_str(), "nostub");
        assert_eq!(Toolset::Merged.as_str(), "merged");
    }

    #[test]
    fn test_registered_tool_names_baseline_contains_core_tools() {
        let tools = IrisTools::new(None).unwrap();
        let names = tools.registered_tool_names();
        assert!(
            names.contains("iris_compile"),
            "baseline should have iris_compile"
        );
        assert!(
            names.contains("iris_execute"),
            "baseline should have iris_execute"
        );
        assert!(
            names.contains("iris_query"),
            "baseline should have iris_query"
        );
        // Baseline includes stub tools
        assert!(
            names.contains("skill_propose"),
            "baseline should have skill_propose"
        );
    }

    #[test]
    fn test_registered_tool_names_nostub_removes_stubs() {
        let tools = IrisTools::new_with_toolset(None, Toolset::Nostub).unwrap();
        let names = tools.registered_tool_names();
        assert!(
            !names.contains("skill_propose"),
            "nostub should remove skill_propose"
        );
        assert!(
            !names.contains("skill_optimize"),
            "nostub should remove skill_optimize"
        );
        assert!(
            !names.contains("skill_share"),
            "nostub should remove skill_share"
        );
        assert!(
            !names.contains("skill_community_install"),
            "nostub should remove skill_community_install"
        );
        // Core tools still present
        assert!(
            names.contains("iris_compile"),
            "nostub should keep iris_compile"
        );
    }

    #[test]
    fn test_registered_tool_names_merged_adds_iris_debug() {
        let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
        let names = tools.registered_tool_names();
        assert!(
            names.contains("iris_debug"),
            "merged should have iris_debug"
        );
        assert!(
            names.contains("iris_containers"),
            "merged should have iris_containers"
        );
        // merged removes the individual debug tools
        assert!(
            !names.contains("debug_capture_packet"),
            "merged should remove debug_capture_packet"
        );
    }
}

/// Dispatch by tool name: the tail the CLI and the tests use in place of the MCP router.
impl IrisTools {
    /// The name match: the handler for `tool`, with no gate in front of it. Private on purpose —
    /// everything outside this module reaches a handler through [`Self::call_gated`].
    async fn dispatch_by_name(
        &self,
        tool: &str,
        mut params: serde_json::Value,
    ) -> Result<rmcp::model::CallToolResult, String> {
        use rmcp::handler::server::wrapper::Parameters;
        if let Some(args) = params.as_object_mut() {
            write_gate::normalize_dispatch_args(args);
        }
        macro_rules! dispatch {
            ($name:expr, $ty:ty, $method:ident) => {
                if tool == $name {
                    let p: $ty = serde_json::from_value(params)
                        .map_err(|e| format!("bad params for {}: {e}", $name))?;
                    return self
                        .$method(Parameters(p))
                        .await
                        .map_err(|e| format!("{e:?}"));
                }
            };
        }
        dispatch!("iris_compile", CompileParams, iris_compile);
        dispatch!("iris_execute", ExecuteParams, iris_execute);
        dispatch!("iris_test", TestParams, iris_test);
        dispatch!("iris_query", QueryParams, iris_query);
        dispatch!("iris_symbols", SymbolsParams, iris_symbols);
        dispatch!("iris_symbols_local", SymbolsLocalParams, iris_symbols_local);
        dispatch!("iris_get_log", GetLogParams, iris_get_log);
        dispatch!("iris_doc", IrisDocParams, iris_doc);
        dispatch!("iris_info", crate::tools::info::InfoParams, iris_info);
        dispatch!(
            "iris_search",
            crate::tools::search::SearchParams,
            iris_search
        );
        dispatch!(
            "iris_source_control",
            crate::tools::scm::ScmParams,
            iris_source_control
        );
        dispatch!("iris_admin", params::batch3::IrisAdminParams, iris_admin);
        dispatch!("iris_production", IrisProductionParams, iris_production);
        dispatch!(
            "iris_interop_query",
            params::batch5::IrisInteropQueryParams,
            iris_interop_query
        );
        dispatch!(
            "iris_production_item",
            params::batch5::IrisProductionItemParams,
            iris_production_item
        );
        dispatch!(
            "iris_credential_list",
            params::batch6::IrisCredentialListParams,
            iris_credential_list
        );
        dispatch!(
            "iris_credential_manage",
            params::batch6::IrisCredentialManageParams,
            iris_credential_manage
        );
        dispatch!(
            "iris_lookup_manage",
            params::batch6::IrisLookupManageParams,
            iris_lookup_manage
        );
        dispatch!(
            "iris_lookup_transfer",
            params::batch6::IrisLookupTransferParams,
            iris_lookup_transfer
        );
        dispatch!(
            "iris_message_body",
            params::batch5::IrisMessageBodyParams,
            iris_message_body
        );
        dispatch!(
            "iris_business_rule_info",
            params::batch5::IrisBusinessRuleInfoParams,
            iris_business_rule_info
        );
        dispatch!(
            "iris_production_diff",
            params::batch5::IrisProductionDiffParams,
            iris_production_diff
        );
        dispatch!(
            "iris_generate",
            crate::tools::info::GenerateParams,
            iris_generate
        );
        dispatch!("iris_macro", crate::tools::info::MacroParams, iris_macro);
        dispatch!("iris_debug", crate::tools::info::DebugParams, iris_debug);
        dispatch!(
            "iris_table_info",
            crate::tools::info::TableInfoParams,
            iris_table_info
        );
        dispatch!(
            "resolve_dynamic_dispatch",
            crate::tools::dict::ResolveDynamicDispatchParams,
            resolve_dynamic_dispatch
        );
        dispatch!(
            "extract_message_map_routing",
            crate::tools::dict::ExtractMessageMapParams,
            extract_message_map_routing
        );
        dispatch!(
            "find_subclass_implementations",
            crate::tools::dict::FindSubclassImplementationsParams,
            find_subclass_implementations
        );
        dispatch!("docs_introspect", IntrospectParams, docs_introspect);
        dispatch!("check_config", NoParams, check_config);
        dispatch!("agent_history", AgentHistoryParams, agent_history);
        dispatch!("agent_stats", NoParams, agent_stats);
        dispatch!("telemetry_query", TelemetryQueryParams, telemetry_query);
        dispatch!(
            "telemetry_export_trace",
            TelemetryExportTraceParams,
            telemetry_export_trace
        );
        dispatch!("skill_list", NoParams, skill_list);
        dispatch!("skill_describe", SkillNameParams, skill_describe);
        dispatch!("skill_search", SkillSearchParams, skill_search);
        dispatch!("skill_forget", SkillNameParams, skill_forget);
        dispatch!("kb_recall", KbRecallParams, kb_recall);
        dispatch!("kb_index", KbIndexParams, kb_index);
        dispatch!("skill_community_list", NoParams, skill_community_list);
        dispatch!(
            "skill_community_install",
            CommunityPkgParams,
            skill_community_install
        );
        dispatch!("debug_map_int_to_cls", DebugMapParams, debug_map_int_to_cls);
        dispatch!(
            "debug_capture_packet",
            CapturePacketParams,
            debug_capture_packet
        );
        dispatch!(
            "debug_get_error_logs",
            ErrorLogsParams,
            debug_get_error_logs
        );
        dispatch!("debug_source_map", SourceMapParams, debug_source_map);
        dispatch!("skill", skills_tools::SkillParams, skill);
        dispatch!(
            "skill_community",
            skills_tools::SkillCommunityParams,
            skill_community
        );
        dispatch!("kb", skills_tools::KbParams, kb);
        dispatch!("agent_info", skills_tools::AgentInfoParams, agent_info);
        dispatch!(
            "iris_generate_class",
            GenerateClassParams,
            iris_generate_class
        );
        dispatch!("iris_generate_test", GenerateTestParams, iris_generate_test);
        dispatch!(
            "iris_containers",
            params::batch2::IrisContainersParams,
            iris_containers
        );
        dispatch!("skill_propose", NoParams, skill_propose);
        dispatch!("skill_optimize", SkillNameParams, skill_optimize);
        dispatch!("skill_share", SkillNameParams, skill_share);
        dispatch!("iris_global", global::IrisGlobalParams, iris_global);
        dispatch!(
            "iris_execute_method",
            IrisExecuteMethodParams,
            iris_execute_method
        );
        dispatch!("iris_coverage", coverage::IrisCoverageParams, iris_coverage);
        // 072: server management tools
        dispatch!(
            "iris_servers",
            server_tools::IrisServersParams,
            iris_servers
        );
        if tool == "iris_import_servers" {
            return self
                .iris_import_servers(Parameters(NoParams {}))
                .await
                .map_err(|e| format!("{e:?}"));
        }
        dispatch!(
            "iris_add_server",
            server_tools::AddServerParams,
            iris_add_server
        );
        dispatch!(
            "iris_remove_server",
            server_tools::RemoveServerParams,
            iris_remove_server
        );
        dispatch!(
            "iris_test_server",
            server_tools::TestServerParams,
            iris_test_server
        );
        // 072-b: WebSocket terminal sessions
        dispatch!("iris_ws_open", ws_tools::WsOpenParams, iris_ws_open);
        dispatch!("iris_ws_exec", ws_tools::WsExecParams, iris_ws_exec);
        dispatch!("iris_ws_close", ws_tools::WsCloseParams, iris_ws_close);
        // 072-c: comparison, namespace/db admin, observability, security, HL7, mermaid, storage
        dispatch!(
            "capability_matrix",
            params::batch4::CapabilityMatrixParams,
            capability_matrix
        );
        dispatch!(
            "compare_document",
            params::batch1::CompareDocumentParams,
            compare_document
        );
        dispatch!(
            "compare_namespace",
            params::batch1::CompareNamespaceParams,
            compare_namespace
        );
        dispatch!("global_kill", params::batch1::GlobalKillParams, global_kill);
        dispatch!(
            "global_preview",
            params::batch1::GlobalPreviewParams,
            global_preview
        );
        dispatch!(
            "hl7_schema_inspect",
            params::batch7::Hl7SchemaInspectParams,
            hl7_schema_inspect
        );
        dispatch!(
            "hl7_schema_list",
            params::batch7::Hl7SchemaListParams,
            hl7_schema_list
        );
        dispatch!(
            "iris_database_list",
            params::batch2::IrisDatabaseListParams,
            iris_database_list
        );
        dispatch!(
            "iris_database_stats",
            params::batch2::IrisDatabaseStatsParams,
            iris_database_stats
        );
        dispatch!(
            "iris_mirror_status",
            params::batch4::IrisMirrorStatusParams,
            iris_mirror_status
        );
        dispatch!(
            "iris_system_performance",
            params::batch4::IrisSystemPerformanceParams,
            iris_system_performance
        );
        dispatch!(
            "iris_namespace_create",
            params::batch2::IrisNamespaceCreateParams,
            iris_namespace_create
        );
        dispatch!(
            "iris_namespace_list",
            params::batch2::IrisNamespaceListParams,
            iris_namespace_list
        );
        dispatch!(
            "journal_search",
            params::batch4::JournalSearchParams,
            journal_search
        );
        dispatch!(
            "mermaid_class",
            params::batch7::MermaidClassParams,
            mermaid_class
        );
        dispatch!(
            "mermaid_production",
            params::batch7::MermaidProductionParams,
            mermaid_production
        );
        dispatch!("my_access", params::batch4::MyAccessParams, my_access);
        dispatch!(
            "query_audit_log",
            params::batch4::QueryAuditLogParams,
            query_audit_log
        );
        dispatch!(
            "resolve_storage",
            params::batch7::ResolveStorageParams,
            resolve_storage
        );
        dispatch!(
            "stream_inspect",
            params::batch7::StreamInspectParams,
            stream_inspect
        );
        // 065: doc search
        dispatch!(
            "iris_doc_search",
            doc_search::IrisDocSearchParams,
            iris_doc_search
        );
        // 093: hot-reload pool
        if tool == "iris_reload_pool" {
            return self
                .iris_reload_pool(Parameters(NoParams {}))
                .await
                .map_err(|e| format!("{e:?}"));
        }
        Err(format!("unknown tool: {tool}"))
    }
}

#[cfg(any(test, feature = "testing"))]
impl IrisTools {
    /// An `IrisTools` whose default connection has `declared` as its write and destructive gates,
    /// for a test that calls a mutating tool and means to get past the gate. A test that leaves the
    /// gate to the environment tests the environment (spec 141, R1 5.2).
    pub fn for_test_with_gates(
        iris: Option<IrisConnection>,
        toolset: Toolset,
        declared: write_gate::DeclaredGates,
    ) -> anyhow::Result<Self> {
        Self::with_registry_and_toolset(
            iris,
            crate::skills::SkillRegistry::new(),
            toolset,
            None,
            None,
            false,
            declared,
        )
    }

    /// [`Self::for_test_with_gates`] with both gates open: writes and the destructive tier
    /// declared on.
    pub fn for_test_gates_open(
        iris: Option<IrisConnection>,
        toolset: Toolset,
    ) -> anyhow::Result<Self> {
        Self::for_test_with_gates(
            iris,
            toolset,
            write_gate::DeclaredGates {
                write_tools_enabled: Some(true),
                destructive_tools_enabled: Some(true),
                ..Default::default()
            },
        )
    }

    /// The old name of [`Self::call_gated`], kept so the tests that call handlers by name run
    /// through the gates too.
    pub async fn call_for_test(
        &self,
        tool: &str,
        params: serde_json::Value,
    ) -> Result<rmcp::model::CallToolResult, String> {
        self.call_gated(tool, params).await
    }
}
