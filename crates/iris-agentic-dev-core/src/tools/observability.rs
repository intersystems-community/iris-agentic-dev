//! System observability actions for the iris_admin dispatcher (055-system-observability).
//!
//! Five read-only actions: view_locks, view_processes, journal_search,
//! namespace_mappings, database_status. All are ToolCategory::Query, permitted
//! on every mcpTemplate value. Called from the iris_admin match dispatcher in mod.rs.

use crate::iris::connection::{
    is_generator_error, is_request_timeout, GeneratorCall, IrisConnection, Secret,
};
use crate::policy::patterns::{
    matches_any, matches_any_owned, PHI_NAME_PATTERNS, SYSTEM_BLOCKLIST,
};
use rmcp::{model::*, ErrorData as McpError};

// ── Shared helpers ────────────────────────────────────────────────────────────

pub(crate) fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::structured(v))
}

pub(crate) fn err_json(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    crate::tools::err_result(
        serde_json::json!({"success": false, "error_code": code, "error": msg}),
    )
}

/// Guard: requires dataPolicy == "allow". Returns Some(blocked error) when not allowed.
/// Used by view_processes (block/redact/allow) and journal_search (allow only).
pub fn require_data_policy_allow(
    data_policy: &str,
    action: &str,
) -> Option<Result<CallToolResult, McpError>> {
    if data_policy != "allow" {
        Some(err_json(
            "DATA_POLICY_BLOCKED",
            &format!(
                "{action} requires dataPolicy=allow — this action exposes PHI-capable data. \
                 Set dataPolicy=allow in your connection policy to proceed."
            ),
        ))
    } else {
        None
    }
}

/// Redact PHI-adjacent fields from a single process entry JSON object.
pub fn redact_process_entry(entry: &mut serde_json::Value) {
    for field in &["username", "client_node_name", "client_ip"] {
        if let Some(obj) = entry.as_object_mut() {
            if obj.contains_key(*field) {
                obj.insert(field.to_string(), serde_json::json!("[REDACTED]"));
            }
        }
    }
}

/// Translate a glob pattern to a SQL LIKE pattern.
/// `*` → `%`, `?` → `_`, literal `%` and `_` are escaped with `\`.
pub fn glob_to_sql_like(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() + 8);
    for ch in pattern.chars() {
        match ch {
            '%' => out.push_str(r"\%"),
            '_' => out.push_str(r"\_"),
            '*' => out.push('%'),
            '?' => out.push('_'),
            c => out.push(c),
        }
    }
    out
}

// Re-exported so existing `tools::observability::resolve_namespace` imports keep working.
// The shared definition lives in `crate::tools` (tools/mod.rs) since every tool module
// resolves per-call namespaces the same way (issue #96).
pub use crate::tools::resolve_namespace;

// ── US1: view_locks ───────────────────────────────────────────────────────────

pub async fn view_locks_impl(iris: Option<&IrisConnection>) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = iris
        .client(crate::iris::http::ClientKind::Call)
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;
    // %SYS.LockQuery has no SQL projection — use the named class query
    let code = r#"Set tRS=##class(%ResultSet).%New("%SYS.LockQuery:Detail")
Set tSC=tRS.Execute()
If $$$ISERR(tSC) { Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }
While tRS.Next() {
  Write tRS.GetData(1),"|",tRS.GetData(2),"|",tRS.GetData(3),"|",tRS.GetData(4),"|",tRS.GetData(5),!
}"#;
    match iris.execute_via_generator(code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if is_generator_error(out) {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            let locks: Vec<serde_json::Value> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let p: Vec<&str> = line.splitn(5, '|').collect();
                    serde_json::json!({
                        "resource":       p.first().copied().unwrap_or(""),
                        "owner_pid":      p.get(1).copied().unwrap_or(""),
                        "lock_type":      p.get(2).copied().unwrap_or(""),
                        "lock_mode":      p.get(3).copied().unwrap_or(""),
                        "owner_username": p.get(4).copied().unwrap_or(""),
                    })
                })
                .collect();
            let count = locks.len();
            ok_json(serde_json::json!({"success": true, "locks": locks, "count": count}))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── US2: view_processes ───────────────────────────────────────────────────────

