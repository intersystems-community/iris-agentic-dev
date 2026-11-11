pub mod code;
pub mod diagram;
pub mod explain;
pub mod globals;
pub mod model;
pub mod path;
pub mod sample;
pub mod shape;
pub mod tree;
pub mod uses;
pub mod validate;
pub mod write;

use crate::iris::connection::IrisConnection;
use crate::tools::{err_result, CallToolResult, McpError};
use serde_json::{json, Value};

/// Action enum for iris_storage_map tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Diagram,
    Uses,
    Compare,
    Preview,
    Propose,
    Check,
    Edit,
    Replace,
}

/// Call shape: action plus opt-in flags.
#[derive(Debug, Clone, PartialEq)]
pub struct CallShape {
    pub action: Action,
    pub seed_counter: bool,
    pub build_indexes: bool,
    pub include_values: bool,
    pub code_changed: Option<bool>,
}

/// Parses the action and opt-ins from params into a CallShape.
///
/// Per FR-030a: unreadable action (missing, not a string, unknown, any whitespace/case variant)
/// becomes Edit with both opt-ins true.
pub fn call_shape(params: &Value) -> CallShape {
    let action_str = params
        .get("action")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .unwrap_or("");

    let (action, is_unreadable) = match action_str.to_lowercase().as_str() {
        "read" => (Action::Read, false),
        "diagram" => (Action::Diagram, false),
        "uses" => (Action::Uses, false),
        "compare" => (Action::Compare, false),
        "preview" => (Action::Preview, false),
        "propose" => (Action::Propose, false),
        "check" => (Action::Check, false),
        "edit" => (Action::Edit, false),
        "replace" => (Action::Replace, false),
        _ => (Action::Edit, true), // Unreadable becomes Edit with opt-ins
    };

    // Opt-ins: only true if the value is boolean true
    let mut seed_counter = params
        .get("seed_counter")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut build_indexes = params
        .get("build_indexes")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let include_values = params
        .get("include_values")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Force both opt-ins to true for unreadable action
    if is_unreadable {
        seed_counter = true;
        build_indexes = true;
    }

    CallShape {
        action,
        seed_counter,
        build_indexes,
        include_values,
        code_changed: None, // Will be set later when we have before/after trees
    }
}

/// Handle iris_storage_map tool calls.
pub async fn handle(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
    _tools: &crate::tools::IrisTools,
) -> Result<CallToolResult, McpError> {
    let action = p.get("action").and_then(|v| v.as_str()).unwrap_or("");

    match action {
        "read" => read(iris, client, p).await,
        "diagram" | "uses" | "compare" | "preview" | "propose" | "check" | "edit" | "replace" => {
            err_result(json!({
                "success": false,
                "error_code": "NOT_IMPLEMENTED",
                "error": format!("Action '{}' not yet implemented", action)
            }))
        }
        _ => err_result(json!({
            "success": false,
            "error_code": "INVALID_ACTION",
            "error": format!("Unknown action: '{}'", action)
        })),
    }
}

