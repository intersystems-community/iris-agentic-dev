//! `iris_check`: ascot's static analysis of one file, with iad's connection answering the lookups.
//!
//! ascot asks its `Atelier` trait for what the file refers to but does not define, as two REST
//! requests iad already makes: `/action/query` and `/doc/{name}`. The trait is synchronous and the
//! connection async, so the check runs on a blocking thread and drives each request through the
//! runtime handle. The context is fresh per call: ascot caches lookups for an hour, and an agent
//! that edits a class on the server and then checks a caller of it must see the edit.

use std::sync::Arc;

use ascot::bindings::{Diagnostic, DiagnosticSeverity, GuestWorkspace};
use ascot::{Atelier, AtelierContext, IrisContext, Workspace};
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::output_schemas::ToolError;
use super::{err_json, host_fs, nopws, ok_json, IrisTools};
use crate::iris::connection::IrisConnection;

/// Parameters of `iris_check`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// A `.cls`, `.mac`, `.int` or `.inc` file inside the project directory.
    pub path: String,
    /// Namespace that resolves the file's references. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisCheckResponse {
    Ok(CheckOk),
    Err(ToolError),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckOk {
    pub success: bool,
    pub path: String,
    /// Whether a connection resolved the file's references; without one they are hints.
    pub iris: bool,
    pub namespace: String,
    pub diagnostics: Vec<Diagnostic>,
    pub errors: usize,
    pub warnings: usize,
    pub hints: usize,
}

/// iad's connection as the two requests ascot asks for.
pub struct IadAtelier {
    conn: Arc<IrisConnection>,
    client: reqwest::Client,
    namespace: String,
    handle: tokio::runtime::Handle,
}

impl Atelier for IadAtelier {
    fn query(&self, sql: &str, params: &[&str]) -> Result<String, String> {
        let params = params.iter().map(|p| Value::from(*p)).collect();
        let body = self
            .handle
            .block_on(self.conn.query(sql, params, &self.namespace, &self.client))
            .map_err(|e| format!("query in {}: {e}", self.namespace))?;
        match body.pointer("/result/content") {
            Some(rows) if rows.is_array() => Ok(rows.to_string()),
            _ => Err(format!("query answered with no rows: {}", body["status"])),
        }
    }

    fn get_doc(&self, name: &str) -> Result<Option<String>, String> {
        let path = format!("/doc/{}", urlencoding::encode(name));
        let url = self.conn.versioned_ns_url(&self.namespace, &path);
        let err = |e: &dyn std::fmt::Display| format!("GET {url}: {e}");
        self.handle.block_on(async {
            let resp = self
                .client
                .get(&url)
                .basic_auth(&self.conn.username, Some(&self.conn.password))
                .send()
                .await
                .map_err(|e| err(&e))?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let body: Value =
                crate::iris::http::capped_json(resp.error_for_status().map_err(|e| err(&e))?)
                    .await
                    .map_err(|e| err(&e))?;
            let lines: Vec<&str> = body["result"]["content"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            Ok((!lines.is_empty()).then(|| lines.join("\n")))
        })
    }
}

/// The diagnostics of one document, `atelier` resolving what it does not define.
pub fn check(uri: &str, text: &str, atelier: Option<IadAtelier>) -> Vec<Diagnostic> {
    let iris = atelier.map(|a| Box::new(AtelierContext::from(a)) as Box<dyn IrisContext>);
    let workspace = Workspace::native(iris, None);
    workspace.open(uri.to_string(), text.to_string());
    workspace.diagnostics(uri.to_string())
}

pub(crate) async fn handle_iris_check(
    tools: &IrisTools,
    path: &str,
    namespace: Option<&str>,
) -> Result<CallToolResult, McpError> {
    // Judged before it is read: inside the project directory, with one of the compile extensions.
    let file = {
        let (root, cwd) = tools.host_path_context();
        let exts = host_fs::COMPILE_EXTENSIONS;
        match host_fs::confine_file(root.as_deref(), &cwd, path, exts) {
            Ok(canonical) => canonical,
            Err(host_fs::PathRefusal::NotFound) => {
                return err_json("FILE_NOT_FOUND", &format!("Local file not found: {path}"))
            }
            Err(why) => return tools.refuse(host_fs::refusal_body("iris_check", path, &why, exts)),
        }
    };
    let namespace = tools.target_ns(namespace)?;
    // Lookups go through the target connection when it speaks Atelier (a docker-only one cannot).
    let atelier = tools
        .target_conn_opt()?
        .filter(|conn| !nopws::atelier_unavailable(conn))
        .map(|conn| IadAtelier {
            conn,
            client: tools.http_client().clone(),
            namespace: namespace.clone(),
            handle: tokio::runtime::Handle::current(),
        });
    let iris = atelier.is_some();
    // ascot is synchronous and its lookups block on the runtime, so it runs off the worker threads.
    let diagnostics = match tokio::task::spawn_blocking(move || {
        std::fs::read_to_string(&file).map(|text| check(&file.to_string_lossy(), &text, atelier))
    })
    .await
    .map_err(|e| McpError::internal_error(format!("check: {e}"), None))?
    {
        Ok(diagnostics) => diagnostics,
        Err(e) => return err_json("FILE_READ_FAILED", &format!("cannot read {path}: {e}")),
    };
    use DiagnosticSeverity as S;
    let n = |s: S| diagnostics.iter().filter(|d| d.severity == s).count();
    let ok = CheckOk {
        success: true,
        path: path.to_string(),
        iris,
        namespace,
        errors: n(S::Error),
        warnings: n(S::Warning),
        hints: n(S::Hint),
        diagnostics,
    };
    ok_json(serde_json::to_value(ok).unwrap_or(Value::Null))
}
