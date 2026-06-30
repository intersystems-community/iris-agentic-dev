//! IRIS administration tools — namespace, database, user, role, and webapp management.
//! All operations execute in the %SYS namespace via HTTP ObjectScript execution.
//! Read operations are always available; write operations require IRIS_ADMIN_TOOLS=1.

use crate::iris::connection::IrisConnection;
use rmcp::{model::*, ErrorData as McpError};

fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}
fn err_json(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    ok_json(serde_json::json!({"success": false, "error_code": code, "error": msg}))
}
fn iris_unreachable() -> McpError {
    McpError::invalid_request("IRIS_UNREACHABLE", None)
}

/// Returns true if write operations are permitted (IRIS_ADMIN_TOOLS=1 or true).
pub fn admin_write_allowed() -> bool {
    std::env::var("IRIS_ADMIN_TOOLS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn write_disabled() -> Result<CallToolResult, McpError> {
    err_json(
        "ADMIN_WRITE_DISABLED",
        "Set IRIS_ADMIN_TOOLS=1 to enable admin write operations.",
    )
}

// ── List namespaces ──────────────────────────────────────────────────────────

pub async fn admin_list_namespaces_impl(
    iris: Option<&IrisConnection>,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    match iris
        .query(
            "SELECT Name, Globals, Routines FROM Config.Namespaces ORDER BY Name",
            vec![],
            "%SYS",
            &client,
        )
        .await
    {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let namespaces: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "name": r["Name"],
                        "code_database": r["Routines"],
                        "data_database": r["Globals"],
                    })
                })
                .collect();
            let count = namespaces.len();
            ok_json(serde_json::json!({"success":true,"namespaces":namespaces,"count":count}))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── List databases ───────────────────────────────────────────────────────────

pub async fn admin_list_databases_impl(
    iris: Option<&IrisConnection>,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    // Use SYS.Database:List query (documented in IRIS class reference)
    let code = r#"Set tRS=##class(%ResultSet).%New("SYS.Database:List")
Set tSC=tRS.Execute()
If $$$ISERR(tSC) { Write "ERROR:"_$System.Status.GetErrorText(tSC) Quit }
While tRS.Next() {
  Write tRS.Get("Directory"),"|",tRS.Get("Mounted"),"|",tRS.Get("Size"),"|",tRS.Get("MaxSize"),!
}"#;
    match iris.execute_via_generator(code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out.starts_with("ERROR:") {
                return err_json("INTEROP_ERROR", out);
            }
            let databases: Vec<serde_json::Value> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let parts: Vec<&str> = line.splitn(4, '|').collect();
                    serde_json::json!({
                        "directory": parts.first().copied().unwrap_or(""),
                        "mounted": parts.get(1).copied().unwrap_or("0") != "0",
                        "size_mb": parts.get(2).copied().unwrap_or("0").trim().parse::<f64>().unwrap_or(0.0),
                        "max_size_mb": parts.get(3).copied().unwrap_or("0").trim().parse::<f64>().unwrap_or(0.0),
                    })
                })
                .collect();
            let count = databases.len();
            ok_json(serde_json::json!({"success":true,"databases":databases,"count":count}))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── List users ───────────────────────────────────────────────────────────────

