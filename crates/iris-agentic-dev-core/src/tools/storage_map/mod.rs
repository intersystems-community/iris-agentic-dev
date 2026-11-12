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
        Action::Edit => edit(iris, client, p, tools, &shape).await,
        Action::Replace => replace(iris, client, p, tools, &shape).await,
        other => err_result(json!({
            "success": false,
            "error_code": "NOT_IMPLEMENTED",
            "error": format!("Action '{other:?}' not yet implemented")
        })),
    }
}

fn fail(code: &str, msg: String, extra: Value) -> Result<CallToolResult, McpError> {
    err_result(write::body(code, msg, extra))
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
    let (class, ns) = class_and_namespace(iris, p, action)?;
    let text = match class_text(iris, client, class, &ns).await {
        Ok(Some(t)) => t,
        Ok(None) => return Err(no_class(class, &ns)),
        Err(r) => return Err(Ok(r)),
    };
    let (span, nodes) = parsed_storage(class, &text)?;
    Ok(StoredClass {
        class,
        ns,
        text,
        storage_name: span.name,
        nodes,
    })
}

/// The `class` param and the namespace the call names, or the refusal when `class` is missing
/// or not a class name.
fn class_and_namespace<'p>(
    iris: &IrisConnection,
    p: &'p Value,
    action: &str,
) -> Result<(&'p str, String), Result<CallToolResult, McpError>> {
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
    Ok((class, ns))
}

fn no_class(class: &str, ns: &str) -> Result<CallToolResult, McpError> {
    fail(
        "STORAGE_MAP_NOT_FOUND",
        format!("no class {class} in namespace {ns}"),
        json!({}),
    )
}

