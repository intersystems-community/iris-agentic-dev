pub mod bitmap;
pub mod bitmap_run;
pub mod candidates;
pub mod candidates_run;
pub mod code;
pub mod diagram;
pub mod explain;
pub mod globals;
pub mod link;
pub mod link_code;
pub mod link_run;
pub mod link_verify;
pub mod master;
pub mod model;
pub mod path;
mod propose_class;
pub mod sample;
pub mod shape;
pub mod tree;
pub mod typing;
pub mod typing_run;
pub mod usability;
pub mod usability_run;
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

impl CallShape {
    /// The call runs as the service account (FR-029): the actions that read data through code
    /// iad runs (a write's data checks among them), and the opt-ins that write it.
    pub fn needs_service_account(&self) -> bool {
        matches!(
            self.action,
            Action::Compare | Action::Preview | Action::Propose | Action::Edit | Action::Replace
        ) || self.seed_counter
            || self.build_indexes
    }
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

/// Where a param does not belong, in words for the caller, or `None`. A null or absent value is
/// not a use: the tool builds its JSON from every declared field.
///
/// `index_globals` and `field_names` mean something only to `propose`; `propose` takes `global`
/// or `class`, one of them; and a `class` names its own properties, so `field_names` goes with a
/// `global` only.
pub fn param_use_error(params: &Value) -> Option<String> {
    let action = params.get("action").and_then(Value::as_str).unwrap_or("");
    let given = |key: &str| params.get(key).is_some_and(|v| !v.is_null());
    if action != "propose" {
        for key in ["index_globals", "field_names"] {
            if given(key) {
                return Some(format!("`{key}` is for `propose`, not `{action}`"));
            }
        }
        return None;
    }
    match (given("global"), given("class")) {
        (true, true) => Some("propose takes `global` or `class`, not both".into()),
        (false, false) => Some("propose needs `global` or `class`".into()),
        (false, true) if given("field_names") => Some(
            "`field_names` goes with `global`, not `class` (a class names its own properties)"
                .into(),
        ),
        _ => None,
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
    if let Some(why) = param_use_error(p) {
        return fail("INVALID_PARAMS", why, json!({}));
    }
    // A call that runs as the service account is refused before any IRIS contact when that
    // account cannot be used, as every other tool that runs the agent's code is.
    if shape.needs_service_account() {
        if let Err(e) = tools.exec_context() {
            return err_result(e.to_json());
        }
    }

    match shape.action {
        Action::Read => read(iris, client, p).await,
        Action::Diagram => diagram(iris, client, p).await,
        Action::Uses => uses_action(iris, client, p).await,
        Action::Check => check_action(iris, client, p, tools, &shape).await,
        Action::Edit => edit_action(iris, client, p, tools, &shape).await,
        Action::Replace => replace_action(iris, client, p, tools, &shape).await,
        Action::Compare => compare_action(iris, client, p, tools, &shape).await,
        Action::Preview => preview_action(iris, client, p, tools, &shape).await,
        Action::Propose if p.get("class").is_some_and(|v| !v.is_null()) => {
            propose_class::propose_class(iris, client, p, tools, &shape).await
        }
        Action::Propose => propose_action(iris, p, tools, &shape).await,
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
    let mut m = model_with_facts(s.class, &s.storage_name, &s.nodes);
    model::mark_compiler_added(&mut m, &s.text);
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
async fn check_action(
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
    if let Err(reply) = put_model(p, &mut nodes, "check") {
        return reply;
    }

    let t = tools.target()?;
    if let Some(body) = code::gate_tree(&nodes, shape, &t.primary.effective, &t.primary.key) {
        return err_result(body);
    }
    if let Err(found) = validate::validate_change(&before, &nodes) {
        return err_result(write::invalid_body(&found));
    }
    // FR-023: while the scratch copy exists the planner is asked about every index map; the lines
    // are advice, so nothing here changes `success`.
    let judged = model::from_tree(class, &storage_name, &nodes);
    let (scratch, lines) = match usability_run::scratch_check_with_usability(
        iris,
        client,
        &text,
        &nodes,
        &ns,
        class,
        &judged.storage.maps,
    )
    .await
    {
        Ok(done) => done,
        Err(answer) => return err_result(answer),
    };
    let warnings: Vec<String> = lines.iter().map(usability::Line::warning).collect();
    Ok(CallToolResult::structured(json!({
        "success": true,
        "model_version": model::MODEL_VERSION,
        "action": "check",
        "class": class,
        "namespace": ns,
        "compiled": true,
        "scratch_class": scratch,
        "usability": lines.iter().map(usability::Line::to_json).collect::<Vec<_>>(),
        "warnings": warnings,
    })))
}

/// The caller's `model`, when the call has one, put into `nodes` for `action`; `Some` is the
/// map's name. The caller's map takes the place of the stored one; a new name adds a map.
fn put_model(
    p: &Value,
    nodes: &mut Vec<tree::Node>,
    action: &str,
) -> Result<Option<String>, Result<CallToolResult, McpError>> {
    // The typed params pass an absent `model` through as `null`.
    let Some(proposed) = p.get("model").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let name = map_name_for(
        p,
        proposed,
        &format!("{action} with a `model` needs `map` or a `name` in the model"),
    )?;
    let node = proposed_node(proposed, &name)?;
    match nodes
        .iter_mut()
        .find(|n| n.tag == "SQLMap" && n.name.as_deref() == Some(name.as_str()))
    {
        Some(slot) => *slot = node,
        None => nodes.push(node),
    }
    Ok(Some(name))
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
        Err(e) => return Err(fail(write::read_failure(&e), e.to_string(), json!({}))),
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
    let ans = match result {
        Ok(ans) => ans,
        // A failed write that changed the class on IRIS is audited too (FR-030c): a put that
        // was restored, a restore that failed, a write whose opt-in failed. A refusal before the
        // PUT changed nothing and is not a write.
        Err(err) if err.get("restored").is_some() || err["written"] == true => {
            let mut record = write::map_write_record(
                class,
                map,
                stored.storage.version.as_str(),
                err["versions"]["after"].as_str().unwrap_or(""),
                changed,
                err["code_changed"] == true,
            );
            record["outcome"] = err["error_code"].clone();
            if let Some(restored) = err.get("restored") {
                record["restored"] = restored.clone();
            }
            tools.audit_map_write("iris_storage_map", record);
            return;
        }
        Err(_) => return,
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
    tools.audit_map_write(
        "iris_storage_map",
        write::map_write_record(
            class,
            map,
            before,
            after,
            changed,
            ans["code_changed"] == true,
        ),
    );
}

/// `compare` (US5, FR-020, FR-020a): sample the map's global and set what the data shows
/// against the map. Runs as the call's execute identity (the service account when set, FR-029),
/// whose name the answer carries.
async fn compare_action(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
    tools: &crate::tools::IrisTools,
    shape: &CallShape,
) -> Result<CallToolResult, McpError> {
    let StoredClass {
        class,
        ns,
        storage_name,
        nodes,
        ..
    } = match load_storage(iris, client, p, "compare").await {
        Ok(stored) => stored,
        Err(reply) => return reply,
    };
    let m = model_with_facts(class, &storage_name, &nodes);
    let wanted = p.get("map").and_then(Value::as_str);
    let Some(map) = m
        .storage
        .maps
        .iter()
        .find(|x| wanted.map_or(x.master, |w| x.name == w))
    else {
        return fail(
            "STORAGE_MAP_NOT_FOUND",
            match wanted {
                Some(w) => format!("{class} has no map {w}"),
                None => format!("{class} has no master map"),
            },
            json!({}),
        );
    };
    let t = tools.target()?;
    if let Some(refusal) = code::gate_tree(&nodes, shape, &t.primary.effective, &t.primary.key) {
        return err_result(refusal);
    }
    if let Some(refusal) = values_refusal(shape, &t.primary) {
        return err_result(refusal);
    }
    let global = match globals::normalize_global(&map.global) {
        Ok(g) => g,
        Err(e) => return fail("INVALID_PARAMS", format!("{}: {e}", map.global), json!({})),
    };
    sampled_answer(tools, p, shape, &ns, &global, |report| {
        let mismatches = shape::compare_map(map, &report);
        let checked = shape::compare_notes(map, &report);
        json!({
            "success": true,
            "model_version": model::MODEL_VERSION,
            "action": "compare",
            "class": class,
            "namespace": ns,
            "map": map.name,
            "capped": report.capped,
            "verified": checked.verified,
            "notes": checked.notes,
            "shape": report,
            "mismatches": mismatches,
        })
    })
    .await
}

/// Sample `global` in `ns` as the call's execute identity and answer with `answer(report)`, or
/// the sampler's error, with the identity attached either way.
async fn sampled_answer(
    tools: &crate::tools::IrisTools,
    p: &Value,
    shape: &CallShape,
    ns: &str,
    global: &globals::GlobalRef,
    answer: impl FnOnce(shape::ShapeReport) -> Value,
) -> Result<CallToolResult, McpError> {
    let cx = match tools.exec_context() {
        Ok(cx) => cx,
        Err(e) => return err_result(e.to_json()),
    };
    let cap = sample::caps(p.get("max_nodes").and_then(Value::as_u64));
    let result = match sample::sample(
        &cx.conn,
        cx.client.as_ref(),
        ns,
        global,
        cap,
        shape.include_values,
    )
    .await
    {
        Ok(report) => Ok(CallToolResult::structured(answer(report))),
        Err(answer) => err_result(answer),
    };
    crate::tools::exec::attach_identity(result, &cx.helper_report())
}

/// [`sample::values_refusal`] for this call's `include_values` and `acknowledgePhi` on `server`.
fn values_refusal(
    shape: &CallShape,
    server: &crate::tools::target::ResolvedServer,
) -> Option<Value> {
    sample::values_refusal(
        shape.include_values,
        shape.acknowledge_phi,
        server.data_policy_name(),
        &server.key,
    )
}

/// `row` with every non-null value replaced by `"<masked>"` (FR-021).
fn masked_row(row: &Value) -> Value {
    let masked = row
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| {
            let shown = if v.is_null() {
                Value::Null
            } else {
                json!("<masked>")
            };
            (k.clone(), shown)
        })
        .collect();
    Value::Object(masked)
}

/// `preview` (US5, FR-029): the rows a proposed map would return, read through a scratch copy of
/// the class that is deleted afterwards. The `SELECT` runs the map's code against the real global,
/// so it runs as the call's execute identity. Values are masked unless `include_values` (FR-021).
async fn preview_action(
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
    } = match load_storage(iris, client, p, "preview").await {
        Ok(stored) => stored,
        Err(reply) => return reply,
    };
    let before = nodes.clone();
    let stored = model::from_tree(class, &storage_name, &nodes);
    if let Some(refusal) = read_only_refusal(class, &stored) {
        return refusal;
    }
    let map_name = match put_model(p, &mut nodes, "preview") {
        Ok(Some(name)) => name,
        Ok(None) => p
            .get("map")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                stored
                    .storage
                    .maps
                    .iter()
                    .find(|m| m.master)
                    .map(|m| m.name.clone())
            })
            .unwrap_or_default(),
        Err(reply) => return reply,
    };
    let t = tools.target()?;
    if let Some(refusal) = code::gate_tree(&nodes, shape, &t.primary.effective, &t.primary.key) {
        return err_result(refusal);
    }
    if let Err(found) = validate::validate_change(&before, &nodes) {
        return err_result(write::invalid_body(&found));
    }
    if let Some(refusal) = values_refusal(shape, &t.primary) {
        return err_result(refusal);
    }
    if let Err(answer) = write::scratch_probe(&text, &nodes) {
        return err_result(answer);
    }
    let max_rows = p
        .get("max_rows")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 1000);
    let cx = match tools.exec_context() {
        Ok(cx) => cx,
        Err(e) => return err_result(e.to_json()),
    };
    let sql = move |scratch: &str| format!("SELECT TOP {max_rows} * FROM {scratch}");
    let compiled = write::scratch_compile(
        &cx.conn,
        cx.client.as_ref(),
        &text,
        &nodes,
        &ns,
        Some(&sql),
        "nothing was read",
    )
    .await;
    let result = match compiled {
        Err(answer) => err_result(answer),
        Ok((_, Some(Err(e)))) => match e.downcast_ref::<crate::iris::connection::QueryTimedOut>() {
            // FR-022: the time cap stopped the scan, and the answer says so.
            Some(cap) => fail(
                "STORAGE_MAP_TIME_CAP",
                format!(
                    "preview stopped at its time cap: {cap}. Lower max_rows or narrow the map."
                ),
                json!({"capped": true, "cap": {"rows": max_rows, "seconds": cap.0}}),
            ),
            None => fail(
                "SQL_ERROR",
                format!("reading through the proposed map failed: {e}"),
                json!({}),
            ),
        },
        Ok((_, None)) => fail(
            "SQL_ERROR",
            "the scratch copy compiled but the preview query did not run".into(),
            json!({}),
        ),
        Ok((scratch, Some(Ok(answer)))) => {
            let Some(rows) = answer["result"]["content"].as_array().cloned() else {
                return crate::tools::exec::attach_identity(
                    fail(
                        "SQL_ERROR",
                        format!("IRIS answered the preview query with no rows: {answer}"),
                        json!({}),
                    ),
                    &cx.helper_report(),
                );
            };
            let columns: Vec<String> = rows
                .first()
                .and_then(Value::as_object)
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            let shown: Vec<Value> = if shape.include_values {
                rows.clone()
            } else {
                rows.iter().map(masked_row).collect()
            };
            Ok(CallToolResult::structured(json!({
                "success": true,
                "model_version": model::MODEL_VERSION,
                "action": "preview",
                "class": class,
                "namespace": ns,
                "map": map_name,
                "scratch_class": scratch,
                "columns": columns,
                "rows": shown,
                "row_count": rows.len(),
                "capped": rows.len() as u64 >= max_rows,
                "values_masked": !shape.include_values,
            })))
        }
    };
    crate::tools::exec::attach_identity(result, &cx.helper_report())
}

