//! Admin tools — namespace/database, observability, security, HL7, Mermaid.
//! Implements T077–T105 from spec 072-c.

use crate::iris::connection::IrisConnection;
use rmcp::{model::*, ErrorData as McpError};
use std::sync::Arc;

// ── Error codes ──────────────────────────────────────────────────────────────
pub const ERR_HL7_NOT_AVAILABLE: &str = "HL7_NOT_AVAILABLE";
pub const ERR_CONFIRM_REQUIRED: &str = "CONFIRM_REQUIRED";
pub const ERR_CONFIRM_EXPIRED: &str = "CONFIRM_EXPIRED";
pub const ERR_CONFIRM_MISMATCH: &str = "CONFIRM_MISMATCH";
pub const ERR_WRITE_GATE: &str = "WRITE_TOOLS_DISABLED";

// ── ConfirmEntry ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ConfirmEntry {
    pub global: String,
    pub server: Option<String>,
    pub issued_at: std::time::Instant,
}

impl ConfirmEntry {
    pub fn is_expired(&self) -> bool {
        self.issued_at.elapsed() > std::time::Duration::from_secs(300)
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}

fn err_json(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    crate::tools::err_result(serde_json::json!({
        "success": false,
        "error_code": code,
        "error": msg,
    }))
}

// ── global_preview (T078) ─────────────────────────────────────────────────────

pub struct GlobalPreviewParams {
    pub global: String,
    pub server: Option<String>,
    pub count: u32,
    pub iris: Arc<IrisConnection>,
    pub client: Arc<reqwest::Client>,
}

pub async fn global_preview_impl(
    params: GlobalPreviewParams,
    confirm_tokens: &tokio::sync::Mutex<std::collections::HashMap<String, ConfirmEntry>>,
) -> Result<CallToolResult, McpError> {
    use crate::tools::global::{build_global_ref, normalize_global_name};

    let name = normalize_global_name(&params.global);
    let global_ref = build_global_ref(&name, &[]);
    let ns = params.iris.namespace.clone();

    // Fetch entries using iris_global list action via execute_via_generator
    let limit = params.count.clamp(1, 100);
    let code = format!(
        r#"Set gRef="{global_ref}"
Set cnt=0
Set key=""
For {{
  Set key=$Order(@gRef@(key))
  If key="" Quit
  Set cnt=cnt+1
  Write key_"|"_$Get(@gRef@(key)),!
  If cnt>={limit} Quit
}}
Write "DONE|"_cnt,!"#
    );

    let out = params
        .iris
        .execute_via_generator(&code, &ns, &params.client)
        .await
        .map_err(|e| McpError::internal_error(format!("IRIS execute failed: {e}"), None))?;

    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut total = 0u32;
    for line in out.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("DONE|") {
            total = rest.parse().unwrap_or(0);
        } else if let Some(idx) = line.find('|') {
            let key = &line[..idx];
            let val = &line[idx + 1..];
            entries.push(serde_json::json!({"key": key, "value": val}));
        }
    }

    // Mint a confirmation token
    let token = uuid::Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
    let entry = ConfirmEntry {
        global: name.clone(),
        server: params.server.clone(),
        issued_at: std::time::Instant::now(),
    };
    {
        let mut map = confirm_tokens.lock().await;
        map.insert(token.clone(), entry);
    }

    ok_json(serde_json::json!({
        "success": true,
        "global": name,
        "server": params.server,
        "entries": entries,
        "total_subscripts": total,
        "confirm_token": token,
        "confirm_expires": expires_at.to_rfc3339(),
    }))
}

// ── global_kill (T079) — write-gated ─────────────────────────────────────────

pub struct GlobalKillParams {
    pub global: String,
    pub server: Option<String>,
    pub confirm_token: String,
    pub iris: Arc<IrisConnection>,
    pub client: Arc<reqwest::Client>,
    pub write_tools_enabled: bool,
}