/// The first Storage block of `class`'s text and its tree, or the refusal when there is none or
/// it cannot be parsed.
fn parsed_storage(
    class: &str,
    text: &str,
) -> Result<(tree::StorageSpan, Vec<tree::Node>), Result<CallToolResult, McpError>> {
    match storage_tree(text) {
        Ok(Some(found)) => Ok(found),
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

/// `STORAGE_MAP_READ_ONLY` when the storage `m` describes is one iad only reads.
fn read_only_refusal(
    class: &str,
    m: &model::StorageModel,
) -> Option<Result<CallToolResult, McpError>> {
    (!m.storage.editable).then(|| {
        let why = m.storage.not_editable_reason.as_deref().unwrap_or_default();
        fail(
            "STORAGE_MAP_READ_ONLY",
            format!("the storage of {class} is read-only in iris_storage_map: {why}"),
            json!({}),
        )
    })
}

/// The `SQLMap` node for a caller's `model`, named `name` when the model leaves its name out.
fn proposed_node(
    proposed: &Value,
    name: &str,
) -> Result<tree::Node, Result<CallToolResult, McpError>> {
    let mut proposed = proposed.clone();
    if let Some(o) = proposed.as_object_mut() {
        o.entry("name").or_insert_with(|| json!(name));
    }
    model::map_node(&proposed).map_err(|e| fail("INVALID_PARAMS", e, json!({})))
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
    let before = nodes.clone();
    let stored = model::from_tree(class, &storage_name, &nodes);
    if let Some(refusal) = read_only_refusal(class, &stored) {
        return refusal;
    }

    // The typed params pass an absent `model` through as `null`.
    if let Some(proposed) = p.get("model").filter(|v| !v.is_null()) {
        let name = match map_name_for(
            p,
            proposed,
            "check with a `model` needs `map` or a `name` in the model",
        ) {
            Ok(n) => n,
            Err(reply) => return reply,
        };
        let node = match proposed_node(proposed, &name) {
            Ok(n) => n,
            Err(reply) => return reply,
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
    if let Err(found) = validate::validate_change(&before, &nodes) {
        return err_result(write::invalid_body(&found));
    }
    let scratch = match write::scratch_check(iris, client, &text, &nodes, &ns, class).await {
        Ok(name) => name,
        Err(answer) => return err_result(answer),
    };
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

/// The map a `model` call is about: `map`, or the model's own `name`; both, and they differ, is
/// `INVALID_PARAMS`, and so is neither (with message `missing`).
fn map_name_for(
    p: &Value,
    model: &Value,
    missing: &str,
) -> Result<String, Result<CallToolResult, McpError>> {
    let map_param = p.get("map").and_then(Value::as_str);
    let model_name = model.get("name").and_then(Value::as_str);
    match (map_param, model_name) {
        (Some(a), Some(b)) if a != b => Err(fail(
            "INVALID_PARAMS",
            format!("`map` is {a} but the model is map {b}"),
            json!({}),
        )),
        (Some(a), _) | (None, Some(a)) => Ok(a.to_string()),
        (None, None) => Err(fail("INVALID_PARAMS", missing.into(), json!({}))),
    }
}

/// The opt-ins run after the real compile in Phase 10 (T094). Until then a call that asks for one
/// is refused before anything is written, rather than written without it.
fn refuse_opt_ins(shape: &CallShape) -> Option<Result<CallToolResult, McpError>> {
    (shape.seed_counter || shape.build_indexes).then(|| {
        fail(
            "NOT_IMPLEMENTED",
            "seed_counter and build_indexes are not available yet; nothing was written. Repeat the \
             call without them."
                .into(),
            json!({}),
        )
    })
}

/// The class as a write needs it: name, namespace, and what [`write::Loaded`] holds, or the
/// answer to send back. Default storage and other read-only storage are refused here.
async fn load_for_write<'p>(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &'p Value,
    action: &str,
) -> Result<(&'p str, String, write::Loaded, model::StorageModel), Result<CallToolResult, McpError>>
{
    let (class, ns) = class_and_namespace(iris, p, action)?;
    let (text, ts) = match write::get_doc(iris, client, &format!("{class}.cls"), &ns).await {
        Ok(Some(found)) => found,
        Ok(None) => return Err(no_class(class, &ns)),
        Err(e) => return Err(fail("IRIS_UNREACHABLE", e.to_string(), json!({}))),
    };
    let (span, nodes) = parsed_storage(class, &text)?;
    let m = model::from_tree(class, &span.name, &nodes);
    if let Some(refusal) = read_only_refusal(class, &m) {
        return Err(refusal);
    }
    Ok((
        class,
        ns,
        write::Loaded {
            text,
            ts,
            span,
            nodes,
        },
        m,
    ))
}

/// `version` against the version the call is judged by, or `STORAGE_MAP_VERSION_CONFLICT` with
/// the current element and its version.
fn check_version(
    p: &Value,
    current_version: &str,
    current: Value,
) -> Option<Result<CallToolResult, McpError>> {
    let Some(version) = p.get("version").and_then(Value::as_str) else {
        return Some(fail(
            "INVALID_PARAMS",
            "a write needs `version`, from read".into(),
            json!({}),
        ));
    };
    (version != current_version).then(|| {
        fail(
            "STORAGE_MAP_VERSION_CONFLICT",
            "the map changed since the version given; read it again and redo the change".into(),
            json!({"current": current, "current_version": current_version}),
        )
    })
}

/// A write's answer: the write path's body plus the action's own keys.
fn written(
    action: &str,
    class: &str,
    ns: &str,
    result: Result<Value, Value>,
    extra: Value,
) -> Result<CallToolResult, McpError> {
    match result {
        Ok(mut ans) => {
            if let (Some(o), Value::Object(e)) = (ans.as_object_mut(), extra) {
                o.extend(e);
                o.insert("success".into(), json!(true));
                o.insert("model_version".into(), json!(model::MODEL_VERSION));
                o.insert("action".into(), json!(action));
                o.insert("class".into(), json!(class));
                o.insert("namespace".into(), json!(ns));
            }
            Ok(CallToolResult::structured(ans))
        }
        Err(answer) => err_result(answer),
    }
}

/// The one map every op of an `edit` is inside (`path` and `to`), if there is one.
fn single_map(ops: &[Value]) -> Option<String> {
    let mut names = Vec::new();
    for op in ops {
        for key in ["path", "to"] {
            if let Some(p) = op.get(key).and_then(Value::as_str) {
                names.push(p.strip_prefix("/maps/").and_then(|r| r.split('/').next()));
            }
        }
    }
    let first = (*names.first()?)?;
    names
        .iter()
        .all(|n| *n == Some(first))
        .then(|| first.to_string())
}

/// Every element and leaf path inside map `name`, the map itself included.
fn paths_in_map(nodes: &[tree::Node], name: &str) -> Vec<String> {
    let prefix = format!("/maps/{name}");
    let inside = format!("{prefix}/");
    path::paths(nodes)
        .into_iter()
        .filter(|q| *q == prefix || q.starts_with(&inside))
        .collect()
}

/// Record a write that went through (FR-030c): the map's versions when the write was inside one
/// map, else the storage's.
fn audit_write(
    tools: &crate::tools::IrisTools,
    class: &str,
    map: Option<&str>,
    stored: &model::StorageModel,
    result: &Result<Value, Value>,
    changed: &[String],
) {
    let Ok(ans) = result else {
        return;
    };
    let (before, after) = match map {
        Some(name) => (
            stored
                .storage
                .maps
                .iter()
                .find(|m| m.name == name)
                .map_or("", |m| m.version.as_str()),
            ans["storage"]["maps"]
                .as_array()
                .and_then(|maps| maps.iter().find(|m| m["name"] == name))
                .and_then(|m| m["version"].as_str())
                .unwrap_or(""),
        ),
        None => (
            stored.storage.version.as_str(),
            ans["versions"]["after"].as_str().unwrap_or(""),
        ),
    };
    tools.audit_map_write(write::map_write_record(
        class,
        map,
        before,
        after,
        changed,
        ans["code_changed"] == true,
    ));
}

/// `edit` (US3, FR-010): path operations on the class's tree, then the write path. The version
/// is the map's when every op is inside one existing map, else the storage version.
async fn edit(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
    tools: &crate::tools::IrisTools,
    shape: &CallShape,
) -> Result<CallToolResult, McpError> {
    if let Some(refusal) = refuse_opt_ins(shape) {
        return refusal;
    }
    let (class, ns, loaded, stored) = match load_for_write(iris, client, p, "edit").await {
        Ok(found) => found,
        Err(reply) => return reply,
    };
    let Some(Value::Array(ops)) = p.get("ops") else {
        return fail(
            "INVALID_PARAMS",
            "edit needs `ops`, a list".into(),
            json!({}),
        );
    };
    let one_map = single_map(ops);
    let (current_version, current) = match one_map
        .as_deref()
        .and_then(|m| stored.storage.maps.iter().find(|x| x.name == m))
    {
        Some(map) => (map.version.clone(), json!(map)),
        None => (stored.storage.version.clone(), json!(stored.storage)),
    };
    if let Some(conflict) = check_version(p, &current_version, current) {
        return conflict;
    }
    let mut after = loaded.nodes.clone();
    let changed = match path::apply(&mut after, ops) {
        Ok(c) => c,
        Err(e) => return fail("INVALID_PARAMS", e, json!({})),
    };
    let t = tools.target()?;
    let w = write::WriteCtx {
        iris,
        client,
        class,
        ns: &ns,
        shape,
        policy: &t.primary.effective,
        server: &t.primary.key,
    };
    let result = write::write_map(&w, &loaded, after).await;
    audit_write(tools, class, one_map.as_deref(), &stored, &result, &changed);
    written("edit", class, &ns, result, json!({"changed": changed}))
}

/// `replace` (US3, FR-011): the whole map from `model`. The first call that would remove
/// elements lists them and writes nothing (`STORAGE_MAP_CONFIRM_REMOVAL`); the same call with
/// `confirm_removal: true` writes.
async fn replace(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
    tools: &crate::tools::IrisTools,
    shape: &CallShape,
) -> Result<CallToolResult, McpError> {
    if let Some(refusal) = refuse_opt_ins(shape) {
        return refusal;
    }
    let Some(proposed) = p.get("model").filter(|v| !v.is_null()) else {
        return fail("INVALID_PARAMS", "replace needs `model`".into(), json!({}));
    };
    let name = match map_name_for(
        p,
        proposed,
        "a `model` needs `map` or a `name` in the model",
    ) {
        Ok(n) => n,
        Err(reply) => return reply,
    };
    let (class, ns, loaded, stored) = match load_for_write(iris, client, p, "replace").await {
        Ok(found) => found,
        Err(reply) => return reply,
    };
    let Some(old) = stored.storage.maps.iter().find(|m| m.name == name) else {
        return fail(
            "STORAGE_MAP_NOT_FOUND",
            format!("{class} has no map {name}; add one with edit"),
            json!({}),
        );
    };
    if let Some(conflict) = check_version(p, &old.version, json!(old)) {
        return conflict;
    }
    let node = match proposed_node(proposed, &name) {
        Ok(n) => n,
        Err(reply) => return reply,
    };
    let mut after = loaded.nodes.clone();
    if let Some(slot) = after
        .iter_mut()
        .find(|n| n.tag == "SQLMap" && n.name.as_deref() == Some(name.as_str()))
    {
        *slot = node;
    }
    let before_paths = paths_in_map(&loaded.nodes, &name);
    let after_paths = paths_in_map(&after, &name);
    let removed: Vec<String> = before_paths
        .iter()
        .filter(|q| !after_paths.contains(q))
        .cloned()
        .collect();
    let confirmed = p.get("confirm_removal").and_then(Value::as_bool) == Some(true);
    if !removed.is_empty() && !confirmed {
        return fail(
            "STORAGE_MAP_CONFIRM_REMOVAL",
            format!(
                "the new map leaves out {} element(s) of {name}; nothing was written. Repeat the \
                 call with confirm_removal: true to remove them",
                removed.len()
            ),
            json!({"removed": removed}),
        );
    }
    let changed: Vec<String> = before_paths
        .iter()
        .chain(after_paths.iter().filter(|q| !before_paths.contains(q)))
        .filter(|q| {
            let a = path::get(&loaded.nodes, q);
            let b = path::get(&after, q);
            match (a, b) {
                (Some(x), Some(y)) => x.is_leaf() && x.value != y.value,
                _ => true,
            }
        })
        .cloned()
        .collect();
    let t = tools.target()?;
    let w = write::WriteCtx {
        iris,
        client,
        class,
        ns: &ns,
        shape,
        policy: &t.primary.effective,
        server: &t.primary.key,
    };
    let result = write::write_map(&w, &loaded, after).await;
    audit_write(tools, class, Some(&name), &stored, &result, &changed);
    written(
        "replace",
        class,
        &ns,
        result,
        json!({"changed": changed, "removed": removed}),
    )
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
    Ok(storage_tree(text)?.map(|(span, nodes)| model_with_facts(class, &span.name, &nodes)))
}

/// The span and parsed nodes of the first Storage block in `text`: `Ok(None)` when there is no
/// Storage block, `Err(ParseError)` when it cannot be parsed.
fn storage_tree(
    text: &str,
) -> Result<Option<(tree::StorageSpan, Vec<tree::Node>)>, tree::ParseError> {
    let Some(span) = tree::split_class(text)?.into_iter().next() else {
        return Ok(None);
    };
    let nodes = tree::parse_body(&text[span.body_start..span.body_end])?;
    Ok(Some((span, nodes)))
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
        "markdown": diagram::markdown(&m, map_filter),
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
