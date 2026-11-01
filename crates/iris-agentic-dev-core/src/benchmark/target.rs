//! Where a benchmark run's IRIS steps go.
//!
//! A run does four things on the server: it runs ObjectScript (the lock, the version query, the
//! test runner), and it stores and compiles classes. [`BenchTarget`] is that surface.
//!
//! - [`Direct`] talks to one connection with no gate in the way. The live tests of the primitives
//!   use it.
//! - [`GatedTarget`] sends every step through the tool pipeline (`IrisTools::call_gated`), the
//!   path the MCP server and the other commands take, so the environment template, `allow`, the
//!   write gate, strict read-only, the role gate and the compile-time gate answer for each step.
//!   The `benchmark` command uses it.
//!
//! A gate that refuses a step is not a failed task. [`GatedTarget`] records the refusal body, the
//! suite loops stop at it ([`BenchTarget::refused`]), and the command prints the body and exits 1
//! instead of reporting a table of errored tasks as a benchmark result.

use crate::iris::connection::IrisConnection;
use crate::tools::IrisTools;
use serde_json::{json, Value};
use std::sync::Mutex;

/// The IRIS steps of a benchmark run.
#[allow(async_fn_in_trait)]
pub trait BenchTarget {
    /// Run ObjectScript in `namespace` and return what it wrote.
    async fn run_objectscript(&self, code: &str, namespace: &str) -> anyhow::Result<String>;

    /// Store `content` as the document `doc_name` and compile it with `cuk`. The errors are the
    /// compiler's; a step that could not run at all is an `Err`.
    async fn write_and_compile(
        &self,
        namespace: &str,
        doc_name: &str,
        content: &str,
    ) -> anyhow::Result<Vec<String>>;

    /// A gate refused a step. The suite loops stop at the first task that sees this.
    fn refused(&self) -> bool {
        false
    }
}

/// One connection, no gate. See the module comment.
pub struct Direct<'a> {
    pub iris: &'a IrisConnection,
    pub client: &'a reqwest::Client,
}

impl BenchTarget for Direct<'_> {
    async fn run_objectscript(&self, code: &str, namespace: &str) -> anyhow::Result<String> {
        self.iris
            .execute_via_generator(code, namespace, self.client)
            .await
    }

    async fn write_and_compile(
        &self,
        namespace: &str,
        doc_name: &str,
        content: &str,
    ) -> anyhow::Result<Vec<String>> {
        super::container::write_and_compile(self.iris, self.client, namespace, doc_name, content)
            .await
    }
}

/// Every step through `IrisTools::call_gated`.
pub struct GatedTarget<'a> {
    tools: &'a IrisTools,
    refusal: Mutex<Option<Value>>,
}

/// The error a refused step returns. A task that meets it ends as an error and the suite stops.
#[derive(Debug)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Refused {}

impl<'a> GatedTarget<'a> {
    pub fn new(tools: &'a IrisTools) -> Self {
        Self {
            tools,
            refusal: Mutex::new(None),
        }
    }

    /// The first refusal a gate gave, with its `refusal` object, if any. Taking it clears it.
    pub fn take_refusal(&self) -> Option<Value> {
        self.refusal.lock().ok().and_then(|mut r| r.take())
    }

    /// Whether a gate has refused a step, without clearing it.
    pub fn was_refused(&self) -> bool {
        self.refusal.lock().map(|r| r.is_some()).unwrap_or(true)
    }

    /// `tool` through the pipeline. `Err(Refused)` for a gate's refusal, which is recorded; any
    /// other `Err` is a step that failed; `Ok` is the tool's body, which may still say
    /// `success: false`.
    async fn call(&self, tool: &str, args: Value) -> anyhow::Result<Value> {
        let result = self
            .tools
            .call_gated(tool, args)
            .await
            .map_err(|e| anyhow::anyhow!("{tool}: {e}"))?;
        let body = result
            .structured_content
            .clone()
            .or_else(|| {
                result
                    .content
                    .iter()
                    .find_map(|c| c.as_text())
                    .and_then(|t| serde_json::from_str::<Value>(&t.text).ok())
            })
            .unwrap_or_else(|| json!({}));
        if body.get("refusal").is_some_and(Value::is_object) {
            if let Ok(mut slot) = self.refusal.lock() {
                slot.get_or_insert_with(|| body.clone());
            }
            let why = body["error"].as_str().unwrap_or("refused").to_string();
            let code = body["error_code"].as_str().unwrap_or("REFUSED");
            return Err(Refused(format!("{code}: {why}")).into());
        }
        Ok(body)
    }
}

/// The text of a tool body's failure: its code and message.
fn failure_text(tool: &str, body: &Value) -> String {
    let code = body["error_code"].as_str().unwrap_or("FAILED");
    let msg = body["error"].as_str().unwrap_or("no message");
    format!("{tool} {code}: {msg}")
}

impl BenchTarget for GatedTarget<'_> {
    async fn run_objectscript(&self, code: &str, namespace: &str) -> anyhow::Result<String> {
        let body = self
            .call(
                "iris_execute",
                json!({"code": code, "namespace": namespace}),
            )
            .await?;
        if body["success"] == false {
            anyhow::bail!("{}", failure_text("iris_execute", &body));
        }
        Ok(body["output"].as_str().unwrap_or_default().to_string())
    }

    async fn write_and_compile(
        &self,
        namespace: &str,
        doc_name: &str,
        content: &str,
    ) -> anyhow::Result<Vec<String>> {
        let body = self
            .call(
                "iris_doc",
                json!({
                    "mode": "put",
                    "name": doc_name,
                    "content": content,
                    "namespace": namespace,
                    "compile": true,
                }),
            )
            .await?;
        // `compiled` is present only when the put succeeded and the compile ran. A put that
        // failed, or a compile the server could not start, has no compile verdict to report.
        match body["compiled"].as_bool() {
            Some(true) => Ok(Vec::new()),
            Some(false) => {
                let errors: Vec<String> = body["compile_errors"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| e.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(if errors.is_empty() {
                    vec![format!("{doc_name} did not compile")]
                } else {
                    errors
                })
            }
            None => anyhow::bail!("{}", failure_text("iris_doc", &body)),
        }
    }

    fn refused(&self) -> bool {
        self.was_refused()
    }
}