fn fail(code: &str, msg: String, extra: Value) -> Result<CallToolResult, McpError> {
    let mut body = json!({"success": false, "error_code": code, "error": msg});
    if let Some(extra_obj) = extra.as_object() {
        if let Some(body_obj) = body.as_object_mut() {
            body_obj.extend(extra_obj.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }
    err_result(body)
}

/// The class's text as IRIS stores it, or `None` when IRIS has no such class.
async fn class_text(
    iris: &IrisConnection,
    client: &reqwest::Client,
    class: &str,
    ns: &str,
) -> Result<Option<String>, CallToolResult> {
    let lines =
        crate::tools::doc::fetch_doc_lines(iris, client, &format!("{class}.cls"), ns).await?;
    Ok(lines.map(|l| l.join("\n")))
}

/// The storage model of `class`'s first Storage block: `Ok(None)` when the class has no Storage
/// block, `Err(ParseError)` when one cannot be parsed.
fn model_of(class: &str, text: &str) -> Result<Option<model::StorageModel>, tree::ParseError> {
    let spans = tree::split_class(text)?;
    let Some(span) = spans.first() else {
        return Ok(None);
    };
    let nodes = tree::parse_body(&text[span.body_start..span.body_end])?;
    let mut m = model::from_tree(class, &span.name, &nodes);
    m.derived = model::derived_facts(&m);
    Ok(Some(m))
}

/// The other classes in `ns` whose data maps use one of `globals` (compiled dictionary).
async fn classes_on_globals(
    iris: &IrisConnection,
    client: &reqwest::Client,
    globals: &[String],
    ns: &str,
    except: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    for g in globals {
        let sql = "SELECT DISTINCT $PIECE(parent,'||',1) AS Cls FROM %Dictionary.CompiledStorageSQLMap WHERE _Global = ? AND Type = 'data'";
        if let Ok(resp) = iris.query(sql, vec![json!(g)], ns, client).await {
            for row in resp["result"]["content"].as_array().into_iter().flatten() {
                if let Some(c) = row["Cls"].as_str() {
                    if c != except && !out.iter().any(|o: &String| o == c) {
                        out.push(c.to_string());
                    }
                }
            }
        }
    }
    out
}

/// The SQL maps of `class`'s Storage block named `storage`, as the JSON model writes them, for
/// `resolve_storage` (FR-004). `None` when the class cannot be read or has no such block.
pub async fn sql_maps_json(
    iris: &IrisConnection,
    client: &reqwest::Client,
    class: &str,
    storage: &str,
    ns: &str,
) -> Option<Value> {
    let text = class_text(iris, client, class, ns).await.ok()??;
    let m = model_of(class, &text).ok()??;
    (m.storage.name == storage).then(|| json!(m.storage.maps))
}

/// `read` (FR-002, FR-003, FR-003a): the class's storage as the JSON model, with derived facts,
/// including nesting with other classes' maps on the same global.
async fn read(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
) -> Result<CallToolResult, McpError> {
    let Some(class) = p.get("class").and_then(Value::as_str) else {
        return fail("INVALID_PARAMS", "read needs `class`".into(), json!({}));
    };
    if let Err(e) = crate::objectscript::validate_class_name(class) {
        return err_result(e);
    }
    let ns = p
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or(&iris.namespace)
        .to_string();
    let text = match class_text(iris, client, class, &ns).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return fail(
                "STORAGE_MAP_NOT_FOUND",
                format!("no class {class} in namespace {ns}"),
                json!({}),
            )
        }
        Err(r) => return Ok(r),
    };
    let mut m = match model_of(class, &text) {
        Ok(Some(m)) => m,
        Ok(None) => {
            return fail(
                "STORAGE_MAP_NOT_FOUND",
                format!("class {class} has no Storage block"),
                json!({}),
            )
        }
        Err(e) => {
            return fail(
                "STORAGE_MAP_PARSE_FAILED",
                format!("cannot parse the Storage block of {class}: {}", e.reason),
                json!({"offset": e.offset, "context": e.context}),
            )
        }
    };
    // FR-003a: maps of other classes stored in the same global, nested either way.
    let globals: Vec<String> = m
        .storage
        .maps
        .iter()
        .filter(|map| map.r#type == "data")
        .map(|map| map.global.clone())
        .collect();
    let mut models = vec![m.clone()];
    for other in classes_on_globals(iris, client, &globals, &ns, class).await {
        if let Ok(Some(t)) = class_text(iris, client, &other, &ns).await {
            if let Ok(Some(om)) = model_of(&other, &t) {
                models.push(om);
            }
        }
    }
    let prefix = format!("{class}.");
    m.derived.extend(
        model::nesting_facts(&models)
            .into_iter()
            .filter(|(parent, _)| parent.starts_with(&prefix))
            .map(|(_, fact)| fact),
    );
    m.derived.extend(
        model::nested_maps(&models)
            .into_iter()
            .filter(|(child, _)| child.starts_with(&prefix))
            .map(|(child, parent)| model::Derived {
                path: format!("/maps/{}", child.rsplit('.').next().unwrap_or(&child)),
                fact: format!(
                    "rows stored inside the row nodes of map {parent}: a SQL DELETE there also deletes these rows"
                ),
            }),
    );
    Ok(CallToolResult::structured(json!({
        "success": true,
        "model_version": model::MODEL_VERSION,
        "action": "read",
        "class": class,
        "namespace": ns,
        "storage": m.storage,
        "derived": m.derived,
    })))
}