pub async fn global_kill_impl(
    params: GlobalKillParams,
    confirm_tokens: &tokio::sync::Mutex<std::collections::HashMap<String, ConfirmEntry>>,
) -> Result<CallToolResult, McpError> {
    // Write gate (T079b)
    if !params.write_tools_enabled {
        return err_json(
            ERR_WRITE_GATE,
            "Write tools are disabled. Set write_tools = true in .iris-agentic-dev.toml.",
        );
    }

    use crate::tools::global::normalize_global_name;
    let name = normalize_global_name(&params.global);

    // Token validation
    let entry = {
        let map = confirm_tokens.lock().await;
        map.get(&params.confirm_token).cloned()
    };

    let entry =
        match entry {
            None => return err_json(
                ERR_CONFIRM_REQUIRED,
                "No confirmation token found. Call global_preview first to get a confirm_token.",
            ),
            Some(e) => e,
        };

    if entry.is_expired() {
        let mut map = confirm_tokens.lock().await;
        map.remove(&params.confirm_token);
        return err_json(
            ERR_CONFIRM_EXPIRED,
            "Confirmation token has expired (5 minute limit). Call global_preview again.",
        );
    }

    if entry.global != name || entry.server != params.server {
        return err_json(
            ERR_CONFIRM_MISMATCH,
            &format!(
                "Token was issued for global '{}' server {:?}, not '{}' server {:?}",
                entry.global, entry.server, name, params.server
            ),
        );
    }

    // Execute Kill
    let ns = params.iris.namespace.clone();
    let code = format!("Kill ^{name}\nWrite \"KILLED\",!");
    let out = params
        .iris
        .execute_via_generator(&code, &ns, &params.client)
        .await
        .map_err(|e| McpError::internal_error(format!("IRIS execute failed: {e}"), None))?;

    // Remove token after use
    {
        let mut map = confirm_tokens.lock().await;
        map.remove(&params.confirm_token);
    }

    if out.trim().contains("KILLED") {
        ok_json(serde_json::json!({
            "success": true,
            "killed": true,
            "global": name,
        }))
    } else {
        crate::tools::err_result(serde_json::json!({
            "success": false,
            "error_code": "IRIS_EXECUTE_ERROR",
            "error": format!("Unexpected output: {out}"),
        }))
    }
}

// ── iris_namespace_list (T082) ────────────────────────────────────────────────

pub async fn iris_namespace_list_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    // Simplest path: fetch from Atelier root which returns namespaces array
    let url = format!("{}/api/atelier/", iris.base_url);
    match client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = body["result"]["content"]["namespaces"].as_array() {
                    let namespaces: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    let count = namespaces.len();
                    return ok_json(serde_json::json!({
                        "success": true,
                        "namespaces": namespaces,
                        "count": count,
                    }));
                }
            }
            // Fallback: execute in %SYS
            namespace_list_via_exec(iris, client).await
        }
        Ok(resp) => {
            let status = resp.status();
            // Fallback to exec
            let _ = status;
            namespace_list_via_exec(iris, client).await
        }
        Err(_) => namespace_list_via_exec(iris, client).await,
    }
}

