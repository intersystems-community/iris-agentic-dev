//! iris_doc — document CRUD via Atelier REST v8.
//! Handles get/put/delete/head with ETag conflict retry and optional SCM hooks.

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DocMode {
    Get,
    Put,
    Delete,
    Head,
    Fragment,
    Compiled,
    List,
}

fn default_mode() -> DocMode {
    DocMode::Get
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IrisDocParams {
    /// Operation: get=fetch source, put=write, delete=remove, head=check existence. Defaults to "get".
    #[serde(default = "default_mode", alias = "action")]
    pub mode: DocMode,
    /// Document name e.g. 'MyApp.Patient.cls'
    #[serde(alias = "document")]
    pub name: Option<String>,
    /// Multiple document names for batch get/delete
    #[serde(default)]
    pub names: Vec<String>,
    /// Source content (required for mode=put)
    pub content: Option<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    /// Elicitation resume ID (from a prior elicitation_required response)
    pub elicitation_id: Option<String>,
    /// User's answer to the elicitation question ("yes" or "no")
    pub elicitation_answer: Option<String>,
    /// If true and mode=put, compile the document after writing (default false).
    /// Saves a round-trip vs calling iris_doc(put) then iris_compile separately.
    #[serde(default)]
    pub compile: bool,
    // mode=fragment params
    /// Fragment start line, 1-based inclusive (required for mode=fragment)
    pub start: Option<i64>,
    /// Fragment end line, 1-based inclusive (required for mode=fragment)
    pub end: Option<i64>,
    // mode=compiled params
    /// Compiled form type: "INT" (default) or "OBJ"
    pub compiled_type: Option<String>,
    // mode=list params
    /// Glob pattern for mode=list (required, e.g. "User.*" or "MyApp.*.cls")
    pub pattern: Option<String>,
    /// Document category filter: "CLS", "MAC", "INT", "INC", or "ALL" (default "ALL")
    pub category: Option<String>,
    /// Max results for mode=list (default 200, max 1000)
    pub max_results: Option<i64>,
}

fn default_namespace() -> String {
    "USER".to_string()
}
use crate::iris::connection::IrisConnection;

fn ok_json(v: serde_json::Value) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    Ok(rmcp::model::CallToolResult::success(vec![
        rmcp::model::Content::text(v.to_string()),
    ]))
}
fn err_json(code: &str, msg: &str) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    ok_json(serde_json::json!({"success": false, "error_code": code, "error": msg}))
}
/// Map a non-2xx HTTP status to an accurate error code.
/// IRIS_UNREACHABLE is reserved for transport errors (reqwest send() failures).
fn http_err_json(
    status: reqwest::StatusCode,
    body_hint: &str,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let code = match status.as_u16() {
        400 => "BAD_REQUEST",
        401 | 403 => "AUTH_ERROR",
        409 => "CONFLICT",
        423 => "LOCKED",
        404 => "NOT_FOUND",
        s if s >= 500 => "SERVER_ERROR",
        _ => "HTTP_ERROR",
    };
    let msg = if body_hint.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {body_hint}")
    };
    err_json(code, &msg)
}

pub async fn handle_iris_doc(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: IrisDocParams,
    elicitation_store: &crate::elicitation::ElicitationStore,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    match p.mode {
        DocMode::Get => handle_get(iris, client, p).await,
        DocMode::Put => handle_put(iris, client, p, elicitation_store).await,
        DocMode::Delete => handle_delete(iris, client, p).await,
        DocMode::Head => handle_head(iris, client, p).await,
        DocMode::Fragment => handle_fragment(iris, client, p).await,
        DocMode::Compiled => handle_compiled(iris, client, p).await,
        DocMode::List => handle_list(iris, client, p).await,
    }
}

async fn handle_get(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: IrisDocParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    // Batch get — Bug 19: fetch concurrently instead of sequentially.
    if !p.names.is_empty() {
        // Build a fresh client for batch gets with a shorter timeout so concurrent
        // requests fail fast and the handler returns within the MCP response deadline.
        let batch_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .danger_accept_invalid_certs(
                std::env::var("IRIS_INSECURE")
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(false),
            )
            .build()
            .unwrap_or_else(|_| client.clone());
        let mut set = tokio::task::JoinSet::new();
        for name in &p.names {
            let url =
                iris.versioned_ns_url(&p.namespace, &format!("/doc/{}", urlencoding::encode(name)));
            let username = iris.username.clone();
            let password = iris.password.clone();
            let name = name.clone();
            let c = batch_client.clone();
            set.spawn(async move {
                let result = c
                    .get(&url)
                    .basic_auth(&username, Some(&password))
                    .send()
                    .await;
                (name, result)
            });
        }
        // Collect results, preserving insertion order via a map then re-order.
        let mut map: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::new();
        while let Some(res) = set.join_next().await {
            if let Ok((name, fetch_result)) = res {
                let entry = match fetch_result {
                    Ok(resp) if resp.status().is_success() => {
                        let body: serde_json::Value = resp.json().await.unwrap_or_default();
                        let content = doc_content_to_string(&body);
                        serde_json::json!({"name": name, "content": content})
                    }
                    Ok(resp) => {
                        serde_json::json!({"name": name, "error": format!("HTTP {}", resp.status())})
                    }
                    Err(e) => serde_json::json!({"name": name, "error": e.to_string()}),
                };
                map.insert(name, entry);
            }
        }
        let results: Vec<_> = p.names.iter().filter_map(|n| map.remove(n)).collect();
        return ok_json(serde_json::json!({"success": true, "documents": results}));
    }

    let name = p.name.as_deref().unwrap_or("");
    let url = iris.versioned_ns_url(&p.namespace, &format!("/doc/{}", urlencoding::encode(name)));
    let resp = client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;

    let status = resp.status();
    if status.as_u16() == 404 {
        return err_json("NOT_FOUND", &format!("Document not found: {name}"));
    }
    if !status.is_success() {
        let body_hint = resp.text().await.unwrap_or_default();
        return http_err_json(status, body_hint.trim());
    }

    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    let content = doc_content_to_string(&body);
    let ts = body["result"]["content"][0]["ts"]
        .as_str()
        .unwrap_or("")
        .to_string();
    ok_json(serde_json::json!({"success": true, "name": name, "content": content, "timestamp": ts}))
}