pub async fn admin_list_users_impl(
    iris: Option<&IrisConnection>,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    // Fetch all to get total_count; password never exposed
    match iris
        .query(
            "SELECT Name, FullName, Enabled, Roles FROM Security.Users ORDER BY Name",
            vec![],
            "%SYS",
            &client,
        )
        .await
    {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let total = rows.len();
            let truncated = total > 100;
            let users: Vec<serde_json::Value> = rows
                .into_iter()
                .take(100)
                .map(|r| {
                    serde_json::json!({
                        "name": r["Name"],
                        "full_name": r["FullName"],
                        "enabled": r["Enabled"],
                        "roles": r["Roles"],
                    })
                })
                .collect();
            let count = users.len();
            ok_json(serde_json::json!({
                "success": true,
                "users": users,
                "count": count,
                "truncated": truncated,
                "total_count": total,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── List roles ───────────────────────────────────────────────────────────────

pub async fn admin_list_roles_impl(
    iris: Option<&IrisConnection>,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    match iris
        .query(
            "SELECT Name, Description FROM Security.Roles ORDER BY Name",
            vec![],
            "%SYS",
            &client,
        )
        .await
    {
        Ok(resp) => {
            let rows = resp["result"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let total = rows.len();
            let truncated = total > 100;
            let roles: Vec<serde_json::Value> = rows
                .into_iter()
                .take(100)
                .map(|r| serde_json::json!({"name": r["Name"], "description": r["Description"]}))
                .collect();
            let count = roles.len();
            ok_json(serde_json::json!({
                "success": true,
                "roles": roles,
                "count": count,
                "truncated": truncated,
                "total_count": total,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── List webapps ─────────────────────────────────────────────────────────────

pub async fn admin_list_webapps_impl(
    iris: Option<&IrisConnection>,
    type_filter: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    // Try with Type column first; fall back to without if needed
    let resp = iris
        .query(
            "SELECT Name, NameSpace, DispatchClass, Enabled, Type FROM Security.Applications ORDER BY Name",
            vec![],
            "%SYS",
            &client,
        )
        .await;
    let rows = match resp {
        Ok(r) => r["result"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        Err(e) => return err_json("IRIS_UNREACHABLE", &e.to_string()),
    };

    let mut webapps: Vec<serde_json::Value> = rows
        .into_iter()
        .filter_map(|r| {
            // Determine type: use Type field if numeric, else infer from DispatchClass
            let type_val = match r["Type"].as_i64() {
                Some(1) => "REST",
                Some(0) => "CSP",
                _ => {
                    if r["DispatchClass"]
                        .as_str()
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
                    {
                        "REST"
                    } else {
                        "CSP"
                    }
                }
            };
            // Apply filter
            if let Some(filter) = type_filter {
                if !type_val.eq_ignore_ascii_case(filter) {
                    return None;
                }
            }
            Some(serde_json::json!({
                "path": r["Name"],
                "namespace": r["NameSpace"],
                "dispatch_class": r["DispatchClass"],
                "enabled": r["Enabled"],
                "type": type_val,
            }))
        })
        .collect();

    let filtered_total = webapps.len();
    let truncated = filtered_total > 100;
    webapps.truncate(100);
    let count = webapps.len();
    ok_json(serde_json::json!({
        "success": true,
        "webapps": webapps,
        "count": count,
        "truncated": truncated,
        "total_count": filtered_total,
    }))
}

// ── List user roles ──────────────────────────────────────────────────────────

pub async fn admin_list_user_roles_impl(
    iris: Option<&IrisConnection>,
    username: &str,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let un = username.replace('\'', "''");
    let code = format!(
        r#"Set tSC=##class(Security.Users).Get("{}",.props)
If $$$ISERR(tSC) {{ Write "ERROR:USER_NOT_FOUND:User not found: {}" Quit }}
Write $GET(props("Roles"))"#,
        un, un
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if let Some(msg) = out.strip_prefix("ERROR:USER_NOT_FOUND:") {
                return err_json("USER_NOT_FOUND", msg);
            }
            let roles: Vec<&str> = if out.is_empty() {
                vec![]
            } else {
                out.split(',').map(|r| r.trim()).collect()
            };
            ok_json(serde_json::json!({"success":true,"username":username,"roles":roles}))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Get webapp ───────────────────────────────────────────────────────────────

pub async fn admin_get_webapp_impl(
    iris: Option<&IrisConnection>,
    path: &str,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let p = path.replace('\'', "''");
    let code = format!(
        r#"Set tSC=##class(Security.Applications).Get("{}",.props)
If $$$ISERR(tSC) {{ Write "ERROR:WEBAPP_NOT_FOUND:Webapp not found: {}" Quit }}
Set dc=$GET(props("DispatchClass"))
Set ns=$GET(props("NameSpace"))
Set en=$GET(props("Enabled"))
Set tp=$SELECT(dc'="":"REST",1:"CSP")
Write ns_"|"_dc_"|"_en_"|"_tp"#,
        p, p
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if let Some(msg) = out.strip_prefix("ERROR:WEBAPP_NOT_FOUND:") {
                return err_json("WEBAPP_NOT_FOUND", msg);
            }
            let parts: Vec<&str> = out.splitn(4, '|').collect();
            if parts.len() < 4 {
                return err_json("INTEROP_ERROR", "unexpected response from IRIS");
            }
            ok_json(serde_json::json!({
                "success": true,
                "path": path,
                "namespace": parts[0],
                "dispatch_class": parts[1],
                "enabled": parts[2] != "0",
                "type": parts[3],
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Check permission ─────────────────────────────────────────────────────────

pub async fn admin_check_permission_impl(
    iris: Option<&IrisConnection>,
    resource: &str,
    permission: &str,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    // Map common permission names to IRIS operation codes
    let perm_upper = permission.to_uppercase();
    let op = match perm_upper.as_str() {
        "USE" | "U" => "U",
        "WRITE" | "W" => "W",
        "READ" | "R" => "R",
        "CREATE" | "C" => "C",
        "DELETE" | "D" => "D",
        other => other,
    };
    let res = resource.replace('\'', "''");
    let op_escaped = op.replace('\'', "''");
    // %SYSTEM.Security.Check(ResourceName, Permissions) — documented IRIS API
    let code = format!(
        r#"If ##class(%SYSTEM.Security).Check("{}","{}") {{ Write "GRANTED" }} Else {{ Write "DENIED" }}"#,
        res, op_escaped
    );
    // Get the current username from env for reporting
    let current_user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let granted = out.trim() == "GRANTED";
            ok_json(serde_json::json!({
                "success": true,
                "resource": resource,
                "permission": permission,
                "granted": granted,
                "user": current_user,
            }))
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Write: create user ────────────────────────────────────────────────────────

pub async fn admin_create_user_impl(
    iris: Option<&IrisConnection>,
    username: &str,
    password: &str,
    full_name: Option<&str>,
    roles: Option<&str>,
) -> Result<CallToolResult, McpError> {
    if !admin_write_allowed() {
        return write_disabled();
    }
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let un = username.replace('\'', "''");
    let pw = password.replace('\'', "''");
    let fn_ = full_name.unwrap_or("").replace('\'', "''");
    let ro = roles.unwrap_or("").replace('\'', "''");
    let code = format!(
        r#"Set props("Password")="{}"
Set props("FullName")="{}"
Set props("Roles")="{}"
Set tSC=##class(Security.Users).Create("{}",.props)
If $$$ISERR(tSC) {{ Write "ERROR:USER_EXISTS:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
        pw, fn_, ro, un
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out == "OK" {
                ok_json(
                    serde_json::json!({"success":true,"action":"create_user","username":username}),
                )
            } else if let Some(msg) = out.strip_prefix("ERROR:USER_EXISTS:") {
                err_json("USER_EXISTS", msg)
            } else {
                err_json("INTEROP_ERROR", out)
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Write: update user ────────────────────────────────────────────────────────

pub async fn admin_update_user_impl(
    iris: Option<&IrisConnection>,
    username: &str,
    password: Option<&str>,
    enabled: Option<bool>,
    roles: Option<&str>,
) -> Result<CallToolResult, McpError> {
    if !admin_write_allowed() {
        return write_disabled();
    }
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let un = username.replace('\'', "''");
    let mut set_lines = String::new();
    if let Some(pw) = password {
        set_lines.push_str(&format!(
            "Set props(\"Password\")=\"{}\"\n",
            pw.replace('\'', "''")
        ));
    }
    if let Some(en) = enabled {
        set_lines.push_str(&format!(
            "Set props(\"Enabled\")={}\n",
            if en { 1 } else { 0 }
        ));
    }
    if let Some(ro) = roles {
        set_lines.push_str(&format!(
            "Set props(\"Roles\")=\"{}\"\n",
            ro.replace('\'', "''")
        ));
    }
    let code = format!(
        r#"Set tSC=##class(Security.Users).Get("{}",.props)
If $$$ISERR(tSC) {{ Write "ERROR:USER_NOT_FOUND:User not found: {}" Quit }}
{}Set tSC2=##class(Security.Users).Modify("{}",.props)
If $$$ISERR(tSC2) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC2) }} Else {{ Write "OK" }}"#,
        un, un, set_lines, un
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out == "OK" {
                ok_json(
                    serde_json::json!({"success":true,"action":"update_user","username":username}),
                )
            } else if let Some(msg) = out.strip_prefix("ERROR:USER_NOT_FOUND:") {
                err_json("USER_NOT_FOUND", msg)
            } else {
                err_json("INTEROP_ERROR", out)
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Write: delete user ────────────────────────────────────────────────────────

pub async fn admin_delete_user_impl(
    iris: Option<&IrisConnection>,
    username: &str,
) -> Result<CallToolResult, McpError> {
    if !admin_write_allowed() {
        return write_disabled();
    }
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let un = username.replace('\'', "''");
    let code = format!(
        r#"If '##class(Security.Users).Exists("{}") {{ Write "ERROR:USER_NOT_FOUND:User not found: {}" Quit }}
Set tSC=##class(Security.Users).Delete("{}")
If $$$ISERR(tSC) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
        un, un, un
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out == "OK" {
                ok_json(
                    serde_json::json!({"success":true,"action":"delete_user","username":username}),
                )
            } else if let Some(msg) = out.strip_prefix("ERROR:USER_NOT_FOUND:") {
                err_json("USER_NOT_FOUND", msg)
            } else {
                err_json("INTEROP_ERROR", out)
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Write: create namespace ───────────────────────────────────────────────────

pub async fn admin_create_namespace_impl(
    iris: Option<&IrisConnection>,
    name: &str,
    code_database: &str,
    data_database: &str,
) -> Result<CallToolResult, McpError> {
    if !admin_write_allowed() {
        return write_disabled();
    }
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let nm = name.replace('\'', "''");
    let cd = code_database.replace('\'', "''");
    let dd = data_database.replace('\'', "''");
    let code = format!(
        r#"Set ns("Globals")="{}"
Set ns("Routines")="{}"
Set ns("Database")="{}"
Set tSC=##class(Config.Namespaces).Create("{}",.ns)
If $$$ISERR(tSC) {{ Write "ERROR:NAMESPACE_EXISTS:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
        dd, cd, dd, nm
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out == "OK" {
                ok_json(serde_json::json!({"success":true,"action":"create_namespace","name":name}))
            } else if let Some(msg) = out.strip_prefix("ERROR:NAMESPACE_EXISTS:") {
                err_json("NAMESPACE_EXISTS", msg)
            } else {
                err_json("INTEROP_ERROR", out)
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Write: delete namespace ───────────────────────────────────────────────────

pub async fn admin_delete_namespace_impl(
    iris: Option<&IrisConnection>,
    name: &str,
) -> Result<CallToolResult, McpError> {
    if !admin_write_allowed() {
        return write_disabled();
    }
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let nm = name.replace('\'', "''");
    // Deletes namespace definition only — databases are NOT deleted (by design)
    let code = format!(
        r#"If '##class(Config.Namespaces).Exists("{}") {{ Write "ERROR:NAMESPACE_NOT_FOUND:Namespace not found: {}" Quit }}
Set tSC=##class(Config.Namespaces).Delete("{}")
If $$$ISERR(tSC) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
        nm, nm, nm
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out == "OK" {
                ok_json(serde_json::json!({"success":true,"action":"delete_namespace","name":name}))
            } else if let Some(msg) = out.strip_prefix("ERROR:NAMESPACE_NOT_FOUND:") {
                err_json("NAMESPACE_NOT_FOUND", msg)
            } else {
                err_json("INTEROP_ERROR", out)
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Write: create webapp ──────────────────────────────────────────────────────

pub async fn admin_create_webapp_impl(
    iris: Option<&IrisConnection>,
    path: &str,
    namespace: &str,
    dispatch_class: Option<&str>,
    enabled: bool,
) -> Result<CallToolResult, McpError> {
    if !admin_write_allowed() {
        return write_disabled();
    }
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let p = path.replace('\'', "''");
    let ns = namespace.replace('\'', "''");
    let dc = dispatch_class.unwrap_or("").replace('\'', "''");
    let en = if enabled { 1 } else { 0 };
    let code = format!(
        r#"Set props("NameSpace")="{}"
Set props("DispatchClass")="{}"
Set props("Enabled")={}
Set props("AutheEnabled")=32
Set tSC=##class(Security.Applications).Create("{}",.props)
If $$$ISERR(tSC) {{ Write "ERROR:WEBAPP_EXISTS:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
        ns, dc, en, p
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out == "OK" {
                ok_json(serde_json::json!({"success":true,"action":"create_webapp","path":path}))
            } else if let Some(msg) = out.strip_prefix("ERROR:WEBAPP_EXISTS:") {
                err_json("WEBAPP_EXISTS", msg)
            } else {
                err_json("INTEROP_ERROR", out)
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

// ── Write: delete webapp ──────────────────────────────────────────────────────

pub async fn admin_delete_webapp_impl(
    iris: Option<&IrisConnection>,
    path: &str,
) -> Result<CallToolResult, McpError> {
    if !admin_write_allowed() {
        return write_disabled();
    }
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let p = path.replace('\'', "''");
    let code = format!(
        r#"Set tSC=##class(Security.Applications).Get("{}",.props)
If $$$ISERR(tSC) {{ Write "ERROR:WEBAPP_NOT_FOUND:Webapp not found: {}" Quit }}
Set tSC2=##class(Security.Applications).Delete("{}")
If $$$ISERR(tSC2) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC2) }} Else {{ Write "OK" }}"#,
        p, p, p
    );
    match iris.execute_via_generator(&code, "%SYS", &client).await {
        Ok(out) => {
            let out = out.trim();
            if out == "OK" {
                ok_json(serde_json::json!({"success":true,"action":"delete_webapp","path":path}))
            } else if let Some(msg) = out.strip_prefix("ERROR:WEBAPP_NOT_FOUND:") {
                err_json("WEBAPP_NOT_FOUND", msg)
            } else {
                err_json("INTEROP_ERROR", out)
            }
        }
        Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_admin_write_allowed_default_false() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("IRIS_ADMIN_TOOLS");
        assert!(!admin_write_allowed());
    }

    #[test]
    fn test_admin_write_allowed_one() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIS_ADMIN_TOOLS", "1");
        let result = admin_write_allowed();
        std::env::remove_var("IRIS_ADMIN_TOOLS");
        assert!(result);
    }

    #[test]
    fn test_admin_write_allowed_true_string() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIS_ADMIN_TOOLS", "true");
        let result = admin_write_allowed();
        std::env::remove_var("IRIS_ADMIN_TOOLS");
        assert!(result);
    }

    #[test]
    fn test_admin_write_allowed_true_upper() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIS_ADMIN_TOOLS", "TRUE");
        let result = admin_write_allowed();
        std::env::remove_var("IRIS_ADMIN_TOOLS");
        assert!(result);
    }

    #[test]
    fn test_admin_write_allowed_false_string() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIS_ADMIN_TOOLS", "false");
        let result = admin_write_allowed();
        std::env::remove_var("IRIS_ADMIN_TOOLS");
        assert!(!result);
    }

    #[test]
    fn test_admin_write_allowed_zero() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIS_ADMIN_TOOLS", "0");
        let result = admin_write_allowed();
        std::env::remove_var("IRIS_ADMIN_TOOLS");
        assert!(!result);
    }

    #[test]
    fn test_ok_json_shape() {
        let v = serde_json::json!({"success": true, "count": 3});
        let result = ok_json(v).unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("success"));
    }

    #[test]
    fn test_err_json_shape() {
        let result = err_json("NOT_FOUND", "Resource not found").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("NOT_FOUND"));
        assert!(text.contains("Resource not found"));
    }

    #[test]
    fn test_iris_unreachable_error_code() {
        let e = iris_unreachable();
        let msg = format!("{e:?}");
        assert!(msg.contains("IRIS_UNREACHABLE"), "iris_unreachable: {msg}");
    }

    #[test]
    fn test_write_disabled_response() {
        let result = write_disabled().unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "ADMIN_WRITE_DISABLED");
    }

    // ── Additional pure helper tests for line coverage ─────────────────────────

    #[test]
    fn test_ok_json_empty_object() {
        let v = serde_json::json!({});
        let result = ok_json(v).unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("{}"), "ok_json should preserve empty object");
    }

    #[test]
    fn test_ok_json_nested_structure() {
        let v = serde_json::json!({
            "success": true,
            "data": {
                "nested": "value",
                "count": 42
            }
        });
        let result = ok_json(v).unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["data"]["nested"], "value");
        assert_eq!(parsed["data"]["count"], 42);
    }

    #[test]
    fn test_ok_json_array() {
        let v = serde_json::json!([1, 2, 3, 4, 5]);
        let result = ok_json(v).unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 5);
    }

    #[test]
    fn test_err_json_code_and_message() {
        let result = err_json("TEST_ERROR", "This is a test error message").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "TEST_ERROR");
        assert_eq!(v["error"], "This is a test error message");
        assert_eq!(v["success"], false);
    }

    #[test]
    fn test_err_json_with_special_chars() {
        let result = err_json("ENCODE_TEST", "Error with \"quotes\" and 'apostrophes'").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "ENCODE_TEST");
        assert!(v["error"].as_str().unwrap().contains("quotes"));
    }

    #[test]
    fn test_err_json_empty_message() {
        let result = err_json("EMPTY_ERROR", "").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "EMPTY_ERROR");
        assert_eq!(v["error"], "");
        assert_eq!(v["success"], false);
    }

    #[test]
    fn test_iris_unreachable_error_format() {
        let err = iris_unreachable();
        let msg = format!("{:?}", err);
        assert!(msg.contains("IRIS_UNREACHABLE"));
    }

    #[test]
    fn test_write_disabled_has_correct_message() {
        let result = write_disabled().unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["success"], false);
        assert_eq!(v["error_code"], "ADMIN_WRITE_DISABLED");
        assert!(v["error"].as_str().unwrap().contains("IRIS_ADMIN_TOOLS"));
    }

    #[test]
    fn test_admin_write_allowed_various_invalid_values() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let invalid_values = vec!["false", "2", "yes", "no", "", "null"];
        for val in invalid_values {
            std::env::set_var("IRIS_ADMIN_TOOLS", val);
            assert!(
                !admin_write_allowed(),
                "Value '{}' should not allow writes",
                val
            );
        }
        std::env::remove_var("IRIS_ADMIN_TOOLS");
    }

    #[test]
    fn test_admin_write_allowed_true_mixed_case() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mixed_cases = vec!["True", "TRUE", "TrUe"];
        for val in mixed_cases {
            std::env::set_var("IRIS_ADMIN_TOOLS", val);
            assert!(admin_write_allowed(), "Value '{}' should allow writes", val);
        }
        std::env::remove_var("IRIS_ADMIN_TOOLS");
    }

    #[test]
    fn test_ok_json_number_values() {
        let v = serde_json::json!({
            "int_val": 42,
            "float_val": 3.14,
            "negative": -10,
            "zero": 0
        });
        let result = ok_json(v).unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["int_val"], 42);
        assert!(parsed["float_val"].as_f64().is_some());
        assert_eq!(parsed["negative"], -10);
        assert_eq!(parsed["zero"], 0);
    }

    #[test]
    fn test_ok_json_boolean_values() {
        let v = serde_json::json!({
            "enabled": true,
            "disabled": false
        });
        let result = ok_json(v).unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["enabled"], true);
        assert_eq!(parsed["disabled"], false);
    }

    #[test]
    fn test_err_json_multi_word_code() {
        let result = err_json("COMPLEX_ERROR_CODE_123", "Multi-code test").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "COMPLEX_ERROR_CODE_123");
    }

    #[test]
    fn test_err_json_unicode_message() {
        let result = err_json("UNICODE_ERROR", "Error with émojis and ümlaut").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "UNICODE_ERROR");
        assert!(v["error"].as_str().unwrap().contains("moji"));
    }

    #[test]
    fn test_response_content_structure() {
        let v = serde_json::json!({"test": "data"});
        let result = ok_json(v).unwrap();
        assert!(!result.content.is_empty());
        let content = &result.content[0];
        assert!(content.raw.as_text().is_some());
    }

    #[test]
    fn test_err_json_response_always_has_success_false() {
        let cases = vec![("CODE1", "msg1"), ("CODE2", "msg2"), ("", "")];
        for (code, msg) in cases {
            let result = err_json(code, msg).unwrap();
            let text = result.content[0].raw.as_text().unwrap().text.clone();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                v["success"], false,
                "err_json should always set success=false"
            );
        }
    }

    #[test]
    fn test_admin_write_allowed_unset_and_set_again() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("IRIS_ADMIN_TOOLS");
        assert!(!admin_write_allowed());
        std::env::set_var("IRIS_ADMIN_TOOLS", "1");
        assert!(admin_write_allowed());
        std::env::remove_var("IRIS_ADMIN_TOOLS");
        assert!(!admin_write_allowed());
    }
}
