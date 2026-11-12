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
    /// The action was missing, not a string, or not exactly one of the action names. `action` is
    /// then `Edit` and both opt-ins are on, and the handler answers `INVALID_ACTION`.
    pub unreadable: bool,
    /// `acknowledgePhi` is boolean `true` (never for an unreadable call).
    pub acknowledge_phi: bool,
    pub seed_counter: bool,
    pub build_indexes: bool,
    pub include_values: bool,
    pub code_changed: Option<bool>,
}

/// Parses the action and opt-ins from params into a CallShape.
///
/// Per FR-030a: an unreadable action (missing, not a string, unknown, any whitespace or case
/// variant) becomes Edit with both opt-ins true.
pub fn call_shape(params: &Value) -> CallShape {
    // Exact names only: `" EDIT "` or `"Read"` is unreadable, so no spelling can be judged as a
    // lighter action than the one the handler would run (FR-030a, T034).
    let action_str = params.get("action").and_then(Value::as_str).unwrap_or("");

    let action = match action_str {
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
        unreadable,
        acknowledge_phi: !unreadable && opt_in("acknowledgePhi"),
        seed_counter: unreadable || opt_in("seed_counter"),
        build_indexes: unreadable || opt_in("build_indexes"),
        include_values: opt_in("include_values"),
        code_changed: match action {
            None => Some(true),
            Some(Action::Edit) => Some(ops_carry_code(params.get("ops"))),
            Some(Action::Replace) => Some(params.get("model").is_some_and(carries_code)),
            Some(_) => None,
        },
    }
}

/// Model keys whose values are literals, never ObjectScript: the JSON names of
/// `code::LITERAL_TAGS` and the model's own bookkeeping. Every other key is judged as code
/// (fail closed, like `code::code_elements`).
const LITERAL_KEYS: &[&str] = &[
    "type",
    "delimiter",
    "piece",
    "node",
    "population_type",
    "population_pct",
    "block_count",
    "structure",
    "deferred",
    "conditional_with_host_vars",
    "field",
    "access_type",
    "name",
    "version",
    "master",
    "used",
    "level",
    "position",
    "variable",
    "editable",
    "not_editable_reason",
    "description",
    "derived",
    "format",
    "computed",
    "filed",
    "model_version",
    "class",
];

/// Whether an `edit`'s ops can change map code (plan "Classification"). Judged from the call
/// alone, before the class is read: an op whose path ends in a code key, a value that carries
/// code, or ops iad cannot read.
fn ops_carry_code(ops: Option<&Value>) -> bool {
    let Some(Value::Array(ops)) = ops else {
        return true;
    };
    ops.iter().any(|op| {
        let last = op
            .get("path")
            .and_then(Value::as_str)
            .and_then(|p| p.rsplit('/').next());
        let Some(last) = last else { return true };
        match op.get("value") {
            Some(v @ (Value::Object(_) | Value::Array(_))) => carries_code(v),
            _ => !LITERAL_KEYS.contains(&last),
        }
    })
}

/// Whether a model fragment sets any non-empty value under a key that is not literal. An
/// `extra` element `{tag, value}` is code unless its tag is literal.
fn carries_code(v: &Value) -> bool {
    match v {
        Value::Object(o) => {
            // An `extra` element `{tag, name, value}`: its value is judged by its tag here, and
            // skipped by the key loop below.
            let tagged = o.contains_key("tag");
            if let (Some(tag), Some(value)) = (o.get("tag"), o.get("value")) {
                let literal = tag
                    .as_str()
                    .is_some_and(|t| code::LITERAL_TAGS.contains(&t));
                if !literal && value.as_str().is_some_and(|s| !s.trim().is_empty()) {
                    return true;
                }
            }
            o.iter().any(|(k, val)| match val {
                Value::String(s) => {
                    !s.trim().is_empty()
                        && !LITERAL_KEYS.contains(&k.as_str())
                        && k != "tag"
                        && !(tagged && k == "value")
                }
                // `data` and `keywords` are keyed by field or keyword name; their values are
                // judged on their own keys, or as code when they are bare strings.
                Value::Object(m) if matches!(k.as_str(), "data" | "keywords") => {
                    m.iter().any(|(kk, vv)| match vv {
                        Value::String(s) => {
                            !s.trim().is_empty() && !code::LITERAL_TAGS.contains(&kk.as_str())
                        }
                        other => carries_code(other),
                    })
                }
                Value::Object(_) | Value::Array(_) => carries_code(val),
                _ => false,
            })
        }
        Value::Array(a) => a.iter().any(carries_code),
        Value::String(s) => !s.trim().is_empty(),
        _ => false,
    }
}

