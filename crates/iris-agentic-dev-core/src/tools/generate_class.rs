//! The store-and-compile step of `iris_generate_class` (spec 141 T211, FR-012 to FR-017).
//!
//! The tool asks a model for class text. That text is stored and compiled through the path
//! `iris_doc` put with `compile` takes, so everything that path checks applies to it: the
//! compile-time gate on the text, the stored `.inc` check, the source-control probe, the storage
//! guardrail and the superclass check (FR-015a). Before this, the tool compiled whatever class
//! already had the name the model wrote and never stored the text, so a description that steered
//! the name ran an existing class's generators with none of those checks.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::iris::connection::IrisConnection;

/// What storing and compiling the generated text came to.
#[derive(Debug)]
pub enum Stored {
    /// Stored, and IRIS compiled it.
    Compiled,
    /// Stored, and IRIS reported these compile errors.
    CompileErrors(Vec<String>),
    /// Nothing compiled, and the reason is the tool's answer: a refusal by a gate, a checkout the
    /// caller must approve, a failed upload or an unreachable server. The result is returned as it
    /// is, an error result for a refusal.
    Stopped(CallToolResult),
}

/// Store `class_text` as `class_name` in `ns` and compile it.
pub(crate) async fn store_and_compile(
    iris: &IrisConnection,
    client: &reqwest::Client,
    class_name: &str,
    class_text: &str,
    ns: &str,
    elicitation_store: &crate::elicitation::ElicitationStore,
    checkout_cache: &crate::elicitation::CheckoutCache,
) -> Result<Stored, McpError> {
    let result = crate::tools::doc::put_and_compile(
        iris,
        client,
        &format!("{class_name}.cls"),
        class_text,
        ns,
        elicitation_store,
        checkout_cache,
    )
    .await?;
    Ok(classify(result))
}

/// Read the answer of a put with `compile`: compiled, compile errors IRIS returned, or a stop.
///
/// A compile that ran and found errors is the only answer worth another attempt by the model.
/// An error result (a refusal, a failed upload or compile request) and a prompt for a checkout are
/// not compile errors, and a retry would only hit them again.
pub fn classify(result: CallToolResult) -> Stored {
    if result.is_error == Some(true) {
        return Stored::Stopped(result);
    }
    let body = result.structured_content.clone().or_else(|| {
        result
            .content
            .iter()
            .find_map(|c| c.as_text())
            .and_then(|t| serde_json::from_str(&t.text).ok())
    });
    let Some(body) = body else {
        return Stored::Stopped(result);
    };
    if body["success"] == serde_json::Value::Bool(true) {
        return Stored::Compiled;
    }
    let errors: Vec<String> = body["compile_errors"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if body["compiled"] == serde_json::Value::Bool(false) && !errors.is_empty() {
        Stored::CompileErrors(errors)
    } else {
        Stored::Stopped(result)
    }
}