pub async fn view_processes_impl(
    iris: Option<&IrisConnection>,
    data_policy: &str,
    namespace_filter: Option<&str>,
) -> Result<CallToolResult, McpError> {
    if data_policy == "block" {
        if let Some(blocked) = require_data_policy_allow("block", "view_processes") {
            return blocked;
        }
    }
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = iris
        .client(crate::iris::http::ClientKind::Call)
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;
    let where_clause = namespace_filter
        .filter(|s| !s.is_empty())
        .map(|ns| format!(" WHERE NameSpace = '{}'", ns.replace('\'', "''")))
        .unwrap_or_default();
    let sql = format!(
        "SELECT Pid, UserName, NameSpace, State, ClientNodeName, ClientIPAddress, Routine \
         FROM %SYS.ProcessQuery{} ORDER BY Pid",
        where_clause
    );
    match iris.query(&sql, vec![], "%SYS", &client).await {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mut processes: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "pid":             r["Pid"],
                        "username":        r["UserName"],
                        "namespace":       r["NameSpace"],
                        "state":           r["State"],
                        "client_node_name": r["ClientNodeName"],
                        "client_ip":       r["ClientIPAddress"],
                        "routine":         r["Routine"],
                    })
                })
                .collect();
            if data_policy == "redact" {
                for entry in &mut processes {
                    redact_process_entry(entry);
                }
            }
            let count = processes.len();
            ok_json(serde_json::json!({"success": true, "processes": processes, "count": count}))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── US3: journal_search ───────────────────────────────────────────────────────

/// The global name a journal record's reference points at, as the blocklist gate reads it: the
/// extended forms `^|"<dir>"|` and `^["<dir>"]` are stripped, then the subscripts and the leading caret (spec 141 C8).
pub fn journal_global_name(global_ref: &str) -> String {
    let mut name = global_ref;
    if let Some(rest) = name.strip_prefix("^|") {
        // `^|"<dir>"|name`: skip the quoted directory, then the bar after it.
        name = rest
            .strip_prefix('"')
            .and_then(|q| q.find('"').map(|end| &q[end + 1..]))
            .and_then(|after| after.strip_prefix('|'))
            .unwrap_or(rest);
    } else if let Some(rest) = name.strip_prefix("^[") {
        // `^["<dir>"]name`, the form IRIS journals.
        name = rest.split_once(']').map_or(rest, |(_, after)| after);
    }
    let bare = name.split('(').next().unwrap_or("");
    bare.strip_prefix('^').unwrap_or(bare).to_string()
}

/// `true` when a journaled value must be withheld: the global is on the system blocklist or the
/// server's `globalBlocklist` (no bypass), or it is PHI-named and `acknowledge_phi` is not set.
pub fn journal_value_withheld(
    global_ref: &str,
    global_blocklist: &[String],
    acknowledge_phi: bool,
) -> bool {
    let name = journal_global_name(global_ref);
    if matches_any(&name, SYSTEM_BLOCKLIST) || matches_any_owned(&name, global_blocklist) {
        return true;
    }
    !acknowledge_phi && matches_any(&name, PHI_NAME_PATTERNS)
}

/// The most journal records one search reads. A search that reaches it stops and says so
/// (`truncated: true`, `truncated_reason: "scan_cap"`) instead of reading to the end of a journal
/// that can hold hundreds of millions.
pub const JOURNAL_SCAN_CAP: u64 = 200_000;

/// The longest one search walks, in seconds. It stays under the 30 s HTTP timeout, so the answer
/// that comes back says how far the walk got instead of an `IRIS_UNREACHABLE` timeout.
pub const JOURNAL_TIME_CAP_SECS: u64 = 20;