/// `propose` (US7): sample a global that has no map and draft one, in model JSON, with every
/// guessed element listed. Nothing is written; the draft goes to `check` or `replace` after the
/// caller names its fields.
async fn propose_action(
    iris: &IrisConnection,
    p: &Value,
    tools: &crate::tools::IrisTools,
    shape: &CallShape,
) -> Result<CallToolResult, McpError> {
    let Some(raw) = p.get("global").and_then(Value::as_str) else {
        return fail("INVALID_PARAMS", "propose needs `global`".into(), json!({}));
    };
    let global = match globals::normalize_global(raw) {
        Ok(g) if g.extended.is_none() && g.subscripts_prefix.is_none() => g,
        Ok(_) => {
            return fail(
                "INVALID_PARAMS",
                format!("propose takes a plain global name, not {raw}"),
                json!({}),
            )
        }
        Err(e) => return fail("INVALID_PARAMS", format!("{raw}: {e}"), json!({})),
    };
    let ns = p
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or(&iris.namespace)
        .to_string();
    let t = tools.target()?;
    if let Some(refusal) = values_refusal(shape, &t.primary) {
        return err_result(refusal);
    }
    let index_names = match candidates::index_names_param(p.get("index_globals")) {
        Ok(names) => names,
        Err(why) => return fail("INVALID_PARAMS", why, json!({})),
    };
    let cx = match tools.exec_context() {
        Ok(cx) => cx,
        Err(e) => return err_result(e.to_json()),
    };
    let cap = sample::caps(p.get("max_nodes").and_then(Value::as_u64));
    let node_cap = cap.nodes;
    let report = match sample::sample(
        &cx.conn,
        cx.client.as_ref(),
        &ns,
        &global,
        cap,
        shape.include_values,
    )
    .await
    {
        Ok(report) => report,
        Err(answer) => {
            return crate::tools::exec::attach_identity(err_result(answer), &cx.helper_report())
        }
    };
    let (mut draft, mut guesses) = shape::draft(&report);
    let field_names: Vec<String> = p
        .get("field_names")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let named = !field_names.is_empty();
    let spec = match master::MasterSpec::from_draft(&report, &field_names) {
        Ok(spec) => spec,
        Err(refusal) => return fail(refusal.code, refusal.message, json!({})),
    };
    if named {
        spec.apply_names(&mut draft, &mut guesses);
    }
    let mut answer = json!({
        "success": true,
        "model_version": model::MODEL_VERSION,
        "action": "propose",
        "namespace": ns,
        "global": global.name,
        "capped": report.capped,
        "shape": report,
        "draft": draft,
        "guesses": guesses,
        "draft_note": "a draft to check against real data: every name, delimiter and piece count is a guess from a sample",
    });
    let mut examined: Vec<Value> = Vec::new();
    let chunk = bitmap_run::chunk_map(
        &cx.conn,
        cx.client.as_ref(),
        &ns,
        &global.name,
        &report,
        node_cap,
    )
    .await;
    if let Some(draft) = chunk.draft {
        answer["chunk_map_draft"] = draft;
    }
    examined.extend(chunk.details);
    // The candidates: the names the caller gave, the sub-trees inside the master and the
    // namespace's globals that begin with its name (FR-001, FR-002), then each one linked.
    let found = candidates_run::discover(
        &cx.conn,
        cx.client.as_ref(),
        &ns,
        &global.name,
        spec.subscripts.len() == 1,
        &index_names,
    )
    .await;
    let run = link_run::Run {
        conn: &cx.conn,
        client: cx.client.as_ref(),
        ns: &ns,
        master: &spec,
        master_map: &draft,
        node_cap,
        policy: &t.primary.effective,
        server: &t.primary.key,
        acknowledge_phi: shape.acknowledge_phi,
        with_values: shape.include_values,
        existing_maps: &[],
    };
    let linked = link_run::link_candidates(&run, found).await;
    let index_call = !linked.indexes.is_empty();
    answer["index_candidates"] = linked.index_candidates;
    let index_maps: Vec<Value> = linked
        .indexes
        .iter()
        .filter_map(|i| i.get("map").cloned())
        .collect();
    let picks = linked.collations;
    if index_call {
        answer["indexes"] = Value::Array(linked.indexes);
    }
    if !linked.bitmaps.is_empty() {
        answer["bitmaps"] = Value::Array(linked.bitmaps);
    }
    answer["limits"] = linked.limits;
    examined.extend(linked.not_inferred_detail);
    // The typed class draft (US6) is asked for by naming fields or index globals.
    if named || !index_names.is_empty() {
        let run = typing_run::DraftRun {
            conn: &cx.conn,
            client: cx.client.as_ref(),
            ns: &ns,
            master: &spec,
            master_map: &draft,
            index_maps: &index_maps,
            picks: &picks,
            node_cap,
        };
        typing_run::add_class_draft(&run, &mut answer, &mut examined).await;
    }
    // The first item (field names are placeholders) no longer applies when the caller named them.
    let (mut strings, mut details) = shape::not_inferred_lists(&examined, index_call);
    if named {
        details.retain(|d| d["what"] != "field names");
        strings.retain(|s| !s.starts_with("field names"));
    }
    answer["not_inferred"] = json!(strings);
    answer["not_inferred_detail"] = Value::Array(details);
    crate::tools::exec::attach_identity(Ok(CallToolResult::structured(answer)), &cx.helper_report())
}

