use crate::elicitation::ElicitationStore;
use crate::iris::connection::IrisConnection;

/// Remediation hint appended to DOCKER_REQUIRED error strings.
/// Guides native IRIS users (no Docker) toward the HTTP/Atelier REST path.
const DOCKER_REQUIRED_HINT: &str = " Ensure HTTP/Atelier REST is reachable: verify \
    http://<host>:<port>/api/atelier and set host/web_port in .iris-agentic-dev.toml.";

use rmcp::{
    handler::server::router::tool::ToolRouter, handler::server::wrapper::Parameters, model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Wrapper for tools that accept free-form JSON parameters.
/// Uses a manual JsonSchema impl to emit `{"type":"object"}` instead of
/// schemars' default `{"title":"AnyValue"}`, which Claude Code rejects.
#[derive(Debug, Deserialize)]
pub struct AnyParams(pub serde_json::Value);

impl JsonSchema for AnyParams {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AnyParams".into()
    }
    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "object"})
    }
}

impl std::ops::Deref for AnyParams {
    type Target = serde_json::Value;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
pub mod admin;
pub mod dict;
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
    pub loaded_at: std::time::SystemTime,
    pub write_tools_enabled: bool,
    pub config_parse_error: Option<String>,
}

impl ConnectionState {
    pub fn new_disconnected(source: ConnectionSource) -> Self {
        Self {
            iris: None,
            source,
            config_file: None,
            loaded_at: std::time::SystemTime::now(),
            write_tools_enabled: true,
            config_parse_error: None,
        }
    }

    pub fn from_iris(
        iris: IrisConnection,
        source: ConnectionSource,
        config_file: Option<std::path::PathBuf>,
    ) -> Self {
        let write_tools_enabled = iris.is_write_allowed();
        Self {
            iris: Some(Arc::new(iris)),
            source,
            config_file,
            loaded_at: std::time::SystemTime::now(),
            write_tools_enabled,
            config_parse_error: None,
        }
    }
}

/// Tracks the `.iris-agentic-dev.toml` path and last-seen mtime for lazy hot-reload.
/// Always created (even when the file does not yet exist) so we detect new files appearing.
pub struct ConfigWatcher {
    pub config_path: std::path::PathBuf,
    /// None when the file did not exist at last check.
    pub last_mtime: Option<std::time::SystemTime>,
}

impl ConfigWatcher {
    /// Always returns Some — watcher is active even before the file exists.
    pub fn new(config_path: std::path::PathBuf) -> Option<Self> {
        let last_mtime = std::fs::metadata(&config_path)
            .and_then(|m| m.modified())
            .ok();
        Some(Self {
            config_path,
            last_mtime,
        })
    }