/// How far one search may read. [`JOURNAL_SCAN_CAP`] and [`JOURNAL_TIME_CAP_SECS`] by default;
/// a test narrows them to reach a cap without a journal of that size.
#[derive(Debug, Clone, Copy)]
pub struct JournalWalkLimits {
    pub scan_cap: u64,
    pub time_cap_secs: u64,
}

impl Default for JournalWalkLimits {
    fn default() -> Self {
        Self {
            scan_cap: JOURNAL_SCAN_CAP,
            time_cap_secs: JOURNAL_TIME_CAP_SECS,
        }
    }
}

/// What `journal_search` was asked for, before the policy and filter checks.
#[derive(Debug, Clone, Copy, Default)]
pub struct JournalQuery<'a> {
    pub global_pattern: Option<&'a str>,
    pub time_range: Option<&'a serde_json::Value>,
    pub max_records: Option<u64>,
}

pub async fn journal_search_impl(
    iris: Option<&IrisConnection>,
    data_policy: &str,
    global_pattern: Option<&str>,
    time_range: Option<&serde_json::Value>,
    max_records: Option<u64>,
    global_blocklist: &[String],
    acknowledge_phi: bool,
) -> Result<CallToolResult, McpError> {
    let query = JournalQuery {
        global_pattern,
        time_range,
        max_records,
    };
    journal_search_with_limits(
        iris,
        data_policy,
        query,
        global_blocklist,
        acknowledge_phi,
        JournalWalkLimits::default(),
    )
    .await
}

/// [`journal_search_impl`] with the walk limits given. The live test for the caps narrows them.
pub async fn journal_search_with_limits(
    iris: Option<&IrisConnection>,
    data_policy: &str,
    query: JournalQuery<'_>,
    global_blocklist: &[String],
    acknowledge_phi: bool,
    limits: JournalWalkLimits,
) -> Result<CallToolResult, McpError> {
    let JournalQuery {
        global_pattern,
        time_range,
        max_records,
    } = query;
    // At least one filter required
    if global_pattern.filter(|s| !s.is_empty()).is_none() && time_range.is_none() {
        return err_json(
            "MISSING_PARAMS",
            "journal_search requires at least one filter: global_pattern or time_range",
        );
    }
    // Hard-block unless dataPolicy=allow — acknowledgePhi does NOT bypass
    if data_policy != "allow" {
        return err_json(
            "DATA_POLICY_BLOCKED",
            "journal_search is a bulk-PHI action. dataPolicy=allow is required. \
             acknowledgePhi does not bypass this block.",
        );
    }
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let cap: u64 = max_records.unwrap_or(100).min(1000);

    // The walk matches the term as a literal substring of the global reference, so glob
    // wildcard characters are stripped from the pattern.
    let search_term = global_pattern
        .filter(|s| !s.is_empty())
        .map(|s| s.replace(['*', '?'], ""))
        .unwrap_or_default();
    let from_ts = time_range
        .and_then(|tr| tr.get("from"))
        .and_then(|v| v.as_str())
        .map(iso8601_to_iris_timestamp)
        .unwrap_or_default();
    let to_ts = time_range
        .and_then(|tr| tr.get("to"))
        .and_then(|v| v.as_str())
        .map(iso8601_to_iris_timestamp)
        .unwrap_or_default();

    let walk = match journal_walk(
        iris,
        &WalkQuery {
            term: &search_term,
            from: &from_ts,
            to: &to_ts,
            cap,
        },
        limits,
    )
    .await
    {
        Ok(walk) => walk,
        Err(e) => return e.into_result(),
    };

    let records: Vec<serde_json::Value> = walk
        .lines
        .iter()
        .map(|line| {
            let withheld =
                journal_value_withheld(&line.global_ref, global_blocklist, acknowledge_phi);
            serde_json::json!({
                "global_ref":      line.global_ref,
                "transaction_id":  line.process_id,
                "operation":       line.operation,
                "timestamp":       line.timestamp,
                "value":           if withheld { "" } else { line.value.as_str() },
                "redacted":        withheld,
            })
        })
        .collect();
    ok_json(serde_json::json!({
        "success": true,
        "count": records.len(),
        "records": records,
        "truncated": walk.stop.is_some(),
        "truncated_reason": walk.stop,
        "scanned": walk.scanned,
    }))
}