async fn handle_put(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: IrisDocParams,
    elicitation_store: &crate::elicitation::ElicitationStore,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let name = p.name.as_deref().unwrap_or("");
    let ns = &p.namespace;

    // Elicitation resume — user answered a prior SCM dialog
    if let (Some(eid), Some(answer)) = (&p.elicitation_id, &p.elicitation_answer) {
        if let Some(pending) = elicitation_store.lookup(eid) {
            elicitation_store.clear(eid);
            if answer.to_lowercase() != "yes" {
                return ok_json(
                    serde_json::json!({"success": false, "error_code": "WRITE_ABORTED", "error": "User declined checkout"}),
                );
            }
            // User said yes — proceed with the stored content directly
            let resume_content = pending.content.as_deref().unwrap_or("");
            return do_write(
                iris,
                client,
                &pending.document,
                resume_content,
                &pending.namespace,
                p.compile,
            )
            .await;
        }
        return err_json(
            "ELICITATION_EXPIRED",
            "Elicitation session expired or not found",
        );
    }

    // Inject ROUTINE header for .mac/.inc if missing
    let raw_content = p.content.as_deref().unwrap_or("");
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    let routine_name = name.rsplit_once('.').map(|(n, _)| n).unwrap_or(name);
    let needs_header = !raw_content
        .trim_start()
        .to_uppercase()
        .starts_with("ROUTINE ");
    let content_owned: String;
    let content: &str = match ext.as_str() {
        "mac" if needs_header => {
            content_owned = format!("ROUTINE {}\n{}", routine_name, raw_content);
            &content_owned
        }
        "inc" if needs_header => {
            content_owned = format!("ROUTINE {} [Type=INC]\n{}", routine_name, raw_content);
            &content_owned
        }
        _ => raw_content,
    };

    // SCM pre-write check — uses SourceControlCreate for a proper session (HTTP-compatible).
    // %GetImplementationObject does not exist on any IRIS version; use Interface API instead.
    let n = name.replace('"', "\"\""); // ObjectScript double-quote escaping
    let scm_check = format!(
        "set scmClass=##class(%Studio.SourceControl.Interface).SourceControlClassGet() if scmClass=\"\" {{ write \"NO_SCM\" }} else {{ set sc=##class(%Studio.SourceControl.Interface).SourceControlCreate(\"{u}\",\"{p}\",.c,.f,.o) set obj=$get(%SourceControl) if '$IsObject(obj) {{ write \"NO_SCM\" }} else {{ set action=0 set msg=\"\" set target=\"\" set reload=0 set sc=obj.UserAction(0,\"%SourceMenu,%CheckOut\",\"{n}\",\"\",.action,.target,.msg,.reload) write action_\"|\"_msg }} }}",
        u = iris.username.replace('"', "\"\""),
        p = iris.password.replace('"', "\"\""),
    );
    if let Ok(out) = iris.execute_via_generator(&scm_check, ns, client).await {
        let out = out.trim().to_string();
        if out != "NO_SCM" && !out.is_empty() {
            let parts: Vec<&str> = out.splitn(2, '|').collect();
            let action_code = parts
                .first()
                .and_then(|s| s.trim().parse::<u8>().ok())
                .unwrap_or(0);
            let msg = parts.get(1).map(|s| s.trim()).unwrap_or("");

            if action_code == 1 {
                let eid = elicitation_store.insert(
                    name,
                    crate::elicitation::ElicitationAction::Put,
                    Some(content.to_string()),
                    None,
                    ns.clone(),
                );
                return ok_json(serde_json::json!({
                    "success": false,
                    "elicitation_required": true,
                    "elicitation_id": eid,
                    "message": if msg.is_empty() { format!("{} requires checkout. Check out and write?", name) } else { msg.to_string() },
                    "options": ["yes", "no"],
                }));
            } else if action_code == 6 {
                return err_json("SCM_REJECTED", &format!("Source control rejected: {}", msg));
            }
            // action_code == 0: proceed
        }
    }

    do_write(iris, client, name, content, ns, p.compile).await
}

