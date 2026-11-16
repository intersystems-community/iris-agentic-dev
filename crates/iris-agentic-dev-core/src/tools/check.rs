//! `iris_check` and `iris_check_compile_execute`: ascot's static analysis of one document, the
//! second going on to save, compile and run; both exist only when the operator sets `ascot = true`
//! in the configuration file or `IRIS_ASCOT=1`. What the document refers to but does not define is
//! looked up in two places in turn: IRIS's own classes in the declarations ascot bundles in the
//! binary, everything else through iad's connection, in a context new per call since ascot caches
//! lookups for an hour. The `Atelier` trait is synchronous, so the check runs on a blocking thread.

use std::sync::{Arc, OnceLock};

use ascot::bindings::{Diagnostic, DiagnosticSeverity, GuestWorkspace};
use ascot::{Atelier, AtelierContext, Chain, Declarations, IrisContext, Workspace};
use rmcp::{handler::server::wrapper::Parameters, model::CallToolResult, ErrorData as McpError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::output_schemas::{CompileErrorEntry, ToolError};
use super::{err_json, exec, nopws, ok_json, IrisTools};
use crate::iris::connection::IrisConnection;

thread_local! {
    static OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// The operator's switch: `IRIS_ASCOT`, which the config loader sets from `ascot = true`.
pub(crate) fn enabled() -> bool {
    OVERRIDE
        .with(std::cell::Cell::get)
        .unwrap_or_else(|| std::env::var("IRIS_ASCOT").is_ok_and(|v| exec::truthy(&v)))
}

/// A test's say over `IRIS_ASCOT` on this thread, without touching the process environment.
#[cfg(feature = "testing")]
pub fn override_on_this_thread(on: Option<bool>) {
    OVERRIDE.with(|o| o.set(on));
}

/// The least severe diagnostic a check reports; each level includes the ones above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
// Inlined so the property carries its type and values; behind a `$ref` a client that does not
// resolve `$defs` sees a parameter with no type.
#[schemars(inline)]
pub enum Level {
    #[default]
    Error,
    Warning,
    Information,
}

impl Level {
    pub fn admits(self, severity: DiagnosticSeverity) -> bool {
        use DiagnosticSeverity as S;
        // The variants are declared from most to least severe; a level admits those up to itself.
        [S::Error, S::Warning, S::Information][..=self as usize].contains(&severity)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// The document to check, with its extension: `Pkg.Class.cls`, `Routine.mac`, `.int` or `.inc`.
    pub name: String,
    /// The document's text, to check code before saving it. Omit to check the document as stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The least severe diagnostic reported: `error` (default), `warning` or `information`.
    #[serde(default)]
    pub level: Level,
    /// Namespace that resolves the document's references. Omit to use the connection's.
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
    pub name: String,
    pub namespace: String,
    #[serde(flatten)]
    pub analysis: Analysis,
}

/// ascot's diagnostics on one document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Analysis {
    /// The least severe diagnostic reported.
    pub level: Level,
    /// The pre-built declarations IRIS's own classes were resolved against.
    pub declarations: String,
    /// False, with `note`, when a class outside IRIS's own went unchecked: no connection, or one
    /// that did not answer a lookup.
    pub iris: bool,
    /// Present when classes other than IRIS's own were not checked: why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub errors: usize,
    pub warnings: usize,
    pub information: usize,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckCompileExecuteParams {
    /// The document to check, save and compile, with its extension: `Pkg.Class.cls`, `Routine.mac`,
    /// `.int` or `.inc`. Goes with `content`; omit both to run `code` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The document's whole text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// ObjectScript run after a clean compile, as the body of a class method: the way `iris_execute`
    /// runs its `code`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The least severe diagnostic the check reports: `error` (default), `warning` or `information`.
    #[serde(default)]
    pub level: Level,
    /// Namespace to save, compile and run in. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IrisCheckCompileExecuteResponse {
    Ok(CheckCompileExecuteOk),
    Err(ToolError),
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CheckCompileExecuteOk {
    pub success: bool,
    pub namespace: String,
    /// ascot's diagnostics on `doc`; absent without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis: Option<Analysis>,
    /// The compiler's verdict on `doc`; absent without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compile: Option<Compiled>,
    /// What `code` wrote, when it ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Why `code` wrote nothing: its run-time error, a refusal, or a document that did not compile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Compiled {
    pub ok: bool,
    /// The compiler's errors as `iris_compile` reports them.
    pub errors: Vec<CompileErrorEntry>,
}

/// What this tool reads of an inner tool's answer. iad's tools report `success` in the body and a
/// refusal as `error_code` and `error`; `iris_compile` adds `errors`, `iris_execute` `output`.
#[derive(Debug, Default, Deserialize)]
struct Answer {
    #[serde(default)]
    success: bool,
    error_code: Option<String>,
    error: Option<String>,
    #[serde(default)]
    errors: Vec<CompileErrorEntry>,
    output: Option<String>,
}

impl Answer {
    /// A protocol-level error, or a body this tool cannot read, is a failed answer with no detail.
    fn of(r: &CallToolResult) -> Answer {
        if r.is_error == Some(true) {
            return Answer::default();
        }
        serde_json::from_value(r.structured_content.clone().unwrap_or(Value::Null))
            .unwrap_or_default()
    }

    fn refused(&self) -> bool {
        !self.success && self.error_code.is_some()
    }

    /// `error_code: error`, whichever of the two the answer carries.
    fn failure(&self) -> String {
        match (&self.error_code, &self.error) {
            (Some(code), Some(error)) => format!("{code}: {error}"),
            (Some(code), None) => code.clone(),
            (None, Some(error)) => error.clone(),
            (None, None) => "ERROR".to_string(),
        }
    }
}

/// The rows of an Atelier `/action/query` answer.
#[derive(Deserialize)]
struct Rows {
    result: RowsResult,
}

#[derive(Deserialize)]
struct RowsResult {
    content: Vec<Value>,
}

/// iad's connection as the two requests ascot asks for.
pub struct IadAtelier {
    conn: Arc<IrisConnection>,
    http: Arc<reqwest::Client>,
    namespace: String,
    handle: tokio::runtime::Handle,
    /// The first failed lookup: else a connection that does not answer passes as a clean check.
    failed: Arc<OnceLock<String>>,
}

impl IadAtelier {
    fn noting<T>(&self, result: Result<T, String>) -> Result<T, String> {
        if let Err(e) = &result {
            let _ = self.failed.set(e.clone());
        }
        result
    }
}

impl Atelier for IadAtelier {
    fn query(&self, sql: &str, params: &[&str]) -> Result<String, String> {
        let params = params.iter().map(|p| Value::from(*p)).collect();
        let answer = self
            .handle
            .block_on(self.conn.query(sql, params, &self.namespace, &self.http))
            .map_err(|e| format!("query in {}: {e}", self.namespace))
            .and_then(|body| {
                serde_json::from_value::<Rows>(body)
                    .map(|rows| Value::Array(rows.result.content).to_string())
                    .map_err(|e| format!("query answered with no rows: {e}"))
            });
        self.noting(answer)
    }

    fn get_doc(&self, name: &str) -> Result<Option<String>, String> {
        let doc = super::doc::fetch_doc(&self.conn, &self.http, name, &self.namespace);
        self.noting(self.handle.block_on(doc))
            .map(|text| text.filter(|t| !t.is_empty()))
    }
}

/// The diagnostics of one document, `iris` resolving what it does not define.
pub fn check(uri: &str, text: &str, iris: Option<Box<dyn IrisContext>>) -> Vec<Diagnostic> {
    let workspace = Workspace::native(iris, None);
    workspace.open(uri.to_string(), text.to_string());
    workspace.diagnostics(uri.to_string())
}

/// The declarations ascot bundles, [`ascot::BUNDLED_DECLARATIONS`], parsed once for the process:
/// every install checks IRIS's own classes against the release the pinned ascot was built for.
pub fn declarations() -> Result<Arc<Declarations>, String> {
    static DECLARATIONS: OnceLock<Result<Arc<Declarations>, String>> = OnceLock::new();
    DECLARATIONS
        .get_or_init(|| {
            let bundled =
                ascot::BUNDLED_DECLARATIONS.ok_or("this build of ascot carries no declarations")?;
            Declarations::from_bytes(bundled.bytes).map(Arc::new)
        })
        .clone()
}

const CONNECT: &str = "no IRIS connection; set the IRIS_HOST and IRIS_WEB_PORT environment \
                       variables, or register a server with iris_add_server";

/// A check that did not run: a protocol error, an internal one, or a refusal (code, message).
enum Failure {
    Mcp(McpError),
    Internal(String),
    Refused(&'static str, String),
}

impl Failure {
    fn result(self) -> Result<CallToolResult, McpError> {
        match self {
            Failure::Mcp(e) => Err(e),
            Failure::Internal(message) => Err(McpError::internal_error(message, None)),
            Failure::Refused(code, message) => err_json(code, &message),
        }
    }
}

async fn analyze(
    tools: &IrisTools,
    name: &str,
    source: Option<&str>,
    namespace: &str,
    level: Level,
) -> Result<CheckOk, Failure> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    if !matches!(ext.as_deref(), Some("cls" | "mac" | "int" | "inc")) {
        return Err(Failure::Refused(
            "INVALID_PARAMS",
            format!("name must end in .cls, .mac, .int or .inc, got {name}"),
        ));
    }
    let conn = tools.target_conn_opt().map_err(Failure::Mcp)?;
    let atelier = match conn.filter(|conn| !nopws::atelier_unavailable(conn)) {
        Some(conn) => Some(IadAtelier {
            http: tools.conn_client(&conn).map_err(Failure::Mcp)?,
            conn,
            namespace: namespace.to_string(),
            handle: tokio::runtime::Handle::current(),
            failed: Arc::default(),
        }),
        None => None,
    };
    let failed = atelier.as_ref().map(|a| Arc::clone(&a.failed));
    let doc = name.to_string();
    let source = source.map(str::to_string);
    let ns = namespace.to_string();
    let mut diagnostics: Vec<Diagnostic> = tokio::task::spawn_blocking(move || {
        let text = match (source, &atelier) {
            (Some(text), _) => text,
            (None, None) => return Err(Failure::Mcp(super::iris_unreachable())),
            (None, Some(a)) => a
                .get_doc(&doc)
                .map_err(|e| Failure::Refused("IRIS_UNREACHABLE", e))?
                .ok_or_else(|| {
                    Failure::Refused("DOCUMENT_NOT_FOUND", format!("no document {doc} in {ns}"))
                })?,
        };
        let declarations =
            declarations().map_err(|e| Failure::Internal(format!("bundled declarations: {e}")))?;
        let mut sources: Vec<Box<dyn IrisContext>> = vec![Box::new(declarations)];
        if let Some(a) = atelier {
            sources.push(Box::new(AtelierContext::from(a)));
        }
        let iris: Box<dyn IrisContext> = Box::new(Chain(sources));
        Ok(check(&doc, &text, Some(iris)))
    })
    .await
    .map_err(|e| Failure::Internal(format!("check: {e}")))??;
    diagnostics.retain(|d| level.admits(d.severity));
    let unchecked = match failed.map(|f| f.get().cloned()) {
        Some(None) => None,
        Some(Some(err)) => Some(format!("the connection did not answer the lookups ({err})")),
        None => Some(CONNECT.to_string()),
    };
    use DiagnosticSeverity as S;
    let n = |s: S| diagnostics.iter().filter(|d| d.severity == s).count();
    Ok(CheckOk {
        success: true,
        name: name.to_string(),
        namespace: namespace.to_string(),
        analysis: Analysis {
            level,
            declarations: ascot::BUNDLED_DECLARATIONS
                .map_or("", |b| b.release)
                .to_string(),
            iris: unchecked.is_none(),
            note: unchecked
                .map(|why| format!("Classes other than IRIS's own were not checked: {why}.")),
            errors: n(S::Error),
            warnings: n(S::Warning),
            information: n(S::Information),
            diagnostics,
        },
    })
}

pub(crate) async fn handle_iris_check(
    tools: &IrisTools,
    name: &str,
    source: Option<&str>,
    level: Level,
    namespace: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let namespace = tools.target_ns(namespace)?;
    match analyze(tools, name, source, &namespace, level).await {
        Ok(ok) => ok_json(serde_json::to_value(ok).unwrap_or(Value::Null)),
        Err(failure) => failure.result(),
    }
}

/// An inner tool's parameters, built as a client would send them so every default applies.
fn params<T: serde::de::DeserializeOwned>(v: Value) -> Result<Parameters<T>, McpError> {
    serde_json::from_value(v)
        .map(Parameters)
        .map_err(|e| McpError::internal_error(format!("inner tool parameters: {e}"), None))
}
pub(crate) async fn handle_check_compile_execute(
    tools: &IrisTools,
    name: Option<String>,
    content: Option<String>,
    code: Option<String>,
    level: Level,
    namespace: Option<&str>,
) -> Result<CallToolResult, McpError> {
    if content.is_none() && code.is_none() {
        return err_json(
            "INVALID_PARAMS",
            "nothing to do: give content to check and compile, code to run, or both",
        );
    }
    if content.is_some() && name.is_none() {
        return err_json(
            "INVALID_PARAMS",
            "content needs name, the document with its extension",
        );
    }
    let namespace = tools.target_ns(namespace)?;
    // With code, the call resolved iris_execute's identity: refused before anything is saved.
    if code.is_some() {
        if let Err(e) = tools.exec_context() {
            return super::exec_refused(e);
        }
    }
    let mut out = CheckCompileExecuteOk {
        success: true,
        namespace: namespace.clone(),
        ..Default::default()
    };
    if let (Some(name), Some(content)) = (&name, &content) {
        let checked = analyze(tools, name, Some(content), &namespace, level).await;
        out.analysis = Some(match checked {
            Ok(checked) => checked.analysis,
            Err(failure) => return failure.result(),
        });
        let put = tools
            .iris_doc(params(json!({
                "mode": "put", "name": name, "content": content, "namespace": namespace,
            }))?)
            .await?;
        if !Answer::of(&put).success {
            return Ok(put);
        }
        let answer = tools
            .iris_compile(params(json!({"target": name, "namespace": namespace}))?)
            .await?;
        let compiled = Answer::of(&answer);
        if compiled.refused() {
            return Ok(answer);
        }
        let ok = compiled.success && compiled.errors.is_empty();
        out.compile = Some(Compiled {
            ok,
            errors: compiled.errors,
        });
        if !ok {
            out.error = code.map(|_| "not run: the document did not compile".to_string());
            return ok_json(serde_json::to_value(out).unwrap_or(Value::Null));
        }
    }
    if let Some(code) = code {
        let ran = Answer::of(
            &tools
                .iris_execute(params(json!({"code": code, "namespace": namespace}))?)
                .await?,
        );
        if ran.success {
            out.output = Some(ran.output.unwrap_or_default());
        } else {
            out.error = Some(ran.failure());
        }
    }
    ok_json(serde_json::to_value(out).unwrap_or(Value::Null))
}
