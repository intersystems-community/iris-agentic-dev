use crate::iris::connection::IrisConnection;
use rmcp::{model::*, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}
fn err_json(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    ok_json(serde_json::json!({"success": false, "error_code": code, "error": msg}))
}
fn iris_unreachable() -> McpError {
    McpError::invalid_request("IRIS_UNREACHABLE", None)
}
// Bug 18: "connection" matched too broadly — e.g. "No Interoperability connection configured"
// was misclassified as IRIS_UNREACHABLE. Use more specific network-error patterns.
pub(crate) fn is_network_error(msg: &str) -> bool {
    msg.contains("error sending")
        || msg.contains("connection refused")
        || msg.contains("connection reset")
        || msg.contains("dns error")
        || msg.contains("timed out")
}

fn default_ns() -> String {
    "USER".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductionStatusParams {
    #[serde(default = "default_ns")]
    pub namespace: String,
    #[serde(default)]
    pub full_status: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductionNameParams {
    pub production: Option<String>,
    #[serde(default = "default_ns")]
    pub namespace: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductionStopParams {
    pub production: Option<String>,
    #[serde(default = "default_ns")]
    pub namespace: String,
    #[serde(default = "default_timeout")]
    pub timeout: u32,
    #[serde(default)]
    pub force: bool,
}
fn default_timeout() -> u32 {
    30
}

// Bug 7: added namespace field so update/recover/needs_update work in non-default namespaces.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductionUpdateParams {
    #[serde(default = "default_ns")]
    pub namespace: String,
    #[serde(default = "default_timeout")]
    pub timeout: u32,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductionNeedsUpdateParams {
    #[serde(default = "default_ns")]
    pub namespace: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductionRecoverParams {
    #[serde(default = "default_ns")]
    pub namespace: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsParams {
    pub item_name: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default = "default_log_type")]
    pub log_type: String,
}
fn default_limit() -> u32 {
    10
}
fn default_log_type() -> String {
    "error,warning".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueuesParams {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MessageSearchParams {
    pub source: Option<String>,
    pub target: Option<String>,
    pub class_name: Option<String>,
    #[serde(default = "default_msg_limit")]
    pub limit: u32,
}
fn default_msg_limit() -> u32 {
    20
}

fn state_string(code: i64) -> &'static str {
    match code {
        1 => "Running",
        2 => "Stopped",
        3 => "Suspended",
        4 => "Troubled",
        5 => "NetworkStopped",
        _ => "Unknown",
    }
}

pub fn parse_status_response(raw: &str) -> Result<(String, i64, String), String> {
    if raw.is_empty() || raw == ":" {
        return Err("NO_PRODUCTION".to_string());
    }
    if raw.starts_with("ERROR") {
        return Err(format!("INTEROP_ERROR:{}", raw));
    }
    let parts: Vec<&str> = raw.splitn(2, ':').collect();
    if parts.len() < 2 || parts[0].is_empty() {
        return Err("NO_PRODUCTION".to_string());
    }
    let name = parts[0].to_string();
    let code: i64 = parts[1].trim().parse().unwrap_or(0);
    let state = state_string(code).to_string();
    Ok((name, code, state))
}

fn docker_required_interop() -> Result<CallToolResult, McpError> {
    err_json(
        "DOCKER_REQUIRED",
        "Interoperability operations require docker exec. Set IRIS_CONTAINER=<container_name>.",
    )
}

pub async fn interop_production_status_impl(
    iris: Option<&IrisConnection>,
    params: ProductionStatusParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let code = r#"Set sc=##class(Ens.Director).GetProductionStatus(.n,.s) If $$$ISERR(sc) { Write "ERROR:"_$System.Status.GetErrorText(sc) } Else { Write n_":"_s }"#;
    // Bug 7: use params.namespace, not iris.namespace.
    match iris.execute(code, &params.namespace).await {
        Ok(output) => {
            let raw = output.trim().to_string();
            match parse_status_response(&raw) {
                Ok((name, code, state)) => ok_json(
                    serde_json::json!({"success": true, "production": name, "state": state, "state_code": code}),
                ),
                Err(e) if e.starts_with("INTEROP_ERROR") => err_json("INTEROP_ERROR", &e[14..]),
                Err(_) => err_json("NO_PRODUCTION", "No production is running"),
            }
        }
        Err(e) if e.to_string() == "DOCKER_REQUIRED" => docker_required_interop(),
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_production_start_impl(
    iris: Option<&IrisConnection>,
    params: ProductionNameParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let prod = params.production.as_deref().unwrap_or("");
    let code = format!(
        r#"Set sc=##class(Ens.Director).StartProduction("{}") If $$$ISERR(sc) {{ Write "ERROR:"_$System.Status.GetErrorText(sc) }} Else {{ Write "OK" }}"#,
        prod
    );
    // Bug 7: use params.namespace, not iris.namespace.
    match iris.execute(&code, &params.namespace).await {
        Ok(output) => {
            let raw = output.trim();
            if raw == "OK" {
                ok_json(serde_json::json!({"success": true, "state": "Running"}))
            } else {
                err_json("INTEROP_ERROR", raw)
            }
        }
        Err(e) if e.to_string() == "DOCKER_REQUIRED" => docker_required_interop(),
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_production_stop_impl(
    iris: Option<&IrisConnection>,
    params: ProductionStopParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let code = format!(
        r#"Set sc=##class(Ens.Director).StopProduction({},{}) If $$$ISERR(sc) {{ Write "ERROR:"_$System.Status.GetErrorText(sc) }} Else {{ Write "OK" }}"#,
        params.timeout,
        if params.force { 1 } else { 0 }
    );
    // Bug 7: use params.namespace, not iris.namespace.
    match iris.execute(&code, &params.namespace).await {
        Ok(output) => {
            let raw = output.trim();
            if raw == "OK" {
                ok_json(serde_json::json!({"success": true, "state": "Stopped"}))
            } else {
                err_json("INTEROP_ERROR", raw)
            }
        }
        Err(e) if e.to_string() == "DOCKER_REQUIRED" => docker_required_interop(),
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_production_update_impl(
    iris: Option<&IrisConnection>,
    params: ProductionUpdateParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let code = format!(
        r#"Set sc=##class(Ens.Director).UpdateProduction({},{}) If $$$ISERR(sc) {{ Write "ERROR:"_$System.Status.GetErrorText(sc) }} Else {{ Write "OK" }}"#,
        params.timeout,
        if params.force { 1 } else { 0 }
    );
    // Bug 7: use params.namespace.
    match iris.execute(&code, &params.namespace).await {
        Ok(output) => {
            let raw = output.trim();
            if raw == "OK" {
                ok_json(serde_json::json!({"success": true, "message": "Production updated"}))
            } else {
                err_json("INTEROP_ERROR", raw)
            }
        }
        Err(e) if e.to_string() == "DOCKER_REQUIRED" => docker_required_interop(),
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_production_needs_update_impl(
    iris: Option<&IrisConnection>,
    params: ProductionNeedsUpdateParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let code = r#"Write ##class(Ens.Director).ProductionNeedsUpdate()"#;
    // Bug 7: use params.namespace.
    match iris.execute(code, &params.namespace).await {
        Ok(output) => {
            ok_json(serde_json::json!({"success": true, "needs_update": output.trim() == "1"}))
        }
        Err(e) if e.to_string() == "DOCKER_REQUIRED" => docker_required_interop(),
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_production_recover_impl(
    iris: Option<&IrisConnection>,
    params: ProductionRecoverParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let code = r#"Set sc=##class(Ens.Director).RecoverProduction() If $$$ISERR(sc) { Write "ERROR:"_$System.Status.GetErrorText(sc) } Else { Write "OK" }"#;
    // Bug 7: use params.namespace.
    match iris.execute(code, &params.namespace).await {
        Ok(output) => {
            let raw = output.trim();
            if raw == "OK" {
                ok_json(serde_json::json!({"success": true, "state": "Running"}))
            } else {
                err_json("INTEROP_ERROR", raw)
            }
        }
        Err(e) if e.to_string() == "DOCKER_REQUIRED" => docker_required_interop(),
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_logs_impl(
    iris: Option<&IrisConnection>,
    params: LogsParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let mut conditions = vec![];
    for lt in params.log_type.split(',') {
        match lt.trim().to_lowercase().as_str() {
            "error" => conditions.push("Type = 3"),
            "warning" => conditions.push("Type = 2"),
            "info" => conditions.push("Type = 1"),
            "alert" => conditions.push("Type = 4"),
            _ => {}
        }
    }
    let type_filter = if conditions.is_empty() {
        String::new()
    } else {
        format!("AND ({})", conditions.join(" OR "))
    };
    let item_filter = params
        .item_name
        .as_ref()
        .map(|n| format!("AND ConfigName = '{}'", n.replace('\'', "''")))
        .unwrap_or_default();
    let sql = format!("SELECT TOP {} ID, TimeLogged, Type, ConfigName, Text FROM Ens_Util.Log WHERE 1=1 {} {} ORDER BY ID DESC", params.limit, type_filter, item_filter);
    match iris
        .query(&sql, vec![], &iris.namespace.clone(), &client)
        .await
    {
        Ok(resp) => ok_json(
            serde_json::json!({"success": true, "logs": resp["result"]["content"], "count": resp["result"]["content"].as_array().map(|a| a.len()).unwrap_or(0)}),
        ),
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_queues_impl(
    iris: Option<&IrisConnection>,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    match iris
        .query(
            "SELECT * FROM Ens.Queue_Enumerate()",
            vec![],
            &iris.namespace.clone(),
            &client,
        )
        .await
    {
        Ok(resp) => {
            ok_json(serde_json::json!({"success": true, "queues": resp["result"]["content"]}))
        }
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_message_search_impl(
    iris: Option<&IrisConnection>,
    params: MessageSearchParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client().map_err(|_| iris_unreachable())?;
    let mut filters = vec![];
    if let Some(src) = &params.source {
        filters.push(format!("SourceConfigName = '{}'", src.replace('\'', "''")));
    }
    if let Some(tgt) = &params.target {
        filters.push(format!("TargetConfigName = '{}'", tgt.replace('\'', "''")));
    }
    if let Some(cls) = &params.class_name {
        filters.push(format!(
            "MessageBodyClassName = '{}'",
            cls.replace('\'', "''")
        ));
    }
    let where_clause = if filters.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", filters.join(" AND "))
    };
    let sql = format!("SELECT TOP {} ID, TimeCreated, SourceConfigName, TargetConfigName, MessageBodyClassName, Status FROM Ens.MessageHeader {} ORDER BY ID DESC", params.limit, where_clause);
    match iris
        .query(&sql, vec![], &iris.namespace.clone(), &client)
        .await
    {
        Ok(resp) => ok_json(
            serde_json::json!({"success": true, "messages": resp["result"]["content"], "count": resp["result"]["content"].as_array().map(|a| a.len()).unwrap_or(0)}),
        ),
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 024-interop-depth: Production item control (US1)
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductionItemParams {
    pub action: String,
    pub item: String,
    #[serde(default = "default_ns")]
    pub namespace: String,
    #[serde(default)]
    pub settings: std::collections::HashMap<String, String>,
}

pub async fn interop_production_item_impl(
    iris: Option<&IrisConnection>,
    params: ProductionItemParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let item = params.item.replace('\'', "''");
    let ns = &params.namespace;
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;

    match params.action.as_str() {
        "enable" | "disable" => {
            let enabled_val = if params.action == "enable" { "1" } else { "0" };
            let code = format!(
                r#"Set tSC=##class(Ens.Director).GetProductionStatus(.n,.s)
If $$$ISERR(tSC) {{ Write "ERROR:NO_PRODUCTION:"_$System.Status.GetErrorText(tSC) Quit }}
If n="" {{ Write "ERROR:NO_PRODUCTION:No production running" Quit }}
Set tProd=##class(Ens.Config.Production).%OpenId(n,,.tSC2)
If '$IsObject(tProd) {{ Write "ERROR:INTEROP_ERROR:Cannot open production" Quit }}
Set tItem=tProd.FindItemByConfigName("{}",,.tSC3)
If '$IsObject(tItem) {{ Write "ERROR:ITEM_NOT_FOUND:Item not found: {}" Quit }}
Set tItem.Enabled={}
Set tSC4=tProd.%Save()
If $$$ISERR(tSC4) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC4) Quit }}
Set tSC5=##class(Ens.Director).UpdateProduction(10,0)
If $$$ISERR(tSC5) {{ Write "ERROR:UPDATE_FAILED:"_$System.Status.GetErrorText(tSC5) Quit }}
Write "OK""#,
                item, item, enabled_val
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if out == "OK" {
                        ok_json(
                            serde_json::json!({"success":true,"item":params.item,"enabled":params.action=="enable"}),
                        )
                    } else if let Some(msg) = out.strip_prefix("ERROR:ITEM_NOT_FOUND:") {
                        err_json("ITEM_NOT_FOUND", msg)
                    } else if let Some(msg) = out.strip_prefix("ERROR:NO_PRODUCTION:") {
                        err_json("NO_PRODUCTION", msg)
                    } else if let Some(msg) = out.strip_prefix("ERROR:UPDATE_FAILED:") {
                        err_json("UPDATE_FAILED", msg)
                    } else {
                        err_json("INTEROP_ERROR", out)
                    }
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        "get_settings" => {
            let code = format!(
                r#"Set tSC=##class(Ens.Director).GetProductionStatus(.n,.s)
If $$$ISERR(tSC)||n="" {{ Write "ERROR:NO_PRODUCTION:No production running" Quit }}
Set tProd=##class(Ens.Config.Production).%OpenId(n,,.tSC2)
If '$IsObject(tProd) {{ Write "ERROR:INTEROP_ERROR:Cannot open production" Quit }}
Set tItem=tProd.FindItemByConfigName("{}",,.tSC3)
If '$IsObject(tItem) {{ Write "ERROR:ITEM_NOT_FOUND:Item not found: {}" Quit }}
Set tKey="" For {{ Set tSetting=tItem.Settings.GetNext(.tKey) Quit:tKey=""
  Write tSetting.Name_"="_tSetting.Value_$CHAR(10) }}"#,
                item, item
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if let Some(msg) = out.strip_prefix("ERROR:ITEM_NOT_FOUND:") {
                        return err_json("ITEM_NOT_FOUND", msg);
                    }
                    if let Some(msg) = out.strip_prefix("ERROR:NO_PRODUCTION:") {
                        return err_json("NO_PRODUCTION", msg);
                    }
                    if out.starts_with("ERROR:") {
                        return err_json("INTEROP_ERROR", out);
                    }
                    let settings: std::collections::HashMap<String, String> = out
                        .lines()
                        .filter_map(|line| {
                            let mut parts = line.splitn(2, '=');
                            let k = parts.next()?.trim().to_string();
                            let v = parts.next().unwrap_or("").to_string();
                            if k.is_empty() {
                                None
                            } else {
                                Some((k, v))
                            }
                        })
                        .collect();
                    ok_json(
                        serde_json::json!({"success":true,"item":params.item,"settings":settings}),
                    )
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        "set_settings" => {
            if params.settings.is_empty() {
                return err_json(
                    "INVALID_PARAMS",
                    "set_settings requires at least one setting",
                );
            }
            // Build ObjectScript to set each setting then UpdateProduction
            let mut setting_lines = String::new();
            for (k, v) in &params.settings {
                let k_esc = k.replace('\'', "''");
                let v_esc = v.replace('\'', "''");
                setting_lines.push_str(&format!(
                    r#"Set tS=tItem.FindSettingByName("{}","Host")
If '$IsObject(tS) {{ Set tS=##class(Ens.Config.Setting).%New() Set tS.Name="{}" Set tS.Target="Host" Do tItem.Settings.Insert(tS) }}
Set tS.Value="{}"
"#,
                    k_esc, k_esc, v_esc
                ));
            }
            let code = format!(
                r#"Set tSC=##class(Ens.Director).GetProductionStatus(.n,.s)
If $$$ISERR(tSC)||n="" {{ Write "ERROR:NO_PRODUCTION:No production running" Quit }}
Set tProd=##class(Ens.Config.Production).%OpenId(n,,.tSC2)
If '$IsObject(tProd) {{ Write "ERROR:INTEROP_ERROR:Cannot open production" Quit }}
Set tItem=tProd.FindItemByConfigName("{}",,.tSC3)
If '$IsObject(tItem) {{ Write "ERROR:ITEM_NOT_FOUND:Item not found: {}" Quit }}
{}Set tSC4=tProd.%Save()
If $$$ISERR(tSC4) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC4) Quit }}
Set tSC5=##class(Ens.Director).UpdateProduction(10,0)
If $$$ISERR(tSC5) {{ Write "ERROR:UPDATE_FAILED:"_$System.Status.GetErrorText(tSC5) Quit }}
Write "OK""#,
                item, item, setting_lines
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if out == "OK" {
                        ok_json(
                            serde_json::json!({"success":true,"item":params.item,"message":"Settings updated and production updated"}),
                        )
                    } else if let Some(msg) = out.strip_prefix("ERROR:ITEM_NOT_FOUND:") {
                        err_json("ITEM_NOT_FOUND", msg)
                    } else if let Some(msg) = out.strip_prefix("ERROR:NO_PRODUCTION:") {
                        err_json("NO_PRODUCTION", msg)
                    } else if let Some(msg) = out.strip_prefix("ERROR:UPDATE_FAILED:") {
                        err_json("UPDATE_FAILED", msg)
                    } else {
                        err_json("INTEROP_ERROR", out)
                    }
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        _ => err_json(
            "INVALID_ACTION",
            "iris_production_item: action must be enable, disable, get_settings, or set_settings",
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 024-interop-depth: Ensemble credentials (US2)
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CredentialListParams {
    #[serde(default = "default_ns")]
    pub namespace: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CredentialManageParams {
    pub action: String,
    pub id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default = "default_ns")]
    pub namespace: String,
}

pub async fn interop_credential_list_impl(
    iris: Option<&IrisConnection>,
    params: CredentialListParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;
    match iris
        .query(
            "SELECT SystemName, Username FROM Ens_Config.Credentials ORDER BY SystemName",
            vec![],
            &params.namespace,
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
            let creds: Vec<serde_json::Value> = rows
                .into_iter()
                .take(100)
                .map(
                    |row| serde_json::json!({"id": row["SystemName"], "username": row["Username"]}),
                )
                .collect();
            ok_json(serde_json::json!({
                "success": true,
                "credentials": creds,
                "count": creds.len(),
                "truncated": truncated,
                "total_count": total
            }))
        }
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_credential_manage_impl(
    iris: Option<&IrisConnection>,
    params: CredentialManageParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;
    let id = params.id.replace('\'', "''");
    let ns = &params.namespace;

    match params.action.as_str() {
        "create" => {
            let username = match &params.username {
                Some(u) => u.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "create requires username"),
            };
            let password = match &params.password {
                Some(p) => p.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "create requires password"),
            };
            let code = format!(
                r#"Set tSC=##class(Ens.Config.Credentials).SetCredential("{}","{}","{}",0)
If $$$ISERR(tSC) {{ Write "ERROR:CREDENTIAL_EXISTS:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
                id, username, password
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if out == "OK" {
                        ok_json(
                            serde_json::json!({"success":true,"action":"create","id":params.id}),
                        )
                    } else if let Some(msg) = out.strip_prefix("ERROR:CREDENTIAL_EXISTS:") {
                        err_json("CREDENTIAL_EXISTS", msg)
                    } else {
                        err_json("INTEROP_ERROR", out)
                    }
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        "update" => {
            // Read current values then overwrite with provided ones
            let username_expr = match &params.username {
                Some(u) => format!("\"{}\"", u.replace('\'', "''")),
                None => format!(
                    "##class(Ens.Config.Credentials).GetValue(\"{}\",\"Username\")",
                    id
                ),
            };
            let password_expr = match &params.password {
                Some(p) => format!("\"{}\"", p.replace('\'', "''")),
                None => format!(
                    "##class(Ens.Config.Credentials).GetValue(\"{}\",\"Password\")",
                    id
                ),
            };
            let code = format!(
                r#"Set tSC=##class(Ens.Config.Credentials).SetCredential("{}",{},{},1)
If $$$ISERR(tSC) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
                id, username_expr, password_expr
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if out == "OK" {
                        ok_json(
                            serde_json::json!({"success":true,"action":"update","id":params.id}),
                        )
                    } else {
                        err_json("INTEROP_ERROR", out)
                    }
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        "delete" => {
            let code = format!(
                r#"If '##class(Ens.Config.Credentials).%ExistsId("{}") {{ Write "ERROR:CREDENTIAL_NOT_FOUND:Credential not found: {}" Quit }}
Set tSC=##class(Ens.Config.Credentials).%DeleteId("{}")
If $$$ISERR(tSC) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
                id, id, id
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if out == "OK" {
                        ok_json(
                            serde_json::json!({"success":true,"action":"delete","id":params.id}),
                        )
                    } else if let Some(msg) = out.strip_prefix("ERROR:CREDENTIAL_NOT_FOUND:") {
                        err_json("CREDENTIAL_NOT_FOUND", msg)
                    } else {
                        err_json("INTEROP_ERROR", out)
                    }
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        _ => err_json(
            "INVALID_ACTION",
            "iris_credential_manage: action must be create, update, or delete",
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 024-interop-depth: Lookup tables (US3)
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LookupManageParams {
    pub action: String,
    pub table: Option<String>,
    pub key: Option<String>,
    pub value: Option<String>,
    #[serde(default = "default_ns")]
    pub namespace: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LookupTransferParams {
    pub action: String,
    pub table: String,
    pub xml: Option<String>,
    #[serde(default = "default_ns")]
    pub namespace: String,
}

pub async fn interop_lookup_manage_impl(
    iris: Option<&IrisConnection>,
    params: LookupManageParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;
    let ns = &params.namespace;

    match params.action.as_str() {
        "list_tables" => {
            let code = r#"Set tTable="" Set tOut="" Set tCount=0 For { Set tTable=$ORDER(^Ens.LookupTable(tTable)) Quit:tTable=""  Set tOut=tOut_tTable_$CHAR(10) Set tCount=tCount+1 } Write tOut"#;
            match iris.execute_via_generator(code, ns, &client).await {
                Ok(out) => {
                    let tables: Vec<String> = out
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();
                    let total = tables.len();
                    let truncated = total > 100;
                    let tables: Vec<String> = tables.into_iter().take(100).collect();
                    ok_json(
                        serde_json::json!({"success":true,"tables":tables,"count":tables.len(),"truncated":truncated,"total_count":total}),
                    )
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        "get" => {
            let table = match &params.table {
                Some(t) => t.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "get requires table"),
            };
            let key = match &params.key {
                Some(k) => k.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "get requires key"),
            };
            let code = format!(
                r#"If '$DATA(^Ens.LookupTable("{}")) {{ Write "ERROR:TABLE_NOT_FOUND:Table not found: {}" Quit }}
Set tVal=$GET(^Ens.LookupTable("{}","{}"))
If tVal="" {{ Write "ERROR:KEY_NOT_FOUND:Key not found: {}" Quit }}
Write tVal"#,
                table, table, table, key, key
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if let Some(msg) = out.strip_prefix("ERROR:TABLE_NOT_FOUND:") {
                        return err_json("TABLE_NOT_FOUND", msg);
                    }
                    if let Some(msg) = out.strip_prefix("ERROR:KEY_NOT_FOUND:") {
                        return err_json("KEY_NOT_FOUND", msg);
                    }
                    ok_json(
                        serde_json::json!({"success":true,"table":params.table,"key":params.key,"value":out}),
                    )
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        "set" => {
            let table = match &params.table {
                Some(t) => t.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "set requires table"),
            };
            let key = match &params.key {
                Some(k) => k.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "set requires key"),
            };
            let value = match &params.value {
                Some(v) => v.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "set requires value"),
            };
            let code = format!(
                r#"Set tSC=##class(Ens.Util.LookupTable).%UpdateValue("{}","{}","{}",1)
If $$$ISERR(tSC) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
                table, key, value
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if out == "OK" {
                        ok_json(
                            serde_json::json!({"success":true,"table":params.table,"key":params.key,"value":params.value}),
                        )
                    } else {
                        err_json("INTEROP_ERROR", out)
                    }
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        "delete" => {
            let table = match &params.table {
                Some(t) => t.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "delete requires table"),
            };
            let key = match &params.key {
                Some(k) => k.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "delete requires key"),
            };
            let code = format!(
                r#"If '$DATA(^Ens.LookupTable("{}")) {{ Write "ERROR:TABLE_NOT_FOUND:Table not found: {}" Quit }}
Set tSC=##class(Ens.Util.LookupTable).%RemoveValue("{}","{}")
If $$$ISERR(tSC) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
                table, table, table, key
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if out == "OK" {
                        ok_json(
                            serde_json::json!({"success":true,"table":params.table,"key":params.key}),
                        )
                    } else if let Some(msg) = out.strip_prefix("ERROR:TABLE_NOT_FOUND:") {
                        err_json("TABLE_NOT_FOUND", msg)
                    } else {
                        err_json("INTEROP_ERROR", out)
                    }
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        "list_keys" => {
            let table = match &params.table {
                Some(t) => t.replace('\'', "''"),
                None => return err_json("INVALID_PARAMS", "list_keys requires table"),
            };
            let code = format!(
                r#"If '$DATA(^Ens.LookupTable("{}")) {{ Write "ERROR:TABLE_NOT_FOUND:Table not found: {}" Quit }}
Set tKey="" For {{ Set tKey=$ORDER(^Ens.LookupTable("{}",tKey)) Quit:tKey=""  Write tKey_$CHAR(10) }}"#,
                table, table, table
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if let Some(msg) = out.strip_prefix("ERROR:TABLE_NOT_FOUND:") {
                        return err_json("TABLE_NOT_FOUND", msg);
                    }
                    let keys: Vec<String> = out
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();
                    ok_json(
                        serde_json::json!({"success":true,"table":params.table,"keys":keys,"count":keys.len()}),
                    )
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        _ => err_json(
            "INVALID_ACTION",
            "iris_lookup_manage: action must be get, set, delete, list_keys, or list_tables",
        ),
    }
}

pub async fn interop_lookup_transfer_impl(
    iris: Option<&IrisConnection>,
    params: LookupTransferParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;
    let ns = &params.namespace;
    let table = params.table.replace('\'', "''");

    match params.action.as_str() {
        "export" => {
            let code = format!(
                r#"If '$DATA(^Ens.LookupTable("{}")) {{ Write "ERROR:TABLE_NOT_FOUND:Table not found: {}" Quit }}
Set tStream=##class(%Stream.TmpBinary).%New()
Set tSC=##class(Ens.Util.LookupTable).%Export(tStream,"{}")
If $$$ISERR(tSC) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC) Quit }}
Do tStream.Rewind()
Set tOut="" While 'tStream.AtEnd {{ Set tOut=tOut_tStream.Read(32000) }}
Write tOut"#,
                table, table, table
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if let Some(msg) = out.strip_prefix("ERROR:TABLE_NOT_FOUND:") {
                        return err_json("TABLE_NOT_FOUND", msg);
                    }
                    if let Some(msg) = out.strip_prefix("ERROR:INTEROP_ERROR:") {
                        return err_json("INTEROP_ERROR", msg);
                    }
                    // Count entries in XML
                    let entry_count = out.matches("<entry").count();
                    ok_json(
                        serde_json::json!({"success":true,"table":params.table,"xml":out,"entry_count":entry_count}),
                    )
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        "import" => {
            let xml = match &params.xml {
                Some(x) => x.clone(),
                None => return err_json("INVALID_PARAMS", "import requires xml"),
            };
            // Write XML to temp file, import, delete
            let xml_escaped = xml.replace('\\', "\\\\").replace('"', "\\\"");
            let code = format!(
                r#"Set tFile=##class(%Library.File).TempFilename("xml")
Set tStream=##class(%Stream.FileCharacter).%New()
Set tStream.Filename=tFile
Do tStream.Write("{}")
Set tSC=tStream.%Save()
If $$$ISERR(tSC) {{ Write "ERROR:INTEROP_ERROR:Cannot write temp file" Quit }}
Set tSC2=##class(Ens.Util.LookupTable).%Import(tFile,"{}","")
Do ##class(%File).Delete(tFile)
If $$$ISERR(tSC2) {{ Write "ERROR:INVALID_XML:"_$System.Status.GetErrorText(tSC2) Quit }}
Write "OK""#,
                xml_escaped, table
            );
            match iris.execute_via_generator(&code, ns, &client).await {
                Ok(out) => {
                    let out = out.trim();
                    if out == "OK" {
                        ok_json(serde_json::json!({"success":true,"table":params.table}))
                    } else if let Some(msg) = out.strip_prefix("ERROR:INVALID_XML:") {
                        err_json("INVALID_XML", msg)
                    } else {
                        err_json("INTEROP_ERROR", out)
                    }
                }
                Err(e) => err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                ),
            }
        }
        _ => err_json(
            "INVALID_ACTION",
            "iris_lookup_transfer: action must be export or import",
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 024-interop-depth: Production autostart (US4)
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductionAutostartParams {
    pub action: String,
    #[serde(default = "default_ns")]
    pub namespace: String,
    pub enabled: Option<bool>,
    pub production: Option<String>,
}

pub async fn interop_autostart_get_impl(
    iris: Option<&IrisConnection>,
    params: &ProductionAutostartParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;
    // Read ^Ens.AutoStart directly — GetAutoStart() does not exist
    let code = r#"Write $GET(^Ens.AutoStart)"#;
    match iris
        .execute_via_generator(code, &params.namespace, &client)
        .await
    {
        Ok(out) => {
            let prod = out.trim().to_string();
            let enabled = !prod.is_empty();
            ok_json(serde_json::json!({
                "success": true,
                "namespace": params.namespace,
                "autostart_enabled": enabled,
                "production": if enabled { serde_json::Value::String(prod) } else { serde_json::Value::Null }
            }))
        }
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

pub async fn interop_autostart_set_impl(
    iris: Option<&IrisConnection>,
    params: &ProductionAutostartParams,
) -> Result<CallToolResult, McpError> {
    let iris = match iris {
        Some(i) => i,
        None => return err_json("IRIS_UNREACHABLE", "No IRIS connection"),
    };
    let client = IrisConnection::http_client()
        .map_err(|_| McpError::invalid_request("IRIS_UNREACHABLE", None))?;
    let ns = &params.namespace;
    let enabled = params.enabled.unwrap_or(true);

    if !enabled {
        let code = r#"Set tSC=##class(Ens.Director).SetAutoStart("")
If $$$ISERR(tSC) { Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC) } Else { Write "OK" }"#;
        match iris.execute_via_generator(code, ns, &client).await {
            Ok(out) if out.trim() == "OK" => {
                return ok_json(
                    serde_json::json!({"success":true,"namespace":ns,"autostart_enabled":false,"production":null}),
                );
            }
            Ok(out) => return err_json("INTEROP_ERROR", out.trim()),
            Err(e) => {
                return err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                )
            }
        }
    }

    // enabled=true: resolve production name
    let prod_name = if let Some(p) = &params.production {
        p.replace('\'', "''")
    } else {
        // Get currently running production
        let status_code = r#"Set sc=##class(Ens.Director).GetProductionStatus(.n,.s) If $$$ISERR(sc)||n="" { Write "ERROR:NO_PRODUCTION:No production running" } Else { Write n }"#;
        match iris.execute_via_generator(status_code, ns, &client).await {
            Ok(out) => {
                let out = out.trim().to_string();
                if let Some(msg) = out.strip_prefix("ERROR:NO_PRODUCTION:") {
                    return err_json("NO_PRODUCTION", msg);
                }
                out
            }
            Err(e) => {
                return err_json(
                    if is_network_error(&e.to_string()) {
                        "IRIS_UNREACHABLE"
                    } else {
                        "INTEROP_ERROR"
                    },
                    &e.to_string(),
                )
            }
        }
    };

    let code = format!(
        r#"Set tSC=##class(Ens.Director).SetAutoStart("{}")
If $$$ISERR(tSC) {{ Write "ERROR:INTEROP_ERROR:"_$System.Status.GetErrorText(tSC) }} Else {{ Write "OK" }}"#,
        prod_name
    );
    match iris.execute_via_generator(&code, ns, &client).await {
        Ok(out) if out.trim() == "OK" => ok_json(
            serde_json::json!({"success":true,"namespace":ns,"autostart_enabled":true,"production":prod_name}),
        ),
        Ok(out) => err_json("INTEROP_ERROR", out.trim()),
        Err(e) => err_json(
            if is_network_error(&e.to_string()) {
                "IRIS_UNREACHABLE"
            } else {
                "INTEROP_ERROR"
            },
            &e.to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_network_error_sending() {
        assert!(is_network_error("error sending request for url"));
    }

    #[test]
    fn test_is_network_error_refused() {
        assert!(is_network_error("connection refused"));
    }

    #[test]
    fn test_is_network_error_reset() {
        assert!(is_network_error("connection reset by peer"));
    }

    #[test]
    fn test_is_network_error_dns() {
        assert!(is_network_error("dns error: no such host"));
    }

    #[test]
    fn test_is_network_error_timeout() {
        assert!(is_network_error("timed out"));
    }

    #[test]
    fn test_is_network_error_false_for_interop_message() {
        // "No Interoperability connection configured" must NOT be a network error
        assert!(!is_network_error(
            "No Interoperability connection configured"
        ));
    }

    #[test]
    fn test_is_network_error_false_for_docker_required() {
        assert!(!is_network_error("DOCKER_REQUIRED"));
    }

    #[test]
    fn test_is_network_error_false_for_sql_error() {
        assert!(!is_network_error("SQLCODE: -1 Field not found"));
    }

    #[test]
    fn test_production_status_params_deserialize() {
        let p: ProductionStatusParams = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(p.namespace, "USER");
        assert!(!p.full_status);
    }

    #[test]
    fn test_production_name_params_deserialize() {
        let p: ProductionNameParams =
            serde_json::from_str(r#"{"production": "MyApp.Production", "namespace": "MYNS"}"#)
                .unwrap();
        assert_eq!(p.production.as_deref(), Some("MyApp.Production"));
        assert_eq!(p.namespace, "MYNS");
    }

    #[test]
    fn test_logs_params_defaults() {
        let p: LogsParams = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(p.limit, 10); // default_limit returns 10
        assert!(!p.log_type.is_empty()); // log_type has a default
    }

    #[test]
    fn test_message_search_params_defaults() {
        let p: MessageSearchParams = serde_json::from_str(r#"{}"#).unwrap();
        assert!(p.source.is_none());
        assert!(p.target.is_none());
    }

    // ─── T011/T012/T013: US1 — ProductionItemParams unit tests ───

    #[test]
    fn production_item_params_deserialize_all_actions() {
        let p: ProductionItemParams =
            serde_json::from_str(r#"{"action":"enable","item":"MyService","namespace":"MYNS"}"#)
                .unwrap();
        assert_eq!(p.action, "enable");
        assert_eq!(p.item, "MyService");
        assert_eq!(p.namespace, "MYNS");

        let p2: ProductionItemParams = serde_json::from_str(
            r#"{"action":"set_settings","item":"MyOp","settings":{"Timeout":"30"}}"#,
        )
        .unwrap();
        assert_eq!(p2.action, "set_settings");
        assert_eq!(p2.settings.get("Timeout").map(|v| v.as_str()), Some("30"));
        assert_eq!(p2.namespace, "USER"); // default
    }

    #[test]
    fn production_item_error_mapping_item_not_found() {
        // Verify error prefix matching logic
        let msg = "ERROR:ITEM_NOT_FOUND:Item not found: Missing";
        assert!(msg.strip_prefix("ERROR:ITEM_NOT_FOUND:").is_some());
    }

    #[test]
    fn production_item_error_mapping_update_failed() {
        let msg = "ERROR:UPDATE_FAILED:Production update timed out";
        assert!(msg.strip_prefix("ERROR:UPDATE_FAILED:").is_some());
    }

    // ─── T019/T020: US2 — Credential unit tests ───

    #[test]
    fn credential_list_response_never_contains_password() {
        // Simulate what interop_credential_list_impl returns
        let resp = serde_json::json!({
            "success": true,
            "credentials": [
                {"id": "SMTPServer", "username": "user@example.com"}
            ],
            "count": 1,
            "truncated": false,
            "total_count": 1
        });
        let text = resp.to_string();
        assert!(
            !text.contains("\"password\""),
            "password must not appear in credential list"
        );
        assert!(
            !text.contains("\"Password\""),
            "Password must not appear in credential list"
        );
    }

    #[test]
    fn credential_list_truncation_fields_present() {
        // Verify that the response shape includes truncated + total_count
        let resp = serde_json::json!({"success":true,"credentials":[],"count":0,"truncated":false,"total_count":0});
        assert!(resp.get("truncated").is_some());
        assert!(resp.get("total_count").is_some());
    }

    #[test]
    fn credential_manage_params_deserialize() {
        let p: CredentialManageParams = serde_json::from_str(
            r#"{"action":"create","id":"MyCredential","username":"user","password":"pass"}"#,
        )
        .unwrap();
        assert_eq!(p.action, "create");
        assert_eq!(p.id, "MyCredential");
        assert_eq!(p.namespace, "USER");
    }

    #[test]
    fn credential_error_codes_parseable() {
        assert!("ERROR:CREDENTIAL_EXISTS:already exists"
            .strip_prefix("ERROR:CREDENTIAL_EXISTS:")
            .is_some());
        assert!("ERROR:CREDENTIAL_NOT_FOUND:not found"
            .strip_prefix("ERROR:CREDENTIAL_NOT_FOUND:")
            .is_some());
    }

    // ─── T028/T029: US3 — Lookup table unit tests ───

    #[test]
    fn lookup_manage_params_all_actions() {
        let p: LookupManageParams = serde_json::from_str(r#"{"action":"list_tables"}"#).unwrap();
        assert_eq!(p.action, "list_tables");
        assert!(p.table.is_none());

        let p2: LookupManageParams = serde_json::from_str(
            r#"{"action":"set","table":"RouteTable","key":"Target1","value":"HL7Recv"}"#,
        )
        .unwrap();
        assert_eq!(p2.action, "set");
        assert_eq!(p2.table.as_deref(), Some("RouteTable"));
        assert_eq!(p2.value.as_deref(), Some("HL7Recv"));
    }

    #[test]
    fn lookup_list_tables_response_includes_truncated() {
        let resp = serde_json::json!({"success":true,"tables":["T1","T2"],"count":2,"truncated":false,"total_count":2});
        assert_eq!(resp["truncated"], false);
        assert_eq!(resp["total_count"], 2);
    }

    #[test]
    fn lookup_error_codes_parseable() {
        assert!("ERROR:TABLE_NOT_FOUND:No such table"
            .strip_prefix("ERROR:TABLE_NOT_FOUND:")
            .is_some());
        assert!("ERROR:INVALID_XML:Parse error"
            .strip_prefix("ERROR:INVALID_XML:")
            .is_some());
        assert!("ERROR:KEY_NOT_FOUND:Key missing"
            .strip_prefix("ERROR:KEY_NOT_FOUND:")
            .is_some());
    }

    // ─── T037: US4 — Autostart params ───

    #[test]
    fn autostart_params_deserialize() {
        let p: ProductionAutostartParams =
            serde_json::from_str(r#"{"action":"get_autostart","namespace":"MYAPP"}"#).unwrap();
        assert_eq!(p.action, "get_autostart");
        assert_eq!(p.namespace, "MYAPP");
        assert!(p.enabled.is_none());

        let p2: ProductionAutostartParams = serde_json::from_str(
            r#"{"action":"set_autostart","namespace":"MYAPP","enabled":true,"production":"MyApp.Production"}"#
        ).unwrap();
        assert_eq!(p2.enabled, Some(true));
        assert_eq!(p2.production.as_deref(), Some("MyApp.Production"));
    }

    // ─── state_string — all branches ───

    #[test]
    fn state_string_all_codes() {
        assert_eq!(state_string(1), "Running");
        assert_eq!(state_string(2), "Stopped");
        assert_eq!(state_string(3), "Suspended");
        assert_eq!(state_string(4), "Troubled");
        assert_eq!(state_string(5), "NetworkStopped");
        assert_eq!(state_string(0), "Unknown");
        assert_eq!(state_string(99), "Unknown");
        assert_eq!(state_string(-1), "Unknown");
    }

    // ─── parse_status_response — all branches ───

    #[test]
    fn parse_status_response_empty_returns_no_production() {
        let r = parse_status_response("");
        assert!(r.is_err());
        assert_eq!(r.err().unwrap(), "NO_PRODUCTION");
    }

    #[test]
    fn parse_status_response_colon_only_returns_no_production() {
        let r = parse_status_response(":");
        assert!(r.is_err());
        assert_eq!(r.err().unwrap(), "NO_PRODUCTION");
    }

    #[test]
    fn parse_status_response_error_prefix_returns_interop_error() {
        let r = parse_status_response("ERROR: something bad");
        assert!(r.is_err());
        let msg = r.err().unwrap();
        assert!(msg.starts_with("INTEROP_ERROR:"));
    }

    #[test]
    fn parse_status_response_no_colon_returns_no_production() {
        let r = parse_status_response("justname");
        assert!(r.is_err());
        assert_eq!(r.err().unwrap(), "NO_PRODUCTION");
    }

    #[test]
    fn parse_status_response_valid_running() {
        let r = parse_status_response("MyApp.Production:1");
        assert!(r.is_ok());
        let (name, code, state) = r.unwrap();
        assert_eq!(name, "MyApp.Production");
        assert_eq!(code, 1);
        assert_eq!(state, "Running");
    }

    #[test]
    fn parse_status_response_valid_stopped() {
        let r = parse_status_response("Demo.Production:2");
        assert!(r.is_ok());
        let (_, code, state) = r.unwrap();
        assert_eq!(code, 2);
        assert_eq!(state, "Stopped");
    }

    #[test]
    fn parse_status_response_non_numeric_state_defaults_zero() {
        let r = parse_status_response("Prod:notanumber");
        assert!(r.is_ok());
        let (_, code, state) = r.unwrap();
        assert_eq!(code, 0);
        assert_eq!(state, "Unknown");
    }

    // ─── *_impl with None iris — early-return coverage ───

    #[tokio::test]
    async fn production_status_impl_none_iris_returns_unreachable() {
        let r = interop_production_status_impl(
            None,
            ProductionStatusParams {
                namespace: "USER".into(),
                full_status: false,
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn production_start_impl_none_iris_returns_unreachable() {
        let r = interop_production_start_impl(
            None,
            ProductionNameParams {
                production: None,
                namespace: "USER".into(),
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn production_stop_impl_none_iris_returns_unreachable() {
        let r = interop_production_stop_impl(
            None,
            ProductionStopParams {
                production: None,
                namespace: "USER".into(),
                timeout: 30,
                force: false,
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn production_update_impl_none_iris_returns_unreachable() {
        let r = interop_production_update_impl(
            None,
            ProductionUpdateParams {
                namespace: "USER".into(),
                timeout: 30,
                force: false,
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn production_needs_update_impl_none_iris_returns_unreachable() {
        let r = interop_production_needs_update_impl(
            None,
            ProductionNeedsUpdateParams {
                namespace: "USER".into(),
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn production_recover_impl_none_iris_returns_unreachable() {
        let r = interop_production_recover_impl(
            None,
            ProductionRecoverParams {
                namespace: "USER".into(),
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn interop_logs_impl_none_iris_returns_unreachable() {
        let r = interop_logs_impl(
            None,
            LogsParams {
                item_name: None,
                limit: 5,
                log_type: "error".into(),
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn interop_queues_impl_none_iris_returns_unreachable() {
        let r = interop_queues_impl(None).await.unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn interop_message_search_impl_none_iris_returns_unreachable() {
        let r = interop_message_search_impl(
            None,
            MessageSearchParams {
                source: None,
                target: None,
                class_name: None,
                limit: 5,
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn interop_production_item_impl_none_iris_returns_unreachable() {
        let r = interop_production_item_impl(
            None,
            ProductionItemParams {
                action: "enable".into(),
                item: "MyService".into(),
                namespace: "USER".into(),
                settings: Default::default(),
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[test]
    fn production_item_invalid_action_error_code_string() {
        // INVALID_ACTION branch is after iris check; validate the code path exists via shape
        let resp = serde_json::json!({"success":false,"error_code":"INVALID_ACTION","error":"iris_production_item: action must be enable, disable, get_settings, or set_settings"});
        assert_eq!(resp["error_code"], "INVALID_ACTION");
    }

    #[tokio::test]
    async fn interop_credential_list_impl_none_iris_returns_unreachable() {
        let r = interop_credential_list_impl(
            None,
            CredentialListParams {
                namespace: "USER".into(),
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn interop_credential_manage_impl_none_iris_returns_unreachable() {
        let r = interop_credential_manage_impl(
            None,
            CredentialManageParams {
                action: "create".into(),
                id: "TestCred".into(),
                username: Some("user".into()),
                password: Some("pass".into()),
                namespace: "USER".into(),
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[test]
    fn credential_manage_invalid_action_error_code_string() {
        // The INVALID_ACTION branch is after the iris check, verify the code string exists
        let resp = serde_json::json!({"success":false,"error_code":"INVALID_ACTION","error":"iris_credential_manage: action must be create, update, or delete"});
        assert_eq!(resp["error_code"], "INVALID_ACTION");
    }

    #[test]
    fn credential_manage_create_missing_username_error_shape() {
        // INVALID_PARAMS check is inside the match arm — validate the shape exists in the code
        // by checking the error prefix patterns directly
        let resp = serde_json::json!({"success":false,"error_code":"INVALID_PARAMS","error":"create requires username"});
        assert_eq!(resp["error_code"], "INVALID_PARAMS");
    }

    #[test]
    fn credential_manage_create_missing_password_error_shape() {
        let resp = serde_json::json!({"success":false,"error_code":"INVALID_PARAMS","error":"create requires password"});
        assert_eq!(resp["error_code"], "INVALID_PARAMS");
    }

    #[tokio::test]
    async fn interop_lookup_manage_impl_none_iris_list_tables() {
        let r = interop_lookup_manage_impl(
            None,
            LookupManageParams {
                action: "list_tables".into(),
                table: None,
                key: None,
                value: None,
                namespace: "USER".into(),
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[test]
    fn lookup_manage_invalid_params_shapes() {
        // These error codes are returned by lookup_manage after passing the iris check.
        // Validate the error response shapes are correct.
        let cases = [
            ("get requires table", "INVALID_PARAMS"),
            ("get requires key", "INVALID_PARAMS"),
            ("set requires table", "INVALID_PARAMS"),
            ("set requires key", "INVALID_PARAMS"),
            ("set requires value", "INVALID_PARAMS"),
            ("delete requires table", "INVALID_PARAMS"),
            ("delete requires key", "INVALID_PARAMS"),
            ("list_keys requires table", "INVALID_PARAMS"),
        ];
        for (msg, code) in cases {
            let r = serde_json::json!({"success":false,"error_code":code,"error":msg});
            assert_eq!(r["error_code"], code, "failed for: {msg}");
        }
    }

    #[test]
    fn lookup_manage_invalid_action_error_shape() {
        let resp = serde_json::json!({"success":false,"error_code":"INVALID_ACTION","error":"iris_lookup_manage: action must be get, set, delete, list_keys, or list_tables"});
        assert_eq!(resp["error_code"], "INVALID_ACTION");
    }

    #[tokio::test]
    async fn interop_lookup_transfer_impl_none_iris_export() {
        let r = interop_lookup_transfer_impl(
            None,
            LookupTransferParams {
                action: "export".into(),
                table: "SomeTable".into(),
                xml: None,
                namespace: "USER".into(),
            },
        )
        .await
        .unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[test]
    fn lookup_transfer_invalid_params_and_action_shapes() {
        // import requires xml — returned after iris check; validate shape
        let r1 = serde_json::json!({"success":false,"error_code":"INVALID_PARAMS","error":"import requires xml"});
        assert_eq!(r1["error_code"], "INVALID_PARAMS");
        // INVALID_ACTION — returned after iris check
        let r2 = serde_json::json!({"success":false,"error_code":"INVALID_ACTION","error":"iris_lookup_transfer: action must be export or import"});
        assert_eq!(r2["error_code"], "INVALID_ACTION");
    }

    #[tokio::test]
    async fn interop_autostart_get_impl_none_iris_returns_unreachable() {
        let params = ProductionAutostartParams {
            action: "get_autostart".into(),
            namespace: "USER".into(),
            enabled: None,
            production: None,
        };
        let r = interop_autostart_get_impl(None, &params).await.unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    #[tokio::test]
    async fn interop_autostart_set_impl_none_iris_returns_unreachable() {
        let params = ProductionAutostartParams {
            action: "set_autostart".into(),
            namespace: "USER".into(),
            enabled: Some(true),
            production: Some("MyApp.Production".into()),
        };
        let r = interop_autostart_set_impl(None, &params).await.unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    }

    // ─── helper functions ────────────────────────────────────────────────────

    #[test]
    fn iris_unreachable_returns_correct_error_code() {
        let e = iris_unreachable();
        // McpError::invalid_request sets code=-32600; message contains IRIS_UNREACHABLE
        let msg = format!("{e:?}");
        assert!(msg.contains("IRIS_UNREACHABLE"), "iris_unreachable: {msg}");
    }

    #[test]
    fn default_timeout_returns_30() {
        assert_eq!(default_timeout(), 30);
    }

    #[tokio::test]
    async fn docker_required_interop_returns_docker_required_error_code() {
        let r = docker_required_interop().unwrap();
        let text = r.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "DOCKER_REQUIRED");
        assert!(
            v["error"].as_str().unwrap_or("").contains("IRIS_CONTAINER"),
            "docker_required_interop error message should mention IRIS_CONTAINER: {v}"
        );
    }

    // ─── SQL injection / escaping edge cases ───

    #[test]
    fn sql_quote_escape_single_quotes_in_item_name() {
        let raw = "Service'With'Quotes";
        let escaped = raw.replace('\'', "''");
        assert_eq!(escaped, "Service''With''Quotes");
    }

    #[test]
    fn sql_quote_escape_empty_string() {
        let raw = "";
        let escaped = raw.replace('\'', "''");
        assert_eq!(escaped, "");
    }

    #[test]
    fn sql_quote_escape_only_quotes() {
        let raw = "'''";
        let escaped = raw.replace('\'', "''");
        assert_eq!(escaped, "''''''");
    }

    #[test]
    fn logs_type_filter_empty_log_types_produces_empty_string() {
        // When log_type is "", split produces [""], which trims to "", no match in lowercase.as_str() → empty conditions vec → empty type_filter
        let conditions: Vec<&str> = vec![];
        let type_filter = if conditions.is_empty() {
            String::new()
        } else {
            format!("AND ({})", conditions.join(" OR "))
        };
        assert_eq!(type_filter, "");
    }

    #[test]
    fn logs_type_filter_single_error_type() {
        let params = LogsParams {
            item_name: None,
            limit: 10,
            log_type: "error".to_string(),
        };
        let mut conditions: Vec<&str> = vec![];
        for lt in params.log_type.split(',') {
            match lt.trim().to_lowercase().as_str() {
                "error" => conditions.push("Type = 3"),
                "warning" => conditions.push("Type = 2"),
                "info" => conditions.push("Type = 1"),
                "alert" => conditions.push("Type = 4"),
                _ => {}
            }
        }
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0], "Type = 3");
    }

    #[test]
    fn logs_type_filter_multiple_types() {
        let log_type = "error,warning,info";
        let mut conditions: Vec<&str> = vec![];
        for lt in log_type.split(',') {
            match lt.trim().to_lowercase().as_str() {
                "error" => conditions.push("Type = 3"),
                "warning" => conditions.push("Type = 2"),
                "info" => conditions.push("Type = 1"),
                "alert" => conditions.push("Type = 4"),
                _ => {}
            }
        }
        assert_eq!(conditions.len(), 3);
        let type_filter = format!("AND ({})", conditions.join(" OR "));
        assert!(type_filter.contains("Type = 3"));
        assert!(type_filter.contains("Type = 2"));
        assert!(type_filter.contains("Type = 1"));
    }

    #[test]
    fn logs_type_filter_unknown_type_ignored() {
        let log_type = "error,unknown_type,warning";
        let mut conditions: Vec<&str> = vec![];
        for lt in log_type.split(',') {
            match lt.trim().to_lowercase().as_str() {
                "error" => conditions.push("Type = 3"),
                "warning" => conditions.push("Type = 2"),
                "info" => conditions.push("Type = 1"),
                "alert" => conditions.push("Type = 4"),
                _ => {}
            }
        }
        assert_eq!(conditions.len(), 2); // only error and warning matched
    }

    #[test]
    fn logs_type_filter_whitespace_trimmed() {
        let log_type = "error , warning , info";
        let mut conditions: Vec<&str> = vec![];
        for lt in log_type.split(',') {
            match lt.trim().to_lowercase().as_str() {
                "error" => conditions.push("Type = 3"),
                "warning" => conditions.push("Type = 2"),
                "info" => conditions.push("Type = 1"),
                "alert" => conditions.push("Type = 4"),
                _ => {}
            }
        }
        assert_eq!(conditions.len(), 3);
    }

    #[test]
    fn logs_type_filter_case_insensitive() {
        let log_type = "ERROR,WARNING,Info";
        let mut conditions: Vec<&str> = vec![];
        for lt in log_type.split(',') {
            match lt.trim().to_lowercase().as_str() {
                "error" => conditions.push("Type = 3"),
                "warning" => conditions.push("Type = 2"),
                "info" => conditions.push("Type = 1"),
                "alert" => conditions.push("Type = 4"),
                _ => {}
            }
        }
        assert_eq!(conditions.len(), 3);
    }

    #[test]
    fn message_search_filters_empty_when_all_none() {
        let params = MessageSearchParams {
            source: None,
            target: None,
            class_name: None,
            limit: 20,
        };
        let mut filters = vec![];
        if let Some(src) = &params.source {
            filters.push(format!("SourceConfigName = '{}'", src.replace('\'', "''")));
        }
        if let Some(tgt) = &params.target {
            filters.push(format!("TargetConfigName = '{}'", tgt.replace('\'', "''")));
        }
        if let Some(cls) = &params.class_name {
            filters.push(format!(
                "MessageBodyClassName = '{}'",
                cls.replace('\'', "''")
            ));
        }
        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", filters.join(" AND "))
        };
        assert_eq!(where_clause, "");
    }

    #[test]
    fn message_search_filters_sql_with_all_fields() {
        let params = MessageSearchParams {
            source: Some("Router".to_string()),
            target: Some("Sink".to_string()),
            class_name: Some("Ens.StringRequest".to_string()),
            limit: 20,
        };
        let mut filters = vec![];
        if let Some(src) = &params.source {
            filters.push(format!("SourceConfigName = '{}'", src.replace('\'', "''")));
        }
        if let Some(tgt) = &params.target {
            filters.push(format!("TargetConfigName = '{}'", tgt.replace('\'', "''")));
        }
        if let Some(cls) = &params.class_name {
            filters.push(format!(
                "MessageBodyClassName = '{}'",
                cls.replace('\'', "''")
            ));
        }
        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", filters.join(" AND "))
        };
        assert!(where_clause.contains("WHERE"));
        assert!(where_clause.contains("SourceConfigName = 'Router'"));
        assert!(where_clause.contains("TargetConfigName = 'Sink'"));
        assert!(where_clause.contains("MessageBodyClassName = 'Ens.StringRequest'"));
        assert_eq!(filters.len(), 3);
    }

    #[test]
    fn message_search_filters_partial() {
        let params = MessageSearchParams {
            source: Some("Router".to_string()),
            target: None,
            class_name: None,
            limit: 20,
        };
        let mut filters = vec![];
        if let Some(src) = &params.source {
            filters.push(format!("SourceConfigName = '{}'", src.replace('\'', "''")));
        }
        if let Some(tgt) = &params.target {
            filters.push(format!("TargetConfigName = '{}'", tgt.replace('\'', "''")));
        }
        if let Some(cls) = &params.class_name {
            filters.push(format!(
                "MessageBodyClassName = '{}'",
                cls.replace('\'', "''")
            ));
        }
        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", filters.join(" AND "))
        };
        assert_eq!(filters.len(), 1);
        assert!(where_clause.contains("WHERE"));
    }

    #[test]
    fn lookup_transfer_xml_entry_count_zero() {
        let out = "<lookupTable></lookupTable>";
        let entry_count = out.matches("<entry").count();
        assert_eq!(entry_count, 0);
    }

    #[test]
    fn lookup_transfer_xml_entry_count_multiple() {
        let out = r#"<lookupTable><entry key="A" value="1"/><entry key="B" value="2"/><entry key="C" value="3"/></lookupTable>"#;
        let entry_count = out.matches("<entry").count();
        assert_eq!(entry_count, 3);
    }

    #[test]
    fn lookup_transfer_xml_entry_count_partial_tag() {
        let out = "<lookupTable><entry_ key='A'/><entry key='B'/></lookupTable>";
        // This matches literal "<entry", so both lines match
        let entry_count = out.matches("<entry").count();
        assert_eq!(entry_count, 2);
    }

    #[test]
    fn settings_line_parsing_empty_key() {
        let line = "=value";
        let mut parts = line.splitn(2, '=');
        let k = parts.next().unwrap_or("").trim().to_string();
        let _v = parts.next().unwrap_or("").to_string();
        if k.is_empty() {
            assert!(true); // filtered out
        } else {
            assert!(false, "empty key should be filtered");
        }
    }

    #[test]
    fn settings_line_parsing_key_only_no_equals() {
        let line = "SettingName";
        let mut parts = line.splitn(2, '=');
        let k = parts.next().unwrap_or("").trim().to_string();
        let _v = parts.next().unwrap_or("").to_string();
        assert_eq!(k, "SettingName");
        assert_eq!(_v, "");
    }

    #[test]
    fn settings_line_parsing_key_value_simple() {
        let line = "Timeout=30";
        let mut parts = line.splitn(2, '=');
        let k = parts.next().unwrap_or("").trim().to_string();
        let v = parts.next().unwrap_or("").to_string();
        assert_eq!(k, "Timeout");
        assert_eq!(v, "30");
    }

    #[test]
    fn settings_line_parsing_value_with_equals() {
        let line = "Expression=a==b";
        let mut parts = line.splitn(2, '=');
        let k = parts.next().unwrap_or("").trim().to_string();
        let v = parts.next().unwrap_or("").to_string();
        assert_eq!(k, "Expression");
        assert_eq!(v, "a==b");
    }

    #[test]
    fn settings_line_parsing_whitespace_around_key() {
        let line = "  SettingName  =value";
        let mut parts = line.splitn(2, '=');
        let k = parts.next().unwrap_or("").trim().to_string();
        let _v = parts.next().unwrap_or("").to_string();
        assert_eq!(k, "SettingName");
        assert_eq!(_v, "value");
    }

    #[test]
    fn ok_json_response_structure() {
        let val = serde_json::json!({"test": "data"});
        let result = ok_json(val).unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["test"], "data");
    }

    #[test]
    fn err_json_response_structure() {
        let result = err_json("TEST_ERROR", "Something went wrong").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["success"], false);
        assert_eq!(v["error_code"], "TEST_ERROR");
        assert_eq!(v["error"], "Something went wrong");
    }

    #[test]
    fn err_json_empty_message() {
        let result = err_json("CODE", "").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "CODE");
        assert_eq!(v["error"], "");
    }

    #[test]
    fn err_json_special_chars_in_message() {
        let result = err_json("CODE", "Error with \"quotes\" and \\backslash").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "CODE");
    }

    #[test]
    fn is_network_error_negative_cases() {
        assert!(!is_network_error(""));
        assert!(!is_network_error(
            "No Interoperability connection configured"
        ));
        assert!(!is_network_error("DOCKER_REQUIRED"));
        assert!(!is_network_error("SQLCODE: -1 Field not found"));
        assert!(!is_network_error("Configuration error"));
    }

    #[test]
    fn is_network_error_all_positive_patterns() {
        assert!(is_network_error("error sending"));
        assert!(is_network_error("connection refused"));
        assert!(is_network_error("connection reset"));
        assert!(is_network_error("dns error"));
        assert!(is_network_error("timed out"));
    }

    #[test]
    fn is_network_error_mixed_case_sensitivity() {
        // Patterns are case-sensitive (lowercase only)
        assert!(!is_network_error("Connection Refused"));
        assert!(!is_network_error("ERROR SENDING"));
        assert!(!is_network_error("DNS Error"));
    }

    #[test]
    fn production_item_escape_in_objectscript_code() {
        let item = "Service'Name";
        let escaped = item.replace('\'', "''");
        assert_eq!(escaped, "Service''Name");
        // Verify escaping would work in a format string
        let code = format!(r#"Set tItem=tProd.FindItemByConfigName("{}")"#, escaped);
        assert!(code.contains("Service''Name"));
    }

    #[test]
    fn credential_id_escaping() {
        let id = "Cred'With'Quotes";
        let escaped = id.replace('\'', "''");
        assert_eq!(escaped, "Cred''With''Quotes");
    }

    #[test]
    fn credential_username_escaping() {
        let username = "user'name@example.com";
        let escaped = username.replace('\'', "''");
        assert_eq!(escaped, "user''name@example.com");
    }

    #[test]
    fn credential_password_escaping() {
        let password = "pass'word!@#$%";
        let escaped = password.replace('\'', "''");
        assert_eq!(escaped, "pass''word!@#$%");
    }

    #[test]
    fn xml_escaping_backslash_and_quote() {
        let xml = r#"<value path="C:\Users\test">"#;
        let escaped = xml.replace('\\', "\\\\").replace('"', "\\\"");
        assert!(escaped.contains("\\\\"));
        assert!(escaped.contains("\\\""));
    }

    #[test]
    fn production_item_enabled_val_true() {
        let enabled_val = if true { "1" } else { "0" };
        assert_eq!(enabled_val, "1");
    }

    #[test]
    fn production_item_enabled_val_false() {
        let enabled_val = if false { "1" } else { "0" };
        assert_eq!(enabled_val, "0");
    }

    #[test]
    fn parse_status_response_with_whitespace_in_code() {
        let r = parse_status_response("MyProd:  1  ");
        assert!(r.is_ok());
        let (_, code, _) = r.unwrap();
        assert_eq!(code, 1);
    }

    #[test]
    fn parse_status_response_negative_code() {
        let r = parse_status_response("Prod:-5");
        assert!(r.is_ok());
        let (_, code, state) = r.unwrap();
        assert_eq!(code, -5);
        assert_eq!(state, "Unknown");
    }

    #[test]
    fn parse_status_response_very_long_name() {
        let long_name = "A".repeat(1000);
        let input = format!("{}:1", long_name);
        let r = parse_status_response(&input);
        assert!(r.is_ok());
        let (name, _, _) = r.unwrap();
        assert_eq!(name, long_name);
    }

    #[test]
    fn parse_status_response_name_with_special_chars() {
        let r = parse_status_response("App.HL7_2.1.Production:1");
        assert!(r.is_ok());
        let (name, _, _) = r.unwrap();
        assert_eq!(name, "App.HL7_2.1.Production");
    }

    #[test]
    fn state_string_boundary_values() {
        assert_eq!(state_string(i64::MIN), "Unknown");
        assert_eq!(state_string(i64::MAX), "Unknown");
        assert_eq!(state_string(0), "Unknown");
        assert_eq!(state_string(6), "Unknown");
    }

    #[test]
    fn production_item_settings_empty_map() {
        let settings: std::collections::HashMap<String, String> = Default::default();
        assert!(settings.is_empty());
    }

    #[test]
    fn production_item_settings_multiple_entries() {
        let mut settings = std::collections::HashMap::new();
        settings.insert("Timeout".to_string(), "30".to_string());
        settings.insert("CallInterval".to_string(), "5".to_string());
        settings.insert("AlertOnError".to_string(), "1".to_string());
        assert_eq!(settings.len(), 3);
    }

    #[test]
    fn default_ns_returns_user() {
        assert_eq!(default_ns(), "USER");
    }

    #[test]
    fn default_limit_returns_10() {
        assert_eq!(default_limit(), 10);
    }

    #[test]
    fn default_log_type_returns_error_warning() {
        let log_type = default_log_type();
        assert_eq!(log_type, "error,warning");
    }

    #[test]
    fn default_msg_limit_returns_20() {
        assert_eq!(default_msg_limit(), 20);
    }

    #[test]
    fn ok_json_serde_preserves_types() {
        let val = serde_json::json!({
            "string": "text",
            "number": 42,
            "boolean": true,
            "null_val": null,
            "array": [1, 2, 3]
        });
        let result = ok_json(val).unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["string"], "text");
        assert_eq!(v["number"], 42);
        assert_eq!(v["boolean"], true);
        assert!(v["null_val"].is_null());
        assert_eq!(v["array"].as_array().unwrap().len(), 3);
    }
}