/// The standalone `journal_search` tool: the same bounded walk, answered as `entries` with no
/// values. `start` and `end` are ISO 8601, `global_pattern` is a literal substring of the global
/// reference, and `max_entries` is clamped to 1 to 500.
pub async fn journal_entries_impl(
    iris: &IrisConnection,
    start: Option<&str>,
    end: Option<&str>,
    global_pattern: Option<&str>,
    max_entries: u32,
) -> Result<CallToolResult, McpError> {
    let from_ts = start.map(iso8601_to_iris_timestamp).unwrap_or_default();
    let to_ts = end.map(iso8601_to_iris_timestamp).unwrap_or_default();
    let walk = match journal_walk(
        iris,
        &WalkQuery {
            term: global_pattern.unwrap_or_default(),
            from: &from_ts,
            to: &to_ts,
            cap: u64::from(max_entries.clamp(1, 500)),
        },
        JournalWalkLimits::default(),
    )
    .await
    {
        Ok(walk) => walk,
        Err(e) => return e.into_result(),
    };
    let entries: Vec<serde_json::Value> = walk
        .lines
        .iter()
        .map(|line| {
            serde_json::json!({
                "timestamp": line.timestamp,
                "type": line.operation,
                "job_id": line.process_id.parse::<i64>().unwrap_or(0),
                "global": line.global_ref,
            })
        })
        .collect();
    ok_json(serde_json::json!({
        "success": true,
        "returned": entries.len(),
        "entries": entries,
        "truncated": walk.stop.is_some(),
        "truncated_reason": walk.stop,
        "scanned": walk.scanned,
    }))
}

/// What one walk looks for. `from` and `to` are IRIS timestamps, empty when unset.
struct WalkQuery<'a> {
    term: &'a str,
    from: &'a str,
    to: &'a str,
    cap: u64,
}

/// One journal record as the walk writes it: `global|process|operation|timestamp|value`.
struct JournalLine {
    global_ref: String,
    process_id: String,
    operation: String,
    timestamp: String,
    value: String,
}

struct JournalWalk {
    lines: Vec<JournalLine>,
    scanned: u64,
    /// Why the walk ended before the end of the journal: `max_records`, `scan_cap` or `time_cap`.
    stop: Option<&'static str>,
}

enum JournalWalkError {
    /// A `from` or `to` that is not a timestamp; carries the refusal body.
    Refused(serde_json::Value),
    /// The generator answered with an error.
    Execute(String),
    /// The request outran the HTTP timeout; carries the journal file and its size.
    Timeout(Option<String>, Option<u64>),
    Unreachable(String),
}

impl JournalWalkError {
    fn into_result(self) -> Result<CallToolResult, McpError> {
        match self {
            Self::Refused(body) => crate::tools::err_result(body),
            Self::Execute(msg) => err_json("IRIS_EXECUTE_ERROR", &msg),
            Self::Timeout(file, size) => {
                crate::tools::err_result(journal_search_timeout_body(file.as_deref(), size))
            }
            Self::Unreachable(msg) => err_json("IRIS_UNREACHABLE", &msg),
        }
    }
}