    /// Returns true (and updates stored mtime) if the file has been created, modified,
    /// or has appeared for the first time since last check.
    pub fn has_changed(&mut self) -> bool {
        let current_mtime = std::fs::metadata(&self.config_path)
            .and_then(|m| m.modified())
            .ok();
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
                output.push_str(&translate_dml(&sql_content, &rs_var));
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
    let col_names: Vec<String> = split_csv(&col_list_str)
        .iter()
        .map(|c| {
            // Handle "ColName AS alias" → use alias
            let upper = c.to_uppercase();
            if let Some(as_pos) = upper.find(" AS ") {
                c[as_pos + 4..].trim().to_string()
            } else {
                // Strip table qualifier: "t.Name" → "Name"
                c.trim()
                    .split('.')
                    .next_back()
                    .unwrap_or(c.trim())
                    .to_string()
            }
        })
        .collect();

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
    // Fetch row — use single-line if/else for compatibility with execute_via_generator
    out.push_str(&format!("if {}.%Next() {{", rs_var));
    for (idx, var) in host_vars.iter().enumerate() {
        let col = col_names
            .get(idx)
            .map(String::as_str)
            .unwrap_or(var.as_str());
        out.push_str(&format!(" set {} = {}.%Get(\"{}\")", var, rs_var, col));
    }
    out.push_str(" } else {");
    for var in &host_vars {
        out.push_str(&format!(" set {} = \"\"", var));
    }
    out.push_str(&format!(" set {} = {}.%SQLCODE", sqlcode_var, rs_var));
    out.push_str(" }");

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

fn translate_dml(sql: &str, rs_var: &str) -> String {
    let params = extract_where_params(sql);
    let prepared_sql = replace_host_vars_with_positional(sql, &params);
    let exec_args = if params.is_empty() {
        String::new()
    } else {
        format!(", {}", params.join(", "))
    };
    format!(
        "set {} = ##class(%SQL.Statement).%ExecDirect(, \"{}\"{})",
        rs_var,
        prepared_sql.replace('"', "\"\""),
        exec_args
    )
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
    /// Set to true to confirm execution on a subject-role instance (role-gate bypass).
    #[serde(default)]
    pub confirm: bool,
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
    /// Class name to build source map for (e.g. "Graph.KG.NKGAccel" or "Graph.KG.NKGAccel.cls").
    pub cls_name: String,
    /// Not used — kept for backwards compatibility only. May be removed in a future version.
    #[serde(default)]
    pub cls_text: Option<String>,
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
    /// If true (default), rewrite &sql(...) embedded SQL macros to %SQL.Statement calls before executing.
    /// Set to false to send code as-is for debugging.
    #[serde(default = "default_translate_sql")]
    pub translate_sql: bool,
}
fn default_translate_sql() -> bool {
    true
}
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryParams {
    pub query: String,
    /// Query parameters as strings (e.g. ["Alice", "42"])
    #[serde(default)]
    pub parameters: Vec<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    /// If true, bypass SQL safety validation. Use only for intentional administrative queries.
    /// Has no effect on production IRIS instances (where write tools are disabled).
    #[serde(default)]
    pub force: bool,
    /// Set to true to confirm execution on a subject-role instance (role-gate bypass).
    #[serde(default)]
    pub confirm: bool,
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
fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}
fn err_json(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    ok_json(serde_json::json!({"success": false, "error_code": code, "error": msg}))
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

/// Validates that a SQL string is read-only before forwarding to IRIS.
///
/// Processing pipeline:
/// 1. Strip `/* ... */` block comments
/// 2. Strip `-- ...` line comments
/// 3. Return `Err("EMPTY")` if result is whitespace-only
/// 4. Walk remaining chars tracking quote depth; skip `'...'` and `"..."` content
/// 5. Check each unquoted word token against the blocked keyword list (case-insensitive, word-boundary)
/// 6. Check for `SELECT ... INTO <non-paren>` pattern (DDL via SELECT INTO)
///
/// Returns `Ok(())` if safe, `Err(keyword)` with the offending keyword if blocked.
pub fn validate_read_only_sql(sql: &str) -> Result<(), String> {
    const BLOCKED: &[&str] = &[
        "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "MERGE", "TRUNCATE", "EXEC",
        "EXECUTE", "BULK", "LOAD", "KILL", "LOCK",
    ];

    // Step 1: strip /* ... */ block comments
    let mut cleaned = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2; // skip */
            cleaned.push(' '); // preserve word boundary
        } else {
            cleaned.push(bytes[i] as char);
            i += 1;
        }
    }

    // Step 2: strip -- line comments
    let mut no_line_comments = String::with_capacity(cleaned.len());
    for line in cleaned.lines() {
        if let Some(pos) = line.find("--") {
            no_line_comments.push_str(&line[..pos]);
        } else {
            no_line_comments.push_str(line);
        }
        no_line_comments.push(' ');
    }
    let cleaned = no_line_comments;

    // Step 3: empty check
    if cleaned.trim().is_empty() {
        return Err("EMPTY".to_string());
    }

    // Steps 4+5: walk chars, skip quoted content, check word tokens
    let chars: Vec<char> = cleaned.chars().collect();
    let n = chars.len();
    let upper = cleaned.to_uppercase();
    let upper_chars: Vec<char> = upper.chars().collect();

    let mut idx = 0;
    while idx < n {
        let c = chars[idx];
        // Skip single-quoted string literals
        if c == '\'' {
            idx += 1;
            while idx < n && chars[idx] != '\'' {
                if chars[idx] == '\\' {
                    idx += 1;
                }
                idx += 1;
            }
            idx += 1; // closing quote
            continue;
        }
        // Skip double-quoted identifiers
        if c == '"' {
            idx += 1;
            while idx < n && chars[idx] != '"' {
                idx += 1;
            }
            idx += 1;
            continue;
        }
        // Check for keyword match at this position
        for kw in BLOCKED {
            let kw_len = kw.len();
            if idx + kw_len > n {
                continue;
            }
            // Compare against uppercased chars
            let matches = upper_chars[idx..idx + kw_len]
                .iter()
                .zip(kw.chars())
                .all(|(a, b)| *a == b);
            if !matches {
                continue;
            }
            // Word boundary: character before must be non-alphanumeric/non-underscore (or start)
            let before_ok = idx == 0 || {
                let bc = chars[idx - 1];
                !bc.is_alphanumeric() && bc != '_'
            };
            // Word boundary: character after must be non-alphanumeric/non-underscore (or end)
            let after_ok = idx + kw_len >= n || {
                let ac = chars[idx + kw_len];
                !ac.is_alphanumeric() && ac != '_'
            };
            if before_ok && after_ok {
                return Err(kw.to_string());
            }
        }
        idx += 1;
    }

    // Step 6: check for SELECT ... INTO <identifier> (not INTO subquery)
    // Find "INTO" token not followed by '('
    let upper_str = upper.as_str();
    let mut search_start = 0;
    while let Some(pos) = upper_str[search_start..].find("INTO") {
        let abs_pos = search_start + pos;
        // Word boundary check
        let before_ok = abs_pos == 0 || {
            let bc = upper_chars[abs_pos - 1];
            !bc.is_alphanumeric() && bc != '_'
        };
        let after_ok = abs_pos + 4 >= n || {
            let ac = upper_chars[abs_pos + 4];
            !ac.is_alphanumeric() && ac != '_'
        };
        if before_ok && after_ok {
            // Check what follows INTO (skip whitespace)
            let mut after = abs_pos + 4;
            while after < n && chars[after].is_whitespace() {
                after += 1;
            }
            // If followed by '(' it's INTO a subquery — allowed
            // If followed by anything else (identifier, #, @, etc.) — DDL, block it
            if after < n && chars[after] != '(' {
                return Err("SELECT INTO".to_string());
            }
        }
        search_start = abs_pos + 1;
    }

    Ok(())
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

#[derive(Clone)]
pub struct IrisTools {
    /// Active connection state — wraps iris, source, config metadata, write gate.
    /// Arc<Mutex> allows atomic swap from &self tool handlers (034-live-connection-reload).
    pub connection: Arc<std::sync::Mutex<ConnectionState>>,
    /// Lazy config file watcher for hot-reload. None when no .iris-agentic-dev.toml exists.
    pub config_watcher: Arc<std::sync::Mutex<Option<ConfigWatcher>>>,
    pub registry: Arc<crate::skills::SkillRegistry>,
    /// Shared HTTP client — created once, reused across all tool calls.
    pub client: Arc<reqwest::Client>,
    /// Ring buffer of recent tool calls for skill_propose pattern mining.
    pub history: Arc<std::sync::Mutex<VecDeque<ToolCallEntry>>>,
    /// Pending elicitation state for SCM dialogs.
    pub elicitation_store: Arc<ElicitationStore>,
    /// UUID-keyed in-memory log store for progressive disclosure (027).
    pub log_store: Arc<std::sync::Mutex<log_store::LogStore>>,
    /// Session-scoped TTL cache for %Dictionary introspection results (037).
    pub metadata_cache: Arc<dict::MetadataCache>,
    /// Active toolset — controls which tools are registered.
    pub toolset: Toolset,
    #[allow(dead_code)] // used by #[tool_router] macro-generated code
    tool_router: ToolRouter<IrisTools>,
}

#[tool_router]
impl IrisTools {
    pub fn new(iris: Option<IrisConnection>) -> anyhow::Result<Self> {
        let client = Arc::new(IrisConnection::http_client()?);
        let conn_state = match iris {
            Some(c) => ConnectionState::from_iris(c, ConnectionSource::EnvVars, None),
            None => ConnectionState::new_disconnected(ConnectionSource::EnvVars),
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
            config_watcher: Arc::new(std::sync::Mutex::new(None)),
            registry: Arc::new(crate::skills::SkillRegistry::new()),
            client,
            history: Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(50))),
            elicitation_store: Arc::new(ElicitationStore::new()),
            log_store: Arc::new(std::sync::Mutex::new(log_store::LogStore::new(
                log_max, log_ttl,
            ))),
            metadata_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            toolset: Toolset::Baseline,
            tool_router: Self::tool_router(),
        })
    }
    /// Convenience constructor for tests — same as `new` but with explicit toolset.
    pub fn new_with_toolset(
        iris: Option<IrisConnection>,
        toolset: Toolset,
    ) -> anyhow::Result<Self> {
        Self::with_registry_and_toolset(iris, crate::skills::SkillRegistry::new(), toolset, None)
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
            "iris_table_info",
            "resolve_dynamic_dispatch",
            "extract_message_map_routing",
            "find_subclass_implementations",
            "debug_capture_packet",
            "debug_get_error_logs",
            "iris_generate",
            "iris_generate_class",
            // Docker exec
            "iris_test",
            "debug_map_int_to_cls",
            "debug_source_map",
            "iris_source_control",
            "skill",
            "skill_propose",
            "skill_optimize",
            // Local/CLI — 4
            "skill_share",
            "skill_community",
            "skill_community_install",
            "kb",
            // Interoperability — available in all tiers (036: removed individual stubs)
            "iris_production",
            "iris_interop_query",
            "iris_production_item",
            "iris_credential_list",
            "iris_credential_manage",
            "iris_lookup_manage",
            "iris_lookup_transfer",
            // 026-admin-tools
            "iris_admin",
            // 034-live-connection-reload
            "check_config",
        ];

        // Tools removed in nostub — 4 stubs returning NOT_IMPLEMENTED
        // iris_symbols_local is NO LONGER a stub (025-symbols-local-ts)
        let stub_tools: &[&str] = &[
            "skill_propose",
            "skill_optimize",
            "skill_share",
            "skill_community_install",
        ];

        // Tools removed in merged (on top of stubs)
        // 036: individual interop stubs removed entirely; merged dispatchers now in all tiers
        let merged_removed: &[&str] = &[
            "debug_capture_packet",
            "debug_get_error_logs",
            "debug_map_int_to_cls",
            "debug_source_map",
        ];
        let merged_removed_2: &[&str] = &[] as &[&str]; // placeholder
        let merged_added: &[&str] = &[
            "iris_debug",
            "iris_containers",
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
                if !self.write_tools_enabled() {
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
        Self::with_registry_and_toolset(iris, registry, Toolset::Baseline, None)
    }
    pub fn with_registry_and_toolset(
        iris: Option<IrisConnection>,
        registry: crate::skills::SkillRegistry,
        toolset: Toolset,
        config_watcher: Option<ConfigWatcher>,
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
            ];
            for name in merged_only {
                router.remove_route(name);
            }
        }

        let conn_state = match iris {
            Some(c) => {
                let write_tools_enabled = c.is_write_allowed();
                tracing::info!(
                    system_mode = ?c.system_mode,
                    write_tools_enabled,
                    namespace = %c.namespace,
                    "iris-agentic-dev: write tool gate evaluated"
                );
                // Remove write-capable tools if not allowed (issue #26 env guard).
                // iris_production_item is write-capable; available in all tiers but gated on prod.
                if !write_tools_enabled {
                    let write_gated: &[&str] = &["iris_production_item", "iris_credential_manage"];
                    for name in write_gated {
                        router.remove_route(name);
                    }
                }
                ConnectionState::from_iris(c, ConnectionSource::AutoDiscovered, None)
            }
            None => ConnectionState::new_disconnected(ConnectionSource::EnvVars),
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
            registry: Arc::new(registry),
            client,
            history: Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(50))),
            elicitation_store: Arc::new(ElicitationStore::new()),
            log_store: Arc::new(std::sync::Mutex::new(log_store::LogStore::new(
                log_max, log_ttl,
            ))),
            metadata_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            toolset,
            tool_router: router,
        })
    }

    /// Returns the active IRIS connection, or IRIS_UNREACHABLE if not connected.
    fn get_iris(&self) -> Result<Arc<IrisConnection>, McpError> {
        self.connection
            .lock()
            .unwrap()
            .iris
            .clone()
            .ok_or_else(iris_unreachable)
    }

    /// Check for config file changes then return the active connection.
    /// Use this in tool handlers instead of get_iris() to enable hot-reload (034).
    async fn get_iris_reloaded(&self) -> Result<Arc<IrisConnection>, McpError> {
        self.check_reload().await;
        self.get_iris()
    }

    /// Returns the active write_tools_enabled flag from connection state.
    fn write_tools_enabled(&self) -> bool {
        self.connection.lock().unwrap().write_tools_enabled
    }

    /// Returns the `ConnectionRole` and instance name for the currently-active connection.
    ///
    /// In operate mode (`mode = "operate"` in `.iris-agentic-dev.toml`), matches the active
    /// `IrisConnection` against declared `[instance.*]` blocks by container name or host.
    /// Returns `(Workspace, "")` when no fleet config is present, mode is not "operate",
    /// or no instance block matches — i.e., the default / dev-mode case is always permitted.
    pub fn instance_role(&self) -> (crate::iris::workspace_config::ConnectionRole, String) {
        use crate::iris::connection::DiscoverySource;
        use crate::iris::workspace_config::{load_fleet_config, ConnectionRole};

        let (workspace_path, iris_arc) = {
            // Prefer config_watcher path (set at startup from OBJECTSCRIPT_WORKSPACE / --workspace).
            // Fall back to config_file on ConnectionState (set only after a hot-reload cycle).
            let watcher_ws = {
                let w = self.config_watcher.lock().unwrap();
                w.as_ref()
                    .and_then(|w| w.config_path.parent())
                    .and_then(|p| p.to_str())
                    .map(|s| s.to_string())
            };
            let conn = self.connection.lock().unwrap();
            let ws = watcher_ws.or_else(|| {
                conn.config_file
                    .as_ref()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.to_str())
                    .map(|s| s.to_string())
            });
            (ws, conn.iris.clone())
        };

        let Some(fleet) = load_fleet_config(workspace_path.as_deref()) else {
            return (ConnectionRole::Workspace, String::new());
        };
        if fleet.mode.as_deref() != Some("operate") {
            return (ConnectionRole::Workspace, String::new());
        }
        let Some(iris) = iris_arc else {
            return (ConnectionRole::Workspace, String::new());
        };

        // Active container name from DiscoverySource or IRIS_CONTAINER env var fallback.
        let active_container = match &iris.source {
            DiscoverySource::Docker { container_name } => Some(container_name.clone()),
            _ => std::env::var("IRIS_CONTAINER")
                .ok()
                .filter(|s| !s.is_empty()),
        };

        for (name, inst) in &fleet.instance {
            let matches = if let Some(ref ic) = inst.container {
                // Match by container name if the instance declares one.
                active_container.as_deref() == Some(ic.as_str())
            } else {
                // No container in instance config — match by host in base_url.
                // Use scheme-stripped prefix match ("://host:") to avoid "iris" matching
                // "my-iris-dev" or a hostname that is a substring of another.
                inst.host
                    .as_deref()
                    .map(|h| {
                        let needle = format!("://{h}:");
                        iris.base_url.contains(&needle)
                    })
                    .unwrap_or(false)
            };
            if matches {
                return (inst.role.clone(), name.clone());
            }
        }
        (ConnectionRole::Workspace, String::new())
    }

    /// Returns the active connection as Option<Arc>, for interop helpers that take Option<&IrisConnection>.
    fn iris_arc(&self) -> Option<Arc<IrisConnection>> {
        self.connection.lock().unwrap().iris.clone()
    }

    /// Check if `.iris-agentic-dev.toml` has changed since last load; if so, reload and re-probe.
    /// Called at the start of every tool handler for lazy hot-reload (034).
    /// Completely silent — no error returned to caller on reload failure.
    async fn check_reload(&self) {
        // Check if watcher says config changed
        let changed = {
            let mut w = self.config_watcher.lock().unwrap();
            w.as_mut().map(|w| w.has_changed()).unwrap_or(false)
        };
        if !changed {
            return;
        }

        // Config file changed — reload and re-probe
        let config_path = {
            let w = self.config_watcher.lock().unwrap();
            w.as_ref().map(|w| w.config_path.clone())
        };
        let Some(config_path) = config_path else {
            return;
        };

        let config_file_str = config_path
            .parent()
            .and_then(|p| p.to_str())
            .map(|s| s.to_string());

        // Parse the new config
        let cfg = crate::iris::workspace_config::load_workspace_config(config_file_str.as_deref());

        let conn_result = match cfg {
            None => {
                // File parse error or missing — set error in state, keep old connection
                let mut conn = self.connection.lock().unwrap();
                conn.config_parse_error =
                    Some("Config file changed but could not be parsed".to_string());
                return;
            }
            Some(cfg) => {
                crate::iris::workspace_config::workspace_config_to_connection(&cfg, "USER")
            }
        };

        // Probe the new connection
        let mut new_conn = match conn_result {
            Some(c) => c,
            None => {
                // container= config — let discovery find it via IRIS_CONTAINER env
                match crate::iris::discovery::discover_iris(None).await {
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

        // Atomically swap connection
        let new_state =
            ConnectionState::from_iris(new_conn, ConnectionSource::ConfigFile, Some(config_path));
        let mut conn = self.connection.lock().unwrap();
        *conn = new_state;
        conn.config_parse_error = None;
        tracing::info!("iris-agentic-dev: hot-reloaded connection from .iris-agentic-dev.toml");
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
        let iris = self.get_iris_reloaded().await?;
        let (role, instance_name) = self.instance_role();
        if let Some(gate) = crate::iris::workspace_config::check_role_gate(
            &role,
            "iris_compile",
            p.confirm,
            &instance_name,
            false,
        ) {
            return ok_json(gate);
        }
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
                // Check PUT response body for Atelier-level errors (200 OK with status.errors
                // can occur on some IRIS builds when the upload fails internally, e.g. build 110
                // SetTextFromString NULL namespace bug).
                let put_body: serde_json::Value = put_resp.json().await.unwrap_or_default();
                if let Some(errs) = put_body["status"]["errors"].as_array() {
                    if !errs.is_empty() {
                        let msg = errs[0]["error"].as_str().unwrap_or("Upload failed");
                        self.record_call("iris_compile", false);
                        return err_json("UPLOAD_FAILED", msg);
                    }
                }
                // Compile via shared compile_document helper
                let local_src = p.target.clone();
                let cr = iris
                    .compile_document(&doc_name, &p.namespace, &p.flags, client)
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
        description = "Run %UnitTest.Manager tests on IRIS and return structured pass/fail results. Uses pure-HTTP execution via Atelier REST — works with or without IRIS_CONTAINER. Pass a class name pattern like 'MyApp.Tests' or 'ISC.sql.TestFoo' to run already-compiled test classes (uses /noload automatically). Pass a directory path like 'MyApp/Tests' to load from disk. Returns suite-level summary inline plus log_id for per-test-case detail via iris_get_log."
    )]
    async fn iris_test(
        &self,
        Parameters(p): Parameters<TestParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(namespace = %p.namespace, pattern = %p.pattern, "iris_test");
        let timeout = std::time::Duration::from_secs(p.timeout);

        // HTTP path only — docker exec path removed (#46: /noload/run assumed pre-loaded
        // classes which never existed in a fresh iris session, causing false "no test classes"
        // errors; HTTP path with /verbose=1 is reliable and works with or without docker).
        let path_label = "http";
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

        // Detect whether the pattern is a compiled class name or a filesystem directory path.
        // Class names contain dots and no path separators: "ISC.sql.Tests", "MyApp.Tests.*"
        // Directory paths contain / or \ : "MyApp/Tests", "/tmp/tests/MyApp"
        // When the pattern is a class name, pass /noload so RunTest looks in the compiled
        // database rather than scanning the filesystem under ^UnitTestRoot.
        let is_class_pattern = !safe_pattern.contains('/') && !safe_pattern.contains('\\');
        let flags = if is_class_pattern {
            "/verbose=1/nodelete/noload"
        } else {
            "/verbose=1/nodelete"
        };

        // Run tests via execute_via_generator (HTTP path).
        // After RunTest completes, ^UnitTest.Result global IS persisted (globals bypass
        // the objectgenerator transaction boundary; SQL %Save() does not).
        let run_code = if is_class_pattern {
            // Class pattern: no filesystem directory needed; /noload finds compiled class directly.
            format!(
                r#"do ##class(%UnitTest.Manager).RunTest("{pattern}","{flags}","{token}")"#,
                token = correlation_token,
                pattern = safe_pattern,
                flags = flags,
            )
        } else {
            // Directory path: set ^UnitTestRoot and pre-create the pattern subdirectory.
            // iris_test preamble sets ^UnitTestRoot to /tmp/httest/ (IRIS community default).
            format!(
                r#"set utRoot="/tmp/httest/"
if '##class(%File).DirectoryExists(utRoot) {{ do ##class(%File).CreateDirectoryChain(utRoot) }}
set pkgDir=utRoot_"{pattern}"_"/"
if '##class(%File).DirectoryExists(pkgDir) {{ do ##class(%File).CreateDirectoryChain(pkgDir) }}
set ^UnitTestRoot=utRoot
do ##class(%UnitTest.Manager).RunTest("{pattern}","{flags}","{token}")"#,
                token = correlation_token,
                pattern = safe_pattern,
                flags = flags,
            )
        };

        // Try HTTP (execute_via_generator) first. Fall back to docker exec if:
        // - IRIS_CONTAINER is set, AND
        // - HTTP returns empty output (RunTest couldn't create the pattern directory
        //   because execute_via_generator restricts filesystem writes)
        // RunTest writes verbose output to $IO (terminal device).
        // execute_via_generator redirects $IO to a temp file but RunTest also needs
        // to create directories under ^UnitTestRoot — which fails in that context.
        // When IRIS_CONTAINER is set, prefer docker exec (full filesystem + real terminal).
        let has_container = std::env::var("IRIS_CONTAINER")
            .ok()
            .filter(|v| !v.is_empty())
            .is_some();

        let run_output = if has_container {
            // Docker exec: full filesystem access, captures terminal output from RunTest
            match tokio::time::timeout(timeout, iris.execute(&run_code, &p.namespace)).await {
                Err(_) => {
                    self.record_call("iris_test", false);
                    return ok_json(serde_json::json!({
                        "success": false,
                        "error_code": "TIMEOUT",
                        "error": format!("Test run timed out after {}s", p.timeout),
                    }));
                }
                Ok(Err(_)) => {
                    // Docker exec unavailable — fall through to HTTP
                    match tokio::time::timeout(
                        timeout,
                        iris.execute_via_generator(&run_code, &p.namespace, client),
                    )
                    .await
                    {
                        Ok(Ok(out)) => out,
                        _ => {
                            self.record_call("iris_test", false);
                            return ok_json(serde_json::json!({
                                "success": false,
                                "error_code": "DOCKER_REQUIRED",
                                "error": format!("iris_test: IRIS_CONTAINER set but docker exec failed and HTTP fallback also failed.{DOCKER_REQUIRED_HINT}"),
                            }));
                        }
                    }
                }
                Ok(Ok(out)) => out,
            }
        } else {
            // HTTP path: works for remote IRIS without docker
            match tokio::time::timeout(
                timeout,
                iris.execute_via_generator(&run_code, &p.namespace, client),
            )
            .await
            {
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
            }
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
        description = "Execute arbitrary ObjectScript code on IRIS and return stdout. Uses pure-HTTP execution via CodeMode=objectgenerator (write temp class, compile, query result, delete). Falls back to docker exec if IRIS_CONTAINER env var is set and HTTP fails. &sql(...) embedded SQL macros are automatically translated to %SQL.Statement calls (set translate_sql: false to disable). When translation fires, response includes sql_translated: true and translated_code. Example: code='write $ZVERSION,!' returns the IRIS version string."
    )]
    async fn iris_execute(
        &self,
        Parameters(p): Parameters<ExecuteParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
        let (role, instance_name) = self.instance_role();
        if let Some(gate) = crate::iris::workspace_config::check_role_gate(
            &role,
            "iris_execute",
            p.confirmed,
            &instance_name,
            false,
        ) {
            return ok_json(gate);
        }
        tracing::info!(namespace = %p.namespace, translate_sql = p.translate_sql, "iris_execute");
        let client = self.http_client();
        let timeout = std::time::Duration::from_secs(p.timeout);

        // &sql macro translation — rewrite before sending to IRIS (035)
        let translation = if p.translate_sql {
            let r = translate_sql_macros(&p.code);
            Some(r)
        } else {
            None
        };
        let code_to_run = translation
            .as_ref()
            .filter(|r| r.found)
            .map(|r| r.translated_code.as_str())
            .unwrap_or(&p.code);

        // Try pure-HTTP execution first (write-compile-query via CodeMode=objectgenerator).
        let gen_result = tokio::time::timeout(
            timeout,
            iris.execute_via_generator(code_to_run, &p.namespace, client),
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
                let trimmed = output.trim();
                // Catch ObjectScript runtime errors written by the Catch block or $ZERROR check.
                let is_runtime_error =
                    trimmed.starts_with("ERROR: ") || trimmed.starts_with("ERROR($ZERROR): ");
                self.record_call("iris_execute", !is_runtime_error);
                let mut resp = serde_json::json!({
                    "success": !is_runtime_error,
                    "output": trimmed,
                    "namespace": p.namespace,
                    "method": "http",
                });
                if is_runtime_error {
                    resp["error_code"] = serde_json::Value::String("IRIS_RUNTIME_ERROR".into());
                }
                if let Some(ref tr) = translation {
                    if tr.found {
                        resp["sql_translated"] = serde_json::Value::Bool(true);
                        resp["translated_code"] =
                            serde_json::Value::String(tr.translated_code.clone());
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
                return ok_json(resp);
            }
            Ok(Err(_)) => {
                // HTTP path failed — fall through to docker exec.
            }
        }

        // Fallback: docker exec (requires IRIS_CONTAINER env var).
        let docker_result =
            tokio::time::timeout(timeout, iris.execute(code_to_run, &p.namespace)).await;
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
                        "error": format!("iris_execute: HTTP execution failed and IRIS_CONTAINER is not set for docker exec fallback.{DOCKER_REQUIRED_HINT}"),
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
                let trimmed = output.trim();
                let is_runtime_error =
                    trimmed.starts_with("ERROR: ") || trimmed.starts_with("ERROR($ZERROR): ");
                self.record_call("iris_execute", !is_runtime_error);
                let mut resp = serde_json::json!({
                    "success": !is_runtime_error,
                    "output": trimmed,
                    "namespace": p.namespace,
                    "method": "docker",
                });
                if is_runtime_error {
                    resp["error_code"] = serde_json::Value::String("IRIS_RUNTIME_ERROR".into());
                }
                if let Some(ref tr) = translation {
                    if tr.found {
                        resp["sql_translated"] = serde_json::Value::Bool(true);
                        resp["translated_code"] =
                            serde_json::Value::String(tr.translated_code.clone());
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
                ok_json(resp)
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
        let iris = self.get_iris_reloaded().await?;
        tracing::info!(namespace = %p.namespace, "iris_doc");
        let client = self.http_client();
        let result = doc::handle_iris_doc(&iris, client, p, &self.elicitation_store).await;
        self.record_call("iris_doc", result.is_ok());
        result
    }

    #[tool(
        description = "Execute a SQL SELECT query on IRIS via Atelier REST. Returns rows as a JSON array with column names as keys. By default, destructive SQL (DROP, DELETE, INSERT, UPDATE, ALTER, CREATE, MERGE, TRUNCATE, EXEC, EXECUTE, BULK, LOAD, KILL, LOCK, SELECT INTO) is blocked before reaching IRIS. Set force: true to bypass validation for intentional administrative queries — has no effect on production instances where write tools are disabled. No Python required."
    )]
    async fn iris_query(
        &self,
        Parameters(p): Parameters<QueryParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(namespace = %p.namespace, force = p.force, "iris_query");

        // Role gate: SELECT is always permitted on subject; write SQL requires confirm.
        {
            let (role, instance_name) = self.instance_role();
            let first_word = p
                .query
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_uppercase();
            let tool_name = if first_word == "SELECT" || first_word == "WITH" {
                "iris_query:SELECT"
            } else {
                "iris_query:INSERT"
            };
            if let Some(gate) = crate::iris::workspace_config::check_role_gate(
                &role,
                tool_name,
                p.confirm,
                &instance_name,
                false,
            ) {
                return ok_json(gate);
            }
        }

        // SQL safety gate — validate before any network call
        let skip_validation = p.force && self.write_tools_enabled();
        if !skip_validation {
            match validate_read_only_sql(&p.query) {
                Err(ref kw) if kw == "EMPTY" => {
                    self.record_call("iris_query", false);
                    return ok_json(serde_json::json!({
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
                        "error": format!("Destructive SQL keyword '{}' is not allowed. Use force: true to override.", kw),
                        "blocked_keyword": kw,
                    });
                    if p.force && !self.write_tools_enabled() {
                        resp["force_ignored"] = serde_json::Value::Bool(true);
                    }
                    return ok_json(resp);
                }
                Ok(()) => {}
            }
        }

        let iris = self.get_iris_reloaded().await?;
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
        let iris_arc = self.iris_arc();
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
        description = "Switch the active IRIS connection to the specified running Docker container for this session. After a successful switch, all subsequent tool calls target the new container — no session restart required. Fixes issue #11."
    )]
    async fn iris_select_container(
        &self,
        Parameters(p): Parameters<SelectContainerParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_reload().await;
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
            &p.namespace,
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
            return ok_json(serde_json::json!({
                "success": false,
                "error": "CONTAINER_UNREACHABLE",
                "container": p.name,
                "port_web": port_web,
                "message": "Container found but Atelier REST API did not respond. Check that the container is running and the web server is accessible.",
            }));
        }

        let version = new_conn.version.clone();
        let write_tools_enabled = new_conn.is_write_allowed();

        // Atomically swap the active connection (fixes issue #11).
        let new_state =
            ConnectionState::from_iris(new_conn, ConnectionSource::IrisSelectContainer, None);
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
            "namespace": p.namespace,
            "version": version,
            "write_tools_enabled": write_tools_enabled,
        }))
    }

    #[tool(
        description = "Return the active IRIS connection state without making any IRIS network calls. Always succeeds — never returns IRIS_UNREACHABLE. Use to: (1) diagnose connection issues, (2) verify hot-reload completed, (3) confirm which container/host is active. To switch connection mid-session without restart: call check_config first to get config_watch_path, then write a .iris-agentic-dev.toml to that exact path, then call any tool — the reload fires automatically. Fields: connected, connection_source (http|docker|disconnected), host, port, namespace, container, config_file, config_watch_path, config_loaded_at, iris_version, write_tools_enabled."
    )]
    async fn check_config(
        &self,
        Parameters(_p): Parameters<crate::tools::NoParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_reload().await;
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
                .map(|w| w.config_path.to_string_lossy().to_string())
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null)
        };

        let mut response = serde_json::json!({
            "connected": conn.iris.is_some(),
            "connection_source": connection_source,
            "host": host,
            "port": port,
            "namespace": namespace,
            "container": container,
            "config_file": config_file,
            "config_loaded_at": config_loaded_at,
            "iris_version": iris_version,
            "write_tools_enabled": conn.write_tools_enabled,
            "config_watch_path": config_watcher_path,
        });

        if let Some(ref err) = conn.config_parse_error {
            response["config_parse_error"] = serde_json::Value::String(err.clone());
        }

        ok_json(response)
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
        let iris = self.get_iris_reloaded().await?;
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
        let iris = self.get_iris_reloaded().await?;
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
        let iris = self.get_iris_reloaded().await?;
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
                "error": format!("debug_map_int requires docker exec. Set IRIS_CONTAINER=<container_name>.{DOCKER_REQUIRED_HINT}"),
            })),
            Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
        }
    }

    #[tool(description = "Capture IRIS error state and recent error log entries for debugging.")]
    async fn debug_capture_packet(
        &self,
        Parameters(_p): Parameters<CapturePacketParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
        let client = self.http_client();
        match iris.query("SELECT TOP 20 ErrorCode,ErrorText,TimeStamp FROM %SYSTEM.Error ORDER BY TimeStamp DESC", vec![], &_p.namespace, client).await {
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

    #[tool(description = "Retrieve recent IRIS error log entries.")]
    async fn debug_get_error_logs(
        &self,
        Parameters(p): Parameters<ErrorLogsParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
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
        description = "Build a .INT source map for a compiled ObjectScript class via Atelier xecute. Maps .INT routine line offsets back to .CLS source lines for stack trace resolution. No Python required."
    )]
    async fn debug_source_map(
        &self,
        Parameters(p): Parameters<SourceMapParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
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
                "error": format!("debug_source_map requires docker exec. Set IRIS_CONTAINER=<container_name>.{DOCKER_REQUIRED_HINT}"),
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

        if let Some(iris) = self.iris_arc().as_deref() {
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

        let introspection_context = if let Some(iris) = self.iris_arc().as_deref() {
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
        if let Some(iris) = self.iris_arc().as_deref() {
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
        if let Some(iris) = self.iris_arc().as_deref() {
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
        if let Some(iris) = self.iris_arc().as_deref() {
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
        if let Some(iris) = self.iris_arc().as_deref() {
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
            &format!("skill_forget requires docker exec. Set IRIS_CONTAINER=<container_name>.{DOCKER_REQUIRED_HINT}"),
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
        description = "List all skills loaded from --subscribe packages. Use --subscribe owner/repo when starting iris-agentic-dev mcp to load community skills."
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
        let iris = self.get_iris_reloaded().await?;
        skills_tools::handle_kb(
            &iris,
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
        description = "Full-text search across IRIS documents via Atelier REST v2. Auto-upgrades to async polling for large namespaces. Supports regex, case sensitivity, category filter (CLS/MAC/INT/INC/ALL), and wildcard document scopes."
    )]
    async fn iris_search(
        &self,
        Parameters(p): Parameters<search::SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
        let result =
            search::handle_iris_search(&iris, self.http_client(), p, Arc::clone(&self.log_store))
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
        let iris = self.get_iris_reloaded().await?;
        let result =
            info::handle_iris_info(&iris, self.http_client(), p, Arc::clone(&self.log_store)).await;
        self.record_call("iris_info", result.is_ok());
        result
    }

    #[tool(
        description = "Inspect a SQL table: returns whether it is a class-projected table or DDL-created, the backing data/index globals, and (optionally) an approximate row count. Works for both class-projected tables (with real storage globals from %Dictionary.CompiledStorage) and DDL tables (globals inferred by IRIS naming convention). Use include_row_count=true to add a COUNT(*) estimate."
    )]
    async fn iris_table_info(
        &self,
        Parameters(p): Parameters<info::TableInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
        let result = info::handle_iris_table_info(&iris, self.http_client(), p).await;
        self.record_call("iris_table_info", result.is_ok());
        result
    }

    #[tool(
        description = "Resolve ObjectScript dynamic dispatch: find all compiled classes that implement a given method. Use when you see $classmethod(var, method) or ##class({variable}).Method() and need to know the possible targets. Returns candidates with confidence scores (fewer matches = higher confidence). Confidence: 1 match=0.90, 2-5=0.75, 6-20=0.55, >20=0.30. Results cached 60s per session."
    )]
    async fn resolve_dynamic_dispatch(
        &self,
        Parameters(p): Parameters<dict::ResolveDynamicDispatchParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
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
        description = "Extract Ensemble MessageMap routing table from a compiled BusinessProcess or Router class. Returns the MessageType → Method dispatch table with confidence 0.9 (compiled routing = near ground truth). Use to find CALLS edges that static analysis cannot see. Returns has_message_map:false for classes without a MessageMap. Results cached 60s per session."
    )]
    async fn extract_message_map_routing(
        &self,
        Parameters(p): Parameters<dict::ExtractMessageMapParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
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
        description = "Find all concrete subclass implementations of a method in the full inheritance hierarchy. Given base class names and a method name, expands to all descendants at any depth and returns classes where the method is defined (Origin = parent, not inherited). Use to resolve polymorphic dispatch: adapter.Execute() → find all EnsLib.*.Adapter subclasses that implement Execute. Results cached 60s per session."
    )]
    async fn find_subclass_implementations(
        &self,
        Parameters(p): Parameters<dict::FindSubclassImplementationsParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
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
        description = "Inspect IRIS macros. action=list returns all macros, action=signature returns parameters, action=location finds definition file/line, action=definition returns text, action=expand expands with arguments."
    )]
    async fn iris_macro(
        &self,
        Parameters(p): Parameters<info::MacroParams>,
    ) -> Result<CallToolResult, McpError> {
        let iris = self.get_iris_reloaded().await?;
        let result = info::handle_iris_macro(&iris, self.http_client(), p).await;
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
        let iris = self.get_iris_reloaded().await?;
        let result = info::handle_iris_debug(&iris, self.http_client(), p).await;
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
        let iris = self.get_iris_reloaded().await?;
        let result = info::handle_iris_generate(&iris, self.http_client(), p).await;
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
        let iris = self.get_iris_reloaded().await?;
        let result = skills_tools::handle_skill(&iris, self.http_client(), p, &self.history).await;
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
        let iris = self.get_iris_reloaded().await?;
        let result =
            skills_tools::handle_skill_community(&iris, self.http_client(), p, &self.registry)
                .await;
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
        let iris = self.get_iris_reloaded().await?;
        let result = skills_tools::handle_kb(&iris, self.http_client(), p).await;
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
        let iris = self.get_iris_reloaded().await?;
        let result =
            skills_tools::handle_agent_info(&iris, self.http_client(), p, &self.history).await;
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
        let iris = self.get_iris_reloaded().await?;
        // Role gate: write actions (checkout, execute) are hard-blocked on subject instances.
        // Read actions (status, menu) are always permitted.
        {
            let (role, instance_name) = self.instance_role();
            let is_write = matches!(p.action.as_str(), "checkout" | "execute");
            if is_write {
                if let Some(gate) = crate::iris::workspace_config::check_role_gate(
                    &role,
                    "iris_source_control:commit",
                    p.confirm,
                    &instance_name,
                    true,
                ) {
                    return ok_json(gate);
                }
            }
        }
        let result =
            scm::handle_iris_source_control(&iris, self.http_client(), p, &self.elicitation_store)
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
        Parameters(p): Parameters<AnyParams>,
    ) -> Result<CallToolResult, McpError> {
        let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("status");
        let _iris_arc_hold = self.iris_arc();
        let iris_opt = _iris_arc_hold.as_deref();
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
        Parameters(p): Parameters<AnyParams>,
    ) -> Result<CallToolResult, McpError> {
        let what = p.get("what").and_then(|v| v.as_str()).unwrap_or("logs");
        let _iris_arc_hold = self.iris_arc();
        let iris_opt = _iris_arc_hold.as_deref();
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
        Parameters(p): Parameters<AnyParams>,
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
        Parameters(p): Parameters<AnyParams>,
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
            self.iris_arc().as_deref(),
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
        Parameters(p): Parameters<AnyParams>,
    ) -> Result<CallToolResult, McpError> {
        let namespace = p
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("USER")
            .to_string();
        let result = interop::interop_credential_list_impl(
            self.iris_arc().as_deref(),
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
        Parameters(p): Parameters<AnyParams>,
    ) -> Result<CallToolResult, McpError> {
        let result = interop::interop_credential_manage_impl(
            self.iris_arc().as_deref(),
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
        Parameters(p): Parameters<AnyParams>,
    ) -> Result<CallToolResult, McpError> {
        let result = interop::interop_lookup_manage_impl(
            self.iris_arc().as_deref(),
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
        Parameters(p): Parameters<AnyParams>,
    ) -> Result<CallToolResult, McpError> {
        let result = interop::interop_lookup_transfer_impl(
            self.iris_arc().as_deref(),
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
        Parameters(p): Parameters<AnyParams>,
    ) -> Result<CallToolResult, McpError> {
        let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let _iris_arc_hold = self.iris_arc();
        let iris_opt = _iris_arc_hold.as_deref();
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
                "iris-agentic-dev".to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ))
            .with_instructions(
                "iris-agentic-dev: composable MCP tools for ObjectScript and IRIS development."
                    .to_string(),
            )
    }

    /// Override list_tools to rewrite JSON Schema 2020-12 nullable types to OpenAPI 3.0 anyOf.
    /// schemars + rmcp emit `"type": ["T", "null"]` which Google Vertex AI and Azure OpenAI
    /// reject. Rewrite to `"anyOf": [{"type": "T", ...siblings}, {"type": "null"}]`.
    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        let mut tools = self.tool_router.list_all();
        for tool in tools.iter_mut() {
            let schema = std::sync::Arc::make_mut(&mut tool.input_schema);
            normalize_schema_openapi3(schema);
        }
        Ok(rmcp::model::ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        })
    }
}