/// `edit` (US3, FR-010): path operations on the class's tree, then the write path. The version
/// is the map's when every op is inside one existing map, else the storage version.
async fn edit_action(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
    tools: &crate::tools::IrisTools,
    shape: &CallShape,
) -> Result<CallToolResult, McpError> {
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
    let cx = match tools.exec_context() {
        Ok(cx) => cx,
        Err(e) => return err_result(e.to_json()),
    };
    let w = write::WriteCtx {
        iris,
        client,
        class,
        ns: &ns,
        shape,
        policy: &t.primary.effective,
        server: &t.primary.key,
        exec: (cx.conn.as_ref(), cx.client.as_ref()),
    };
    let result = write::write_map(&w, &loaded, after).await;
    audit_write(tools, class, one_map.as_deref(), &stored, &result, &changed);
    crate::tools::exec::attach_identity(
        written("edit", class, &ns, result, json!({"changed": changed})),
        &cx.helper_report(),
    )
}

/// `replace` (US3, FR-011): the whole map from `model`. The first call that would remove
/// elements lists them and writes nothing (`STORAGE_MAP_CONFIRM_REMOVAL`); the same call with
/// `confirm_removal: true` writes.
async fn replace_action(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: &Value,
    tools: &crate::tools::IrisTools,
    shape: &CallShape,
) -> Result<CallToolResult, McpError> {
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
    let cx = match tools.exec_context() {
        Ok(cx) => cx,
        Err(e) => return err_result(e.to_json()),
    };
    let w = write::WriteCtx {
        iris,
        client,
        class,
        ns: &ns,
        shape,
        policy: &t.primary.effective,
        server: &t.primary.key,
        exec: (cx.conn.as_ref(), cx.client.as_ref()),
    };
    let result = write::write_map(&w, &loaded, after).await;
    audit_write(tools, class, Some(&name), &stored, &result, &changed);
    crate::tools::exec::attach_identity(
        written(
            "replace",
            class,
            &ns,
            result,
            json!({"changed": changed, "removed": removed}),
        ),
        &cx.helper_report(),
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

/// The message of a reply `class_text` failed with.
fn reply_error(reply: &CallToolResult) -> String {
    reply
        .structured_content
        .as_ref()
        .and_then(|v| v["error"].as_str().map(str::to_string))
        .or_else(|| {
            reply
                .content
                .iter()
                .find_map(|c| c.as_text().map(|t| t.text.clone()))
        })
        .unwrap_or_else(|| "no detail".into())
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
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for g in globals {
        let sql = "SELECT DISTINCT $PIECE(parent,'||',1) AS Cls FROM %Dictionary.CompiledStorageSQLMap WHERE _Global = ? AND Type = 'data'";
        match iris.query(sql, vec![json!(g)], ns, client).await {
            Ok(resp) => {
                for row in uses::rows(&resp) {
                    if let Some(c) = row["Cls"].as_str() {
                        if c != except && !out.iter().any(|o: &String| o == c) {
                            out.push(c.to_string());
                        }
                    }
                }
            }
            Err(e) => warnings.push(format!(
                "the other classes on {g} could not be read ({e}), so facts about rows nested \
                 with theirs may be missing"
            )),
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
) -> Result<Option<Value>, String> {
    let Some(text) = class_text(iris, client, class, ns)
        .await
        .map_err(|e| format!("the class text could not be read: {}", reply_error(&e)))?
    else {
        return Ok(None);
    };
    let Some(m) = model_of(class, &text)
        .map_err(|e| format!("the Storage block does not parse: {}", e.reason))?
    else {
        return Ok(None);
    };
    Ok((m.storage.name == storage).then(|| json!(m.storage.maps)))
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
    let mut warnings = Vec::new();
    for other in classes_on_globals(iris, client, &globals, &ns, class, &mut warnings).await {
        match class_text(iris, client, &other, &ns).await.map(|t| t.map(|t| model_of(&other, &t))) {
            Ok(Some(Ok(Some(om)))) => models.push(om),
            Ok(Some(Ok(None))) | Ok(None) => {}
            Ok(Some(Err(e))) => warnings.push(format!(
                "{other} shares a global with {class} and its Storage block does not parse ({}), so facts about rows nested with its may be missing",
                e.reason
            )),
            Err(e) => warnings.push(format!(
                "{other} shares a global with {class} and could not be read ({}), so facts about rows nested with its may be missing",
                reply_error(&e)
            )),
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
    let mut answer = json!({
        "success": true,
        "model_version": model::MODEL_VERSION,
        "action": "read",
        "class": class,
        "namespace": ns,
        "storage": m.storage,
        "derived": m.derived,
        "warnings": warnings,
    });
    // Keywords the compiler wrote and the class does not use stay in `storage`, named here so a
    // reader (and `diagram`) can leave them out.
    if !m.compiler_added.is_empty() {
        answer["compiler_added"] = json!(m.compiler_added);
    }
    Ok(CallToolResult::structured(answer))
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