/// Walk the current journal file from its last record back, in the generator.
///
/// `%SYS.Journal.File:Search` is not used: it writes one `[Utility.Event] thisrec:` line to
/// `messages.log` for every record it visits, about 500,000 lines for one search of a busy
/// journal, which rolled the log over and stopped `iris-main`. `LastRecordGet()` and `Prev` log
/// nothing. The match, the time filter and the three stops (`max_records`, [`JournalWalkLimits`])
/// are in the generated code. The term is `tIadArgs.%Get(0)`, sent in `X-IAD-Args`: a search term
/// is often a secret, and scratch-class source is journaled (spec 141 T176).
///
/// Timestamps compare with `]` (follows), not `<`: `<` is numeric in ObjectScript, so two
/// timestamps in the same year compare equal. The walk goes newest first, because what is searched
/// for is usually recent and a busy file holds more records than the scan cap: walking from the
/// start, a search for the last hour found nothing on a file of a day's test runs. A record before
/// `from` ends the walk, and that end is not a truncation.
async fn journal_walk(
    iris: &IrisConnection,
    query: &WalkQuery<'_>,
    limits: JournalWalkLimits,
) -> Result<JournalWalk, JournalWalkError> {
    for ts in [query.from, query.to] {
        if !ts.is_empty() {
            if let Err(e) = crate::objectscript::validate_timestamp(ts) {
                return Err(JournalWalkError::Refused(e));
            }
        }
    }
    let client = iris
        .client(crate::iris::http::ClientKind::Call)
        .map_err(|e| JournalWalkError::Unreachable(e.to_string()))?;
    let code = format!(
        r#"Set tFile=##class(%SYS.Journal.System).GetCurrentFile()
Set tRec=tFile.LastRecordGet()
Set tTerm=tIadArgs.%Get(0)
// splice-ok: from and to passed validate_timestamp, the caps are integers
Set tFrom="{from}",tTo="{to}"
Set tScanned=0,tCount=0,tStop="",tStart=$ZH
While $IsObject(tRec) {{
  If tScanned>={scan_cap} {{ Set tStop="scan_cap" Quit }}
  If ($ZH-tStart)>={time_cap} {{ Set tStop="time_cap" Quit }}
  Set tScanned=tScanned+1
  Set tTS=tRec.TimeStamp
  If (tFrom'="")&&(tFrom]tTS) {{ Quit }}
  Set tKeep=1
  If (tTo'="")&&(tTS]tTo) {{ Set tKeep=0 }}
  If tKeep&&(tTerm'="") {{ Set tKeep=tRec.%IsA("%SYS.Journal.SetKillRecord")&&(tRec.GlobalReference[tTerm) }}
  If tKeep {{
    If tCount>={cap} {{ Set tStop="max_records" Quit }}
    Set tGlobalRef="" Set tValue=""
    If tRec.%IsA("%SYS.Journal.SetKillRecord") {{
      Set tGlobalRef=tRec.GlobalReference
      Set tValue=tRec.NewValue
    }}
    Write tGlobalRef,"|",tRec.ProcessID,"|",tRec.TypeName,"|",tTS,"|",tValue,!
    Set tCount=tCount+1
  }}
  Set tRec=tRec.Prev
}}
Write "COUNT:",tCount,!,"SCANNED:",tScanned,!,"STOP:",tStop,!"#,
        from = query.from,
        to = query.to,
        cap = query.cap,
        scan_cap = limits.scan_cap,
        time_cap = limits.time_cap_secs,
    );
    let call = GeneratorCall {
        code,
        args: vec![Secret::new(query.term.to_string())],
    };
    match iris
        .execute_via_generator_with_args(&call, "%SYS", &client)
        .await
    {
        Ok(out) => {
            let out = out.trim();
            if is_generator_error(out) {
                return Err(JournalWalkError::Execute(out.to_string()));
            }
            Ok(parse_journal_walk(out))
        }
        Err(e) if is_request_timeout(&e) => {
            let (file, size) = journal_file_and_size(iris, &client).await;
            Err(JournalWalkError::Timeout(file, size))
        }
        Err(e) => Err(JournalWalkError::Unreachable(e.to_string())),
    }
}

/// Read the lines `journal_walk` writes: record lines, then `COUNT:`, `SCANNED:` and `STOP:`.
fn parse_journal_walk(out: &str) -> JournalWalk {
    let mut walk = JournalWalk {
        lines: Vec::new(),
        scanned: 0,
        stop: None,
    };
    for line in out.lines() {
        if line.starts_with("COUNT:") {
            continue;
        } else if let Some(n) = line.strip_prefix("SCANNED:") {
            walk.scanned = n.trim().parse().unwrap_or(0);
        } else if let Some(reason) = line.strip_prefix("STOP:") {
            walk.stop = match reason.trim() {
                "max_records" => Some("max_records"),
                "scan_cap" => Some("scan_cap"),
                "time_cap" => Some("time_cap"),
                _ => None,
            };
        } else if !line.is_empty() {
            let p: Vec<&str> = line.splitn(5, '|').collect();
            let field = |i: usize| p.get(i).copied().unwrap_or("").to_string();
            walk.lines.push(JournalLine {
                global_ref: field(0),
                process_id: field(1),
                operation: field(2),
                timestamp: field(3),
                value: field(4),
            });
        }
    }
    walk
}

/// The body of a `journal_search` that outran the HTTP timeout. It names the journal file and its
/// size (`None` when the lookup could not answer) and suggests a narrower `time_range`, because a
/// search over the whole current journal is what runs long (spec 141 T174).
pub fn journal_search_timeout_body(
    file: Option<&str>,
    size_bytes: Option<u64>,
) -> serde_json::Value {
    let file = file.unwrap_or("unknown");
    let size = size_bytes.map_or_else(
        || "unknown size".to_string(),
        |b| format!("{} MB", b / 1_048_576),
    );
    serde_json::json!({
        "success": false,
        "error_code": "JOURNAL_SEARCH_TIMEOUT",
        "error": format!(
            "journal_search timed out reading the current journal file {file} ({size}). \
             Narrow the search with a shorter time_range (for example the last hour), then widen it."
        ),
    })
}

/// The current journal file and its size in bytes, for the timeout message. Best effort: a lookup
/// that fails yields `(None, None)` and the timeout is still reported.
async fn journal_file_and_size(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> (Option<String>, Option<u64>) {
    let code = "Set tFile=##class(%SYS.Journal.System).GetCurrentFileName()\n\
                Write tFile,\"|\",##class(%File).GetFileSize(tFile),!";
    let Ok(out) = iris.execute_via_generator(code, "%SYS", client).await else {
        return (None, None);
    };
    let mut parts = out.trim().splitn(2, '|');
    let file = parts.next().filter(|f| !f.is_empty()).map(str::to_string);
    let size = parts.next().and_then(|s| s.trim().parse::<u64>().ok());
    (file, size)
}

/// Convert an ISO 8601 timestamp ("2026-06-29T10:00:00Z") to IRIS %TimeStamp
/// format ("2026-06-29 10:00:00") for string comparison against journal record
/// TimeStamp values. Returns empty string for empty/unparseable input.
pub fn iso8601_to_iris_timestamp(iso: &str) -> String {
    let trimmed = iso.trim().trim_end_matches('Z');
    trimmed.replacen('T', " ", 1)
}

// ── US4: namespace_mappings ───────────────────────────────────────────────────

pub async fn namespace_mappings_impl(
    iris: Option<&IrisConnection>,
    namespace_param: Option<&str>,
    connection_ns: &str,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let ns = resolve_namespace(namespace_param, connection_ns);
    let client = iris
        .client(crate::iris::http::ClientKind::Call)
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;

    // Check namespace exists
    let exists_sql = "SELECT Name FROM Config.Namespaces WHERE Name = ?";
    match iris
        .query(exists_sql, vec![serde_json::json!(ns)], "%SYS", &client)
        .await
    {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            if rows == 0 {
                return err_json(
                    "NAMESPACE_NOT_FOUND",
                    &format!("Namespace '{ns}' does not exist on this IRIS instance"),
                );
            }
        }
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    }

    // Query the three mapping tables (all use column name "Database")
    let map_globals = "SELECT Name, Database FROM Config.MapGlobals WHERE Namespace = ?";
    let map_packages = "SELECT Name, Database FROM Config.MapPackages WHERE Namespace = ?";
    let map_routines = "SELECT Name, Database FROM Config.MapRoutines WHERE Namespace = ?";

    fn rows_to_mappings(resp: &serde_json::Value) -> Vec<serde_json::Value> {
        resp["result"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|r| serde_json::json!({"name": r["Name"], "database": r["Database"]}))
            .collect()
    }

    let globals = match iris
        .query(map_globals, vec![serde_json::json!(ns)], "%SYS", &client)
        .await
    {
        Ok(r) => rows_to_mappings(&r),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    let packages = match iris
        .query(map_packages, vec![serde_json::json!(ns)], "%SYS", &client)
        .await
    {
        Ok(r) => rows_to_mappings(&r),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    let routines = match iris
        .query(map_routines, vec![serde_json::json!(ns)], "%SYS", &client)
        .await
    {
        Ok(r) => rows_to_mappings(&r),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };

    ok_json(serde_json::json!({
        "success": true,
        "namespace": ns,
        "mappings": {
            "globals":   globals,
            "packages":  packages,
            "routines":  routines,
        }
    }))
}

// ── US5: database_status ──────────────────────────────────────────────────────

pub async fn database_status_impl(
    iris: Option<&IrisConnection>,
    name_filter: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = iris
        .client(crate::iris::http::ClientKind::Call)
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;

    // SYS.Database has no SQL projection — use SYS.Database:FreeSpace named query.
    // Execute("*") = all databases; Execute(name) = single database filter.
    let arg_lit =
        crate::objectscript::os_lit_inner(name_filter.filter(|s| !s.is_empty()).unwrap_or("*"));
    let code = format!(
        r#"Set tRS=##class(%ResultSet).%New("SYS.Database:FreeSpace")
Set tSC=tRS.Execute("{arg_lit}")
If $$$ISERR(tSC) {{ Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }}
While tRS.Next() {{
  Set tMirror="none"
  Set tDB=##class(SYS.Database).%OpenId(tRS.Get("Directory"))
  If $IsObject(tDB) {{ Set:tDB.Mirrored'=0 tMirror=tDB.MirrorSetName }}
  Write tRS.Get("DatabaseName"),"|",tRS.Get("Directory"),"|",tRS.Get("Status"),"|",
        tRS.Get("AvailableNum"),"|",tRS.Get("ReadOnly"),"|",tMirror,!
}}"#,
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if is_generator_error(out) {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            let databases: Vec<serde_json::Value> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let p: Vec<&str> = line.splitn(6, '|').collect();
                    let status = p.get(2).copied().unwrap_or("");
                    let free_mb: f64 = p.get(3).and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
                    serde_json::json!({
                        "name":          p.first().copied().unwrap_or(""),
                        "directory":     p.get(1).copied().unwrap_or(""),
                        "mounted":       status.contains("Mounted"),
                        "status":        status,
                        "free_space_mb": free_mb,
                        "read_only":     p.get(4).copied().unwrap_or("0") != "0",
                        "mirror_state":  p.get(5).copied().unwrap_or("none"),
                    })
                })
                .collect();
            if databases.is_empty() && name_filter.filter(|s| !s.is_empty()).is_some() {
                return err_json(
                    "DATABASE_NOT_FOUND",
                    &format!("Database '{}' not found", name_filter.unwrap_or("")),
                );
            }
            let count = databases.len();
            ok_json(serde_json::json!({"success": true, "databases": databases, "count": count}))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}