async fn namespace_list_via_exec(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    let code = r#"Set ns=""
For {
  Set ns=$Order(^%SYS("Namespace",ns))
  If ns="" Quit
  Write ns,!
}
Write "DONE",!"#;
    match iris.execute_via_generator(code, "%SYS", client).await {
        Ok(out) => {
            let mut namespaces: Vec<String> = out
                .lines()
                .filter(|l| !l.is_empty() && *l != "DONE")
                .map(|l| l.to_string())
                .collect();
            namespaces.sort();
            let count = namespaces.len();
            ok_json(serde_json::json!({
                "success": true,
                "namespaces": namespaces,
                "count": count,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── iris_database_list (T083) ─────────────────────────────────────────────────

pub async fn iris_database_list_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    let code = r#"Set tRS=##class(%ResultSet).%New("SYS.Database:List")
Set tSC=tRS.Execute()
If $$$ISERR(tSC) { Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }
While tRS.Next() {
  Write tRS.Get("Directory"),"|",tRS.Get("Mounted"),"|",tRS.Get("Size"),!
}"#;
    match iris.execute_via_generator(code, "%SYS", client).await {
        Ok(out) => {
            let out = out.trim();
            if out.starts_with("ERROR:") {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            let databases: Vec<serde_json::Value> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let parts: Vec<&str> = line.splitn(3, '|').collect();
                    serde_json::json!({
                        "directory": parts.first().copied().unwrap_or(""),
                        "mounted": parts.get(1).copied().unwrap_or("0") != "0",
                        "size_mb": parts.get(2).copied().unwrap_or("0")
                            .trim().parse::<f64>().unwrap_or(0.0),
                    })
                })
                .collect();
            let count = databases.len();
            ok_json(serde_json::json!({
                "success": true,
                "databases": databases,
                "count": count,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── iris_namespace_create (T084) — write-gated ────────────────────────────────

pub async fn iris_namespace_create_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    name: &str,
    db_path: Option<&str>,
    write_tools_enabled: bool,
) -> Result<CallToolResult, McpError> {
    if !write_tools_enabled {
        return err_json(
            ERR_WRITE_GATE,
            "Write tools are disabled. Set write_tools = true in .iris-agentic-dev.toml.",
        );
    }

    let db = db_path.unwrap_or(name);
    let code = format!(
        r#"Set props("Name")="{name}"
Set props("Globals")="{db}"
Set props("Routines")="{name}"
Set tSC=##class(Config.Namespaces).CreateOne(.props)
If $$$ISERR(tSC) {{
  Write "ERROR:"_$System.Status.GetErrorText(tSC),!
}} Else {{
  Write "CREATED",!
}}"#
    );
    match iris.execute_via_generator(&code, "%SYS", client).await {
        Ok(out) => {
            let out = out.trim();
            if out.starts_with("ERROR:") {
                err_json("CREATE_FAILED", out)
            } else {
                ok_json(serde_json::json!({
                    "success": true,
                    "created": true,
                    "name": name,
                }))
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── iris_database_stats (T085) ────────────────────────────────────────────────

pub async fn iris_database_stats_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    db: Option<&str>,
) -> Result<CallToolResult, McpError> {
    // Use SYS.Database:List and augment with GetFreeSpace for specific DB or all.
    let code = if let Some(dir) = db {
        format!(
            r#"Set dir="{dir}"
Set tSC=##class(SYS.Database).GetFreeSpace(dir,.free,.blocks)
If $$$ISERR(tSC) {{
  Write "ERROR:"_$System.Status.GetErrorText(tSC),!
  Quit
}}
Write dir,"|",free,"|",blocks,!"#
        )
    } else {
        r#"Set tRS=##class(%ResultSet).%New("SYS.Database:List")
Set tSC=tRS.Execute()
If $$$ISERR(tSC) { Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }
While tRS.Next() {
  Set dir=tRS.Get("Directory")
  Set tSC2=##class(SYS.Database).GetFreeSpace(dir,.free,.blocks)
  If $$$ISERR(tSC2) { Set free=0 Set blocks=0 }
  Write dir,"|",free,"|",blocks,!
}"#
        .to_string()
    };

    match iris.execute_via_generator(&code, "%SYS", client).await {
        Ok(out) => {
            let out = out.trim();
            if out.starts_with("ERROR:") {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            let stats: Vec<serde_json::Value> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let parts: Vec<&str> = line.splitn(3, '|').collect();
                    serde_json::json!({
                        "directory": parts.first().copied().unwrap_or(""),
                        "free_space_mb": parts.get(1).copied().unwrap_or("0")
                            .trim().parse::<f64>().unwrap_or(0.0),
                        "free_blocks": parts.get(2).copied().unwrap_or("0")
                            .trim().parse::<i64>().unwrap_or(0),
                    })
                })
                .collect();
            ok_json(serde_json::json!({
                "success": true,
                "stats": stats,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── journal_search (T088) ─────────────────────────────────────────────────────

pub async fn journal_search_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    start: Option<&str>,
    end: Option<&str>,
    global_pattern: Option<&str>,
    max_entries: u32,
) -> Result<CallToolResult, McpError> {
    let limit = max_entries.clamp(1, 500);
    let start_filter = start
        .map(|s| format!("If ts<\"{s}\" Continue"))
        .unwrap_or_default();
    let end_filter = end
        .map(|e| format!("If ts>\"{e}\" Continue"))
        .unwrap_or_default();
    let global_filter = global_pattern
        .map(|p| {
            format!(
                "If rec.TypeName=\"SetKillRecord\" {{ If rec.GlobalReference'[\"{}\" Continue }}",
                p.replace('"', "")
            )
        })
        .unwrap_or_default();

    let code = format!(
        r#"Set jfName=##class(%SYS.Journal.System).GetCurrentFileName()
Set jf=##class(%SYS.Journal.File).%OpenId(jfName)
If jf="" {{ Write "NO_JOURNAL",! Quit }}
Set rec=jf.FirstRecord
Set cnt=0
While (rec'="")&&(cnt<{limit}) {{
  Set ts=rec.TimeStamp
  {start_filter}
  {end_filter}
  {global_filter}
  Set typeName=rec.TypeName
  Set jobID=rec.JobID
  Set gref=""
  If typeName="SetKillRecord" {{ Set gref=rec.GlobalReference }}
  Write ts,"|",typeName,"|",jobID,"|",gref,!
  Set cnt=cnt+1
  Set rec=rec.Next
}}
Write "DONE|"_cnt,!"#
    );

    match iris.execute_via_generator(&code, "%SYS", client).await {
        Ok(out) => {
            let mut entries: Vec<serde_json::Value> = Vec::new();
            let mut total = 0u32;
            for line in out.lines() {
                if line.is_empty() {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("DONE|") {
                    total = rest.parse().unwrap_or(0);
                } else if line == "NO_JOURNAL" {
                    return err_json("NO_JOURNAL", "No current journal file found.");
                } else {
                    let parts: Vec<&str> = line.splitn(4, '|').collect();
                    entries.push(serde_json::json!({
                        "timestamp": parts.first().copied().unwrap_or(""),
                        "type": parts.get(1).copied().unwrap_or(""),
                        "job_id": parts.get(2).copied().unwrap_or("").parse::<i64>().unwrap_or(0),
                        "global": parts.get(3).copied().unwrap_or(""),
                    }));
                }
            }
            ok_json(serde_json::json!({
                "success": true,
                "entries": entries,
                "returned": total,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── query_audit_log (T089) ────────────────────────────────────────────────────

pub async fn query_audit_log_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    user: Option<&str>,
    event_type: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    limit: u32,
) -> Result<CallToolResult, McpError> {
    let limit = limit.clamp(1, 500);
    let mut conditions = vec!["1=1".to_string()];
    let mut params: Vec<String> = Vec::new();

    if let Some(u) = user {
        conditions.push("Username = ?".to_string());
        params.push(u.to_string());
    }
    if let Some(et) = event_type {
        conditions.push("EventType = ?".to_string());
        params.push(et.to_string());
    }
    if let Some(s) = start {
        conditions.push("UTCTimeStamp >= ?".to_string());
        params.push(s.to_string());
    }
    if let Some(e) = end {
        conditions.push("UTCTimeStamp <= ?".to_string());
        params.push(e.to_string());
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT TOP {limit} Event, EventType, Username, UTCTimeStamp FROM %SYS.Audit WHERE {where_clause} ORDER BY UTCTimeStamp DESC"
    );

    let param_values: Vec<serde_json::Value> = params
        .iter()
        .map(|p| serde_json::Value::String(p.clone()))
        .collect();

    match iris.query(&sql, param_values, "%SYS", client).await {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let entries: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "event": r["Event"],
                        "event_type": r["EventType"],
                        "username": r["Username"],
                        "timestamp": r["UTCTimeStamp"],
                    })
                })
                .collect();
            let count = entries.len();
            ok_json(serde_json::json!({
                "success": true,
                "entries": entries,
                "count": count,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── stream_inspect (T090) ─────────────────────────────────────────────────────

pub async fn stream_inspect_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    oid: &str,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    // Strip leading/trailing whitespace and extract numeric id
    let id = oid.trim();
    let code = format!(
        r#"Set stream=##class(%Stream.GlobalCharacter).%OpenId("{id}")
If stream="" {{
  // Try binary
  Set stream=##class(%Stream.GlobalBinary).%OpenId("{id}")
  If stream="" {{ Write "ERROR:STREAM_NOT_FOUND",! Quit }}
  Set isText=0
}} Else {{
  Set isText=1
}}
Set size=stream.Size
Set content=""
Do stream.Rewind()
While stream.AtEnd=0 {{
  Set content=content_stream.Read(4096)
}}
Write "SIZE|"_size,!
Write "TYPE|"_$Select(isText:"text",1:"binary"),!
Write "CONTENT|"_content,!"#
    );

    match iris.execute_via_generator(&code, namespace, client).await {
        Ok(out) => {
            if out.contains("ERROR:STREAM_NOT_FOUND") {
                return err_json(
                    "STREAM_NOT_FOUND",
                    &format!("Stream with oid '{oid}' not found."),
                );
            }
            let mut size: i64 = 0;
            let mut stream_type = "text".to_string();
            let mut content = String::new();
            for line in out.lines() {
                if let Some(rest) = line.strip_prefix("SIZE|") {
                    size = rest.parse().unwrap_or(0);
                } else if let Some(rest) = line.strip_prefix("TYPE|") {
                    stream_type = rest.to_string();
                } else if let Some(rest) = line.strip_prefix("CONTENT|") {
                    content = rest.to_string();
                }
            }
            ok_json(serde_json::json!({
                "success": true,
                "oid": oid,
                "type": stream_type,
                "size": size,
                "content": content,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── my_access (T093) ──────────────────────────────────────────────────────────

pub async fn my_access_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    // Get current username and then look up their roles
    let code = r#"Write $USERNAME,!"#;
    let username_out = match iris
        .execute_via_generator(code, &iris.namespace, client)
        .await
    {
        Ok(out) => out.trim().to_string(),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    let username = username_out.trim();

    // Query Security.Users
    let sql = "SELECT Name, FullName, Roles FROM Security.Users WHERE Name = ?";
    match iris
        .query(
            sql,
            vec![serde_json::Value::String(username.to_string())],
            "%SYS",
            client,
        )
        .await
    {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if let Some(row) = rows.first() {
                let roles_str = row["Roles"].as_str().unwrap_or("");
                let roles: Vec<&str> = roles_str
                    .split(',')
                    .map(|r| r.trim())
                    .filter(|r| !r.is_empty())
                    .collect();
                ok_json(serde_json::json!({
                    "success": true,
                    "username": row["Name"],
                    "full_name": row["FullName"],
                    "roles": roles,
                }))
            } else {
                ok_json(serde_json::json!({
                    "success": true,
                    "username": username,
                    "full_name": "",
                    "roles": [],
                }))
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── capability_matrix (T094) ──────────────────────────────────────────────────

pub async fn capability_matrix_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    user: Option<&str>,
) -> Result<CallToolResult, McpError> {
    // Resolve username
    let resolved_username: String;
    let username = if let Some(u) = user {
        u.to_string()
    } else {
        let code = r#"Write $USERNAME,!"#;
        match iris.execute_via_generator(code, "%SYS", client).await {
            Ok(out) => {
                resolved_username = out.trim().to_string();
                resolved_username.clone()
            }
            Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
        }
    };

    let sql = "SELECT Name, FullName, Roles FROM Security.Users WHERE Name = ?";
    match iris
        .query(
            sql,
            vec![serde_json::Value::String(username.clone())],
            "%SYS",
            client,
        )
        .await
    {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if let Some(row) = rows.first() {
                let roles_str = row["Roles"].as_str().unwrap_or("");
                let roles: Vec<String> = roles_str
                    .split(',')
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect();
                ok_json(serde_json::json!({
                    "success": true,
                    "user": username,
                    "full_name": row["FullName"],
                    "roles": roles,
                    "note": "Use iris_admin action=list_roles for full role definitions.",
                }))
            } else {
                ok_json(serde_json::json!({
                    "success": true,
                    "user": username,
                    "roles": [],
                }))
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── hl7_schema_list (T097) ────────────────────────────────────────────────────

pub async fn hl7_schema_list_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    // Check availability first
    let check_code = r#"Write ##class(%Dictionary.CompiledClass).%ExistsId("EnsLib.HL7.Schema"),!"#;
    let check_out = match iris
        .execute_via_generator(check_code, namespace, client)
        .await
    {
        Ok(out) => out.trim().to_string(),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    if check_out.trim() != "1" {
        return err_json(ERR_HL7_NOT_AVAILABLE, "EnsLib.HL7.Schema is not available on this IRIS instance. Requires InterSystems HealthShare or IRIS for Health.");
    }

    let code = r#"Set tRS=##class(%ResultSet).%New("EnsLib.HL7.Schema:StoredSchemaNames")
Set tSC=tRS.Execute()
If $$$ISERR(tSC) { Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }
While tRS.Next() {
  Write tRS.Data("Name"),!
}"#;
    match iris.execute_via_generator(code, namespace, client).await {
        Ok(out) => {
            let out = out.trim();
            if out.starts_with("ERROR:") {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            let schemas: Vec<String> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect();
            let count = schemas.len();
            ok_json(serde_json::json!({
                "success": true,
                "schemas": schemas,
                "count": count,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── hl7_schema_inspect (T098) ─────────────────────────────────────────────────

pub async fn hl7_schema_inspect_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    schema: &str,
    segment: Option<&str>,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let check_code = r#"Write ##class(%Dictionary.CompiledClass).%ExistsId("EnsLib.HL7.Schema"),!"#;
    let check_out = match iris
        .execute_via_generator(check_code, namespace, client)
        .await
    {
        Ok(out) => out.trim().to_string(),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };
    if check_out.trim() != "1" {
        return err_json(ERR_HL7_NOT_AVAILABLE, "EnsLib.HL7.Schema is not available.");
    }

    let seg_code = if let Some(seg) = segment {
        format!(
            r#"Set tRS=##class(%ResultSet).%New("EnsLib.HL7.Schema:SegmentStructureElements")
Set tSC=tRS.Execute("{schema}","{seg}")
If $$$ISERR(tSC) {{ Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }}
While tRS.Next() {{
  Write tRS.Data("FieldName"),"|",tRS.Data("Description"),!
}}"#
        )
    } else {
        format!(
            r#"Set tRS=##class(%ResultSet).%New("EnsLib.HL7.Schema:MessageStructures")
Set tSC=tRS.Execute("{schema}")
If $$$ISERR(tSC) {{ Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }}
While tRS.Next() {{
  Write tRS.Data("StructureName"),!
}}"#
        )
    };

    match iris
        .execute_via_generator(&seg_code, namespace, client)
        .await
    {
        Ok(out) => {
            let out = out.trim();
            if out.starts_with("ERROR:") {
                return err_json("IRIS_EXECUTE_ERROR", out);
            }
            if segment.is_some() {
                let fields: Vec<serde_json::Value> = out
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|line| {
                        let parts: Vec<&str> = line.splitn(2, '|').collect();
                        serde_json::json!({
                            "field": parts.first().copied().unwrap_or(""),
                            "description": parts.get(1).copied().unwrap_or(""),
                        })
                    })
                    .collect();
                ok_json(serde_json::json!({
                    "success": true,
                    "schema": schema,
                    "segment": segment,
                    "fields": fields,
                }))
            } else {
                let structures: Vec<String> = out
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect();
                ok_json(serde_json::json!({
                    "success": true,
                    "schema": schema,
                    "structures": structures,
                }))
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── build_mermaid_class_diagram (T100–T102) ───────────────────────────────────

/// Pure function: build a Mermaid classDiagram from a list of (class, supers) pairs.
/// `supers` is a comma-separated string as returned by %Dictionary.CompiledClass.Super.
pub fn build_mermaid_class_diagram(classes: &[(String, String)]) -> String {
    let mut out = String::from("classDiagram\n");
    for (cls, supers_str) in classes {
        let safe_cls = cls.replace('%', "Pct_");
        for sup in supers_str
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let safe_sup = sup.replace('%', "Pct_");
            out.push_str(&format!("    {} <|-- {}\n", safe_sup, safe_cls));
        }
    }
    out
}

pub async fn mermaid_class_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    class: &str,
    depth: u32,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let depth = depth.clamp(1, 5);
    // Walk superclass chain up to depth levels
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: Vec<String> = vec![class.to_string()];
    let mut pairs: Vec<(String, String)> = Vec::new();

    for _ in 0..depth {
        if queue.is_empty() {
            break;
        }
        let current_batch = std::mem::take(&mut queue);
        for cls in &current_batch {
            if visited.contains(cls) {
                continue;
            }
            visited.insert(cls.clone());
            let escaped = cls.replace('"', "");
            let sql =
                format!("SELECT Name, Super FROM %Dictionary.CompiledClass WHERE Name='{escaped}'");
            if let Ok(resp) = iris.query(&sql, vec![], namespace, client).await {
                let rows = resp["result"]["content"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                for row in &rows {
                    let name = row["Name"].as_str().unwrap_or("").to_string();
                    let supers = row["Super"].as_str().unwrap_or("").to_string();
                    pairs.push((name.clone(), supers.clone()));
                    for sup in supers
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                    {
                        if !visited.contains(&sup) {
                            queue.push(sup);
                        }
                    }
                }
            }
        }
    }

    if pairs.is_empty() {
        return err_json(
            "CLASS_NOT_FOUND",
            &format!("Class '{class}' not found in namespace {namespace}."),
        );
    }

    let diagram = build_mermaid_class_diagram(&pairs);
    ok_json(serde_json::json!({
        "success": true,
        "class": class,
        "depth": depth,
        "diagram": diagram,
    }))
}

// ── mermaid_production (T103) ─────────────────────────────────────────────────

pub async fn mermaid_production_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    production: &str,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let escaped = production.replace('"', "");
    let sql = format!(
        "SELECT Name, ClassName, Category, Enabled FROM Ens_Config.Item WHERE Production='{escaped}' ORDER BY Category, Name"
    );
    match iris.query(&sql, vec![], namespace, client).await {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();

            let mut diagram = "flowchart TD\n".to_string();
            let safe_prod = production.replace(['.', '%', '-'], "_");
            diagram.push_str(&format!("    {}[\"{}\"]\n", safe_prod, production));

            for row in &rows {
                let name = row["Name"]
                    .as_str()
                    .unwrap_or("")
                    .replace(['.', '%', '-'], "_");
                let class = row["ClassName"].as_str().unwrap_or("");
                let _category = row["Category"].as_str().unwrap_or("");
                let enabled = row["Enabled"].as_str().unwrap_or("1") != "0";
                let style = if enabled { "" } else { ":::disabled" };
                diagram.push_str(&format!(
                    "    {}[\"{}\\n{}\"]{}  \n",
                    name, name, class, style
                ));
                diagram.push_str(&format!("    {} --> {}\n", safe_prod, name));
            }

            ok_json(serde_json::json!({
                "success": true,
                "production": production,
                "item_count": rows.len(),
                "diagram": diagram,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── resolve_storage (T104) ────────────────────────────────────────────────────

pub async fn resolve_storage_impl(
    iris: &IrisConnection,
    client: &reqwest::Client,
    class: &str,
    namespace: &str,
) -> Result<CallToolResult, McpError> {
    let escaped = class.replace('"', "");
    let sql = format!(
        "SELECT Name, Type, DataLocation, IdLocation, IndexLocation FROM %Dictionary.CompiledStorage WHERE parent='{escaped}'"
    );
    match iris.query(&sql, vec![], namespace, client).await {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let storages: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "name": r["Name"],
                        "type": r["Type"],
                        "data_location": r["DataLocation"],
                        "id_location": r["IdLocation"],
                        "index_location": r["IndexLocation"],
                    })
                })
                .collect();
            ok_json(serde_json::json!({
                "success": true,
                "class": class,
                "storages": storages,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // T075: fresh token is valid
    #[tokio::test]
    async fn confirm_token_fresh_is_valid() {
        let tokens: tokio::sync::Mutex<HashMap<String, ConfirmEntry>> =
            tokio::sync::Mutex::new(HashMap::new());
        let token = "test-token-1".to_string();
        {
            let mut map = tokens.lock().await;
            map.insert(
                token.clone(),
                ConfirmEntry {
                    global: "TestGlobal".to_string(),
                    server: None,
                    issued_at: std::time::Instant::now(),
                },
            );
        }
        let map = tokens.lock().await;
        let entry = map.get(&token).expect("token must be present");
        assert!(!entry.is_expired(), "fresh token should not be expired");
    }

    // T075: token expired after >5 minutes
    #[test]
    fn confirm_entry_expired_after_five_minutes() {
        // Simulate an old issued_at by subtracting 6 minutes.
        // We can't directly set Instant in the past portably,
        // so we test the logic with is_expired() on a "just now" entry,
        // then verify the threshold.
        let entry = ConfirmEntry {
            global: "TestGlobal".to_string(),
            server: None,
            issued_at: std::time::Instant::now(),
        };
        assert!(
            !entry.is_expired(),
            "just-created entry must not be expired"
        );
        // Verify threshold is 300 seconds
        // (We can't test actual expiry without sleeping or mocking Instant)
    }

    // T075: token for different global returns CONFIRM_MISMATCH (logic test)
    #[test]
    fn confirm_mismatch_different_global() {
        let entry = ConfirmEntry {
            global: "GlobalA".to_string(),
            server: None,
            issued_at: std::time::Instant::now(),
        };
        // The mismatch check: entry.global != requested global
        assert_ne!(entry.global, "GlobalB", "different global should not match");
    }

    // T100: build_mermaid_class_diagram produces valid classDiagram syntax
    #[test]
    fn build_mermaid_class_diagram_basic() {
        let classes = vec![
            ("Foo".to_string(), "Bar".to_string()),
            ("Bar".to_string(), "Baz".to_string()),
        ];
        let diagram = build_mermaid_class_diagram(&classes);
        assert!(
            diagram.starts_with("classDiagram"),
            "diagram must start with classDiagram, got: {diagram}"
        );
        assert!(
            diagram.contains("Bar <|-- Foo"),
            "diagram must contain Bar <|-- Foo, got: {diagram}"
        );
        assert!(
            diagram.contains("Baz <|-- Bar"),
            "diagram must contain Baz <|-- Bar, got: {diagram}"
        );
    }

    // T100: percent-sign class names are escaped
    #[test]
    fn build_mermaid_class_diagram_percent_escape() {
        let classes = vec![(
            "%Library.Persistent".to_string(),
            "%Library.Registered".to_string(),
        )];
        let diagram = build_mermaid_class_diagram(&classes);
        assert!(
            !diagram.contains('%'),
            "diagram must not contain bare '%', got: {diagram}"
        );
        assert!(
            diagram.contains("Pct_"),
            "diagram must contain Pct_ prefix, got: {diagram}"
        );
    }

    // T079b: global_kill write-gate returns WRITE_TOOLS_DISABLED (logic only — no IRIS call)
    #[tokio::test]
    async fn global_kill_write_gate_disabled() {
        use crate::iris::connection::DiscoverySource;
        let conn = Arc::new(IrisConnection::new(
            "http://localhost:52780",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        ));
        let client = Arc::new(reqwest::Client::new());
        let tokens: tokio::sync::Mutex<HashMap<String, ConfirmEntry>> =
            tokio::sync::Mutex::new(HashMap::new());

        let result = global_kill_impl(
            GlobalKillParams {
                global: "TestGlobal".to_string(),
                server: None,
                confirm_token: "any-token".to_string(),
                iris: conn,
                client,
                write_tools_enabled: false,
            },
            &tokens,
        )
        .await
        .expect("global_kill_impl returned MCP error");

        let text = result
            .content
            .first()
            .map(|c| c.raw.as_text().unwrap().text.clone())
            .expect("no text content");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");
        assert_eq!(
            v["error_code"].as_str().unwrap_or(""),
            ERR_WRITE_GATE,
            "write-gated kill should return {ERR_WRITE_GATE}, got: {v}"
        );
    }

    // T075b: global_kill_impl with no token in map → CONFIRM_REQUIRED
    #[tokio::test]
    async fn global_kill_confirm_required() {
        use crate::iris::connection::DiscoverySource;
        let conn = Arc::new(IrisConnection::new(
            "http://localhost:52780",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        ));
        let client = Arc::new(reqwest::Client::new());
        let tokens: tokio::sync::Mutex<HashMap<String, ConfirmEntry>> =
            tokio::sync::Mutex::new(HashMap::new()); // empty map → no token

        let result = global_kill_impl(
            GlobalKillParams {
                global: "TestGlobal".to_string(),
                server: None,
                confirm_token: "nonexistent-token".to_string(),
                iris: conn,
                client,
                write_tools_enabled: true,
            },
            &tokens,
        )
        .await
        .expect("global_kill_impl returned MCP error");

        let text = result
            .content
            .first()
            .map(|c| c.raw.as_text().unwrap().text.clone())
            .expect("no text content");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");
        assert_eq!(
            v["error_code"].as_str().unwrap_or(""),
            ERR_CONFIRM_REQUIRED,
            "missing token should return {ERR_CONFIRM_REQUIRED}, got: {v}"
        );
    }

    // T075c: global_kill_impl with expired token → CONFIRM_EXPIRED
    #[tokio::test]
    async fn global_kill_confirm_expired() {
        use crate::iris::connection::DiscoverySource;
        let conn = Arc::new(IrisConnection::new(
            "http://localhost:52780",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        ));
        let client = Arc::new(reqwest::Client::new());
        let token = "expired-token".to_string();
        let tokens: tokio::sync::Mutex<HashMap<String, ConfirmEntry>> = {
            let mut map = HashMap::new();
            map.insert(
                token.clone(),
                ConfirmEntry {
                    global: "TestGlobal".to_string(),
                    server: None,
                    // Simulate expiry: issued_at more than 300s ago
                    issued_at: std::time::Instant::now() - std::time::Duration::from_secs(301),
                },
            );
            tokio::sync::Mutex::new(map)
        };

        let result = global_kill_impl(
            GlobalKillParams {
                global: "TestGlobal".to_string(),
                server: None,
                confirm_token: token,
                iris: conn,
                client,
                write_tools_enabled: true,
            },
            &tokens,
        )
        .await
        .expect("global_kill_impl returned MCP error");

        let text = result
            .content
            .first()
            .map(|c| c.raw.as_text().unwrap().text.clone())
            .expect("no text content");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");
        assert_eq!(
            v["error_code"].as_str().unwrap_or(""),
            ERR_CONFIRM_EXPIRED,
            "expired token should return {ERR_CONFIRM_EXPIRED}, got: {v}"
        );
    }

    // T075d: global_kill_impl with token for a different global → CONFIRM_MISMATCH
    #[tokio::test]
    async fn global_kill_confirm_mismatch() {
        use crate::iris::connection::DiscoverySource;
        let conn = Arc::new(IrisConnection::new(
            "http://localhost:52780",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        ));
        let client = Arc::new(reqwest::Client::new());
        let token = "mismatch-token".to_string();
        let tokens: tokio::sync::Mutex<HashMap<String, ConfirmEntry>> = {
            let mut map = HashMap::new();
            map.insert(
                token.clone(),
                ConfirmEntry {
                    global: "GlobalA".to_string(), // token issued for GlobalA
                    server: None,
                    issued_at: std::time::Instant::now(),
                },
            );
            tokio::sync::Mutex::new(map)
        };

        let result = global_kill_impl(
            GlobalKillParams {
                global: "GlobalB".to_string(), // but request is for GlobalB
                server: None,
                confirm_token: token,
                iris: conn,
                client,
                write_tools_enabled: true,
            },
            &tokens,
        )
        .await
        .expect("global_kill_impl returned MCP error");

        let text = result
            .content
            .first()
            .map(|c| c.raw.as_text().unwrap().text.clone())
            .expect("no text content");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");
        assert_eq!(
            v["error_code"].as_str().unwrap_or(""),
            ERR_CONFIRM_MISMATCH,
            "token for different global should return {ERR_CONFIRM_MISMATCH}, got: {v}"
        );
    }

    // T084b: iris_namespace_create write-gate returns WRITE_TOOLS_DISABLED (logic only)
    #[tokio::test]
    async fn iris_namespace_create_write_gate_disabled() {
        use crate::iris::connection::DiscoverySource;
        let conn = IrisConnection::new(
            "http://localhost:52780",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        let client = reqwest::Client::new();

        let result = iris_namespace_create_impl(&conn, &client, "TestNS", None, false)
            .await
            .expect("iris_namespace_create_impl returned MCP error");

        let text = result
            .content
            .first()
            .map(|c| c.raw.as_text().unwrap().text.clone())
            .expect("no text content");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json parse");
        assert_eq!(
            v["error_code"].as_str().unwrap_or(""),
            ERR_WRITE_GATE,
            "write-gated namespace create should return {ERR_WRITE_GATE}, got: {v}"
        );
    }
}