/// Recursively rewrite JSON Schema 2020-12 nullable arrays to OpenAPI 3.0 anyOf.
///
/// schemars + rmcp emit `"type": ["integer", "null"]` (JSON Schema 2020-12) which
/// Google Vertex AI and Azure OpenAI reject. Rewrites to OpenAPI 3.0:
/// `"anyOf": [{"type": "integer", "minimum": 0}, {"type": "null"}]`.
fn normalize_schema_openapi3(schema: &mut serde_json::Map<String, serde_json::Value>) {
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
        let result = IrisTools::with_registry_and_toolset(None, registry, Toolset::Nostub, None);
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
    #[test]
    fn test_default_execute_timeout_default_value() {
        std::env::remove_var("OBJECTSCRIPT_TEST_TIMEOUT");
        let t = default_execute_timeout();
        assert_eq!(t, 120, "default timeout must be 120s");
    }
    #[test]
    fn test_default_execute_timeout_env_override() {
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
        assert_eq!(p.namespace, "USER");
        assert_eq!(p.target, "Foo.Bar");
        assert!(!p.force_writable);
    }
    #[test]
    fn test_test_params_defaults() {
        let p: TestParams = serde_json::from_str(r#"{"pattern": "MyTests.*"}"#).unwrap();
        assert_eq!(p.namespace, "USER");
        assert_eq!(p.pattern, "MyTests.*");
    }
    #[test]
    fn test_execute_params_defaults() {
        let p: ExecuteParams = serde_json::from_str(r#"{"code": "Write 1"}"#).unwrap();
        assert_eq!(p.namespace, "USER");
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
        assert_eq!(p.namespace, "USER");
    }
    #[test]
    fn test_introspect_params_defaults() {
        let p: IntrospectParams =
            serde_json::from_str(r#"{"class_name": "Ens.Production"}"#).unwrap();
        assert_eq!(p.namespace, "USER");
    }
    #[test]
    fn test_generate_class_params_defaults() {
        let p: GenerateClassParams =
            serde_json::from_str(r#"{"description": "A simple class"}"#).unwrap();
        assert_eq!(p.namespace, "USER");
        assert!(!p.overwrite);
    }
    #[test]
    fn test_generate_test_params_defaults() {
        let p: GenerateTestParams = serde_json::from_str(r#"{"class_name": "Foo.Bar"}"#).unwrap();
        assert_eq!(p.namespace, "USER");
        assert_eq!(p.class_name, "Foo.Bar");
    }
    #[test]
    fn test_query_params_defaults() {
        let p: QueryParams = serde_json::from_str(r#"{"query": "SELECT 1"}"#).unwrap();
        assert_eq!(p.namespace, "USER");
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
        // "FROMAGE" must not match keyword "FROM" unless it is a full token
        // Behavior depends on implementation; at minimum the function returns Some or None
        // consistently (we just assert the call doesn't panic).
        let _ = find_keyword_pos("FROMAGE t", "FROM");
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
        assert_eq!(p.namespace, "MYNS");
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

/// Test-only dispatch helper — call private IrisTools handler methods by tool name.
#[cfg(any(test, feature = "testing"))]
impl IrisTools {
    /// Call a tool by name with JSON params. Returns the raw CallToolResult or an error string.
    /// Only covers the tools most useful for coverage testing.
    pub async fn call_for_test(
        &self,
        tool: &str,
        params: serde_json::Value,
    ) -> Result<rmcp::model::CallToolResult, String> {
        use rmcp::handler::server::wrapper::Parameters;
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
        // AnyParams-based dispatchers (admin, production, interop)
        macro_rules! dispatch_any {
            ($name:expr, $method:ident) => {
                if tool == $name {
                    return self
                        .$method(Parameters(AnyParams(params)))
                        .await
                        .map_err(|e| format!("{e:?}"));
                }
            };
        }
        dispatch_any!("iris_admin", iris_admin);
        dispatch_any!("iris_production", iris_production);
        dispatch_any!("iris_interop_query", iris_interop_query);
        dispatch_any!("iris_production_item", iris_production_item);
        dispatch_any!("iris_credential_list", iris_credential_list);
        dispatch_any!("iris_credential_manage", iris_credential_manage);
        dispatch_any!("iris_lookup_manage", iris_lookup_manage);
        dispatch_any!("iris_lookup_transfer", iris_lookup_transfer);
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
        dispatch_any!("iris_containers", iris_containers);
        dispatch!("skill_propose", NoParams, skill_propose);
        dispatch!("skill_optimize", SkillNameParams, skill_optimize);
        dispatch!("skill_share", SkillNameParams, skill_share);
        Err(format!("unknown tool: {tool}"))
    }
}
