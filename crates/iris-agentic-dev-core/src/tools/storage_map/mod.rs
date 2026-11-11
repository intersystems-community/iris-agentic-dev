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

    let action = match action_str.to_lowercase().as_str() {
        "read" => Some(Action::Read),
        "diagram" => Some(Action::Diagram),
        "uses" => Some(Action::Uses),
        "compare" => Some(Action::Compare),
        "preview" => Some(Action::Preview),
        "propose" => Some(Action::Propose),
        "check" => Some(Action::Check),
        "edit" => Some(Action::Edit),
        "replace" => Some(Action::Replace),
        _ => None,
    };
    let unreadable = action.is_none();

    // An opt-in is on only when its value is boolean `true`.
    let opt_in = |key: &str| params.get(key).and_then(Value::as_bool).unwrap_or(false);

    CallShape {
        action: action.unwrap_or(Action::Edit),
        seed_counter: unreadable || opt_in("seed_counter"),
        build_indexes: unreadable || opt_in("build_indexes"),
        include_values: opt_in("include_values"),
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
        "diagram" => diagram(iris, client, p).await,
        "uses" => uses_action(iris, client, p).await,
        "compare" | "preview" | "propose" | "check" | "edit" | "replace" => err_result(json!({
            "success": false,
            "error_code": "NOT_IMPLEMENTED",
            "error": format!("Action '{}' not yet implemented", action)
        })),
        _ => err_result(json!({
            "success": false,
            "error_code": "INVALID_ACTION",
            "error": format!("Unknown action: '{}'", action)
        })),
    }
}

fn fail(code: &str, msg: String, extra: Value) -> Result<CallToolResult, McpError> {
    let mut body = json!({"success": false, "error_code": code, "error": msg});
    if let (Some(body_obj), Value::Object(extra_obj)) = (body.as_object_mut(), extra) {
        body_obj.extend(extra_obj);
    }
    err_result(body)
}

/// The `class` param, the namespace and the parsed storage model that `read` and `diagram`
/// both start from, or the refusal to return when any of them is missing.
async fn load_model<'p>(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &'p Value,
    action: &str,
) -> Result<(&'p str, String, model::StorageModel), Result<CallToolResult, McpError>> {
    let Some(class) = p.get("class").and_then(Value::as_str) else {
        return Err(fail(
            "INVALID_PARAMS",
            format!("{action} needs `class`"),
            json!({}),
        ));
    };
    if let Err(e) = crate::objectscript::validate_class_name(class) {
        return Err(err_result(e));
    }
    let ns = p
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or(&iris.namespace)
        .to_string();
    let text = match class_text(iris, client, class, &ns).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err(fail(
                "STORAGE_MAP_NOT_FOUND",
                format!("no class {class} in namespace {ns}"),
                json!({}),
            ))
        }
        Err(r) => return Err(Ok(r)),
    };
    match model_of(class, &text) {
        Ok(Some(m)) => Ok((class, ns, m)),
        Ok(None) => Err(fail(
            "STORAGE_MAP_NOT_FOUND",
            format!("class {class} has no Storage block"),
            json!({}),
        )),
        Err(e) => Err(fail(
            "STORAGE_MAP_PARSE_FAILED",
            format!("cannot parse the Storage block of {class}: {}", e.reason),
            json!({"offset": e.offset, "context": e.context}),
        )),
    }
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
    let (class, ns, mut m) = match load_model(iris, client, p, "read").await {
        Ok(loaded) => loaded,
        Err(reply) => return reply,
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

/// `diagram` (FR-005, US2): Mermaid flowchart and JSON graph of a storage map.
async fn diagram(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
) -> Result<CallToolResult, McpError> {
    let (class, ns, m) = match load_model(iris, client, p, "diagram").await {
        Ok(loaded) => loaded,
        Err(reply) => return reply,
    };
    let map_filter = p.get("map").and_then(Value::as_str);

    Ok(CallToolResult::structured(json!({
        "success": true,
        "model_version": model::MODEL_VERSION,
        "action": "diagram",
        "class": class,
        "namespace": ns,
        "mermaid": diagram::mermaid(&m, map_filter),
        "graph": diagram::graph(&m, map_filter),
    })))
}

/// `uses` (R11, US2): Find all classes whose storage maps use a given global.
async fn uses_action(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
) -> Result<CallToolResult, McpError> {
    let Some(global_raw) = p.get("global").and_then(Value::as_str) else {
        return fail("INVALID_PARAMS", "uses needs `global`".into(), json!({}));
    };

    let global_ref = match globals::normalize_global(global_raw) {
        Ok(r) => r,
        Err(e) if e == "dynamic" => {
            return fail(
                "INVALID_PARAMS",
                format!("global reference contains dynamic elements: {global_raw}"),
                json!({}),
            )
        }
        Err(e) => return fail("INVALID_PARAMS", e, json!({})),
    };

    // The connection's namespace first, then each extra one once.
    let mut namespaces = vec![iris.namespace.clone()];
    let extra = p.get("namespaces").and_then(Value::as_array);
    for ns in extra.into_iter().flatten().filter_map(Value::as_str) {
        if !namespaces.iter().any(|n| n == ns) {
            namespaces.push(ns.to_string());
        }
    }

    let uses_list = match uses::uses(iris, client, &global_ref, &namespaces).await {
        Ok(list) => list,
        Err(e) => {
            return fail(
                "USES_QUERY_FAILED",
                format!("failed to query uses: {}", e),
                json!({}),
            )
        }
    };

    let uses_json: Vec<Value> = uses_list.iter().map(uses::use_to_json).collect();
    let count = uses_json.len();

    Ok(CallToolResult::structured(json!({
        "success": true,
        "model_version": model::MODEL_VERSION,
        "action": "uses",
        "global": global_ref.name,
        "namespaces_searched": namespaces,
        "uses": uses_json,
        "count": count,
    })))
}