async fn do_write(
    iris: &IrisConnection,
    client: &reqwest::Client,
    name: &str,
    content: &str,
    namespace: &str,
    compile_after: bool,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    // I-3: strip Storage blocks — IRIS 2025.1 UDL parser (#5559) fails on Storage XML.
    // IRIS will auto-generate correct storage on first compile.
    // strip_storage_blocks handles the no-block case cheaply (single pass, no alloc).
    let (content_for_write, storage_stripped) = strip_storage_blocks(content);
    let lines: Vec<&str> = content_for_write.lines().collect();

    // I-4: use ?ignoreConflict=1 — IRIS accepts the write unconditionally, never returns 409.
    let url = iris.versioned_ns_url(
        namespace,
        &format!("/doc/{}?ignoreConflict=1", urlencoding::encode(name)),
    );

    let resp = client
        .put(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .json(&serde_json::json!({"enc": false, "content": lines}))
        .send()
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;

    let put_status = resp.status();
    if !put_status.is_success() {
        let body_hint = resp.text().await.unwrap_or_default();
        return http_err_json(put_status, body_hint.trim());
    }
    // Check body for Atelier-level errors (200 OK with status.errors, e.g. build 110
    // SetTextFromString NULL namespace bug via web gateway).
    let put_body: serde_json::Value = resp.json().await.unwrap_or_default();
    if let Some(errs) = put_body["status"]["errors"].as_array() {
        if !errs.is_empty() {
            let msg = errs[0]["error"]
                .as_str()
                .unwrap_or("Document upload failed");
            return err_json("UPLOAD_FAILED", msg);
        }
    }

    // Write open hint for VS Code auto-open
    crate::tools::write_open_hint(namespace, name);

    let open_uri = format!("isfs://{}/{}", namespace, name);

    if compile_after {
        let compile_url = iris.versioned_ns_url(namespace, "/action/compile?flags=cuk");
        let compile_resp = client
            .post(&compile_url)
            .basic_auth(&iris.username, Some(&iris.password))
            .json(&serde_json::json!([name]))
            .send()
            .await;

        let (compile_ok, compile_errors, compile_console) = match compile_resp {
            Err(e) => (false, vec![e.to_string()], vec![]),
            Ok(r) => {
                // Non-2xx (e.g. HTTP 400 on concurrent compile conflict) means compile did not run.
                let compile_status = r.status();
                if !compile_status.is_success() {
                    let hint = r.text().await.unwrap_or_default();
                    let msg = format!(
                        "Compile request failed: HTTP {} {}",
                        compile_status,
                        hint.trim()
                    );
                    return err_json("COMPILE_FAILED", &msg);
                }
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                let console: Vec<String> = body["console"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut errs: Vec<String> = vec![];
                if let Some(se) = body["status"]["errors"].as_array() {
                    for e in se {
                        if let Some(msg) = e["error"].as_str() {
                            errs.push(msg.to_string());
                        }
                    }
                }
                for line in &console {
                    if line.trim().starts_with("ERROR ") {
                        let msg = line.trim().to_string();
                        if errs.iter().all(|e| !e.contains(line.trim())) {
                            errs.push(msg);
                        }
                    }
                }
                (errs.is_empty(), errs, console)
            }
        };

        return ok_json(serde_json::json!({
            "success": compile_ok,
            "name": name,
            "open_uri": open_uri,
            "storage_stripped": storage_stripped,
            "compiled": compile_ok,
            "compile_errors": compile_errors,
            "compile_console": compile_console,
        }));
    }

    ok_json(
        serde_json::json!({"success": true, "name": name, "open_uri": open_uri, "storage_stripped": storage_stripped}),
    )
}

async fn handle_delete(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: IrisDocParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    // Batch delete
    if !p.names.is_empty() {
        let mut deleted = vec![];
        let mut errors = vec![];
        for name in &p.names {
            let url =
                iris.versioned_ns_url(&p.namespace, &format!("/doc/{}", urlencoding::encode(name)));
            match client
                .delete(&url)
                .basic_auth(&iris.username, Some(&iris.password))
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => deleted.push(name.clone()),
                Ok(r) => errors.push(
                    serde_json::json!({"name": name, "error": format!("HTTP {}", r.status())}),
                ),
                Err(e) => errors.push(serde_json::json!({"name": name, "error": e.to_string()})),
            }
        }
        return ok_json(
            serde_json::json!({"success": errors.is_empty(), "deleted": deleted, "errors": errors}),
        );
    }

    let name = p.name.as_deref().unwrap_or("");
    let url = iris.versioned_ns_url(&p.namespace, &format!("/doc/{}", urlencoding::encode(name)));
    let resp = client
        .delete(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;

    let del_status = resp.status();
    if del_status.as_u16() == 404 {
        return err_json("NOT_FOUND", &format!("Document not found: {name}"));
    }
    if !del_status.is_success() {
        let body_hint = resp.text().await.unwrap_or_default();
        return http_err_json(del_status, body_hint.trim());
    }
    ok_json(serde_json::json!({"success": true, "name": name}))
}

async fn handle_head(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: IrisDocParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let name = p.name.as_deref().unwrap_or("");
    let url = iris.versioned_ns_url(&p.namespace, &format!("/doc/{}", urlencoding::encode(name)));
    let resp = client
        .head(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;

    let exists = resp.status().is_success();
    let ts = resp
        .headers()
        .get("ETag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    ok_json(serde_json::json!({"success": true, "name": name, "exists": exists, "timestamp": ts}))
}

// ── Phase 2: Foundational helpers ────────────────────────────────────────────

/// Clamp max_results to [1, 1000].
pub fn clamp_max_results(v: i64) -> i64 {
    v.clamp(1, 1000)
}

/// Validate a glob pattern for mode=list.
/// Rejects empty string, bare "*", "**", or patterns starting with "*" (no prefix).
pub fn validate_list_pattern(pattern: &str) -> Result<(), serde_json::Value> {
    if pattern.is_empty() || pattern == "*" || pattern == "**" || pattern.starts_with('*') {
        return Err(serde_json::json!({
            "success": false,
            "error_code": "MISSING_PARAMS",
            "error": "pattern must have a non-wildcard prefix (e.g. 'User.*', 'MyApp.*.cls')"
        }));
    }
    Ok(())
}

/// Slice a line array by 1-based start/end range.
/// Returns (sliced_lines, actual_start, actual_end, was_clamped).
pub fn slice_lines(lines: &[String], start: i64, end: i64) -> (Vec<String>, i64, i64, bool) {
    let len = lines.len() as i64;
    if len == 0 || start > len {
        return (vec![], start, start, true);
    }
    let actual_start = start.max(1);
    let actual_end_raw = end;
    let actual_end = actual_end_raw.min(len);
    let clamped = actual_end < actual_end_raw;
    let s = (actual_start - 1) as usize;
    let e = actual_end as usize;
    (lines[s..e].to_vec(), actual_start, actual_end, clamped)
}

// ── Phase 3: mode=fragment ────────────────────────────────────────────────────

async fn handle_fragment(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: IrisDocParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let name = p.name.as_deref().unwrap_or("");
    let start = match p.start {
        Some(v) => v.max(1),
        None => return err_json("MISSING_PARAMS", "start is required for mode=fragment"),
    };
    let end = match p.end {
        Some(v) => v,
        None => return err_json("MISSING_PARAMS", "end is required for mode=fragment"),
    };
    if end < start {
        return err_json(
            "INVALID_PARAMS",
            &format!("start ({start}) must be <= end ({end})"),
        );
    }

    let url = iris.versioned_ns_url(&p.namespace, &format!("/doc/{}", urlencoding::encode(name)));
    let resp = client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(format!("HTTP error: {e}"), None))?;

    let status = resp.status();
    if status.as_u16() == 404 {
        return err_json("NOT_FOUND", &format!("Document not found: {name}"));
    }
    if !status.is_success() {
        let body_hint = resp.text().await.unwrap_or_default();
        return http_err_json(status, body_hint.trim());
    }

    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    let all_lines: Vec<String> = body["result"]["content"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let total_lines = all_lines.len() as i64;
    let (sliced, actual_start, actual_end, clamped) = slice_lines(&all_lines, start, end);
    ok_json(serde_json::json!({
        "success": true,
        "name": name,
        "lines": sliced,
        "start": actual_start,
        "end": actual_end,
        "clamped": clamped,
        "total_lines": total_lines,
    }))
}

// ── Phase 4: mode=compiled ────────────────────────────────────────────────────

/// Derive the IRIS routine name from a document name.
/// .cls → strip extension, append ".1"
/// .mac → strip extension
/// .int → use as-is (strip extension)
/// .inc → None (no INT form)
fn derive_routine_name(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    if lower.ends_with(".inc") {
        return None; // include files have no INT form
    }
    if lower.ends_with(".cls") {
        let base = &name[..name.len() - 4];
        return Some(format!("{base}.1"));
    }
    if lower.ends_with(".mac") {
        return Some(name[..name.len() - 4].to_string());
    }
    if lower.ends_with(".int") {
        return Some(name[..name.len() - 4].to_string());
    }
    // Unknown extension — try stripping it
    if let Some(dot) = name.rfind('.') {
        Some(name[..dot].to_string())
    } else {
        Some(name.to_string())
    }
}

async fn handle_compiled(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: IrisDocParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let name = p.name.as_deref().unwrap_or("");
    let lower = name.to_lowercase();

    // .INC files have no INT form
    if lower.ends_with(".inc") {
        return err_json(
            "NOT_COMPILED",
            "Include files (.INC) do not compile to INT form",
        );
    }

    // Validate compiled_type
    if let Some(ref ct) = p.compiled_type {
        if ct.to_uppercase() != "INT" {
            // OBJ is not yet implemented; any other value is invalid
            return err_json(
                "INVALID_PARAMS",
                &format!("compiled_type '{ct}' not supported; only 'INT' is supported in v1"),
            );
        }
    }

    let routine = match derive_routine_name(name) {
        Some(r) => r,
        None => {
            return err_json(
                "NOT_COMPILED",
                "Cannot determine routine name for this document type",
            )
        }
    };

    let code = format!(
        " Set rtn = ##class(%Library.Routine).%OpenId(\"{routine}.INT\")\n If rtn = \"\" {{ Write \"NOT_COMPILED\",$C(10)  Quit }}\n Do rtn.Rewind()\n While 'rtn.AtEnd {{ Write rtn.ReadLine(),$C(10) }}\n Write \"DONE\",$C(10)"
    );

    let output = match iris
        .execute_via_generator(&code, &p.namespace, client)
        .await
    {
        Ok(s) => s,
        Err(e) => return err_json("IRIS_EXECUTE_ERROR", &e.to_string()),
    };

    let first_line = output.lines().next().unwrap_or("").trim();
    if first_line == "NOT_COMPILED" {
        return err_json(
            "NOT_COMPILED",
            &format!("No compiled INT form found for '{name}'"),
        );
    }

    // Collect lines until DONE sentinel
    let mut content_lines: Vec<&str> = Vec::new();
    for line in output.lines() {
        if line.trim() == "DONE" {
            break;
        }
        content_lines.push(line);
    }
    let content = content_lines.join("\n");
    let total_lines = content_lines.len() as i64;

    ok_json(serde_json::json!({
        "success": true,
        "name": name,
        "routine": routine,
        "category": "INT",
        "content": content,
        "total_lines": total_lines,
    }))
}

// ── Phase 5: mode=list ────────────────────────────────────────────────────────

/// Convert a glob pattern to a regex string.
/// * → .*, ? → ., dots escaped.
fn glob_to_regex(pattern: &str) -> String {
    let mut re = String::from("(?i)^");
    for ch in pattern.chars() {
        match ch {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            '.' => re.push_str("\\."),
            c => re.push(c),
        }
    }
    re.push('$');
    re
}

async fn fetch_docnames_for_cat(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
    cat: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let url = iris.versioned_ns_url(namespace, &format!("/docnames/{cat}"));
    let resp = client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    let docs = body["result"]["content"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(docs)
}

async fn handle_list(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: IrisDocParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let pattern = match p.pattern.as_deref() {
        Some(pat) => pat,
        None => return err_json("MISSING_PARAMS", "pattern is required for mode=list"),
    };

    if let Err(e) = validate_list_pattern(pattern) {
        return ok_json(e);
    }

    let category = p.category.as_deref().unwrap_or("ALL").to_uppercase();
    let allowed = ["CLS", "MAC", "INT", "INC", "ALL"];
    if !allowed.contains(&category.as_str()) {
        return err_json(
            "INVALID_PARAMS",
            &format!("category '{category}' not valid; use CLS, MAC, INT, INC, or ALL"),
        );
    }

    let max_results = clamp_max_results(p.max_results.unwrap_or(200));

    // Fetch docs for selected categories
    let cats: &[&str] = if category == "ALL" {
        &["CLS", "MAC", "INT", "INC"]
    } else {
        // We'll build a single-element slice from the string
        match category.as_str() {
            "CLS" => &["CLS"],
            "MAC" => &["MAC"],
            "INT" => &["INT"],
            "INC" => &["INC"],
            _ => &["CLS"],
        }
    };

    let re_str = glob_to_regex(pattern);
    let re = match regex::Regex::new(&re_str) {
        Ok(r) => r,
        Err(e) => {
            return err_json(
                "INVALID_PARAMS",
                &format!("invalid pattern '{pattern}': {e}"),
            )
        }
    };

    let mut all_docs: Vec<serde_json::Value> = Vec::new();
    for cat in cats {
        match fetch_docnames_for_cat(iris, client, &p.namespace, cat).await {
            Ok(docs) => all_docs.extend(docs),
            Err(e) => {
                return err_json(
                    "SERVER_ERROR",
                    &format!("failed to fetch {cat} docnames: {e}"),
                )
            }
        }
    }

    // Filter by pattern
    let mut matched: Vec<serde_json::Value> = all_docs
        .into_iter()
        .filter(|doc| {
            doc["name"]
                .as_str()
                .map(|n| re.is_match(n))
                .unwrap_or(false)
        })
        .map(|doc| {
            serde_json::json!({
                "name": doc["name"],
                "category": doc["cat"],
                "ts": doc["ts"],
            })
        })
        .collect();

    let total = matched.len();
    let truncated = total > max_results as usize;
    matched.truncate(max_results as usize);
    let count = matched.len() as i64;

    ok_json(serde_json::json!({
        "success": true,
        "documents": matched,
        "count": count,
        "truncated": truncated,
        "namespace": p.namespace,
    }))
}

// ── 053: iris_execute_method handler ─────────────────────────────────────────

pub async fn handle_iris_execute_method(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &crate::tools::IrisExecuteMethodParams,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let class = &p.class;
    let method = &p.method;

    // Injection guard: reject class/method containing { } or ;
    for ch in ['{', '}', ';'] {
        if class.contains(ch) || method.contains(ch) {
            return err_json(
                "INVALID_PARAMS",
                "class and method names must not contain '{', '}', or ';'",
            );
        }
    }

    // Build CSV args with ObjectScript double-quote escaping
    let args_csv: String = p
        .args
        .iter()
        .map(|a| format!("\"{}\"", a.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");

    let call_expr = if args_csv.is_empty() {
        format!("##class({class}).{method}()")
    } else {
        format!("##class({class}).{method}({args_csv})")
    };

    let code = format!(" Set result = {call_expr}\n Write result,$C(10)");

    let output = match iris
        .execute_via_generator(&code, &p.namespace, client)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            let msg = e.to_string();
            return err_json("IRIS_EXECUTE_ERROR", &msg);
        }
    };

    // Check for generator-level errors (Catch block writes "ERROR: ...")
    let trimmed = output.trim();
    if let Some(stripped) = trimmed.strip_prefix("ERROR: ") {
        return err_json("IRIS_EXECUTE_ERROR", stripped.trim());
    }

    // Take first line only — the method may write side-effects on subsequent lines
    let return_value = output.lines().next().unwrap_or("").to_string();

    ok_json(serde_json::json!({
        "success": true,
        "return_value": return_value,
    }))
}

/// Strip `Storage Name { ... }` blocks from ObjectScript class content.
/// Returns (content_without_storage, storage_was_present).
/// IRIS 2025.1 UDL parser fails on explicit Storage XML blocks (#5559);
/// omitting them lets IRIS auto-generate correct storage on first compile.
pub fn strip_storage_blocks(content: &str) -> (String, bool) {
    let mut result = Vec::new();
    let mut in_storage = false;
    let mut brace_depth: i32 = 0;
    let mut found = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if !in_storage {
            // Detect start of Storage block: "Storage Name" or "Storage Name {"
            let is_storage_start = {
                let mut parts = trimmed.split_whitespace();
                parts.next() == Some("Storage") && parts.next().is_some()
            };
            if is_storage_start {
                in_storage = true;
                found = true;
                // Count any opening braces on this line
                brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
                brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;
                if brace_depth <= 0 {
                    // Single-line storage (rare) — done immediately
                    in_storage = false;
                    brace_depth = 0;
                }
                continue; // skip this line
            }
            result.push(line);
        } else {
            // Inside storage block — track brace depth
            brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
            brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;
            if brace_depth <= 0 {
                in_storage = false;
                brace_depth = 0;
                // Don't add this closing-brace line to result
            }
            // Skip all lines inside storage block
        }
    }

    if found {
        // Remove trailing blank lines that were before the storage block
        while result
            .last()
            .map(|l: &&str| l.trim().is_empty())
            .unwrap_or(false)
        {
            result.pop();
        }
        (result.join("\n") + "\n", true)
    } else {
        (content.to_string(), false)
    }
}

fn doc_content_to_string(body: &serde_json::Value) -> String {
    // Atelier GET /doc/<name> returns result.content as a flat array of line strings.
    body["result"]["content"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doc_content_to_string_flat_array() {
        let body = serde_json::json!({
            "result": {
                "content": ["Class Foo", "{", "}", ""]
            }
        });
        let s = doc_content_to_string(&body);
        assert!(s.contains("Class Foo"));
        assert!(s.contains("{"));
    }

    #[test]
    fn test_doc_content_to_string_empty_array() {
        let body = serde_json::json!({"result": {"content": []}});
        let s = doc_content_to_string(&body);
        assert_eq!(s, "");
    }

    #[test]
    fn test_doc_content_to_string_missing_result() {
        let body = serde_json::json!({});
        let s = doc_content_to_string(&body);
        assert_eq!(s, "");
    }

    #[test]
    fn test_strip_storage_blocks_single_line_storage() {
        // Storage on one line (unusual but possible)
        let cls = "Class Foo {\nStorage Default {}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag, "should detect storage");
        assert!(!stripped.contains("Storage Default"), "should strip");
    }

    #[test]
    fn test_strip_storage_blocks_preserves_class_wrapper() {
        // Storage block with opening brace on same line as Storage keyword
        let cls = "Class Foo {\nProperty X As %String;\nStorage Default {\n<Type>T</Type>\n}\n}";
        let (stripped, _) = strip_storage_blocks(cls);
        assert!(stripped.contains("Class Foo"), "class wrapper preserved");
        assert!(stripped.contains("Property X"), "property preserved");
        assert!(
            stripped.trim_end().ends_with('}'),
            "closing brace preserved"
        );
    }

    #[test]
    fn test_strip_storage_blocks_inline_brace_strips_content() {
        let cls =
            "Class Foo {\nStorage Default {\n<Data>\n<Value>{ nested }</Value>\n</Data>\n}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag);
        assert!(!stripped.contains("Storage Default"));
        assert!(!stripped.contains("nested"));
    }

    // ── IrisDocParams serde ───────────────────────────────────────────────────
    #[test]
    fn test_iris_doc_params_defaults() {
        let p: IrisDocParams = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(p.namespace, "USER");
        assert!(p.name.is_none());
        assert!(p.names.is_empty());
        assert!(p.content.is_none());
        assert!(!p.compile);
    }

    #[test]
    fn test_iris_doc_params_get_mode_default() {
        let p: IrisDocParams = serde_json::from_str(r#"{"name": "Foo.cls"}"#).unwrap();
        assert!(matches!(p.mode, DocMode::Get));
    }

    #[test]
    fn test_iris_doc_params_put_mode() {
        let p: IrisDocParams = serde_json::from_str(
            r#"{"mode": "put", "name": "Foo.cls", "content": "Class Foo {}"}"#,
        )
        .unwrap();
        assert!(matches!(p.mode, DocMode::Put));
        assert_eq!(p.content.as_deref(), Some("Class Foo {}"));
    }

    #[test]
    fn test_iris_doc_params_mode_alias_action() {
        let p: IrisDocParams =
            serde_json::from_str(r#"{"action": "delete", "name": "Foo.cls"}"#).unwrap();
        assert!(matches!(p.mode, DocMode::Delete));
    }

    #[test]
    fn test_iris_doc_params_with_compile() {
        let p: IrisDocParams =
            serde_json::from_str(r#"{"mode": "put", "name": "Foo.cls", "compile": true}"#).unwrap();
        assert!(p.compile);
    }

    #[test]
    fn test_iris_doc_params_batch_names() {
        let p: IrisDocParams =
            serde_json::from_str(r#"{"names": ["Foo.cls", "Bar.cls"]}"#).unwrap();
        assert_eq!(p.names.len(), 2);
    }

    // ── http_err_json ─────────────────────────────────────────────────────────
    #[test]
    fn test_http_err_json_404_returns_not_found() {
        let result = http_err_json(reqwest::StatusCode::NOT_FOUND, "").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("NOT_FOUND"), "{text}");
    }

    #[test]
    fn test_http_err_json_401_returns_auth_error() {
        let result = http_err_json(reqwest::StatusCode::UNAUTHORIZED, "").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("AUTH_ERROR"), "{text}");
    }

    #[test]
    fn test_http_err_json_500_returns_server_error() {
        let result = http_err_json(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "boom").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("SERVER_ERROR"), "{text}");
        assert!(text.contains("boom"), "{text}");
    }

    #[test]
    fn test_http_err_json_409_returns_conflict() {
        let result = http_err_json(reqwest::StatusCode::CONFLICT, "locked").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("CONFLICT"), "{text}");
    }

    #[test]
    fn test_iris_doc_params_head_mode() {
        let p: IrisDocParams =
            serde_json::from_str(r#"{"mode": "head", "name": "Foo.cls"}"#).unwrap();
        assert!(matches!(p.mode, DocMode::Head));
    }

    #[test]
    fn test_iris_doc_params_delete_mode() {
        let p: IrisDocParams =
            serde_json::from_str(r#"{"mode": "delete", "name": "Foo.cls"}"#).unwrap();
        assert!(matches!(p.mode, DocMode::Delete));
    }

    #[test]
    fn test_iris_doc_params_document_alias_for_name() {
        // "document" is an alias for "name"
        let p: IrisDocParams =
            serde_json::from_str(r#"{"document": "MyApp.Patient.cls"}"#).unwrap();
        assert_eq!(p.name.as_deref(), Some("MyApp.Patient.cls"));
    }

    #[test]
    fn test_iris_doc_params_namespace_override() {
        let p: IrisDocParams =
            serde_json::from_str(r#"{"name": "Foo.cls", "namespace": "MYNS"}"#).unwrap();
        assert_eq!(p.namespace, "MYNS");
    }

    #[test]
    fn test_iris_doc_params_elicitation_fields() {
        let p: IrisDocParams = serde_json::from_str(
            r#"{"mode": "put", "elicitation_id": "abc123", "elicitation_answer": "yes"}"#,
        )
        .unwrap();
        assert_eq!(p.elicitation_id.as_deref(), Some("abc123"));
        assert_eq!(p.elicitation_answer.as_deref(), Some("yes"));
    }

    #[test]
    fn test_iris_doc_params_elicitation_fields_absent_by_default() {
        let p: IrisDocParams = serde_json::from_str(r#"{}"#).unwrap();
        assert!(p.elicitation_id.is_none());
        assert!(p.elicitation_answer.is_none());
    }

    #[test]
    fn test_http_err_json_400_returns_bad_request() {
        let result = http_err_json(reqwest::StatusCode::BAD_REQUEST, "").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "BAD_REQUEST");
        assert_eq!(v["success"], false);
    }

    #[test]
    fn test_http_err_json_403_returns_auth_error() {
        let result = http_err_json(reqwest::StatusCode::FORBIDDEN, "forbidden").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "AUTH_ERROR");
        assert!(v["error"].as_str().unwrap().contains("forbidden"));
    }

    #[test]
    fn test_http_err_json_423_returns_locked() {
        let result =
            http_err_json(reqwest::StatusCode::from_u16(423).unwrap(), "file locked").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "LOCKED");
    }

    #[test]
    fn test_http_err_json_unknown_status_returns_http_error() {
        // 418 I'm a teapot — not explicitly mapped
        let result = http_err_json(reqwest::StatusCode::from_u16(418).unwrap(), "teapot").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], "HTTP_ERROR");
    }

    #[test]
    fn test_http_err_json_empty_hint_omits_colon() {
        let result = http_err_json(reqwest::StatusCode::NOT_FOUND, "").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        // When no body hint, message should be "HTTP 404 Not Found" without a colon suffix
        assert!(!v["error"].as_str().unwrap().contains(": "));
    }

    #[test]
    fn test_http_err_json_with_hint_includes_colon() {
        let result = http_err_json(reqwest::StatusCode::NOT_FOUND, "gone away").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v["error"].as_str().unwrap().contains(": gone away"));
    }

    #[test]
    fn test_strip_storage_blocks_no_storage_returns_unchanged() {
        let cls = "Class Foo {\nProperty X As %String;\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(!flag, "no storage block should be flagged false");
        assert_eq!(stripped, cls);
    }

    #[test]
    fn test_strip_storage_blocks_multiple_storage_blocks() {
        let cls = "Class Foo {\nStorage A {\n<one/>\n}\nStorage B {\n<two/>\n}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag);
        assert!(!stripped.contains("Storage A"));
        assert!(!stripped.contains("Storage B"));
        assert!(stripped.contains("Class Foo"));
    }

    #[test]
    fn test_doc_content_to_string_skips_non_string_elements() {
        // Non-string array elements should be skipped (filter_map with as_str)
        let body = serde_json::json!({
            "result": {
                "content": ["line one", 42, null, "line two"]
            }
        });
        let s = doc_content_to_string(&body);
        assert!(s.contains("line one"));
        assert!(s.contains("line two"));
        // 42 and null have no string representation — they are filtered out
        assert!(!s.contains("42"));
    }

    #[test]
    fn test_strip_storage_blocks_trailing_blank_lines_removed() {
        // Lines 520-526: trailing blank lines before the Storage block are removed.
        let cls =
            "Class Foo {\nProperty X As %String;\n\n\nStorage Default {\n<Type>T</Type>\n}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag, "should detect storage");
        assert!(!stripped.contains("Storage Default"));
        // The blank lines before Storage should be trimmed
        assert!(
            !stripped.ends_with("\n\n"),
            "should not end with multiple blank lines: {:?}",
            stripped
        );
    }

    #[test]
    fn test_strip_storage_blocks_two_named_blocks() {
        // Edge case: class with two named Storage blocks (both should be stripped)
        let cls = "Class Foo {\nProperty X As %String;\nStorage Default {\n<Data/>\n}\nStorage Old {\n<Data/>\n}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag);
        assert!(!stripped.contains("Storage Default"));
        assert!(!stripped.contains("Storage Old"));
        assert!(stripped.contains("Property X"));
    }

    // ── derive_routine_name pure function tests ───────────────────────────────

    #[test]
    fn test_derive_routine_name_cls() {
        // .cls files should map to .1 routine name
        let result = derive_routine_name("MyApp.Foo.cls");
        assert_eq!(result, Some("MyApp.Foo.1".to_string()));
    }

    #[test]
    fn test_derive_routine_name_cls_uppercase() {
        // Uppercase .CLS should also work
        let result = derive_routine_name("MyApp.Foo.CLS");
        assert_eq!(result, Some("MyApp.Foo.1".to_string()));
    }

    #[test]
    fn test_derive_routine_name_cls_mixed_case() {
        let result = derive_routine_name("MyApp.Foo.Cls");
        assert_eq!(result, Some("MyApp.Foo.1".to_string()));
    }

    #[test]
    fn test_derive_routine_name_mac() {
        // .mac files should strip extension
        let result = derive_routine_name("MyRoutine.mac");
        assert_eq!(result, Some("MyRoutine".to_string()));
    }

    #[test]
    fn test_derive_routine_name_mac_uppercase() {
        let result = derive_routine_name("MyRoutine.MAC");
        assert_eq!(result, Some("MyRoutine".to_string()));
    }

    #[test]
    fn test_derive_routine_name_int() {
        // .int files should strip extension
        let result = derive_routine_name("MyRoutine.int");
        assert_eq!(result, Some("MyRoutine".to_string()));
    }

    #[test]
    fn test_derive_routine_name_int_uppercase() {
        let result = derive_routine_name("MyRoutine.INT");
        assert_eq!(result, Some("MyRoutine".to_string()));
    }

    #[test]
    fn test_derive_routine_name_inc_returns_none() {
        // .inc files have no INT form — return None
        let result = derive_routine_name("MyMacros.inc");
        assert_eq!(result, None);
    }

    #[test]
    fn test_derive_routine_name_inc_uppercase_returns_none() {
        let result = derive_routine_name("MyMacros.INC");
        assert_eq!(result, None);
    }

    #[test]
    fn test_derive_routine_name_unknown_ext_strips_ext() {
        // Unknown extension should strip it
        let result = derive_routine_name("MyFile.xyz");
        assert_eq!(result, Some("MyFile".to_string()));
    }

    #[test]
    fn test_derive_routine_name_no_extension() {
        // No extension — return as-is
        let result = derive_routine_name("MyRoutine");
        assert_eq!(result, Some("MyRoutine".to_string()));
    }

    #[test]
    fn test_derive_routine_name_complex_namespace() {
        // Complex multi-level namespace
        let result = derive_routine_name("App.Module.Sub.Class.cls");
        assert_eq!(result, Some("App.Module.Sub.Class.1".to_string()));
    }

    // ── glob_to_regex pure function tests ──────────────────────────────────────

    #[test]
    fn test_glob_to_regex_basic_wildcard() {
        let re_str = glob_to_regex("User.*");
        // Should convert * to .* and escape literal dots
        assert!(re_str.contains("User\\..*"));
        assert!(re_str.starts_with("(?i)^"));
        assert!(re_str.ends_with("$"));
    }

    #[test]
    fn test_glob_to_regex_question_mark() {
        let re_str = glob_to_regex("My?.cls");
        // ? should map to . (any single char)
        assert!(re_str.contains("My."));
        let re = regex::Regex::new(&re_str).unwrap();
        assert!(re.is_match("Myf.cls"));
        assert!(re.is_match("MYF.CLS")); // case-insensitive
    }

    #[test]
    fn test_glob_to_regex_case_insensitive() {
        let re_str = glob_to_regex("User.*");
        let re = regex::Regex::new(&re_str).unwrap();
        // Regex should be case-insensitive (?i prefix)
        assert!(re.is_match("USER.Foo"));
        assert!(re.is_match("user.bar"));
        assert!(re.is_match("User.Test"));
    }

    #[test]
    fn test_glob_to_regex_exact_match() {
        let re_str = glob_to_regex("Exact.Name.cls");
        // No wildcards — exact match with escaping
        assert!(re_str.contains("Exact\\.Name\\.cls"));
        let re = regex::Regex::new(&re_str).unwrap();
        assert!(re.is_match("Exact.Name.cls"));
        assert!(!re.is_match("Exact.Name.clsx"));
    }

    #[test]
    fn test_glob_to_regex_multiple_wildcards() {
        let re_str = glob_to_regex("App.*.Sub.*");
        let re = regex::Regex::new(&re_str).unwrap();
        assert!(re.is_match("App.Module.Sub.Class.cls"));
        assert!(re.is_match("App.X.Sub.Y"));
    }

    #[test]
    fn test_glob_to_regex_all_wildcards() {
        let re_str = glob_to_regex("*.*");
        let re = regex::Regex::new(&re_str).unwrap();
        assert!(re.is_match("Anything.AtAll"));
        assert!(re.is_match("X.Y"));
    }

    #[test]
    fn test_glob_to_regex_dot_escaping() {
        let re_str = glob_to_regex("File.mac");
        // Literal dots must be escaped in regex
        let re = regex::Regex::new(&re_str).unwrap();
        assert!(re.is_match("File.mac"));
        assert!(!re.is_match("FilXmac")); // dot is not a wildcard
    }

    #[test]
    fn test_glob_to_regex_empty_string() {
        let re_str = glob_to_regex("");
        // Empty pattern should produce (?i)^$ regex
        assert_eq!(re_str, "(?i)^$");
    }

    // ── Additional slice_lines edge case tests ─────────────────────────────────

    #[test]
    fn test_slice_lines_single_line_request() {
        let lines = vec!["only".to_string()];
        let (sliced, start, end, clamped) = slice_lines(&lines, 1, 1);
        assert_eq!(sliced.len(), 1);
        assert_eq!(sliced[0], "only");
        assert_eq!(start, 1);
        assert_eq!(end, 1);
        assert!(!clamped);
    }

    #[test]
    fn test_slice_lines_empty_array() {
        let lines: Vec<String> = vec![];
        let (sliced, _, _, clamped) = slice_lines(&lines, 1, 10);
        assert!(sliced.is_empty());
        assert!(clamped);
    }

    #[test]
    fn test_slice_lines_negative_start_clamped() {
        let lines: Vec<String> = (1..=5).map(|i| format!("line{i}")).collect();
        let (sliced, start, _, _) = slice_lines(&lines, -10, 3);
        // Negative start should be clamped to 1
        assert_eq!(start, 1);
        assert_eq!(sliced.len(), 3);
    }

    #[test]
    fn test_slice_lines_zero_start_clamped() {
        let lines: Vec<String> = (1..=5).map(|i| format!("line{i}")).collect();
        let (sliced, start, _, _) = slice_lines(&lines, 0, 3);
        // Zero start should be clamped to 1
        assert_eq!(start, 1);
        assert_eq!(sliced.len(), 3);
    }

    #[test]
    fn test_slice_lines_exact_boundaries() {
        let lines: Vec<String> = (1..=10).map(|i| format!("line{i}")).collect();
        let (sliced, start, end, clamped) = slice_lines(&lines, 5, 10);
        assert_eq!(sliced.len(), 6); // lines 5-10 inclusive = 6 lines
        assert_eq!(start, 5);
        assert_eq!(end, 10);
        assert!(!clamped);
        assert_eq!(sliced[0], "line5");
        assert_eq!(sliced[5], "line10");
    }

    #[test]
    fn test_slice_lines_middle_range() {
        let lines: Vec<String> = (1..=20).map(|i| format!("line{i}")).collect();
        let (sliced, _, _, _) = slice_lines(&lines, 5, 10);
        assert_eq!(sliced.len(), 6);
        assert_eq!(sliced[0], "line5");
    }

    // ── Additional strip_storage_blocks edge cases ─────────────────────────────

    #[test]
    fn test_strip_storage_blocks_nested_braces() {
        // Storage block with nested braces
        let cls =
            "Class Foo {\nStorage Default {\n<Data>{\n  <Item>{value}</Item>\n}\n</Data>\n}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag);
        assert!(!stripped.contains("Storage Default"));
        assert!(stripped.contains("Class Foo"));
    }

    #[test]
    fn test_strip_storage_blocks_empty_class() {
        let cls = "Class Foo {\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(!flag);
        assert_eq!(stripped, cls);
    }

    #[test]
    fn test_strip_storage_blocks_only_storage() {
        // Class with only Storage block
        let cls = "Class Foo {\nStorage Default {\n<Type>T</Type>\n}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag);
        assert!(stripped.contains("Class Foo"));
        assert!(!stripped.contains("Storage"));
    }

    #[test]
    fn test_strip_storage_blocks_storage_with_different_name() {
        let cls = "Class Foo {\nStorage MyCustom {\n<Data/>\n}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag);
        assert!(!stripped.contains("Storage MyCustom"));
        assert!(stripped.contains("Class Foo"));
    }

    #[test]
    fn test_strip_storage_blocks_storage_at_end() {
        let cls = "Class Foo {\nProperty X As %String;\nStorage Default {\n<Type>T</Type>\n}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag);
        assert!(stripped.contains("Property X"));
        assert!(!stripped.contains("Storage Default"));
    }

    #[test]
    fn test_strip_storage_blocks_storage_multiple_lines_no_brace_on_first() {
        // Storage block where opening brace is on next line
        let cls = "Class Foo {\nStorage Default\n{\n<Type>T</Type>\n}\n}";
        let (stripped, flag) = strip_storage_blocks(cls);
        assert!(flag);
        assert!(!stripped.contains("Storage Default"));
        assert!(stripped.contains("Class Foo"));
    }

    // ── Additional doc_content_to_string edge cases ────────────────────────────

    #[test]
    fn test_doc_content_to_string_mixed_types() {
        let body = serde_json::json!({
            "result": {
                "content": ["line1", 123, "line2", null, "line3"]
            }
        });
        let s = doc_content_to_string(&body);
        assert!(s.contains("line1"));
        assert!(s.contains("line2"));
        assert!(s.contains("line3"));
        // Non-string values are filtered out
        let line_count = s.lines().count();
        assert_eq!(line_count, 3);
    }

    #[test]
    fn test_doc_content_to_string_all_non_strings() {
        let body = serde_json::json!({
            "result": {
                "content": [123, null, true]
            }
        });
        let s = doc_content_to_string(&body);
        assert_eq!(s, "");
    }

    #[test]
    fn test_doc_content_to_string_single_line() {
        let body = serde_json::json!({
            "result": {
                "content": ["single line"]
            }
        });
        let s = doc_content_to_string(&body);
        assert_eq!(s, "single line");
    }

    #[test]
    fn test_doc_content_to_string_with_empty_strings() {
        let body = serde_json::json!({
            "result": {
                "content": ["line1", "", "line2"]
            }
        });
        let s = doc_content_to_string(&body);
        assert!(s.contains("line1"));
        assert!(s.contains("line2"));
        let parts: Vec<&str> = s.split('\n').collect();
        assert_eq!(parts.len(), 3); // line1, empty string, line2
    }

    #[test]
    fn test_doc_content_to_string_special_chars() {
        let body = serde_json::json!({
            "result": {
                "content": ["Class Foo {", "  Property X;", "}"]
            }
        });
        let s = doc_content_to_string(&body);
        assert!(s.contains("Class Foo {"));
        assert!(s.contains("Property X"));
        assert!(s.contains("}"));
    }

    // ── ok_json and err_json helper tests ──────────────────────────────────────

    #[test]
    fn test_ok_json_creates_success_response() {
        let val = serde_json::json!({"data": "test"});
        let result = ok_json(val).unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["data"], "test");
    }

    #[test]
    fn test_err_json_creates_error_response() {
        let result = err_json("TEST_ERROR", "Test message").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["success"], false);
        assert_eq!(v["error_code"], "TEST_ERROR");
        assert_eq!(v["error"], "Test message");
    }

    #[test]
    fn test_err_json_empty_message() {
        let result = err_json("CODE", "").unwrap();
        let text = result.content[0].raw.as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error"], "");
    }
}