/// Handle iris_storage_map tool calls.
pub async fn handle(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
    tools: &crate::tools::IrisTools,
) -> Result<CallToolResult, McpError> {
    // The same reader the classifier, strict read-only and the service-account check use.
    let shape = call_shape(p);
    if shape.unreadable {
        let shown = p.get("action").map(Value::to_string).unwrap_or_default();
        return err_result(json!({
            "success": false,
            "error_code": "INVALID_ACTION",
            "error": format!(
                "Unknown action: {shown}. Use one of read, diagram, uses, compare, preview, \
                 propose, check, edit, replace (exact, lowercase)."
            )
        }));
    }

    match shape.action {
        Action::Read => read(iris, client, p).await,
        Action::Diagram => diagram(iris, client, p).await,
        Action::Uses => uses_action(iris, client, p).await,
        Action::Check => check(iris, client, p, tools, &shape).await,
        other => err_result(json!({
            "success": false,
            "error_code": "NOT_IMPLEMENTED",
            "error": format!("Action '{other:?}' not yet implemented")
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

/// What `read`, `diagram` and `check` start from: the `class` param, the namespace, the class's
/// text and its first Storage block parsed.
struct StoredClass<'p> {
    class: &'p str,
    ns: String,
    text: String,
    storage_name: String,
    nodes: Vec<tree::Node>,
}

/// The `class` param, the namespace and the parsed storage model that `read` and `diagram`
/// both start from, or the refusal to return when any of them is missing.
async fn load_model<'p>(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &'p Value,
    action: &str,
) -> Result<(&'p str, String, model::StorageModel), Result<CallToolResult, McpError>> {
    let s = load_storage(iris, client, p, action).await?;
    let m = model_with_facts(s.class, &s.storage_name, &s.nodes);
    Ok((s.class, s.ns, m))
}

/// The [`StoredClass`] the params name, or the refusal to return when the class param, the
/// class or its Storage block is missing, or the block cannot be parsed.
async fn load_storage<'p>(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &'p Value,
    action: &str,
) -> Result<StoredClass<'p>, Result<CallToolResult, McpError>> {
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
    match storage_tree(&text) {
        Ok(Some((storage_name, nodes))) => Ok(StoredClass {
            class,
            ns,
            text,
            storage_name,
            nodes,
        }),
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

/// `check` (US4, FR-015, FR-015a, FR-017): the class's map, or the caller's `model` in place of
/// the map it names, through the gates, then compiled in a scratch copy that is deleted
/// afterwards. The real class is never written. A copy that does not compile answers
/// `STORAGE_MAP_COMPILE_FAILED` with IRIS's errors and their explanations.
async fn check(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
    tools: &crate::tools::IrisTools,
    shape: &CallShape,
) -> Result<CallToolResult, McpError> {
    let StoredClass {
        class,
        ns,
        text,
        storage_name,
        mut nodes,
    } = match load_storage(iris, client, p, "check").await {
        Ok(stored) => stored,
        Err(reply) => return reply,
    };
    let stored = model::from_tree(class, &storage_name, &nodes);
    if !stored.storage.editable {
        let why = stored.storage.not_editable_reason.unwrap_or_default();
        return fail(
            "STORAGE_MAP_READ_ONLY",
            format!("the storage of {class} is read-only in iris_storage_map: {why}"),
            json!({}),
        );
    }

    // The typed params pass an absent `model` through as `null`.
    if let Some(proposed) = p.get("model").filter(|v| !v.is_null()) {
        let map_param = p.get("map").and_then(Value::as_str);
        let model_name = proposed.get("name").and_then(Value::as_str);
        let name = match (map_param, model_name) {
            (Some(a), Some(b)) if a != b => {
                return fail(
                    "INVALID_PARAMS",
                    format!("`map` is {a} but the model is map {b}"),
                    json!({}),
                )
            }
            (Some(a), _) | (None, Some(a)) => a.to_string(),
            (None, None) => {
                return fail(
                    "INVALID_PARAMS",
                    "check with a `model` needs `map` or a `name` in the model".into(),
                    json!({}),
                )
            }
        };
        let mut proposed = proposed.clone();
        if let Some(o) = proposed.as_object_mut() {
            o.entry("name").or_insert_with(|| json!(name));
        }
        let node = match model::map_node(&proposed) {
            Ok(n) => n,
            Err(e) => return fail("INVALID_PARAMS", e, json!({})),
        };
        // The caller's map takes the place of the stored one; a new name adds a map.
        match nodes
            .iter_mut()
            .find(|n| n.tag == "SQLMap" && n.name.as_deref() == Some(name.as_str()))
        {
            Some(slot) => *slot = node,
            None => nodes.push(node),
        }
    }

    let t = tools.target()?;
    if let Some(body) = code::gate_tree(&nodes, shape, &t.primary.effective, &t.primary.key) {
        return err_result(body);
    }
    if let Err(rules) = validate::validate(&nodes) {
        return fail(
            "STORAGE_MAP_INVALID",
            format!(
                "the map breaks {} rule(s): {}",
                rules.len(),
                rules.join("; ")
            ),
            json!({"rules": rules}),
        );
    }
    // Build the copy once under a stand-in name, so a failure or a compile-time refusal comes
    // back before any request. The copy under the real name differs only in the name.
    let probe = match write::scratch_source(&text, &nodes, "IrisDevTmp.SmCheck") {
        Ok(s) => s,
        Err(e) => return fail("STORAGE_MAP_INVALID", e, json!({})),
    };
    if crate::tools::target::compile_gate()?.on {
        if let Some(body) = crate::policy::code_edit_gate::check_compile_time_code_mode(
            &probe,
            "IrisDevTmp.SmCheck.cls",
        ) {
            return err_result(body);
        }
    }
    let compiled = iris
        .compile_scratch_class(
            |name| write::scratch_source(&text, &nodes, name).unwrap_or_default(),
            &ns,
            client,
            "Sm",
        )
        .await;
    let (scratch, result) = match compiled {
        Ok(done) => done,
        Err(e) => {
            return fail(
                "UPLOAD_FAILED",
                format!("the scratch copy could not be sent to IRIS: {e}"),
                json!({}),
            )
        }
    };
    if !result.success() {
        return fail(
            "STORAGE_MAP_COMPILE_FAILED",
            format!(
                "the map does not compile ({} error(s)); the real class {class} was not touched",
                result.errors.len()
            ),
            json!({
                "scratch_class": scratch,
                "iris_errors": result.errors,
                "explanations": explain::explanations(&result.errors),
            }),
        );
    }
    Ok(CallToolResult::structured(json!({
        "success": true,
        "model_version": model::MODEL_VERSION,
        "action": "check",
        "class": class,
        "namespace": ns,
        "compiled": true,
        "scratch_class": scratch,
        "warnings": [],
    })))
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
    Ok(storage_tree(text)?.map(|(name, nodes)| model_with_facts(class, &name, &nodes)))
}

/// The name and parsed nodes of the first Storage block in `text`: `Ok(None)` when there is no
/// Storage block, `Err(ParseError)` when it cannot be parsed.
fn storage_tree(text: &str) -> Result<Option<(String, Vec<tree::Node>)>, tree::ParseError> {
    let Some(span) = tree::split_class(text)?.into_iter().next() else {
        return Ok(None);
    };
    let nodes = tree::parse_body(&text[span.body_start..span.body_end])?;
    Ok(Some((span.name, nodes)))
}

/// The storage model of one parsed Storage block, with its derived facts.
fn model_with_facts(class: &str, storage_name: &str, nodes: &[tree::Node]) -> model::StorageModel {
    let mut m = model::from_tree(class, storage_name, nodes);
    m.derived = model::derived_facts(&m);
    m
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
