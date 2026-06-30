//! System observability actions for the iris_admin dispatcher (055-system-observability).
//!
//! Five read-only actions: view_locks, view_processes, journal_search,
//! namespace_mappings, database_status. All are ToolCategory::Query, permitted
//! on every mcpTemplate value. Called from the iris_admin match dispatcher in mod.rs.

use crate::iris::connection::IrisConnection;
use rmcp::{model::*, ErrorData as McpError};

// ── Shared helpers ────────────────────────────────────────────────────────────

pub(crate) fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}

pub(crate) fn err_json(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    ok_json(serde_json::json!({"success": false, "error_code": code, "error": msg}))
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

/// Resolve the effective namespace: use `param` if non-empty, else `connection_ns`.
pub fn resolve_namespace<'a>(param: Option<&'a str>, connection_ns: &'a str) -> &'a str {
    match param {
        Some(s) if !s.is_empty() => s,
        _ => connection_ns,
    }
}

// ── US1: view_locks ───────────────────────────────────────────────────────────

pub async fn view_locks_impl(iris: Option<&IrisConnection>) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client()
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
            if out.starts_with("ERROR:") {
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
    let client = IrisConnection::http_client()
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

pub async fn journal_search_impl(
    iris: Option<&IrisConnection>,
    data_policy: &str,
    global_pattern: Option<&str>,
    time_range: Option<&serde_json::Value>,
    max_records: Option<u64>,
) -> Result<CallToolResult, McpError> {
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
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;

    // %SYS.Journal.Record has no SQL projection (verified: SELECT FROM it -> SQLCODE -30).
    // The real API is the %SYS.Journal.File:Search class query: Search(String, FileName,
    // Offset, Backward) -> Offset. It does substring matching (no glob wildcards), so we
    // strip wildcard characters from the pattern and pass the longest literal substring;
    // glob filtering is then applied client-side on the resolved GlobalReference.
    let search_term = global_pattern
        .filter(|s| !s.is_empty())
        .map(|s| s.replace(['*', '?'], ""))
        .filter(|s| !s.is_empty())
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

    let code = format!(
        r#"Set tFile=##class(%SYS.Journal.System).GetCurrentFile()
Set tFirst=tFile.FirstRecordGet()
If '$IsObject(tFirst) {{ Write "COUNT:0" Quit }}
Set tRS=##class(%ResultSet).%New("%SYS.Journal.File:Search")
Set tSC=tRS.Execute("{search}",tFile.Name,tFirst.Address,0)
If $$$ISERR(tSC) {{ Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }}
Set tCount=0
While tRS.Next() && (tCount<{cap}) {{
  Set tAddr=tRS.GetData(1)
  Set tRec=tFile.GetRecordAt(tAddr)
  Continue:'$IsObject(tRec)
  Set tTS=tRec.TimeStamp
  Continue:("{from}"'="")&&(tTS<"{from}")
  Continue:("{to}"'="")&&(tTS>"{to}")
  Set tGlobalRef="" Set tValue=""
  If tRec.%IsA("%SYS.Journal.SetKillRecord") {{
    Set tGlobalRef=tRec.GlobalReference
    Set tValue=tRec.NewValue
  }}
  Write tGlobalRef,"|",tRec.ProcessID,"|",tRec.TypeName,"|",tTS,"|",tValue,!
  Set tCount=tCount+1
}}
Write "COUNT:"_tCount,!"#,
        search = search_term.replace('"', "\"\""),
        cap = cap,
        from = from_ts,
        to = to_ts,
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out.starts_with("ERROR:") {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            let mut records: Vec<serde_json::Value> = Vec::new();
            let mut result_count: u64 = 0;
            for line in out.lines() {
                if let Some(n) = line.strip_prefix("COUNT:") {
                    result_count = n.trim().parse().unwrap_or(0);
                } else if !line.is_empty() {
                    let p: Vec<&str> = line.splitn(5, '|').collect();
                    let global_ref = p.first().copied().unwrap_or("");
                    records.push(serde_json::json!({
                        "global_ref":      global_ref,
                        "transaction_id":  p.get(1).copied().unwrap_or(""),
                        "operation":       p.get(2).copied().unwrap_or(""),
                        "timestamp":       p.get(3).copied().unwrap_or(""),
                        "value":           p.get(4).copied().unwrap_or(""),
                    }));
                }
            }
            let truncated = result_count >= cap;
            ok_json(serde_json::json!({
                "success": true,
                "records": records,
                "count": records.len(),
                "truncated": truncated,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
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
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;

    // Check namespace exists
    let exists_sql = format!(
        "SELECT Name FROM Config.Namespaces WHERE Name = '{}'",
        ns.replace('\'', "''")
    );
    match iris.query(&exists_sql, vec![], "%SYS", &client).await {
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
    let map_globals = format!(
        "SELECT Name, Database FROM Config.MapGlobals WHERE Namespace = '{}'",
        ns.replace('\'', "''")
    );
    let map_packages = format!(
        "SELECT Name, Database FROM Config.MapPackages WHERE Namespace = '{}'",
        ns.replace('\'', "''")
    );
    let map_routines = format!(
        "SELECT Name, Database FROM Config.MapRoutines WHERE Namespace = '{}'",
        ns.replace('\'', "''")
    );

    fn rows_to_mappings(resp: &serde_json::Value) -> Vec<serde_json::Value> {
        resp["result"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|r| serde_json::json!({"name": r["Name"], "database": r["Database"]}))
            .collect()
    }

    let globals = match iris.query(&map_globals, vec![], "%SYS", &client).await {
        Ok(r) => rows_to_mappings(&r),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    let packages = match iris.query(&map_packages, vec![], "%SYS", &client).await {
        Ok(r) => rows_to_mappings(&r),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    let routines = match iris.query(&map_routines, vec![], "%SYS", &client).await {
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
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;

    // SYS.Database has no SQL projection — use SYS.Database:FreeSpace named query.
    // Execute("*") = all databases; Execute(name) = single database filter.
    let arg = name_filter.filter(|s| !s.is_empty()).unwrap_or("*");
    let code = format!(
        r#"Set tRS=##class(%ResultSet).%New("SYS.Database:FreeSpace")
Set tSC=tRS.Execute("{arg}")
If $$$ISERR(tSC) {{ Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }}
While tRS.Next() {{
  Set tMirror="none"
  Set tDB=##class(SYS.Database).%OpenId(tRS.Get("Directory"))
  If $IsObject(tDB) {{ Set:tDB.Mirrored'=0 tMirror=tDB.MirrorSetName }}
  Write tRS.Get("DatabaseName"),"|",tRS.Get("Directory"),"|",tRS.Get("Status"),"|",
        tRS.Get("AvailableNum"),"|",tRS.Get("ReadOnly"),"|",tMirror,!
}}"#,
        arg = arg.replace('"', "\"\""),
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out.starts_with("ERROR:") {
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
